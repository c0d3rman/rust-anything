//! lib/rules/no-debugger.js

use crate::rules::{Rule, RuleDef};
use crate::ast::NodeId;
use crate::linter::Cx;
use crate::rule::Report;
use serde_json::Value;

pub static DEF: RuleDef = RuleDef {
    name: "no-debugger",
    messages: &[("unexpected", "Unexpected 'debugger' statement.")],
    has_default_options: false,
    create,
};

struct R;

fn create(options: &[Value]) -> Result<Box<dyn Rule>, String> {
    if !options.is_empty() {
        return Err("no options allowed".into());
    }
    Ok(Box::new(R))
}

impl Rule for R {
    fn listeners(&self) -> &'static [&'static str] {
        &["DebuggerStatement"]
    }

    fn on(&mut self, _: usize, node: NodeId, cx: &mut Cx) {
        cx.report(Report::node(node, "unexpected"));
    }
}
