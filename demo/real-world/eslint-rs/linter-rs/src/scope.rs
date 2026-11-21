//! Scope analysis for rules: eslint-scope 9.1.2 (`scope/analyze.rs`) on the `Ast` arena, plus
//! the globals `SourceCode#applyLanguageOptions`, `applyInlineConfig` and `finalize` add.

mod analyze;
mod globals;

use crate::ast::{Ast, NodeId};
use crate::linter::{Cx, Options, RuleConf, Unsupported};
use crate::source::{SourceCode, SourceType};
use std::cell::{Cell, OnceCell};
use std::collections::HashMap;

pub type ScopeId = u32;
pub type VarId = u32;
pub type RefId = u32;

pub const READ: u8 = 1;
pub const WRITE: u8 = 2;
pub const RW: u8 = 3;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ScopeType {
    Global,
    Module,
    Function,
    FunctionExpressionName,
    Block,
    Switch,
    Catch,
    With,
    For,
    Class,
    ClassFieldInitializer,
    ClassStaticBlock,
}

impl ScopeType {
    pub fn as_str(self) -> &'static str {
        match self {
            ScopeType::Global => "global",
            ScopeType::Module => "module",
            ScopeType::Function => "function",
            ScopeType::FunctionExpressionName => "function-expression-name",
            ScopeType::Block => "block",
            ScopeType::Switch => "switch",
            ScopeType::Catch => "catch",
            ScopeType::With => "with",
            ScopeType::For => "for",
            ScopeType::Class => "class",
            ScopeType::ClassFieldInitializer => "class-field-initializer",
            ScopeType::ClassStaticBlock => "class-static-block",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DefType {
    CatchClause,
    Parameter,
    FunctionName,
    ClassName,
    Variable,
    ImportBinding,
    ImplicitGlobalVariable,
}

impl DefType {
    pub fn as_str(self) -> &'static str {
        match self {
            DefType::CatchClause => "CatchClause",
            DefType::Parameter => "Parameter",
            DefType::FunctionName => "FunctionName",
            DefType::ClassName => "ClassName",
            DefType::Variable => "Variable",
            DefType::ImportBinding => "ImportBinding",
            DefType::ImplicitGlobalVariable => "ImplicitGlobalVariable",
        }
    }
}

#[derive(Debug, Clone)]
pub struct Definition {
    pub kind: DefType,
    pub name: NodeId,
    pub node: NodeId,
    pub parent: Option<NodeId>,
    pub index: Option<usize>,
    /// `def.kind`: the declaration kind (`var`, `let`, ...) for `Variable` definitions.
    pub decl_kind: Option<String>,
    /// `ParameterDefinition#rest`.
    pub rest: Option<bool>,
}

#[derive(Debug)]
pub struct Variable {
    pub name: String,
    pub identifiers: Vec<NodeId>,
    pub references: Vec<RefId>,
    pub defs: Vec<Definition>,
    pub tainted: bool,
    pub stack: bool,
    pub scope: ScopeId,
    pub eslint_implicit_global_setting: Option<String>,
    pub eslint_explicit_global: bool,
    /// Comment indices (into `sc.comments`) of the `/* global */` comments declaring it.
    pub eslint_explicit_global_comments: Option<Vec<usize>>,
    /// `variable.writeable`: set on variables `finalize` added (`None` = `undefined`).
    pub writeable: Option<bool>,
    pub eslint_used: Cell<bool>,
    pub eslint_exported: bool,
}

#[derive(Debug, Clone)]
pub struct Reference {
    pub identifier: NodeId,
    pub from: ScopeId,
    pub tainted: bool,
    pub resolved: Option<VarId>,
    pub flag: u8,
    pub write_expr: Option<NodeId>,
    /// Set only for write references, as in eslint-scope.
    pub partial: Option<bool>,
    pub init: Option<bool>,
    pub(crate) maybe_implicit_global: Option<(NodeId, NodeId)>,
}

impl Reference {
    pub fn is_write(&self) -> bool {
        self.flag & WRITE != 0
    }
    pub fn is_read(&self) -> bool {
        self.flag & READ != 0
    }
    pub fn is_read_only(&self) -> bool {
        self.flag == READ
    }
    pub fn is_write_only(&self) -> bool {
        self.flag == WRITE
    }
    pub fn is_read_write(&self) -> bool {
        self.flag == RW
    }
}

#[derive(Debug, Default)]
pub struct Implicit {
    pub set: HashMap<String, VarId>,
    pub variables: Vec<VarId>,
    pub left: Vec<RefId>,
}

#[derive(Debug)]
pub struct Scope {
    pub kind: ScopeType,
    pub set: HashMap<String, VarId>,
    pub dynamic: bool,
    pub block: NodeId,
    pub through: Vec<RefId>,
    pub variables: Vec<VarId>,
    pub references: Vec<RefId>,
    pub variable_scope: ScopeId,
    pub function_expression_scope: bool,
    pub direct_call_to_eval_scope: bool,
    pub this_found: bool,
    pub upper: Option<ScopeId>,
    pub is_strict: bool,
    pub child_scopes: Vec<ScopeId>,
    /// Global scope only.
    pub implicit: Implicit,
    pub(crate) left: Option<Vec<RefId>>,
}

#[derive(Debug, Default)]
pub struct ScopeManager {
    pub scopes: Vec<Scope>,
    pub vars: Vec<Variable>,
    pub refs: Vec<Reference>,
    pub(crate) node_to_scope: HashMap<NodeId, Vec<ScopeId>>,
    pub(crate) declared: HashMap<NodeId, Vec<VarId>>,
    pub(crate) current: Option<ScopeId>,
}

impl ScopeManager {
    pub fn global_scope(&self) -> ScopeId {
        0
    }

