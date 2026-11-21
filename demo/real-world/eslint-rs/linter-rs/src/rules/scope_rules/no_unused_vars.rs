//! lib/rules/no-unused-vars.js

use super::common::{simple_pattern, Pat};
use crate::ast::{Ast, NodeId};
use crate::eslint_utils::is_in_loop;
use crate::linter::Cx;
use crate::report::SugVal;
use crate::rule::{fixer, Report, SuggestSpec};
use crate::rules::{Rule, RuleDef};
use crate::scope::{DefType, RefId, ScopeId, ScopeManager, ScopeType, VarId};
use crate::source::{SourceCode, TokKind, Token};
use crate::tokens::{Opts, N};
use crate::utils::is_function;
use serde_json::Value;

pub static DEF: RuleDef = RuleDef {
    name: "no-unused-vars",
    messages: &[
        ("unusedVar", "'{{varName}}' is {{action}} but never used{{additional}}."),
        ("usedIgnoredVar", "'{{varName}}' is marked as ignored but is used{{additional}}."),
        ("removeVar", "Remove unused variable '{{varName}}'."),
    ],
    has_default_options: true,
    create,
};

struct Pattern {
    pat: Pat,
    display: String,
}

struct R {
    vars_all: bool,
    args: String,
    ignore_rest_siblings: bool,
    caught_errors_none: bool,
    ignore_class_with_static_init_block: bool,
    ignore_using_declarations: bool,
    report_used_ignore_pattern: bool,
    vars_ignore: Option<Pattern>,
    args_ignore: Option<Pattern>,
    caught_ignore: Option<Pattern>,
    array_ignore: Option<Pattern>,
}

fn pattern(v: &Value) -> Result<Option<Pattern>, String> {
    let Value::String(s) = v else { return Err("invalid option".into()) };
    if s.is_empty() {
        return Ok(None);
    }
    let pat = simple_pattern(s).ok_or("ignore pattern not ported")?;
    Ok(Some(Pattern { pat, display: format!("/{s}/u") }))
}

fn create(options: &[Value]) -> Result<Box<dyn Rule>, String> {
    let mut r = R {
        vars_all: true,
        args: "after-used".into(),
        ignore_rest_siblings: false,
        caught_errors_none: false,
        ignore_class_with_static_init_block: false,
        ignore_using_declarations: false,
        report_used_ignore_pattern: false,
        vars_ignore: None,
        args_ignore: None,
        caught_ignore: None,
        array_ignore: None,
    };
    match options {
        [] => {}
        [Value::String(s)] if s == "all" || s == "local" => r.vars_all = s == "all",
        [Value::Object(o)] => {
            for (k, v) in o {
                match (k.as_str(), v) {
                    ("vars", Value::String(s)) if s == "all" || s == "local" => r.vars_all = s == "all",
                    ("args", Value::String(s)) if matches!(s.as_str(), "all" | "after-used" | "none") => {
                        r.args = s.clone()
                    }
                    ("caughtErrors", Value::String(s)) if s == "all" || s == "none" => r.caught_errors_none = s == "none",
                    ("ignoreRestSiblings", Value::Bool(b)) => r.ignore_rest_siblings = *b,
                    ("ignoreClassWithStaticInitBlock", Value::Bool(b)) => r.ignore_class_with_static_init_block = *b,
                    ("ignoreUsingDeclarations", Value::Bool(b)) => r.ignore_using_declarations = *b,
                    ("reportUsedIgnorePattern", Value::Bool(b)) => r.report_used_ignore_pattern = *b,
                    ("varsIgnorePattern", _) => r.vars_ignore = pattern(v)?,
                    ("argsIgnorePattern", _) => r.args_ignore = pattern(v)?,
                    ("caughtErrorsIgnorePattern", _) => r.caught_ignore = pattern(v)?,
                    ("destructuredArrayIgnorePattern", _) => r.array_ignore = pattern(v)?,
                    _ => return Err("invalid option".into()),
                }
            }
        }
        _ => return Err("invalid option".into()),
    }
    Ok(Box::new(r))
}

#[derive(Clone, Copy)]
enum VarType {
    ArrayDestructure,
    CatchClause,
    Parameter,
    Variable,
}

