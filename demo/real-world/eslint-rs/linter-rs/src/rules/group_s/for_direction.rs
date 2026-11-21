//! lib/rules/for-direction.js

use crate::ast::NodeId;
use crate::linter::Cx;
use crate::rule::Report;
use crate::rules::{Rule, RuleDef};
use crate::source::Loc;
use crate::static_value::{V, get_static_value};
use crate::tokens::N;
use serde_json::Value;

pub static DEF: RuleDef = RuleDef {
    name: "for-direction",
    messages: &[(
        "incorrectDirection",
        "The update clause in this loop moves the variable in the wrong direction.",
    )],
    has_default_options: false,
    create,
};

struct R {
    unsupported: bool,
}

fn create(options: &[Value]) -> Result<Box<dyn Rule>, String> {
    super::common::no_options(options)?;
    Ok(Box::new(R { unsupported: false }))
}

impl R {
    fn right_direction(&mut self, cx: &Cx, update: NodeId, dir: i32) -> i32 {
        let ast = &cx.sc.ast;
        let Some(right) = ast.child(update, "right") else {
            return 0;
        };
        let scope = cx.get_scope(update);
        let v = match get_static_value(ast, cx.sm(), right, Some(scope)) {
            Ok(v) => v,
            Err(_) => {
                self.unsupported = true;
                return 0;
            }
        };
        let n = match v.map(|v| v.value) {
            Some(V::Num(n)) => n,
            Some(V::Bool(b)) => f64::from(u8::from(b)),
            Some(V::BigInt(b)) => b as f64,
            _ => return 0,
        };
        let sign = if n.is_nan() || n == 0.0 {
            0
        } else if n > 0.0 {
            1
        } else {
            -1
        };
        dir * sign
    }

    fn direction(&mut self, cx: &Cx, expr: NodeId, counter: &str) -> i32 {
        let ast = &cx.sc.ast;
        let op = ast.str(expr, "operator").unwrap_or("");
        if ast.is(expr, "UpdateExpression") {
            if ast
                .child(expr, "argument")
                .is_some_and(|a| ast.is(a, "Identifier") && ast.name(a) == Some(counter))
            {
                return match op {
                    "++" => 1,
                    "--" => -1,
                    _ => 0,
                };
            }
            return 0;
        }
        if ast.is(expr, "AssignmentExpression")
            && ast.child(expr, "left").and_then(|l| ast.name(l)) == Some(counter)
        {
            return match op {
                "+=" => self.right_direction(cx, expr, 1),
                "-=" => self.right_direction(cx, expr, -1),
                _ => 0,
            };
        }
        0
    }
}

fn modifying(cx: &Cx, node: NodeId, counter: &str, out: &mut Vec<NodeId>) {
    let ast = &cx.sc.ast;
    match ast.kind(node) {
        "SequenceExpression" => {
            for e in ast.nodes_of(node, "expressions") {
                modifying(cx, e, counter, out);
            }
        }
        "UpdateExpression" => {
            if ast
                .child(node, "argument")
                .is_some_and(|a| ast.is(a, "Identifier") && ast.name(a) == Some(counter))
            {
                out.push(node);
            }
        }
        "AssignmentExpression"
            if ast
                .child(node, "left")
                .is_some_and(|a| ast.is(a, "Identifier") && ast.name(a) == Some(counter))
            => {
                out.push(node);
            }
        _ => {}
    }
}

impl Rule for R {
    fn listeners(&self) -> &'static [&'static str] {
        &["ForStatement"]
    }

    fn on(&mut self, _: usize, node: NodeId, cx: &mut Cx) {
        let ast = &cx.sc.ast;
        let (Some(test), Some(update)) = (ast.child(node, "test"), ast.child(node, "update"))
        else {
            return;
        };
        if !ast.is(test, "BinaryExpression") {
            return;
        }
        for pos in ["left", "right"] {
            let Some(side) = ast.child(test, pos) else {
                continue;
            };
            if !ast.is(side, "Identifier") {
                continue;
            }
            let counter = ast.name(side).unwrap_or("").to_string();
            let wrong = match ast.str(test, "operator") {
                Some("<" | "<=") => {
                    if pos == "left" {
                        -1
                    } else {
                        1
                    }
                }
                Some(">" | ">=") => {
                    if pos == "left" {
                        1
                    } else {
                        -1
                    }
                }
                _ => return,
            };
            let mut exprs = Vec::new();
            modifying(cx, update, &counter, &mut exprs);
            if exprs.len() == 1 && self.direction(cx, exprs[0], &counter) == wrong {
                let Some(body) = ast.child(node, "body") else {
                    return;
                };
                let Some(before) = cx.sc.token_before(body, N) else {
                    return;
                };
                let loc = Loc {
                    start: cx.sc.node_loc(node).start,
                    end: cx.sc.loc_of(before.end),
                };
                cx.report(Report::loc(loc, "incorrectDirection"));
            }
        }
    }

    fn unsupported(&self) -> Option<&str> {
        self.unsupported
            .then_some("getStaticValue met a value this port does not model")
    }
}
