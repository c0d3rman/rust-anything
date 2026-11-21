//! lib/rules/prefer-const.js

use crate::ast::{Ast, NodeId};
use crate::linter::Cx;
use crate::rule::{fixer, Report};
use crate::rules::{Rule, RuleDef};
use crate::scope::{DefType, RefId, ScopeId, ScopeManager, ScopeType, VarId};
use serde_json::Value;

pub static DEF: RuleDef = RuleDef {
    name: "prefer-const",
    messages: &[("useConst", "'{{name}}' is never reassigned. Use 'const' instead.")],
    has_default_options: true,
    create,
};

struct R {
    match_any: bool,
    ignore_read_before_assign: bool,
    variables: Vec<VarId>,
    report_count: usize,
    checked_id: Option<NodeId>,
    checked_name: Option<String>,
}

fn create(options: &[Value]) -> Result<Box<dyn Rule>, String> {
    let mut destructuring = "any".to_string();
    let mut ignore = false;
    match options {
        [] => {}
        [Value::Object(o)] => {
            for (k, v) in o {
                match (k.as_str(), v) {
                    ("destructuring", Value::String(s)) if s == "any" || s == "all" => destructuring = s.clone(),
                    ("ignoreReadBeforeAssign", Value::Bool(b)) => ignore = *b,
                    _ => return Err("invalid option".into()),
                }
            }
        }
        _ => return Err("invalid option".into()),
    }
    Ok(Box::new(R {
        match_any: destructuring != "all",
        ignore_read_before_assign: ignore,
        variables: Vec::new(),
        report_count: 0,
        checked_id: None,
        checked_name: Some(String::new()),
    }))
}

fn is_pattern_type(kind: &str) -> bool {
    (kind.len() > "Pattern".len() && kind.ends_with("Pattern"))
        || matches!(kind, "RestElement" | "SpreadProperty" | "ExperimentalRestProperty" | "Property")
}

fn skip_patterns(ast: &Ast, id: NodeId) -> Option<NodeId> {
    let mut node = ast.parent(id)?;
    while is_pattern_type(ast.kind(node)) {
        node = ast.parent(node)?;
    }
    Some(node)
}

fn can_become_variable_declaration(ast: &Ast, id: NodeId) -> bool {
    let Some(node) = skip_patterns(ast, id) else { return false };
    if ast.is(node, "VariableDeclarator") {
        return true;
    }
    ast.is(node, "AssignmentExpression")
        && ast.parent(node).is_some_and(|p| {
            ast.is(p, "ExpressionStatement")
                && ast
                    .parent(p)
                    .is_some_and(|g| matches!(ast.kind(g), "Program" | "BlockStatement" | "StaticBlock" | "SwitchCase"))
        })
}

fn is_outer_variable_in_destructing(sm: &ScopeManager, name: Option<&str>, scope: ScopeId) -> bool {
    let Some(name) = name else { return false };
    if sm.scope(scope).through.iter().any(|r| sm.reference(*r).resolved.is_some_and(|v| sm.var(v).name == name)) {
        return true;
    }
    match sm.variable_by_name(scope, name) {
        Some(v) => sm.var(v).defs.iter().any(|d| d.kind == DefType::Parameter),
        None => false,
    }
}

fn destructuring_host(sm: &ScopeManager, ast: &Ast, r: RefId) -> Option<NodeId> {
    let rf = sm.reference(r);
    if !rf.is_write() {
        return None;
    }
    let node = skip_patterns(ast, rf.identifier)?;
    matches!(ast.kind(node), "VariableDeclarator" | "AssignmentExpression").then_some(node)
}

fn has_member_expression_assignment(ast: &Ast, node: NodeId) -> bool {
    match ast.kind(node) {
        "ObjectPattern" => ast.nodes_of(node, "properties").any(|p| {
            match ast.child(p, "argument").or_else(|| ast.child(p, "value")) {
                Some(x) => has_member_expression_assignment(ast, x),
                None => false,
            }
        }),
        "ArrayPattern" => ast.nodes_of(node, "elements").any(|e| has_member_expression_assignment(ast, e)),
        "AssignmentPattern" => ast.child(node, "left").is_some_and(|l| has_member_expression_assignment(ast, l)),
        "MemberExpression" => true,
        _ => false,
    }
}

fn identifier_if_should_be_const(sm: &ScopeManager, ast: &Ast, v: VarId, ignore_read_before_assign: bool) -> Option<NodeId> {
    let var = sm.var(v);
    if var.eslint_used.get() && sm.scope(var.scope).kind == ScopeType::Global {
        return None;
    }
    let mut writer: Option<RefId> = None;
    let mut read_before_init = false;
    for &r in &var.references {
        let rf = sm.reference(r);
        if rf.is_write() {
            if let Some(w) = writer
                && sm.reference(w).identifier != rf.identifier
            {
                return None;
            }
            if let Some(host) = destructuring_host(sm, ast, r)
                && let Some(left) = ast.child(host, "left")
            {
                let (outer, non_ids) = match ast.kind(left) {
                    "ObjectPattern" => (
                        ast.nodes_of(left, "properties")
                            .filter_map(|p| ast.child(p, "value"))
                            .any(|val| is_outer_variable_in_destructing(sm, ast.name(val), var.scope)),
                        has_member_expression_assignment(ast, left),
                    ),
                    "ArrayPattern" => (
                        ast.list(left, "elements")
                            .iter()
                            .any(|e| is_outer_variable_in_destructing(sm, e.and_then(|e| ast.name(e)), var.scope)),
                        has_member_expression_assignment(ast, left),
                    ),
                    _ => (false, false),
                };
                if outer || non_ids {
                    return None;
                }
            }
            writer = Some(r);
        } else if rf.is_read() && writer.is_none() {
            if ignore_read_before_assign {
                return None;
            }
            read_before_init = true;
        }
    }
    let w = writer?;
    let wr = sm.reference(w);
    if wr.from != var.scope || !can_become_variable_declaration(ast, wr.identifier) {
        return None;
    }
    if read_before_init {
        return var.defs.first().map(|d| d.name);
    }
    Some(wr.identifier)
}

