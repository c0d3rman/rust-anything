//! lib/rules/no-unsafe-optional-chaining.js

use crate::ast::{Ast, NodeId};
use crate::linter::Cx;
use crate::rule::Report;
use crate::rules::{Rule, RuleDef};
use serde_json::Value;

pub static DEF: RuleDef = RuleDef {
    name: "no-unsafe-optional-chaining",
    messages: &[
        (
            "unsafeOptionalChain",
            "Unsafe usage of optional chaining. If it short-circuits with 'undefined' the evaluation will throw TypeError.",
        ),
        (
            "unsafeArithmetic",
            "Unsafe arithmetic operation on optional chaining. It can result in NaN.",
        ),
    ],
    has_default_options: true,
    create,
};

struct R {
    arithmetic: bool,
}

fn create(options: &[Value]) -> Result<Box<dyn Rule>, String> {
    if options.len() > 1 {
        return Err("invalid option".into());
    }
    let o = super::bool_props(options, 0, &["disallowArithmeticOperators"])?;
    Ok(Box::new(R {
        arithmetic: o[0].unwrap_or(false),
    }))
}

fn check(ast: &Ast, node: Option<NodeId>, out: &mut Vec<NodeId>) {
    let Some(node) = node else { return };
    match ast.kind(node) {
        "LogicalExpression" => match ast.str(node, "operator") {
            Some("||" | "??") => check(ast, ast.child(node, "right"), out),
            Some("&&") => {
                check(ast, ast.child(node, "left"), out);
                check(ast, ast.child(node, "right"), out);
            }
            _ => {}
        },
        "SequenceExpression" => check(
            ast,
            ast.list(node, "expressions").last().copied().flatten(),
            out,
        ),
        "ConditionalExpression" => {
            check(ast, ast.child(node, "consequent"), out);
            check(ast, ast.child(node, "alternate"), out);
        }
        "AwaitExpression" => check(ast, ast.child(node, "argument"), out),
        "ChainExpression" => out.push(node),
        _ => {}
    }
}

fn is_destructuring(ast: &Ast, n: Option<NodeId>) -> bool {
    n.is_some_and(|n| matches!(ast.kind(n), "ObjectPattern" | "ArrayPattern"))
}

const ARITH: &[&str] = &["+", "-", "/", "*", "%", "**"];

impl Rule for R {
    fn listeners(&self) -> &'static [&'static str] {
        &[
            "AssignmentExpression, AssignmentPattern",
            "ClassDeclaration, ClassExpression",
            "CallExpression",
            "NewExpression",
            "VariableDeclarator",
            "MemberExpression",
            "TaggedTemplateExpression",
            "ForOfStatement",
            "SpreadElement",
            "YieldExpression",
            "BinaryExpression",
            "WithStatement",
            "UnaryExpression",
            "AssignmentExpression",
        ]
    }

    fn on(&mut self, l: usize, node: NodeId, cx: &mut Cx) {
        let sc = cx.sc;
        let ast = &sc.ast;
        let mut usage = Vec::new();
        let mut arith = Vec::new();
        let op = ast.str(node, "operator").unwrap_or("");
        match l {
            0 => {
                if is_destructuring(ast, ast.child(node, "left")) {
                    check(ast, ast.child(node, "right"), &mut usage);
                }
            }
            1 => check(ast, ast.child(node, "superClass"), &mut usage),
            2 => {
                if !ast.bool(node, "optional") {
                    check(ast, ast.child(node, "callee"), &mut usage);
                }
            }
            3 => check(ast, ast.child(node, "callee"), &mut usage),
            4 => {
                if is_destructuring(ast, ast.child(node, "id")) {
                    check(ast, ast.child(node, "init"), &mut usage);
                }
            }
            5 => {
                if !ast.bool(node, "optional") {
                    check(ast, ast.child(node, "object"), &mut usage);
                }
            }
            6 => check(ast, ast.child(node, "tag"), &mut usage),
            7 => check(ast, ast.child(node, "right"), &mut usage),
            8 => {
                if ast
                    .parent(node)
                    .is_some_and(|p| !ast.is(p, "ObjectExpression"))
                {
                    check(ast, ast.child(node, "argument"), &mut usage);
                }
            }
            9 => {
                if ast.bool(node, "delegate") {
                    check(ast, ast.child(node, "argument"), &mut usage);
                }
            }
            10 => {
                if op == "in" || op == "instanceof" {
                    check(ast, ast.child(node, "right"), &mut usage);
                }
                if self.arithmetic && ARITH.contains(&op) {
                    check(ast, ast.child(node, "right"), &mut arith);
                    check(ast, ast.child(node, "left"), &mut arith);
                }
            }
            11 => check(ast, ast.child(node, "object"), &mut usage),
            12 => {
                if self.arithmetic && ARITH.contains(&op) {
                    check(ast, ast.child(node, "argument"), &mut arith);
                }
            }
            _ => {
                if self.arithmetic && matches!(op, "+=" | "-=" | "/=" | "*=" | "%=" | "**=") {
                    check(ast, ast.child(node, "right"), &mut arith);
                }
            }
        }
        for n in usage {
            cx.report(Report::node(n, "unsafeOptionalChain"));
        }
        for n in arith {
            cx.report(Report::node(n, "unsafeArithmetic"));
        }
    }
}
