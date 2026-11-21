//! `Linter#verify` and `Linter#verifyAndFix` (lib/linter/linter.js) for one JavaScript file
//! and a flat config made of CLI options.

use crate::core_rules::{CORE_RULES, REPLACED, RULE_TYPES};
use crate::directives::{self, ApplyOptions, DType, Directive, Parent};
use crate::levn;
use crate::report::{self, Fix, Problem, Shape, SugVal, Suggestion};
use crate::rule::{Report, Sel};
use crate::rules::{self, Rule, RuleDef};
use crate::source::{Loc, ParseFail, ParseOptions, Pos, SourceCode, SourceType, TokKind};
use serde_json::Value;

pub type Unsupported = String;

#[derive(Debug, Clone)]
pub struct RuleConf {
    pub severity: u8,
    pub options: Vec<Value>,
}

#[derive(Debug, Clone)]
pub struct Options {
    pub rules: Vec<(String, RuleConf)>,
    pub source_type: SourceType,
    pub jsx: bool,
    pub global_return: bool,
    pub globals: Vec<(String, String)>,
    pub allow_inline_config: bool,
    pub report_unused_disable_directives: &'static str,
    pub report_unused_inline_configs: &'static str,
    pub fix: bool,
    pub fix_types: Option<Vec<String>>,
    pub quiet: bool,
}

pub struct Outcome {
    pub messages: Vec<Problem>,
    pub suppressed: Vec<Problem>,
    pub fixed: bool,
    pub output: String,
}

pub fn severity_of(v: &Value) -> Option<u8> {
    match v {
        Value::Number(n) => match n.as_f64() {
            Some(0.0) => Some(0),
            Some(1.0) => Some(1),
            Some(2.0) => Some(2),
            _ => None,
        },
        Value::String(s) => match s.as_str() {
            "off" | "0" => Some(0),
            "warn" | "1" => Some(1),
            "error" | "2" => Some(2),
            _ => None,
        },
        _ => None,
    }
}

fn sev_name(s: u8) -> &'static str {
    match s {
        0 => "off",
        1 => "warn",
        _ => "error",
    }
}

pub fn rule_type(name: &str) -> Option<&'static str> {
    RULE_TYPES.iter().find(|(n, _)| *n == name).map(|(_, t)| *t)
}

fn is_core(name: &str) -> bool {
    CORE_RULES.binary_search(&name).is_ok()
}

fn missing_rule_message(rule_id: &str) -> String {
    match REPLACED.iter().find(|(k, _)| *k == rule_id) {
        Some((_, r)) => format!("Rule '{rule_id}' was removed and replaced by: {r}"),
        None => format!("Definition for rule '{rule_id}' was not found."),
    }
}

/// Everything a running rule can reach: `context`, `sourceCode`, the report.
pub struct Cx<'a> {
    pub sc: &'a SourceCode,
    pub def: &'static RuleDef,
    pub severity: u8,
    pub problems: &'a mut Vec<Problem>,
    pub disable_fixes: bool,
    pub scope: &'a crate::scope::ScopeData,
}

