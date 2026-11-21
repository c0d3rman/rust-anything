//! lib/rules/no-func-assign.js

use super::common::{modifying_references, no_options};
use crate::ast::NodeId;
use crate::linter::Cx;
use crate::rule::Report;
use crate::rules::{Rule, RuleDef};
use serde_json::Value;

pub static DEF: RuleDef = RuleDef {
    name: "no-func-assign",
    messages: &[("isAFunction", "'{{name}}' is a function.")],
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
        &["FunctionDeclaration", "FunctionExpression"]
    }

    fn on(&mut self, _: usize, node: NodeId, cx: &mut Cx) {
        let sm = cx.sm();
        let ast = &cx.sc.ast;
        for &v in cx.declared_variables(node) {
            let var = sm.var(v);
            if var.defs.first().map(|d| d.kind) != Some(crate::scope::DefType::FunctionName) {
                continue;
            }
            for r in modifying_references(sm, &var.references) {
                let id = sm.reference(r).identifier;
                let name = ast.name(id).unwrap_or("").to_string();
                cx.report(Report::node(id, "isAFunction").data("name", name));
            }
        }
    }
}
