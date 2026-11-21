//! lib/rules/no-prototype-builtins.js

use crate::ast::NodeId;
use crate::linter::Cx;
use crate::report::SugVal;
use crate::rule::{Report, SuggestSpec, fixer};
use crate::rules::{Rule, RuleDef};
use crate::scope::ScopeType;
use crate::utils::{
    get_precedence, get_static_property_name, is_opening_paren, skip_chain_expression,
};
use serde_json::Value;

pub static DEF: RuleDef = RuleDef {
    name: "no-prototype-builtins",
    messages: &[
        (
            "prototypeBuildIn",
            "Do not access Object.prototype method '{{prop}}' from target object.",
        ),
        (
            "callObjectPrototype",
            "Call Object.prototype.{{prop}} explicitly.",
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

fn is_after_optional(cx: &Cx, node: NodeId) -> bool {
    let ast = &cx.sc.ast;
    let left = match ast.kind(node) {
        "MemberExpression" => ast.child(node, "object"),
        "CallExpression" => ast.child(node, "callee"),
        _ => return false,
    };
    if ast.bool(node, "optional") {
        return true;
    }
    left.is_some_and(|l| is_after_optional(cx, l))
}

fn suggestion_fix(cx: &Cx, node: NodeId, callee: NodeId, prop: &str) -> Option<crate::report::Fix> {
    let ast = &cx.sc.ast;
    if is_after_optional(cx, node) {
        return None;
    }
    let raw_callee = ast.child(node, "callee")?;
    if ast.is(raw_callee, "ChainExpression") {
        return None;
    }
    let object = ast.child(callee, "object")?;
    if ast.is(object, "Super") {
        return None;
    }
    let sm = cx.sm();
    let v = sm.variable_by_name(cx.get_scope(node), "Object")?;
    let var = sm.var(v);
    if sm.scope(var.scope).kind != ScopeType::Global || !var.defs.is_empty() {
        return None;
    }
    let mut text = cx.sc.node_text(object).to_string();
    if get_precedence(ast, object) <= 0 {
        text = format!("({text})");
    }
    let open = cx.sc.token_after_f(raw_callee, is_opening_paren)?;
    let delim = if ast.list(node, "arguments").is_empty() {
        ""
    } else {
        ", "
    };
    crate::report::merge_fixes(
        vec![
            fixer::replace(cx.sc, callee, format!("Object.prototype.{prop}.call")),
            fixer::insert_after_range((open.start, open.end), text + delim),
        ],
        cx.sc,
    )
}

impl Rule for R {
    fn listeners(&self) -> &'static [&'static str] {
        &["CallExpression"]
    }

    fn on(&mut self, _: usize, node: NodeId, cx: &mut Cx) {
        let ast = &cx.sc.ast;
        let Some(callee) = ast
            .child(node, "callee")
            .map(|c| skip_chain_expression(ast, c))
        else {
            return;
        };
        if !ast.is(callee, "MemberExpression") {
            return;
        }
        let Some(prop) = get_static_property_name(ast, callee) else {
            return;
        };
        if !matches!(
            prop.as_str(),
            "hasOwnProperty" | "isPrototypeOf" | "propertyIsEnumerable"
        ) {
            return;
        }
        let Some(property) = ast.child(callee, "property") else {
            return;
        };
        let mut r =
            Report::loc(cx.sc.node_loc(property), "prototypeBuildIn").data("prop", prop.as_str());
        r.node = Some(node);
        r.suggest.push(SuggestSpec {
            keys: vec![
                ("messageId", SugVal::Str("callObjectPrototype".into())),
                (
                    "data",
                    SugVal::Data(vec![("prop".into(), Value::from(prop.as_str()))]),
                ),
                ("fix", SugVal::Fix),
            ],
            fix: suggestion_fix(cx, node, callee, &prop),
        });
        cx.report(r);
    }
}
