//! Directive comments: `ConfigCommentParser` (@eslint/plugin-kit 0.7.3),
//! `SourceCode#getInlineConfigNodes/getDisableDirectives/applyInlineConfig`, and
//! `lib/linter/apply-disable-directives.js`.

use crate::report::{Fix, Problem, Shape};
use crate::source::{SourceCode, TokKind, Token};

pub struct DirectiveComment<'a> {
    pub label: &'a str,
    pub value: &'a str,
    pub justification: &'a str,
}

pub fn is_ws(c: char) -> bool {
    matches!(
        c,
        '\t' | '\n' | '\u{b}' | '\u{c}' | '\r' | ' ' | '\u{a0}' | '\u{1680}' | '\u{2000}'
            ..='\u{200a}'
                | '\u{2028}'
                | '\u{2029}'
                | '\u{202f}'
                | '\u{205f}'
                | '\u{3000}'
                | '\u{feff}'
    )
}

/// `/\s-{2,}\s/u` split, then `directivesPattern = /^([a-z]+(?:-[a-z]+)*)(?:\s|$)/u`.
pub fn parse_directive(value: &str) -> Option<DirectiveComment<'_>> {
    let (directive, justification) = {
        let chars: Vec<(usize, char)> = value.char_indices().collect();
        let mut found = None;
        for i in 0..chars.len() {
            if !is_ws(chars[i].1) {
                continue;
            }
            let mut j = i + 1;
            while j < chars.len() && chars[j].1 == '-' {
                j += 1;
            }
            if j - (i + 1) >= 2 && j < chars.len() && is_ws(chars[j].1) {
                found = Some((chars[i].0, chars[j].0 + chars[j].1.len_utf8()));
                break;
            }
        }
        match found {
            None => (value.trim_matches(is_ws), ""),
            Some((s, e)) => (
                value[..s].trim_matches(is_ws),
                value[e..].trim_matches(is_ws),
            ),
        }
    };
    let b = directive.as_bytes();
    let mut i = 0;
    loop {
        let st = i;
        while i < b.len() && b[i].is_ascii_lowercase() {
            i += 1;
        }
        if i == st {
            return None;
        }
        if i < b.len() && b[i] == b'-' && i + 1 < b.len() && b[i + 1].is_ascii_lowercase() {
            i += 1;
            continue;
        }
        break;
    }
    let label = &directive[..i];
    let rest = &directive[i..];
    if !(rest.is_empty() || rest.starts_with(is_ws)) {
        return None;
    }
    Some(DirectiveComment {
        label,
        value: rest.trim_matches(is_ws),
        justification,
    })
}

fn is_eslint_label(label: &str) -> bool {
    matches!(
        label,
        "eslint"
            | "eslint-env"
            | "eslint-enable"
            | "eslint-disable"
            | "eslint-disable-line"
            | "eslint-disable-next-line"
            | "exported"
            | "global"
            | "globals"
    )
}

pub fn inline_config_nodes(sc: &SourceCode) -> Vec<&Token> {
    sc.comments
        .iter()
        .filter(|c| {
            if c.kind == TokKind::Shebang {
                return false;
            }
            let Some(d) = parse_directive(&c.value) else {
                return false;
            };
            if !is_eslint_label(d.label) {
                return false;
            }
            c.kind != TokKind::Line
                || matches!(d.label, "eslint-disable-line" | "eslint-disable-next-line")
        })
        .collect()
}

/// `parseListConfig`: rule ids in order, unique.
pub fn parse_list_config(s: &str) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for part in s.split(',') {
        let t = part.trim_matches(is_ws);
        let t = match t.chars().next() {
            Some(q @ ('\'' | '"')) if t.len() >= 2 && t.ends_with(q) => &t[1..t.len() - 1],
            _ => t,
        };
        if !t.is_empty() && !out.iter().any(|x| x == t) {
            out.push(t.to_string());
        }
    }
    out
}

