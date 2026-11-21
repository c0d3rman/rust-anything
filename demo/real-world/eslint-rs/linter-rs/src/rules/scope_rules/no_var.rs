//! lib/rules/no-var.js

use super::common::no_options;
use crate::ast::{Ast, NodeId};
use crate::eslint_utils::is_in_loop;
use crate::linter::Cx;
use crate::rule::{fixer, Report};
use crate::rules::{Rule, RuleDef};
use crate::scope::{ScopeId, ScopeManager, ScopeType, VarId};
use crate::utils::is_function;
use serde_json::Value;

pub static DEF: RuleDef = RuleDef {
    name: "no-var",
    messages: &[("unexpectedVar", "Unexpected var, use let or const instead.")],
    has_default_options: false,
    create,
};

struct R;

fn create(options: &[Value]) -> Result<Box<dyn Rule>, String> {
    no_options(options)?;
    Ok(Box::new(R))
}

fn enclosing_function_scope(sm: &ScopeManager, scope: ScopeId) -> ScopeId {
    let mut cur = scope;
    while !matches!(sm.scope(cur).kind, ScopeType::Function | ScopeType::Global) {
        match sm.scope(cur).upper {
            Some(u) => cur = u,
            None => break,
        }
    }
    cur
}

fn is_referenced_in_closure(sm: &ScopeManager, v: VarId) -> bool {
    let var = sm.var(v);
    let enclosing = enclosing_function_scope(sm, var.scope);
    var.references.iter().any(|r| enclosing_function_scope(sm, sm.reference(*r).from) != enclosing)
}

fn is_loop_assignee(ast: &Ast, node: NodeId) -> bool {
    ast.parent(node).is_some_and(|p| {
        matches!(ast.kind(p), "ForOfStatement" | "ForInStatement") && ast.child(p, "left") == Some(node)
    })
}

fn is_declaration_initialized(ast: &Ast, node: NodeId) -> bool {
    ast.nodes_of(node, "declarations").all(|d| ast.child(d, "init").is_some())
}

fn get_scope_node(ast: &Ast, node: NodeId) -> Option<NodeId> {
    let mut cur = Some(node);
    while let Some(n) = cur {
        if matches!(
            ast.kind(n),
            "Program" | "BlockStatement" | "SwitchStatement" | "ForStatement" | "ForInStatement" | "ForOfStatement"
        ) {
            return Some(n);
        }
        cur = ast.parent(n);
    }
    None
}

fn has_reference_in_tdz(sm: &ScopeManager, ast: &Ast, init: NodeId, v: VarId) -> bool {
    let (init_start, init_end) = ast.range(init);
    let var = sm.var(v);
    let Some(d) = var.defs.first() else { return false };
    let id = d.name;
    let id_start = ast.range(id).0;
    let default = ast
        .parent(id)
        .filter(|p| ast.is(*p, "AssignmentPattern"))
        .and_then(|p| ast.child(p, "right"))
        .map(|r| ast.range(r));
    var.references.iter().any(|r| {
        let rf = sm.reference(*r);
        let (start, end) = ast.range(rf.identifier);
        rf.init != Some(true)
            && (start < id_start
                || default.is_some_and(|(ds, de)| start >= ds && end <= de)
                || (!is_function(ast, init) && start >= init_start && end <= init_end))
    })
}

fn has_reference_before_declaration(sm: &ScopeManager, ast: &Ast, v: VarId) -> bool {
    let var = sm.var(v);
    let Some(d) = var.defs.first() else { return false };
    let decl_start = ast.range(d.node).0;
    var.references.iter().any(|r| {
        let rf = sm.reference(*r);
        rf.init != Some(true) && ast.range(rf.identifier).0 < decl_start
    })
}

