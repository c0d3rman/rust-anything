//! lib/rules/curly.js

use crate::ast::NodeId;
use crate::linter::Cx;
use crate::report::Fix;
use crate::rule::{Report, fixer};
use crate::rules::{Rule, RuleDef};
use crate::source::{SourceCode, TokKind, Token};
use crate::tokens::{N, Opts};
use crate::utils::{
    are_braces_necessary, can_tokens_be_adjacent, is_semicolon, node_by_range_index,
};
use serde_json::Value;

pub static DEF: RuleDef = RuleDef {
    name: "curly",
    messages: &[
        ("missingCurlyAfter", "Expected { after '{{name}}'."),
        (
            "missingCurlyAfterCondition",
            "Expected { after '{{name}}' condition.",
        ),
        ("unexpectedCurlyAfter", "Unnecessary { after '{{name}}'."),
        (
            "unexpectedCurlyAfterCondition",
            "Unnecessary { after '{{name}}' condition.",
        ),
    ],
    has_default_options: true,
    create,
};

#[derive(PartialEq)]
enum Mode {
    All,
    Multi,
    MultiLine,
    MultiOrNest,
}

struct R {
    mode: Mode,
    consistent: bool,
}

fn create(options: &[Value]) -> Result<Box<dyn Rule>, String> {
    let s = |i: usize| options.get(i).and_then(Value::as_str);
    let r = match options.len() {
        0 => R {
            mode: Mode::All,
            consistent: false,
        },
        1 | 2 => {
            let mode = match s(0) {
                Some("all") if options.len() == 1 => Mode::All,
                Some("multi") => Mode::Multi,
                Some("multi-line") => Mode::MultiLine,
                Some("multi-or-nest") => Mode::MultiOrNest,
                _ => return Err("invalid option".into()),
            };
            if options.len() == 2 && s(1) != Some("consistent") {
                return Err("invalid option".into());
            }
            R {
                mode,
                consistent: options.len() == 2,
            }
        }
        _ => return Err("invalid option".into()),
    };
    Ok(Box::new(r))
}

struct Check {
    node: NodeId,
    body: NodeId,
    name: &'static str,
    condition: bool,
    actual: bool,
    expected: Option<bool>,
}

fn last_excluding_semicolon<'a>(sc: &'a SourceCode, last: &'a Token) -> Option<&'a Token> {
    if is_semicolon(last) {
        sc.token_before_tok(last, N)
    } else {
        Some(last)
    }
}

fn is_collapsed_one_liner(sc: &SourceCode, node: NodeId) -> Option<bool> {
    let before = sc.token_before(node, N)?;
    let last = last_excluding_semicolon(sc, sc.last_token(node, N)?)?;
    Some(sc.loc_of(before.start).line == sc.loc_of(last.end).line)
}

fn is_one_liner(sc: &SourceCode, node: NodeId) -> Option<bool> {
    if sc.ast.is(node, "EmptyStatement") {
        return Some(true);
    }
    let first = sc.first_token(node, N)?;
    let last = last_excluding_semicolon(sc, sc.last_token(node, N)?)?;
    Some(sc.loc_of(first.start).line == sc.loc_of(last.end).line)
}

fn needs_semicolon(sc: &SourceCode, closing: &Token) -> Option<bool> {
    let ast = &sc.ast;
    let before = sc.token_before_tok(closing, N)?;
    let after = sc.token_after_tok(closing, N);
    let last_block_node = node_by_range_index(sc, before.start)?;
    if is_semicolon(before) {
        return Some(false);
    }
    let Some(after) = after else {
        return Some(false);
    };
    if ast.is(last_block_node, "BlockStatement") {
        let pk = ast
            .parent(last_block_node)
            .map(|p| ast.kind(p))
            .unwrap_or("");
        if pk != "FunctionExpression" && pk != "ArrowFunctionExpression" {
            return Some(false);
        }
    }
    if sc.loc_of(before.end).line == sc.loc_of(after.start).line {
        return Some(true);
    }
    if after.value.starts_with(['(', '[', '/', '`', '+', '-']) {
        return Some(true);
    }
    Some(before.kind == TokKind::Punctuator && (before.value == "++" || before.value == "--"))
}

