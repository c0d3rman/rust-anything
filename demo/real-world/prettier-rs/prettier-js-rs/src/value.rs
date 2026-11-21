use std::cell::RefCell;
use std::fmt;

use serde::de::{self, Deserialize, Deserializer, MapAccess, SeqAccess, Visitor};

#[derive(Clone, Debug, PartialEq, Default)]
pub enum Value {
    #[default]
    Null,
    Bool(bool),
    Number(Number),
    String(String),
    Array(Vec<Value>),
    Object(Map),
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Number {
    U(u64),
    I(i64),
    F(f64),
}

#[derive(Clone, Debug, PartialEq, Default)]
pub struct Map {
    entries: Vec<(&'static str, Value)>,
}

thread_local! {
    static KEYS: RefCell<doc_printer_rs::port::fx::FxHashMap<String, &'static str>> = RefCell::new(Default::default());
}

fn intern(key: &str) -> &'static str {
    KEYS.with(|keys| {
        let mut keys = keys.borrow_mut();
        if let Some(k) = keys.get(key) {
            return *k;
        }
        let leaked: &'static str = Box::leak(key.to_string().into_boxed_str());
        keys.insert(key.to_string(), leaked);
        leaked
    })
}

impl Map {
    pub fn new() -> Self {
        Map::default()
    }

    pub fn get(&self, key: &str) -> Option<&Value> {
        self.entries.iter().find(|(k, _)| *k == key).map(|(_, v)| v)
    }

    pub fn get_mut(&mut self, key: &str) -> Option<&mut Value> {
        self.entries
            .iter_mut()
            .find(|(k, _)| *k == key)
            .map(|(_, v)| v)
    }

    pub fn insert(&mut self, key: String, value: Value) -> Option<Value> {
        if let Some(slot) = self.get_mut(&key) {
            return Some(std::mem::replace(slot, value));
        }
        self.entries.push((intern(&key), value));
        None
    }

    pub fn remove(&mut self, key: &str) -> Option<Value> {
        let i = self.entries.iter().position(|(k, _)| *k == key)?;
        Some(self.entries.remove(i).1)
    }

    pub fn contains_key(&self, key: &str) -> bool {
        self.get(key).is_some()
    }

    pub fn iter(&self) -> impl Iterator<Item = (&'static str, &Value)> {
        self.entries.iter().map(|(k, v)| (*k, v))
    }

    pub fn iter_mut(&mut self) -> impl Iterator<Item = (&'static str, &mut Value)> {
        self.entries.iter_mut().map(|(k, v)| (*k, v))
    }

    pub fn values(&self) -> impl Iterator<Item = &Value> {
        self.entries.iter().map(|(_, v)| v)
    }
}

impl<'a> IntoIterator for &'a Map {
    type Item = (&'static str, &'a Value);
    type IntoIter = std::iter::Map<
        std::slice::Iter<'a, (&'static str, Value)>,
        fn(&'a (&'static str, Value)) -> (&'static str, &'a Value),
    >;
    fn into_iter(self) -> Self::IntoIter {
        self.entries.iter().map(|(k, v)| (*k, v))
    }
}

static NULL: Value = Value::Null;

impl std::ops::Index<&str> for Value {
    type Output = Value;
    fn index(&self, key: &str) -> &Value {
        self.get(key).unwrap_or(&NULL)
    }
}

impl From<i32> for Value {
    fn from(n: i32) -> Self {
        if n >= 0 {
            Value::Number(Number::U(n as u64))
        } else {
            Value::Number(Number::I(i64::from(n)))
        }
    }
}

pub trait Index {
    fn index_into<'v>(&self, v: &'v Value) -> Option<&'v Value>;
    fn index_into_mut<'v>(&self, v: &'v mut Value) -> Option<&'v mut Value>;
}

impl Index for str {
    fn index_into<'v>(&self, v: &'v Value) -> Option<&'v Value> {
        match v {
            Value::Object(m) => m.get(self),
            _ => None,
        }
    }
    fn index_into_mut<'v>(&self, v: &'v mut Value) -> Option<&'v mut Value> {
        match v {
            Value::Object(m) => m.get_mut(self),
            _ => None,
        }
    }
}

impl Index for usize {
    fn index_into<'v>(&self, v: &'v Value) -> Option<&'v Value> {
        match v {
            Value::Array(a) => a.get(*self),
            _ => None,
        }
    }
    fn index_into_mut<'v>(&self, v: &'v mut Value) -> Option<&'v mut Value> {
        match v {
            Value::Array(a) => a.get_mut(*self),
            _ => None,
        }
    }
}

