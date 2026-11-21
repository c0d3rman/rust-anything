//! lib/rules/no-duplicate-case.js

use crate::ast::NodeId;
use crate::linter::Cx;
use crate::rule::Report;
use crate::rules::{Rule, RuleDef};
use crate::utils::equal_tokens;
use serde_json::Value;

pub static DEF: RuleDef = RuleDef {
    name: "no-duplicate-case",
    messages: &[("unexpected", "Duplicate case label.")],
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
        &["SwitchStatement"]
    }

    fn on(&mut self, _: usize, node: NodeId, cx: &mut Cx) {
        let sc = cx.sc;
        let ast = &sc.ast;
        let mut previous: Vec<NodeId> = Vec::new();
        for case in ast.nodes_of(node, "cases") {
            let Some(test) = ast.child(case, "test") else {
                continue;
            };
            if previous
                .iter()
                .any(|p| ast.kind(*p) == ast.kind(test) && equal_tokens(sc, *p, test))
            {
                cx.report(Report::node(case, "unexpected"));
            } else {
                previous.push(test);
            }
        }
    }
}
