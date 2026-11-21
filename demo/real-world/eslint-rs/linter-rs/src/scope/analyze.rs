//! eslint-scope 9.1.2: `Referencer`, `PatternVisitor`, `Scope#__close` and friends, with the
//! options ESLint passes (`ignoreEval`, ecmaVersion latest, no implied strict).

use super::{
    DefType, Definition, Implicit, READ, RW, Reference, Scope, ScopeId, ScopeManager, ScopeType,
    VarId, Variable, WRITE,
};
use crate::ast::{Ast, NodeId};
use crate::source::SourceType;
use std::cell::Cell;
use std::collections::HashMap;

pub fn analyze(ast: &Ast, root: NodeId, source_type: SourceType, jsx: bool) -> ScopeManager {
    let mut r = Referencer {
        ast,
        sm: ScopeManager::default(),
        inner_method: false,
        global_return: source_type == SourceType::CommonJs,
        module: source_type == SourceType::Module,
        jsx,
    };
    r.visit(Some(root));
    r.sm
}

struct PatEvent {
    pattern: NodeId,
    top_level: bool,
    rest: bool,
    assignments: Vec<NodeId>,
}

struct PatternVisitor<'a> {
    ast: &'a Ast,
    root: NodeId,
    events: Vec<PatEvent>,
    rhs: Vec<NodeId>,
    assignments: Vec<NodeId>,
    rest_elements: Vec<NodeId>,
}

impl PatternVisitor<'_> {
    fn visit(&mut self, n: Option<NodeId>) {
        let Some(n) = n else { return };
        let ast = self.ast;
        match ast.kind(n) {
            "Identifier" => {
                let rest = self
                    .rest_elements
                    .last()
                    .is_some_and(|r| ast.child(*r, "argument") == Some(n));
                self.events.push(PatEvent {
                    pattern: n,
                    top_level: n == self.root,
                    rest,
                    assignments: self.assignments.clone(),
                });
            }
            "Property" => {
                if ast.bool(n, "computed")
                    && let Some(k) = ast.child(n, "key")
                {
                    self.rhs.push(k);
                }
                self.visit(ast.child(n, "value"));
            }
            "ArrayPattern" | "ArrayExpression" => {
                for e in ast.list(n, "elements").to_vec() {
                    self.visit(e);
                }
            }
            "AssignmentPattern" | "AssignmentExpression" => {
                self.assignments.push(n);
                self.visit(ast.child(n, "left"));
                if let Some(r) = ast.child(n, "right") {
                    self.rhs.push(r);
                }
                self.assignments.pop();
            }
            "RestElement" => {
                self.rest_elements.push(n);
                self.visit(ast.child(n, "argument"));
                self.rest_elements.pop();
            }
            "MemberExpression" => {
                if ast.bool(n, "computed")
                    && let Some(p) = ast.child(n, "property")
                {
                    self.rhs.push(p);
                }
                if let Some(o) = ast.child(n, "object") {
                    self.rhs.push(o);
                }
            }
            "SpreadElement" => self.visit(ast.child(n, "argument")),
            "CallExpression" => {
                for a in ast.nodes_of(n, "arguments") {
                    self.rhs.push(a);
                }
                self.visit(ast.child(n, "callee"));
            }
            _ => {
                for c in ast.children(n) {
                    self.visit(Some(c));
                }
            }
        }
    }
}

fn is_pattern(ast: &Ast, n: NodeId) -> bool {
    matches!(
        ast.kind(n),
        "Identifier"
            | "ObjectPattern"
            | "ArrayPattern"
            | "SpreadElement"
            | "RestElement"
            | "AssignmentPattern"
    )
}

struct Referencer<'a> {
    ast: &'a Ast,
    sm: ScopeManager,
    inner_method: bool,
    global_return: bool,
    module: bool,
    jsx: bool,
}

