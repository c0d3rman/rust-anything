//! lib/rules/no-useless-catch.js

use crate::ast::NodeId;
use crate::linter::Cx;
use crate::rule::Report;
use crate::rules::{Rule, RuleDef};
use serde_json::Value;

pub static DEF: RuleDef = RuleDef {
    name: "no-useless-catch",
    messages: &[
        ("unnecessaryCatchClause", "Unnecessary catch clause."),
        ("unnecessaryCatch", "Unnecessary try/catch wrapper."),
    ],
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
        &["CatchClause"]
    }

    fn on(&mut self, _: usize, node: NodeId, cx: &mut Cx) {
        let ast = &cx.sc.ast;
        let Some(param) = ast.child(node, "param") else {
            return;
        };
        if !ast.is(param, "Identifier") {
            return;
        }
        let Some(body) = ast.child(node, "body") else {
            return;
        };
        let Some(Some(first)) = ast.list(body, "body").first().copied() else {
            return;
        };
        if !ast.is(first, "ThrowStatement") {
            return;
        }
        let Some(arg) = ast.child(first, "argument") else {
            return;
        };
        if !(ast.is(arg, "Identifier") && ast.name(arg) == ast.name(param)) {
            return;
        }
        let Some(parent) = ast.parent(node) else {
            return;
        };
        if ast.child(parent, "finalizer").is_some() {
            cx.report(Report::node(node, "unnecessaryCatchClause"));
        } else {
            cx.report(Report::node(parent, "unnecessaryCatch"));
        }
    }
}
