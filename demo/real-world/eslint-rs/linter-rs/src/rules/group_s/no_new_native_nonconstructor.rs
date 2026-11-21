//! lib/rules/no-new-native-nonconstructor.js

use crate::ast::NodeId;
use crate::linter::Cx;
use crate::rule::Report;
use crate::rules::{Rule, RuleDef};
use serde_json::Value;

pub static DEF: RuleDef = RuleDef {
    name: "no-new-native-nonconstructor",
    messages: &[(
        "noNewNonconstructor",
        "`{{name}}` cannot be called as a constructor.",
    )],
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
        &["Program:exit"]
    }

    fn on(&mut self, _: usize, node: NodeId, cx: &mut Cx) {
        let sm = cx.sm();
        let global = cx.get_scope(node);
        for name in ["Symbol", "BigInt"] {
            let Some(v) = sm.lookup(global, name) else {
                continue;
            };
            let var = sm.var(v);
            if !var.defs.is_empty() {
                continue;
            }
            for &r in &var.references {
                let id = sm.reference(r).identifier;
                let ast = &cx.sc.ast;
                if let Some(p) = ast.parent(id)
                    && ast.is(p, "NewExpression")
                    && ast.child(p, "callee") == Some(id)
                {
                    cx.report(Report::node(id, "noNewNonconstructor").data("name", name));
                }
            }
        }
    }
}