fn is_inside(ast: &Ast, inner: NodeId, outer: NodeId) -> bool {
    let (a, b) = ast.range(inner);
    let (c, d) = ast.range(outer);
    a >= c && b <= d
}

fn is_unused_expression(ast: &Ast, node: NodeId) -> bool {
    let Some(parent) = ast.parent(node) else { return false };
    match ast.kind(parent) {
        "ExpressionStatement" => true,
        "SequenceExpression" => {
            if ast.list(parent, "expressions").last().copied().flatten() != Some(node) {
                return true;
            }
            is_unused_expression(ast, parent)
        }
        _ => false,
    }
}

fn upper_function(ast: &Ast, node: NodeId) -> Option<NodeId> {
    let mut cur = Some(node);
    while let Some(n) = cur {
        if is_function(ast, n) {
            return Some(n);
        }
        cur = ast.parent(n);
    }
    None
}

fn is_storable_function(ast: &Ast, func: NodeId, rhs: NodeId) -> bool {
    let mut node = func;
    let mut parent = ast.parent(func);
    while let Some(p) = parent {
        if !is_inside(ast, p, rhs) {
            break;
        }
        match ast.kind(p) {
            "SequenceExpression" => {
                if ast.list(p, "expressions").last().copied().flatten() != Some(node) {
                    return false;
                }
            }
            "CallExpression" | "NewExpression" => return ast.child(p, "callee") != Some(node),
            "AssignmentExpression" | "TaggedTemplateExpression" | "YieldExpression" => return true,
            k => {
                if k.ends_with("Statement") || k.ends_with("Declaration") {
                    return true;
                }
            }
        }
        node = p;
        parent = ast.parent(p);
    }
    false
}

fn is_inside_of_storable_function(ast: &Ast, id: NodeId, rhs: NodeId) -> bool {
    upper_function(ast, id).is_some_and(|f| is_inside(ast, f, rhs) && is_storable_function(ast, f, rhs))
}

fn is_read_for_itself(sm: &ScopeManager, ast: &Ast, r: RefId, rhs: Option<NodeId>) -> bool {
    let rf = sm.reference(r);
    let id = rf.identifier;
    let Some(parent) = ast.parent(id) else { return false };
    rf.is_read()
        && ((ast.is(parent, "AssignmentExpression")
            && ast.child(parent, "left") == Some(id)
            && is_unused_expression(ast, parent)
            && !matches!(ast.str(parent, "operator"), Some("&&=" | "||=" | "??=")))
            || (ast.is(parent, "UpdateExpression") && is_unused_expression(ast, parent))
            || rhs.is_some_and(|rhs| is_inside(ast, id, rhs) && !is_inside_of_storable_function(ast, id, rhs)))
}

fn get_rhs_node(sm: &ScopeManager, ast: &Ast, r: RefId, prev: Option<NodeId>) -> Option<NodeId> {
    let rf = sm.reference(r);
    let id = rf.identifier;
    let parent = ast.parent(id)?;
    let ref_scope = sm.scope(rf.from).variable_scope;
    let var_scope = rf.resolved.map(|v| sm.scope(sm.var(v).scope).variable_scope);
    let can_be_used_later = Some(ref_scope) != var_scope || is_in_loop(ast, id);
    if let Some(p) = prev
        && is_inside(ast, id, p)
    {
        return Some(p);
    }
    if ast.is(parent, "AssignmentExpression")
        && is_unused_expression(ast, parent)
        && ast.child(parent, "left") == Some(id)
        && !can_be_used_later
    {
        return ast.child(parent, "right");
    }
    None
}

fn is_for_in_of_ref(sm: &ScopeManager, ast: &Ast, r: RefId) -> bool {
    let Some(mut target) = ast.parent(sm.reference(r).identifier) else { return false };
    if ast.is(target, "VariableDeclarator") {
        let Some(t) = ast.parent(target).and_then(|p| ast.parent(p)) else { return false };
        target = t;
    }
    if !matches!(ast.kind(target), "ForInStatement" | "ForOfStatement") {
        return false;
    }
    let Some(body) = ast.child(target, "body") else { return false };
    let t = if ast.is(body, "BlockStatement") { ast.list(body, "body").first().copied().flatten() } else { Some(body) };
    t.is_some_and(|t| ast.is(t, "ReturnStatement"))
}

