//! lib/rules/no-redeclare.js

use super::common::bool_object;
use crate::ast::NodeId;
use crate::linter::Cx;
use crate::rule::Report;
use crate::rules::{Rule, RuleDef};
use crate::scope::{ScopeId, ScopeType};
use crate::source::{Loc, Pos};
use serde_json::Value;

pub static DEF: RuleDef = RuleDef {
    name: "no-redeclare",
    messages: &[
        ("redeclared", "'{{id}}' is already defined."),
        ("redeclaredAsBuiltin", "'{{id}}' is already defined as a built-in global variable."),
        ("redeclaredBySyntax", "'{{id}}' is already defined by a variable declaration."),
    ],
    has_default_options: true,
    create,
};

struct R {
    builtin_globals: bool,
}

fn create(options: &[Value]) -> Result<Box<dyn Rule>, String> {
    let o = bool_object(options, &["builtinGlobals"])?;
    Ok(Box::new(R { builtin_globals: o[0].unwrap_or(true) }))
}

#[derive(PartialEq, Clone, Copy)]
enum Kind {
    Builtin,
    Syntax,
    Comment,
}

/// ast-utils `getNameLocationInGlobalDirectiveComment(sourceCode, comment, name)`.
pub fn name_location_in_global_comment(cx: &Cx, ci: usize, name: &str) -> Loc {
    let c = &cx.sc.comments[ci];
    let value: Vec<u16> = c.value.encode_utf16().collect();
    let name16: Vec<u16> = name.encode_utf16().collect();
    let global: Vec<u16> = "global".encode_utf16().collect();
    let idx = value.windows(global.len()).position(|w| w == global.as_slice()).map(|i| i as i64).unwrap_or(-1);
    let from = (idx + 6).max(0) as usize;
    let is_sep = |u: u16| u == b',' as u16 || char::from_u32(u as u32).is_some_and(crate::directives::is_ws);
    let mut found = None;
    let mut i = from;
    while i < value.len() {
        if is_sep(value[i]) && value[i + 1..].starts_with(&name16) {
            let after = i + 1 + name16.len();
            if after == value.len() || is_sep(value[after]) || value[after] == b':' as u16 {
                found = Some(i);
                break;
            }
        }
        i += 1;
    }
    let start = cx.sc.loc_of(c.start + 2 + found.map(|m| m as u32 + 1).unwrap_or(0));
    let end = Pos { line: start.line, column: start.column + if found.is_some() { name16.len() as u32 } else { 1 } };
    Loc { start, end }
}

impl R {
    fn find_variables_in_scope(&self, cx: &mut Cx, scope: ScopeId) {
        let sm = cx.sm();
        for &v in &sm.scope(scope).variables {
            let var = sm.var(v);
            let is_builtin = self.builtin_globals
                && matches!(var.eslint_implicit_global_setting.as_deref(), Some("readonly" | "writable"));
            let comments = var.eslint_explicit_global_comments.as_deref().unwrap_or(&[]);
            let count = is_builtin as usize + var.identifiers.len() + comments.len();
            if count < 2 {
                continue;
            }
            let mut decls: Vec<(Kind, Option<Loc>, Option<NodeId>)> = Vec::new();
            if is_builtin {
                decls.push((Kind::Builtin, None, None));
            }
            for &id in &var.identifiers {
                decls.push((Kind::Syntax, None, Some(id)));
            }
            for &ci in comments {
                decls.push((Kind::Comment, Some(name_location_in_global_comment(cx, ci, &var.name)), None));
            }
            let first = decls[0].0;
            let detail = if first == Kind::Builtin { "redeclaredAsBuiltin" } else { "redeclaredBySyntax" };
            for &(kind, loc, node) in &decls[1..] {
                let mid = if kind == first { "redeclared" } else { detail };
                let r = match (loc, node) {
                    (Some(l), _) => Report::loc(l, mid),
                    (None, Some(n)) => Report::node(n, mid),
                    _ => continue,
                };
                cx.report(r.data("id", var.name.clone()));
            }
        }
    }
}

impl Rule for R {
    fn listeners(&self) -> &'static [&'static str] {
        &[
            "Program",
            "FunctionDeclaration",
            "FunctionExpression",
            "ArrowFunctionExpression",
            "StaticBlock",
            "BlockStatement",
            "ForStatement",
            "ForInStatement",
            "ForOfStatement",
            "SwitchStatement",
        ]
    }

    fn on(&mut self, listener: usize, node: NodeId, cx: &mut Cx) {
        let sm = cx.sm();
        let scope = cx.get_scope(node);
        if listener == 0 {
            self.find_variables_in_scope(cx, scope);
            let s = sm.scope(scope);
            if s.kind == ScopeType::Global
                && let Some(&child) = s.child_scopes.first()
                && s.block == sm.scope(child).block
            {
                self.find_variables_in_scope(cx, child);
            }
        } else if sm.scope(scope).block == node {
            self.find_variables_in_scope(cx, scope);
        }
    }
}
