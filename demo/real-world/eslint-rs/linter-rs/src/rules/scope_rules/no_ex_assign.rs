//! lib/rules/no-ex-assign.js

use super::common::{modifying_references, no_options};
use crate::ast::NodeId;
use crate::linter::Cx;
use crate::rule::Report;
use crate::rules::{Rule, RuleDef};
use serde_json::Value;

pub static DEF: RuleDef = RuleDef {
    name: "no-ex-assign",
    messages: &[("unexpected", "Do not assign to the exception parameter.")],
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
        &["CatchClause"]
    }

    fn on(&mut self, _: usize, node: NodeId, cx: &mut Cx) {
        let sm = cx.sm();
        for &v in cx.declared_variables(node) {
            let var = sm.var(v);
            for r in modifying_references(sm, &var.references) {
                let id = sm.reference(r).identifier;
                cx.report(Report::node(id, "unexpected"));
            }
        }
    }
}
