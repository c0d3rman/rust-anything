//! lib/rules/no-fallthrough.js

use crate::rules::group_a::mini_regex::Regex;
use super::segs;
use crate::ast::NodeId;
use crate::code_path::{Event, Graph, SegId};
use crate::linter::Cx;
use crate::rule::Report;
use crate::rules::{Rule, RuleDef};
use crate::source::Token;
use crate::tokens::N;
use crate::utils::{is_js_whitespace, js_trim};
use serde_json::Value;
use std::rc::Rc;

pub static DEF: RuleDef = RuleDef {
    name: "no-fallthrough",
    messages: &[
        (
            "unusedFallthroughComment",
            "Found a comment that would permit fallthrough, but case cannot fall through.",
        ),
        ("case", "Expected a 'break' statement before 'case'."),
        ("default", "Expected a 'break' statement before 'default'."),
    ],
    has_default_options: true,
    create,
};

struct PrevCase {
    node: NodeId,
    is_switch_exit_reachable: bool,
    is_fallthrough: bool,
}

struct R {
    graph: Option<Rc<Graph>>,
    code_path_segments: Vec<Vec<SegId>>,
    current: Vec<SegId>,
    allow_empty_case: bool,
    report_unused: bool,
    pattern: Option<Regex>,
    unsupported: Option<&'static str>,
    previous: Option<PrevCase>,
}

fn create(options: &[Value]) -> Result<Box<dyn Rule>, String> {
    let mut r = R {
        graph: None,
        code_path_segments: Vec::new(),
        current: Vec::new(),
        allow_empty_case: false,
        report_unused: false,
        pattern: None,
        unsupported: None,
        previous: None,
    };
    match options {
        [] => {}
        [Value::Object(o)] => {
            for (k, v) in o {
                match (k.as_str(), v) {
                    ("commentPattern", Value::String(s)) => {
                        if !s.is_empty() {
                            match Regex::new(s) {
                                Some(re) => r.pattern = Some(re),
                                None => r.unsupported = Some("commentPattern with regular expression syntax"),
                            }
                        }
                    }
                    ("allowEmptyCase", Value::Bool(b)) => r.allow_empty_case = *b,
                    ("reportUnusedFallthroughComment", Value::Bool(b)) => r.report_unused = *b,
                    _ => return Err("invalid option".into()),
                }
            }
        }
        _ => return Err("invalid option".into()),
    }
    Ok(Box::new(r))
}

/// `/falls?\s?through/iu.test(s)`.
fn default_pattern(s: &str) -> bool {
    let c: Vec<char> = s.chars().collect();
    let eq = |x: char, l: char| x.to_ascii_lowercase() == l || (l == 's' && x == '\u{17f}');
    let lit = |i: &mut usize, w: &str| {
        for l in w.chars() {
            if *i < c.len() && eq(c[*i], l) {
                *i += 1;
            } else {
                return false;
            }
        }
        true
    };
    for start in 0..c.len() {
        let mut i = start;
        if !lit(&mut i, "fall") {
            continue;
        }
        if i < c.len() && eq(c[i], 's') {
            i += 1;
        }
        if i < c.len() && is_js_whitespace(c[i]) {
            i += 1;
        }
        if lit(&mut i, "through") {
            return true;
        }
    }
    false
}

/// `directivesPattern` (lib/shared/directives.js).
fn is_directive(s: &str) -> bool {
    let words = [
        "eslint-disable-next-line",
        "eslint-disable-line",
        "eslint-disable",
        "eslint-enable",
        "eslint-env",
        "eslint",
        "exported",
        "globals",
        "global",
    ];
    words.iter().any(|w| {
        s.strip_prefix(w)
            .is_some_and(|rest| rest.chars().next().is_none_or(is_js_whitespace))
    })
}

impl R {
    fn is_fall_through_comment(&self, value: &str) -> bool {
        let m = match &self.pattern {
            Some(p) => p.test(value),
            None => default_pattern(value),
        };
        m && !is_directive(js_trim(value))
    }

    fn fallthrough_comment<'a>(
        &self,
        cx: &'a Cx,
        case: NodeId,
        subsequent: NodeId,
    ) -> Option<&'a Token> {
        let sc = cx.sc;
        let ast = &sc.ast;
        let cons = ast.list(case, "consequent");
        if cons.len() == 1
            && let Some(block) = cons[0]
            && ast.is(block, "BlockStatement")
            && let Some(close) = sc.last_token(block, N)
            && let Some(c) = sc.comments_before(close.start).pop()
            && self.is_fall_through_comment(&c.value)
        {
            return Some(c);
        }
        let c = sc.comments_before(ast.range(subsequent).0).pop()?;
        self.is_fall_through_comment(&c.value).then_some(c)
    }
}

impl Rule for R {
    fn listeners(&self) -> &'static [&'static str] {
        &[
            "onCodePathStart",
            "onCodePathEnd",
            "onUnreachableCodePathSegmentStart",
            "onUnreachableCodePathSegmentEnd",
            "onCodePathSegmentStart",
            "onCodePathSegmentEnd",
            "SwitchCase",
            "SwitchCase:exit",
        ]
    }

    fn unsupported(&self) -> Option<&str> {
        self.unsupported
    }

    fn on_code_path(&mut self, l: usize, ev: &Event, _cx: &mut Cx) {
        segs::keep(&mut self.graph, ev);
        match l {
            0 => self
                .code_path_segments
                .push(std::mem::take(&mut self.current)),
            1 => self.current = self.code_path_segments.pop().unwrap_or_default(),
            2 | 4 => {
                if let Some(s) = ev.segment() {
                    segs::add(&mut self.current, s);
                }
            }
            3 | 5 => {
                if let Some(s) = ev.segment() {
                    segs::delete(&mut self.current, s);
                }
            }
            _ => {}
        }
    }

    fn on(&mut self, l: usize, node: NodeId, cx: &mut Cx) {
        let sc = cx.sc;
        let ast = &sc.ast;
        if l == 6 {
            if let Some(prev) = &self.previous
                && ast.parent(prev.node) == ast.parent(node)
            {
                let comment = self
                    .fallthrough_comment(cx, prev.node, node)
                    .map(|c| cx.sc.tok_loc(c));
                if prev.is_fallthrough && comment.is_none() {
                    let id = if ast.child(node, "test").is_some() {
                        "case"
                    } else {
                        "default"
                    };
                    cx.report(Report::node(node, id));
                } else if self.report_unused
                    && !prev.is_switch_exit_reachable
                    && let Some(loc) = comment
                {
                    cx.report(Report::loc(loc, "unusedFallthroughComment"));
                }
            }
            self.previous = None;
        } else {
            let Some(next) = cx.sc.token_after(node, N) else {
                return;
            };
            let next_line = cx.sc.loc_of(next.start).line;
            let reachable = segs::any_reachable(self.graph.as_ref(), &self.current);
            let blank = next_line > cx.sc.node_loc(node).end.line + 1;
            let last = ast
                .parent(node)
                .and_then(|p| ast.list(p, "cases").last().copied().flatten());
            let is_fallthrough = reachable
                && (!ast.list(node, "consequent").is_empty() || (!self.allow_empty_case && blank))
                && last != Some(node);
            self.previous = Some(PrevCase {
                node,
                is_switch_exit_reachable: reachable,
                is_fallthrough,
            });
        }
    }
}
