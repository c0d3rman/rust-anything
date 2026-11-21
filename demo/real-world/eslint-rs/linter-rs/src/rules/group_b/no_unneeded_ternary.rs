//! lib/rules/no-unneeded-ternary.js

use crate::ast::{NodeId, Prop};
use crate::linter::Cx;
use crate::rule::{Report, fixer};
use crate::rules::{Rule, RuleDef};
use crate::source::{SourceCode, TokKind, Token};
use crate::tokens::N;
use crate::utils::{
    can_tokens_be_adjacent, get_parenthesised_text, get_precedence, is_closing_paren,
    is_coalesce_expression, is_opening_paren, is_parenthesised,
};
use serde_json::Value;

pub static DEF: RuleDef = RuleDef {
    name: "no-unneeded-ternary",
    messages: &[
        (
            "unnecessaryConditionalExpression",
            "Unnecessary use of boolean literals in conditional expression.",
        ),
        (
            "unnecessaryConditionalAssignment",
            "Unnecessary use of conditional expression for default assignment.",
        ),
    ],
    has_default_options: true,
    create,
};

struct R {
    default_assignment: bool,
}

fn create(options: &[Value]) -> Result<Box<dyn Rule>, String> {
    if options.len() > 1 {
        return Err("invalid option".into());
    }
    let o = super::bool_props(options, 0, &["defaultAssignment"])?;
    Ok(Box::new(R {
        default_assignment: o[0].unwrap_or(true),
    }))
}

fn bool_value(sc: &SourceCode, n: NodeId) -> Option<bool> {
    if !sc.ast.is(n, "Literal") {
        return None;
    }
    match sc.ast.prop(n, "value") {
        Some(Prop::Bool(b)) => Some(*b),
        _ => None,
    }
}

fn inverse(op: &str) -> Option<&'static str> {
    match op {
        "==" => Some("!="),
        "!=" => Some("=="),
        "===" => Some("!=="),
        "!==" => Some("==="),
        _ => None,
    }
}

fn invert_expression(sc: &SourceCode, node: NodeId) -> Option<String> {
    let ast = &sc.ast;
    if ast.is(node, "BinaryExpression") {
        let op = ast.str(node, "operator").unwrap_or("");
        if let Some(inv) = inverse(op) {
            let (l, r) = (ast.child(node, "left")?, ast.child(node, "right")?);
            let op_tok = sc.first_token_between(l, r, |t| t.value == op)?;
            let mut left = sc.first_token(node, N)?;
            let mut right = sc.last_token(node, N)?;
            while let (Some(b), Some(a)) =
                (sc.token_before_tok(left, N), sc.token_after_tok(right, N))
            {
                if !(is_opening_paren(b) && is_closing_paren(a)) {
                    break;
                }
                left = b;
                right = a;
            }
            return Some(format!(
                "{}{}{}",
                sc.slice16(left.start, op_tok.start),
                inv,
                sc.slice16(op_tok.end, right.end)
            ));
        }
    }
    if get_precedence(ast, node) < 16 {
        return Some(format!("!({})", get_parenthesised_text(sc, node)));
    }
    Some(format!("!{}", get_parenthesised_text(sc, node)))
}

fn is_boolean_expression(sc: &SourceCode, n: NodeId) -> bool {
    let ast = &sc.ast;
    let op = ast.str(n, "operator").unwrap_or("");
    (ast.is(n, "BinaryExpression")
        && matches!(
            op,
            "==" | "===" | "!=" | "!==" | ">" | ">=" | "<" | "<=" | "in" | "instanceof"
        ))
        || (ast.is(n, "UnaryExpression") && op == "!")
}

fn fix_expression(
    sc: &SourceCode,
    node: NodeId,
    test: NodeId,
    cons: bool,
    alt: bool,
) -> Option<String> {
    let ast = &sc.ast;
    if cons == alt {
        if !ast.is(test, "Identifier") {
            return None;
        }
        let replacement = if cons { "true" } else { "false" };
        let range = ast.range(node);
        let word = Token {
            kind: TokKind::Boolean,
            start: 0,
            end: 0,
            value: replacement.into(),
        };
        let prefix = match sc.token_before(node, N) {
            Some(b) if b.end == range.0 && !can_tokens_be_adjacent(b, &word) => " ",
            _ => "",
        };
        return Some(format!("{prefix}{replacement}"));
    }
    if alt {
        return invert_expression(sc, test);
    }
    if is_boolean_expression(sc, test) {
        Some(get_parenthesised_text(sc, test))
    } else {
        Some(format!("!{}", invert_expression(sc, test)?))
    }
}

impl Rule for R {
    fn listeners(&self) -> &'static [&'static str] {
        &["ConditionalExpression"]
    }

    fn on(&mut self, _: usize, node: NodeId, cx: &mut Cx) {
        let sc = cx.sc;
        let ast = &sc.ast;
        let (Some(test), Some(cons), Some(alt)) = (
            ast.child(node, "test"),
            ast.child(node, "consequent"),
            ast.child(node, "alternate"),
        ) else {
            return;
        };
        if let (Some(c), Some(a)) = (bool_value(sc, cons), bool_value(sc, alt)) {
            let mut r = Report::node(node, "unnecessaryConditionalExpression");
            if let Some(text) = fix_expression(sc, node, test, c, a) {
                r = r.fix(fixer::replace(sc, node, text));
            }
            cx.report(r);
        } else if !self.default_assignment
            && ast.is(test, "Identifier")
            && ast.is(cons, "Identifier")
            && ast.name(test) == ast.name(cons)
        {
            let paren = (get_precedence(ast, alt) < 4 || is_coalesce_expression(ast, alt))
                && !is_parenthesised(sc, alt);
            let alt_text = if paren {
                format!("({})", sc.node_text(alt))
            } else {
                get_parenthesised_text(sc, alt)
            };
            let test_text = get_parenthesised_text(sc, test);
            cx.report(
                Report::node(node, "unnecessaryConditionalAssignment").fix(fixer::replace(
                    sc,
                    node,
                    format!("{test_text} || {alt_text}"),
                )),
            );
        }
    }
}
