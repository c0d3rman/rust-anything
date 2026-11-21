//! lib/rules/prefer-template.js

use crate::ast::{Ast, NodeId};
use crate::linter::Cx;
use crate::rule::{Report, fixer};
use crate::rules::{Rule, RuleDef};
use crate::source::SourceCode;
use crate::tokens::N;
use crate::utils::{
    has_octal_or_non_octal_decimal_escape_sequence, is_start_of_expression_statement,
    is_string_literal, literal_typeof, needs_preceding_semicolon,
};
use serde_json::Value;
use std::collections::HashSet;

pub static DEF: RuleDef = RuleDef {
    name: "prefer-template",
    messages: &[(
        "unexpectedStringConcatenation",
        "Unexpected string concatenation.",
    )],
    has_default_options: false,
    create,
};

struct R {
    done: HashSet<u32>,
}

fn create(options: &[Value]) -> Result<Box<dyn Rule>, String> {
    super::no_options(options)?;
    Ok(Box::new(R {
        done: HashSet::new(),
    }))
}

fn is_concatenation(ast: &Ast, n: NodeId) -> bool {
    ast.is(n, "BinaryExpression") && ast.str(n, "operator") == Some("+")
}

fn is_string(ast: &Ast, n: NodeId) -> bool {
    ast.is(n, "Literal") && literal_typeof(ast, n) == "string"
}

fn top_concat(ast: &Ast, n: NodeId) -> NodeId {
    let mut cur = n;
    while let Some(p) = ast.parent(cur) {
        if !is_concatenation(ast, p) {
            break;
        }
        cur = p;
    }
    cur
}

fn sides(ast: &Ast, n: NodeId) -> Option<(NodeId, NodeId)> {
    Some((ast.child(n, "left")?, ast.child(n, "right")?))
}

fn has_octal_escape(ast: &Ast, n: NodeId) -> bool {
    if is_concatenation(ast, n) {
        return sides(ast, n)
            .is_some_and(|(l, r)| has_octal_escape(ast, l) || has_octal_escape(ast, r));
    }
    is_string(ast, n)
        && has_octal_or_non_octal_decimal_escape_sequence(ast.str(n, "raw").unwrap_or(""))
}

fn has_string_literal(ast: &Ast, n: NodeId) -> bool {
    if is_concatenation(ast, n) {
        return sides(ast, n)
            .is_some_and(|(l, r)| has_string_literal(ast, r) || has_string_literal(ast, l));
    }
    is_string_literal(ast, n)
}

fn has_non_string_literal(ast: &Ast, n: NodeId) -> bool {
    if is_concatenation(ast, n) {
        return sides(ast, n).is_some_and(|(l, r)| {
            has_non_string_literal(ast, r) || has_non_string_literal(ast, l)
        });
    }
    !is_string_literal(ast, n)
}

fn quasi_empty(ast: &Ast, n: NodeId, last: bool) -> bool {
    let q = ast.list(n, "quasis");
    let e = if last { q.last() } else { q.first() };
    let Some(Some(e)) = e else { return false };
    !ast.list(n, "expressions").is_empty() && {
        let (s, t) = ast.range(*e);
        s == t
    }
}

fn starts_with_template_curly(ast: &Ast, n: NodeId) -> bool {
    if ast.is(n, "BinaryExpression") {
        return ast
            .child(n, "left")
            .is_some_and(|l| starts_with_template_curly(ast, l));
    }
    if ast.is(n, "TemplateLiteral") {
        return quasi_empty(ast, n, false);
    }
    !is_string(ast, n)
}

fn ends_with_template_curly(ast: &Ast, n: NodeId) -> bool {
    if ast.is(n, "BinaryExpression") {
        return ast
            .child(n, "right")
            .is_some_and(|r| starts_with_template_curly(ast, r));
    }
    if ast.is(n, "TemplateLiteral") {
        return quasi_empty(ast, n, true);
    }
    !is_string(ast, n)
}

fn text_between(sc: &SourceCode, a: (u32, u32), b: (u32, u32)) -> String {
    let mut ranges = vec![a];
    ranges.extend(
        sc.tokens_between(a.1, b.0, N)
            .iter()
            .map(|t| (t.start, t.end)),
    );
    ranges.push(b);
    let mut out = String::new();
    for w in ranges.windows(2) {
        out.push_str(sc.slice16(w[0].1, w[1].0));
    }
    out
}