    pub fn scope(&self, s: ScopeId) -> &Scope {
        &self.scopes[s as usize]
    }

    pub fn var(&self, v: VarId) -> &Variable {
        &self.vars[v as usize]
    }

    pub fn reference(&self, r: RefId) -> &Reference {
        &self.refs[r as usize]
    }

    /// `ScopeManager#getDeclaredVariables`.
    pub fn declared_variables(&self, node: NodeId) -> &[VarId] {
        self.declared.get(&node).map(Vec::as_slice).unwrap_or(&[])
    }

    /// `ScopeManager#acquire(node, inner)`.
    pub fn acquire(&self, node: NodeId, inner: bool) -> Option<ScopeId> {
        let scopes = self.node_to_scope.get(&node)?;
        if scopes.is_empty() {
            return None;
        }
        if scopes.len() == 1 {
            return Some(scopes[0]);
        }
        let ok = |s: &ScopeId| {
            let sc = self.scope(*s);
            !(sc.kind == ScopeType::Function && sc.function_expression_scope)
        };
        if inner {
            scopes.iter().rev().find(|s| ok(s)).copied()
        } else {
            scopes.iter().find(|s| ok(s)).copied()
        }
    }

    pub fn acquire_all(&self, node: NodeId) -> &[ScopeId] {
        self.node_to_scope
            .get(&node)
            .map(Vec::as_slice)
            .unwrap_or(&[])
    }

    /// `SourceCode#getScope(node)`.
    pub fn get_scope(&self, ast: &Ast, node: NodeId) -> ScopeId {
        let inner = ast.kind(node) != "Program";
        let mut cur = Some(node);
        while let Some(n) = cur {
            if let Some(s) = self.acquire(n, inner) {
                let sc = self.scope(s);
                if sc.kind == ScopeType::FunctionExpressionName {
                    return sc.child_scopes.first().copied().unwrap_or(s);
                }
                return s;
            }
            cur = ast.parent(n);
        }
        0
    }

    /// `scope.set.get(name)`.
    pub fn lookup(&self, s: ScopeId, name: &str) -> Option<VarId> {
        self.scope(s).set.get(name).copied()
    }

    /// `astUtils.getVariableByName(initScope, name)`.
    pub fn variable_by_name(&self, s: ScopeId, name: &str) -> Option<VarId> {
        let mut cur = Some(s);
        while let Some(c) = cur {
            if let Some(v) = self.lookup(c, name) {
                return Some(v);
            }
            cur = self.scope(c).upper;
        }
        None
    }