impl Cx<'_> {
    pub fn report(&mut self, r: Report) {
        let template = match (r.message_id, &r.message) {
            (_, Some(m)) => m.clone(),
            (Some(id), None) => self
                .def
                .messages
                .iter()
                .find(|(k, _)| *k == id)
                .map(|(_, v)| v.to_string())
                .unwrap_or_default(),
            _ => String::new(),
        };
        let message = report::interpolate(&template, &r.data);
        let (start, end) = match (r.loc, r.node) {
            (Some(l), _) => (l.start, if r.loc_start_only { None } else { Some(l.end) }),
            (None, Some(n)) => {
                let l = self.sc.node_loc(n);
                (l.start, Some(l.end))
            }
            _ => (Pos { line: 1, column: 0 }, None),
        };
        let fix = if self.disable_fixes {
            None
        } else {
            r.fix.and_then(|f| report::merge_fixes(f, self.sc))
        };
        let mut suggestions = Vec::new();
        if !self.disable_fixes {
            for s in r.suggest {
                let Some(fix) = s.fix else { continue };
                let mut keys = s.keys;
                if !keys.iter().any(|(k, _)| *k == "desc") {
                    let mid = keys.iter().find_map(|(k, v)| match (k, v) {
                        (&"messageId", SugVal::Str(m)) => Some(m.clone()),
                        _ => None,
                    });
                    let data: Vec<(String, Value)> = keys
                        .iter()
                        .find_map(|(k, v)| match (k, v) {
                            (&"data", SugVal::Data(d)) => Some(d.clone()),
                            _ => None,
                        })
                        .unwrap_or_default();
                    let tmpl = mid
                        .and_then(|m| {
                            self.def
                                .messages
                                .iter()
                                .find(|(k, _)| *k == m)
                                .map(|(_, v)| v.to_string())
                        })
                        .unwrap_or_default();
                    keys.push(("desc", SugVal::Str(report::interpolate(&tmpl, &data))));
                }
                suggestions.push(Suggestion { keys, fix });
            }
        }
        self.problems.push(Problem {
            shape: Shape::Rule,
            rule_id: Some(self.def.name.to_string()),
            severity: self.severity,
            message,
            line: start.line,
            column: start.column + 1,
            message_id: r.message_id.map(str::to_string),
            end_line: end.map(|e| e.line),
            end_column: end.map(|e| e.column + 1),
            fix,
            suggestions,
            fatal: false,
            suppressions: None,
        });
    }
}

struct Running {
    def: &'static RuleDef,
    severity: u8,
    rule: Box<dyn Rule>,
}

struct Entry {
    sel: Sel,
    callbacks: Vec<(usize, usize)>,
}

