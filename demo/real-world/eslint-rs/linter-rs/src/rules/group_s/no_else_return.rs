//! lib/rules/no-else-return.js

use crate::ast::NodeId;
use crate::linter::Cx;
use crate::report::Fix;
use crate::rule::{Report, fixer};
use crate::rules::{Rule, RuleDef};
use crate::scope::{ScopeId, ScopeType};
use crate::source::TokKind;
use crate::tokens::N;
use crate::utils::get_upper_function;
use serde_json::Value;

pub static DEF: RuleDef = RuleDef {
    name: "no-else-return",
    messages: &[("unexpected", "Unnecessary 'else' after 'return'.")],
    has_default_options: true,
    create,
};

struct R {
    allow_else_if: bool,
}

fn create(options: &[Value]) -> Result<Box<dyn Rule>, String> {
    let o = super::common::bool_object(options, &["allowElseIf"])?;
    Ok(Box::new(R {
        allow_else_if: o[0].unwrap_or(true),
    }))
}

fn is_safe_to_declare(cx: &Cx, names: &[String], scope: ScopeId) -> bool {
    if names.is_empty() {
        return true;
    }
    let sm = cx.sm();
    let ast = &cx.sc.ast;
    let s = sm.scope(scope);
    let function_scope = s.variable_scope;
    let has = |n: &str| names.iter().any(|x| x == n);
    if s.variables
        .iter()
        .any(|v| !sm.var(*v).defs.is_empty() && has(&sm.var(*v).name))
    {
        return false;
    }
    if scope != function_scope
        && let Some(up) = s.upper
        && sm.scope(up).kind == ScopeType::Catch
        && sm.scope(up).variables.iter().any(|v| has(&sm.var(*v).name))
    {
        return false;
    }
    if s.variables.iter().any(|v| {
        let var = sm.var(*v);
        var.defs.is_empty() && !var.references.is_empty() && has(&var.name)
    }) {
        return false;
    }
    if s.through
        .iter()
        .any(|r| ast.name(sm.reference(*r).identifier).is_some_and(has))
    {
        return false;
    }
    if scope != function_scope {
        let (bs, be) = ast.range(s.block);
        let clash = sm
            .scope(function_scope)
            .variables
            .iter()
            .filter(|v| has(&sm.var(**v).name))
            .any(|v| {
                sm.var(*v).defs.iter().any(|d| {
                    let (ds, de) = ast.range(d.node);
                    bs <= ds && de <= be
                })
            });
        if clash {
            return false;
        }
    }
    true
}

fn is_safe_from_name_collisions(cx: &Cx, node: NodeId, scope: ScopeId) -> bool {
    let ast = &cx.sc.ast;
    if ast.is(node, "FunctionDeclaration") {
        return false;
    }
    if !ast.is(node, "BlockStatement") {
        return true;
    }
    let sm = cx.sm();
    let Some(else_scope) = sm
        .scope(scope)
        .child_scopes
        .iter()
        .copied()
        .find(|c| sm.scope(*c).block == node)
    else {
        return true;
    };
    let names: Vec<String> = sm
        .scope(else_scope)
        .variables
        .iter()
        .map(|v| sm.var(*v).name.clone())
        .collect();
    is_safe_to_declare(cx, &names, scope)
}

fn unsafe_start(v: &str) -> bool {
    v.starts_with(['(', '[', '/', '+', '`', '-'])
}

