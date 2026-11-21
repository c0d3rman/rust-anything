//! lib/rules/no-shadow-restricted-names.js

use super::common::bool_object;
use crate::ast::NodeId;
use crate::linter::Cx;
use crate::rule::Report;
use crate::rules::{Rule, RuleDef};
use crate::scope::{ScopeManager, VarId};
use serde_json::Value;

pub static DEF: RuleDef = RuleDef {
    name: "no-shadow-restricted-names",
    messages: &[("shadowingRestrictedName", "Shadowing of global property '{{name}}'.")],
    has_default_options: true,
    create,
};

struct R {
    report_global_this: bool,
    reported: Vec<NodeId>,
}

fn create(options: &[Value]) -> Result<Box<dyn Rule>, String> {
    let o = bool_object(options, &["reportGlobalThis"])?;
    Ok(Box::new(R { report_global_this: o[0].unwrap_or(true), reported: Vec::new() }))
}

fn safely_shadows_undefined(sm: &ScopeManager, ast: &crate::ast::Ast, v: VarId) -> bool {
    let var = sm.var(v);
    var.name == "undefined"
        && var.references.iter().all(|r| !sm.reference(*r).is_write())
        && var.defs.iter().all(|d| ast.is(d.node, "VariableDeclarator") && ast.is_null(d.node, "init"))
}

impl Rule for R {
    fn listeners(&self) -> &'static [&'static str] {
        &["VariableDeclaration, :function, CatchClause, ImportDeclaration, ClassDeclaration, ClassExpression"]
    }

    fn on(&mut self, _: usize, node: NodeId, cx: &mut Cx) {
        let sm = cx.sm();
        let ast = &cx.sc.ast;
        for &v in cx.declared_variables(node) {
            let var = sm.var(v);
            let restricted = matches!(var.name.as_str(), "undefined" | "NaN" | "Infinity" | "arguments" | "eval")
                || (self.report_global_this && var.name == "globalThis");
            if var.defs.is_empty() || !restricted || safely_shadows_undefined(sm, ast, v) {
                continue;
            }
            for d in &var.defs {
                if !self.reported.contains(&d.name) {
                    self.reported.push(d.name);
                    cx.report(Report::node(d.name, "shadowingRestrictedName").data("name", var.name.clone()));
                }
            }
        }
    }
}
