//! lib/rules/no-implied-eval.js

use crate::ast::{NodeId, Prop};
use crate::eslint_utils::is_specific_member_access;
use crate::linter::Cx;
use crate::rule::Report;
use crate::rules::{Rule, RuleDef};
use crate::static_value::{V, get_static_value};
use crate::utils::get_static_property_name;
use serde_json::Value;

pub static DEF: RuleDef = RuleDef {
    name: "no-implied-eval",
    messages: &[
        (
            "impliedEval",
            "Implied eval. Consider passing a function instead of a string.",
        ),
        ("execScript", "Implied eval. Do not use execScript()."),
    ],
    has_default_options: false,
    create,
};

struct R {
    unsupported: bool,
}

fn create(options: &[Value]) -> Result<Box<dyn Rule>, String> {
    super::common::no_options(options)?;
    Ok(Box::new(R { unsupported: false }))
}

fn eval_like(name: &str) -> bool {
    matches!(name, "setInterval" | "setTimeout" | "execScript")
}

fn is_evaluated_string(cx: &Cx, node: NodeId) -> bool {
    let ast = &cx.sc.ast;
    if (ast.is(node, "Literal")
        && ast.prop(node, "regex").is_none()
        && matches!(ast.prop(node, "value"), Some(Prop::Str(_))))
        || ast.is(node, "TemplateLiteral")
    {
        return true;
    }
    if ast.is(node, "BinaryExpression") && ast.str(node, "operator") == Some("+") {
        return ast
            .child(node, "left")
            .is_some_and(|l| is_evaluated_string(cx, l))
            || ast
                .child(node, "right")
                .is_some_and(|r| is_evaluated_string(cx, r));
    }
    false
}

impl R {
    fn report_call(&mut self, cx: &mut Cx, node: NodeId) {
        let ast = &cx.sc.ast;
        let Some(first) = ast.list(node, "arguments").first().copied().flatten() else {
            return;
        };
        let scope = cx.get_scope(node);
        let is_static_string = match get_static_value(ast, cx.sm(), first, Some(scope)) {
            Ok(v) => v.is_some_and(|v| matches!(v.value, V::Str(_))),
            Err(_) => {
                self.unsupported = true;
                return;
            }
        };
        if !(is_static_string || is_evaluated_string(cx, first)) {
            return;
        }
        let Some(callee) = ast.child(node, "callee") else {
            return;
        };
        let name = if ast.is(callee, "Identifier") {
            ast.name(callee).map(str::to_string)
        } else {
            get_static_property_name(ast, callee)
        };
        let id = if name.as_deref() == Some("execScript") {
            "execScript"
        } else {
            "impliedEval"
        };
        cx.report(Report::node(node, id));
    }
}

impl Rule for R {
    fn listeners(&self) -> &'static [&'static str] {
        &["CallExpression", "Program:exit"]
    }

    fn on(&mut self, li: usize, node: NodeId, cx: &mut Cx) {
        let ast = &cx.sc.ast;
        if li == 0 {
            let Some(callee) = ast.child(node, "callee") else {
                return;
            };
            if ast.is(callee, "Identifier")
                && ast.name(callee).is_some_and(eval_like)
                && cx.sm().is_global_reference(ast, callee)
            {
                self.report_call(cx, node);
            }
            return;
        }
        let sm = cx.sm();
        let global = cx.get_scope(node);
        for candidate in ["global", "window", "globalThis", "self"] {
            let Some(v) = sm.variable_by_name(global, candidate) else {
                continue;
            };
            let var = sm.var(v);
            if !var.defs.is_empty() {
                continue;
            }
            let name = var.name.clone();
            for &r in &var.references {
                let ast = &cx.sc.ast;
                let Some(mut node) = ast.parent(sm.reference(r).identifier) else {
                    continue;
                };
                while is_specific_member_access(ast, node, None, Some(&|p: &str| p == name)) {
                    match ast.parent(node) {
                        Some(p) => node = p,
                        None => break,
                    }
                }
                if is_specific_member_access(ast, node, None, Some(&eval_like)) {
                    let Some(p) = ast.parent(node) else { continue };
                    let callee = if ast.is(p, "ChainExpression") {
                        p
                    } else {
                        node
                    };
                    if let Some(parent) = ast.parent(callee)
                        && ast.is(parent, "CallExpression")
                        && ast.child(parent, "callee") == Some(callee)
                    {
                        self.report_call(cx, parent);
                    }
                }
            }
        }
    }

    fn unsupported(&self) -> Option<&str> {
        self.unsupported
            .then_some("getStaticValue met a value this port does not model")
    }
}
