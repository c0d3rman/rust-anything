//! lib/rules/default-case.js

use super::mini_regex::Regex;
use crate::ast::NodeId;
use crate::linter::Cx;
use crate::rule::Report;
use crate::rules::{Rule, RuleDef};
use crate::source::Loc;
use crate::utils::{is_closing_paren, js_trim};
use serde_json::Value;

pub static DEF: RuleDef = RuleDef {
    name: "default-case",
    messages: &[("missingDefaultCase", "Expected a default case.")],
    has_default_options: true,
    create,
};

struct R {
    pattern: Option<Regex>,
    unsupported: Option<String>,
}

fn create(options: &[Value]) -> Result<Box<dyn Rule>, String> {
    let mut pattern = None;
    match options {
        [] => {}
        [Value::Object(o)] => {
            for (k, v) in o {
                match (k.as_str(), v) {
                    ("commentPattern", Value::String(s)) => pattern = Some(s.clone()),
                    _ => return Err("invalid option".into()),
                }
            }
        }
        _ => return Err("invalid option".into()),
    }
    let mut r = R {
        pattern: None,
        unsupported: None,
    };
    if let Some(p) = pattern.filter(|p| !p.is_empty()) {
        match Regex::new(&p) {
            Some(re) => r.pattern = Some(re),
            None => r.unsupported = Some(format!("commentPattern {p:?}")),
        }
    }
    Ok(Box::new(r))
}

impl R {
    fn matches(&self, text: &str) -> bool {
        match &self.pattern {
            Some(re) => re.test(text),
            None => text.eq_ignore_ascii_case("no default"),
        }
    }
}

impl Rule for R {
    fn listeners(&self) -> &'static [&'static str] {
        &["SwitchStatement"]
    }

    fn on(&mut self, _: usize, node: NodeId, cx: &mut Cx) {
        if self.unsupported.is_some() {
            return;
        }
        let sc = cx.sc;
        let ast = &sc.ast;
        let cases: Vec<NodeId> = ast.nodes_of(node, "cases").collect();
        let (Some(&first), Some(&last)) = (cases.first(), cases.last()) else {
            return;
        };
        if cases.iter().any(|c| ast.child(*c, "test").is_none()) {
            return;
        }
        let comments = sc.comments_after(ast.range(last).1);
        if let Some(comment) = comments.last()
            && self.matches(js_trim(&comment.value))
        {
            return;
        }
        let Some(paren) = sc.token_before_f(first, is_closing_paren) else {
            return;
        };
        let loc = Loc {
            start: sc.node_loc(node).start,
            end: sc.loc_of(paren.end),
        };
        let mut r = Report::loc(loc, "missingDefaultCase");
        r.node = Some(node);
        cx.report(r);
    }

    fn unsupported(&self) -> Option<&str> {
        self.unsupported.as_deref()
    }
}
