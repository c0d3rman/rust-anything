//! lib/rules/no-nonoctal-decimal-escape.js

use crate::ast::NodeId;
use crate::linter::Cx;
use crate::report::SugVal;
use crate::rule::{Report, SuggestSpec, fixer};
use crate::rules::{Rule, RuleDef};
use crate::source::{Loc, SourceCode};
use crate::utils::literal_typeof;
use serde_json::Value;

pub static DEF: RuleDef = RuleDef {
    name: "no-nonoctal-decimal-escape",
    messages: &[
        (
            "decimalEscape",
            "Don't use '{{decimalEscape}}' escape sequence.",
        ),
        (
            "refactor",
            "Replace '{{original}}' with '{{replacement}}'. This maintains the current functionality.",
        ),
        (
            "escapeBackslash",
            "Replace '{{original}}' with '{{replacement}}' to include the actual backslash character.",
        ),
    ],
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

fn suggestion(
    sc: &SourceCode,
    message_id: &str,
    range: (u32, u32),
    replacement: String,
) -> SuggestSpec {
    SuggestSpec {
        keys: vec![
            ("messageId", SugVal::Str(message_id.into())),
            (
                "data",
                SugVal::Data(vec![
                    ("original".into(), sc.slice16(range.0, range.1).into()),
                    ("replacement".into(), replacement.as_str().into()),
                ]),
            ),
            ("fix", SugVal::Fix),
        ],
        fix: Some(fixer::replace_range(range, replacement)),
    }
}

/// The matches of `/(?:[^\\]|(?<previousEscape>\\.))*?(?<decimalEscape>\\[89])/suy` over `raw`:
/// (UTF-16 end of the match, previousEscape, the escaped digit).
fn decimal_escapes(raw: &str) -> Vec<(u32, Option<char>, char)> {
    let chars: Vec<char> = raw.chars().collect();
    let mut out = Vec::new();
    let mut i = 0;
    let mut pos16 = 0u32;
    let mut previous: Option<char> = None;
    while i < chars.len() {
        if chars[i] == '\\' {
            match chars.get(i + 1) {
                Some(d @ ('8' | '9')) => {
                    pos16 += 2;
                    out.push((pos16, previous, *d));
                    previous = None;
                    i += 2;
                }
                Some(c) => {
                    previous = Some(*c);
                    pos16 += 1 + c.len_utf16() as u32;
                    i += 2;
                }
                None => break,
            }
        } else {
            previous = None;
            pos16 += chars[i].len_utf16() as u32;
            i += 1;
        }
    }
    out
}

impl Rule for R {
    fn listeners(&self) -> &'static [&'static str] {
        &["Literal"]
    }

    fn on(&mut self, _: usize, node: NodeId, cx: &mut Cx) {
        let sc = cx.sc;
        let ast = &sc.ast;
        if literal_typeof(ast, node) != "string" {
            return;
        }
        let raw = ast.str(node, "raw").unwrap_or("");
        let start = ast.range(node).0;
        for (end, previous, digit) in decimal_escapes(raw) {
            let range_end = start + end;
            let range_start = range_end - 2;
            let range = (range_start, range_end);
            let decimal_escape = format!("\\{digit}");
            let mut suggest = Vec::new();
            if previous == Some('0') {
                suggest.push(suggestion(
                    sc,
                    "refactor",
                    (range_start - 2, range_end),
                    format!("\\u0000{digit}"),
                ));
                suggest.push(suggestion(
                    sc,
                    "refactor",
                    range,
                    format!("\\u{:04x}", digit as u32),
                ));
            } else {
                suggest.push(suggestion(sc, "refactor", range, digit.to_string()));
            }
            suggest.push(suggestion(
                sc,
                "escapeBackslash",
                range,
                format!("\\{decimal_escape}"),
            ));
            let loc = Loc {
                start: sc.loc_of(range_start),
                end: sc.loc_of(range_end),
            };
            let mut r =
                Report::loc(loc, "decimalEscape").data("decimalEscape", decimal_escape.as_str());
            r.node = Some(node);
            r.suggest = suggest;
            cx.report(r);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::decimal_escapes;

    #[test]
    fn scan() {
        assert_eq!(decimal_escapes(r#""\8""#), vec![(3, None, '8')]);
        assert_eq!(
            decimal_escapes(r#""\0\9\8""#),
            vec![(5, Some('0'), '9'), (7, None, '8')]
        );
        assert_eq!(decimal_escapes(r#""\\8""#), vec![]);
        assert_eq!(decimal_escapes(r#""é\8""#), vec![(4, None, '8')]);
    }
}
