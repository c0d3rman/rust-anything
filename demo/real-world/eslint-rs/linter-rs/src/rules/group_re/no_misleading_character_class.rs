//! lib/rules/no-misleading-character-class.js

use super::util::{
    CodeUnit, Static, is_combining_character, is_emoji_modifier, is_regional_indicator_symbol,
    is_surrogate_pair, is_valid_with_unicode_flag, node_text16, parse_string_literal,
    parse_template_token, regex_of, regexp_global_calls, static_value, string_if_constant, utf16,
};
use crate::ast::NodeId;
use crate::linter::Cx;
use crate::regexpp::{K, Nid, RegExpAst, parse_pattern};
use crate::report::{Fix, SugVal};
use crate::rule::{Report, SuggestSpec, fixer};
use crate::rules::{Rule, RuleDef};
use crate::source::Loc;
use crate::utils::{is_comma, is_static_template_literal};
use serde_json::Value;

pub static DEF: RuleDef = RuleDef {
    name: "no-misleading-character-class",
    messages: &[
        (
            "surrogatePairWithoutUFlag",
            "Unexpected surrogate pair in character class. Use 'u' flag.",
        ),
        (
            "surrogatePair",
            "Unexpected surrogate pair in character class.",
        ),
        (
            "combiningClass",
            "Unexpected combined character in character class.",
        ),
        (
            "emojiModifier",
            "Unexpected modified Emoji in character class.",
        ),
        (
            "regionalIndicatorSymbol",
            "Unexpected national flag in character class.",
        ),
        (
            "zwj",
            "Unexpected joined character sequence in character class.",
        ),
        ("suggestUnicodeFlag", "Add unicode 'u' flag to regex."),
    ],
    has_default_options: true,
    create,
};

const KINDS: [&str; 6] = [
    "surrogatePairWithoutUFlag",
    "surrogatePair",
    "combiningClass",
    "emojiModifier",
    "regionalIndicatorSymbol",
    "zwj",
];

const ECMA_VERSION: u32 = 2026;

struct R {
    allow_escape: bool,
    checked: Vec<NodeId>,
    unsupported: Option<&'static str>,
}

fn create(options: &[Value]) -> Result<Box<dyn Rule>, String> {
    let mut allow_escape = false;
    match options {
        [] => {}
        [Value::Object(o)] => {
            for (k, v) in o {
                match (k.as_str(), v) {
                    ("allowEscape", Value::Bool(b)) => allow_escape = *b,
                    _ => return Err("invalid option".into()),
                }
            }
        }
        _ => return Err("invalid option".into()),
    }
    Ok(Box::new(R {
        allow_escape,
        checked: Vec::new(),
        unsupported: None,
    }))
}

fn value(ast: &RegExpAst, id: Nid) -> u32 {
    match ast.node(id).k {
        K::Character { value } => value,
        _ => u32::MAX,
    }
}

fn iterate_character_sequence(ast: &RegExpAst, elements: &[Nid]) -> Vec<Vec<Nid>> {
    let mut out = Vec::new();
    let mut seq: Vec<Nid> = Vec::new();
    for &node in elements {
        match &ast.node(node).k {
            K::Character { .. } => seq.push(node),
            K::CharacterClassRange { min, max } => {
                seq.push(*min);
                out.push(std::mem::replace(&mut seq, vec![*max]));
            }
            K::CharacterSet { .. }
            | K::CharacterClass { .. }
            | K::ClassStringDisjunction { .. }
            | K::ExpressionCharacterClass { .. }
                if !seq.is_empty() => {
                    out.push(std::mem::take(&mut seq));
                }
            _ => {}
        }
    }
    if !seq.is_empty() {
        out.push(seq);
    }
    out
}

fn is_unicode_code_point_escape(ast: &RegExpAst, id: Nid) -> bool {
    let raw = ast.raw16(id);
    raw.len() > 4
        && raw[..3] == [0x5c, 0x75, 0x7b]
        && raw[raw.len() - 1] == 0x7d
        && raw[3..raw.len() - 1]
            .iter()
            .all(|c| *c < 0x80 && (*c as u8).is_ascii_hexdigit())
}

