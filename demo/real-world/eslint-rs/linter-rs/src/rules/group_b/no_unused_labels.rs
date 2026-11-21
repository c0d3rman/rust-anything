//! lib/rules/no-unused-labels.js

use crate::ast::NodeId;
use crate::linter::Cx;
use crate::rule::{Report, fixer};
use crate::rules::{Rule, RuleDef};
use crate::tokens::{C, N};
use crate::utils::{is_function, is_static_template_literal, literal_typeof};
use serde_json::Value;

pub static DEF: RuleDef = RuleDef {
    name: "no-unused-labels",
    messages: &[("unused", "'{{name}}:' is defined but never used.")],
    has_default_options: false,
    create,
};

struct Info {
    label: String,
    used: bool,
}

struct R {
    stack: Vec<Info>,
}

fn create(options: &[Value]) -> Result<Box<dyn Rule>, String> {
    super::no_options(options)?;
    Ok(Box::new(R { stack: Vec::new() }))
}

fn is_fixable(cx: &Cx, node: NodeId) -> bool {
    let sc = cx.sc;
    let ast = &sc.ast;
    let (Some(label), Some(body)) = (ast.child(node, "label"), ast.child(node, "body")) else {
        return false;
    };
    let after = sc.token_after(label, C).map(|t| t.start);
    let before = sc.token_before(body, C).map(|t| t.start);
    if after != before {
        return false;
    }
    let mut ancestor = ast.parent(node);
    while let Some(a) = ancestor {
        if !ast.is(a, "LabeledStatement") {
            break;
        }
        ancestor = ast.parent(a);
    }
    let Some(ancestor) = ancestor else {
        return false;
    };
    if (ast.is(ancestor, "Program")
        || (ast.is(ancestor, "BlockStatement")
            && ast.parent(ancestor).is_some_and(|p| is_function(ast, p))))
        && ast.is(body, "ExpressionStatement")
        && let Some(e) = ast.child(body, "expression")
        && ((ast.is(e, "Literal") && literal_typeof(ast, e) == "string")
            || is_static_template_literal(ast, e))
    {
        return false;
    }
    let token_before = sc.token_before(node, N);
    let Some(first_body) = sc.first_token(body, N) else {
        return false;
    };
    if let Some(tb) = token_before
        && !matches!(tb.value.as_str(), ":" | ";" | "{")
        && first_body.value.starts_with(['(', '[', '-', '+', '/', '`'])
    {
        return false;
    }
    true
}

impl Rule for R {
    fn listeners(&self) -> &'static [&'static str] {
        &[
            "LabeledStatement",
            "LabeledStatement:exit",
            "BreakStatement",
            "ContinueStatement",
        ]
    }

    fn on(&mut self, l: usize, node: NodeId, cx: &mut Cx) {
        let sc = cx.sc;
        let ast = &sc.ast;
        let Some(label) = ast.child(node, "label") else {
            return;
        };
        let name = ast.name(label).unwrap_or("");
        match l {
            0 => self.stack.push(Info {
                label: name.to_string(),
                used: false,
            }),
            1 => {
                let Some(info) = self.stack.pop() else { return };
                if info.used {
                    return;
                }
                let mut r = Report::node(label, "unused").data("name", name);
                if is_fixable(cx, node)
                    && let Some(body) = ast.child(node, "body")
                {
                    r = r.fix(fixer::remove_range((ast.range(node).0, ast.range(body).0)));
                }
                cx.report(r);
            }
            _ => {
                if let Some(info) = self.stack.iter_mut().rev().find(|i| i.label == name) {
                    info.used = true;
                }
            }
        }
    }
}
