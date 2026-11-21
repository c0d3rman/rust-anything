//! lib/rules/no-shadow.js

use crate::ast::{Ast, NodeId};
use crate::linter::Cx;
use crate::rule::Report;
use crate::rules::{Rule, RuleDef};
use crate::scope::{DefType, ScopeId, ScopeManager, ScopeType, VarId};
use serde_json::Value;

pub static DEF: RuleDef = RuleDef {
    name: "no-shadow",
    messages: &[
        (
            "noShadow",
            "'{{name}}' is already declared in the upper scope on line {{shadowedLine}} column {{shadowedColumn}}.",
        ),
        ("noShadowGlobal", "'{{name}}' is already a global variable."),
    ],
    has_default_options: true,
    create,
};

struct R {
    builtin_globals: bool,
    hoist: String,
    allow: Vec<String>,
    ignore_on_initialization: bool,
}

fn create(options: &[Value]) -> Result<Box<dyn Rule>, String> {
    let mut r = R { builtin_globals: false, hoist: "functions".into(), allow: Vec::new(), ignore_on_initialization: false };
    match options {
        [] => {}
        [Value::Object(o)] => {
            for (k, v) in o {
                match (k.as_str(), v) {
                    ("builtinGlobals", Value::Bool(b)) => r.builtin_globals = *b,
                    ("ignoreOnInitialization", Value::Bool(b)) => r.ignore_on_initialization = *b,
                    ("ignoreTypeValueShadow" | "ignoreFunctionTypeParameterNameValueShadow", Value::Bool(_)) => {}
                    ("hoist", Value::String(s))
                        if matches!(s.as_str(), "all" | "functions" | "never" | "types" | "functions-and-types") =>
                    {
                        r.hoist = s.clone()
                    }
                    ("allow", Value::Array(a)) => {
                        r.allow.clear();
                        for x in a {
                            let Value::String(s) = x else { return Err("invalid option".into()) };
                            r.allow.push(s.clone());
                        }
                    }
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

fn outer_scope(sm: &ScopeManager, s: ScopeId) -> Option<ScopeId> {
    let upper = sm.scope(s).upper?;
    if sm.scope(upper).kind == ScopeType::FunctionExpressionName {
        return sm.scope(upper).upper;
    }
    Some(upper)
}

fn is_init_pattern_node(sm: &ScopeManager, ast: &Ast, v: VarId, shadowed: VarId) -> bool {
    let Some(outer_def) = sm.var(shadowed).defs.first() else { return false };
    let vs = sm.scope(sm.var(v).scope).variable_scope;
    let fun = sm.scope(vs).block;
    if !(matches!(ast.kind(fun), "ArrowFunctionExpression" | "FunctionExpression")
        && outer_scope(sm, vs) == Some(sm.var(shadowed).scope))
    {
        return false;
    }
    let mut call = ast.parent(fun);
    while let Some(c) = call {
        if ast.is(c, "CallExpression") {
            break;
        }
        call = ast.parent(c);
    }
    let Some(call) = call else { return false };
    let location = ast.range(call).1;
    let mut node = Some(outer_def.name);
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

fn unwrap_expression(ast: &Ast, e: NodeId) -> NodeId {
    let Some(p) = ast.parent(e) else { return e };
    let unwrap = ast.is(p, "LogicalExpression")
        || (ast.is(p, "ConditionalExpression") && ast.child(p, "test") != Some(e));
    if unwrap { unwrap_expression(ast, p) } else { e }
}

fn is_function_name_initializer_exception(sm: &ScopeManager, ast: &Ast, inner: VarId, outer: VarId) -> bool {
    let (Some(od), Some(id)) = (sm.var(outer).defs.first(), sm.var(inner).defs.first()) else { return false };
    if !((id.kind == DefType::FunctionName && ast.is(id.node, "FunctionExpression"))
        || (id.kind == DefType::ClassName && ast.is(id.node, "ClassExpression")))
    {
        return false;
    }
    let Some(op) = ast.parent(od.name) else { return false };
    let init = match ast.kind(op) {
        "VariableDeclarator" => ast.child(op, "init"),
        "AssignmentPattern" => ast.child(op, "right"),
        _ => None,
    };
    let Some(init) = init else { return false };
    let (is, ie) = ast.range(init);
    let (ns, ne) = ast.range(id.node);
    if !(is <= ns && ne <= ie) {
        return false;
    }
    init == unwrap_expression(ast, id.node)
}

impl R {
    fn is_in_tdz(&self, sm: &ScopeManager, ast: &Ast, v: VarId, outer: VarId) -> bool {
        let Some(outer_def) = sm.var(outer).defs.first() else { return false };
        let Some(inner_def) = sm.var(v).defs.first() else { return false };
        let inner = ast.range(inner_def.name);
        let outer_r = ast.range(outer_def.name);
        if inner.1 >= outer_r.0 {
            return false;
        }
        match self.hoist.as_str() {
            "types" => true,
            "functions-and-types" => !ast.is(outer_def.node, "FunctionDeclaration"),
            h => h != "functions" || !ast.is(outer_def.node, "FunctionDeclaration"),
        }
    }

    fn check_for_shadows(&self, cx: &mut Cx, scope: ScopeId) {
        let sm = cx.sm();
        let ast = &cx.sc.ast;
        let Some(upper) = sm.scope(scope).upper else { return };
        for &v in &sm.scope(scope).variables {
            let var = sm.var(v);
            let Some(&first_id) = var.identifiers.first() else { continue };
            let block = sm.scope(var.scope).block;
            if (ast.is(block, "ClassDeclaration") && ast.child(block, "id") == Some(first_id))
                || self.allow.contains(&var.name)
                || var.name == "this"
            {
                continue;
            }
            let Some(shadowed) = sm.variable_by_name(upper, &var.name) else { continue };
            let sv = sm.var(shadowed);
            if (!sv.identifiers.is_empty() || (self.builtin_globals && sv.writeable.is_some()))
                && !is_function_name_initializer_exception(sm, ast, v, shadowed)
                && !(self.ignore_on_initialization && is_init_pattern_node(sm, ast, v, shadowed))
                && !(self.hoist != "all" && self.is_in_tdz(sm, ast, v, shadowed))
            {
                let r = match sv.identifiers.first() {
                    Some(&sid) => {
                        let start = cx.sc.node_loc(sid).start;
                        Report::node(first_id, "noShadow")
                            .data("name", var.name.clone())
                            .data("shadowedLine", start.line)
                            .data("shadowedColumn", start.column + 1)
                    }
                    None => Report::node(first_id, "noShadowGlobal").data("name", var.name.clone()),
                };
                cx.report(r);
            }
        }
    }
}

impl Rule for R {
    fn listeners(&self) -> &'static [&'static str] {
        &["Program:exit"]
    }

    fn on(&mut self, _: usize, node: NodeId, cx: &mut Cx) {
        let sm = cx.sm();
        let global = cx.get_scope(node);
        let mut stack: Vec<ScopeId> = sm.scope(global).child_scopes.clone();
        while let Some(s) = stack.pop() {
            stack.extend(sm.scope(s).child_scopes.iter().copied());
            self.check_for_shadows(cx, s);
        }
    }
}