impl<T: Index + ?Sized> Index for &T {
    fn index_into<'v>(&self, v: &'v Value) -> Option<&'v Value> {
        (**self).index_into(v)
    }
    fn index_into_mut<'v>(&self, v: &'v mut Value) -> Option<&'v mut Value> {
        (**self).index_into_mut(v)
    }
}

impl Value {
    pub fn get<I: Index>(&self, i: I) -> Option<&Value> {
        i.index_into(self)
    }

    pub fn get_mut<I: Index>(&mut self, i: I) -> Option<&mut Value> {
        i.index_into_mut(self)
    }

    pub fn take(&mut self) -> Value {
        std::mem::take(self)
    }

    pub fn is_null(&self) -> bool {
        matches!(self, Value::Null)
    }

    pub fn is_string(&self) -> bool {
        matches!(self, Value::String(_))
    }

    pub fn is_array(&self) -> bool {
        matches!(self, Value::Array(_))
    }

    pub fn is_number(&self) -> bool {
        matches!(self, Value::Number(_))
    }

    pub fn is_boolean(&self) -> bool {
        matches!(self, Value::Bool(_))
    }

    pub fn as_str(&self) -> Option<&str> {
        match self {
            Value::String(s) => Some(s),
            _ => None,
        }
    }

    pub fn as_bool(&self) -> Option<bool> {
        match self {
            Value::Bool(b) => Some(*b),
            _ => None,
        }
    }

    pub fn as_u64(&self) -> Option<u64> {
        match self {
            Value::Number(Number::U(n)) => Some(*n),
            Value::Number(Number::I(n)) => u64::try_from(*n).ok(),
            _ => None,
        }
    }

    pub fn as_f64(&self) -> Option<f64> {
        match self {
            Value::Number(Number::U(n)) => Some(*n as f64),
            Value::Number(Number::I(n)) => Some(*n as f64),
            Value::Number(Number::F(n)) => Some(*n),
            _ => None,
        }
    }

    pub fn as_array(&self) -> Option<&Vec<Value>> {
        match self {
            Value::Array(a) => Some(a),
            _ => None,
        }
    }

    pub fn as_object(&self) -> Option<&Map> {
        match self {
            Value::Object(m) => Some(m),
            _ => None,
        }
    }
}

impl From<usize> for Value {
    fn from(n: usize) -> Self {
        Value::Number(Number::U(n as u64))
    }
}

impl From<u64> for Value {
    fn from(n: u64) -> Self {
        Value::Number(Number::U(n))
    }
}

impl From<bool> for Value {
    fn from(b: bool) -> Self {
        Value::Bool(b)
    }
}

impl From<String> for Value {
    fn from(s: String) -> Self {
        Value::String(s)
    }
}

impl From<&str> for Value {
    fn from(s: &str) -> Self {
        Value::String(s.to_string())
    }
}

struct ValueVisitor;

impl<'de> Visitor<'de> for ValueVisitor {
    type Value = Value;

    fn expecting(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.write_str("any JSON value")
    }

    fn visit_bool<E>(self, v: bool) -> Result<Value, E> {
        Ok(Value::Bool(v))
    }

    fn visit_i64<E>(self, v: i64) -> Result<Value, E> {
        Ok(Value::Number(if v >= 0 {
            Number::U(v as u64)
        } else {
            Number::I(v)
        }))
    }

    fn visit_u64<E>(self, v: u64) -> Result<Value, E> {
        Ok(Value::Number(Number::U(v)))
    }

    fn visit_f64<E>(self, v: f64) -> Result<Value, E> {
        Ok(Value::Number(Number::F(v)))
    }

    fn visit_str<E: de::Error>(self, v: &str) -> Result<Value, E> {
        Ok(Value::String(v.to_string()))
    }

    fn visit_string<E>(self, v: String) -> Result<Value, E> {
        Ok(Value::String(v))
    }

    fn visit_unit<E>(self) -> Result<Value, E> {
        Ok(Value::Null)
    }

    fn visit_none<E>(self) -> Result<Value, E> {
        Ok(Value::Null)
    }

    fn visit_some<D: Deserializer<'de>>(self, d: D) -> Result<Value, D::Error> {
        Deserialize::deserialize(d)
    }

    fn visit_seq<A: SeqAccess<'de>>(self, mut seq: A) -> Result<Value, A::Error> {
        let mut out = Vec::with_capacity(seq.size_hint().unwrap_or(0));
        while let Some(v) = seq.next_element()? {
            out.push(v);
        }
        Ok(Value::Array(out))
    }

    fn visit_map<A: MapAccess<'de>>(self, mut map: A) -> Result<Value, A::Error> {
        let mut entries = Vec::with_capacity(map.size_hint().unwrap_or(8));
        while let Some(key) = map.next_key::<std::borrow::Cow<'de, str>>()? {
            let value: Value = map.next_value()?;
            entries.push((intern(&key), value));
        }
        Ok(Value::Object(Map { entries }))
    }
}

