//! lib/rules/no-extra-boolean-cast.js

use crate::ast::NodeId;
use crate::linter::Cx;
use crate::report::Fix;
use crate::rule::{Report, fixer};
use crate::rules::{Rule, RuleDef};
use crate::source::{TokKind, Token};
use crate::tokens::N;
use crate::utils::{
    can_tokens_be_adjacent, get_precedence, is_mixed_logical_and_coalesce_expressions,
    is_parenthesized_times,
};
use serde_json::Value;

pub static DEF: RuleDef = RuleDef {
    name: "no-extra-boolean-cast",
    messages: &[
        ("unexpectedCall", "Redundant Boolean call."),
        ("unexpectedNegation", "Redundant double negation."),
    ],
    has_default_options: true,
    create,
};

struct R {
    logical: bool,
    inner: bool,
    unsupported: bool,
}

fn create(options: &[Value]) -> Result<Box<dyn Rule>, String> {
    let (mut logical, mut inner) = (false, false);
    match options {
        [] => {}
        [Value::Object(o)] => {
            if o.len() > 1 {
                return Err("invalid option".into());
            }
            for (k, v) in o {
                let Value::Bool(b) = v else {
                    return Err("invalid option".into());
                };
                match k.as_str() {
                    "enforceForLogicalOperands" => logical = *b,
                    "enforceForInnerExpressions" => inner = *b,
                    _ => return Err("invalid option".into()),
                }
            }
        }
        _ => return Err("invalid option".into()),
    }
    Ok(Box::new(R {
        logical,
        inner,
        unsupported: false,
    }))
}

fn is_boolean_call(cx: &Cx, node: NodeId) -> bool {
    let ast = &cx.sc.ast;
    matches!(ast.kind(node), "CallExpression" | "NewExpression")
        && ast.child(node, "callee").is_some_and(|c| {
            ast.is(c, "Identifier")
                && ast.name(c) == Some("Boolean")
                && cx.sm().is_global_reference(ast, c)
        })
}

fn is_in_boolean_context(cx: &Cx, node: NodeId) -> bool {
    let ast = &cx.sc.ast;
    let Some(parent) = ast.parent(node) else {
        return false;
    };
    (is_boolean_call(cx, parent)
        && ast.list(parent, "arguments").first().copied().flatten() == Some(node))
        || (matches!(
            ast.kind(parent),
            "IfStatement"
                | "DoWhileStatement"
                | "WhileStatement"
                | "ConditionalExpression"
                | "ForStatement"
        ) && ast.child(parent, "test") == Some(node))
        || (ast.is(parent, "UnaryExpression") && ast.str(parent, "operator") == Some("!"))
}

impl R {
    fn is_in_flagged_context(&self, cx: &Cx, node: NodeId) -> bool {
        let ast = &cx.sc.ast;
        let Some(parent) = ast.parent(node) else {
            return false;
        };
        if ast.is(parent, "ChainExpression") {
            return self.is_in_flagged_context(cx, parent);
        }
        if (self.logical || self.inner) && ast.is(parent, "LogicalExpression") {
            let op = ast.str(parent, "operator").unwrap_or("");
            if op == "||" || op == "&&" {
                return self.is_in_flagged_context(cx, parent);
            }
            if self.inner && op == "??" && ast.child(parent, "right") == Some(node) {
                return self.is_in_flagged_context(cx, parent);
            }
        }
        if self.inner {
            if ast.is(parent, "ConditionalExpression")
                && (ast.child(parent, "consequent") == Some(node)
                    || ast.child(parent, "alternate") == Some(node))
            {
                return self.is_in_flagged_context(cx, parent);
            }
            if ast.is(parent, "SequenceExpression")
                && ast.list(parent, "expressions").last().copied().flatten() == Some(node)
            {
                return self.is_in_flagged_context(cx, parent);
            }
        }
        is_in_boolean_context(cx, node)
    }