/// `parseStringConfig`: `name[:value]` pairs; later duplicates overwrite earlier ones in place.
pub fn parse_string_config(s: &str) -> Vec<(String, Option<String>)> {
    let trimmed = s.trim_matches(is_ws);
    // .replace(/(?<!\s)\s*([:,])\s*/gu, "$1")
    let chars: Vec<char> = trimmed.chars().collect();
    let mut norm = String::new();
    let mut i = 0;
    while i < chars.len() {
        let prev_ws = i > 0 && is_ws(chars[i - 1]);
        if !prev_ws {
            let mut j = i;
            while j < chars.len() && is_ws(chars[j]) {
                j += 1;
            }
            if j < chars.len() && (chars[j] == ':' || chars[j] == ',') {
                norm.push(chars[j]);
                j += 1;
                while j < chars.len() && is_ws(chars[j]) {
                    j += 1;
                }
                i = j;
                continue;
            }
        }
        norm.push(chars[i]);
        i += 1;
    }
    let mut out: Vec<(String, Option<String>)> = Vec::new();
    let mut cur = String::new();
    let flush = |cur: &mut String, out: &mut Vec<(String, Option<String>)>| {
        if !cur.is_empty() {
            let mut it = cur.split(':');
            let key = it.next().unwrap_or("").to_string();
            let val = it.next().map(str::to_string);
            if let Some(slot) = out.iter_mut().find(|(k, _)| *k == key) {
                slot.1 = val;
            } else {
                out.push((key, val));
            }
        }
        cur.clear();
    };
    for c in norm.chars() {
        if is_ws(c) || c == ',' {
            flush(&mut cur, &mut out);
        } else {
            cur.push(c);
        }
    }
    flush(&mut cur, &mut out);
    out
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DType {
    Disable,
    Enable,
    DisableLine,
    DisableNextLine,
}

#[derive(Debug, Clone)]
pub struct Parent {
    pub comment: usize,
    pub value: String,
    pub rule_ids: Vec<String>,
}

#[derive(Debug, Clone)]
pub struct Directive {
    pub kind: DType,
    pub line: u32,
    pub column: u32,
    pub rule_id: Option<String>,
    pub justification: String,
    pub parent: usize,
}

#[derive(Debug, Clone)]
struct Item {
    kind: DType,
    line: u32,
    column: u32,
    rule_id: Option<String>,
    unprocessed: usize,
}

pub struct ApplyOptions<'a> {
    pub directives: &'a [Directive],
    pub parents: &'a [Parent],
    pub problems: Vec<Problem>,
    pub report_unused: &'a str,
    pub disable_fixes: bool,
    pub rules_to_ignore: &'a [Option<String>],
}

fn cmp_loc(a: (u32, u32), b: (u32, u32)) -> std::cmp::Ordering {
    a.0.cmp(&b.0).then(a.1.cmp(&b.1))
}

struct Unused {
    description: Option<String>,
    fix: Fix,
    unprocessed: usize,
}

fn individual_removals(
    group: &[&Item],
    parent: &Parent,
    sc: &SourceCode,
    comment: &Token,
) -> Option<Vec<Unused>> {
    let list_text = parent.value.trim_matches(is_ws);
    let from_b = sc.byte(comment.start);
    let list_start_b = from_b + sc.text[from_b..].find(list_text)?;
    let mut out = Vec::new();
    for d in group {
        let rule_id = d.rule_id.as_deref()?;
        let (ms, me) = find_rule_in_list(list_text, rule_id)?;
        let matched = &list_text[ms..me];
        let first = matched.find(',');
        let last = matched.rfind(',');
        let base = list_start_b + ms;
        let (rs, re) = match (first, last) {
            (Some(f), Some(l)) if f != l => (base + f, base + l),
            _ => (base, base + matched.len()),
        };
        out.push(Unused {
            description: Some(format!("'{rule_id}'")),
            fix: Fix {
                range: (sc.u16_of(rs), sc.u16_of(re)),
                text: String::new(),
            },
            unprocessed: d.unprocessed,
        });
    }
    Some(out)
}

/// `(?:^|\s*,\s*)(['"]?)<ruleId>\1(?:\s*,\s*|$)`: the leftmost match, as byte offsets.
fn find_rule_in_list(text: &str, rule_id: &str) -> Option<(usize, usize)> {
    let idx: Vec<usize> = text
        .char_indices()
        .map(|(i, _)| i)
        .chain(std::iter::once(text.len()))
        .collect();
    let skip_ws = |mut p: usize| {
        while p < text.len() {
            let c = text[p..].chars().next().unwrap_or(' ');
            if is_ws(c) {
                p += c.len_utf8();
            } else {
                break;
            }
        }
        p
    };
    let tail = |p: usize| -> Option<usize> {
        let q = skip_ws(p);
        if text[q..].starts_with(',') {
            return Some(skip_ws(q + 1));
        }
        if p == text.len() {
            return Some(p);
        }
        None
    };
    let body = |p: usize| -> Option<usize> {
        for quote in ["'", "\"", ""] {
            if !quote.is_empty() && !text[p..].starts_with(quote) {
                continue;
            }
            let a = p + quote.len();
            if !text[a..].starts_with(rule_id) {
                continue;
            }
            let b = a + rule_id.len();
            if !text[b..].starts_with(quote) {
                continue;
            }
            if let Some(e) = tail(b + quote.len()) {
                return Some(e);
            }
        }
        None
    };
    for &p in &idx {
        if p == 0
            && let Some(e) = body(0)
        {
            return Some((0, e));
        }
        let q = skip_ws(p);
        if text[q..].starts_with(',') {
            let r = skip_ws(q + 1);
            if let Some(e) = body(r) {
                return Some((p, e));
            }
        }
    }
    None
}