impl Referencer<'_> {
    fn cur(&self) -> ScopeId {
        self.sm.current.unwrap_or(0)
    }

    fn scope_mut(&mut self, s: ScopeId) -> &mut Scope {
        &mut self.sm.scopes[s as usize]
    }

    fn is_strict_scope(
        &self,
        kind: ScopeType,
        upper: Option<ScopeId>,
        block: NodeId,
        is_method: bool,
    ) -> bool {
        let ast = self.ast;
        if upper.is_some_and(|u| self.sm.scope(u).is_strict) {
            return true;
        }
        if is_method {
            return true;
        }
        let body = match kind {
            ScopeType::Class | ScopeType::Module => return true,
            ScopeType::Block | ScopeType::Switch => return false,
            ScopeType::Function => {
                if ast.is(block, "ArrowFunctionExpression")
                    && !ast
                        .child(block, "body")
                        .is_some_and(|b| ast.is(b, "BlockStatement"))
                {
                    return false;
                }
                if ast.is(block, "Program") {
                    block
                } else {
                    match ast.child(block, "body") {
                        Some(b) => b,
                        None => return false,
                    }
                }
            }
            ScopeType::Global => block,
            _ => return false,
        };
        for stmt in ast.nodes_of(body, "body") {
            match ast.str(stmt, "directive") {
                Some("use strict") => return true,
                Some(_) => {}
                None => break,
            }
        }
        false
    }

    fn nest(&mut self, kind: ScopeType, block: NodeId, is_method: bool) -> ScopeId {
        let upper = self.sm.current;
        let id = self.sm.scopes.len() as ScopeId;
        let variable_scope = match kind {
            ScopeType::Global
            | ScopeType::Module
            | ScopeType::Function
            | ScopeType::ClassFieldInitializer
            | ScopeType::ClassStaticBlock => id,
            _ => upper.map(|u| self.sm.scope(u).variable_scope).unwrap_or(id),
        };
        let is_strict = self.is_strict_scope(kind, upper, block, is_method);
        self.sm.scopes.push(Scope {
            kind,
            set: HashMap::new(),
            dynamic: matches!(kind, ScopeType::Global | ScopeType::With),
            block,
            through: Vec::new(),
            variables: Vec::new(),
            references: Vec::new(),
            variable_scope,
            function_expression_scope: false,
            direct_call_to_eval_scope: false,
            this_found: false,
            upper,
            is_strict,
            child_scopes: Vec::new(),
            implicit: Implicit::default(),
            left: Some(Vec::new()),
        });
        if let Some(u) = upper {
            self.scope_mut(u).child_scopes.push(id);
        }
        self.sm.node_to_scope.entry(block).or_default().push(id);
        self.sm.current = Some(id);
        match kind {
            ScopeType::Function => {
                if !self.ast.is(block, "ArrowFunctionExpression") {
                    self.define_generic(id, "arguments", false, None, None);
                }
            }
            ScopeType::FunctionExpressionName => {
                if let Some(fid) = self.ast.child(block, "id") {
                    self.define(id, Some(fid), def(DefType::FunctionName, fid, block));
                }
                self.scope_mut(id).function_expression_scope = true;
            }
            _ => {}
        }
        id
    }

    fn add_declared(&mut self, var: VarId, node: Option<NodeId>) {
        let Some(node) = node else { return };
        let list = self.sm.declared.entry(node).or_default();
        if !list.contains(&var) {
            list.push(var);
        }
    }

    fn define_generic(
        &mut self,
        s: ScopeId,
        name: &str,
        implicit: bool,
        node: Option<NodeId>,
        d: Option<Definition>,
    ) {
        let existing = if implicit {
            self.sm.scope(s).implicit.set.get(name)
        } else {
            self.sm.scope(s).set.get(name)
        };
        let var = match existing {
            Some(v) => *v,
            None => {
                let v = self.sm.vars.len() as VarId;
                self.sm.vars.push(Variable {
                    name: name.to_string(),
                    identifiers: Vec::new(),
                    references: Vec::new(),
                    defs: Vec::new(),
                    tainted: false,
                    stack: true,
                    scope: s,
                    eslint_implicit_global_setting: None,
                    eslint_explicit_global: false,
                    eslint_explicit_global_comments: None,
                    writeable: None,
                    eslint_used: Cell::new(false),
                    eslint_exported: false,
                });
                let sc = self.scope_mut(s);
                if implicit {
                    sc.implicit.set.insert(name.to_string(), v);
                    sc.implicit.variables.push(v);
                } else {
                    sc.set.insert(name.to_string(), v);
                    sc.variables.push(v);
                }
                v
            }
        };
        if let Some(d) = d {
            let (dn, dp) = (d.node, d.parent);
            self.sm.vars[var as usize].defs.push(d);
            self.add_declared(var, Some(dn));
            self.add_declared(var, dp);
        }
        if let Some(n) = node {
            self.sm.vars[var as usize].identifiers.push(n);
        }
    }

    fn define(&mut self, s: ScopeId, node: Option<NodeId>, d: Definition) {
        let Some(n) = node else { return };
        if !self.ast.is(n, "Identifier") {
            return;
        }
        let name = self.ast.name(n).unwrap_or("").to_string();
        self.define_generic(s, &name, false, Some(n), Some(d));
    }

    #[allow(clippy::too_many_arguments)]
    fn referencing(
        &mut self,
        node: Option<NodeId>,
        assign: u8,
        write_expr: Option<NodeId>,
        mig: Option<(NodeId, NodeId)>,
        partial: bool,
        init: bool,
    ) {
        let Some(n) = node else { return };
        if !matches!(self.ast.kind(n), "Identifier" | "JSXIdentifier") {
            return;
        }
        if self.ast.name(n) == Some("super") {
            return;
        }
        let s = self.cur();
        let flag = if assign == 0 { READ } else { assign };
        let write = flag & WRITE != 0;
        let r = self.sm.refs.len() as u32;
        self.sm.refs.push(Reference {
            identifier: n,
            from: s,
            tainted: false,
            resolved: None,
            flag,
            write_expr: if write { write_expr } else { None },
            partial: if write { Some(partial) } else { None },
            init: if write { Some(init) } else { None },
            maybe_implicit_global: mig,
        });
        let sc = self.scope_mut(s);
        sc.references.push(r);
        if let Some(l) = sc.left.as_mut() {
            l.push(r);
        }
    }

    fn referencing_default_value(
        &mut self,
        pattern: NodeId,
        assignments: &[NodeId],
        mig: Option<(NodeId, NodeId)>,
        init: bool,
    ) {
        for &a in assignments {
            let right = self.ast.child(a, "right");
            let partial = self.ast.child(a, "left") != Some(pattern);
            self.referencing(Some(pattern), WRITE, right, mig, partial, init);
        }
    }

    fn collect_pattern(&self, root: Option<NodeId>) -> (Vec<PatEvent>, Vec<NodeId>) {
        let Some(root) = root else {
            return (Vec::new(), Vec::new());
        };
        let mut v = PatternVisitor {
            ast: self.ast,
            root,
            events: Vec::new(),
            rhs: Vec::new(),
            assignments: Vec::new(),
            rest_elements: Vec::new(),
        };
        v.visit(Some(root));
        (v.events, v.rhs)
    }

    fn visit_rhs(&mut self, rhs: Vec<NodeId>) {
        for n in rhs {
            self.visit(Some(n));
        }
    }

    fn resolve(&mut self, s: ScopeId, r: u32) -> bool {
        let ident = self.sm.refs[r as usize].identifier;
        let name = self.ast.name(ident).unwrap_or("");
        let Some(&var) = self.sm.scope(s).set.get(name) else {
            return false;
        };
        let sc = self.sm.scope(s);
        if sc.kind == ScopeType::Function && !self.ast.is(sc.block, "Program") {
            let body_start = self
                .ast
                .child(sc.block, "body")
                .map(|b| self.ast.range(b).0)
                .unwrap_or(0);
            let v = self.sm.var(var);
            let invalid = v.scope == s
                && self.ast.range(ident).0 < body_start
                && v.defs
                    .iter()
                    .all(|d| self.ast.range(d.name).0 >= body_start);
            if invalid {
                return false;
            }
        }
        let from_vs = self.sm.scope(self.sm.refs[r as usize].from).variable_scope;
        let same = from_vs == self.sm.scope(s).variable_scope;
        let tainted = self.sm.refs[r as usize].tainted;
        let v = &mut self.sm.vars[var as usize];
        v.references.push(r);
        v.stack = v.stack && same;
        if tainted {
            v.tainted = true;
        }
        self.sm.refs[r as usize].resolved = Some(var);
        true
    }

    fn delegate(&mut self, s: ScopeId, r: u32) {
        if let Some(u) = self.sm.scope(s).upper
            && let Some(l) = self.scope_mut(u).left.as_mut()
        {
            l.push(r);
        }
        self.scope_mut(s).through.push(r);
    }

    fn close_scope(&mut self, s: ScopeId) -> Option<ScopeId> {
        let kind = self.sm.scope(s).kind;
        let left = self.scope_mut(s).left.take().unwrap_or_default();
        if kind == ScopeType::Global {
            let mut implicit = Vec::new();
            for &r in &left {
                let rf = &self.sm.refs[r as usize];
                if let Some(info) = rf.maybe_implicit_global
                    && !self
                        .sm
                        .scope(s)
                        .set
                        .contains_key(self.ast.name(rf.identifier).unwrap_or(""))
                {
                    implicit.push(info);
                }
            }
            for (pattern, node) in implicit {
                if self.ast.is(pattern, "Identifier") {
                    let name = self.ast.name(pattern).unwrap_or("").to_string();
                    self.define_generic(
                        s,
                        &name,
                        true,
                        Some(pattern),
                        Some(def(DefType::ImplicitGlobalVariable, pattern, node)),
                    );
                }
            }
        }
        if kind == ScopeType::With {
            for r in left {
                self.sm.refs[r as usize].tainted = true;
                self.delegate(s, r);
            }
            return self.sm.scope(s).upper;
        }
        for r in left {
            if !self.resolve(s, r) {
                self.delegate(s, r);
            }
        }
        if kind == ScopeType::Global {
            let through = self.sm.scope(s).through.clone();
            self.scope_mut(s).implicit.left = through;
            return None;
        }
        self.sm.scope(s).upper
    }

    fn close(&mut self, node: NodeId) {
        while let Some(c) = self.sm.current {
            if self.sm.scope(c).block != node {
                break;
            }
            self.sm.current = self.close_scope(c);
        }
    }

    fn visit_children(&mut self, n: NodeId) {
        for c in self.ast.children(n) {
            self.visit(Some(c));
        }
    }

    fn visit(&mut self, n: Option<NodeId>) {
        let Some(n) = n else { return };
        let ast = self.ast;
        match ast.kind(n) {
            "AssignmentExpression" => self.assignment_expression(n),
            "CatchClause" => {
                self.nest(ScopeType::Catch, n, false);
                let (events, rhs) = self.collect_pattern(ast.child(n, "param"));
                for e in events {
                    let s = self.cur();
                    self.define(s, Some(e.pattern), def(DefType::CatchClause, e.pattern, n));
                    self.referencing_default_value(e.pattern, &e.assignments, None, true);
                }
                self.visit_rhs(rhs);
                self.visit(ast.child(n, "body"));
                self.close(n);
            }
            "Program" => {
                let g = self.nest(ScopeType::Global, n, false);
                if self.global_return {
                    self.scope_mut(g).is_strict = false;
                    self.nest(ScopeType::Function, n, false);
                }
                if self.module {
                    self.nest(ScopeType::Module, n, false);
                }
                self.visit_children(n);
                self.close(n);
            }
            "Identifier" => self.referencing(Some(n), 0, None, None, false, false),
            "PrivateIdentifier" | "BreakStatement" | "ContinueStatement" | "MetaProperty" => {}
            "UpdateExpression" => match ast.child(n, "argument") {
                Some(a) if is_pattern(ast, a) => {
                    self.referencing(Some(a), RW, None, None, false, false)
                }
                _ => self.visit_children(n),
            },
            "MemberExpression" => {
                self.visit(ast.child(n, "object"));
                if ast.bool(n, "computed") {
                    self.visit(ast.child(n, "property"));
                }
            }
            "Property" | "MethodDefinition" => {
                if ast.bool(n, "computed") {
                    self.visit(ast.child(n, "key"));
                }
                let is_method = ast.is(n, "MethodDefinition");
                let prev = self.inner_method;
                if is_method {
                    self.inner_method = true;
                }
                self.visit(ast.child(n, "value"));
                if is_method {
                    self.inner_method = prev;
                }
            }
            "PropertyDefinition" => {
                if ast.bool(n, "computed") {
                    self.visit(ast.child(n, "key"));
                }
                if let Some(v) = ast.child(n, "value") {
                    self.nest(ScopeType::ClassFieldInitializer, v, true);
                    self.visit(Some(v));
                    self.close(v);
                }
            }
            "StaticBlock" => {
                self.nest(ScopeType::ClassStaticBlock, n, true);
                self.visit_children(n);
                self.close(n);
            }
            "LabeledStatement" => self.visit(ast.child(n, "body")),
            "ForStatement" => {
                if let Some(init) = ast.child(n, "init")
                    && ast.is(init, "VariableDeclaration")
                    && ast.str(init, "kind") != Some("var")
                {
                    self.nest(ScopeType::For, n, false);
                }
                self.visit_children(n);
                self.close(n);
            }
            "ClassExpression" | "ClassDeclaration" => self.visit_class(n),
            "BlockStatement" => {
                self.nest(ScopeType::Block, n, false);
                self.visit_children(n);
                self.close(n);
            }
            "ThisExpression" => {
                let vs = self.sm.scope(self.cur()).variable_scope;
                self.scope_mut(vs).this_found = true;
            }
            "WithStatement" => {
                self.visit(ast.child(n, "object"));
                self.nest(ScopeType::With, n, false);
                self.visit(ast.child(n, "body"));
                self.close(n);
            }
            "VariableDeclaration" => {
                let cur = self.cur();
                let target = if ast.str(n, "kind") == Some("var") {
                    self.sm.scope(cur).variable_scope
                } else {
                    cur
                };
                let decls: Vec<Option<NodeId>> = ast.list(n, "declarations").to_vec();
                for (i, d) in decls.iter().enumerate() {
                    let Some(d) = *d else { continue };
                    self.visit_variable_declaration(target, n, d, i);
                    if let Some(init) = ast.child(d, "init") {
                        self.visit(Some(init));
                    }
                }
            }
            "SwitchStatement" => {
                self.visit(ast.child(n, "discriminant"));
                self.nest(ScopeType::Switch, n, false);
                for c in ast.nodes_of(n, "cases").collect::<Vec<_>>() {
                    self.visit(Some(c));
                }
                self.close(n);
            }
            "FunctionDeclaration" | "FunctionExpression" | "ArrowFunctionExpression" => {
                self.visit_function(n)
            }
            "ForOfStatement" | "ForInStatement" => self.visit_for_in(n),
            "ImportDeclaration" => {
                for spec in ast.nodes_of(n, "specifiers").collect::<Vec<_>>() {
                    if let Some(local) = ast.child(spec, "local") {
                        let (events, _) = self.collect_pattern(Some(local));
                        for e in events {
                            let s = self.cur();
                            let mut d = def(DefType::ImportBinding, e.pattern, spec);
                            d.parent = Some(n);
                            self.define(s, Some(e.pattern), d);
                        }
                    }
                }
            }
            "ExportAllDeclaration" | "ExportDefaultDeclaration" | "ExportNamedDeclaration" => {
                if ast.child(n, "source").is_some() {
                    return;
                }
                if let Some(d) = ast.child(n, "declaration") {
                    self.visit(Some(d));
                    return;
                }
                self.visit_children(n);
            }
            "ExportSpecifier" => self.visit(ast.child(n, "local")),
            "JSXIdentifier" => {
                if self.jsx && ast.name(n) != Some("this") {
                    self.referencing(Some(n), 0, None, None, false, false);
                }
            }
            "JSXMemberExpression" => self.visit(ast.child(n, "object")),
            "JSXElement" => {
                if self.jsx {
                    self.visit(ast.child(n, "openingElement"));
                    for c in ast.nodes_of(n, "children").collect::<Vec<_>>() {
                        self.visit(Some(c));
                    }
                } else {
                    self.visit_children(n);
                }
            }
            "JSXOpeningElement" => {
                if self.jsx
                    && let Some(name) = ast.child(n, "name")
                {
                    let is_component_name = ast.is(name, "JSXIdentifier")
                        && ast
                            .name(name)
                            .and_then(|s| s.chars().next())
                            .is_some_and(|c| c.to_uppercase().eq(std::iter::once(c)));
                    if is_component_name || ast.is(name, "JSXMemberExpression") {
                        self.visit(Some(name));
                    }
                }
                for a in ast.nodes_of(n, "attributes").collect::<Vec<_>>() {
                    self.visit(Some(a));
                }
            }
            "JSXAttribute" => self.visit(ast.child(n, "value")),
            "JSXExpressionContainer" => self.visit(ast.child(n, "expression")),
            "JSXNamespacedName" => {
                self.visit(ast.child(n, "namespace"));
                self.visit(ast.child(n, "name"));
            }
            _ => self.visit_children(n),
        }
    }

    fn assignment_expression(&mut self, n: NodeId) {
        let ast = self.ast;
        let left = ast.child(n, "left");
        let right = ast.child(n, "right");
        match left {
            Some(l) if is_pattern(ast, l) => {
                if ast.str(n, "operator") == Some("=") {
                    let (events, rhs) = self.collect_pattern(Some(l));
                    for e in events {
                        let mig = if !self.sm.scope(self.cur()).is_strict {
                            Some((e.pattern, n))
                        } else {
                            None
                        };
                        self.referencing_default_value(e.pattern, &e.assignments, mig, false);
                        self.referencing(Some(e.pattern), WRITE, right, mig, !e.top_level, false);
                    }
                    self.visit_rhs(rhs);
                } else {
                    self.referencing(Some(l), RW, right, None, false, false);
                }
            }
            _ => self.visit(left),
        }
        self.visit(right);
    }

    fn visit_variable_declaration(
        &mut self,
        target: ScopeId,
        node: NodeId,
        decl: NodeId,
        index: usize,
    ) {
        let ast = self.ast;
        let init = ast.child(decl, "init");
        let kind = ast.str(node, "kind").map(str::to_string);
        let (events, rhs) = self.collect_pattern(ast.child(decl, "id"));
        for e in events {
            let d = Definition {
                kind: DefType::Variable,
                name: e.pattern,
                node: decl,
                parent: Some(node),
                index: Some(index),
                decl_kind: kind.clone(),
                rest: None,
            };
            self.define(target, Some(e.pattern), d);
            self.referencing_default_value(e.pattern, &e.assignments, None, true);
            if init.is_some() {
                self.referencing(Some(e.pattern), WRITE, init, None, !e.top_level, true);
            }
        }
        self.visit_rhs(rhs);
    }

    fn visit_function(&mut self, n: NodeId) {
        let ast = self.ast;
        if ast.is(n, "FunctionDeclaration") {
            let cur = self.cur();
            if let Some(id) = ast.child(n, "id") {
                self.define(cur, Some(id), def(DefType::FunctionName, id, n));
            }
        }
        if ast.is(n, "FunctionExpression") && ast.child(n, "id").is_some() {
            self.nest(ScopeType::FunctionExpressionName, n, false);
        }
        let inner = self.inner_method;
        self.nest(ScopeType::Function, n, inner);
        let params: Vec<Option<NodeId>> = ast.list(n, "params").to_vec();
        for (i, p) in params.iter().enumerate() {
            let (events, rhs) = self.collect_pattern(*p);
            for e in events {
                let s = self.cur();
                let d = Definition {
                    kind: DefType::Parameter,
                    name: e.pattern,
                    node: n,
                    parent: None,
                    index: Some(i),
                    decl_kind: None,
                    rest: Some(e.rest),
                };
                self.define(s, Some(e.pattern), d);
                self.referencing_default_value(e.pattern, &e.assignments, None, true);
            }
            self.visit_rhs(rhs);
        }
        if let Some(body) = ast.child(n, "body") {
            if ast.is(body, "BlockStatement") {
                self.visit_children(body);
            } else {
                self.visit(Some(body));
            }
        }
        self.close(n);
    }

    fn visit_class(&mut self, n: NodeId) {
        let ast = self.ast;
        let id = ast.child(n, "id");
        if ast.is(n, "ClassDeclaration") {
            let cur = self.cur();
            if let Some(id) = id {
                self.define(cur, Some(id), def(DefType::ClassName, id, n));
            }
        }
        let s = self.nest(ScopeType::Class, n, false);
        if let Some(id) = id {
            self.define(s, Some(id), def(DefType::ClassName, id, n));
        }
        self.visit(ast.child(n, "superClass"));
        self.visit(ast.child(n, "body"));
        self.close(n);
    }

    fn visit_for_in(&mut self, n: NodeId) {
        let ast = self.ast;
        let left = ast.child(n, "left");
        let right = ast.child(n, "right");
        let left_decl = left.filter(|l| ast.is(*l, "VariableDeclaration"));
        if let Some(l) = left_decl
            && ast.str(l, "kind") != Some("var")
        {
            self.nest(ScopeType::For, n, false);
        }
        if let Some(l) = left_decl {
            self.visit(Some(l));
            let first = ast.list(l, "declarations").first().copied().flatten();
            let (events, _) = self.collect_pattern(first.and_then(|d| ast.child(d, "id")));
            for e in events {
                self.referencing(Some(e.pattern), WRITE, right, None, true, true);
            }
        } else {
            let (events, rhs) = self.collect_pattern(left);
            for e in events {
                let mig = if !self.sm.scope(self.cur()).is_strict {
                    Some((e.pattern, n))
                } else {
                    None
                };
                self.referencing_default_value(e.pattern, &e.assignments, mig, false);
                self.referencing(Some(e.pattern), WRITE, right, mig, true, false);
            }
            self.visit_rhs(rhs);
        }
        self.visit(right);
        self.visit(ast.child(n, "body"));
        self.close(n);
    }
}

fn def(kind: DefType, name: NodeId, node: NodeId) -> Definition {
    Definition {
        kind,
        name,
        node,
        parent: None,
        index: None,
        decl_kind: None,
        rest: None,
    }
}
