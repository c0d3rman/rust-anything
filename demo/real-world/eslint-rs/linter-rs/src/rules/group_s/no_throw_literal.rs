//! lib/rules/no-throw-literal.js

use crate::ast::NodeId;
use crate::linter::Cx;
use crate::rule::Report;
use crate::rules::{Rule, RuleDef};
use crate::utils::could_be_error;
use serde_json::Value;

pub static DEF: RuleDef = RuleDef {
    name: "no-throw-literal",
    messages: &[
        ("object", "Expected an error object to be thrown."),
        ("undef", "Do not throw undefined."),
    ],
    has_default_options: false,
    create,
};

struct R;

fn create(options: &[Value]) -> Result<Box<dyn Rule>, String> {
    super::common::no_options(options)?;
    Ok(Box::new(R))
}

impl Rule for R {
    fn listeners(&self) -> &'static [&'static str] {
        &["ThrowStatement"]
    }

    fn on(&mut self, _: usize, node: NodeId, cx: &mut Cx) {
        let ast = &cx.sc.ast;
        let Some(arg) = ast.child(node, "argument") else {
            return;
        };
        if !could_be_error(ast, arg) {
            cx.report(Report::node(node, "object"));
        } else if ast.is(arg, "Identifier")
            && ast.name(arg) == Some("undefined")
            && cx.sm().is_global_reference(ast, arg)
        {
            cx.report(Report::node(node, "undef"));
        }
    }
}