    /// `Scope#resolve(ident)`.
    pub fn resolve(&self, s: ScopeId, ident: NodeId) -> Option<RefId> {
        self.scope(s)
            .references
            .iter()
            .copied()
            .find(|r| self.reference(*r).identifier == ident)
    }

    /// `SourceCode#markVariableAsUsed(name, refNode)`.
    pub fn mark_variable_as_used(
        &self,
        ast: &Ast,
        root: NodeId,
        name: &str,
        ref_node: NodeId,
    ) -> bool {
        let current = self.get_scope(ast, ref_node);
        let mut initial = current;
        let cs = self.scope(current);
        if cs.kind == ScopeType::Global
            && let Some(&first) = cs.child_scopes.first()
            && self.scope(first).block == root
        {
            initial = first;
        }
        let mut cur = Some(initial);
        while let Some(c) = cur {
            if let Some(v) = self
                .scope(c)
                .variables
                .iter()
                .copied()
                .find(|v| self.var(*v).name == name)
            {
                self.var(v).eslint_used.set(true);
                return true;
            }
            cur = self.scope(c).upper;
        }
        false
    }

    /// `SourceCode#isGlobalReference(node)`.
    pub fn is_global_reference(&self, ast: &Ast, node: NodeId) -> bool {
        if !ast.is(node, "Identifier") {
            return false;
        }
        let Some(name) = ast.name(node) else {
            return false;
        };
        let Some(v) = self.lookup(0, name) else {
            return false;
        };
        let var = self.var(v);
        if !var.defs.is_empty() {
            return false;
        }
        var.references
            .iter()
            .any(|r| self.reference(*r).identifier == node)
    }

    /// Whether `scope` is a strict-mode scope (`scope.isStrict`).
    pub fn is_strict(&self, s: ScopeId) -> bool {
        self.scope(s).is_strict
    }
}

pub struct ScopeData {
    pub globals: Vec<(String, String)>,
    pub inline_globals: Vec<(String, String, usize)>,
    pub exported: Vec<String>,
    source_type: SourceType,
    jsx: bool,
    inline_comments: Vec<(String, String, Vec<usize>)>,
    manager: OnceCell<ScopeManager>,
}

impl ScopeData {
    pub fn new(
        sc: &SourceCode,
        o: &Options,
        inline_globals: &[(String, String, usize)],
        exported: &[String],
        _rules: &[(String, RuleConf)],
    ) -> Result<ScopeData, Unsupported> {
        let inline_comments = if o.allow_inline_config {
            globals::inline_global_comments(sc)
        } else {
            Vec::new()
        };
        Ok(ScopeData {
            globals: o.globals.clone(),
            inline_globals: inline_globals.to_vec(),
            exported: exported.to_vec(),
            source_type: o.source_type,
            jsx: o.jsx,
            inline_comments,
            manager: OnceCell::new(),
        })
    }

    /// The analyzed scopes, built on first use.
    pub fn manager(&self, sc: &SourceCode) -> &ScopeManager {
        self.manager.get_or_init(|| {
            let mut sm = analyze::analyze(&sc.ast, sc.root, self.source_type, self.jsx);
            globals::finalize(
                &mut sm,
                &sc.ast,
                self.source_type,
                &self.globals,
                &self.inline_comments,
                &self.exported,
            );
            sm
        })
    }
}

impl<'a> Cx<'a> {
    /// `context.sourceCode.scopeManager`, analyzed on first use.
    pub fn sm(&self) -> &'a ScopeManager {
        self.scope.manager(self.sc)
    }

    /// `sourceCode.getScope(node)`.
    pub fn get_scope(&self, node: NodeId) -> ScopeId {
        self.sm().get_scope(&self.sc.ast, node)
    }

    /// `sourceCode.getDeclaredVariables(node)`.
    pub fn declared_variables(&self, node: NodeId) -> &'a [VarId] {
        self.sm().declared_variables(node)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::source::ParseOptions;

