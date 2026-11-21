//! lib/rules/no-undef.js

use super::common::bool_object;
use crate::ast::NodeId;
use crate::linter::Cx;
use crate::rule::Report;
use crate::rules::{Rule, RuleDef};
use serde_json::Value;

pub static DEF: RuleDef = RuleDef {
    name: "no-undef",
    messages: &[("undef", "'{{name}}' is not defined.")],
    has_default_options: true,
    create,
};

struct R {
    consider_typeof: bool,
}

fn create(options: &[Value]) -> Result<Box<dyn Rule>, String> {
    let o = bool_object(options, &["typeof"])?;
    Ok(Box::new(R { consider_typeof: o[0].unwrap_or(false) }))
}

impl Rule for R {
    fn listeners(&self) -> &'static [&'static str] {
        &["Program:exit"]
    }

    fn on(&mut self, _: usize, node: NodeId, cx: &mut Cx) {
        let sm = cx.sm();
        let ast = &cx.sc.ast;
        let global = cx.get_scope(node);
        for &r in &sm.scope(global).through {
            let id = sm.reference(r).identifier;
            if !self.consider_typeof
                && let Some(p) = ast.parent(id)
                && ast.is(p, "UnaryExpression")
                && ast.str(p, "operator") == Some("typeof")
            {
                continue;
            }
            let name = ast.name(id).unwrap_or("").to_string();
            cx.report(Report::node(id, "undef").data("name", name));
        }
    }
}
