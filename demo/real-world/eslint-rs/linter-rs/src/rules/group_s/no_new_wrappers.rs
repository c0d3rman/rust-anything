//! lib/rules/no-new-wrappers.js

use crate::ast::NodeId;
use crate::linter::Cx;
use crate::rule::Report;
use crate::rules::{Rule, RuleDef};
use serde_json::Value;

pub static DEF: RuleDef = RuleDef {
    name: "no-new-wrappers",
    messages: &[("noConstructor", "Do not use {{fn}} as a constructor.")],
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
        &["NewExpression"]
    }

    fn on(&mut self, _: usize, node: NodeId, cx: &mut Cx) {
        let ast = &cx.sc.ast;
        let Some(callee) = ast.child(node, "callee") else {
            return;
        };
        let Some(name) = ast
            .name(callee)
            .filter(|n| matches!(*n, "String" | "Number" | "Boolean"))
        else {
            return;
        };
        let sm = cx.sm();
        let Some(v) = sm.variable_by_name(cx.get_scope(node), name) else {
            return;
        };
        if sm.var(v).identifiers.is_empty() {
            let name = name.to_string();
            cx.report(Report::node(node, "noConstructor").data("fn", name));
        }
    }
}
