//! lib/rules/no-empty-pattern.js

use crate::ast::NodeId;
use crate::linter::Cx;
use crate::rule::Report;
use crate::rules::{Rule, RuleDef};
use crate::utils::is_function;
use serde_json::Value;

pub static DEF: RuleDef = RuleDef {
    name: "no-empty-pattern",
    messages: &[("unexpected", "Unexpected empty {{type}} pattern.")],
    has_default_options: true,
    create,
};

struct R {
    allow_params: bool,
}

fn create(options: &[Value]) -> Result<Box<dyn Rule>, String> {
    let mut allow_params = false;
    match options {
        [] => {}
        [Value::Object(o)] => {
            for (k, v) in o {
                match (k.as_str(), v) {
                    ("allowObjectPatternsAsParameters", Value::Bool(b)) => allow_params = *b,
                    _ => return Err("invalid option".into()),
                }
            }
        }
        _ => return Err("invalid option".into()),
    }
    Ok(Box::new(R { allow_params }))
}

impl Rule for R {
    fn listeners(&self) -> &'static [&'static str] {
        &["ObjectPattern", "ArrayPattern"]
    }

    fn on(&mut self, l: usize, node: NodeId, cx: &mut Cx) {
        let ast = &cx.sc.ast;
        if l == 0 {
            if !ast.list(node, "properties").is_empty() {
                return;
            }
            if self.allow_params
                && let Some(parent) = ast.parent(node)
            {
                if is_function(ast, parent) {
                    return;
                }
                if ast.is(parent, "AssignmentPattern")
                    && ast.parent(parent).is_some_and(|g| is_function(ast, g))
                    && ast.child(parent, "right").is_some_and(|r| {
                        ast.is(r, "ObjectExpression") && ast.list(r, "properties").is_empty()
                    })
                {
                    return;
                }
            }
            cx.report(Report::node(node, "unexpected").data("type", "object"));
        } else if ast.list(node, "elements").is_empty() {
            cx.report(Report::node(node, "unexpected").data("type", "array"));
        }
    }
}
