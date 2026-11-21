//! lib/rules/no-useless-constructor.js

use crate::ast::{Ast, NodeId};
use crate::linter::Cx;
use crate::report::SugVal;
use crate::rule::{Report, SuggestSpec, fixer};
use crate::rules::{Rule, RuleDef};
use crate::source::Loc;
use crate::tokens::N;
use crate::utils::{
    can_continue_expression_in_class_body, is_opening_paren, needs_preceding_semicolon,
};
use serde_json::Value;

pub static DEF: RuleDef = RuleDef {
    name: "no-useless-constructor",
    messages: &[
        ("noUselessConstructor", "Useless constructor."),
        ("removeConstructor", "Remove the constructor."),
    ],
    has_default_options: false,
    create,
};

struct R {
    unsupported: Option<&'static str>,
}

fn create(options: &[Value]) -> Result<Box<dyn Rule>, String> {
    super::no_options(options)?;
    Ok(Box::new(R { unsupported: None }))
}

fn is_identifier_pair(ast: &Ast, p: Option<NodeId>, a: Option<NodeId>) -> bool {
    match (p, a) {
        (Some(p), Some(a)) => {
            ast.is(p, "Identifier") && ast.is(a, "Identifier") && ast.name(p) == ast.name(a)
        }
        _ => false,
    }
}

fn is_valid_pair(ast: &Ast, p: NodeId, a: NodeId) -> bool {
    is_identifier_pair(ast, Some(p), Some(a))
        || (ast.is(p, "RestElement")
            && ast.is(a, "SpreadElement")
            && is_identifier_pair(ast, ast.child(p, "argument"), ast.child(a, "argument")))
}

fn is_redundant_super_call(ast: &Ast, body: &[Option<NodeId>], params: &[Option<NodeId>]) -> bool {
    let [Some(stmt)] = body else { return false };
    if !ast.is(*stmt, "ExpressionStatement") {
        return false;
    }
    let Some(call) = ast.child(*stmt, "expression") else {
        return false;
    };
    if !ast.is(call, "CallExpression")
        || !ast
            .child(call, "callee")
            .is_some_and(|c| ast.is(c, "Super"))
    {
        return false;
    }
    if !params
        .iter()
        .all(|p| p.is_some_and(|p| matches!(ast.kind(p), "Identifier" | "RestElement")))
    {
        return false;
    }
    let args = ast.list(call, "arguments");
    let spread_arguments = matches!(args, [Some(a)] if ast.is(*a, "SpreadElement")
        && ast.child(*a, "argument").is_some_and(|x| ast.is(x, "Identifier") && ast.name(x) == Some("arguments")));
    if spread_arguments {
        return true;
    }
    params.len() == args.len()
        && params.iter().zip(args).all(|(p, a)| match (p, a) {
            (Some(p), Some(a)) => is_valid_pair(ast, *p, *a),
            _ => false,
        })
}

impl Rule for R {
    fn listeners(&self) -> &'static [&'static str] {
        &["MethodDefinition"]
    }

    fn on(&mut self, _: usize, node: NodeId, cx: &mut Cx) {
        let sc = cx.sc;
        let ast = &sc.ast;
        if ast.str(node, "kind") != Some("constructor") {
            return;
        }
        let Some(value) = ast.child(node, "value") else {
            return;
        };
        if !ast.is(value, "FunctionExpression") {
            return;
        }
        if ast
            .nodes_of(value, "params")
            .any(|p| !ast.list(p, "decorators").is_empty() || ast.is(p, "TSParameterProperty"))
        {
            return;
        }
        match ast.str(node, "accessibility") {
            Some("protected" | "private") => return,
            Some("public") => {
                self.unsupported = Some("accessibility modifier");
                return;
            }
            _ => {}
        }
        let Some(body) = ast.child(value, "body") else {
            return;
        };
        let stmts = ast.list(body, "body");
        let params = ast.list(value, "params");
        let super_class = ast
            .parent(node)
            .and_then(|b| ast.parent(b))
            .and_then(|c| ast.child(c, "superClass"));
        let Some(paren) = sc.first_token_f(node, is_opening_paren) else {
            return;
        };
        let Some(before) = sc.token_before_tok(paren, N) else {
            return;
        };
        let loc = Loc {
            start: sc.loc_of(ast.range(node).0),
            end: sc.loc_of(before.end),
        };
        let useless = if super_class.is_some() {
            is_redundant_super_call(ast, stmts, params)
        } else {
            stmts.is_empty()
        };
        if !useless {
            return;
        }
        let add_semi = sc
            .token_after(node, N)
            .is_some_and(can_continue_expression_in_class_body)
            && needs_preceding_semicolon(sc, node);
        let mut r = Report::loc(loc, "noUselessConstructor");
        r.suggest.push(SuggestSpec {
            keys: vec![
                ("messageId", SugVal::Str("removeConstructor".into())),
                ("fix", SugVal::Fix),
            ],
            fix: Some(fixer::replace(sc, node, if add_semi { ";" } else { "" })),
        });
        cx.report(r);
    }

    fn unsupported(&self) -> Option<&str> {
        self.unsupported
    }
}
