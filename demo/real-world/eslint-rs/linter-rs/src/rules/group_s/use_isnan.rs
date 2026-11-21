//! lib/rules/use-isnan.js

use crate::ast::NodeId;
use crate::linter::Cx;
use crate::report::SugVal;
use crate::rule::{Report, SuggestSpec, fixer};
use crate::rules::{Rule, RuleDef};
use crate::utils::{
    get_static_property_name, is_specific_id, is_specific_member_access, skip_chain_expression,
};
use serde_json::Value;

pub static DEF: RuleDef = RuleDef {
    name: "use-isnan",
    messages: &[
        (
            "comparisonWithNaN",
            "Use the isNaN function to compare with NaN.",
        ),
        (
            "switchNaN",
            "'switch(NaN)' can never match a case clause. Use Number.isNaN instead of the switch.",
        ),
        (
            "caseNaN",
            "'case NaN' can never match. Use Number.isNaN before the switch.",
        ),
        (
            "indexOfNaN",
            "Array prototype method '{{ methodName }}' cannot find NaN.",
        ),
        ("replaceWithIsNaN", "Replace with Number.isNaN."),
        (
            "replaceWithCastingAndIsNaN",
            "Replace with Number.isNaN and cast to a Number.",
        ),
        (
            "replaceWithFindIndex",
            "Replace with Array.prototype.{{ methodName }}.",
        ),
    ],
    has_default_options: true,
    create,
};

struct R {
    listeners: &'static [&'static str],
}

fn create(options: &[Value]) -> Result<Box<dyn Rule>, String> {
    let o = super::common::bool_object(options, &["enforceForSwitchCase", "enforceForIndexOf"])?;
    let switch = o[0].unwrap_or(true);
    let index_of = o[1].unwrap_or(false);
    let listeners: &'static [&'static str] = match (switch, index_of) {
        (true, true) => &["BinaryExpression", "SwitchStatement", "CallExpression"],
        (true, false) => &["BinaryExpression", "SwitchStatement"],
        (false, true) => &["BinaryExpression", "CallExpression"],
        (false, false) => &["BinaryExpression"],
    };
    Ok(Box::new(R { listeners }))
}

fn is_nan_identifier(cx: &Cx, node: Option<NodeId>) -> bool {
    let Some(node) = node else { return false };
    let ast = &cx.sc.ast;
    let check = if ast.is(node, "SequenceExpression") {
        match ast.list(node, "expressions").last().copied().flatten() {
            Some(n) => n,
            None => return false,
        }
    } else {
        node
    };
    if is_specific_id(ast, check, "NaN") {
        return cx.sm().is_global_reference(ast, check);
    }
    if is_specific_member_access(ast, check, Some("Number"), Some("NaN")) {
        let Some(obj) = ast.child(skip_chain_expression(ast, check), "object") else {
            return false;
        };
        return cx.sm().is_global_reference(ast, obj);
    }
    false
}

fn binary_fix(cx: &Cx, node: NodeId, wrap: &dyn Fn(&str) -> String) -> Option<crate::report::Fix> {
    let ast = &cx.sc.ast;
    let left = ast.child(node, "left")?;
    let right = ast.child(node, "right")?;
    let compared = if is_nan_identifier(cx, Some(left)) {
        right
    } else {
        left
    };
    let negation = if ast.str(node, "operator").unwrap_or("").starts_with('!') {
        "!"
    } else {
        ""
    };
    let mut text = cx.sc.node_text(compared).to_string();
    if ast.is(compared, "SequenceExpression") {
        text = format!("({text})");
    }
    Some(fixer::replace(
        cx.sc,
        node,
        format!("{negation}{}", wrap(&text)),
    ))
}