fn process_unused(
    items: &[&Item],
    directives: &[Directive],
    parents: &[Parent],
    sc: &SourceCode,
) -> Option<Vec<Unused>> {
    let mut groups: Vec<(usize, Vec<&Item>)> = Vec::new();
    for it in items {
        let p = directives[it.unprocessed].parent;
        match groups.iter_mut().find(|(k, _)| *k == p) {
            Some((_, g)) => g.push(it),
            None => groups.push((p, vec![it])),
        }
    }
    let mut out = Vec::new();
    for (p, group) in groups {
        let parent = &parents[p];
        let mut remaining: Vec<&String> = parent.rule_ids.iter().collect();
        for d in &group {
            if let Some(r) = &d.rule_id {
                remaining.retain(|x| *x != r);
            }
        }
        let comment = &sc.comments[parent.comment];
        if !remaining.is_empty() {
            out.extend(individual_removals(&group, parent, sc, comment)?);
        } else {
            let ids: Vec<String> = group
                .iter()
                .filter_map(|d| d.rule_id.as_ref())
                .map(|r| format!("'{r}'"))
                .collect();
            let description = if ids.len() <= 2 {
                ids.join(" or ")
            } else {
                format!(
                    "{}, or {}",
                    ids[..ids.len() - 1].join(", "),
                    ids[ids.len() - 1]
                )
            };
            out.push(Unused {
                description: Some(description),
                fix: Fix {
                    range: (comment.start, comment.end),
                    text: " ".into(),
                },
                unprocessed: group[0].unprocessed,
            });
        }
    }
    Some(out)
}

fn collect_used_enable(items: &[Item]) -> Vec<usize> {
    let mut enabled: Vec<(Option<String>, usize)> = Vec::new();
    let mut used: Vec<usize> = Vec::new();
    for idx in (0..items.len()).rev() {
        let d = &items[idx];
        match d.kind {
            DType::Disable => {
                if enabled.is_empty() {
                    continue;
                }
                if d.rule_id.is_none() {
                    for (_, e) in &enabled {
                        used.push(*e);
                    }
                    enabled.clear();
                } else if let Some((_, e)) = enabled.iter().find(|(r, _)| *r == d.rule_id) {
                    used.push(*e);
                } else if let Some((_, e)) = enabled.iter().find(|(r, _)| r.is_none()) {
                    used.push(*e);
                }
            }
            DType::Enable => {
                if d.rule_id.is_none() {
                    enabled.clear();
                    enabled.push((None, idx));
                } else if let Some(slot) = enabled.iter_mut().find(|(r, _)| *r == d.rule_id) {
                    slot.1 = idx;
                } else {
                    enabled.push((d.rule_id.clone(), idx));
                }
            }
            _ => {}
        }
    }
    used
}

