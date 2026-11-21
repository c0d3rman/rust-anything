//! lib/rules/no-unexpected-multiline.js

use crate::ast::NodeId;
use crate::linter::Cx;
use crate::rule::Report;
use crate::rules::{Rule, RuleDef};
use crate::source::{Loc, Pos, TokKind};
use crate::tokens::N;
use crate::utils::is_closing_paren;
use serde_json::Value;

pub static DEF: RuleDef = RuleDef {
    name: "no-unexpected-multiline",
    messages: &[
        (
            "function",
            "Unexpected newline between function and ( of function call.",
        ),
        (
            "property",
            "Unexpected newline between object and [ of property access.",
        ),
        (
            "taggedTemplate",
            "Unexpected newline between template tag and template literal.",
        ),
        (
            "division",
            "Unexpected newline between numerator and division operator.",
        ),
    ],
    has_default_options: false,
    create,
};

struct R;

fn create(options: &[Value]) -> Result<Box<dyn Rule>, String> {
    super::no_options(options)?;
    Ok(Box::new(R))
}

fn check_for_break_after(cx: &mut Cx, node: NodeId, message_id: &'static str) {
    let sc = cx.sc;
    let Some(open) = sc.token_after_offset(sc.ast.range(node).1, N, |t| !is_closing_paren(t))
    else {
        return;
    };
    let Some(before) = sc.token_before_tok(open, N) else {
        return;
    };
    let loc = sc.tok_loc(open);
    if loc.start.line != sc.loc_of(before.end).line {
        let mut r = Report::loc(loc, message_id);
        r.node = Some(node);
        cx.report(r);
    }
}

impl Rule for R {
    fn listeners(&self) -> &'static [&'static str] {
        &[
            "MemberExpression",
            "TaggedTemplateExpression",
            "CallExpression",
            "BinaryExpression[operator='/'] > BinaryExpression[operator='/'].left",
        ]
    }

    fn on(&mut self, l: usize, node: NodeId, cx: &mut Cx) {
        let sc = cx.sc;
        let ast = &sc.ast;
        match l {
            0 => {
                if !ast.bool(node, "computed") || ast.bool(node, "optional") {
                    return;
                }
                if let Some(o) = ast.child(node, "object") {
                    check_for_break_after(cx, o, "property");
                }
            }
            1 => {
                let Some(quasi) = ast.child(node, "quasi") else {
                    return;
                };
                let Some(before) = sc.token_before(quasi, N) else {
                    return;
                };
                let start = sc.loc_of(ast.range(quasi).0);
                if sc.loc_of(before.end).line != start.line {
                    let loc = Loc {
                        start,
                        end: Pos {
                            line: start.line,
                            column: start.column + 1,
                        },
                    };
                    let mut r = Report::loc(loc, "taggedTemplate");
                    r.node = Some(node);
                    cx.report(r);
                }
            }
            2 => {
                if ast.list(node, "arguments").is_empty() || ast.bool(node, "optional") {
                    return;
                }
                if let Some(c) = ast.child(node, "callee") {
                    check_for_break_after(cx, c, "function");
                }
            }
            _ => {
                if ast.str(node, "operator") != Some("/") {
                    return;
                }
                let Some(parent) = ast.parent(node) else {
                    return;
                };
                if !(ast.is(parent, "BinaryExpression")
                    && ast.str(parent, "operator") == Some("/")
                    && ast.child(parent, "left") == Some(node))
                {
                    return;
                }
                let Some(second) = sc.token_after_offset(ast.range(node).1, N, |t| t.value == "/")
                else {
                    return;
                };
                let Some(after) = sc.token_after_tok(second, N) else {
                    return;
                };
                if after.kind == TokKind::Identifier
                    && !after.value.is_empty()
                    && after.value.chars().all(|c| "dgimsuvy".contains(c))
                    && second.end == after.start
                    && let Some(left) = ast.child(node, "left")
                {
                    check_for_break_after(cx, left, "division");
                }
            }
        }
    }
}
