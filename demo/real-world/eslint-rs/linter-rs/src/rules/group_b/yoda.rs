//! lib/rules/yoda.js

use crate::ast::{Ast, NodeId, Prop};
use crate::linter::Cx;
use crate::rule::{Report, fixer};
use crate::rules::{Rule, RuleDef};
use crate::source::{SourceCode, TokKind, Token};
use crate::tokens::N;
use crate::utils::{
    can_tokens_be_adjacent, is_numeric_literal, is_parenthesised, is_same_reference,
    is_start_of_expression_statement, is_static_template_literal, needs_preceding_semicolon,
    template_element_cooked,
};
use serde_json::Value;

pub static DEF: RuleDef = RuleDef {
    name: "yoda",
    messages: &[(
        "expected",
        "Expected literal to be on the {{expectedSide}} side of {{operator}}.",
    )],
    has_default_options: true,
    create,
};

struct R {
    always: bool,
    except_range: bool,
    only_equality: bool,
    unsupported: Option<&'static str>,
}

fn create(options: &[Value]) -> Result<Box<dyn Rule>, String> {
    if options.len() > 2 {
        return Err("invalid option".into());
    }
    let when = match options.first() {
        None => "never",
        Some(Value::String(s)) if s == "always" || s == "never" => s.as_str(),
        _ => return Err("invalid option".into()),
    };
    let o = super::bool_props(options, 1, &["exceptRange", "onlyEquality"])?;
    Ok(Box::new(R {
        always: when == "always",
        except_range: o[0].unwrap_or(false),
        only_equality: o[1].unwrap_or(false),
        unsupported: None,
    }))
}

fn is_negative_numeric_literal(ast: &Ast, n: NodeId) -> bool {
    ast.is(n, "UnaryExpression")
        && ast.str(n, "operator") == Some("-")
        && ast.bool(n, "prefix")
        && ast
            .child(n, "argument")
            .is_some_and(|a| is_numeric_literal(ast, a))
}

fn looks_like_literal(ast: &Ast, n: NodeId) -> bool {
    is_negative_numeric_literal(ast, n) || is_static_template_literal(ast, n)
}

enum Lit {
    Num(f64),
    Big(i64),
    Str(String),
    Unknown,
}

fn literal_value(ast: &Ast, n: NodeId) -> Lit {
    if let Some(b) = ast.str(n, "bigint") {
        return b.parse::<i64>().map(Lit::Big).unwrap_or(Lit::Unknown);
    }
    if ast.prop(n, "regex").is_some() {
        return Lit::Unknown;
    }
    match ast.prop(n, "value") {
        Some(Prop::Num(x)) => Lit::Num(*x),
        Some(Prop::Str(s)) => Lit::Str(s.clone()),
        Some(Prop::Bool(b)) => Lit::Num(if *b { 1.0 } else { 0.0 }),
        Some(Prop::Null) => Lit::Num(0.0),
        _ => Lit::Unknown,
    }
}

fn normalized_literal(ast: &Ast, n: NodeId) -> Option<Lit> {
    if ast.is(n, "Literal") {
        return Some(literal_value(ast, n));
    }
    if is_negative_numeric_literal(ast, n) {
        return Some(
            match ast.child(n, "argument").map(|a| literal_value(ast, a)) {
                Some(Lit::Num(x)) => Lit::Num(-x),
                Some(Lit::Big(x)) => Lit::Big(-x),
                _ => Lit::Unknown,
            },
        );
    }
    if is_static_template_literal(ast, n) {
        let q = ast.list(n, "quasis").first().copied().flatten();
        return Some(match q.and_then(|q| template_element_cooked(ast, q)) {
            Some(s) => Lit::Str(s.to_string()),
            None => Lit::Unknown,
        });
    }
    None
}

/// `StringToNumber` for plain decimal strings; `None` for anything else that is not obviously `NaN`.
fn string_to_number(s: &str) -> Option<f64> {
    let t = s.trim_matches(super::is_js_whitespace);
    if t.is_empty() {
        return Some(0.0);
    }
    let body = t.strip_prefix(['+', '-']).unwrap_or(t);
    let (mant, exp) = match body.find(['e', 'E']) {
        Some(i) => (&body[..i], Some(&body[i + 1..])),
        None => (body, None),
    };
    let digits = mant.replacen('.', "", 1);
    let mant_ok = !digits.is_empty() && digits.bytes().all(|b| b.is_ascii_digit());
    let exp_ok = exp.is_none_or(|e| {
        let e = e.strip_prefix(['+', '-']).unwrap_or(e);
        !e.is_empty() && e.bytes().all(|b| b.is_ascii_digit())
    });
    if mant_ok && exp_ok {
        return t.parse::<f64>().ok();
    }
    if t.chars().all(|c| c.is_ascii_alphabetic()) && !t.contains("Infinity") {
        return Some(f64::NAN);
    }
    None
}

