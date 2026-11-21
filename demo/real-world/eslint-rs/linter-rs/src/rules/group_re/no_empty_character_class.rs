//! lib/rules/no-empty-character-class.js

use super::util::regex_of;
use crate::ast::NodeId;
use crate::linter::Cx;
use crate::regexpp::{K, parse_pattern};
use crate::rule::Report;
use crate::rules::{Rule, RuleDef};
use serde_json::Value;

pub static DEF: RuleDef = RuleDef {
    name: "no-empty-character-class",
    messages: &[("unexpected", "Empty class.")],
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
        &["Literal[regex]"]
    }

    fn on(&mut self, _: usize, node: NodeId, cx: &mut Cx) {
        let Some((pattern, flags)) = regex_of(cx, node) else {
            return;
        };
        if !pattern.windows(2).any(|w| w == [0x5b, 0x5d]) {
            return;
        }
        let Ok(ast) = parse_pattern(&pattern, flags.contains('u'), flags.contains('v')) else {
            return;
        };
        let mut count = 0;
        ast.visit(&mut |a, id, enter| {
            if let (
                true,
                K::CharacterClass {
                    negate: false,
                    elements,
                    ..
                },
            ) = (enter, &a.node(id).k)
                && elements.is_empty() {
                    count += 1;
                }
        });
        for _ in 0..count {
            cx.report(Report::node(node, "unexpected"));
        }
    }
}
