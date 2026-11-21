//! Lint problems as `lib/linter/file-report.js` builds them, with their JSON key order.

use crate::json;
use crate::source::{Loc, Pos, SourceCode};
use serde_json::Value;

#[derive(Debug, Clone, PartialEq)]
pub struct Fix {
    pub range: (u32, u32),
    pub text: String,
}

#[derive(Debug, Clone)]
pub struct Suggestion {
    /// Keys of the rule's suggestion object, in the order the rule wrote them; `fix` holds the
    /// position of the fix and `desc` is appended unless the rule set it.
    pub keys: Vec<(&'static str, SugVal)>,
    pub fix: Fix,
}

#[derive(Debug, Clone)]
pub enum SugVal {
    Str(String),
    Data(Vec<(String, Value)>),
    Fix,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Shape {
    /// `createProblem`: ruleId, severity, message, line, column, messageId, endLine, endColumn, fix, suggestions.
    Rule,
    /// `createLintingProblem`: ruleId, message, line, column, endLine, endColumn, severity (, fatal).
    Linting,
    /// Unused directive: ruleId, message, line, column, severity, fix.
    Directive,
}

#[derive(Debug, Clone)]
pub struct Problem {
    pub shape: Shape,
    pub rule_id: Option<String>,
    pub severity: u8,
    pub message: String,
    pub line: u32,
    pub column: u32,
    pub message_id: Option<String>,
    pub end_line: Option<u32>,
    pub end_column: Option<u32>,
    pub fix: Option<Fix>,
    pub suggestions: Vec<Suggestion>,
    pub fatal: bool,
    pub suppressions: Option<Vec<String>>,
}

impl Problem {
    pub fn linting(rule_id: Option<String>, message: String, loc: Loc, severity: u8) -> Problem {
        Problem {
            shape: Shape::Linting,
            rule_id,
            severity,
            message,
            line: loc.start.line,
            column: loc.start.column + 1,
            message_id: None,
            end_line: Some(loc.end.line),
            end_column: Some(loc.end.column + 1),
            fix: None,
            suggestions: Vec::new(),
            fatal: false,
            suppressions: None,
        }
    }

    pub fn write_json(&self, out: &mut String) {
        out.push_str("{\"ruleId\":");
        match &self.rule_id {
            Some(r) => json::quote(r, out),
            None => out.push_str("null"),
        }
        let sev = |out: &mut String| {
            out.push_str(",\"severity\":");
            out.push_str(&self.severity.to_string());
        };
        if self.shape == Shape::Rule {
            sev(out);
        }
        out.push_str(",\"message\":");
        json::quote(&self.message, out);
        out.push_str(&format!(
            ",\"line\":{},\"column\":{}",
            self.line, self.column
        ));
        if let Some(m) = &self.message_id {
            out.push_str(",\"messageId\":");
            json::quote(m, out);
        }
        if let (Some(l), Some(c)) = (self.end_line, self.end_column) {
            out.push_str(&format!(",\"endLine\":{l},\"endColumn\":{c}"));
        }
        if self.shape != Shape::Rule {
            sev(out);
        }
        if self.fatal {
            out.push_str(",\"fatal\":true");
        }
        if let Some(f) = &self.fix {
            out.push_str(",\"fix\":");
            write_fix(f, out);
        }
        if !self.suggestions.is_empty() {
            out.push_str(",\"suggestions\":[");
            for (i, s) in self.suggestions.iter().enumerate() {
                if i > 0 {
                    out.push(',');
                }
                out.push('{');
                for (j, (k, v)) in s.keys.iter().enumerate() {
                    if j > 0 {
                        out.push(',');
                    }
                    json::quote(k, out);
                    out.push(':');
                    match v {
                        SugVal::Str(t) => json::quote(t, out),
                        SugVal::Fix => write_fix(&s.fix, out),
                        SugVal::Data(d) => {
                            out.push('{');
                            for (n, (dk, dv)) in d.iter().enumerate() {
                                if n > 0 {
                                    out.push(',');
                                }
                                json::quote(dk, out);
                                out.push(':');
                                json::value(dv, out);
                            }
                            out.push('}');
                        }
                    }
                }
                out.push('}');
            }
            out.push(']');
        }
        if let Some(sup) = &self.suppressions {
            out.push_str(",\"suppressions\":[");
            for (i, j) in sup.iter().enumerate() {
                if i > 0 {
                    out.push(',');
                }
                out.push_str("{\"kind\":\"directive\",\"justification\":");
                json::quote(j, out);
                out.push('}');
            }
            out.push(']');
        }
        out.push('}');
    }
}

fn write_fix(f: &Fix, out: &mut String) {
    out.push_str(&format!(
        "{{\"range\":[{},{}],\"text\":",
        f.range.0, f.range.1
    ));
    json::quote(&f.text, out);
    out.push('}');
}

/// `interpolate(text, data)` from lib/linter/interpolate.js.
pub fn interpolate(text: &str, data: &[(String, Value)]) -> String {
    if data.is_empty() || !text.contains("{{") {
        return text.to_string();
    }
    let mut out = String::new();
    let mut rest = text;
    while let Some(i) = rest.find("{{") {
        let after = &rest[i + 2..];
        // /\{\{([^{}]+)\}\}/: the shortest run without braces, closed by "}}".
        let close = after.find(['{', '}']);
        match close {
            Some(j) if j > 0 && after[j..].starts_with("}}") => {
                let term = after[..j].trim();
                out.push_str(&rest[..i]);
                match data.iter().find(|(k, _)| k == term) {
                    Some((_, v)) => out.push_str(&json::js_string(v)),
                    None => out.push_str(&rest[i..i + 2 + j + 2]),
                }
                rest = &after[j + 2..];
            }
            _ => {
                out.push_str(&rest[..i + 1]);
                rest = &rest[i + 1..];
            }
        }
    }
    out.push_str(rest);
    out
}

/// `mergeFixes` from file-report.js.
pub fn merge_fixes(mut fixes: Vec<Fix>, sc: &SourceCode) -> Option<Fix> {
    if fixes.is_empty() {
        return None;
    }
    if fixes.len() == 1 {
        return fixes.pop();
    }
    fixes.sort_by(|a, b| a.range.0.cmp(&b.range.0).then(a.range.1.cmp(&b.range.1)));
    let start = fixes[0].range.0;
    let end = fixes[fixes.len() - 1].range.1;
    let mut text = String::new();
    let mut last: i64 = i64::MIN;
    for f in &fixes {
        if (f.range.0 as i64) < last {
            return None;
        }
        let from = (start as i64).max(last).max(0) as u32;
        if f.range.0 >= from {
            text.push_str(sc.slice16(from, f.range.0));
        }
        text.push_str(&f.text);
        last = f.range.1 as i64;
    }
    let from = (start as i64).max(last).max(0) as u32;
    if end >= from {
        text.push_str(sc.slice16(from, end));
    }
    Some(Fix {
        range: (start, end),
        text,
    })
}

pub fn pos1(p: Pos) -> (u32, u32) {
    (p.line, p.column + 1)
}
