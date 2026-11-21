//! lib/rules/no-console.js

use crate::ast::NodeId;
use crate::linter::Cx;
use crate::report::SugVal;
use crate::rule::{Report, SuggestSpec, fixer};
use crate::rules::{Rule, RuleDef};
use crate::scope::RefId;
use crate::tokens::N;
use crate::utils::get_static_property_name;
use serde_json::Value;

pub static DEF: RuleDef = RuleDef {
    name: "no-console",
    messages: &[
        ("unexpected", "Unexpected console statement."),
        (
            "limited",
            "Unexpected console statement. Only these console methods are allowed: {{ allowed }}.",
        ),
        ("removeConsole", "Remove the console.{{ propertyName }}()."),
        ("removeMethodCall", "Remove the console method call."),
    ],
    has_default_options: true,
    create,
};

struct R {
    allowed: Vec<String>,
}

fn create(options: &[Value]) -> Result<Box<dyn Rule>, String> {
    let bad = || Err("invalid option".to_string());
    let mut allowed = Vec::new();
    match options {
        [] => {}
        [Value::Object(o)] => {
            for (k, v) in o {
                if k != "allow" {
                    return bad();
                }
                let Value::Array(a) = v else { return bad() };
                if a.is_empty() {
                    return bad();
                }
                for x in a {
                    let Value::String(s) = x else { return bad() };
                    if allowed.contains(s) {
                        return bad();
                    }
                    allowed.push(s.clone());
                }
            }
        }
        _ => return bad(),
    }
    Ok(Box::new(R { allowed }))
}

impl R {
    fn is_member_access_except_allowed(&self, cx: &Cx, r: RefId) -> bool {
        let ast = &cx.sc.ast;
        let node = cx.sm().reference(r).identifier;
        let Some(parent) = ast.parent(node) else {
            return false;
        };
        if !(ast.is(parent, "MemberExpression") && ast.child(parent, "object") == Some(node)) {
            return false;
        }
        let allowed = get_static_property_name(ast, parent)
            .is_some_and(|p| !p.is_empty() && self.allowed.contains(&p));
        !allowed
    }

    fn report(&self, cx: &mut Cx, r: RefId) {
        let ast = &cx.sc.ast;
        let Some(node) = ast.parent(cx.sm().reference(r).identifier) else {
            return;
        };
        let mut rep = Report::node(
            node,
            if self.allowed.is_empty() {
                "unexpected"
            } else {
                "limited"
            },
        )
        .data("allowed", self.allowed.join(", "));
        if can_provide_suggestions(cx, node)
            && let Some(stmt) = ast.parent(node).and_then(|p| ast.parent(p))
        {
            let fix = Some(fixer::remove(cx.sc, stmt));
            let keys = if ast.bool(node, "computed") {
                vec![
                    ("fix", SugVal::Fix),
                    ("messageId", SugVal::Str("removeMethodCall".into())),
                ]
            } else {
                let name = ast
                    .child(node, "property")
                    .and_then(|p| ast.name(p))
                    .unwrap_or("")
                    .to_string();
                vec![
                    ("fix", SugVal::Fix),
                    ("messageId", SugVal::Str("removeConsole".into())),
                    (
                        "data",
                        SugVal::Data(vec![("propertyName".into(), Value::from(name))]),
                    ),
                ]
            };
            rep.suggest.push(SuggestSpec { keys, fix });
        }
        cx.report(rep);
    }
}

fn maybe_asi_hazard(cx: &Cx, node: NodeId) -> bool {
    let before = cx.sc.token_before(node, N);
    let after = cx.sc.token_after(node, N);
    let Some(after) = after else { return false };
    let unsafe_after = after.value.starts_with(['-', '[', '(', '/', '+', '`'])
        && after.value != "++"
        && after.value != "--";
    unsafe_after && before.is_some_and(|b| !matches!(b.value.as_str(), ":" | ";" | "{"))
}

fn can_provide_suggestions(cx: &Cx, node: NodeId) -> bool {
    let ast = &cx.sc.ast;
    let Some(call) = ast.parent(node) else {
        return false;
    };
    if !(ast.is(call, "CallExpression") && ast.child(call, "callee") == Some(node)) {
        return false;
    }
    let Some(stmt) = ast.parent(call) else {
        return false;
    };
    if !ast.is(stmt, "ExpressionStatement") {
        return false;
    }
    let Some(list) = ast.parent(stmt) else {
        return false;
    };
    matches!(
        ast.kind(list),
        "Program" | "BlockStatement" | "StaticBlock" | "SwitchCase"
    ) && !maybe_asi_hazard(cx, stmt)
}

impl Rule for R {
    fn listeners(&self) -> &'static [&'static str] {
        &["Program:exit"]
    }

    fn on(&mut self, _: usize, node: NodeId, cx: &mut Cx) {
        let sm = cx.sm();
        let scope = cx.get_scope(node);
        let console = sm.variable_by_name(scope, "console");
        if console.is_some_and(|v| !sm.var(v).defs.is_empty()) {
            return;
        }
        let refs: Vec<RefId> = match console {
            Some(v) => sm.var(v).references.clone(),
            None => sm
                .scope(scope)
                .through
                .iter()
                .copied()
                .filter(|r| cx.sc.ast.name(sm.reference(*r).identifier) == Some("console"))
                .collect(),
        };
        let refs: Vec<RefId> = refs
            .into_iter()
            .filter(|r| self.is_member_access_except_allowed(cx, *r))
            .collect();
        for r in refs {
            self.report(cx, r);
        }
    }
}
