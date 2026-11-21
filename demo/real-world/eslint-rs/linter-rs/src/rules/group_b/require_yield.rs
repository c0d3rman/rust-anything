//! lib/rules/require-yield.js

use crate::ast::NodeId;
use crate::linter::Cx;
use crate::rule::Report;
use crate::rules::{Rule, RuleDef};
use crate::utils::get_function_head_loc;
use serde_json::Value;

pub static DEF: RuleDef = RuleDef {
    name: "require-yield",
    messages: &[(
        "missingYield",
        "This generator function does not have 'yield'.",
    )],
    has_default_options: false,
    create,
};

struct R {
    stack: Vec<u32>,
}

fn create(options: &[Value]) -> Result<Box<dyn Rule>, String> {
    super::no_options(options)?;
    Ok(Box::new(R { stack: Vec::new() }))
}

impl Rule for R {
    fn listeners(&self) -> &'static [&'static str] {
        &[
            "FunctionDeclaration",
            "FunctionDeclaration:exit",
            "FunctionExpression",
            "FunctionExpression:exit",
            "YieldExpression",
        ]
    }

    fn on(&mut self, l: usize, node: NodeId, cx: &mut Cx) {
        let ast = &cx.sc.ast;
        match l {
            0 | 2 => {
                if ast.bool(node, "generator") {
                    self.stack.push(0);
                }
            }
            1 | 3 => {
                if !ast.bool(node, "generator") {
                    return;
                }
                let count = self.stack.pop();
                let body_len = ast
                    .child(node, "body")
                    .map(|b| ast.list(b, "body").len())
                    .unwrap_or(0);
                if count == Some(0) && body_len > 0 {
                    let Some(loc) = get_function_head_loc(cx.sc, node) else {
                        return;
                    };
                    cx.report(Report::loc(loc, "missingYield"));
                }
            }
            _ => {
                if let Some(last) = self.stack.last_mut() {
                    *last += 1;
                }
            }
        }
    }
}
