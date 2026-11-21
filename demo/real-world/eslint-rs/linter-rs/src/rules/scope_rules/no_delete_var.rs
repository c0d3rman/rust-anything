//! lib/rules/no-delete-var.js

use super::common::no_options;
use crate::ast::NodeId;
use crate::linter::Cx;
use crate::rule::Report;
use crate::rules::{Rule, RuleDef};
use serde_json::Value;

pub static DEF: RuleDef = RuleDef {
    name: "no-delete-var",
    messages: &[("unexpected", "Variables should not be deleted.")],
    has_default_options: false,
    create,
};

struct R;

fn create(options: &[Value]) -> Result<Box<dyn Rule>, String> {
    no_options(options)?;
    Ok(Box::new(R))
}

impl Rule for R {
    fn listeners(&self) -> &'static [&'static str] {
        &["UnaryExpression"]
    }

    fn on(&mut self, _: usize, node: NodeId, cx: &mut Cx) {
        let ast = &cx.sc.ast;
        if ast.str(node, "operator") == Some("delete") && ast.child(node, "argument").is_some_and(|a| ast.is(a, "Identifier")) {
            cx.report(Report::node(node, "unexpected"));
        }
    }
}
