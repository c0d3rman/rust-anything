//! lib/rules/valid-typeof.js

use crate::ast::{NodeId, Prop};
use crate::linter::Cx;
use crate::report::SugVal;
use crate::rule::{Report, SuggestSpec, fixer};
use crate::rules::{Rule, RuleDef};
use crate::utils::{is_static_template_literal, template_element_cooked};
use serde_json::Value;

pub static DEF: RuleDef = RuleDef {
    name: "valid-typeof",
    messages: &[
        ("invalidValue", "Invalid typeof comparison value."),
        (
            "notString",
            "Typeof comparisons should be to string literals.",
        ),
        ("suggestString", "Use `\"{{type}}\"` instead of `{{type}}`."),
    ],
    has_default_options: true,
    create,
};

const VALID_TYPES: &[&str] = &[
    "symbol",
    "undefined",
    "object",
    "boolean",
    "number",
    "string",
    "function",
    "bigint",
];

struct R {
    require_string_literals: bool,
}

fn create(options: &[Value]) -> Result<Box<dyn Rule>, String> {
    let o = super::common::bool_object(options, &["requireStringLiterals"])?;
    Ok(Box::new(R {
        require_string_literals: o[0].unwrap_or(false),
    }))
}

fn is_typeof(cx: &Cx, n: NodeId) -> bool {
    cx.sc.ast.is(n, "UnaryExpression") && cx.sc.ast.str(n, "operator") == Some("typeof")
}

fn is_reference_to_global_variable(cx: &Cx, node: NodeId) -> bool {
    let sm = cx.sm();
    let Some(name) = cx.sc.ast.name(node) else {
        return false;
    };
    let Some(v) = sm.lookup(sm.global_scope(), name) else {
        return false;
    };
    let var = sm.var(v);
    var.defs.is_empty()
        && var
            .references
            .iter()
            .any(|r| sm.reference(*r).identifier == node)
}

impl Rule for R {
    fn listeners(&self) -> &'static [&'static str] {
        &["Program", "UnaryExpression"]
    }

    fn on(&mut self, li: usize, node: NodeId, cx: &mut Cx) {
        if li == 0 || !is_typeof(cx, node) {
            return;
        }
        let ast = &cx.sc.ast;
        let Some(parent) = ast.parent(node) else {
            return;
        };
        if !(ast.is(parent, "BinaryExpression")
            && matches!(
                ast.str(parent, "operator"),
                Some("==" | "===" | "!=" | "!==")
            ))
        {
            return;
        }
        let (Some(left), Some(right)) = (ast.child(parent, "left"), ast.child(parent, "right"))
        else {
            return;
        };
        let sibling = if left == node { right } else { left };
        if ast.is(sibling, "Literal") || is_static_template_literal(ast, sibling) {
            let valid = if ast.is(sibling, "Literal") {
                ast.prop(sibling, "regex").is_none()
                    && ast.prop(sibling, "bigint").is_none()
                    && matches!(ast.prop(sibling, "value"), Some(Prop::Str(s)) if VALID_TYPES.contains(&s.as_str()))
            } else {
                ast.list(sibling, "quasis")
                    .first()
                    .copied()
                    .flatten()
                    .and_then(|q| template_element_cooked(ast, q))
                    .is_some_and(|s| VALID_TYPES.contains(&s))
            };
            if !valid {
                cx.report(Report::node(sibling, "invalidValue"));
            }
        } else if ast.is(sibling, "Identifier")
            && ast.name(sibling) == Some("undefined")
            && is_reference_to_global_variable(cx, sibling)
        {
            let mut r = Report::node(
                sibling,
                if self.require_string_literals {
                    "notString"
                } else {
                    "invalidValue"
                },
            );
            r.suggest.push(SuggestSpec {
                keys: vec![
                    ("messageId", SugVal::Str("suggestString".into())),
                    (
                        "data",
                        SugVal::Data(vec![("type".into(), Value::from("undefined"))]),
                    ),
                    ("fix", SugVal::Fix),
                ],
                fix: Some(fixer::replace(cx.sc, sibling, "\"undefined\"")),
            });
            cx.report(r);
        } else if self.require_string_literals && !is_typeof(cx, sibling) {
            cx.report(Report::node(sibling, "notString"));
        }
    }
}
