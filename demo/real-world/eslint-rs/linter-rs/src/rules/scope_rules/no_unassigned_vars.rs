//! lib/rules/no-unassigned-vars.js

use super::common::no_options;
use crate::ast::NodeId;
use crate::linter::Cx;
use crate::rule::Report;
use crate::rules::{Rule, RuleDef};
use serde_json::Value;

pub static DEF: RuleDef = RuleDef {
    name: "no-unassigned-vars",
    messages: &[("unassigned", "'{{name}}' is always 'undefined' because it's never assigned.")],
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
        &["TSModuleDeclaration[declare=true]", "TSModuleDeclaration[declare=true]:exit", "VariableDeclarator"]
    }

    fn on(&mut self, listener: usize, node: NodeId, cx: &mut Cx) {
        if listener != 2 {
            return;
        }
        let sm = cx.sm();
        let ast = &cx.sc.ast;
        let Some(decl) = ast.parent(node) else { return };
        let Some(id) = ast.child(node, "id") else { return };
        if ast.child(node, "init").is_some()
            || !ast.is(id, "Identifier")
            || ast.str(decl, "kind") == Some("const")
            || ast.bool(decl, "declare")
        {
            return;
        }
        let Some(&v) = cx.declared_variables(node).first() else { return };
        let mut has_read = false;
        for &r in &sm.var(v).references {
            let rf = sm.reference(r);
            if rf.is_write() {
                return;
            }
            if rf.is_read() {
                has_read = true;
            }
        }
        if !has_read {
            return;
        }
        let name = ast.name(id).unwrap_or("").to_string();
        cx.report(Report::node(node, "unassigned").data("name", name));
    }
}
