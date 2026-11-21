//! lib/rules/no-nested-ternary.js

use crate::ast::NodeId;
use crate::linter::Cx;
use crate::rule::Report;
use crate::rules::{Rule, RuleDef};
use serde_json::Value;

pub static DEF: RuleDef = RuleDef {
    name: "no-nested-ternary",
    messages: &[("noNestedTernary", "Do not nest ternary expressions.")],
    has_default_options: false,
    create,
};

struct R;

fn create(options: &[Value]) -> Result<Box<dyn Rule>, String> {
    if !options.is_empty() {
        return Err("invalid option".into());
    }
    Ok(Box::new(R))
}

impl Rule for R {
    fn listeners(&self) -> &'static [&'static str] {
        &["ConditionalExpression"]
    }

    fn on(&mut self, _: usize, node: NodeId, cx: &mut Cx) {
        let ast = &cx.sc.ast;
        let nested = |k: &str| {
            ast.child(node, k)
                .is_some_and(|c| ast.is(c, "ConditionalExpression"))
        };
        if nested("alternate") || nested("consequent") {
            cx.report(Report::node(node, "noNestedTernary"));
        }
    }
}
