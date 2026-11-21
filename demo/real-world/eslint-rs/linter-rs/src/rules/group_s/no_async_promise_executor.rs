//! lib/rules/no-async-promise-executor.js

use crate::ast::NodeId;
use crate::linter::Cx;
use crate::rule::Report;
use crate::rules::{Rule, RuleDef};
use serde_json::Value;

pub static DEF: RuleDef = RuleDef {
    name: "no-async-promise-executor",
    messages: &[("async", "Promise executor functions should not be async.")],
    has_default_options: false,
    create,
};

struct R;

fn create(options: &[Value]) -> Result<Box<dyn Rule>, String> {
    super::common::no_options(options)?;
    Ok(Box::new(R))
}

impl Rule for R {
    fn listeners(&self) -> &'static [&'static str] {
        &["NewExpression[callee.name='Promise'][arguments.0.async=true]"]
    }

    fn on(&mut self, _: usize, node: NodeId, cx: &mut Cx) {
        let ast = &cx.sc.ast;
        let Some(callee) = ast.child(node, "callee") else {
            return;
        };
        if ast.name(callee) != Some("Promise") {
            return;
        }
        let Some(first) = ast.list(node, "arguments").first().copied().flatten() else {
            return;
        };
        if !ast.bool(first, "async") {
            return;
        }
        if !cx.sm().is_global_reference(ast, callee) {
            return;
        }
        let Some(tok) = cx.sc.first_token_f(first, |t| t.value == "async") else {
            return;
        };
        let loc = cx.sc.tok_loc(tok);
        cx.report(Report::loc(loc, "async"));
    }
}