    fn needs_parens(&mut self, cx: &Cx, previous: NodeId, node: NodeId) -> bool {
        let ast = &cx.sc.ast;
        let Some(parent) = ast.parent(previous) else {
            return false;
        };
        if ast.is(parent, "ChainExpression") {
            return self.needs_parens(cx, parent, node);
        }
        if is_parenthesized_times(cx.sc, 1, previous) {
            return false;
        }
        let p = |n: NodeId| get_precedence(ast, n);
        match ast.kind(parent) {
            "CallExpression" | "NewExpression" => ast.is(node, "SequenceExpression"),
            "IfStatement" | "DoWhileStatement" | "WhileStatement" | "ForStatement"
            | "SequenceExpression" => false,
            "ConditionalExpression" => {
                if ast.child(parent, "test") == Some(previous) {
                    p(node) <= p(parent)
                } else {
                    p(node) < 1
                }
            }
            "UnaryExpression" => p(node) < p(parent),
            "LogicalExpression" => {
                if is_mixed_logical_and_coalesce_expressions(ast, node, parent) {
                    return true;
                }
                if ast.child(parent, "left") == Some(previous) {
                    p(node) < p(parent)
                } else {
                    p(node) <= p(parent)
                }
            }
            _ => {
                self.unsupported = true;
                false
            }
        }
    }

    fn prefix(cx: &Cx, parent: NodeId, first: &Token) -> &'static str {
        match cx.sc.token_before(parent, N) {
            Some(t) if t.end == cx.sc.ast.range(parent).0 && !can_tokens_be_adjacent(t, first) => {
                " "
            }
            _ => "",
        }
    }

    fn negation_fix(&mut self, cx: &Cx, parent: NodeId, node: NodeId) -> Option<Fix> {
        let ast = &cx.sc.ast;
        if !cx.sc.comments_inside(parent).is_empty() {
            return None;
        }
        let arg = ast.child(node, "argument")?;
        if self.needs_parens(cx, parent, arg) {
            return Some(fixer::replace(
                cx.sc,
                parent,
                format!("({})", cx.sc.node_text(arg)),
            ));
        }
        let first = cx.sc.first_token(arg, N)?;
        let prefix = Self::prefix(cx, parent, first);
        Some(fixer::replace(
            cx.sc,
            parent,
            format!("{prefix}{}", cx.sc.node_text(arg)),
        ))
    }

    fn call_fix(&mut self, cx: &Cx, node: NodeId) -> Option<Fix> {
        let ast = &cx.sc.ast;
        let parent = ast.parent(node)?;
        let args = ast.list(node, "arguments");
        if args.is_empty() {
            if ast.is(parent, "UnaryExpression") && ast.str(parent, "operator") == Some("!") {
                if !cx.sc.comments_inside(parent).is_empty() {
                    return None;
                }
                let t = Token {
                    kind: TokKind::Boolean,
                    start: 0,
                    end: 4,
                    value: "true".into(),
                };
                let prefix = Self::prefix(cx, parent, &t);
                return Some(fixer::replace(cx.sc, parent, format!("{prefix}true")));
            }
            if !cx.sc.comments_inside(node).is_empty() {
                return None;
            }
            return Some(fixer::replace(cx.sc, node, "false"));
        }
        if args.len() == 1 {
            let arg = args[0]?;
            if ast.is(arg, "SpreadElement") || !cx.sc.comments_inside(node).is_empty() {
                return None;
            }
            if self.needs_parens(cx, node, arg) {
                return Some(fixer::replace(
                    cx.sc,
                    node,
                    format!("({})", cx.sc.node_text(arg)),
                ));
            }
            return Some(fixer::replace(cx.sc, node, cx.sc.node_text(arg)));
        }
        None
    }
}

impl Rule for R {
    fn listeners(&self) -> &'static [&'static str] {
        &["UnaryExpression", "CallExpression"]
    }

    fn on(&mut self, li: usize, node: NodeId, cx: &mut Cx) {
        let ast = &cx.sc.ast;
        if li == 0 {
            let Some(parent) = ast.parent(node) else {
                return;
            };
            if ast.str(node, "operator") != Some("!")
                || !ast.is(parent, "UnaryExpression")
                || ast.str(parent, "operator") != Some("!")
            {
                return;
            }
            if self.is_in_flagged_context(cx, parent) {
                let mut r = Report::node(parent, "unexpectedNegation");
                if let Some(f) = self.negation_fix(cx, parent, node) {
                    r = r.fix(f);
                }
                cx.report(r);
            }
            return;
        }
        let Some(callee) = ast.child(node, "callee") else {
            return;
        };
        if !(ast.is(callee, "Identifier")
            && ast.name(callee) == Some("Boolean")
            && cx.sm().is_global_reference(ast, callee))
        {
            return;
        }
        if self.is_in_flagged_context(cx, node) {
            let mut r = Report::node(node, "unexpectedCall");
            if let Some(f) = self.call_fix(cx, node) {
                r = r.fix(f);
            }
            cx.report(r);
        }
    }

    fn unsupported(&self) -> Option<&str> {
        self.unsupported
            .then_some("needsParens met an unexpected parent")
    }
}
