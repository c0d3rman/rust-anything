//! lib/rules/dot-notation.js

use super::mini_regex::Regex;
use crate::ast::{NodeId, Prop};
use crate::linter::Cx;
use crate::report::Fix;
use crate::rule::{Report, fixer};
use crate::rules::{Rule, RuleDef};
use crate::source::{TokKind, Token};
use crate::tokens::N;
use crate::utils::{
    KEYWORDS, can_tokens_be_adjacent, is_decimal_integer, is_null_literal, is_opening_bracket,
    is_static_template_literal, template_element_cooked,
};
use serde_json::Value;

pub static DEF: RuleDef = RuleDef {
    name: "dot-notation",
    messages: &[
        ("useDot", "[{{key}}] is better written in dot notation."),
        ("useBrackets", ".{{key}} is a syntax error."),
    ],
    has_default_options: true,
    create,
};

struct R {
    allow_keywords: bool,
    allow_pattern: Option<Regex>,
    unsupported: Option<String>,
}

fn create(options: &[Value]) -> Result<Box<dyn Rule>, String> {
    let mut r = R {
        allow_keywords: true,
        allow_pattern: None,
        unsupported: None,
    };
    let mut pattern = String::new();
    match options {
        [] => {}
        [Value::Object(o)] => {
            for (k, v) in o {
                match (k.as_str(), v) {
                    ("allowKeywords", Value::Bool(b)) => r.allow_keywords = *b,
                    ("allowPattern", Value::String(s)) => pattern = s.clone(),
                    _ => return Err("invalid option".into()),
                }
            }
        }
        _ => return Err("invalid option".into()),
    }
    if !pattern.is_empty() {
        match Regex::new(&pattern) {
            Some(re) => r.allow_pattern = Some(re),
            None => r.unsupported = Some(format!("allowPattern {pattern:?}")),
        }
    }
    Ok(Box::new(r))
}

fn valid_identifier(s: &str) -> bool {
    let b = s.as_bytes();
    !b.is_empty()
        && (b[0].is_ascii_alphabetic() || b[0] == b'_' || b[0] == b'$')
        && b[1..]
            .iter()
            .all(|c| c.is_ascii_alphanumeric() || *c == b'_' || *c == b'$')
}

impl R {
    fn check_computed_property(&self, cx: &mut Cx, node: NodeId, value: &str, formatted: String) {
        let sc = cx.sc;
        let ast = &sc.ast;
        if !(valid_identifier(value)
            && (self.allow_keywords || !KEYWORDS.contains(&value))
            && !self.allow_pattern.as_ref().is_some_and(|p| p.test(value)))
        {
            return;
        }
        let Some(property) = ast.child(node, "property") else {
            return;
        };
        let mut r = Report::node(property, "useDot").data("key", formatted);
        if let Some(fixes) = self.use_dot_fix(cx, node, value) {
            r = r.fixes(fixes);
        }
        cx.report(r);
    }

    fn use_dot_fix(&self, cx: &Cx, node: NodeId, value: &str) -> Option<Vec<Fix>> {
        let sc = cx.sc;
        let ast = &sc.ast;
        let object = ast.child(node, "object")?;
        let left = sc.token_after_f(object, is_opening_bracket)?;
        let right = sc.last_token(node, N)?;
        let next = sc.token_after(node, N);
        if sc.comments_exist_between(left.end, right.start) {
            return Some(Vec::new());
        }
        let mut fixes = Vec::new();
        if !ast.bool(node, "optional") {
            let dot = if is_decimal_integer(ast, object) {
                " ."
            } else {
                "."
            };
            fixes.push(fixer::insert_before_range((left.start, left.end), dot));
        }
        fixes.push(fixer::replace_range((left.start, right.end), value));
        let word = Token {
            kind: TokKind::Identifier,
            start: 0,
            end: 0,
            value: value.to_string(),
        };
        if let Some(next) = next
            && right.end == next.start
            && !can_tokens_be_adjacent(&word, next)
        {
            fixes.push(fixer::insert_after_range(ast.range(node), " "));
        }
        Some(fixes)
    }

    fn use_brackets_fix(
        &self,
        cx: &Cx,
        node: NodeId,
        property: NodeId,
        name: &str,
    ) -> Option<Vec<Fix>> {
        let sc = cx.sc;
        let ast = &sc.ast;
        let dot = sc.token_before(property, N)?;
        let object = ast.child(node, "object")?;
        let optional = ast.bool(node, "optional");
        if ast.is(object, "Identifier") && ast.name(object) == Some("let") && !optional {
            return Some(Vec::new());
        }
        if sc.comments_exist_between(dot.end, ast.range(property).0) {
            return Some(Vec::new());
        }
        let mut fixes = Vec::new();
        if !optional {
            fixes.push(fixer::remove_range((dot.start, dot.end)));
        }
        fixes.push(fixer::replace(sc, property, format!("[\"{name}\"]")));
        Some(fixes)
    }
}

impl Rule for R {
    fn listeners(&self) -> &'static [&'static str] {
        &["MemberExpression"]
    }

    fn on(&mut self, _: usize, node: NodeId, cx: &mut Cx) {
        if self.unsupported.is_some() {
            return;
        }
        let ast = &cx.sc.ast;
        let computed = ast.bool(node, "computed");
        let Some(property) = ast.child(node, "property") else {
            return;
        };
        if computed && ast.is(property, "Literal") {
            let value = match ast.prop(property, "value") {
                Some(Prop::Str(s)) => {
                    let mut quoted = String::new();
                    crate::json::quote(s, &mut quoted);
                    Some((s.clone(), quoted))
                }
                Some(Prop::Bool(b)) => Some((b.to_string(), b.to_string())),
                _ if is_null_literal(ast, property) => {
                    Some(("null".to_string(), "null".to_string()))
                }
                _ => None,
            };
            if let Some((value, formatted)) = value {
                self.check_computed_property(cx, node, &value, formatted);
            }
        }
        if computed && is_static_template_literal(ast, property) {
            let cooked = ast
                .list(property, "quasis")
                .first()
                .copied()
                .flatten()
                .and_then(|q| template_element_cooked(ast, q));
            match cooked {
                Some(v) => {
                    let v = v.to_string();
                    self.check_computed_property(cx, node, &v, format!("`{v}`"));
                }
                None => {
                    self.unsupported = Some("template with null cooked value".into());
                    return;
                }
            }
        }
        let ast = &cx.sc.ast;
        if !self.allow_keywords
            && !computed
            && ast.is(property, "Identifier")
            && let Some(name) = ast.name(property)
            && KEYWORDS.contains(&name)
        {
            let name = name.to_string();
            let mut r = Report::node(property, "useBrackets").data("key", name.as_str());
            if let Some(fixes) = self.use_brackets_fix(cx, node, property, &name) {
                r = r.fixes(fixes);
            }
            cx.report(r);
        }
    }

    fn unsupported(&self) -> Option<&str> {
        self.unsupported.as_deref()
    }
}
