//! lib/rules/no-const-assign.js

use super::common::{modifying_references, no_options};
use crate::ast::NodeId;
use crate::linter::Cx;
use crate::rule::Report;
use crate::rules::{Rule, RuleDef};
use serde_json::Value;

pub static DEF: RuleDef = RuleDef {
    name: "no-const-assign",
    messages: &[("const", "'{{name}}' is constant.")],
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
        &["VariableDeclaration"]
    }

    fn on(&mut self, _: usize, node: NodeId, cx: &mut Cx) {
        let sm = cx.sm();
        let ast = &cx.sc.ast;
        if !matches!(ast.str(node, "kind"), Some("const" | "using" | "await using")) {
            return;
        }
        for &v in cx.declared_variables(node) {
            let var = sm.var(v);
            for r in modifying_references(sm, &var.references) {
                let id = sm.reference(r).identifier;
                let name = ast.name(id).unwrap_or("").to_string();
                cx.report(Report::node(id, "const").data("name", name));
            }
        }
    }
}
