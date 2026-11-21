//! lib/rules/no-dupe-else-if.js

use crate::ast::{Ast, NodeId};
use crate::linter::Cx;
use crate::rule::Report;
use crate::rules::{Rule, RuleDef};
use crate::source::SourceCode;
use crate::utils::equal_tokens;
use serde_json::Value;

pub static DEF: RuleDef = RuleDef {
    name: "no-dupe-else-if",
    messages: &[(
        "unexpected",
        "This branch can never execute. Its condition is a duplicate or covered by previous conditions in the if-else-if chain.",
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

fn split(ast: &Ast, op: &str, n: NodeId, out: &mut Vec<NodeId>) {
    if ast.is(n, "LogicalExpression")
        && ast.str(n, "operator") == Some(op)
        && let (Some(l), Some(r)) = (ast.child(n, "left"), ast.child(n, "right"))
    {
        split(ast, op, l, out);
        split(ast, op, r, out);
        return;
    }
    out.push(n);
}

fn split_by(ast: &Ast, op: &str, n: NodeId) -> Vec<NodeId> {
    let mut out = Vec::new();
    split(ast, op, n, &mut out);
    out
}

fn or_of_ands(ast: &Ast, n: NodeId) -> Vec<Vec<NodeId>> {
    split_by(ast, "||", n)
        .into_iter()
        .map(|c| split_by(ast, "&&", c))
        .collect()
}

fn equal(sc: &SourceCode, a: NodeId, b: NodeId) -> bool {
    let ast = &sc.ast;
    if ast.kind(a) != ast.kind(b) {
        return false;
    }
    let op = ast.str(a, "operator");
    if ast.is(a, "LogicalExpression")
        && matches!(op, Some("||" | "&&"))
        && op == ast.str(b, "operator")
    {
        let (Some(al), Some(ar), Some(bl), Some(br)) = (
            ast.child(a, "left"),
            ast.child(a, "right"),
            ast.child(b, "left"),
            ast.child(b, "right"),
        ) else {
            return false;
        };
        return (equal(sc, al, bl) && equal(sc, ar, br))
            || (equal(sc, al, br) && equal(sc, ar, bl));
    }
    equal_tokens(sc, a, b)
}

fn is_subset(sc: &SourceCode, a: &[NodeId], b: &[NodeId]) -> bool {
    a.iter().all(|x| b.iter().any(|y| equal(sc, *x, *y)))
}

impl Rule for R {
    fn listeners(&self) -> &'static [&'static str] {
        &["IfStatement"]
    }

    fn on(&mut self, _: usize, node: NodeId, cx: &mut Cx) {
        let sc = cx.sc;
        let ast = &sc.ast;
        let Some(test) = ast.child(node, "test") else {
            return;
        };
        let mut conditions = vec![test];
        if ast.is(test, "LogicalExpression") && ast.str(test, "operator") == Some("&&") {
            conditions.extend(split_by(ast, "&&", test));
        }
        let mut list: Vec<Vec<Vec<NodeId>>> =
            conditions.iter().map(|c| or_of_ands(ast, *c)).collect();
        let mut current = node;
        while let Some(parent) = ast.parent(current)
            && ast.is(parent, "IfStatement")
            && ast.child(parent, "alternate") == Some(current)
        {
            current = parent;
            let Some(ptest) = ast.child(current, "test") else {
                return;
            };
            let current_or = or_of_ands(ast, ptest);
            for or_operands in list.iter_mut() {
                or_operands.retain(|op| !current_or.iter().any(|c| is_subset(sc, c, op)));
            }
            if list.iter().any(Vec::is_empty) {
                cx.report(Report::node(test, "unexpected"));
                break;
            }
        }
    }
}
