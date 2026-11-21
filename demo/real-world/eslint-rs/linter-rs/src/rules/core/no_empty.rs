//! lib/rules/no-empty.js

use crate::rules::{Rule, RuleDef};
use crate::ast::NodeId;
use crate::linter::Cx;
use crate::report::SugVal;
use crate::rule::{fixer, Report, SuggestSpec};
use crate::source::Loc;
use crate::utils::{is_function, is_opening_brace};
use serde_json::Value;

pub static DEF: RuleDef = RuleDef {
    name: "no-empty",
    messages: &[
        ("unexpected", "Empty {{type}} statement."),
        ("suggestComment", "Add comment inside empty {{type}} statement."),
    ],
    has_default_options: true,
    create,
};

struct R {
    allow_empty_catch: bool,
}

fn create(options: &[Value]) -> Result<Box<dyn Rule>, String> {
    let mut allow = false;
    match options {
        [] => {}
        [Value::Object(o)] => {
            for (k, v) in o {
                match (k.as_str(), v) {
                    ("allowEmptyCatch", Value::Bool(b)) => allow = *b,
                    _ => return Err("invalid option".into()),
                }
            }
        }
        _ => return Err("invalid option".into()),
    }
    Ok(Box::new(R { allow_empty_catch: allow }))
}

fn suggest(kind: &str, range: (u32, u32)) -> SuggestSpec {
    SuggestSpec {
        keys: vec![
            ("messageId", SugVal::Str("suggestComment".into())),
            ("data", SugVal::Data(vec![("type".into(), kind.into())])),
            ("fix", SugVal::Fix),
        ],
        fix: Some(fixer::replace_range(range, " /* empty */ ")),
    }
}

impl Rule for R {
    fn listeners(&self) -> &'static [&'static str] {
        &["BlockStatement", "SwitchStatement"]
    }

    fn on(&mut self, l: usize, node: NodeId, cx: &mut Cx) {
        let ast = &cx.sc.ast;
        if l == 0 {
            if !ast.list(node, "body").is_empty() {
                return;
            }
            let Some(parent) = ast.parent(node) else { return };
            if is_function(ast, parent) {
                return;
            }
            if self.allow_empty_catch && ast.is(parent, "CatchClause") {
                return;
            }
            if !cx.sc.comments_inside(node).is_empty() {
                return;
            }
            let (s, e) = ast.range(node);
            let mut r = Report::node(node, "unexpected").data("type", "block");
            r.suggest.push(suggest("block", (s + 1, e - 1)));
            cx.report(r);
        } else {
            if !ast.list(node, "cases").is_empty() {
                return;
            }
            let Some(disc) = ast.child(node, "discriminant") else { return };
            let Some(open) = cx.sc.token_after_f(disc, is_opening_brace) else { return };
            let Some(close) = cx.sc.last_token(node, crate::tokens::N) else { return };
            if cx.sc.comments_exist_between(open.end, close.start) {
                return;
            }
            let loc = Loc { start: cx.sc.loc_of(open.start), end: cx.sc.loc_of(close.end) };
            let range = (open.end, close.start);
            let mut r = Report::loc(loc, "unexpected").data("type", "switch");
            r.node = Some(node);
            r.suggest.push(suggest("switch", range));
            cx.report(r);
        }
    }
}
