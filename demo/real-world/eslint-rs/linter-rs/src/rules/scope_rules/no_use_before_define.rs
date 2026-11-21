//! lib/rules/no-use-before-define.js

use crate::ast::{Ast, NodeId};
use crate::linter::Cx;
use crate::rule::Report;
use crate::rules::{Rule, RuleDef};
use crate::scope::{DefType, RefId, ScopeId, ScopeManager, ScopeType};
use serde_json::Value;

pub static DEF: RuleDef = RuleDef {
    name: "no-use-before-define",
    messages: &[("usedBeforeDefined", "'{{name}}' was used before it was defined.")],
    has_default_options: true,
    create,
};

struct R {
    functions: bool,
    classes: bool,
    variables: bool,
    allow_named_exports: bool,
}

fn create(options: &[Value]) -> Result<Box<dyn Rule>, String> {
    let mut r = R { functions: true, classes: true, variables: true, allow_named_exports: false };
    match options {
        [] => {}
        [Value::String(s)] if s == "nofunc" => r.functions = false,
        [Value::Object(o)] => {
            for (k, v) in o {
                let Value::Bool(b) = v else { return Err("invalid option".into()) };
                match k.as_str() {
                    "functions" => r.functions = *b,
                    "classes" => r.classes = *b,
                    "variables" => r.variables = *b,
                    "allowNamedExports" => r.allow_named_exports = *b,
                    "enums" | "typedefs" | "ignoreTypeReferences" => {}
                    _ => return Err("invalid option".into()),
                }
            }
        }
        _ => return Err("invalid option".into()),
    }
    Ok(Box::new(r))
}

fn is_in_range(ast: &Ast, node: Option<NodeId>, location: u32) -> bool {
    node.is_some_and(|n| {
        let (s, e) = ast.range(n);
        s <= location && location <= e
    })
}

fn is_in_class_static_initializer_range(ast: &Ast, body: NodeId, location: u32) -> bool {
    ast.nodes_of(body, "body").any(|m| {
        (ast.is(m, "StaticBlock") && is_in_range(ast, Some(m), location))
            || (ast.is(m, "PropertyDefinition")
                && ast.bool(m, "static")
                && is_in_range(ast, ast.child(m, "value"), location))
    })
}

fn is_class_static_initializer_scope(sm: &ScopeManager, ast: &Ast, s: ScopeId) -> bool {
    let sc = sm.scope(s);
    match sc.kind {
        ScopeType::ClassStaticBlock => true,
        ScopeType::ClassFieldInitializer => ast.parent(sc.block).is_some_and(|p| ast.bool(p, "static")),
        _ => false,
    }
}

fn is_iife_scope(sm: &ScopeManager, ast: &Ast, s: ScopeId) -> bool {
    let sc = sm.scope(s);
    if sc.kind != ScopeType::Function {
        return false;
    }
    let node = sc.block;
    let Some(p) = ast.parent(node) else { return false };
    if !ast.is(p, "CallExpression") || ast.child(p, "callee") != Some(node) {
        return false;
    }
    !ast.bool(node, "generator") && !ast.bool(node, "async")
}

fn is_from_separate_execution_context(sm: &ScopeManager, ast: &Ast, r: RefId) -> bool {
    let rf = sm.reference(r);
    let Some(v) = rf.resolved else { return false };
    let target = sm.scope(sm.var(v).scope).variable_scope;
    let mut scope = rf.from;
    while target != sm.scope(scope).variable_scope {
        let vs = sm.scope(scope).variable_scope;
        if is_class_static_initializer_scope(sm, ast, vs) || is_iife_scope(sm, ast, vs) {
            match sm.scope(vs).upper {
                Some(u) => scope = u,
                None => return false,
            }
        } else {
            return true;
        }
    }
    false
}

fn is_evaluated_during_initialization(sm: &ScopeManager, ast: &Ast, r: RefId) -> bool {
    if is_from_separate_execution_context(sm, ast, r) {
        return false;
    }
    let rf = sm.reference(r);
    let location = ast.range(rf.identifier).1;
    let Some(v) = rf.resolved else { return false };
    let Some(d) = sm.var(v).defs.first() else { return false };
    if d.kind == DefType::ClassName {
        let class = d.node;
        return is_in_range(ast, Some(class), location)
            && !ast.child(class, "body").is_some_and(|b| is_in_class_static_initializer_range(ast, b, location));
    }
    let mut node = ast.parent(d.name);
    while let Some(n) = node {
        match ast.kind(n) {
            "VariableDeclarator" => {
                if is_in_range(ast, ast.child(n, "init"), location) {
                    return true;
                }
                if let Some(gp) = ast.parent(n).and_then(|p| ast.parent(p))
                    && matches!(ast.kind(gp), "ForInStatement" | "ForOfStatement")
                    && is_in_range(ast, ast.child(gp, "right"), location)
                {
                    return true;
                }
                break;
            }
            "AssignmentPattern" => {
                if is_in_range(ast, ast.child(n, "right"), location) {
                    return true;
                }
            }
            "FunctionDeclaration" | "FunctionExpression" | "ClassDeclaration" | "ClassExpression"
            | "ArrowFunctionExpression" | "CatchClause" | "ImportDeclaration" | "ExportNamedDeclaration" => break,
            _ => {}
        }
        node = ast.parent(n);
    }
    false
}

impl R {
    fn should_check(&self, sm: &ScopeManager, ast: &Ast, r: RefId) -> bool {
        let rf = sm.reference(r);
        if rf.init == Some(true) {
            return false;
        }
        let id = rf.identifier;
        let parent = ast.parent(id);
        if self.allow_named_exports
            && let Some(p) = parent
            && ast.is(p, "ExportSpecifier")
            && ast.child(p, "local") == Some(id)
        {
            return false;
        }
        let Some(v) = rf.resolved else { return false };
        let Some(d) = sm.var(v).defs.first() else { return false };
        if !self.functions && d.kind == DefType::FunctionName {
            return false;
        }
        if ((!self.variables && d.kind == DefType::Variable) || (!self.classes && d.kind == DefType::ClassName))
            && is_from_separate_execution_context(sm, ast, r)
        {
            return false;
        }
        true
    }

    fn check_scope(&self, cx: &mut Cx, scope: ScopeId) {
        let sm = cx.sm();
        let ast = &cx.sc.ast;
        for &r in &sm.scope(scope).references {
            if !self.should_check(sm, ast, r) {
                continue;
            }
            let rf = sm.reference(r);
            let Some(v) = rf.resolved else { continue };
            let Some(d) = sm.var(v).defs.first() else { continue };
            if ast.range(rf.identifier).1 < ast.range(d.name).1 || is_evaluated_during_initialization(sm, ast, r) {
                let name = ast.name(rf.identifier).unwrap_or("").to_string();
                cx.report(Report::node(rf.identifier, "usedBeforeDefined").data("name", name));
            }
        }
        for &c in &sm.scope(scope).child_scopes {
            self.check_scope(cx, c);
        }
    }
}

impl Rule for R {
    fn listeners(&self) -> &'static [&'static str] {
        &["Program"]
    }

    fn on(&mut self, _: usize, node: NodeId, cx: &mut Cx) {
        let s = cx.get_scope(node);
        self.check_scope(cx, s);
    }
}