impl R {
    fn check_binary(&self, cx: &mut Cx, node: NodeId) {
        let ast = &cx.sc.ast;
        let op = ast.str(node, "operator").unwrap_or("");
        if !matches!(op, "<" | ">" | "<=" | ">=" | "==" | "!=" | "===" | "!==") {
            return;
        }
        let (left, right) = (ast.child(node, "left"), ast.child(node, "right"));
        let left_nan = is_nan_identifier(cx, left);
        if !left_nan && !is_nan_identifier(cx, right) {
            return;
        }
        let Some(nan_node) = (if left_nan { left } else { right }) else {
            return;
        };
        let suggestable =
            matches!(op, "==" | "===" | "!=" | "!==") && !ast.is(nan_node, "SequenceExpression");
        let castable = matches!(op, "==" | "!=");
        let mut r = Report::node(node, "comparisonWithNaN");
        if suggestable {
            r.suggest.push(SuggestSpec {
                keys: vec![
                    ("messageId", SugVal::Str("replaceWithIsNaN".into())),
                    ("fix", SugVal::Fix),
                ],
                fix: binary_fix(cx, node, &|v| format!("Number.isNaN({v})")),
            });
            if castable {
                r.suggest.push(SuggestSpec {
                    keys: vec![
                        (
                            "messageId",
                            SugVal::Str("replaceWithCastingAndIsNaN".into()),
                        ),
                        ("fix", SugVal::Fix),
                    ],
                    fix: binary_fix(cx, node, &|v| format!("Number.isNaN(Number({v}))")),
                });
            }
        }
        cx.report(r);
    }

    fn check_switch(&self, cx: &mut Cx, node: NodeId) {
        let ast = &cx.sc.ast;
        if is_nan_identifier(cx, ast.child(node, "discriminant")) {
            cx.report(Report::node(node, "switchNaN"));
        }
        let cases: Vec<NodeId> = cx.sc.ast.nodes_of(node, "cases").collect();
        for c in cases {
            if is_nan_identifier(cx, cx.sc.ast.child(c, "test")) {
                cx.report(Report::node(c, "caseNaN"));
            }
        }
    }

    fn check_call(&self, cx: &mut Cx, node: NodeId) {
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
        let method = get_static_property_name(ast, callee);
        let args = ast.list(node, "arguments");
        let Some(method) = method.filter(|m| m == "indexOf" || m == "lastIndexOf") else {
            return;
        };
        if args.len() > 2 || !is_nan_identifier(cx, args.first().copied().flatten()) {
            return;
        }
        let Some(first) = args[0] else { return };
        let suggestable =
            !ast.is(first, "SequenceExpression") && args.get(1).copied().flatten().is_none();
        let mut r = Report::node(node, "indexOfNaN").data("methodName", method.as_str());
        if suggestable && let Some(prop) = ast.child(callee, "property") {
            let find = if method == "indexOf" {
                "findIndex"
            } else {
                "findLastIndex"
            };
            let name = if ast.bool(callee, "computed") {
                format!("\"{find}\"")
            } else {
                find.to_string()
            };
            r.suggest.push(SuggestSpec {
                keys: vec![
                    ("messageId", SugVal::Str("replaceWithFindIndex".into())),
                    (
                        "data",
                        SugVal::Data(vec![("methodName".into(), Value::from(find))]),
                    ),
                    ("fix", SugVal::Fix),
                ],
                fix: crate::report::merge_fixes(
                    vec![
                        fixer::replace(cx.sc, prop, name),
                        fixer::replace(cx.sc, first, "Number.isNaN"),
                    ],
                    cx.sc,
                ),
            });
        }
        cx.report(r);
    }
}

impl Rule for R {
    fn listeners(&self) -> &'static [&'static str] {
        self.listeners
    }

    fn on(&mut self, li: usize, node: NodeId, cx: &mut Cx) {
        match self.listeners[li] {
            "BinaryExpression" => self.check_binary(cx, node),
            "SwitchStatement" => self.check_switch(cx, node),
            _ => self.check_call(cx, node),
        }
    }
}
