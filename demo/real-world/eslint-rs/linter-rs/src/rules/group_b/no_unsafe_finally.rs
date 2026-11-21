//! lib/rules/no-unsafe-finally.js

use crate::ast::NodeId;
use crate::linter::Cx;
use crate::rule::Report;
use crate::rules::{Rule, RuleDef};
use serde_json::Value;

pub static DEF: RuleDef = RuleDef {
    name: "no-unsafe-finally",
    messages: &[("unsafeUsage", "Unsafe usage of {{nodeType}}.")],
    has_default_options: false,
    create,
};

struct R;

fn create(options: &[Value]) -> Result<Box<dyn Rule>, String> {
    super::no_options(options)?;
    Ok(Box::new(R))
}

fn is_function_or_class(kind: &str) -> bool {
    matches!(
        kind,
        "Program"
            | "FunctionDeclaration"
            | "FunctionExpression"
            | "ClassDeclaration"
            | "ClassExpression"
            | "ArrowFunctionExpression"
    )
}

fn is_loop(kind: &str) -> bool {
    matches!(
        kind,
        "DoWhileStatement"
            | "WhileStatement"
            | "ForOfStatement"
            | "ForInStatement"
            | "ForStatement"
    )
}

impl Rule for R {
    fn listeners(&self) -> &'static [&'static str] {
        &[
            "ReturnStatement",
            "ThrowStatement",
            "BreakStatement",
            "ContinueStatement",
        ]
    }

    fn on(&mut self, _: usize, node: NodeId, cx: &mut Cx) {
        let sc = cx.sc;
        let ast = &sc.ast;
        let kind = ast.kind(node);
        let label = ast.child(node, "label");
        let label_name = label.and_then(|l| ast.name(l));
        let sentinel = |k: &str| match (kind, label) {
            ("BreakStatement", None) => {
                is_function_or_class(k) || is_loop(k) || k == "SwitchStatement"
            }
            ("ContinueStatement", None) => is_function_or_class(k) || is_loop(k),
            _ => is_function_or_class(k),
        };
        let mut label_inside = false;
        let mut current = node;
        let mut inside = false;
        while !sentinel(ast.kind(current)) {
            let Some(parent) = ast.parent(current) else {
                break;
            };
            if let (Some(pl), Some(name)) = (ast.child(parent, "label"), label_name)
                && ast.name(pl) == Some(name)
            {
                label_inside = true;
            }
            if ast.is(parent, "TryStatement") && ast.child(parent, "finalizer") == Some(current) {
                inside = !(label.is_some() && label_inside);
                break;
            }
            current = parent;
        }
        if inside {
            cx.report(Report::node(node, "unsafeUsage").data("nodeType", kind));
        }
    }
}