fn apply(
    items: &[Item],
    o: &ApplyOptions,
    problems: Vec<Problem>,
    sc: &SourceCode,
) -> Option<(Vec<Problem>, Vec<Problem>)> {
    if items.is_empty() {
        return Some((problems, Vec::new()));
    }
    let mut out = Vec::with_capacity(problems.len());
    let mut used_disable: Vec<usize> = Vec::new();
    for mut problem in problems {
        let mut for_problem: Vec<usize> = Vec::new();
        let mut next = 0;
        while next < items.len()
            && cmp_loc(
                (items[next].line, items[next].column),
                (problem.line, problem.column),
            )
            .is_le()
        {
            let d = &items[next];
            next += 1;
            if d.rule_id.is_none() || d.rule_id == problem.rule_id {
                match d.kind {
                    DType::Disable => for_problem.push(next - 1),
                    DType::Enable => for_problem.clear(),
                    _ => {}
                }
            }
        }
        if !for_problem.is_empty() {
            let sups: Vec<String> = for_problem
                .iter()
                .map(|i| o.directives[items[*i].unprocessed].justification.clone())
                .collect();
            match &mut problem.suppressions {
                Some(existing) => existing.extend(sups),
                None => {
                    problem.suppressions = Some(sups);
                    if let Some(last) = for_problem.last() {
                        used_disable.push(*last);
                    }
                }
            }
        }
        out.push(problem);
    }
    let unused_disable: Vec<&Item> = items
        .iter()
        .enumerate()
        .filter(|(i, d)| {
            d.kind == DType::Disable
                && !used_disable.contains(i)
                && !o.rules_to_ignore.contains(&d.rule_id)
        })
        .map(|(_, d)| d)
        .collect();
    let mut unused_enable_idx: Vec<usize> = items
        .iter()
        .enumerate()
        .filter(|(_, d)| {
            o.directives[d.unprocessed].kind == DType::Enable
                && !o.rules_to_ignore.contains(&d.rule_id)
        })
        .map(|(i, _)| i)
        .collect();
    if !unused_enable_idx.is_empty() {
        let used = collect_used_enable(items);
        unused_enable_idx.retain(|i| !used.contains(i));
    }
    let unused_enable: Vec<&Item> = unused_enable_idx.iter().map(|i| &items[*i]).collect();
    let mut processed = process_unused(&unused_disable, o.directives, o.parents, sc)?;
    processed.extend(process_unused(&unused_enable, o.directives, o.parents, sc)?);
    let mut unused = Vec::new();
    for u in processed {
        let d = &o.directives[u.unprocessed];
        let message = if d.kind == DType::Enable {
            match &u.description {
                Some(desc) if !desc.is_empty() => format!("Unused eslint-enable directive (no matching eslint-disable directives were found for {desc})."),
                _ => "Unused eslint-enable directive (no matching eslint-disable directives were found).".to_string(),
            }
        } else {
            match &u.description {
                Some(desc) if !desc.is_empty() => format!(
                    "Unused eslint-disable directive (no problems were reported from {desc})."
                ),
                _ => "Unused eslint-disable directive (no problems were reported).".to_string(),
            }
        };
        let comment = &sc.comments[o.parents[d.parent].comment];
        let loc = sc.tok_loc(comment);
        let (line, column) = if d.kind == DType::DisableNextLine {
            (loc.start.line, loc.start.column + 1)
        } else {
            (d.line, d.column)
        };
        unused.push(Problem {
            shape: Shape::Directive,
            rule_id: None,
            severity: if o.report_unused == "warn" { 1 } else { 2 },
            message,
            line,
            column,
            message_id: None,
            end_line: None,
            end_column: None,
            fix: if o.disable_fixes { None } else { Some(u.fix) },
            suggestions: Vec::new(),
            fatal: false,
            suppressions: None,
        });
    }
    Some((out, unused))
}

/// The module export of apply-disable-directives.js. `None` when a case is outside the port.
pub fn apply_disable_directives(o: ApplyOptions, sc: &SourceCode) -> Option<Vec<Problem>> {
    let mut block: Vec<Item> = o
        .directives
        .iter()
        .enumerate()
        .filter(|(_, d)| matches!(d.kind, DType::Disable | DType::Enable))
        .map(|(i, d)| Item {
            kind: d.kind,
            line: d.line,
            column: d.column,
            rule_id: d.rule_id.clone(),
            unprocessed: i,
        })
        .collect();
    block.sort_by(|a, b| cmp_loc((a.line, a.column), (b.line, b.column)));
    let mut line_items: Vec<Item> = Vec::new();
    for (i, d) in o.directives.iter().enumerate() {
        let (first, second) = match d.kind {
            DType::DisableLine => (d.line, d.line + 1),
            DType::DisableNextLine => (d.line + 1, d.line + 2),
            _ => continue,
        };
        line_items.push(Item {
            kind: DType::Disable,
            line: first,
            column: 1,
            rule_id: d.rule_id.clone(),
            unprocessed: i,
        });
        line_items.push(Item {
            kind: DType::Enable,
            line: second,
            column: 0,
            rule_id: d.rule_id.clone(),
            unprocessed: i,
        });
    }
    line_items.sort_by(|a, b| cmp_loc((a.line, a.column), (b.line, b.column)));
    let problems = o.problems.clone();
    let (p1, u1) = apply(&block, &o, problems, sc)?;
    let (p2, u2) = apply(&line_items, &o, p1, sc)?;
    if o.report_unused != "off" {
        let mut all = p2;
        all.extend(u1);
        all.extend(u2);
        all.sort_by(|a, b| cmp_loc((a.line, a.column), (b.line, b.column)));
        Some(all)
    } else {
        Some(p2)
    }
}