impl R {
    fn check_group(&mut self, cx: &mut Cx, nodes: &[Option<NodeId>]) {
        let ast = &cx.sc.ast;
        let to_report: Vec<NodeId> = nodes.iter().flatten().copied().collect();
        if nodes.is_empty() || !(self.match_any || to_report.len() == nodes.len()) {
            return;
        }
        let mut decl = None;
        let mut cur = nodes[0];
        while let Some(n) = cur {
            if ast.kind(n).ends_with("Statement") {
                break;
            }
            if ast.is(n, "VariableDeclaration") {
                decl = Some(n);
                break;
            }
            cur = ast.parent(n);
        }
        if let Some(d) = decl
            && let Some(first) = ast.nodes_of(d, "declarations").next()
            && let Some(init) = ast.child(first, "init")
            && let Some(fdp) = ast.parent(init)
            && ast.is(fdp, "VariableDeclarator")
        {
            let id = ast.child(fdp, "id");
            let id_name = id.and_then(|i| ast.name(i)).map(str::to_string);
            if id_name != self.checked_name {
                self.checked_name = id_name;
                self.report_count = 0;
            }
            if id.is_some_and(|i| ast.is(i, "ObjectPattern")) {
                let init_name = ast.child(fdp, "init").and_then(|i| ast.name(i)).map(str::to_string);
                if init_name != self.checked_name {
                    self.checked_name = init_name;
                    self.report_count = 0;
                }
            }
            if id != self.checked_id {
                self.checked_id = id;
                self.report_count = 0;
            }
        }
        let mut should_fix = decl.is_some_and(|d| {
            ast.parent(d).is_some_and(|p| matches!(ast.kind(p), "ForInStatement" | "ForOfStatement"))
                || ast.nodes_of(d, "declarations").all(|x| ast.child(x, "init").is_some())
        }) && to_report.len() == nodes.len();
        if let Some(d) = decl {
            let count = ast.list(d, "declarations").len();
            if count > 1 {
                self.report_count += to_report.len();
                let mut total = 0;
                for x in ast.nodes_of(d, "declarations") {
                    total += match ast.child(x, "id") {
                        Some(i) if ast.is(i, "ObjectPattern") => ast.list(i, "properties").len(),
                        Some(i) if ast.is(i, "ArrayPattern") => ast.list(i, "elements").len(),
                        _ => 1,
                    };
                }
                should_fix = should_fix && self.report_count == total;
            }
        }
        for n in to_report {
            let name = ast.name(n).unwrap_or("").to_string();
            let mut r = Report::node(n, "useConst").data("name", name);
            if should_fix
                && let Some(d) = decl
            {
                let kind = ast.str(d, "kind").unwrap_or("").to_string();
                if let Some(tok) = cx.sc.first_token_f(d, |t| t.value == kind) {
                    let (ds, de) = ast.range(d);
                    let s = ds.min(tok.start);
                    let e = de.max(tok.end);
                    let text = format!("{}const{}", cx.sc.slice16(s, tok.start), cx.sc.slice16(tok.end, e));
                    r = r.fix(fixer::replace_range((s, e), text));
                }
            }
            cx.report(r);
        }
    }
}

impl Rule for R {
    fn listeners(&self) -> &'static [&'static str] {
        &["Program:exit", "VariableDeclaration"]
    }

    fn on(&mut self, listener: usize, node: NodeId, cx: &mut Cx) {
        let ast = &cx.sc.ast;
        if listener == 1 {
            let for_init = ast.parent(node).is_some_and(|p| ast.is(p, "ForStatement") && ast.child(p, "init") == Some(node));
            if ast.str(node, "kind") == Some("let") && !for_init {
                self.variables.extend_from_slice(cx.declared_variables(node));
            }
            return;
        }
        let sm = cx.sm();
        let mut groups: Vec<(NodeId, Vec<Option<NodeId>>)> = Vec::new();
        for &v in &self.variables {
            let ident = identifier_if_should_be_const(sm, ast, v, self.ignore_read_before_assign);
            let mut prev = None;
            for &r in &sm.var(v).references {
                let id = sm.reference(r).identifier;
                if Some(id) == prev {
                    continue;
                }
                prev = Some(id);
                if let Some(g) = destructuring_host(sm, ast, r) {
                    match groups.iter_mut().find(|(k, _)| *k == g) {
                        Some(slot) => slot.1.push(ident),
                        None => groups.push((g, vec![ident])),
                    }
                }
            }
        }
        for (_, nodes) in groups {
            self.check_group(cx, &nodes);
        }
    }
}
