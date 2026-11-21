//! lib/rules/no-alert.js

use crate::ast::NodeId;
use crate::linter::Cx;
use crate::rule::Report;
use crate::rules::{Rule, RuleDef};
use crate::scope::{RefId, ScopeId, ScopeType};
use crate::utils::{get_static_property_name, skip_chain_expression};
use serde_json::Value;

pub static DEF: RuleDef = RuleDef {
    name: "no-alert",
    messages: &[("unexpected", "Unexpected {{name}}.")],
    has_default_options: false,
    create,
};

struct R;

fn create(options: &[Value]) -> Result<Box<dyn Rule>, String> {
    super::common::no_options(options)?;
    Ok(Box::new(R))
}

fn is_prohibited(name: &str) -> bool {
    matches!(name, "alert" | "confirm" | "prompt")
}

fn find_reference(cx: &Cx, scope: ScopeId, node: NodeId) -> Option<RefId> {
    let sm = cx.sm();
    let range = cx.sc.ast.range(node);
    let found: Vec<RefId> = sm
        .scope(scope)
        .references
        .iter()
        .copied()
        .filter(|r| cx.sc.ast.range(sm.reference(*r).identifier) == range)
        .collect();
    if found.len() == 1 {
        Some(found[0])
    } else {
        None
    }
}

fn is_shadowed(cx: &Cx, scope: ScopeId, node: NodeId) -> bool {
    let sm = cx.sm();
    find_reference(cx, scope, node)
        .and_then(|r| sm.reference(r).resolved)
        .is_some_and(|v| !sm.var(v).defs.is_empty())
}

fn is_global_this_or_window(cx: &Cx, scope: ScopeId, node: NodeId) -> bool {
    let ast = &cx.sc.ast;
    let sm = cx.sm();
    if sm.scope(scope).kind == ScopeType::Global && ast.is(node, "ThisExpression") {
        return true;
    }
    if ast.is(node, "Identifier")
        && (ast.name(node) == Some("window")
            || (ast.name(node) == Some("globalThis")
                && sm.variable_by_name(scope, "globalThis").is_some()))
    {
        return !is_shadowed(cx, scope, node);
    }
    false
}

impl Rule for R {
    fn listeners(&self) -> &'static [&'static str] {
        &["CallExpression"]
    }

    fn on(&mut self, _: usize, node: NodeId, cx: &mut Cx) {
        let ast = &cx.sc.ast;
        let Some(callee) = ast
            .child(node, "callee")
            .map(|c| skip_chain_expression(ast, c))
        else {
            return;
        };
        let scope = cx.get_scope(node);
        if ast.is(callee, "Identifier") {
            let name = ast.name(callee).unwrap_or("").to_string();
            if !is_shadowed(cx, scope, callee) && is_prohibited(&name) {
                cx.report(Report::node(node, "unexpected").data("name", name));
            }
        } else if ast.is(callee, "MemberExpression")
            && let Some(obj) = ast.child(callee, "object")
            && is_global_this_or_window(cx, scope, obj)
        {
            let name = get_static_property_name(ast, callee);
            if let Some(name) = name.filter(|n| is_prohibited(n)) {
                cx.report(Report::node(node, "unexpected").data("name", name));
            }
        }
    }
}