fn function_definitions(sm: &ScopeManager, ast: &Ast, v: VarId) -> Vec<NodeId> {
    let mut out = Vec::new();
    for d in &sm.var(v).defs {
        if d.kind == DefType::FunctionName {
            out.push(d.node);
        }
        if d.kind == DefType::Variable
            && let Some(init) = ast.child(d.node, "init")
            && matches!(ast.kind(init), "FunctionExpression" | "ArrowFunctionExpression")
        {
            out.push(init);
        }
    }
    out
}

fn is_self_reference(sm: &ScopeManager, r: RefId, nodes: &[NodeId]) -> bool {
    let mut scope = Some(sm.reference(r).from);
    while let Some(s) = scope {
        if nodes.contains(&sm.scope(s).block) {
            return true;
        }
        scope = sm.scope(s).upper;
    }
    false
}

fn is_used_variable(sm: &ScopeManager, ast: &Ast, v: VarId) -> bool {
    let var = sm.var(v);
    if var.eslint_used.get() {
        return true;
    }
    let fns = function_definitions(sm, ast, v);
    let mut rhs = None;
    var.references.iter().any(|&r| {
        if is_for_in_of_ref(sm, ast, r) {
            return true;
        }
        let for_itself = is_read_for_itself(sm, ast, r, rhs);
        rhs = get_rhs_node(sm, ast, r, rhs);
        sm.reference(r).is_read() && !for_itself && !(!fns.is_empty() && is_self_reference(sm, r, &fns))
    })
}

fn is_exported(sm: &ScopeManager, ast: &Ast, v: VarId) -> bool {
    let Some(d) = sm.var(v).defs.first() else { return false };
    let mut node = d.node;
    if ast.is(node, "VariableDeclarator") {
        match ast.parent(node) {
            Some(p) => node = p,
            None => return false,
        }
    } else if d.kind == DefType::Parameter {
        return false;
    }
    ast.parent(node).is_some_and(|p| ast.kind(p).starts_with("Export"))
}

fn has_rest_sibling(ast: &Ast, node: Option<NodeId>) -> bool {
    let Some(node) = node else { return false };
    ast.is(node, "Property")
        && ast.parent(node).is_some_and(|p| {
            ast.is(p, "ObjectPattern")
                && ast.list(p, "properties").last().copied().flatten().is_some_and(|l| ast.is(l, "RestElement"))
        })
}

impl R {
    fn description(&self, t: VarType) -> (&'static str, Option<&str>) {
        let (desc, p) = match t {
            VarType::ArrayDestructure => ("elements of array destructuring", &self.array_ignore),
            VarType::CatchClause => ("caught errors", &self.caught_ignore),
            VarType::Parameter => ("args", &self.args_ignore),
            VarType::Variable => ("vars", &self.vars_ignore),
        };
        (desc, p.as_ref().map(|p| p.display.as_str()))
    }

    fn def_type(&self, ast: &Ast, sm: &ScopeManager, v: VarId) -> Option<VarType> {
        let d = sm.var(v).defs.first()?;
        if self.array_ignore.is_some() && ast.parent(d.name).is_some_and(|p| ast.is(p, "ArrayPattern")) {
            return Some(VarType::ArrayDestructure);
        }
        Some(match d.kind {
            DefType::CatchClause => VarType::CatchClause,
            DefType::Parameter => VarType::Parameter,
            _ => VarType::Variable,
        })
    }

    fn additional(&self, ast: &Ast, sm: &ScopeManager, v: VarId) -> String {
        if let Some(t) = self.def_type(ast, sm, v)
            && let (desc, Some(p)) = self.description(t)
        {
            return format!(". Allowed unused {desc} must match {p}");
        }
        String::new()
    }

    fn report_used_ignored(&self, cx: &mut Cx, v: VarId, t: VarType) {
        let sm = cx.sm();
        let ast = &cx.sc.ast;
        if !(self.report_used_ignore_pattern && is_used_variable(sm, ast, v)) {
            return;
        }
        let Some(d) = sm.var(v).defs.first() else { return };
        let (desc, p) = self.description(t);
        let additional = match p {
            Some(p) => format!(". Used {desc} must not match {p}"),
            None => String::new(),
        };
        cx.report(
            Report::node(d.name, "usedIgnoredVar").data("varName", sm.var(v).name.clone()).data("additional", additional),
        );
    }

