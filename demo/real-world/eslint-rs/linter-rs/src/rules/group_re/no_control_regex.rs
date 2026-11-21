//! lib/rules/no-control-regex.js

use super::util::{regex_of, string_value, utf16};
use crate::ast::NodeId;
use crate::linter::Cx;
use crate::regexpp::validator::{Handler, Options, Validator};
use crate::rule::Report;
use crate::rules::{Rule, RuleDef};
use serde_json::Value;

pub static DEF: RuleDef = RuleDef {
    name: "no-control-regex",
    messages: &[(
        "unexpected",
        "Unexpected control character(s) in regular expression: {{controlChars}}.",
    )],
    has_default_options: false,
    create,
};

struct R;

fn create(options: &[Value]) -> Result<Box<dyn Rule>, String> {
    super::no_options(options)?;
    Ok(Box::new(R))
}

struct Collector {
    source: Vec<u16>,
    control_chars: Vec<String>,
}

impl Handler for Collector {
    fn on_pattern_enter(&mut self, _start: usize) {
        self.control_chars.clear();
    }

    fn on_character(&mut self, start: usize, end: usize, value: f64) {
        if !(0.0..=31.0).contains(&value) {
            return;
        }
        let cp = value as u32;
        let s = &self.source;
        let slice = &s[start.min(s.len())..end.min(s.len())];
        let at = s.get(start).map(|c| u32::from(*c));
        if at == Some(cp) || slice.starts_with(&[0x5c, 0x78]) || slice.starts_with(&[0x5c, 0x75]) {
            self.control_chars.push(format!("\\x{cp:02x}"));
        }
    }
}

fn collect_control_chars(pattern: &[u16], flags: Option<&[u16]>) -> Vec<String> {
    let has = |c: u8| flags.is_some_and(|f| f.contains(&u16::from(c)));
    let collector = Collector {
        source: pattern.to_vec(),
        control_chars: Vec::new(),
    };
    let mut v = Validator::new(Options::default(), collector);
    let _ = v.validate_pattern(pattern, 0, pattern.len(), Some((has(b'u'), has(b'v'))));
    v.h.control_chars
}

impl Rule for R {
    fn listeners(&self) -> &'static [&'static str] {
        &["Literal"]
    }

    fn on(&mut self, _: usize, node: NodeId, cx: &mut Cx) {
        let ast = &cx.sc.ast;
        let (pattern, flags) = if let Some((p, f)) = regex_of(cx, node) {
            (p, Some(utf16(&f)))
        } else {
            let Some(value) = string_value(cx, node) else {
                return;
            };
            let Some(parent) = ast.parent(node) else {
                return;
            };
            if !matches!(ast.kind(parent), "NewExpression" | "CallExpression") {
                return;
            }
            let Some(callee) = ast.child(parent, "callee") else {
                return;
            };
            if !ast.is(callee, "Identifier") || ast.name(callee) != Some("RegExp") {
                return;
            }
            if !cx.sm().is_global_reference(ast, callee) {
                return;
            }
            let args = ast.list(parent, "arguments");
            if args.first().copied().flatten() != Some(node) {
                return;
            }
            let flags = args
                .get(1)
                .copied()
                .flatten()
                .and_then(|f| string_value(cx, f));
            (value, flags)
        };
        let chars = collect_control_chars(&pattern, flags.as_deref());
        if !chars.is_empty() {
            cx.report(Report::node(node, "unexpected").data("controlChars", chars.join(", ")));
        }
    }
}
