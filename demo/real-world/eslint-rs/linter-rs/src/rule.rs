//! The rule API: listeners keyed by esquery selectors (ordered as
//! `source-code-traverser.js` orders them), `context.report()` and the fixer.

use crate::ast::NodeId;
use crate::report::{Fix, SugVal};
use crate::source::{Loc, SourceCode, Token};
use serde_json::Value;

/// One listener of a rule: its raw selector, as the rule's `create()` returns it.
pub struct Listener {
    pub source: &'static str,
}

#[derive(Clone)]
pub struct Sel {
    pub source: &'static str,
    pub exit: bool,
    pub types: Option<Vec<&'static str>>,
    pub attrs: u32,
    pub idents: u32,
}

impl Sel {
    pub fn parse(raw: &'static str) -> Sel {
        thread_local! {
            static CACHE: std::cell::RefCell<std::collections::HashMap<&'static str, Sel, crate::ast::FnvBuild>> =
                std::cell::RefCell::new(std::collections::HashMap::default());
        }
        if let Some(s) = CACHE.with(|c| c.borrow().get(raw).cloned()) {
            return s;
        }
        let s = Sel::parse_uncached(raw);
        CACHE.with(|c| c.borrow_mut().insert(raw, s.clone()));
        s
    }

    fn parse_uncached(raw: &'static str) -> Sel {
        let (body, exit) = match raw.strip_suffix(":exit") {
            Some(b) => (b, true),
            None => (raw, false),
        };
        let mut p = SelParser {
            s: body.as_bytes(),
            i: 0,
            attrs: 0,
            idents: 0,
        };
        let types = p.selectors();
        Sel {
            source: raw,
            exit,
            types,
            attrs: p.attrs,
            idents: p.idents,
        }
    }

    pub fn specificity_cmp(&self, other: &Sel) -> std::cmp::Ordering {
        self.attrs
            .cmp(&other.attrs)
            .then(self.idents.cmp(&other.idents))
            .then_with(|| {
                if self.source <= other.source {
                    std::cmp::Ordering::Less
                } else {
                    std::cmp::Ordering::Greater
                }
            })
    }
}

struct SelParser<'a> {
    s: &'a [u8],
    i: usize,
    attrs: u32,
    idents: u32,
}

impl SelParser<'_> {
    fn ws(&mut self) {
        while self.i < self.s.len() && self.s[self.i] == b' ' {
            self.i += 1;
        }
    }

    // selectors: complex ("," complex)*  -> matches
    fn selectors(&mut self) -> Option<Vec<&'static str>> {
        let mut all: Vec<Option<Vec<&'static str>>> = vec![self.complex()];
        loop {
            self.ws();
            if self.i < self.s.len() && self.s[self.i] == b',' {
                self.i += 1;
                all.push(self.complex());
            } else {
                break;
            }
        }
        if all.len() == 1 {
            return all.pop().flatten();
        }
        if all.iter().all(Option::is_some) {
            let mut u: Vec<&'static str> = Vec::new();
            for t in all.into_iter().flatten().flatten() {
                if !u.contains(&t) {
                    u.push(t);
                }
            }
            Some(u)
        } else {
            None
        }
    }

    // complex: compound (combinator compound)* -> types of the rightmost
    fn complex(&mut self) -> Option<Vec<&'static str>> {
        self.ws();
        let mut last = self.compound();
        loop {
            let save = self.i;
            self.ws();
            if self.i >= self.s.len() || matches!(self.s[self.i], b',' | b')') {
                self.i = save;
                break;
            }
            if matches!(self.s[self.i], b'>' | b'~' | b'+') {
                self.i += 1;
                self.ws();
            }
            last = self.compound();
        }
        last
    }

    fn compound(&mut self) -> Option<Vec<&'static str>> {
        let mut parts: Vec<Option<Vec<&'static str>>> = Vec::new();
        while self.i < self.s.len() {
            let c = self.s[self.i];
            if c.is_ascii_alphabetic() || c == b'_' || c == b'$' {
                let st = self.i;
                while self.i < self.s.len()
                    && (self.s[self.i].is_ascii_alphanumeric()
                        || matches!(self.s[self.i], b'_' | b'$' | b'-'))
                {
                    self.i += 1;
                }
                self.idents += 1;
                let w: &'static str = Box::leak(
                    String::from_utf8_lossy(&self.s[st..self.i])
                        .into_owned()
                        .into_boxed_str(),
                );
                parts.push(Some(vec![w]));
            } else if c == b'*' {
                self.i += 1;
                parts.push(None);
            } else if c == b'[' {
                let mut depth = 0;
                let mut quote = 0u8;
                while self.i < self.s.len() {
                    let ch = self.s[self.i];
                    self.i += 1;
                    if quote != 0 {
                        if ch == b'\\' {
                            self.i += 1;
                        } else if ch == quote {
                            quote = 0;
                        }
                        continue;
                    }
                    match ch {
                        b'"' | b'\'' => quote = ch,
                        b'/' if depth > 0 => {
                            while self.i < self.s.len() && self.s[self.i] != b'/' {
                                if self.s[self.i] == b'\\' {
                                    self.i += 1;
                                }
                                self.i += 1;
                            }
                            self.i += 1;
                        }
                        b'[' => depth += 1,
                        b']' => {
                            depth -= 1;
                            if depth == 0 {
                                break;
                            }
                        }
                        _ => {}
                    }
                }
                self.attrs += 1;
                parts.push(None);
            } else if c == b'.' {
                self.i += 1;
                while self.i < self.s.len()
                    && (self.s[self.i].is_ascii_alphanumeric()
                        || matches!(self.s[self.i], b'_' | b'.'))
                {
                    self.i += 1;
                }
                self.attrs += 1;
                parts.push(None);
            } else if c == b':' {
                self.i += 1;
                let st = self.i;
                while self.i < self.s.len()
                    && (self.s[self.i].is_ascii_alphanumeric() || self.s[self.i] == b'-')
                {
                    self.i += 1;
                }
                let name = String::from_utf8_lossy(&self.s[st..self.i]).into_owned();
                if self.i < self.s.len() && self.s[self.i] == b'(' {
                    self.i += 1;
                    match name.as_str() {
                        "matches" | "is" => {
                            let t = self.selectors();
                            parts.push(t);
                        }
                        "not" => {
                            self.selectors();
                            parts.push(None);
                        }
                        "has" => {
                            let a = self.attrs;
                            let b = self.idents;
                            self.selectors();
                            self.attrs = a;
                            self.idents = b;
                            parts.push(None);
                        }
                        _ => {
                            while self.i < self.s.len() && self.s[self.i] != b')' {
                                self.i += 1;
                            }
                            self.attrs += 1;
                            parts.push(None);
                        }
                    }
                    self.ws();
                    if self.i < self.s.len() && self.s[self.i] == b')' {
                        self.i += 1;
                    }
                } else if name == "function" {
                    parts.push(Some(vec![
                        "FunctionDeclaration",
                        "FunctionExpression",
                        "ArrowFunctionExpression",
                    ]));
                } else {
                    parts.push(None);
                }
            } else {
                break;
            }
        }
        let typed: Vec<Vec<&'static str>> = parts.into_iter().flatten().collect();
        if typed.is_empty() {
            return None;
        }
        let mut res = typed[0].clone();
        for t in &typed[1..] {
            res.retain(|x| t.contains(x));
        }
        Some(res)
    }
}

