//! lib/rules/no-empty-static-block.js

use crate::ast::NodeId;
use crate::linter::Cx;
use crate::report::SugVal;
use crate::rule::{Report, SuggestSpec, fixer};
use crate::rules::{Rule, RuleDef};
use crate::source::Loc;
use crate::tokens::{N, Opts};
use serde_json::Value;

pub static DEF: RuleDef = RuleDef {
    name: "no-empty-static-block",
    messages: &[
        ("unexpected", "Unexpected empty static block."),
        ("suggestComment", "Add comment inside empty static block."),
    ],
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
        &["StaticBlock"]
    }

    fn on(&mut self, _: usize, node: NodeId, cx: &mut Cx) {
        let sc = cx.sc;
        if !sc.ast.list(node, "body").is_empty() {
            return;
        }
        let Some(open) = sc.first_token(
            node,
            Opts {
                include_comments: false,
                skip: 1,
            },
        ) else {
            return;
        };
        let Some(close) = sc.last_token(node, N) else {
            return;
        };
        if !sc.comments_before(close.start).is_empty() {
            return;
        }
        let loc = Loc {
            start: sc.loc_of(open.start),
            end: sc.loc_of(close.end),
        };
        let mut r = Report::loc(loc, "unexpected");
        r.node = Some(node);
        r.suggest.push(SuggestSpec {
            keys: vec![
                ("messageId", SugVal::Str("suggestComment".into())),
                ("fix", SugVal::Fix),
            ],
            fix: Some(fixer::replace_range(
                (open.end, close.start),
                " /* empty */ ",
            )),
        });
        cx.report(r);
    }
}
