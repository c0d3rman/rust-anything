//! lib/rules/no-regex-spaces.js

use super::util::{node_text16, regex_of, string_value, utf16};
use crate::ast::NodeId;
use crate::linter::Cx;
use crate::regexpp::{K, parse_pattern};
use crate::rule::{Report, fixer};
use crate::rules::{Rule, RuleDef};
use serde_json::Value;

pub static DEF: RuleDef = RuleDef {
    name: "no-regex-spaces",
    messages: &[(
        "multipleSpaces",
        "Spaces are hard to count. Use {{{length}}}.",
    )],
    has_default_options: false,
    create,
};

struct R;

fn create(options: &[Value]) -> Result<Box<dyn Rule>, String> {
    super::no_options(options)?;
    Ok(Box::new(R))
}

const SPACE: u16 = 0x20;

fn is_quantifier_start(c: u16) -> bool {
    matches!(c, 0x2b | 0x2a | 0x7b | 0x3f)
}

fn exec_spaces(p: &[u16], last_index: usize) -> Option<(usize, usize, usize)> {
    let width = |j: usize| {
        let lead = p[j];
        if (0xd800..0xdc00).contains(&lead)
            && p.get(j + 1).is_some_and(|t| (0xdc00..0xe000).contains(t))
        {
            2
        } else {
            1
        }
    };
    let mut i = last_index;
    while i <= p.len() {
        let n = p[i..].iter().take_while(|c| **c == SPACE).count();
        for k in (2..=n).rev() {
            let j = i + k;
            if p.get(j) == Some(&SPACE) && p.get(j + 1).is_some_and(|c| is_quantifier_start(*c)) {
                return Some((i, k, j + 2));
            }
            if j < p.len() && !is_quantifier_start(p[j]) {
                return Some((i, k, j + width(j)));
            }
            if j == p.len() {
                return Some((i, k, j));
            }
        }
        if i == p.len() {
            break;
        }
        i += width(i);
    }
    None
}

fn check_regex(
    cx: &mut Cx,
    node: NodeId,
    pattern: &[u16],
    raw_pattern: &[u16],
    raw_start: u32,
    flags: &[u16],
) {
    if !raw_pattern.windows(2).any(|w| w == [SPACE, SPACE]) {
        return;
    }
    let has = |c: u8| flags.contains(&u16::from(c));
    let Ok(ast) = parse_pattern(pattern, has(b'u'), has(b'v')) else {
        return;
    };
    let mut classes = Vec::new();
    ast.visit(&mut |a, id, enter| {
        if enter && matches!(a.node(id).k, K::CharacterClass { .. }) {
            classes.push((a.node(id).start, a.node(id).end));
        }
    });
    let mut last_index = 0;
    while let Some((index, length, next)) = exec_spaces(pattern, last_index) {
        last_index = next;
        if classes
            .iter()
            .all(|(start, end)| index < *start || *end <= index)
        {
            let mut r = Report::node(node, "multipleSpaces").data("length", length);
            if pattern == raw_pattern {
                let s = raw_start + index as u32;
                r = r.fix(fixer::replace_range(
                    (s, s + length as u32),
                    format!(" {{{length}}}"),
                ));
            }
            cx.report(r);
            return;
        }
    }
}

impl Rule for R {
    fn listeners(&self) -> &'static [&'static str] {
        &["Literal", "CallExpression", "NewExpression"]
    }

    fn on(&mut self, l: usize, node: NodeId, cx: &mut Cx) {
        let ast = &cx.sc.ast;
        if l == 0 {
            let Some((pattern, flags)) = regex_of(cx, node) else {
                return;
            };
            let raw = node_text16(cx, node);
            let Some(last) = raw.iter().rposition(|c| *c == 0x2f) else {
                return;
            };
            let raw_pattern = raw[1..last].to_vec();
            let start = ast.range(node).0 + 1;
            check_regex(cx, node, &pattern, &raw_pattern, start, &utf16(&flags));
            return;
        }
        let scope = cx.get_scope(node);
        let sm = cx.sm();
        let shadowed = sm
            .variable_by_name(scope, "RegExp")
            .is_some_and(|v| !sm.var(v).defs.is_empty());
        let Some(callee) = ast.child(node, "callee") else {
            return;
        };
        let args = ast.list(node, "arguments");
        let Some(pattern_node) = args.first().copied().flatten() else {
            return;
        };
        if !ast.is(callee, "Identifier") || ast.name(callee) != Some("RegExp") || shadowed {
            return;
        }
        let Some(pattern) = string_value(cx, pattern_node) else {
            return;
        };
        let raw = node_text16(cx, pattern_node);
        if raw.len() < 2 {
            return;
        }
        let raw_pattern = raw[1..raw.len() - 1].to_vec();
        let start = ast.range(pattern_node).0 + 1;
        let flags = if args.len() < 2 {
            Vec::new()
        } else {
            match args[1].and_then(|f| string_value(cx, f)) {
                Some(f) => f,
                None => return,
            }
        };
        check_regex(cx, node, &pattern, &raw_pattern, start, &flags);
    }
}