fn find_sequences(
    ast: &RegExpAst,
    kind: &str,
    chars: &[Option<Nid>],
    unfiltered: &[Nid],
) -> Vec<Vec<Nid>> {
    let mut out = Vec::new();
    let v = |id: Nid| value(ast, id);
    let ucpe = |id: Nid| is_unicode_code_point_escape(ast, id);
    if kind == "zwj" {
        let mut sequence: Option<Vec<Nid>> = None;
        for index in 0..chars.len() {
            let previous = if index > 0 { chars[index - 1] } else { None };
            let (Some(previous), Some(ch), Some(next)) = (
                previous,
                chars[index],
                chars.get(index + 1).copied().flatten(),
            ) else {
                continue;
            };
            if v(ch) == 0x200d && v(previous) != 0x200d && v(next) != 0x200d {
                match &mut sequence {
                    Some(s) if s.last() == Some(&previous) => {
                        s.push(ch);
                        s.push(next);
                    }
                    Some(s) => {
                        out.push(std::mem::replace(s, vec![previous, ch, next]));
                    }
                    None => sequence = Some(vec![previous, ch, next]),
                }
            }
        }
        if let Some(s) = sequence {
            out.push(s);
        }
        return out;
    }
    for index in 0..chars.len() {
        let previous = if index == 0 {
            None
        } else if kind == "combiningClass" {
            unfiltered.get(index - 1).copied()
        } else {
            chars[index - 1]
        };
        let (Some(previous), Some(ch)) = (previous, chars[index]) else {
            continue;
        };
        let hit = match kind {
            "surrogatePairWithoutUFlag" => {
                is_surrogate_pair(v(previous), v(ch)) && !ucpe(previous) && !ucpe(ch)
            }
            "surrogatePair" => {
                is_surrogate_pair(v(previous), v(ch)) && (ucpe(previous) || ucpe(ch))
            }
            "combiningClass" => {
                is_combining_character(v(ch)) && !is_combining_character(v(previous))
            }
            "emojiModifier" => is_emoji_modifier(v(ch)) && !is_emoji_modifier(v(previous)),
            _ => is_regional_indicator_symbol(v(ch)) && is_regional_indicator_symbol(v(previous)),
        };
        if hit {
            out.push(vec![previous, ch]);
        }
    }
    out
}

fn code_points(s: &[u16]) -> Vec<&[u16]> {
    let mut out = Vec::new();
    let mut i = 0;
    while i < s.len() {
        let w = if (0xd800..0xdc00).contains(&s[i])
            && s.get(i + 1).is_some_and(|t| (0xdc00..0xe000).contains(t))
        {
            2
        } else {
            1
        };
        out.push(&s[i..i + w]);
        i += w;
    }
    out
}

fn from_code_point(cp: u32) -> Vec<u16> {
    match char::from_u32(cp) {
        Some(c) => utf16(&c.to_string()),
        None => vec![cp as u16],
    }
}

fn check_for_acceptable_escape(char_value: u32, char_source: &[u16]) -> bool {
    if char_source.first() != Some(&0x5c) {
        return false;
    }
    let cps = code_points(char_source);
    let matched = cps.len() >= 2 && cps[..cps.len() - 1].iter().all(|c| *c == [0x5c]);
    if !matched {
        return true;
    }
    cps[cps.len() - 1] != from_code_point(char_value).as_slice()
}

enum Source {
    Regex,
    Units(Vec<u16>, Vec<CodeUnit>),
    Other,
}