fn run_rules(
    sc: &SourceCode,
    configured: &[(String, RuleConf)],
    quiet: bool,
    problems: &mut Vec<Problem>,
    scope: &crate::scope::ScopeData,
) -> Result<(), Unsupported> {
    let mut running: Vec<Running> = Vec::new();
    for (id, conf) in configured {
        if conf.severity == 0 || (quiet && conf.severity < 2) {
            continue;
        }
        let def = rules::find(id).ok_or_else(|| format!("rule {id} not ported"))?;
        let rule = (def.create)(&conf.options).map_err(|e| format!("{id}: {e}"))?;
        running.push(Running {
            def,
            severity: conf.severity,
            rule,
        });
    }
    let mut entries: Vec<Entry> = Vec::new();
    for (ri, r) in running.iter().enumerate() {
        for (li, source) in r.rule.listeners().iter().enumerate() {
            if source.starts_with("onCodePath") || source.starts_with("onUnreachableCodePath") {
                continue;
            }
            match entries.iter_mut().find(|e| e.sel.source == *source) {
                Some(e) => e.callbacks.push((ri, li)),
                None => entries.push(Entry {
                    sel: Sel::parse(source),
                    callbacks: vec![(ri, li)],
                }),
            }
        }
    }
    entries.sort_by(|a, b| a.sel.specificity_cmp(&b.sel));
    let mut by_type: std::collections::HashMap<
        (&'static str, bool),
        Vec<usize>,
        crate::ast::FnvBuild,
    > = std::collections::HashMap::default();
    let mut any: [Vec<usize>; 2] = [Vec::new(), Vec::new()];
    for (i, e) in entries.iter().enumerate() {
        match &e.sel.types {
            Some(ts) => {
                for t in ts {
                    by_type.entry((t, e.sel.exit)).or_default().push(i);
                }
            }
            None => any[e.sel.exit as usize].push(i),
        }
    }
    let mut events: Vec<(&'static str, Vec<(usize, usize)>)> = Vec::new();
    for (ri, r) in running.iter().enumerate() {
        for (li, l) in r.rule.listeners().iter().enumerate() {
            if l.starts_with("onCodePath") || l.starts_with("onUnreachableCodePath") {
                match events.iter_mut().find(|(n, _)| n == l) {
                    Some((_, v)) => v.push((ri, li)),
                    None => events.push((l, vec![(ri, li)])),
                }
            }
        }
    }
    let uses_cp = !events.is_empty();
    let mut orders: std::collections::HashMap<(usize, bool), Vec<usize>, crate::ast::FnvBuild> =
        std::collections::HashMap::default();
    let empty = Vec::new();
    for step in crate::code_path::steps(sc, uses_cp) {
        let (node, exit) = match step {
            crate::code_path::Step::Enter(n) => (n, false),
            crate::code_path::Step::Exit(n) => (n, true),
            crate::code_path::Step::Event(ev) => {
                let name = crate::code_path::event_name(&ev);
                let Some((_, calls)) = events.iter().find(|(n, _)| *n == name) else {
                    continue;
                };
                for &(ri, li) in calls {
                    let r = &mut running[ri];
                    let mut cx = Cx {
                        sc,
                        def: r.def,
                        severity: r.severity,
                        problems,
                        disable_fixes: false,
                        scope,
                    };
                    r.rule.on_code_path(li, &ev, &mut cx);
                    if let Some(err) = r.rule.unsupported() {
                        return Err(format!("{}: {err}", r.def.name));
                    }
                }
                continue;
            }
        };
        let kind = sc.ast.kind(node);
        let order = orders
            .entry((kind.as_ptr() as usize, exit))
            .or_insert_with(|| {
                let typed = by_type.get(&(kind, exit)).unwrap_or(&empty);
                let anys = &any[exit as usize];
                let mut order = Vec::with_capacity(typed.len() + anys.len());
                let (mut i, mut j) = (0, 0);
                while i < typed.len() || j < anys.len() {
                    if i >= typed.len()
                        || (j < anys.len()
                            && entries[anys[j]]
                                .sel
                                .specificity_cmp(&entries[typed[i]].sel)
                                .is_lt())
                    {
                        order.push(anys[j]);
                        j += 1;
                    } else {
                        order.push(typed[i]);
                        i += 1;
                    }
                }
                order
            });
        for &ei in order.iter() {
            for &(ri, li) in &entries[ei].callbacks {
                let r = &mut running[ri];
                let mut cx = Cx {
                    sc,
                    def: r.def,
                    severity: r.severity,
                    problems,
                    disable_fixes: false,
                    scope,
                };
                r.rule.on(li, node, &mut cx);
                if let Some(err) = r.rule.unsupported() {
                    return Err(format!("{}: {err}", r.def.name));
                }
            }
        }
    }
    Ok(())
}

pub fn verify(text: &str, o: &Options) -> Result<(Vec<Problem>, Vec<Problem>), Unsupported> {
    let sc = match SourceCode::parse(
        text,
        &ParseOptions {
            source_type: o.source_type,
            jsx: o.jsx,
            global_return: o.global_return,
        },
    ) {
        Ok(sc) => sc,
        Err(ParseFail::Syntax(m)) => return Err(format!("parse error: {m}")),
        Err(ParseFail::Unsupported(m)) => return Err(m),
    };
    let mut problems: Vec<Problem> = Vec::new();
    let mut inline_rules: Vec<(String, RuleConf)> = Vec::new();
    let mut inline_globals: Vec<(String, String, usize)> = Vec::new();
    let mut exported: Vec<String> = Vec::new();
    let config_nodes: Vec<usize> = if o.allow_inline_config {
        let nodes = directives::inline_config_nodes(&sc);
        nodes
            .iter()
            .map(|t| {
                sc.comments
                    .iter()
                    .position(|c| std::ptr::eq(c, *t))
                    .unwrap_or(0)
            })
            .collect()
    } else {
        Vec::new()
    };
    for &ci in &config_nodes {
        let c = &sc.comments[ci];
        let Some(d) = directives::parse_directive(&c.value) else {
            continue;
        };
        let loc = sc.tok_loc(c);
        match d.label {
            "exported" => {
                for n in directives::parse_list_config(d.value) {
                    if !exported.contains(&n) {
                        exported.push(n);
                    }
                }
            }
            "global" | "globals" => {
                for (id, setting) in directives::parse_string_config(d.value) {
                    let norm = match setting.as_deref() {
                        Some("off") => "off",
                        Some("true") | Some("writeable") | Some("writable") => "writable",
                        None | Some("false") | Some("readable") | Some("readonly") => "readonly",
                        Some(other) => {
                            problems.push(Problem::linting(None, format!("'{other}' is not a valid configuration for a global (use 'readonly', 'writable', or 'off')"), loc, 2));
                            continue;
                        }
                    };
                    if let Some(slot) = inline_globals.iter_mut().find(|(k, _, _)| *k == id) {
                        slot.1 = norm.to_string();
                    } else {
                        inline_globals.push((id, norm.to_string(), ci));
                    }
                }
            }
            "eslint" => {
                let parsed = levn::parse_rules_config(d.value).ok_or("inline config not parsed")?;
                apply_inline_rules(parsed, loc, o, &mut inline_rules, &mut problems)?;
            }
            "eslint-env" => {
                let mut p = Problem::linting(
                    None,
                    "/* eslint-env */ comments are no longer supported.".into(),
                    loc,
                    2,
                );
                p.fatal = true;
                problems.push(p);
            }
            _ => {}
        }
    }
    let mut dirs: Vec<Directive> = Vec::new();
    let mut parents: Vec<Parent> = Vec::new();
    for &ci in &config_nodes {
        let c = &sc.comments[ci];
        let Some(d) = directives::parse_directive(&c.value) else {
            continue;
        };
        let line_supported = matches!(d.label, "eslint-disable-line" | "eslint-disable-next-line");
        if c.kind == TokKind::Line && !line_supported {
            continue;
        }
        let loc = sc.tok_loc(c);
        if d.label == "eslint-disable-line" && loc.start.line != loc.end.line {
            problems.push(Problem::linting(
                None,
                "eslint-disable-line comment should not span multiple lines.".into(),
                loc,
                2,
            ));
            continue;
        }
        let kind = match d.label {
            "eslint-disable" => DType::Disable,
            "eslint-enable" => DType::Enable,
            "eslint-disable-line" => DType::DisableLine,
            "eslint-disable-next-line" => DType::DisableNextLine,
            _ => continue,
        };
        let rule_ids = directives::parse_list_config(d.value);
        let pi = parents.len();
        parents.push(Parent {
            comment: ci,
            value: d.value.to_string(),
            rule_ids: rule_ids.clone(),
        });
        let targets: Vec<Option<String>> = if rule_ids.is_empty() {
            vec![None]
        } else {
            rule_ids.into_iter().map(Some).collect()
        };
        for rid in targets {
            let known = match &rid {
                None => true,
                Some(r) => is_core(r),
            };
            if known {
                let p = if kind == DType::DisableNextLine {
                    loc.end
                } else {
                    loc.start
                };
                dirs.push(Directive {
                    kind,
                    line: p.line,
                    column: p.column + 1,
                    rule_id: rid,
                    justification: d.justification.to_string(),
                    parent: pi,
                });
            } else {
                let r = rid.unwrap_or_default();
                problems.push(Problem::linting(
                    Some(r.clone()),
                    missing_rule_message(&r),
                    loc,
                    2,
                ));
            }
        }
    }
    let mut configured: Vec<(String, RuleConf)> = o.rules.clone();
    for (k, v) in inline_rules {
        match configured.iter_mut().find(|(n, _)| *n == k) {
            Some(slot) => slot.1 = v,
            None => configured.push((k, v)),
        }
    }
    let scope = crate::scope::ScopeData::new(&sc, o, &inline_globals, &exported, &configured)?;
    run_rules(&sc, &configured, o.quiet, &mut problems, &scope)?;
    problems.sort_by(|a, b| a.line.cmp(&b.line).then(a.column.cmp(&b.column)));
    let rules_to_ignore: Vec<Option<String>> = if o.quiet {
        let mut v: Vec<Option<String>> = configured
            .iter()
            .filter(|(_, c)| c.severity == 1)
            .map(|(k, _)| Some(k.clone()))
            .collect();
        if !v.is_empty() {
            v.push(None);
        }
        v
    } else {
        Vec::new()
    };
    let all = directives::apply_disable_directives(
        ApplyOptions {
            directives: &dirs,
            parents: &parents,
            problems,
            report_unused: o.report_unused_disable_directives,
            disable_fixes: false,
            rules_to_ignore: &rules_to_ignore,
        },
        &sc,
    )
    .ok_or("unused directive removal")?;
    let (suppressed, messages): (Vec<Problem>, Vec<Problem>) =
        all.into_iter().partition(|p| p.suppressions.is_some());
    Ok((messages, suppressed))
}

fn apply_inline_rules(
    parsed: Vec<(String, Value)>,
    loc: Loc,
    o: &Options,
    merged: &mut Vec<(String, RuleConf)>,
    problems: &mut Vec<Problem>,
) -> Result<(), Unsupported> {
    for (rule_id, value) in parsed {
        if !is_core(&rule_id) {
            problems.push(Problem::linting(
                Some(rule_id.clone()),
                missing_rule_message(&rule_id),
                loc,
                2,
            ));
            continue;
        }
        let def =
            rules::find(&rule_id).ok_or_else(|| format!("inline rule {rule_id} not ported"))?;
        if merged.iter().any(|(k, _)| *k == rule_id) {
            problems.push(Problem::linting(
                None,
                format!("Rule \"{rule_id}\" is already configured by another configuration comment in the preceding code. This configuration is ignored."),
                loc,
                2,
            ));
            continue;
        }
        let arr: Vec<Value> = match &value {
            Value::Array(a) => a.clone(),
            v => vec![v.clone()],
        };
        let severity = arr
            .first()
            .and_then(severity_of)
            .ok_or("inline severity invalid")?;
        let inline_len = arr.len();
        let existing = o.rules.iter().find(|(k, _)| *k == rule_id).map(|(_, c)| c);
        let options: Vec<Value> = match existing {
            Some(c) if inline_len == 1 => c.options.clone(),
            _ => arr[1..].to_vec(),
        };
        if o.report_unused_inline_configs != "off" {
            let existing_sev = existing.map(|c| c.severity).unwrap_or(0);
            if existing_sev == severity {
                let existing_opts = existing.map(|c| c.options.clone()).unwrap_or_default();
                if !existing_opts.is_empty() || !options.is_empty() || def.has_default_options {
                    return Err("unused inline config with options".into());
                }
                let already = if existing.is_some() {
                    format!("is already configured to '{}'", sev_name(existing_sev))
                } else {
                    "is not enabled so can't be turned off".to_string()
                };
                let message = format!("Unused inline config ('{rule_id}' {already}).");
                let s = if o.report_unused_inline_configs == "warn" {
                    1
                } else {
                    2
                };
                problems.push(Problem::linting(None, message, loc, s));
            }
        }
        let validate = !(inline_len == 1 && existing.is_some_and(|c| c.severity > 0));
        if validate && severity > 0 {
            (def.create)(&options).map_err(|e| format!("inline options for {rule_id}: {e}"))?;
        } else if validate {
            (def.create)(&options).map_err(|e| format!("inline options for {rule_id}: {e}"))?;
        }
        merged.push((rule_id, RuleConf { severity, options }));
    }
    Ok(())
}

pub fn should_fix(p: &Problem, o: &Options) -> bool {
    if o.quiet && p.severity != 2 {
        return false;
    }
    if let Some(types) = &o.fix_types {
        return match &p.rule_id {
            None => types.iter().any(|t| t == "directive"),
            Some(r) => rule_type(r).is_some_and(|t| types.iter().any(|x| x == t)),
        };
    }
    true
}

/// `SourceCodeFixer.applyFixes`.
pub fn apply_fixes(
    text: &str,
    messages: Vec<Problem>,
    o: &Options,
) -> (bool, Vec<Problem>, String) {
    let (bom, body) = match text.strip_prefix('\u{FEFF}') {
        Some(b) => ("\u{FEFF}", b),
        None => ("", text),
    };
    let u16_idx: Vec<usize> = {
        let mut v = Vec::with_capacity(body.len() + 1);
        for (b, ch) in body.char_indices() {
            for _ in 0..ch.len_utf16() {
                v.push(b);
            }
        }
        v.push(body.len());
        v
    };
    let at = |u: i64| -> usize {
        if u <= 0 {
            return 0;
        }
        u16_idx.get(u as usize).copied().unwrap_or(body.len())
    };
    let mut remaining = Vec::new();
    let mut fixes = Vec::new();
    for m in messages {
        if m.fix.is_some() {
            fixes.push(m);
        } else {
            remaining.push(m);
        }
    }
    if fixes.is_empty() {
        return (false, remaining, format!("{bom}{body}"));
    }
    fixes.sort_by(|a, b| {
        let fa = a.fix.as_ref().map(|f| f.range).unwrap_or_default();
        let fb = b.fix.as_ref().map(|f| f.range).unwrap_or_default();
        fa.0.cmp(&fb.0).then(fa.1.cmp(&fb.1))
    });
    let mut output = bom.to_string();
    let mut last: i64 = i64::MIN;
    let mut applied = false;
    for p in fixes {
        if !should_fix(&p, o) {
            remaining.push(p);
            continue;
        }
        let Fix { range, text: ftext } = p.fix.clone().unwrap_or(Fix {
            range: (0, 0),
            text: String::new(),
        });
        let (s, e) = (range.0 as i64, range.1 as i64);
        if last >= s || s > e {
            remaining.push(p);
            applied = true;
            continue;
        }
        if s == 0 && ftext.starts_with('\u{FEFF}') {
            output.clear();
        }
        output.push_str(&body[at(last.max(0))..at(s.max(0))]);
        output.push_str(&ftext);
        last = e;
        applied = true;
    }
    output.push_str(&body[at(last.max(0))..]);
    remaining.sort_by(|a, b| a.line.cmp(&b.line).then(a.column.cmp(&b.column)));
    (applied, remaining, output)
}

pub fn verify_and_fix(text: &str, o: &Options) -> Result<Outcome, Unsupported> {
    let mut current = text.to_string();
    let mut fixed_any = false;
    let mut pass = 0;
    let mut previous: Option<String> = None;
    let mut second_previous: Option<String>;
    let mut messages;
    let mut suppressed;
    let mut fixed_this;
    loop {
        pass += 1;
        let (m, s) = verify(&current, o)?;
        suppressed = s;
        let (fixed, rest, output) = if o.fix {
            apply_fixes(&current, m, o)
        } else {
            (false, m, current.clone())
        };
        fixed_this = fixed;
        messages = rest;
        fixed_any = fixed_any || fixed;
        second_previous = previous.take();
        previous = Some(std::mem::replace(&mut current, output));
        if pass > 1 && second_previous.as_deref() == Some(current.as_str()) {
            return Err("circular fixes".into());
        }
        if !(fixed && pass < 10) {
            break;
        }
    }
    if fixed_this {
        let (m, s) = verify(&current, o)?;
        messages = m;
        suppressed = s;
    }
    Ok(Outcome {
        messages,
        suppressed,
        fixed: fixed_any,
        output: current,
    })
}
