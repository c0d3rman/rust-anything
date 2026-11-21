//! lib/rules/no-dupe-keys.js

use crate::ast::NodeId;
use crate::linter::Cx;
use crate::rule::Report;
use crate::rules::{Rule, RuleDef};
use crate::utils::get_static_property_name;
use serde_json::Value;
use std::collections::HashMap;

pub static DEF: RuleDef = RuleDef {
    name: "no-dupe-keys",
    messages: &[("unexpected", "Duplicate key '{{name}}'.")],
    has_default_options: false,
    create,
};

struct ObjectInfo {
    node: NodeId,
    properties: HashMap<String, (bool, bool)>,
}

struct R {
    stack: Vec<ObjectInfo>,
}

fn create(options: &[Value]) -> Result<Box<dyn Rule>, String> {
    if !options.is_empty() {
        return Err("invalid option".into());
    }
    Ok(Box::new(R { stack: Vec::new() }))
}

impl Rule for R {
    fn listeners(&self) -> &'static [&'static str] {
        &["ObjectExpression", "ObjectExpression:exit", "Property"]
    }

    fn on(&mut self, l: usize, node: NodeId, cx: &mut Cx) {
        match l {
            0 => self.stack.push(ObjectInfo {
                node,
                properties: HashMap::new(),
            }),
            1 => {
                self.stack.pop();
            }
            _ => {
                let ast = &cx.sc.ast;
                let name = get_static_property_name(ast, node);
                if !ast
                    .parent(node)
                    .is_some_and(|p| ast.is(p, "ObjectExpression"))
                {
                    return;
                }
                let Some(name) = name else { return };
                let kind = ast.str(node, "kind").unwrap_or("");
                if name == "__proto__"
                    && kind == "init"
                    && !ast.bool(node, "computed")
                    && !ast.bool(node, "shorthand")
                    && !ast.bool(node, "method")
                {
                    return;
                }
                let Some(info) = self.stack.last_mut() else {
                    return;
                };
                let is_get = kind == "init" || kind == "get";
                let is_set = kind == "init" || kind == "set";
                let entry = info
                    .properties
                    .entry(name.clone())
                    .or_insert((false, false));
                let defined = (is_get && entry.0) || (is_set && entry.1);
                if is_get {
                    entry.0 = true;
                }
                if is_set {
                    entry.1 = true;
                }
                if defined {
                    let Some(key) = ast.child(node, "key") else {
                        return;
                    };
                    let mut r = Report::loc(cx.sc.node_loc(key), "unexpected").data("name", name);
                    r.node = Some(info.node);
                    cx.report(r);
                }
            }
        }
    }
}