    fn build(code: &str, st: SourceType) -> (SourceCode, ScopeManager) {
        let sc = SourceCode::parse(
            code,
            &ParseOptions {
                source_type: st,
                jsx: false,
                global_return: false,
            },
        )
        .ok()
        .unwrap_or_else(|| panic!("parse"));
        let mut sm = analyze::analyze(&sc.ast, sc.root, st, false);
        globals::finalize(&mut sm, &sc.ast, st, &[], &[], &[]);
        (sc, sm)
    }

    fn names(sm: &ScopeManager, refs: &[RefId], sc: &SourceCode) -> Vec<String> {
        refs.iter()
            .map(|r| {
                sc.ast
                    .name(sm.reference(*r).identifier)
                    .unwrap_or("")
                    .to_string()
            })
            .collect()
    }

    #[test]
    fn closure_reference_resolves_through() {
        let (sc, sm) = build(
            "let a = 1; function f() { return a + b; }",
            SourceType::Module,
        );
        let module = sm.scope(0).child_scopes[0];
        assert_eq!(sm.scope(module).kind, ScopeType::Module);
        let f = sm.scope(module).child_scopes[0];
        assert_eq!(sm.scope(f).kind, ScopeType::Function);
        assert_eq!(names(&sm, &sm.scope(f).through, &sc), vec!["a", "b"]);
        assert_eq!(names(&sm, &sm.scope(0).through, &sc), vec!["b"]);
        let a = sm.lookup(module, "a").unwrap_or(u32::MAX);
        assert_eq!(sm.var(a).references.len(), 2);
        assert!(!sm.var(a).stack);
    }

    #[test]
    fn var_hoists_into_function_scope() {
        let (_sc, sm) = build("function f() { { var x = 1; let y; } }", SourceType::Module);
        let module = sm.scope(0).child_scopes[0];
        let f = sm.scope(module).child_scopes[0];
        let block = sm.scope(f).child_scopes[0];
        assert_eq!(sm.scope(block).kind, ScopeType::Block);
        assert!(sm.lookup(f, "x").is_some());
        assert!(sm.lookup(block, "x").is_none());
        assert!(sm.lookup(block, "y").is_some());
        let names: Vec<&str> = sm
            .scope(f)
            .variables
            .iter()
            .map(|v| sm.var(*v).name.as_str())
            .collect();
        assert_eq!(names, vec!["arguments", "x"]);
    }

    #[test]
    fn class_reference_in_tdz_resolves_to_class_scope() {
        let (_sc, sm) = build("class A { static x = A; }", SourceType::Module);
        let module = sm.scope(0).child_scopes[0];
        let outer = sm.lookup(module, "A").unwrap_or(u32::MAX);
        let class = sm.scope(module).child_scopes[0];
        assert_eq!(sm.scope(class).kind, ScopeType::Class);
        let inner = sm.lookup(class, "A").unwrap_or(u32::MAX);
        assert_eq!(sm.var(inner).references.len(), 1);
        assert!(sm.var(outer).references.is_empty());
        let field = sm.scope(class).child_scopes[0];
        assert_eq!(sm.scope(field).kind, ScopeType::ClassFieldInitializer);
        assert!(sm.scope(field).is_strict);
    }

    #[test]
    fn commonjs_gets_function_scope_and_globals() {
        let (sc, sm) = build("var a = require('x'); b = 1;", SourceType::CommonJs);
        let f = sm.scope(0).child_scopes[0];
        assert_eq!(sm.scope(f).kind, ScopeType::Function);
        assert_eq!(sm.scope(f).block, sc.root);
        assert!(sm.lookup(f, "a").is_some());
        assert!(sm.lookup(0, "require").is_some());
        assert_eq!(names(&sm, &sm.scope(0).through, &sc), vec!["b"]);
        assert_eq!(sm.scope(0).implicit.variables.len(), 1);
    }

    #[test]
    fn default_parameter_does_not_see_body_variables() {
        let (sc, sm) = build(
            "const x = 1; function f(a = x) { const x = 2; }",
            SourceType::Module,
        );
        let module = sm.scope(0).child_scopes[0];
        let outer = sm.lookup(module, "x").unwrap_or(u32::MAX);
        assert_eq!(names(&sm, &sm.var(outer).references, &sc), vec!["x", "x"]);
    }
}