    fn is_after_last_used_arg(&self, cx: &Cx, v: VarId) -> bool {
        let sm = cx.sm();
        let Some(d) = sm.var(v).defs.first() else { return true };
        let params = cx.declared_variables(d.node);
        let after = match params.iter().position(|x| *x == v) {
            Some(i) => &params[i + 1..],
            None => params,
        };
        !after.iter().any(|p| !sm.var(*p).references.is_empty() || sm.var(*p).eslint_used.get())
    }

    fn collect(&self, cx: &mut Cx, scope: ScopeId, out: &mut Vec<VarId>) {
        let sm = cx.sm();
        let ast = &cx.sc.ast;
        let sc = sm.scope(scope);
        if sc.kind != ScopeType::Global || self.vars_all {
            for &v in &sc.variables {
                let var = sm.var(v);
                if sc.kind == ScopeType::Class && var.identifiers.first().is_some_and(|i| ast.child(sc.block, "id") == Some(*i)) {
                    continue;
                }
                if sc.function_expression_scope {
                    continue;
                }
                if !self.report_used_ignore_pattern && var.eslint_used.get() {
                    continue;
                }
                if sc.kind == ScopeType::Function && var.name == "arguments" && var.identifiers.is_empty() {
                    continue;
                }
                if let Some(d) = var.defs.first() {
                    let name = ast.name(d.name).unwrap_or("");
                    let ref_in_array = var
                        .references
                        .iter()
                        .any(|r| ast.parent(sm.reference(*r).identifier).is_some_and(|p| ast.is(p, "ArrayPattern")));
                    if (ast.parent(d.name).is_some_and(|p| ast.is(p, "ArrayPattern")) || ref_in_array)
                        && self.array_ignore.as_ref().is_some_and(|p| p.pat.test(name))
                    {
                        self.report_used_ignored(cx, v, VarType::ArrayDestructure);
                        continue;
                    }
                    if d.kind == DefType::ClassName {
                        let has_static = ast
                            .child(d.node, "body")
                            .is_some_and(|b| ast.nodes_of(b, "body").any(|n| ast.is(n, "StaticBlock")));
                        if self.ignore_class_with_static_init_block && has_static {
                            continue;
                        }
                    }
                    if d.kind == DefType::CatchClause {
                        if self.caught_errors_none {
                            continue;
                        }
                        if self.caught_ignore.as_ref().is_some_and(|p| p.pat.test(name)) {
                            self.report_used_ignored(cx, v, VarType::CatchClause);
                            continue;
                        }
                    } else if d.kind == DefType::Parameter {
                        if let Some(p) = ast.parent(d.node)
                            && matches!(ast.kind(p), "Property" | "MethodDefinition")
                            && ast.str(p, "kind") == Some("set")
                        {
                            continue;
                        }
                        if self.args == "none" {
                            continue;
                        }
                        if self.args_ignore.as_ref().is_some_and(|p| p.pat.test(name)) {
                            self.report_used_ignored(cx, v, VarType::Parameter);
                            continue;
                        }
                        if self.args == "after-used"
                            && ast.parent(d.name).is_some_and(|p| is_function(ast, p))
                            && !self.is_after_last_used_arg(cx, v)
                        {
                            continue;
                        }
                    } else if self.vars_ignore.as_ref().is_some_and(|p| p.pat.test(name)) {
                        self.report_used_ignored(cx, v, VarType::Variable);
                        continue;
                    }
                }
                let uses_erm = var.defs.first().is_some_and(|d| {
                    d.kind == DefType::Variable && matches!(d.parent.and_then(|p| ast.str(p, "kind")), Some("using" | "await using"))
                });
                let rest_sibling = self.ignore_rest_siblings
                    && (var.defs.iter().any(|d| has_rest_sibling(ast, ast.parent(d.name)))
                        || var.references.iter().any(|r| has_rest_sibling(ast, ast.parent(sm.reference(*r).identifier))));
                if !is_used_variable(sm, ast, v)
                    && !is_exported(sm, ast, v)
                    && !(self.ignore_using_declarations && uses_erm)
                    && !rest_sibling
                {
                    out.push(v);
                }
            }
        }
        for &c in &sc.child_scopes {
            self.collect(cx, c, out);
        }
    }
}

type F = Option<((u32, u32), String)>;