fn remove_braces_fix(sc: &SourceCode, body: NodeId) -> Option<Fix> {
    let (bs, _) = sc.ast.range(body);
    let before_body = sc.token_before(body, N)?;
    let needs_space = before_body.end == bs
        && !can_tokens_be_adjacent(
            before_body,
            sc.first_token(
                body,
                Opts {
                    include_comments: false,
                    skip: 1,
                },
            )?,
        );
    let open = sc.first_token(body, N)?;
    let close = sc.last_token(body, N)?;
    let last_in_block = sc.token_before_tok(close, N)?;
    if needs_semicolon(sc, close)? {
        return None;
    }
    let text = format!(
        "{}{}{}{}",
        if needs_space { " " } else { "" },
        sc.slice16(open.end, last_in_block.start),
        sc.slice16(last_in_block.start, last_in_block.end),
        sc.slice16(last_in_block.end, close.start)
    );
    Some(fixer::replace(sc, body, text))
}

impl R {
    fn prepare(
        &self,
        sc: &SourceCode,
        node: NodeId,
        body: NodeId,
        name: &'static str,
        condition: bool,
    ) -> Option<Check> {
        let ast = &sc.ast;
        let has_block = ast.is(body, "BlockStatement");
        let mut expected = None;
        if has_block && (ast.list(body, "body").len() != 1 || are_braces_necessary(sc, body)) {
            expected = Some(true);
        } else if self.mode == Mode::Multi {
            expected = Some(false);
        } else if self.mode == Mode::MultiLine {
            if !is_collapsed_one_liner(sc, body)? {
                expected = Some(true);
            }
        } else if self.mode == Mode::MultiOrNest {
            if has_block {
                let statement = ast.list(body, "body").first().copied().flatten()?;
                let leading = sc.comments_before(ast.range(statement).0);
                expected = Some(!is_one_liner(sc, statement)? || !leading.is_empty());
            } else {
                expected = Some(!is_one_liner(sc, body)?);
            }
        } else {
            expected = Some(true);
        }
        Some(Check {
            node,
            body,
            name,
            condition,
            actual: has_block,
            expected,
        })
    }

    fn check(&self, cx: &mut Cx, c: &Check) {
        let sc = cx.sc;
        let Some(expected) = c.expected else { return };
        if expected == c.actual {
            return;
        }
        let loc = sc.node_loc(c.body);
        let mut r = if expected {
            let id = if c.condition {
                "missingCurlyAfterCondition"
            } else {
                "missingCurlyAfter"
            };
            Report::loc(loc, id).fix(fixer::replace(
                sc,
                c.body,
                format!("{{{}}}", sc.node_text(c.body)),
            ))
        } else {
            let id = if c.condition {
                "unexpectedCurlyAfterCondition"
            } else {
                "unexpectedCurlyAfter"
            };
            let mut r = Report::loc(loc, id);
            if let Some(f) = remove_braces_fix(sc, c.body) {
                r = r.fix(f);
            }
            r
        };
        r.node = Some(c.node);
        cx.report(r.data("name", c.name));
    }

    fn prepare_if_checks(&self, sc: &SourceCode, node: NodeId) -> Option<Vec<Check>> {
        let ast = &sc.ast;
        let mut checks = Vec::new();
        let mut current = Some(node);
        while let Some(n) = current {
            checks.push(self.prepare(sc, n, ast.child(n, "consequent")?, "if", true)?);
            let alternate = ast.child(n, "alternate");
            if let Some(a) = alternate
                && !ast.is(a, "IfStatement")
            {
                checks.push(self.prepare(sc, n, a, "else", false)?);
                break;
            }
            current = alternate;
        }
        if self.consistent {
            let expected = checks.iter().any(|c| c.expected.unwrap_or(c.actual));
            for c in checks.iter_mut() {
                c.expected = Some(expected);
            }
        }
        Some(checks)
    }
}

impl Rule for R {
    fn listeners(&self) -> &'static [&'static str] {
        &[
            "IfStatement",
            "WhileStatement",
            "DoWhileStatement",
            "ForStatement",
            "ForInStatement",
            "ForOfStatement",
        ]
    }

    fn on(&mut self, l: usize, node: NodeId, cx: &mut Cx) {
        let sc = cx.sc;
        let ast = &sc.ast;
        let checks = if l == 0 {
            let Some(parent) = ast.parent(node) else {
                return;
            };
            if ast.is(parent, "IfStatement") && ast.child(parent, "alternate") == Some(node) {
                return;
            }
            self.prepare_if_checks(sc, node)
        } else {
            let (name, condition) = match l {
                1 => ("while", true),
                2 => ("do", false),
                3 => ("for", true),
                4 => ("for-in", false),
                _ => ("for-of", false),
            };
            ast.child(node, "body")
                .and_then(|b| self.prepare(sc, node, b, name, condition))
                .map(|c| vec![c])
        };
        for c in checks.iter().flatten() {
            self.check(cx, c);
        }
    }
}
