//! lib/rules/no-with.js

use crate::ast::NodeId;
use crate::linter::Cx;
use crate::rule::Report;
use crate::rules::{Rule, RuleDef};
use serde_json::Value;

pub static DEF: RuleDef = RuleDef {
    name: "no-with",
    messages: &[("unexpectedWith", "Unexpected use of 'with' statement.")],
    has_default_options: false,
    create,
};

struct R;

fn create(options: &[Value]) -> Result<Box<dyn Rule>, String> {
    super::no_options(options)?;
    Ok(Box::new(R))
}

impl Rule for R {
    fn listeners(&self) -> &'static [&'static str] {
        &["WithStatement"]
    }

    fn on(&mut self, _: usize, node: NodeId, cx: &mut Cx) {
        let Some(first) = cx.sc.first_token(node, crate::tokens::N) else {
            return;
        };
        let mut r = Report::loc(cx.sc.tok_loc(first), "unexpectedWith");
        r.node = Some(node);
        cx.report(r);
    }
}