impl R {
    fn verify(
        &mut self,
        cx: &mut Cx,
        node: NodeId,
        pattern: &[u16],
        flags: &[u16],
        unicode_fix: Option<Fix>,
    ) {
        let Ok(ast) = parse_pattern(pattern, flags.contains(&0x75), flags.contains(&0x76)) else {
            return;
        };
        let sast = &cx.sc.ast;
        let source = if sast.is(node, "Literal") && sast.prop(node, "regex").is_some() {
            Source::Regex
        } else if sast.is(node, "Literal")
            && matches!(sast.prop(node, "value"), Some(crate::ast::Prop::Str(_)))
        {
            let text = node_text16(cx, node);
            let units = parse_string_literal(&text);
            Source::Units(text, units)
        } else if is_static_template_literal(sast, node) {
            let text = node_text16(cx, node);
            let units = parse_template_token(&text);
            Source::Units(text, units)
        } else {
            Source::Other
        };
        let mut bad_index = false;
        let acceptable = |id: Nid, bad: &mut bool| -> bool {
            let ch = value(&ast, id);
            match &source {
                Source::Regex => check_for_acceptable_escape(ch, ast.raw16(id)),
                Source::Units(text, units) => {
                    let n = ast.node(id);
                    let (Some(first), Some(last)) = (
                        units.get(n.start),
                        n.end.checked_sub(1).and_then(|i| units.get(i)),
                    ) else {
                        *bad = true;
                        return false;
                    };
                    let (s, e) = (first.start.min(text.len()), last.end().min(text.len()));
                    check_for_acceptable_escape(ch, if s < e { &text[s..e] } else { &text[0..0] })
                }
                Source::Other => false,
            }
        };
        let mut found: Vec<(&'static str, Vec<Vec<Nid>>)> = Vec::new();
        let mut classes = Vec::new();
        ast.visit(&mut |a, id, enter| {
            if let (true, K::CharacterClass { elements, .. }) = (enter, &a.node(id).k) {
                classes.push(elements.clone());
            }
        });
        for elements in classes {
            for unfiltered in iterate_character_sequence(&ast, &elements) {
                let chars: Vec<Option<Nid>> = if self.allow_escape {
                    unfiltered
                        .iter()
                        .map(|c| {
                            if acceptable(*c, &mut bad_index) {
                                None
                            } else {
                                Some(*c)
                            }
                        })
                        .collect()
                } else {
                    unfiltered.iter().map(|c| Some(*c)).collect()
                };
                for kind in KINDS {
                    let matches = find_sequences(&ast, kind, &chars, &unfiltered);
                    match found.iter_mut().find(|(k, _)| *k == kind) {
                        Some((_, list)) => list.extend(matches),
                        None => found.push((kind, matches)),
                    }
                }
            }
        }
        let is_literal = sast.is(node, "Literal");
        let node_start = sast.range(node).0 as usize;
        let mut reports = Vec::new();
        for (kind, matches) in &found {
            let locs: Vec<Loc> = if !is_static_template_literal(sast, node) && !is_literal {
                if matches.is_empty() {
                    Vec::new()
                } else {
                    vec![cx.sc.node_loc(node)]
                }
            } else {
                let mut locs = Vec::new();
                for chars in matches {
                    let (Some(first), Some(last)) = (chars.first(), chars.last()) else {
                        continue;
                    };
                    let first_index = ast.node(*first).start;
                    let Some(last_index) = ast.node(*last).end.checked_sub(1) else {
                        bad_index = true;
                        continue;
                    };
                    let (start, end) = match &source {
                        Source::Units(_, units) => {
                            let (Some(f), Some(l)) =
                                (units.get(first_index), units.get(last_index))
                            else {
                                bad_index = true;
                                continue;
                            };
                            (node_start + f.start, node_start + l.end())
                        }
                        _ => (
                            node_start + 1 + first_index,
                            node_start + 1 + last_index + 1,
                        ),
                    };
                    locs.push(Loc {
                        start: cx.sc.loc_of(start as u32),
                        end: cx.sc.loc_of(end as u32),
                    });
                }
                locs
            };
            for loc in locs {
                reports.push((*kind, loc));
            }
        }
        if bad_index {
            self.unsupported = Some("code unit index out of range");
            return;
        }
        for (kind, loc) in reports {
            let mut r = Report::loc(loc, kind);
            r.node = Some(node);
            if kind == "surrogatePairWithoutUFlag" {
                r.suggest.push(SuggestSpec {
                    keys: vec![
                        ("messageId", SugVal::Str("suggestUnicodeFlag".into())),
                        ("fix", SugVal::Fix),
                    ],
                    fix: unicode_fix.clone(),
                });
            }
            cx.report(r);
        }
    }
}

impl Rule for R {
    fn listeners(&self) -> &'static [&'static str] {
        &["Literal[regex]", "Program"]
    }