fn fix(cx: &Cx, else_node: NodeId, scope: ScopeId) -> Option<Fix> {
    let sc = cx.sc;
    let ast = &sc.ast;
    if !is_safe_from_name_collisions(cx, else_node, scope) {
        return None;
    }
    let start = sc.first_token(else_node, N)?;
    let else_tok = sc.token_before_tok(start, N)?;
    let source = sc.node_text(else_node);
    let last_if = sc.token_before_tok(else_tok, N)?;
    let brace = start.kind == TokKind::Punctuator && start.value == "{";
    let first_of_block = if brace {
        sc.token_after_tok(start, N)?
    } else {
        start
    };
    let parent = ast.parent(else_node)?;
    let if_unsafe = !ast
        .child(parent, "consequent")
        .is_some_and(|c| ast.is(c, "BlockStatement"))
        && last_if.value != ";";
    if if_unsafe && unsafe_start(&first_of_block.value) {
        return None;
    }
    let end = sc.last_token(else_node, N)?;
    let last_of_block = sc.token_before_tok(end, N)?;
    if last_of_block.value != ";" {
        let next = sc.token_after_tok(end, N);
        let next_unsafe = next.is_some_and(|t| unsafe_start(&t.value));
        let same_line =
            next.is_some_and(|t| sc.loc_of(t.start).line == sc.loc_of(last_of_block.start).line);
        if next_unsafe || (same_line && next.is_some_and(|t| t.value != "}")) {
            return None;
        }
    }
    let fixed: String = if brace {
        let u: Vec<u16> = source.encode_utf16().collect();
        if u.len() < 2 {
            return None;
        }
        String::from_utf16(&u[1..u.len() - 1]).ok()?
    } else {
        source.to_string()
    };
    let retained = match get_upper_function(ast, else_node) {
        Some(f) => ast.range(f),
        None => ast.range(sc.root),
    };
    let range = (else_tok.start, ast.range(else_node).1);
    let actual = (retained.0.min(range.0), retained.1.max(range.1));
    let text = format!(
        "{}{}{}",
        sc.slice16(actual.0, range.0),
        fixed,
        sc.slice16(range.1, actual.1)
    );
    Some(fixer::replace_range(actual, text))
}

fn display_report(cx: &mut Cx, else_node: NodeId) {
    let Some(parent) = cx.sc.ast.parent(else_node) else {
        return;
    };
    let scope = cx.get_scope(parent);
    let mut r = Report::node(else_node, "unexpected");
    if let Some(f) = fix(cx, else_node, scope) {
        r = r.fix(f);
    }
    cx.report(r);
}

fn naive_has_return(cx: &Cx, n: NodeId) -> bool {
    let ast = &cx.sc.ast;
    if ast.is(n, "BlockStatement") {
        return ast
            .list(n, "body")
            .last()
            .copied()
            .flatten()
            .is_some_and(|l| ast.is(l, "ReturnStatement"));
    }
    ast.is(n, "ReturnStatement")
}

fn check_for_return_or_if(cx: &Cx, n: NodeId) -> bool {
    let ast = &cx.sc.ast;
    if ast.is(n, "ReturnStatement") {
        return true;
    }
    ast.is(n, "IfStatement")
        && match (ast.child(n, "consequent"), ast.child(n, "alternate")) {
            (Some(c), Some(a)) => naive_has_return(cx, a) && naive_has_return(cx, c),
            _ => false,
        }
}

fn always_returns(cx: &Cx, n: NodeId) -> bool {
    let ast = &cx.sc.ast;
    if ast.is(n, "BlockStatement") {
        return ast
            .nodes_of(n, "body")
            .any(|s| check_for_return_or_if(cx, s));
    }
    check_for_return_or_if(cx, n)
}

impl Rule for R {
    fn listeners(&self) -> &'static [&'static str] {
        &["IfStatement:exit"]
    }

    fn on(&mut self, _: usize, node: NodeId, cx: &mut Cx) {
        let ast = &cx.sc.ast;
        let Some(parent) = ast.parent(node) else {
            return;
        };
        if !matches!(
            ast.kind(parent),
            "Program" | "BlockStatement" | "StaticBlock" | "SwitchCase"
        ) {
            return;
        }
        if self.allow_else_if {
            let mut consequents = Vec::new();
            let mut alternate = None;
            let mut cur = node;
            while ast.is(cur, "IfStatement") {
                let Some(a) = ast.child(cur, "alternate") else {
                    return;
                };
                if let Some(c) = ast.child(cur, "consequent") {
                    consequents.push(c);
                }
                alternate = Some(a);
                cur = a;
            }
            if let Some(a) = alternate
                && consequents.iter().all(|c| always_returns(cx, *c))
            {
                display_report(cx, a);
            }
        } else if let Some(a) = ast.child(node, "alternate")
            && ast
                .child(node, "consequent")
                .is_some_and(|c| always_returns(cx, c))
        {
            display_report(cx, a);
        }
    }
}
