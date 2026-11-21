//! lib/rules/no-compare-neg-zero.js

use crate::ast::{NodeId, Prop};
use crate::linter::Cx;
use crate::report::SugVal;
use crate::rule::{Report, SuggestSpec, fixer};
use crate::rules::{Rule, RuleDef};
use serde_json::Value;

pub static DEF: RuleDef = RuleDef {
    name: "no-compare-neg-zero",
    messages: &[
        (
            "unexpected",
            "Do not use the '{{operator}}' operator to compare against -0.",
        ),
        (
            "suggestRemoveMinus",
            "Replace '-0' with '0' (keeps the current comparison behavior).",
        ),
        (
            "suggestObjectIs",
            "Replace with 'Object.is()' (changes the comparison to distinguish -0 from +0).",
        ),
        (
            "suggestNotObjectIs",
            "Replace with '!Object.is()' (changes the comparison to distinguish -0 from +0).",
        ),
    ],
    has_default_options: false,
    create,
};

struct R;

fn create(options: &[Value]) -> Result<Box<dyn Rule>, String> {
    super::common::no_options(options)?;
    Ok(Box::new(R))
}

fn is_neg_zero(cx: &Cx, n: NodeId) -> bool {
    let ast = &cx.sc.ast;
    ast.is(n, "UnaryExpression")
        && ast.str(n, "operator") == Some("-")
        && ast.child(n, "argument").is_some_and(|a| {
            ast.is(a, "Literal")
                && ast.prop(a, "bigint").is_none()
                && matches!(ast.prop(a, "value"), Some(Prop::Num(v)) if *v == 0.0)
        })
}

fn operand_text(cx: &Cx, n: NodeId) -> String {
    let t = cx.sc.node_text(n);
    if cx.sc.ast.is(n, "SequenceExpression") {
        format!("({t})")
    } else {
        t.to_string()
    }
}

impl Rule for R {
    fn listeners(&self) -> &'static [&'static str] {
        &["BinaryExpression"]
    }

    fn on(&mut self, _: usize, node: NodeId, cx: &mut Cx) {
        let ast = &cx.sc.ast;
        let op = ast.str(node, "operator").unwrap_or("");
        if !matches!(op, ">" | ">=" | "<" | "<=" | "==" | "===" | "!=" | "!==") {
            return;
        }
        let (Some(left), Some(right)) = (ast.child(node, "left"), ast.child(node, "right")) else {
            return;
        };
        let (ln, rn) = (is_neg_zero(cx, left), is_neg_zero(cx, right));
        if !ln && !rn {
            return;
        }
        let mut fixes = Vec::new();
        if ln {
            fixes.push(fixer::replace(cx.sc, left, "0"));
        }
        if rn {
            fixes.push(fixer::replace(cx.sc, right, "0"));
        }
        let first = crate::report::merge_fixes(fixes, cx.sc);
        let second = (|| {
            if (op != "===" && op != "!==") || !cx.sc.comments_inside(node).is_empty() {
                return None;
            }
            let sm = cx.sm();
            let v = sm.variable_by_name(cx.get_scope(node), "Object")?;
            if !sm.var(v).identifiers.is_empty() {
                return None;
            }
            let neg = if op == "===" { "" } else { "!" };
            Some(fixer::replace(
                cx.sc,
                node,
                format!(
                    "{neg}Object.is({}, {})",
                    operand_text(cx, left),
                    operand_text(cx, right)
                ),
            ))
        })();
        let mut r = Report::node(node, "unexpected").data("operator", op);
        r.suggest.push(SuggestSpec {
            keys: vec![
                ("messageId", SugVal::Str("suggestRemoveMinus".into())),
                ("fix", SugVal::Fix),
            ],
            fix: first,
        });
        r.suggest.push(SuggestSpec {
            keys: vec![
                (
                    "messageId",
                    SugVal::Str(
                        if op == "===" {
                            "suggestObjectIs"
                        } else {
                            "suggestNotObjectIs"
                        }
                        .into(),
                    ),
                ),
                ("fix", SugVal::Fix),
            ],
            fix: second,
        });
        cx.report(r);
    }
}
