//! lib/rules/no-unsafe-negation.js

use crate::ast::NodeId;
use crate::linter::Cx;
use crate::report::SugVal;
use crate::rule::{Report, SuggestSpec, fixer};
use crate::rules::{Rule, RuleDef};
use crate::utils::is_parenthesised;
use serde_json::Value;

pub static DEF: RuleDef = RuleDef {
    name: "no-unsafe-negation",
    messages: &[
        (
            "unexpected",
            "Unexpected negating the left operand of '{{operator}}' operator.",
        ),
        (
            "suggestNegatedExpression",
            "Negate '{{operator}}' expression instead of its left operand. This changes the current behavior.",
        ),
        (
            "suggestParenthesisedNegation",
            "Wrap negation in '()' to make the intention explicit. This preserves the current behavior.",
        ),
    ],
    has_default_options: true,
    create,
};

struct R {
    ordering: bool,
}

fn create(options: &[Value]) -> Result<Box<dyn Rule>, String> {
    if options.len() > 1 {
        return Err("invalid option".into());
    }
    let o = super::bool_props(options, 0, &["enforceForOrderingRelations"])?;
    Ok(Box::new(R {
        ordering: o[0].unwrap_or(false),
    }))
}

impl Rule for R {
    fn listeners(&self) -> &'static [&'static str] {
        &["BinaryExpression"]
    }

    fn on(&mut self, _: usize, node: NodeId, cx: &mut Cx) {
        let sc = cx.sc;
        let ast = &sc.ast;
        let op = ast.str(node, "operator").unwrap_or("");
        let ordering = self.ordering && matches!(op, "<" | ">" | ">=" | "<=");
        let Some(left) = ast.child(node, "left") else {
            return;
        };
        if !((op == "in" || op == "instanceof" || ordering)
            && ast.is(left, "UnaryExpression")
            && ast.str(left, "operator") == Some("!")
            && !is_parenthesised(sc, left))
        {
            return;
        }
        let Some(neg) = sc.first_token(left, crate::tokens::N) else {
            return;
        };
        let range = (neg.end, ast.range(node).1);
        let text = format!("({})", sc.slice16(range.0, range.1));
        let mut r = Report::loc(sc.node_loc(left), "unexpected").data("operator", op);
        r.node = Some(node);
        r.suggest.push(SuggestSpec {
            keys: vec![
                ("messageId", SugVal::Str("suggestNegatedExpression".into())),
                ("data", SugVal::Data(vec![("operator".into(), op.into())])),
                ("fix", SugVal::Fix),
            ],
            fix: Some(fixer::replace_range(range, text)),
        });
        r.suggest.push(SuggestSpec {
            keys: vec![
                (
                    "messageId",
                    SugVal::Str("suggestParenthesisedNegation".into()),
                ),
                ("fix", SugVal::Fix),
            ],
            fix: Some(fixer::replace(
                sc,
                left,
                format!("({})", sc.node_text(left)),
            )),
        });
        cx.report(r);
    }
}