fn big_as_f64(b: i64) -> Option<f64> {
    if b.unsigned_abs() < (1u64 << 53) {
        Some(b as f64)
    } else {
        None
    }
}

fn less_or_equal(a: &Lit, b: &Lit) -> Option<bool> {
    match (a, b) {
        (Lit::Num(x), Lit::Num(y)) => Some(x <= y),
        (Lit::Big(x), Lit::Big(y)) => Some(x <= y),
        (Lit::Big(x), Lit::Num(y)) => Some(big_as_f64(*x)? <= *y),
        (Lit::Num(x), Lit::Big(y)) => Some(*x <= big_as_f64(*y)?),
        (Lit::Str(x), Lit::Str(y)) => {
            Some(x.encode_utf16().cmp(y.encode_utf16()) != std::cmp::Ordering::Greater)
        }
        (Lit::Str(x), Lit::Num(y)) => Some(string_to_number(x)? <= *y),
        (Lit::Num(x), Lit::Str(y)) => Some(*x <= string_to_number(y)?),
        _ => None,
    }
}

fn is_range_test(cx: &Cx, node: NodeId) -> Option<bool> {
    let sc = cx.sc;
    let ast = &sc.ast;
    if !ast.is(node, "LogicalExpression") {
        return Some(false);
    }
    let (Some(left), Some(right)) = (ast.child(node, "left"), ast.child(node, "right")) else {
        return Some(false);
    };
    let range_op = |n: NodeId| {
        ast.is(n, "BinaryExpression") && matches!(ast.str(n, "operator"), Some("<" | "<="))
    };
    if !range_op(left) || !range_op(right) {
        return Some(false);
    }
    let (Some(ll), Some(lr), Some(rl), Some(rr)) = (
        ast.child(left, "left"),
        ast.child(left, "right"),
        ast.child(right, "left"),
        ast.child(right, "right"),
    ) else {
        return Some(false);
    };
    let op = ast.str(node, "operator");
    let compare = |a: Option<Lit>, b: Option<Lit>| -> Option<bool> {
        match (a, b) {
            (None, None) => Some(false),
            (None, _) | (_, None) => Some(true),
            (Some(a), Some(b)) => less_or_equal(&a, &b),
        }
    };
    let between = if op == Some("&&") && is_same_reference(ast, lr, rl, false) {
        compare(normalized_literal(ast, ll), normalized_literal(ast, rr))?
    } else {
        false
    };
    let outside = !between
        && if op == Some("||") && is_same_reference(ast, ll, rr, false) {
            compare(normalized_literal(ast, lr), normalized_literal(ast, rl))?
        } else {
            false
        };
    Some((between || outside) && is_parenthesised(sc, node))
}

fn starts_function_or_class(sc: &SourceCode, token: &Token, node: NodeId) -> bool {
    if token.kind == TokKind::Keyword && (token.value == "function" || token.value == "class") {
        return true;
    }
    let next = sc.token_after_tok(token, N);
    matches!(token.kind, TokKind::Identifier | TokKind::Keyword)
        && token.value == "async"
        && next.is_some_and(|n| {
            n.end <= sc.ast.range(node).1 && n.kind == TokKind::Keyword && n.value == "function"
        })
}

fn can_continue_expression(t: &Token) -> bool {
    matches!(t.kind, TokKind::Template | TokKind::RegularExpression)
        || (t.kind == TokKind::Punctuator && matches!(t.value.as_str(), "(" | "[" | "+" | "-"))
}

fn continues_postfix_update(t: &Token, before: Option<&Token>) -> bool {
    before.is_some_and(|b| b.kind == TokKind::Punctuator && (b.value == "++" || b.value == "--"))
        && (t.kind == TokKind::RegularExpression
            || (t.kind == TokKind::Punctuator && (t.value == "+" || t.value == "-")))
}

fn flip(op: &str) -> &str {
    match op {
        "<" => ">",
        ">" => "<",
        "<=" => ">=",
        ">=" => "<=",
        o => o,
    }
}