impl<'de> Deserialize<'de> for Value {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Value, D::Error> {
        d.deserialize_any(ValueVisitor)
    }
}

/// What an overflowing number becomes in the JSON text before serde_json reads it.
const INFINITY_MARK_JSON: &str = "\\u0000prettier-js-rs:Infinity";

/// Reads oxc's ESTree JSON. oxc writes an overflowing number literal's value (`1e400`) as
/// `1e+400`, which `JSON.parse` reads as `Infinity` but serde_json rejects; those tokens come
/// back as `Number::F(±INFINITY)`, the value Prettier sees.
pub fn from_estree_json(json: &str) -> Result<Value, serde_json::Error> {
    let marked = json.contains("1e+400").then(|| mark_infinities(json));
    let mut de = serde_json::Deserializer::from_str(marked.as_ref().map_or(json, |m| &m.0));
    de.disable_recursion_limit();
    let mut value: Value = Deserialize::deserialize(&mut de)?;
    if let Some((_, rewritten)) = &marked {
        let mut it = rewritten.iter().peekable();
        restore_infinities(&mut value, &mut 0, &mut it);
    }
    Ok(value)
}

/// Replaces `1e+400` and `-1e+400` number tokens (outside strings) with marker strings, and
/// returns, for each, its index among the string values of the JSON text and its sign.
fn mark_infinities(json: &str) -> (String, Vec<(usize, bool)>) {
    let b = json.as_bytes();
    let mut out = String::with_capacity(json.len());
    let mut rewritten = Vec::new();
    let (mut i, mut last, mut strings) = (0, 0, 0);
    while i < b.len() {
        match b[i] {
            b'"' => {
                i += 1;
                while i < b.len() && b[i] != b'"' {
                    i += if b[i] == b'\\' { 2 } else { 1 };
                }
                i += 1;
                let mut j = i;
                while j < b.len() && b[j].is_ascii_whitespace() {
                    j += 1;
                }
                if b.get(j) != Some(&b':') {
                    strings += 1;
                }
            }
            b'-' | b'1' if i > 0 && matches!(b[i - 1], b':' | b',' | b'[') => {
                let neg = b[i] == b'-';
                let after = i + usize::from(neg) + "1e+400".len();
                if json[i + usize::from(neg)..].starts_with("1e+400")
                    && matches!(b.get(after), Some(b',' | b'}' | b']'))
                {
                    out.push_str(&json[last..i]);
                    out.push('"');
                    out.push_str(INFINITY_MARK_JSON);
                    out.push('"');
                    rewritten.push((strings, neg));
                    strings += 1;
                    last = after;
                    i = after;
                } else {
                    i += 1;
                }
            }
            _ => i += 1,
        }
    }
    out.push_str(&json[last..]);
    (out, rewritten)
}

fn restore_infinities<'a>(
    v: &mut Value,
    strings: &mut usize,
    rewritten: &mut std::iter::Peekable<std::slice::Iter<'a, (usize, bool)>>,
) {
    match v {
        Value::String(_) => {
            if let Some(&&(at, neg)) = rewritten.peek()
                && at == *strings
            {
                rewritten.next();
                let inf = if neg {
                    f64::NEG_INFINITY
                } else {
                    f64::INFINITY
                };
                *v = Value::Number(Number::F(inf));
            }
            *strings += 1;
        }
        Value::Array(items) => items
            .iter_mut()
            .for_each(|x| restore_infinities(x, strings, rewritten)),
        Value::Object(map) => map
            .iter_mut()
            .for_each(|(_, x)| restore_infinities(x, strings, rewritten)),
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    fn format(text: &str) -> String {
        let args = ["--stdin-filepath".to_string(), "a.js".to_string()];
        let opts = crate::Options::from_cli(&args).unwrap();
        crate::format(text, &opts).unwrap()
    }

    #[test]
    fn strings_are_not_read_as_overflowing_numbers() {
        let text = "const a = [\"1e+400\", \"\\0prettier-js-rs:Infinity\"];\n";
        assert_eq!(format(text), text);
        assert_eq!(
            format(
                "const a = [1e400, \"\\0prettier-js-rs:Infinity\", -1e400, \"-\\0prettier-js-rs:Infinity\"];"
            ),
            "const a = [\n  1e400,\n  \"\\0prettier-js-rs:Infinity\",\n  -1e400,\n  \"-\\0prettier-js-rs:Infinity\",\n];\n"
        );
    }
}