/// `` raw.slice(1, -1).replace(/(?<!\\)\\*(\$\{|`)/gu, ...).replace(new RegExp(`\\\\${raw[0]}`, "gu"), raw[0]) ``
fn escape_string_body(raw: &str) -> String {
    let mut chars = raw.chars();
    let quote = chars.next().unwrap_or('"');
    chars.next_back();
    let c: Vec<char> = chars.collect();
    let mut out = String::new();
    let mut i = 0;
    while i < c.len() {
        if i == 0 || c[i - 1] != '\\' {
            let mut j = i;
            while j < c.len() && c[j] == '\\' {
                j += 1;
            }
            let tail = if c.get(j) == Some(&'$') && c.get(j + 1) == Some(&'{') {
                2
            } else if c.get(j) == Some(&'`') {
                1
            } else {
                0
            };
            if tail > 0 {
                if (j - i) % 2 == 0 {
                    out.push('\\');
                }
                out.extend(&c[i..j + tail]);
                i = j + tail;
                continue;
            }
        }
        out.push(c[i]);
        i += 1;
    }
    out.replace(&format!("\\{quote}"), &quote.to_string())
}

fn template_literal(
    sc: &SourceCode,
    n: NodeId,
    before: Option<&str>,
    after: Option<&str>,
) -> Option<String> {
    let ast = &sc.ast;
    if is_string(ast, n) {
        return Some(format!("`{}`", escape_string_body(ast.str(n, "raw")?)));
    }
    if ast.is(n, "TemplateLiteral") {
        return Some(sc.node_text(n).to_string());
    }
    if is_concatenation(ast, n) && has_string_literal(ast, n) {
        let (l, r) = sides(ast, n)?;
        let plus = sc.first_token_between(l, r, |t| t.value == "+")?;
        let plus = (plus.start, plus.end);
        let before_plus = text_between(sc, ast.range(l), plus);
        let after_plus = text_between(sc, plus, ast.range(r));
        if ends_with_template_curly(ast, l) {
            let joined = format!("{before_plus}{after_plus}");
            let a = template_literal(sc, l, before, Some(&joined))?;
            let b = template_literal(sc, r, None, after)?;
            return Some(format!("{}{}", drop_last(&a), drop_first(&b)));
        }
        if starts_with_template_curly(ast, r) {
            let joined = format!("{before_plus}{after_plus}");
            let a = template_literal(sc, l, before, None)?;
            let b = template_literal(sc, r, Some(&joined), after)?;
            return Some(format!("{}{}", drop_last(&a), drop_first(&b)));
        }
        let a = template_literal(sc, l, before, None)?;
        let b = template_literal(sc, r, after, None)?;
        return Some(format!("{a}{before_plus}+{after_plus}{b}"));
    }
    Some(format!(
        "`${{{}{}{}}}`",
        before.unwrap_or(""),
        sc.node_text(n),
        after.unwrap_or("")
    ))
}

fn drop_last(s: &str) -> &str {
    let mut c = s.chars();
    c.next_back();
    c.as_str()
}

fn drop_first(s: &str) -> &str {
    let mut c = s.chars();
    c.next();
    c.as_str()
}

impl Rule for R {
    fn listeners(&self) -> &'static [&'static str] {
        &["Program", "Literal", "TemplateLiteral"]
    }

    fn on(&mut self, l: usize, node: NodeId, cx: &mut Cx) {
        let sc = cx.sc;
        let ast = &sc.ast;
        if l == 0 {
            self.done.clear();
            return;
        }
        let Some(parent) = ast.parent(node) else {
            return;
        };
        if !is_string_literal(ast, node) || !is_concatenation(ast, parent) {
            return;
        }
        let top = top_concat(ast, parent);
        if !self.done.insert(ast.range(top).0) {
            return;
        }
        if !has_non_string_literal(ast, top) {
            return;
        }
        let mut r = Report::node(top, "unexpectedStringConcatenation");
        if !has_octal_escape(ast, top) {
            let prefix = if is_start_of_expression_statement(ast, top)
                && needs_preceding_semicolon(sc, top)
            {
                ";"
            } else {
                ""
            };
            if let Some(t) = template_literal(sc, top, None, None) {
                r = r.fix(fixer::replace(sc, top, format!("{prefix}{t}")));
            }
        }
        cx.report(r);
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn escapes() {
        assert_eq!(super::escape_string_body(r"'a${b}'"), r"a\${b}");
        assert_eq!(super::escape_string_body(r"'a\${b}'"), r"a\${b}");
        assert_eq!(super::escape_string_body(r"'a\\${b}'"), r"a\\\${b}");
        assert_eq!(super::escape_string_body("'`'"), r"\`");
        assert_eq!(super::escape_string_body(r"'\'x'"), "'x");
        assert_eq!(super::escape_string_body(r#""\"x""#), "\"x");
    }
}
