//! lib/rules/no-irregular-whitespace.js

use crate::ast::NodeId;
use crate::linter::Cx;
use crate::rule::Report;
use crate::rules::{Rule, RuleDef};
use crate::source::{Loc, Pos};
use crate::utils::{literal_typeof, template_element_raw};
use serde_json::Value;

pub static DEF: RuleDef = RuleDef {
    name: "no-irregular-whitespace",
    messages: &[("noIrregularWhitespace", "Irregular whitespace not allowed.")],
    has_default_options: true,
    create,
};

struct R {
    skip_comments: bool,
    skip_strings: bool,
    skip_regexps: bool,
    skip_templates: bool,
    skip_jsx_text: bool,
    active: bool,
    errors: Vec<(NodeId, Loc)>,
}

fn create(options: &[Value]) -> Result<Box<dyn Rule>, String> {
    let mut r = R {
        skip_comments: false,
        skip_strings: true,
        skip_regexps: false,
        skip_templates: false,
        skip_jsx_text: false,
        active: false,
        errors: Vec::new(),
    };
    match options {
        [] => {}
        [Value::Object(o)] => {
            for (k, v) in o {
                let Value::Bool(b) = v else {
                    return Err("invalid option".into());
                };
                match k.as_str() {
                    "skipComments" => r.skip_comments = *b,
                    "skipStrings" => r.skip_strings = *b,
                    "skipTemplates" => r.skip_templates = *b,
                    "skipRegExps" => r.skip_regexps = *b,
                    "skipJSXText" => r.skip_jsx_text = *b,
                    _ => return Err("invalid option".into()),
                }
            }
        }
        _ => return Err("invalid option".into()),
    }
    Ok(Box::new(r))
}

fn is_irregular_whitespace(c: char) -> bool {
    matches!(
        c,
        '\u{c}' | '\u{b}' | '\u{85}' | '\u{feff}' | '\u{a0}' | '\u{1680}' | '\u{180e}' | '\u{2000}'
            ..='\u{200b}' | '\u{202f}' | '\u{205f}' | '\u{3000}'
    )
}

fn has_any_irregular(s: &str) -> bool {
    s.chars()
        .any(|c| is_irregular_whitespace(c) || c == '\u{2028}' || c == '\u{2029}')
}

impl R {
    fn remove_errors(&mut self, loc: Loc) {
        let (s, e) = (loc.start, loc.end);
        self.errors.retain(|(_, l)| {
            let p = l.start;
            p.line < s.line
                || (p.line == s.line && p.column < s.column)
                || (p.line == e.line && p.column >= e.column)
                || p.line > e.line
        });
    }

    fn check_whitespace(&mut self, cx: &Cx, node: NodeId) {
        for (i, line) in cx.sc.lines.iter().enumerate() {
            let line_no = i as u32 + 1;
            let mut col = 0u32;
            let mut run: Option<(u32, u32)> = None;
            for c in line.chars() {
                if is_irregular_whitespace(c) {
                    let start = run.map_or(col, |r| r.0);
                    run = Some((start, col + 1));
                } else if let Some((a, b)) = run.take() {
                    self.push(node, line_no, a, line_no, b);
                }
                col += c.len_utf16() as u32;
            }
            if let Some((a, b)) = run {
                self.push(node, line_no, a, line_no, b);
            }
        }
    }

    fn check_line_terminators(&mut self, cx: &Cx, node: NodeId) {
        let text = &cx.sc.text;
        let mut index = 0usize;
        let mut chars = text.chars().peekable();
        while let Some(c) = chars.next() {
            match c {
                '\r' => {
                    if chars.peek() == Some(&'\n') {
                        chars.next();
                    }
                    index += 1;
                }
                '\n' => index += 1,
                '\u{2028}' | '\u{2029}' => {
                    let len = cx
                        .sc
                        .lines
                        .get(index)
                        .map_or(0, |l| l.encode_utf16().count() as u32);
                    let line = index as u32 + 1;
                    self.push(node, line, len, line + 1, 0);
                    index += 1;
                }
                _ => {}
            }
        }
    }

    fn push(&mut self, node: NodeId, l1: u32, c1: u32, l2: u32, c2: u32) {
        let loc = Loc {
            start: Pos {
                line: l1,
                column: c1,
            },
            end: Pos {
                line: l2,
                column: c2,
            },
        };
        self.errors.push((node, loc));
    }
}

impl Rule for R {
    fn listeners(&self) -> &'static [&'static str] {
        &[
            "Program",
            "Literal",
            "TemplateElement",
            "JSXText",
            "Program:exit",
        ]
    }

    fn on(&mut self, l: usize, node: NodeId, cx: &mut Cx) {
        let sc = cx.sc;
        let ast = &sc.ast;
        if l == 0 {
            self.active = has_any_irregular(&sc.text);
            if self.active {
                self.check_whitespace(cx, node);
                self.check_line_terminators(cx, node);
            }
            return;
        }
        if !self.active {
            return;
        }
        match l {
            1 => {
                let strings = self.skip_strings && literal_typeof(ast, node) == "string";
                let regexps = self.skip_regexps && ast.prop(node, "regex").is_some();
                if (strings || regexps) && has_any_irregular(ast.str(node, "raw").unwrap_or("")) {
                    self.remove_errors(sc.node_loc(node));
                }
            }
            2 => {
                if self.skip_templates
                    && template_element_raw(ast, node).is_some_and(has_any_irregular)
                {
                    self.remove_errors(sc.node_loc(node));
                }
            }
            3 => {
                if self.skip_jsx_text && has_any_irregular(ast.str(node, "raw").unwrap_or("")) {
                    self.remove_errors(sc.node_loc(node));
                }
            }
            _ => {
                if self.skip_comments {
                    for c in &sc.comments {
                        if has_any_irregular(&c.value) {
                            self.remove_errors(sc.tok_loc(c));
                        }
                    }
                }
                for (node, loc) in std::mem::take(&mut self.errors) {
                    let mut r = Report::loc(loc, "noIrregularWhitespace");
                    r.node = Some(node);
                    cx.report(r);
                }
            }
        }
    }
}
