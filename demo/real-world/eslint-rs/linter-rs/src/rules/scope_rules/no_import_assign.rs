//! lib/rules/no-import-assign.js

use super::common::no_options;
use crate::ast::{Ast, NodeId};
use crate::eslint_utils::{find_variable, is_specific_member_access, skip_chain_expression};
use crate::linter::Cx;
use crate::rule::Report;
use crate::rules::{Rule, RuleDef};
use crate::scope::{ScopeId, ScopeManager, ScopeType};
use serde_json::Value;

pub static DEF: RuleDef = RuleDef {
    name: "no-import-assign",
    messages: &[("readonly", "'{{name}}' is read-only."), ("readonlyMember", "The members of '{{name}}' are read-only.")],
    has_default_options: false,
    create,
};

struct R;

fn create(options: &[Value]) -> Result<Box<dyn Rule>, String> {
    no_options(options)?;
    Ok(Box::new(R))
}

fn is_assignment_left(ast: &Ast, node: NodeId) -> bool {
    let Some(parent) = ast.parent(node) else { return false };
    match ast.kind(parent) {
        "AssignmentExpression" | "AssignmentPattern" => ast.child(parent, "left") == Some(node),
        "ArrayPattern" | "RestElement" => true,
        "Property" => {
            ast.child(parent, "value") == Some(node) && ast.parent(parent).is_some_and(|g| ast.is(g, "ObjectPattern"))
        }
        _ => false,
    }
}

fn is_operand_of_mutation_unary_operator(ast: &Ast, node: NodeId) -> bool {
    let Some(p) = ast.parent(node) else { return false };
    let arg = if ast.is(p, "ChainExpression") { p } else { node };
    let Some(parent) = ast.parent(arg) else { return false };
    (ast.is(parent, "UpdateExpression") && ast.child(parent, "argument") == Some(arg))
        || (ast.is(parent, "UnaryExpression")
            && ast.str(parent, "operator") == Some("delete")
            && ast.child(parent, "argument") == Some(arg))
}

fn is_iteration_variable(ast: &Ast, node: NodeId) -> bool {
    let Some(parent) = ast.parent(node) else { return false };
    matches!(ast.kind(parent), "ForInStatement" | "ForOfStatement") && ast.child(parent, "left") == Some(node)
}

fn is_object_mutator(name: &str) -> bool {
    matches!(name, "assign" | "defineProperty" | "defineProperties" | "freeze" | "setPrototypeOf")
}

fn is_reflect_mutator(name: &str) -> bool {
    matches!(name, "defineProperty" | "deleteProperty" | "set" | "setPrototypeOf")
}

fn is_argument_of_well_known_mutation_function(sm: &ScopeManager, ast: &Ast, node: NodeId, scope: ScopeId) -> bool {
    let Some(parent) = ast.parent(node) else { return false };
    if !ast.is(parent, "CallExpression") || ast.list(parent, "arguments").first().copied().flatten() != Some(node) {
        return false;
    }
    let Some(callee) = ast.child(parent, "callee") else { return false };
    let callee = skip_chain_expression(ast, callee);
    let object_ok = is_specific_member_access(ast, callee, Some(&|n| n == "Object"), Some(&is_object_mutator));
    let reflect_ok = is_specific_member_access(ast, callee, Some(&|n| n == "Reflect"), Some(&is_reflect_mutator));
    if !object_ok && !reflect_ok {
        return false;
    }
    let Some(object) = ast.child(callee, "object") else { return false };
    find_variable(sm, ast, scope, object).is_some_and(|v| sm.scope(sm.var(v).scope).kind == ScopeType::Global)
}

fn is_member_write(sm: &ScopeManager, ast: &Ast, id: NodeId, scope: ScopeId) -> bool {
    let member = ast.parent(id).filter(|p| ast.is(*p, "MemberExpression") && ast.child(*p, "object") == Some(id));
    if let Some(p) = member
        && (is_assignment_left(ast, p) || is_operand_of_mutation_unary_operator(ast, p) || is_iteration_variable(ast, p))
    {
        return true;
    }
    is_argument_of_well_known_mutation_function(sm, ast, id, scope)
}

fn get_write_node(ast: &Ast, id: NodeId) -> NodeId {
    let mut node = ast.parent(id);
    while let Some(n) = node {
        if matches!(
            ast.kind(n),
            "AssignmentExpression"
                | "UpdateExpression"
                | "UnaryExpression"
                | "CallExpression"
                | "ForInStatement"
                | "ForOfStatement"
        ) {
            return n;
        }
        node = ast.parent(n);
    }
    id
}

impl Rule for R {
    fn listeners(&self) -> &'static [&'static str] {
        &["ImportDeclaration"]
    }

    fn on(&mut self, _: usize, node: NodeId, cx: &mut Cx) {
        let sm = cx.sm();
        let ast = &cx.sc.ast;
        let scope = cx.get_scope(node);
        for &v in cx.declared_variables(node) {
            let var = sm.var(v);
            let check_members = var.defs.iter().any(|d| ast.is(d.node, "ImportNamespaceSpecifier"));
            let mut prev = None;
            for &r in &var.references {
                let rf = sm.reference(r);
                let id = rf.identifier;
                if Some(id) == prev {
                    continue;
                }
                prev = Some(id);
                let name = ast.name(id).unwrap_or("").to_string();
                if rf.is_write() {
                    cx.report(Report::node(get_write_node(ast, id), "readonly").data("name", name));
                } else if check_members && is_member_write(sm, ast, id, scope) {
                    cx.report(Report::node(get_write_node(ast, id), "readonlyMember").data("name", name));
                }
            }
        }
    }
}
