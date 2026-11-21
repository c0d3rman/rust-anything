//! lib/rules/no-lonely-if.js

use crate::ast::NodeId;
use crate::linter::Cx;
use crate::report::Fix;
use crate::rule::{Report, fixer};
use crate::rules::{Rule, RuleDef};
use crate::source::SourceCode;
use crate::tokens::N;
use crate::utils::{are_braces_necessary, js_trim};
use serde_json::Value;

pub static DEF: RuleDef = RuleDef {
    name: "no-lonely-if",
    messages: &[(
        "unexpectedLonelyIf",
        "Unexpected if as the only statement in an else block.",
    )],
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

fn fix(sc: &SourceCode, node: NodeId, parent: NodeId) -> Option<Fix> {
    let ast = &sc.ast;
    let open = sc.first_token(parent, N)?;
    let close = sc.last_token(parent, N)?;
    let else_kw = sc.token_before_tok(open, N)?;
    let after = sc.token_after_tok(close, N);
    let consequent = ast.child(node, "consequent")?;
    let last_if = sc.last_token(consequent, N)?;
    let (ns, ne) = ast.range(node);
    if !js_trim(sc.slice16(open.end, ns)).is_empty()
        || !js_trim(sc.slice16(ne, close.start)).is_empty()
    {
        return None;
    }
    if !ast.is(consequent, "BlockStatement")
        && last_if.value != ";"
        && let Some(after) = after
        && (sc.node_loc(consequent).end.line == sc.loc_of(after.start).line
            || after.value.starts_with(['(', '[', '/', '+', '`', '-'])
            || last_if.value == "++"
            || last_if.value == "--")
    {
        return None;
    }
    let prefix = if else_kw.end == open.start { " " } else { "" };
    Some(fixer::replace_range(
        (open.start, close.end),
        format!("{prefix}{}", sc.node_text(node)),
    ))
}

impl Rule for R {
    fn listeners(&self) -> &'static [&'static str] {
        &["IfStatement"]
    }

    fn on(&mut self, _: usize, node: NodeId, cx: &mut Cx) {
        let sc = cx.sc;
        let ast = &sc.ast;
        let Some(parent) = ast.parent(node) else {
            return;
        };
        if !(ast.is(parent, "BlockStatement")
            && ast.list(parent, "body").len() == 1
            && !are_braces_necessary(sc, parent))
        {
            return;
        }
        let Some(grand) = ast.parent(parent) else {
            return;
        };
        if !(ast.is(grand, "IfStatement") && ast.child(grand, "alternate") == Some(parent)) {
            return;
        }
        let mut r = Report::node(node, "unexpectedLonelyIf");
        if let Some(f) = fix(sc, node, parent) {
            r = r.fix(f);
        }
        cx.report(r);
    }
}
