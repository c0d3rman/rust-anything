//! lib/rules/no-global-assign.js

use crate::ast::NodeId;
use crate::linter::Cx;
use crate::rule::Report;
use crate::rules::{Rule, RuleDef};
use serde_json::Value;

pub static DEF: RuleDef = RuleDef {
    name: "no-global-assign",
    messages: &[("globalShouldNotBeModified", "Read-only global '{{name}}' should not be modified.")],
    has_default_options: true,
    create,
};

struct R {
    exceptions: Vec<String>,
}

fn create(options: &[Value]) -> Result<Box<dyn Rule>, String> {
    let mut exceptions = Vec::new();
    match options {
        [] => {}
        [Value::Object(o)] => {
            for (k, v) in o {
                match (k.as_str(), v) {
                    ("exceptions", Value::Array(a)) => {
                        for x in a {
                            let Value::String(s) = x else { return Err("invalid option".into()) };
                            if exceptions.contains(s) {
                                return Err("invalid option".into());
                            }
                            exceptions.push(s.clone());
                        }
                    }
                    _ => return Err("invalid option".into()),
                }
            }
        }
        _ => return Err("invalid option".into()),
    }
    Ok(Box::new(R { exceptions }))
}

impl Rule for R {
    fn listeners(&self) -> &'static [&'static str] {
        &["Program"]
    }

    fn on(&mut self, _: usize, node: NodeId, cx: &mut Cx) {
        let sm = cx.sm();
        let ast = &cx.sc.ast;
        let global = cx.get_scope(node);
        for &v in &sm.scope(global).variables {
            let var = sm.var(v);
            if var.writeable != Some(false) || self.exceptions.contains(&var.name) {
                continue;
            }
            for (i, &r) in var.references.iter().enumerate() {
                let rf = sm.reference(r);
                let id = rf.identifier;
                if rf.init == Some(false) && rf.is_write() && (i == 0 || sm.reference(var.references[i - 1]).identifier != id) {
                    let name = ast.name(id).unwrap_or("").to_string();
                    cx.report(Report::node(id, "globalShouldNotBeModified").data("name", name));
                }
            }
        }
    }
}
