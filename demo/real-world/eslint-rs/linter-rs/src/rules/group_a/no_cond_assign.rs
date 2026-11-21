//! lib/rules/no-cond-assign.js

use crate::ast::NodeId;
use crate::linter::Cx;
use crate::rule::Report;
use crate::rules::{Rule, RuleDef};
use crate::source::SourceCode;
use crate::tokens::Opts;
use crate::utils::{is_closing_paren, is_function, is_opening_paren, is_parenthesised};
use serde_json::Value;

pub static DEF: RuleDef = RuleDef {
    name: "no-cond-assign",
    messages: &[
        ("unexpected", "Unexpected assignment within {{type}}."),
        (
            "missing",
            "Expected a conditional expression and instead saw an assignment.",
        ),
    ],
    has_default_options: true,
    create,
};

struct R {
    always: bool,
}

fn create(options: &[Value]) -> Result<Box<dyn Rule>, String> {
    let always = match options {
        [] => false,
        [Value::String(s)] if s == "except-parens" => false,
        [Value::String(s)] if s == "always" => true,
        _ => return Err("invalid option".into()),
    };
    Ok(Box::new(R { always }))
}

fn is_conditional_test(sc: &SourceCode, n: NodeId) -> bool {
    let ast = &sc.ast;
    ast.parent(n).is_some_and(|p| {
        matches!(
            ast.kind(p),
            "IfStatement"
                | "WhileStatement"
                | "DoWhileStatement"
                | "ForStatement"
                | "ConditionalExpression"
        ) && ast.child(p, "test") == Some(n)
    })
}

fn find_conditional_ancestor(sc: &SourceCode, node: NodeId) -> Option<NodeId> {
    let ast = &sc.ast;
    let mut cur = node;
    loop {
        if is_conditional_test(sc, cur) {
            return ast.parent(cur);
        }
        cur = ast.parent(cur)?;
        if is_function(ast, cur) {
            return None;
        }
    }
}

fn is_parenthesised_twice(sc: &SourceCode, n: NodeId) -> bool {
    let (s, e) = sc.ast.range(n);
    let skip1 = Opts {
        include_comments: false,
        skip: 1,
    };
    let (prev, next) = (sc.token_before(n, skip1), sc.token_after(n, skip1));
    is_parenthesised(sc, n)
        && prev.is_some_and(|p| is_opening_paren(p) && p.end <= s)
        && next.is_some_and(|x| is_closing_paren(x) && x.start >= e)
}

impl Rule for R {
    fn listeners(&self) -> &'static [&'static str] {
        if self.always {
            &["AssignmentExpression"]
        } else {
            &[
                "DoWhileStatement",
                "ForStatement",
                "IfStatement",
                "WhileStatement",
                "ConditionalExpression",
            ]
        }
    }

    fn on(&mut self, _: usize, node: NodeId, cx: &mut Cx) {
        let sc = cx.sc;
        let ast = &sc.ast;
        if self.always {
            if let Some(ancestor) = find_conditional_ancestor(sc, node) {
                let kind = ast.kind(ancestor);
                let ty = match kind {
                    "DoWhileStatement" => "a 'do...while' statement",
                    "ForStatement" => "a 'for' statement",
                    "IfStatement" => "an 'if' statement",
                    "WhileStatement" => "a 'while' statement",
                    k => k,
                };
                cx.report(Report::node(node, "unexpected").data("type", ty));
            }
            return;
        }
        let Some(test) = ast.child(node, "test") else {
            return;
        };
        if !ast.is(test, "AssignmentExpression") {
            return;
        }
        let ok = if ast.is(node, "ForStatement") {
            is_parenthesised(sc, test)
        } else {
            is_parenthesised_twice(sc, test)
        };
        if !ok {
            cx.report(Report::node(test, "missing"));
        }
    }
}
