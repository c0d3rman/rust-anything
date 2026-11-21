//! lib/rules/radix.js

use crate::ast::{NodeId, Prop};
use crate::linter::Cx;
use crate::report::SugVal;
use crate::rule::{Report, SuggestSpec, fixer};
use crate::rules::{Rule, RuleDef};
use crate::tokens::N;
use crate::utils::{is_callee, is_comma, is_specific_member_access};
use serde_json::Value;

pub static DEF: RuleDef = RuleDef {
    name: "radix",
    messages: &[
        ("missingParameters", "Missing parameters."),
        ("missingRadix", "Missing radix parameter."),
        (
            "invalidRadix",
            "Invalid radix parameter, must be an integer between 2 and 36.",
        ),
        (
            "addRadixParameter10",
            "Add radix parameter `10` for parsing decimal numbers.",
        ),
    ],
    has_default_options: false,
    create,
};

struct R;

fn create(options: &[Value]) -> Result<Box<dyn Rule>, String> {
    match options {
        [] => {}
        [Value::String(s)] if s == "always" || s == "as-needed" => {}
        _ => return Err("invalid option".into()),
    }
    Ok(Box::new(R))
}

fn valid(v: f64) -> bool {
    v.fract() == 0.0 && (2.0..=36.0).contains(&v)
}

fn is_valid_radix(cx: &Cx, radix: NodeId) -> bool {
    let ast = &cx.sc.ast;
    let num = |n: NodeId| match (ast.prop(n, "bigint"), ast.prop(n, "value")) {
        (None, Some(Prop::Num(v))) => Some(*v),
        _ => None,
    };
    if ast.is(radix, "UnaryExpression")
        && let Some(op @ ("-" | "+")) = ast.str(radix, "operator")
        && let Some(arg) = ast.child(radix, "argument")
        && ast.is(arg, "Literal")
        && let Some(v) = num(arg)
    {
        let value = if op == "-" { -v } else { v };
        return valid(value);
    }
    !((ast.is(radix, "Literal") && !num(radix).is_some_and(valid))
        || (ast.is(radix, "Identifier")
            && ast.name(radix) == Some("undefined")
            && cx.sm().is_global_reference(ast, radix)))
}

fn check_arguments(cx: &mut Cx, node: NodeId) {
    let ast = &cx.sc.ast;
    let args: Vec<NodeId> = ast.nodes_of(node, "arguments").collect();
    if let Some(i) = args.iter().position(|a| ast.is(*a, "SpreadElement"))
        && i < 2
    {
        return;
    }
    match args.len() {
        0 => cx.report(Report::node(node, "missingParameters")),
        1 => {
            let fix = cx.sc.last_token(node, N).and_then(|last| {
                let prev = cx.sc.token_before_tok(last, N)?;
                let text = if is_comma(prev) { " 10," } else { ", 10" };
                Some(fixer::insert_before_range((last.start, last.end), text))
            });
            let mut r = Report::node(node, "missingRadix");
            r.suggest.push(SuggestSpec {
                keys: vec![
                    ("messageId", SugVal::Str("addRadixParameter10".into())),
                    ("fix", SugVal::Fix),
                ],
                fix,
            });
            cx.report(r);
        }
        _ => {
            if !is_valid_radix(cx, args[1]) {
                cx.report(Report::node(node, "invalidRadix"));
            }
        }
    }
}

impl Rule for R {
    fn listeners(&self) -> &'static [&'static str] {
        &["Program:exit"]
    }

    fn on(&mut self, _: usize, node: NodeId, cx: &mut Cx) {
        let sm = cx.sm();
        let scope = cx.get_scope(node);
        if let Some(v) = sm.variable_by_name(scope, "parseInt")
            && sm.var(v).defs.is_empty()
        {
            for &r in &sm.var(v).references {
                let id = sm.reference(r).identifier;
                if is_callee(&cx.sc.ast, id)
                    && let Some(p) = cx.sc.ast.parent(id)
                {
                    check_arguments(cx, p);
                }
            }
        }
        if let Some(v) = sm.variable_by_name(scope, "Number")
            && sm.var(v).defs.is_empty()
        {
            for &r in &sm.var(v).references {
                let ast = &cx.sc.ast;
                let Some(parent) = ast.parent(sm.reference(r).identifier) else {
                    continue;
                };
                let Some(pp) = ast.parent(parent) else {
                    continue;
                };
                let maybe_callee = if ast.is(pp, "ChainExpression") {
                    pp
                } else {
                    parent
                };
                if is_specific_member_access(ast, parent, Some("Number"), Some("parseInt"))
                    && is_callee(ast, maybe_callee)
                    && let Some(call) = ast.parent(maybe_callee)
                {
                    check_arguments(cx, call);
                }
            }
        }
    }
}
