//! lib/rules/no-invalid-regexp.js

use super::util::{string_value, utf16};
use crate::ast::NodeId;
use crate::linter::Cx;
use crate::regexpp::validate_pattern;
use crate::rule::Report;
use crate::rules::{Rule, RuleDef};
use serde_json::Value;

pub static DEF: RuleDef = RuleDef {
    name: "no-invalid-regexp",
    messages: &[("regexMessage", "{{message}}.")],
    has_default_options: true,
    create,
};

const VALID_FLAGS: &str = "dgimsuvy";

struct R {
    allowed_flags: Vec<Vec<u16>>,
    unsupported: Option<&'static str>,
}

fn create(options: &[Value]) -> Result<Box<dyn Rule>, String> {
    let allow = super::string_array_option(options, "allowConstructorFlags")?;
    let mut allowed_flags: Vec<Vec<u16>> = Vec::new();
    if let Some(list) = allow {
        let temp: String = list
            .concat()
            .chars()
            .filter(|c| !VALID_FLAGS.contains(*c))
            .collect();
        for c in temp.chars() {
            let u = utf16(&c.to_string());
            if !allowed_flags.contains(&u) {
                allowed_flags.push(u);
            }
        }
    }
    Ok(Box::new(R {
        allowed_flags,
        unsupported: None,
    }))
}

fn code_points(s: &[u16]) -> Vec<Vec<u16>> {
    let mut out = Vec::new();
    let mut i = 0;
    while i < s.len() {
        let w = if (0xd800..0xdc00).contains(&s[i])
            && s.get(i + 1).is_some_and(|t| (0xdc00..0xe000).contains(t))
        {
            2
        } else {
            1
        };
        out.push(s[i..i + w].to_vec());
        i += w;
    }
    out
}

fn replace_first(s: &[u16], needle: &[u16]) -> Vec<u16> {
    if needle.is_empty() {
        return s.to_vec();
    }
    match s.windows(needle.len()).position(|w| w == needle) {
        Some(i) => [&s[..i], &s[i + needle.len()..]].concat(),
        None => s.to_vec(),
    }
}

impl R {
    fn report(&mut self, cx: &mut Cx, node: NodeId, message: &[u16], lossy: bool) {
        let Ok(message) = String::from_utf16(message) else {
            self.unsupported = Some("message with a lone surrogate");
            return;
        };
        if lossy {
            self.unsupported = Some("message with a lone surrogate");
            return;
        }
        cx.report(Report::node(node, "regexMessage").data("message", message));
    }
}

fn validate(pattern: &[u16], u: bool, v: bool) -> Option<(Vec<u16>, bool)> {
    validate_pattern(pattern, u, v)
        .err()
        .map(|e| (utf16(&e.message), e.lossy))
}

impl Rule for R {
    fn listeners(&self) -> &'static [&'static str] {
        &["CallExpression, NewExpression"]
    }

    fn on(&mut self, _: usize, node: NodeId, cx: &mut Cx) {
        let ast = &cx.sc.ast;
        let Some(callee) = ast.child(node, "callee") else {
            return;
        };
        if !ast.is(callee, "Identifier")
            || ast.name(callee) != Some("RegExp")
            || !cx.sm().is_global_reference(ast, callee)
        {
            return;
        }
        let args = ast.list(node, "arguments");
        let flags: Option<Vec<u16>> = if args.len() < 2 {
            Some(Vec::new())
        } else {
            args[1].and_then(|f| string_value(cx, f))
        };
        let mut all_flags: Vec<Vec<u16>> =
            VALID_FLAGS.chars().map(|c| utf16(&c.to_string())).collect();
        all_flags.extend(self.allowed_flags.iter().cloned());
        let mut flags_to_check = flags.clone();
        if let Some(f) = flags.as_ref().filter(|f| !f.is_empty()) {
            let mut rest = f.clone();
            for flag in &all_flags {
                rest = replace_first(&rest, flag);
            }
            flags_to_check = Some(rest);
        }
        let mut duplicate: Vec<u16> = Vec::new();
        if let Some(check) = &flags_to_check {
            for cp in code_points(check) {
                if all_flags.contains(&cp) {
                    duplicate.extend(cp);
                }
            }
        }
        let message: Option<Vec<u16>> = if flags
            .as_ref()
            .is_some_and(|f| f.contains(&0x75) && f.contains(&0x76))
        {
            Some(utf16("Regex 'u' and 'v' flags cannot be used together"))
        } else if !duplicate.is_empty() {
            Some(
                [
                    utf16("Duplicate flags ('"),
                    duplicate,
                    utf16("') supplied to RegExp constructor"),
                ]
                .concat(),
            )
        } else {
            flags_to_check.as_ref().filter(|f| !f.is_empty()).map(|f| [
                        utf16("Invalid flags supplied to RegExp constructor '"),
                        f.clone(),
                        utf16("'"),
                    ]
                    .concat())
        };
        if let Some(m) = message {
            self.report(cx, node, &m, false);
            return;
        }
        let Some(pattern) = args
            .first()
            .copied()
            .flatten()
            .and_then(|p| string_value(cx, p))
        else {
            return;
        };
        let message = match &flags {
            None => validate(&pattern, true, false)
                .and_then(|_| validate(&pattern, false, true))
                .and_then(|_| validate(&pattern, false, false)),
            Some(f) => validate(&pattern, f.contains(&0x75), f.contains(&0x76)),
        };
        if let Some((m, lossy)) = message {
            self.report(cx, node, &m, lossy);
        }
    }

    fn unsupported(&self) -> Option<&str> {
        self.unsupported
    }
}