pub type Data = Vec<(String, Value)>;

pub struct SuggestSpec {
    pub keys: Vec<(&'static str, SugVal)>,
    pub fix: Option<Fix>,
}

pub struct Report {
    pub node: Option<NodeId>,
    pub loc: Option<Loc>,
    pub loc_start_only: bool,
    pub message_id: Option<&'static str>,
    pub message: Option<String>,
    pub data: Data,
    pub fix: Option<Vec<Fix>>,
    pub suggest: Vec<SuggestSpec>,
}

impl Report {
    pub fn node(node: NodeId, message_id: &'static str) -> Report {
        Report {
            node: Some(node),
            loc: None,
            loc_start_only: false,
            message_id: Some(message_id),
            message: None,
            data: Vec::new(),
            fix: None,
            suggest: Vec::new(),
        }
    }

    pub fn loc(loc: Loc, message_id: &'static str) -> Report {
        Report {
            node: None,
            loc: Some(loc),
            loc_start_only: false,
            message_id: Some(message_id),
            message: None,
            data: Vec::new(),
            fix: None,
            suggest: Vec::new(),
        }
    }

    pub fn data(mut self, k: &str, v: impl Into<Value>) -> Report {
        self.data.push((k.to_string(), v.into()));
        self
    }

    pub fn fix(mut self, f: Fix) -> Report {
        self.fix = Some(vec![f]);
        self
    }

    pub fn fixes(mut self, f: Vec<Fix>) -> Report {
        self.fix = Some(f);
        self
    }
}

/// `RuleFixer` (lib/linter/rule-fixer.js).
pub mod fixer {
    use super::*;

    pub fn replace_range(range: (u32, u32), text: impl Into<String>) -> Fix {
        Fix {
            range,
            text: text.into(),
        }
    }

    pub fn replace(sc: &SourceCode, node: NodeId, text: impl Into<String>) -> Fix {
        replace_range(sc.ast.range(node), text)
    }

    pub fn replace_tok(t: &Token, text: impl Into<String>) -> Fix {
        replace_range((t.start, t.end), text)
    }

    pub fn insert_before_range(range: (u32, u32), text: impl Into<String>) -> Fix {
        replace_range((range.0, range.0), text)
    }

    pub fn insert_after_range(range: (u32, u32), text: impl Into<String>) -> Fix {
        replace_range((range.1, range.1), text)
    }

    pub fn remove_range(range: (u32, u32)) -> Fix {
        replace_range(range, "")
    }

    pub fn remove(sc: &SourceCode, node: NodeId) -> Fix {
        remove_range(sc.ast.range(node))
    }
}