fn remove(r: (u32, u32)) -> F {
    Some((r, String::new()))
}

struct Fixes<'a> {
    sc: &'a SourceCode,
    ast: &'a Ast,
}

impl Fixes<'_> {
    fn before(&self, start: u32, skip: usize) -> Option<&Token> {
        self.sc.token_before_offset(start, Opts { include_comments: false, skip }, |_| true)
    }
    fn after(&self, end: u32) -> Option<&Token> {
        self.sc.token_after_offset(end, N, |_| true)
    }
    fn prev_start(&self, node: NodeId, skip: usize) -> Option<u32> {
        self.before(self.ast.range(node).0, skip).map(|t| t.start)
    }
    fn next_end(&self, node: NodeId) -> Option<u32> {
        self.after(self.ast.range(node).1).map(|t| t.end)
    }
    fn before_value(&self, node: NodeId) -> Option<&str> {
        self.before(self.ast.range(node).0, 0).map(|t| t.value.as_str())
    }
    fn after_value(&self, node: NodeId) -> Option<&str> {
        self.after(self.ast.range(node).1).map(|t| t.value.as_str())
    }
    fn kind(&self, n: Option<NodeId>) -> &'static str {
        n.map(|n| self.ast.kind(n)).unwrap_or("")
    }
    fn parent(&self, n: NodeId) -> Option<NodeId> {
        self.ast.parent(n)
    }
    fn has_single_element(&self, node: NodeId) -> bool {
        self.ast.nodes_of(node, "elements").count() == 1
    }
    fn has_import_of(&self, node: NodeId, t: &str) -> bool {
        self.ast.nodes_of(node, "specifiers").any(|s| self.ast.is(s, t))
    }
    fn is_loop(&self, n: Option<NodeId>) -> bool {
        matches!(self.kind(n), "DoWhileStatement" | "ForStatement" | "ForInStatement" | "ForOfStatement" | "WhileStatement")
    }

    fn node_by_range_index(&self, index: u32) -> Option<NodeId> {
        let root = self.sc.root;
        let contains = |n: NodeId| {
            let (s, e) = self.ast.range(n);
            s <= index && index < e
        };
        if !contains(root) {
            return None;
        }
        let mut cur = root;
        while let Some(c) = self.ast.children(cur).into_iter().find(|c| contains(*c)) {
            cur = c;
        }
        Some(cur)
    }

    fn is_closing_brace_of_block(&self, t: &Token) -> bool {
        if !(t.kind == TokKind::Punctuator && t.value == "}") {
            return false;
        }
        let Some(node) = self.node_by_range_index(t.start) else { return false };
        match self.ast.kind(node) {
            "BlockStatement" => !matches!(self.kind(self.parent(node)), "FunctionExpression" | "ArrowFunctionExpression"),
            "ClassBody" => self.kind(self.parent(node)) == "ClassDeclaration",
            "SwitchStatement" => true,
            _ => false,
        }
    }

    fn not_safe_to_remove(&self, next: &Token, prev: Option<&Token>) -> bool {
        next.kind == TokKind::String
            || prev.is_some_and(|p| {
                !(p.kind == TokKind::Punctuator && (p.value == ";" || p.value == "{")) && !self.is_closing_brace_of_block(p)
            })
    }

    fn fix_function_parameters(&self, node: NodeId) -> F {
        let parent = self.parent(node)?;
        if is_function(self.ast, parent) {
            if self.ast.list(parent, "params").len() == 1 {
                return remove(self.ast.range(node));
            }
            if self.before_value(node) == Some("(") && self.after_value(node) == Some(",") {
                return remove((self.ast.range(node).0, self.next_end(node)?));
            }
            return remove((self.prev_start(node, 0)?, self.ast.range(node).1));
        }
        None
    }

    fn fix_declarator_in_list(&self, decl: NodeId) -> F {
        let list = self.parent(decl)?;
        if self.before_value(decl) == Some(",") {
            if self.ast.list(list, "declarations").last().copied().flatten() == Some(decl) {
                let last = self.sc.last_token(list, N)?;
                if !(last.kind == TokKind::Punctuator && last.value == ";")
                    && let Some(next) = self.after(self.ast.range(list).1)
                    && self.not_safe_to_remove(next, self.before(self.ast.range(decl).0, 1))
                {
                    return None;
                }
            }
            return remove((self.prev_start(decl, 0)?, self.ast.range(decl).1));
        }
        remove((self.ast.range(decl).0, self.next_end(decl)?))
    }

    fn fix_variables(&self, node: NodeId) -> F {
        let parent = self.parent(node)?;
        if self.ast.is(parent, "VariableDeclarator") {
            let list = self.parent(parent)?;
            if self.is_loop(self.parent(list)) {
                return None;
            }
            if self.ast.list(list, "declarations").len() == 1 {
                if let Some(next) = self.after(self.ast.range(list).1)
                    && self.not_safe_to_remove(next, self.before(self.ast.range(list).0, 0))
                {
                    return None;
                }
                return remove(self.ast.range(list));
            }
            return self.fix_declarator_in_list(parent);
        }
        if self.before_value(node) == Some(":") && self.kind(self.parent(parent)) == "ObjectPattern" {
            return self.fix_object_with_value_separator(node);
        }
        self.fix_function_parameters(node)
    }

    fn fix_nested_object_variable(&self, node: NodeId) -> F {
        let parent = self.parent(node)?;
        let pp = self.parent(parent)?;
        let pppp = self.parent(pp).and_then(|x| self.parent(x));
        if self.kind(pppp) == "ObjectPattern" && self.ast.list(pp, "properties").len() == 1 {
            return self.fix_nested_object_variable(pp);
        }
        if self.ast.is(pp, "ObjectPattern") {
            if self.ast.list(pp, "properties").len() == 1 {
                return self.fix_variables(pp);
            }
            if self.before_value(parent) == Some("{") {
                return remove((self.ast.range(parent).0, self.next_end(parent)?));
            }
            return remove((self.prev_start(parent, 0)?, self.ast.range(parent).1));
        }
        None
    }

    fn fix_nested_array_variable(&self, node: NodeId) -> F {
        let parent = self.parent(node)?;
        if self.kind(self.parent(parent)) == "ArrayPattern" && self.has_single_element(parent) {
            return self.fix_nested_array_variable(parent);
        }
        if self.has_single_element(parent) {
            if self.before_value(parent) == Some(":") {
                return self.fix_variables(parent);
            }
            if self.kind(self.parent(parent)) == "RestElement" {
                return self.fix_rest_in_pattern(self.parent(parent)?);
            }
            return self.fix_variables(parent);
        }
        if self.before_value(node) == Some(",") && self.after_value(node) == Some("]") {
            return remove((self.prev_start(node, 0)?, self.ast.range(node).1));
        }
        remove(self.ast.range(node))
    }

    fn fix_object_with_value_separator(&self, node: NodeId) -> F {
        let parent = self.parent(self.parent(node)?)?;
        if self.kind(self.parent(parent)) == "ArrayPattern" && self.ast.list(parent, "properties").len() == 1 {
            return self.fix_nested_array_variable(parent);
        }
        self.fix_nested_object_variable(node)
    }

    fn fix_rest_in_pattern(&self, node: NodeId) -> F {
        let parent = self.parent(node)?;
        if is_function(self.ast, parent) {
            if self.ast.list(parent, "params").len() == 1 {
                return remove(self.ast.range(node));
            }
            return remove((self.prev_start(node, 0)?, self.ast.range(node).1));
        }
        if self.ast.is(parent, "ArrayPattern") {
            if self.has_single_element(parent) {
                if self.kind(self.parent(parent)) == "ArrayPattern" {
                    return self.fix_nested_array_variable(parent);
                }
                return self.fix_variables(parent);
            }
            return remove((self.prev_start(node, 0)?, self.ast.range(node).1));
        }
        None
    }

    fn handle(&self, sm: &ScopeManager, v: VarId) -> F {
        let ast = self.ast;
        let var = sm.var(v);
        let id = *var.identifiers.first()?;
        let parent = self.parent(id)?;
        let pt = ast.kind(parent);
        let (id_s, id_e) = ast.range(id);
        let token_before = self.before(id_s, 0);
        let token_after = self.after(id_e);
        if var.references.iter().any(|r| {
            let rf = sm.reference(*r);
            rf.is_write() && ast.range(rf.identifier).0 != id_s
        }) {
            return None;
        }
        let pp = self.parent(parent);
        if pt == "VariableDeclarator" {
            let list = pp?;
            let lp = self.parent(list);
            if ast.list(list, "declarations").len() == 1 {
                if self.is_loop(lp) && lp.and_then(|l| ast.child(l, "body")) != Some(list) {
                    return None;
                }
                if self.kind(lp) == "IfStatement"
                    || self.is_loop(lp)
                    || (self.kind(lp) == "WithStatement" && lp.and_then(|l| ast.child(l, "body")) == Some(list))
                {
                    return Some((ast.range(list), ";".to_string()));
                }
                if let Some(next) = self.after(ast.range(list).1)
                    && self.not_safe_to_remove(next, self.before(ast.range(list).0, 0))
                {
                    return None;
                }
                return remove(ast.range(list));
            }
            if token_before?.value == "," {
                if ast.list(list, "declarations").last().copied().flatten() == Some(parent) {
                    let last = self.sc.last_token(list, N)?;
                    if !(last.kind == TokKind::Punctuator && last.value == ";")
                        && let Some(next) = self.after(ast.range(list).1)
                        && self.not_safe_to_remove(next, self.before(ast.range(parent).0, 1))
                    {
                        return None;
                    }
                }
                return remove((token_before?.start, ast.range(parent).1));
            }
            return remove((ast.range(parent).0, self.next_end(parent)?));
        }
        if self.kind(pp) == "ObjectPattern" {
            let pp = pp?;
            if ast.list(pp, "properties").len() == 1 {
                let ppp = self.parent(pp);
                if self.kind(ppp) == "RestElement" {
                    return self.fix_rest_in_pattern(ppp?);
                }
                if self.kind(ppp) == "ArrayPattern" {
                    return self.fix_nested_array_variable(pp);
                }
                return self.fix_variables(pp);
            }
            if token_before?.value == ":" {
                if self.before_value(parent) == Some("{") && self.after_value(parent) == Some(",") {
                    return remove((ast.range(parent).0, self.next_end(parent)?));
                }
                return remove((self.prev_start(parent, 0)?, id_e));
            }
        }
        if pt == "ArrayPattern" {
            if self.has_single_element(parent) {
                if self.kind(pp) == "RestElement" {
                    return self.fix_rest_in_pattern(pp?);
                }
                if self.kind(pp) == "ArrayPattern" {
                    return self.fix_nested_array_variable(parent);
                }
                return self.fix_variables(parent);
            }
            if token_before?.value == "," && token_after?.value == "," {
                return remove((id_s, id_e));
            }
        }
        if pt == "RestElement" {
            let pp = pp?;
            if ast.is(pp, "ArrayPattern") {
                if self.has_single_element(pp) {
                    if self.kind(self.parent(pp)) == "ArrayPattern" {
                        return self.fix_nested_array_variable(pp);
                    }
                    return self.fix_variables(pp);
                }
                return remove((self.prev_start(id, 1)?, id_e));
            }
            if ast.is(pp, "ObjectPattern") {
                if ast.list(pp, "properties").len() == 1 {
                    return self.fix_variables(pp);
                }
                return remove((self.prev_start(id, 1)?, id_e));
            }
            if is_function(ast, pp) {
                if ast.list(pp, "params").len() == 1 {
                    return remove(ast.range(parent));
                }
                return remove((self.prev_start(parent, 0)?, ast.range(parent).1));
            }
        }
        if pt == "AssignmentPattern" {
            let pp = pp?;
            if ast.is(pp, "ArrayPattern") {
                return self.fix_nested_array_variable(parent);
            }
            let ppp = self.parent(pp);
            if self.kind(ppp) == "ObjectPattern" {
                let ppp = ppp?;
                if ast.list(ppp, "properties").len() == 1 {
                    if self.kind(self.parent(ppp)) == "ArrayPattern" {
                        return self.fix_nested_array_variable(ppp);
                    }
                    return self.fix_variables(ppp);
                }
                if self.before_value(pp) == Some("{") && self.after_value(pp) == Some(",") {
                    return remove((ast.range(pp).0, self.next_end(pp)?));
                }
                return remove((self.prev_start(pp, 0)?, ast.range(pp).1));
            }
            if is_function(ast, pp) {
                return self.fix_function_parameters(parent);
            }
        }
        if (pt == "FunctionDeclaration" && ast.child(parent, "id") == Some(id)) || pt == "ClassDeclaration" {
            if let Some(next) = self.after(ast.range(parent).1)
                && self.not_safe_to_remove(next, self.before(ast.range(parent).0, 0))
            {
                return None;
            }
            return remove(ast.range(parent));
        }
        if pt == "ImportDefaultSpecifier" {
            let decl = pp?;
            if !self.has_import_of(decl, "ImportSpecifier") && !self.has_import_of(decl, "ImportNamespaceSpecifier") {
                return remove((ast.range(parent).0, ast.range(ast.child(decl, "source")?).0));
            }
            return remove((id_s, token_after?.end));
        }
        if pt == "ImportSpecifier" {
            let decl = pp?;
            if ast.nodes_of(decl, "specifiers").filter(|s| ast.is(*s, "ImportSpecifier")).count() == 1 {
                if !self.has_import_of(decl, "ImportDefaultSpecifier") {
                    return remove(ast.range(decl));
                }
                return remove((self.prev_start(parent, 1)?, token_after?.end));
            }
            if self.before_value(parent) == Some("{") {
                return remove((ast.range(parent).0, self.next_end(parent)?));
            }
            return remove((self.prev_start(parent, 0)?, ast.range(parent).1));
        }
        if pt == "ImportNamespaceSpecifier" {
            let decl = pp?;
            if self.has_import_of(decl, "ImportDefaultSpecifier") {
                return remove((self.prev_start(parent, 0)?, ast.range(parent).1));
            }
            return remove((ast.range(parent).0, ast.range(ast.child(decl, "source")?).0));
        }
        if pt == "CatchClause" {
            return None;
        }
        if let Some(tb) = token_before
            && tb.value == ","
        {
            return remove((tb.start, id_e));
        }
        if token_after?.value == "," {
            let tb = token_before?;
            if tb.value == "(" || tb.value == "{" {
                return remove((id_s, token_after?.end));
            }
        }
        if pt == "ArrowFunctionExpression"
            && ast.list(parent, "params").len() == 1
            && token_after.map(|t| t.value.as_str()) != Some(")")
        {
            return Some(((id_s, id_e), "()".to_string()));
        }
        remove((id_s, id_e))
    }
}