fn has_unsafe_hoisted_function_reference(sm: &ScopeManager, ast: &Ast, v: VarId) -> bool {
    let var = sm.var(v);
    let Some(d) = var.defs.first() else { return false };
    let decl_start = ast.range(d.node).0;
    for &r in &var.references {
        let rf = sm.reference(r);
        if rf.init == Some(true) || ast.range(rf.identifier).0 < decl_start {
            continue;
        }
        let mut cur = sm.scope(rf.from).variable_scope;
        while cur != var.scope {
            let sc = sm.scope(cur);
            let Some(upper) = sc.upper else { break };
            if ast.is(sc.block, "FunctionDeclaration")
                && let Some(fid) = ast.child(sc.block, "id")
                && let Some(fv) = sm.lookup(upper, ast.name(fid).unwrap_or(""))
            {
                for &fr in &sm.var(fv).references {
                    let ref_id = sm.reference(fr).identifier;
                    if let Some(p) = ast.parent(ref_id)
                        && ast.is(p, "CallExpression")
                        && ast.child(p, "callee") == Some(ref_id)
                        && ast.range(ref_id).0 < decl_start
                    {
                        return true;
                    }
                }
            }
            cur = sm.scope(upper).variable_scope;
        }
    }
    false
}

fn can_fix(cx: &Cx, node: NodeId) -> bool {
    let sm = cx.sm();
    let ast = &cx.sc.ast;
    let variables = cx.declared_variables(node);
    let Some(scope_node) = get_scope_node(ast, node) else { return false };
    let (ss, se) = ast.range(scope_node);
    let Some(parent) = ast.parent(node) else { return false };
    let scope = cx.get_scope(node);
    let self_tdz = ast.nodes_of(node, "declarations").any(|d| match ast.child(d, "init") {
        Some(init) => cx.declared_variables(d).iter().any(|v| has_reference_in_tdz(sm, ast, init, *v)),
        None => false,
    });
    if ast.is(parent, "SwitchCase")
        || self_tdz
        || variables.iter().any(|v| sm.scope(sm.var(*v).scope).kind == ScopeType::Global)
        || variables.iter().any(|v| sm.var(*v).defs.len() >= 2)
        || variables.iter().any(|v| {
            sm.var(*v).references.iter().any(|r| {
                let (s, e) = ast.range(sm.reference(*r).identifier);
                s < ss || e > se
            })
        })
        || variables.iter().any(|v| sm.var(*v).name == "let")
        || variables.iter().any(|v| has_reference_before_declaration(sm, ast, *v))
        || variables.iter().any(|v| has_unsafe_hoisted_function_reference(sm, ast, *v))
        || variables.iter().any(|v| sm.variable_by_name(scope, &sm.var(*v).name) != Some(*v))
    {
        return false;
    }
    if is_in_loop(ast, node) {
        if variables.iter().any(|v| is_referenced_in_closure(sm, *v)) {
            return false;
        }
        if !is_loop_assignee(ast, node) && !is_declaration_initialized(ast, node) {
            return false;
        }
    }
    !(!is_loop_assignee(ast, node)
        && !(ast.is(parent, "ForStatement") && ast.child(parent, "init") == Some(node)) && !matches!(ast.kind(parent), "Program" | "BlockStatement" | "StaticBlock" | "SwitchCase" | "TSModuleBlock"))
}

impl Rule for R {
    fn listeners(&self) -> &'static [&'static str] {
        &["VariableDeclaration:exit"]
    }

    fn on(&mut self, _: usize, node: NodeId, cx: &mut Cx) {
        let ast = &cx.sc.ast;
        if ast.str(node, "kind") != Some("var") {
            return;
        }
        let mut r = Report::node(node, "unexpectedVar");
        if let Some(var_tok) = cx.sc.first_token_f(node, |t| t.value == "var")
            && can_fix(cx, node)
        {
            let (ns, ne) = ast.range(node);
            let text = format!("{}let{}", cx.sc.slice16(ns, var_tok.start), cx.sc.slice16(var_tok.end, ne));
            r = r.fix(fixer::replace_range((ns.min(var_tok.start), ne.max(var_tok.end)), text));
        }
        cx.report(r);
    }
}
