//! lib/rules/no-useless-backreference.js

use super::util::{Static, regex_of, regexp_global_calls, string_if_constant, utf16};
use crate::ast::NodeId;
use crate::linter::Cx;
use crate::regexpp::{K, Nid, RegExpAst, parse_pattern};
use crate::rule::Report;
use crate::rules::{Rule, RuleDef};
use serde_json::Value;

pub static DEF: RuleDef = RuleDef {
    name: "no-useless-backreference",
    messages: &[
        (
            "nested",
            "Backreference '{{ bref }}' will be ignored. It references group '{{ group }}'{{ otherGroups }} from within that group.",
        ),
        (
            "forward",
            "Backreference '{{ bref }}' will be ignored. It references group '{{ group }}'{{ otherGroups }} which appears later in the pattern.",
        ),
        (
            "backward",
            "Backreference '{{ bref }}' will be ignored. It references group '{{ group }}'{{ otherGroups }} which appears before in the same lookbehind.",
        ),
        (
            "disjunctive",
            "Backreference '{{ bref }}' will be ignored. It references group '{{ group }}'{{ otherGroups }} which is in another alternative.",
        ),
        (
            "intoNegativeLookaround",
            "Backreference '{{ bref }}' will be ignored. It references group '{{ group }}'{{ otherGroups }} which is in a negative lookaround.",
        ),
    ],
    has_default_options: false,
    create,
};

struct R {
    unsupported: Option<&'static str>,
}

fn create(options: &[Value]) -> Result<Box<dyn Rule>, String> {
    super::no_options(options)?;
    Ok(Box::new(R { unsupported: None }))
}

fn path_to_root(ast: &RegExpAst, node: Nid) -> Vec<Nid> {
    let mut path = vec![node];
    let mut cur = ast.node(node).parent;
    while let Some(p) = cur {
        path.push(p);
        cur = ast.node(p).parent;
    }
    path
}

fn is_lookaround(ast: &RegExpAst, node: Nid) -> bool {
    matches!(
        ast.node(node).k,
        K::Assertion {
            kind: "lookahead" | "lookbehind",
            ..
        }
    )
}

fn is_negative_lookaround(ast: &RegExpAst, node: Nid) -> bool {
    is_lookaround(ast, node) && matches!(ast.node(node).k, K::Assertion { negate: true, .. })
}

fn problem(ast: &RegExpAst, bref: Nid, group: Nid) -> Option<&'static str> {
    let bref_path = path_to_root(ast, bref);
    let group_path = path_to_root(ast, group);
    if bref_path.contains(&group) {
        return Some("nested");
    }
    let mut i = bref_path.len() as isize - 1;
    let mut j = group_path.len() as isize - 1;
    let at = |p: &Vec<Nid>, k: isize| {
        if k < 0 {
            None
        } else {
            p.get(k as usize).copied()
        }
    };
    loop {
        i -= 1;
        j -= 1;
        if at(&bref_path, i) != at(&group_path, j) {
            break;
        }
    }
    let lca = (j + 1).max(0) as usize;
    let group_cut = &group_path[..lca.min(group_path.len())];
    let common_path = &group_path[lca.min(group_path.len())..];
    let lowest_common_lookaround = common_path.iter().copied().find(|n| is_lookaround(ast, *n));
    let is_matching_backward = lowest_common_lookaround.is_some_and(|n| {
        matches!(
            ast.node(n).k,
            K::Assertion {
                kind: "lookbehind",
                ..
            }
        )
    });
    if group_cut
        .last()
        .is_some_and(|n| matches!(ast.node(*n).k, K::Alternative { .. }))
    {
        return Some("disjunctive");
    }
    let (b, g) = (ast.node(bref), ast.node(group));
    if !is_matching_backward && b.end <= g.start {
        return Some("forward");
    }
    if is_matching_backward && g.end <= b.start {
        return Some("backward");
    }
    if group_cut.iter().any(|n| is_negative_lookaround(ast, *n)) {
        return Some("intoNegativeLookaround");
    }
    None
}

impl R {
    fn check_regex(&mut self, cx: &mut Cx, node: NodeId, pattern: &[u16], flags: &[u16]) {
        let Ok(ast) = parse_pattern(pattern, flags.contains(&0x75), flags.contains(&0x76)) else {
            return;
        };
        let mut reports = Vec::new();
        ast.visit(&mut |a, id, enter| {
            let (true, K::Backreference { resolved, .. }) = (enter, &a.node(id).k) else {
                return;
            };
            let problems: Vec<Option<(&'static str, Nid)>> = resolved
                .iter()
                .map(|g| problem(a, id, *g).map(|m| (m, *g)))
                .collect();
            if problems.is_empty() || problems.iter().any(Option::is_none) {
                return;
            }
            let problems: Vec<(&'static str, Nid)> = problems.into_iter().flatten().collect();
            let same: Vec<(&'static str, Nid)> = problems
                .iter()
                .copied()
                .filter(|p| p.0 != "disjunctive")
                .collect();
            let to_report = if same.is_empty() { problems } else { same };
            let (message_id, group) = to_report[0];
            let other = to_report.len() - 1;
            let other_groups = match other {
                0 => String::new(),
                1 => " and another group".to_string(),
                n => format!(" and other {n} groups"),
            };
            reports.push((message_id, a.raw(id), a.raw(group), other_groups));
        });
        for (message_id, bref, group, other_groups) in reports {
            let (Some(bref), Some(group)) = (bref, group) else {
                self.unsupported = Some("pattern with a lone surrogate");
                return;
            };
            cx.report(
                Report::node(node, message_id)
                    .data("bref", bref)
                    .data("group", group)
                    .data("otherGroups", other_groups),
            );
        }
    }
}

impl Rule for R {
    fn listeners(&self) -> &'static [&'static str] {
        &["Literal[regex]", "Program"]
    }

    fn on(&mut self, l: usize, node: NodeId, cx: &mut Cx) {
        if l == 0 {
            let Some((pattern, flags)) = regex_of(cx, node) else {
                return;
            };
            self.check_regex(cx, node, &pattern, &utf16(&flags));
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
            let args = cx.sc.ast.list(call, "arguments");
            let (pattern_node, flags_node) = (
                args.first().copied().flatten(),
                args.get(1).copied().flatten(),
            );
            let pattern = string_if_constant(cx, pattern_node, scope);
            let flags = string_if_constant(cx, flags_node, scope);
            match (pattern, flags) {
                (Static::Null, _) => {}
                (Static::Unknown, _) | (_, Static::Unknown) => {
                    self.unsupported = Some("RegExp argument this port does not evaluate");
                    return;
                }
                (Static::Str(p), f) => {
                    let f = if let Static::Str(f) = f {
                        f
                    } else {
                        Vec::new()
                    };
                    self.check_regex(cx, call, &p, &f);
                }
                _ => {}
            }
        }
    }

    fn unsupported(&self) -> Option<&str> {
        self.unsupported
    }
}
