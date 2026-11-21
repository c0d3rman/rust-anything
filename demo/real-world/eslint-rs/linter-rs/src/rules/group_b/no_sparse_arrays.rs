//! lib/rules/no-sparse-arrays.js

use crate::ast::NodeId;
use crate::linter::Cx;
use crate::rule::Report;
use crate::rules::{Rule, RuleDef};
use crate::tokens::N;
use crate::utils::is_comma;
use serde_json::Value;

pub static DEF: RuleDef = RuleDef {
    name: "no-sparse-arrays",
    messages: &[(
        "unexpectedSparseArray",
        "Unexpected comma in middle of array.",
    )],
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
        &["ArrayExpression"]
    }

    fn on(&mut self, _: usize, node: NodeId, cx: &mut Cx) {
        let sc = cx.sc;
        let elements = sc.ast.list(node, "elements");
        if !elements.contains(&None) {
            return;
        }
        let mut comma_end: Option<u32> = None;
        for (index, element) in elements.iter().enumerate() {
            if index == elements.len() - 1 && element.is_some() {
                return;
            }
            let from = match (element, comma_end) {
                (Some(e), _) => sc.ast.range(*e).1,
                (None, Some(c)) => c,
                (None, None) => match sc.first_token(node, N) {
                    Some(t) => t.end,
                    None => return,
                },
            };
            let Some(comma) = sc.token_after_offset(from, N, is_comma) else {
                return;
            };
            comma_end = Some(comma.end);
            if element.is_some() {
                continue;
            }
            let mut r = Report::loc(sc.tok_loc(comma), "unexpectedSparseArray");
            r.node = Some(node);
            cx.report(r);
        }
    }
}
