//! lib/rules/no-octal.js

use crate::ast::NodeId;
use crate::linter::Cx;
use crate::rule::Report;
use crate::rules::{Rule, RuleDef};
use crate::utils::literal_typeof;
use serde_json::Value;

pub static DEF: RuleDef = RuleDef {
    name: "no-octal",
    messages: &[("noOctal", "Octal literals should not be used.")],
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
        &["Literal"]
    }

    fn on(&mut self, _: usize, node: NodeId, cx: &mut Cx) {
        let ast = &cx.sc.ast;
        if literal_typeof(ast, node) != "number" {
            return;
        }
        let raw = ast.str(node, "raw").unwrap_or("").as_bytes();
        if raw.len() >= 2 && raw[0] == b'0' && raw[1].is_ascii_digit() {
            cx.report(Report::node(node, "noOctal"));
        }
    }
}