    fn on(&mut self, l: usize, node: NodeId, cx: &mut Cx) {
        if l == 0 {
            if self.checked.contains(&node) {
                return;
            }
            let Some((pattern, flags)) = regex_of(cx, node) else {
                return;
            };
            let end = cx.sc.ast.range(node).1;
            let fix = is_valid_with_unicode_flag(ECMA_VERSION, &pattern)
                .then(|| fixer::insert_after_range((end, end), "u"));
            self.verify(cx, node, &pattern, &utf16(&flags), fix);
            return;
        }
        let scope = cx.get_scope(node);
        let calls = match regexp_global_calls(cx) {
            Ok(c) => c,
            Err(e) => {
                self.unsupported = Some(e);
                return;
            }
        };
        for call in calls {
            let ast = &cx.sc.ast;
            let args: Vec<Option<NodeId>> = ast.list(call, "arguments").to_vec();
            let (Some(pattern_node), flags_node) = (
                args.first().copied().flatten(),
                args.get(1).copied().flatten(),
            ) else {
                continue;
            };
            let regex = regex_of(cx, pattern_node);
            let evaluated = if regex.is_some() {
                Static::Regex
            } else {
                match static_value(cx, Some(pattern_node), scope) {
                    Static::Regex => Static::Null,
                    other => other,
                }
            };
            let pattern = match (&evaluated, &regex) {
                (Static::Null, _) => continue,
                (Static::Unknown, _) => {
                    self.unsupported = Some("RegExp argument this port does not evaluate");
                    return;
                }
                (Static::Regex, Some((p, _))) => {
                    if flags_node.is_none() {
                        continue;
                    }
                    self.checked.push(pattern_node);
                    p.clone()
                }
                (Static::Str(s), _) => s.clone(),
                _ => continue,
            };
            let flags = match flags_node {
                Some(f) => match string_if_constant(cx, Some(f), scope) {
                    Static::Str(s) => Some(s),
                    Static::Null => None,
                    _ => {
                        self.unsupported = Some("RegExp argument this port does not evaluate");
                        return;
                    }
                },
                None => Some(Vec::new()),
            };
            let Some(flags) = flags else { continue };
            let fix = if !is_valid_with_unicode_flag(ECMA_VERSION, &pattern) {
                None
            } else if args.len() == 1 {
                cx.sc
                    .last_token(
                        call,
                        crate::tokens::Opts {
                            include_comments: false,
                            skip: 1,
                        },
                    )
                    .map(|t| {
                        fixer::insert_after_range(
                            (t.start, t.end),
                            if is_comma(t) { " \"u\"," } else { ", \"u\"" },
                        )
                    })
            } else {
                flags_node.and_then(|f| {
                    let ast = &cx.sc.ast;
                    let string = ast.is(f, "Literal")
                        && matches!(ast.prop(f, "value"), Some(crate::ast::Prop::Str(_)));
                    (string || ast.is(f, "TemplateLiteral")).then(|| {
                        let (s, e) = ast.range(f);
                        fixer::insert_after_range((s, e - 1), "u")
                    })
                })
            };
            self.verify(cx, pattern_node, &pattern, &flags, fix);
        }
    }

    fn unsupported(&self) -> Option<&str> {
        self.unsupported
    }
}