fn flipped_string(sc: &SourceCode, node: NodeId) -> Option<String> {
    let ast = &sc.ast;
    let op = ast.str(node, "operator")?;
    let (l, r) = (ast.child(node, "left")?, ast.child(node, "right")?);
    let op_tok = sc.first_token_between(l, r, |t| t.value == op)?;
    let last_left = sc.token_before_tok(op_tok, N)?;
    let first_right = sc.token_after_tok(op_tok, N)?;
    let (ns, ne) = ast.range(node);
    let left_text = sc.slice16(ns, last_left.end);
    let before_op = sc.slice16(last_left.end, op_tok.start);
    let after_op = sc.slice16(op_tok.end, first_right.start);
    let right_text = sc.slice16(first_right.start, ne);
    let token_before = sc.token_before(node, N);
    let token_after = sc.token_after(node, N);
    let mut prefix = "";
    let mut suffix = "";
    if let Some(tb) = token_before
        && tb.end == ns
        && !can_tokens_be_adjacent(tb, first_right)
    {
        prefix = " ";
    }
    if let Some(ta) = token_after
        && ne == ta.start
        && !can_tokens_be_adjacent(last_left, ta)
    {
        suffix = " ";
    }
    let mut flipped = format!(
        "{right_text}{before_op}{}{after_op}{left_text}",
        flip(&op_tok.value)
    );
    let start_of_stmt = is_start_of_expression_statement(ast, node);
    let needs_parens = ((start_of_stmt
        || token_before.is_some_and(|t| t.kind == TokKind::Keyword && t.value == "default"))
        && starts_function_or_class(sc, first_right, node))
        || ((start_of_stmt
            || token_before.is_some_and(|t| t.kind == TokKind::Punctuator && t.value == "=>"))
            && first_right.kind == TokKind::Punctuator
            && first_right.value == "{");
    if needs_parens {
        flipped = format!("({flipped})");
    }
    if prefix.is_empty()
        && start_of_stmt
        && (needs_parens || can_continue_expression(first_right))
        && (needs_preceding_semicolon(sc, node)
            || continues_postfix_update(first_right, token_before))
    {
        prefix = ";";
    }
    Some(format!("{prefix}{flipped}{suffix}"))
}

impl Rule for R {
    fn listeners(&self) -> &'static [&'static str] {
        &["BinaryExpression"]
    }

    fn on(&mut self, _: usize, node: NodeId, cx: &mut Cx) {
        let sc = cx.sc;
        let ast = &sc.ast;
        let (Some(left), Some(right)) = (ast.child(node, "left"), ast.child(node, "right")) else {
            return;
        };
        let (expected_literal, expected_non_literal) = if self.always {
            (left, right)
        } else {
            (right, left)
        };
        let op = ast.str(node, "operator").unwrap_or("");
        let is_lit = |n: NodeId| ast.is(n, "Literal") || looks_like_literal(ast, n);
        if !(is_lit(expected_non_literal)
            && !is_lit(expected_literal)
            && !(!matches!(op, "==" | "===") && self.only_equality)
            && matches!(op, "==" | "===" | "!=" | "!==" | "<" | ">" | "<=" | ">="))
        {
            return;
        }
        if self.except_range
            && let Some(parent) = ast.parent(node)
        {
            match is_range_test(cx, parent) {
                Some(true) => return,
                Some(false) => {}
                None => {
                    self.unsupported = Some("comparing literal values of mixed types");
                    return;
                }
            }
        }
        let mut r = Report::node(node, "expected")
            .data("operator", op)
            .data("expectedSide", if self.always { "left" } else { "right" });
        if let Some(text) = flipped_string(sc, node) {
            r = r.fix(fixer::replace(sc, node, text));
        }
        cx.report(r);
    }

    fn unsupported(&self) -> Option<&str> {
        self.unsupported
    }
}

#[cfg(test)]
mod tests {
    use super::{Lit, less_or_equal, string_to_number};

    #[test]
    fn string_numbers() {
        assert_eq!(string_to_number(" 12 "), Some(12.0));
        assert_eq!(string_to_number(""), Some(0.0));
        assert_eq!(string_to_number("1e3"), Some(1000.0));
        assert_eq!(string_to_number(".5"), Some(0.5));
        assert!(string_to_number("a").is_some_and(f64::is_nan));
        assert_eq!(string_to_number("0x10"), None);
        assert_eq!(string_to_number("Infinity"), None);
        assert_eq!(
            less_or_equal(&Lit::Str("a".into()), &Lit::Num(1.0)),
            Some(false)
        );
        assert_eq!(less_or_equal(&Lit::Big(-1), &Lit::Big(1)), Some(true));
    }
}
