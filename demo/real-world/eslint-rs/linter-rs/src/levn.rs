//! The part of levn 0.4.1 that ESLint uses to read `--rule` values and `/* eslint */` comments:
//! `levn.parse("Object", text)` with untyped values. Anything outside the plain grammar
//! (regular expressions, dates, `undefined`, typed casts) is declined with `None`.

use serde_json::{Map, Number, Value};

struct P<'a> {
    s: &'a [char],
    i: usize,
}

fn is_ws(c: char) -> bool {
    crate::directives::is_ws(c)
}

impl P<'_> {
    fn ws(&mut self) {
        while self.i < self.s.len() && is_ws(self.s[self.i]) {
            self.i += 1;
        }
    }

    fn peek(&self) -> Option<char> {
        self.s.get(self.i).copied()
    }

    fn quoted(&mut self) -> Option<String> {
        let q = self.peek()?;
        self.i += 1;
        let mut out = String::new();
        while let Some(c) = self.peek() {
            self.i += 1;
            if c == '\\' {
                let n = self.peek()?;
                self.i += 1;
                out.push(n);
            } else if c == q {
                return Some(out);
            } else {
                out.push(c);
            }
        }
        None
    }

    fn bare(&mut self, stops: &[char]) -> String {
        let st = self.i;
        while let Some(c) = self.peek() {
            if stops.contains(&c) {
                break;
            }
            self.i += 1;
        }
        self.s[st..self.i]
            .iter()
            .collect::<String>()
            .trim_matches(is_ws)
            .to_string()
    }

    fn value(&mut self, stops: &[char]) -> Option<Value> {
        self.ws();
        match self.peek()? {
            '[' => {
                self.i += 1;
                let mut arr = Vec::new();
                loop {
                    self.ws();
                    if self.peek()? == ']' {
                        self.i += 1;
                        break;
                    }
                    arr.push(self.value(&[',', ']'])?);
                    self.ws();
                    match self.peek()? {
                        ',' => self.i += 1,
                        ']' => {
                            self.i += 1;
                            break;
                        }
                        _ => return None,
                    }
                }
                Some(Value::Array(arr))
            }
            '{' => {
                self.i += 1;
                let m = self.object(Some('}'))?;
                Some(Value::Object(m))
            }
            '"' | '\'' => self.quoted().map(Value::String),
            '/' | '#' => None,
            _ => {
                let w = self.bare(stops);
                scalar(&w)
            }
        }
    }

    fn object(&mut self, close: Option<char>) -> Option<Map<String, Value>> {
        let mut m = Map::new();
        loop {
            self.ws();
            match (self.peek(), close) {
                (None, None) => break,
                (Some(c), Some(cl)) if c == cl => {
                    self.i += 1;
                    break;
                }
                (None, Some(_)) => return None,
                _ => {}
            }
            let key = match self.peek()? {
                '"' | '\'' => self.quoted()?,
                _ => {
                    let k = self.bare(&[':', ',']);
                    if k.is_empty() {
                        return None;
                    }
                    k
                }
            };
            self.ws();
            if self.peek()? != ':' {
                return None;
            }
            self.i += 1;
            let stops: &[char] = if close.is_some() { &[',', '}'] } else { &[','] };
            let v = self.value(stops)?;
            m.insert(key, v);
            self.ws();
            match self.peek() {
                Some(',') => self.i += 1,
                Some(c) if Some(c) == close => {}
                None if close.is_none() => {}
                _ => return None,
            }
        }
        Some(m)
    }
}

fn scalar(w: &str) -> Option<Value> {
    match w {
        "" => None,
        "true" => Some(Value::Bool(true)),
        "false" => Some(Value::Bool(false)),
        "null" => Some(Value::Null),
        "undefined" | "NaN" | "Infinity" | "-Infinity" => None,
        _ => {
            let looks_numeric = w
                .chars()
                .next()
                .is_some_and(|c| c.is_ascii_digit() || c == '-' || c == '+' || c == '.');
            if looks_numeric {
                if w.chars().all(|c| c.is_ascii_digit()) && w.len() < 16 {
                    return w
                        .parse::<i64>()
                        .ok()
                        .map(|n| Value::Number(Number::from(n)));
                }
                return None;
            }
            if w.contains(['(', ')', '|', '#', '{', '}', '[', ']', ':']) {
                return None;
            }
            Some(Value::String(w.to_string()))
        }
    }
}

/// `levn.parse("Object", text)`, braces optional at the top.
pub fn parse_object(text: &str) -> Option<Vec<(String, Value)>> {
    let chars: Vec<char> = text.chars().collect();
    let mut p = P { s: &chars, i: 0 };
    p.ws();
    let m = if p.peek() == Some('{') {
        p.i += 1;
        let m = p.object(Some('}'))?;
        p.ws();
        if p.i != chars.len() {
            return None;
        }
        m
    } else {
        p.object(None)?
    };
    Some(m.into_iter().collect())
}

/// `ConfigCommentParser#parseJSONLikeConfig`, for the configs whose severities are valid.
pub fn parse_rules_config(text: &str) -> Option<Vec<(String, Value)>> {
    let items = parse_object(text)?;
    let valid = items.iter().all(|(_, v)| {
        let sev = match v {
            Value::Array(a) => a.first(),
            other => Some(other),
        };
        sev.is_some_and(|s| {
            crate::linter::severity_of(s).is_some()
                && !matches!(s, Value::String(x) if x == "0" || x == "1" || x == "2")
        })
    });
    if valid { Some(items) } else { None }
}
