//! lib/rules/no-obj-calls.js

use crate::ast::NodeId;
use crate::linter::Cx;
use crate::rule::Report;
use crate::rules::{Rule, RuleDef};
use crate::static_value::{ReferenceTracker, TraceMap, get_property_name};
use serde_json::Value;

pub static DEF: RuleDef = RuleDef {
    name: "no-obj-calls",
    messages: &[
        ("unexpectedCall", "'{{name}}' is not a function."),
        (
            "unexpectedRefCall",
            "'{{name}}' is reference to '{{ref}}', which is not a function.",
        ),
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

const NON_CALLABLE: &[&str] = &[
    "Atomics",
    "JSON",
    "Math",
    "Reflect",
    "Intl",
    "Temporal",
    "globalThis",
];

fn report_name(cx: &Cx, node: NodeId) -> Result<Option<String>, crate::static_value::Unknown> {
    let ast = &cx.sc.ast;
    match ast.kind(node) {
        "ChainExpression" => match ast.child(node, "expression") {
            Some(e) => report_name(cx, e),
            None => Ok(None),
        },
        "MemberExpression" => get_property_name(ast, cx.sm(), node, None),
        _ => Ok(ast.name(node).map(str::to_string)),
    }
}

impl Rule for R {
    fn listeners(&self) -> &'static [&'static str] {
        &["Program"]
    }

    fn on(&mut self, _: usize, node: NodeId, cx: &mut Cx) {
        let scope = cx.get_scope(node);
        let map = TraceMap {
            keys: NON_CALLABLE
                .iter()
                .map(|g| {
                    (
                        g.to_string(),
                        TraceMap {
                            call: Some(1),
                            construct: Some(1),
                            ..Default::default()
                        },
                    )
                })
                .collect(),
            ..Default::default()
        };
        let refs = match ReferenceTracker::new(&cx.sc.ast, cx.sm(), scope)
            .iterate_global_references(&map)
        {
            Ok(r) => r,
            Err(_) => {
                self.unsupported = true;
                return;
            }
        };
        for r in refs {
            let Some(callee) = cx.sc.ast.child(r.node, "callee") else {
                continue;
            };
            let name = match report_name(cx, callee) {
                Ok(n) => n,
                Err(_) => {
                    self.unsupported = true;
                    return;
                }
            };
            let Some(name) = name else {
                self.unsupported = true;
                return;
            };
            let reference = r.path[0].clone();
            let id = if name == reference {
                "unexpectedCall"
            } else {
                "unexpectedRefCall"
            };
            cx.report(
                Report::node(r.node, id)
                    .data("name", name)
                    .data("ref", reference),
            );
        }
    }

    fn unsupported(&self) -> Option<&str> {
        self.unsupported
            .then_some("a property name this port does not model")
    }
}