impl Rule for R {
    fn listeners(&self) -> &'static [&'static str] {
        &["Program:exit"]
    }

    fn on(&mut self, _: usize, node: NodeId, cx: &mut Cx) {
        let sm = cx.sm();
        let ast = &cx.sc.ast;
        let scope = cx.get_scope(node);
        let mut unused = Vec::new();
        self.collect(cx, scope, &mut unused);
        for v in unused {
            let var = sm.var(v);
            if !var.defs.is_empty() {
                let vs = sm.scope(var.scope).variable_scope;
                let last_write = var
                    .references
                    .iter().rfind(|r| {
                        let rf = sm.reference(**r);
                        rf.is_write() && sm.scope(rf.from).variable_scope == vs
                    });
                let target = match last_write {
                    Some(r) => sm.reference(*r).identifier,
                    None => match var.identifiers.first() {
                        Some(i) => *i,
                        None => continue,
                    },
                };
                let assigned = var.references.iter().any(|r| sm.reference(*r).is_write());
                let fx = Fixes { sc: cx.sc, ast };
                let fix = fx.handle(sm, v).map(|(r, t)| fixer::replace_range(r, t));
                let mut r = Report::node(target, "unusedVar")
                    .data("varName", var.name.clone())
                    .data("action", if assigned { "assigned a value" } else { "defined" })
                    .data("additional", self.additional(ast, sm, v));
                r.suggest.push(SuggestSpec {
                    keys: vec![
                        ("messageId", SugVal::Str("removeVar".into())),
                        ("data", SugVal::Data(vec![("varName".into(), var.name.clone().into())])),
                        ("fix", SugVal::Fix),
                    ],
                    fix,
                });
                cx.report(r);
            } else if let Some(&ci) = var.eslint_explicit_global_comments.as_ref().and_then(|c| c.first()) {
                let loc = super::no_redeclare::name_location_in_global_comment(cx, ci, &var.name);
                cx.report(
                    Report::loc(loc, "unusedVar")
                        .data("varName", var.name.clone())
                        .data("action", "defined")
                        .data("additional", self.additional(ast, sm, v)),
                );
            }
        }
    }
}
