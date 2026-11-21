//! lib/rules/no-dupe-class-members.js

use crate::ast::NodeId;
use crate::linter::Cx;
use crate::rule::Report;
use crate::rules::{Rule, RuleDef};
use crate::utils::get_static_property_name;
use serde_json::Value;
use std::collections::HashMap;

pub static DEF: RuleDef = RuleDef {
    name: "no-dupe-class-members",
    messages: &[("unexpected", "Duplicate name '{{name}}'.")],
    has_default_options: false,
    create,
};

#[derive(Default, Clone, Copy)]
struct State {
    init: bool,
    get: bool,
    set: bool,
}

struct R {
    stack: Vec<HashMap<(String, bool), State>>,
}

fn create(options: &[Value]) -> Result<Box<dyn Rule>, String> {
    if !options.is_empty() {
        return Err("invalid option".into());
    }
    Ok(Box::new(R { stack: Vec::new() }))
}

impl Rule for R {
    fn listeners(&self) -> &'static [&'static str] {
        &[
            "Program",
            "ClassBody",
            "ClassBody:exit",
            "MethodDefinition, PropertyDefinition",
        ]
    }

    fn on(&mut self, l: usize, node: NodeId, cx: &mut Cx) {
        match l {
            0 => self.stack.clear(),
            1 => self.stack.push(HashMap::new()),
            2 => {
                self.stack.pop();
            }
            _ => {
                let ast = &cx.sc.ast;
                if ast
                    .child(node, "value")
                    .is_some_and(|v| ast.is(v, "TSEmptyBodyFunctionExpression"))
                {
                    return;
                }
                let Some(name) = get_static_property_name(ast, node) else {
                    return;
                };
                let kind = if ast.is(node, "MethodDefinition") {
                    ast.str(node, "kind").unwrap_or("")
                } else {
                    "field"
                };
                if kind == "constructor" {
                    return;
                }
                let Some(map) = self.stack.last_mut() else {
                    return;
                };
                let state = map
                    .entry((name.clone(), ast.bool(node, "static")))
                    .or_default();
                let duplicate = match kind {
                    "get" => {
                        let d = state.init || state.get;
                        state.get = true;
                        d
                    }
                    "set" => {
                        let d = state.init || state.set;
                        state.set = true;
                        d
                    }
                    _ => {
                        let d = state.init || state.get || state.set;
                        state.init = true;
                        d
                    }
                };
                if duplicate {
                    let Some(key) = ast.child(node, "key") else {
                        return;
                    };
                    cx.report(Report::loc(cx.sc.node_loc(key), "unexpected").data("name", name));
                }
            }
        }
    }
}
