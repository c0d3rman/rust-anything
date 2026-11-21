//! lib/rules/no-dupe-args.js

use crate::ast::NodeId;
use crate::linter::Cx;
use crate::rule::Report;
use crate::rules::{Rule, RuleDef};
use crate::scope::DefType;
use crate::source::Loc;
use crate::tokens::N;
use crate::utils::get_opening_paren_of_params;
use serde_json::Value;

pub static DEF: RuleDef = RuleDef {
    name: "no-dupe-args",
    messages: &[("unexpected", "Duplicate param '{{name}}'.")],
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
        &["FunctionDeclaration", "FunctionExpression"]
    }

    fn on(&mut self, _: usize, node: NodeId, cx: &mut Cx) {
        let sm = cx.sm();
        for &v in cx.declared_variables(node) {
            let var = sm.var(v);
            let defs = var
                .defs
                .iter()
                .filter(|d| d.kind == DefType::Parameter)
                .count();
            if defs < 2 {
                continue;
            }
            let Some(open) = get_opening_paren_of_params(cx.sc, node) else {
                continue;
            };
            let Some(body) = cx.sc.ast.child(node, "body") else {
                continue;
            };
            let Some(before) = cx.sc.token_before(body, N) else {
                continue;
            };
            let loc = Loc {
                start: cx.sc.loc_of(open.start),
                end: cx.sc.loc_of(before.end),
            };
            cx.report(Report::loc(loc, "unexpected").data("name", var.name.as_str()));
        }
    }
}
