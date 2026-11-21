//! Bounded-exhaustive enumeration of native `Doc` values: the representation the port's printers
//! actually build and hand to `propagate_breaks` and `print`, not docs read through
//! `from_value`. Each doc is printed natively here and written in Prettier's doc JSON (`to_value`)
//! for the JS side, which rebuilds it with Prettier's object identity and prints it with
//! `printDocToString`.
//!
//! Docs are unranked by index (`nth`), so a shard is a range of indices and nothing is held in
//! memory beyond the per-size counts.

use std::cell::Cell;
use std::collections::HashMap;
use std::rc::Rc;

use doc_printer_rs::port::{self, Align, Doc, PrintOptions};
use serde_json::{Map, Value, json};

type Leaf = fn() -> Doc;
type Unary = fn(Doc) -> Doc;
type Binary = fn(Doc, Doc) -> Doc;
type List = (usize, fn(Vec<Doc>) -> Doc);

pub struct Alphabet {
    leaves: Vec<Leaf>,
    unary: Vec<Unary>,
    binary: Vec<Binary>,
    lists: Vec<List>,
}

fn s(t: &'static str) -> Doc {
    Doc::Str(t.into())
}
fn line(hard: bool, soft: bool, literal: bool) -> Doc {
    Doc::Line {
        hard,
        soft,
        literal,
    }
}
fn group(c: Doc, brk: bool, id: Option<&str>) -> Doc {
    Doc::Group {
        contents: Box::new(c),
        brk: Cell::new(brk),
        expanded: None,
        id: id.map(String::from),
    }
}
/// `conditionalGroup(states, {shouldBreak})`, as `prettier_js_rs::doc::cond_group` builds it.
fn cond(states: Vec<Doc>, brk: bool, id: Option<&str>) -> Doc {
    let (contents, states) = port::conditional_group_parts(states);
    Doc::Group {
        contents: Box::new(contents),
        brk: Cell::new(brk),
        expanded: Some(states),
        id: id.map(String::from),
    }
}
fn align(n: Align, c: Doc) -> Doc {
    Doc::Align(n, Box::new(c))
}
fn iib(c: Doc, id: Option<&str>, negate: bool) -> Doc {
    Doc::IndentIfBreak {
        contents: Box::new(c),
        group_id: id.map(String::from),
        negate,
    }
}
fn if_break(a: Doc, b: Doc, id: Option<&str>) -> Doc {
    Doc::IfBreak {
        brk: Box::new(a),
        flat: Box::new(b),
        group_id: id.map(String::from),
    }
}

/// The `full` alphabet of `grammar.mjs`, built natively, plus the native shapes the JSON
/// alphabet cannot express or never produced: `Align::Text("")` / `Text(" ")` (what the YAML and
/// Markdown printers build for a zero / one column string align), `Align::Width(0)`, a `Shared`
/// node, one `Shared` node reached twice, and a group with expanded states whose `contents` is
/// not `states[0]` (what `strip_trailing_hardline` leaves).
pub fn native() -> Alphabet {
    Alphabet {
        leaves: vec![
            || s(""),
            || s("a"),
            || s(" "),
            || s("\t"),
            || s("字"),
            || line(false, false, false),
            || line(false, true, false),
            || line(true, false, false),
            || line(true, false, true),
            || Doc::Array(vec![line(true, false, false), Doc::BreakParent]),
            || Doc::Array(vec![line(true, false, true), Doc::BreakParent]),
            || Doc::BreakParent,
            || Doc::Trim,
            || Doc::Cursor,
            || Doc::LineSuffixBoundary,
        ],
        unary: vec![
            |c| Doc::Indent(Box::new(c)),
            |c| align(Align::Width(2.0), c),
            |c| align(Align::Width(-1.0), c),
            |c| align(Align::Text("> ".into()), c),
            |c| align(Align::DedentToRoot, c),
            |c| align(Align::Root, c),
            |c| align(Align::Text(String::new()), c),
            |c| align(Align::Text(" ".into()), c),
            |c| align(Align::Width(0.0), c),
            |c| group(c, false, None),
            |c| group(c, true, None),
            |c| group(c, false, Some("A")),
            |c| group(c, false, Some("B")),
            |c| group(c, true, Some("A")),
            |c| group(c, true, Some("B")),
            |c| Doc::LineSuffix(Box::new(c)),
            |c| Doc::Label("L".into(), Box::new(c)),
            |c| iib(c, Some("A"), false),
            |c| iib(c, Some("A"), true),
            |c| iib(c, Some("B"), false),
            |c| iib(c, Some("B"), true),
            |c| iib(c, None, false),
            |c| iib(c, None, true),
            |c| Doc::Shared(Rc::new(c)),
            |c| {
                let shared = Doc::Shared(Rc::new(c));
                Doc::Array(vec![shared.clone(), shared])
            },
        ],
        binary: vec![
            |a, b| if_break(a, b, None),
            |a, b| if_break(a, b, Some("A")),
            |a, b| if_break(a, b, Some("B")),
            |a, b| Doc::Group {
                contents: Box::new(a),
                brk: Cell::new(false),
                expanded: Some(vec![b]),
                id: None,
            },
        ],
        lists: vec![
            (2, Doc::Array),
            (0, Doc::Fill),
            (1, |xs| cond(xs, false, None)),
            (1, |xs| cond(xs, true, None)),
            (1, |xs| cond(xs, false, Some("A"))),
        ],
    }
}

pub struct Enumerator {
    alpha: Alphabet,
    counts: Vec<u128>,
    seq: HashMap<(usize, usize), u128>,
}

impl Enumerator {
    pub fn new(alpha: Alphabet) -> Self {
        Enumerator {
            alpha,
            counts: vec![0],
            seq: HashMap::new(),
        }
    }

    pub fn count(&mut self, n: usize) -> u128 {
        while self.counts.len() <= n {
            let k = self.counts.len();
            let c = self.compute(k);
            self.counts.push(c);
        }
        self.counts[n]
    }

    fn compute(&mut self, n: usize) -> u128 {
        if n == 1 {
            return (self.alpha.leaves.len() + self.alpha.lists.iter().filter(|l| l.0 == 0).count())
                as u128;
        }
        let mut total = self.alpha.unary.len() as u128 * self.count(n - 1);
        for k in 1..=n.saturating_sub(2) {
            total += self.alpha.binary.len() as u128 * self.count(k) * self.count(n - 1 - k);
        }
        let mins: Vec<usize> = self.alpha.lists.iter().map(|l| l.0.max(1)).collect();
        for m in mins {
            total += self.seqs(n - 1, m);
        }
        total
    }

    /// Ordered sequences of docs whose sizes sum to `total`, at least `min` long.
    fn seqs(&mut self, total: usize, min: usize) -> u128 {
        if total == 0 {
            return u128::from(min == 0);
        }
        if let Some(&c) = self.seq.get(&(total, min)) {
            return c;
        }
        let mut c = 0;
        for first in 1..=total {
            c += self.count(first) * self.seqs(total - first, min.saturating_sub(1));
        }
        self.seq.insert((total, min), c);
        c
    }

    pub fn nth(&mut self, n: usize, mut i: u128) -> Doc {
        if n == 1 {
            let leaves = self.alpha.leaves.len() as u128;
            if i < leaves {
                return (self.alpha.leaves[i as usize])();
            }
            i -= leaves;
            for l in &self.alpha.lists {
                if l.0 == 0 {
                    if i == 0 {
                        return (l.1)(Vec::new());
                    }
                    i -= 1;
                }
            }
            unreachable!("index past size 1");
        }
        let below = self.count(n - 1);
        for u in 0..self.alpha.unary.len() {
            if i < below {
                let c = self.nth(n - 1, i);
                return (self.alpha.unary[u])(c);
            }
            i -= below;
        }
        for k in 1..=n.saturating_sub(2) {
            let (ck, cr) = (self.count(k), self.count(n - 1 - k));
            for b in 0..self.alpha.binary.len() {
                if i < ck * cr {
                    let x = self.nth(k, i / cr);
                    let y = self.nth(n - 1 - k, i % cr);
                    return (self.alpha.binary[b])(x, y);
                }
                i -= ck * cr;
            }
        }
        for l in 0..self.alpha.lists.len() {
            let (min, make) = self.alpha.lists[l];
            let c = self.seqs(n - 1, min.max(1));
            if i < c {
                let xs = self.nth_seq(n - 1, min.max(1), i);
                return make(xs);
            }
            i -= c;
        }
        unreachable!("index past size {n}");
    }

    fn nth_seq(&mut self, total: usize, min: usize, mut i: u128) -> Vec<Doc> {
        if total == 0 {
            return Vec::new();
        }
        for first in 1..=total {
            let rest = self.seqs(total - first, min.saturating_sub(1));
            let block = self.count(first) * rest;
            if i < block {
                let mut out = vec![self.nth(first, i / rest)];
                out.extend(self.nth_seq(total - first, min.saturating_sub(1), i % rest));
                return out;
            }
            i -= block;
        }
        unreachable!("sequence index out of range");
    }
}

/// The first group id in preorder (a node's own id before its children's).
pub fn first_id(d: &Doc) -> Option<&str> {
    match d {
        Doc::Group {
            contents,
            expanded,
            id,
            ..
        } => id.as_deref().or_else(|| match expanded {
            Some(states) => states.iter().find_map(first_id),
            None => first_id(contents),
        }),
        Doc::IfBreak {
            brk,
            flat,
            group_id,
        } => group_id
            .as_deref()
            .or_else(|| first_id(brk))
            .or_else(|| first_id(flat)),
        Doc::IndentIfBreak {
            contents, group_id, ..
        } => group_id.as_deref().or_else(|| first_id(contents)),
        Doc::Array(parts) | Doc::Fill(parts) => parts.iter().find_map(first_id),
        Doc::Indent(c) | Doc::Align(_, c) | Doc::LineSuffix(c) | Doc::Label(_, c) => first_id(c),
        Doc::Shared(inner) => first_id(inner),
        _ => None,
    }
}

/// Does printing depend on `useTabs` / `tabWidth` (an indent, align or indent-if-break)?
pub fn has_indentation(d: &Doc) -> bool {
    match d {
        Doc::Indent(_) | Doc::Align(..) | Doc::IndentIfBreak { .. } => true,
        Doc::Group {
            contents, expanded, ..
        } => {
            has_indentation(contents)
                || expanded
                    .as_ref()
                    .is_some_and(|s| s.iter().any(has_indentation))
        }
        Doc::IfBreak { brk, flat, .. } => has_indentation(brk) || has_indentation(flat),
        Doc::Array(parts) | Doc::Fill(parts) => parts.iter().any(has_indentation),
        Doc::LineSuffix(c) | Doc::Label(_, c) => has_indentation(c),
        Doc::Shared(inner) => has_indentation(inner),
        _ => false,
    }
}

fn num(f: f64) -> Value {
    if f.is_finite() {
        serde_json::Number::from_f64(f).map_or(Value::Null, Value::Number)
    } else if f.is_nan() {
        json!({"$num": "NaN"})
    } else if f > 0.0 {
        json!({"$num": "Infinity"})
    } else {
        json!({"$num": "-Infinity"})
    }
}

/// Prettier's doc for a native `Doc`, as its builders make it. A `Shared` node is written once as
/// `{"$def": k, "doc": ...}` and then as `{"$ref": k}`, so the JS side can give every reach of it
/// the same object (as one JS value used twice would be).
pub fn to_value(d: &Doc) -> Value {
    fn obj(ty: &str, fields: Vec<(&str, Value)>) -> Value {
        let mut m = Map::new();
        m.insert("type".into(), ty.into());
        for (k, v) in fields {
            m.insert(k.into(), v);
        }
        Value::Object(m)
    }
    fn rec(d: &Doc, defs: &mut HashMap<*const Doc, usize>) -> Value {
        match d {
            Doc::Str(t) => Value::String(t.to_string()),
            Doc::Array(items) => Value::Array(items.iter().map(|i| rec(i, defs)).collect()),
            Doc::Cursor => obj("cursor", vec![]),
            Doc::Indent(c) => obj("indent", vec![("contents", rec(c, defs))]),
            Doc::Align(n, c) => {
                let n = match n {
                    Align::Width(f) => num(*f),
                    Align::Text(t) => Value::String(t.clone()),
                    Align::DedentToRoot => num(f64::NEG_INFINITY),
                    Align::Root => json!({"type": "root"}),
                };
                obj("align", vec![("n", n), ("contents", rec(c, defs))])
            }
            Doc::Trim => obj("trim", vec![]),
            Doc::Group {
                contents,
                brk,
                expanded,
                id,
            } => {
                let mut f = Vec::new();
                if let Some(id) = id {
                    f.push(("id", Value::String(id.clone())));
                }
                f.push(("contents", rec(contents, defs)));
                f.push(("break", Value::Bool(brk.get())));
                if let Some(states) = expanded {
                    let st = states.iter().map(|s| rec(s, defs)).collect();
                    f.push(("expandedStates", Value::Array(st)));
                }
                obj("group", f)
            }
            Doc::Fill(parts) => obj(
                "fill",
                vec![(
                    "parts",
                    Value::Array(parts.iter().map(|p| rec(p, defs)).collect()),
                )],
            ),
            Doc::IfBreak {
                brk,
                flat,
                group_id,
            } => {
                let mut f = Vec::new();
                if let Some(id) = group_id {
                    f.push(("groupId", Value::String(id.clone())));
                }
                f.push(("breakContents", rec(brk, defs)));
                f.push(("flatContents", rec(flat, defs)));
                obj("if-break", f)
            }
            Doc::IndentIfBreak {
                contents,
                group_id,
                negate,
            } => {
                let mut f = Vec::new();
                if let Some(id) = group_id {
                    f.push(("groupId", Value::String(id.clone())));
                }
                f.push(("contents", rec(contents, defs)));
                f.push(("negate", Value::Bool(*negate)));
                obj("indent-if-break", f)
            }
            Doc::LineSuffix(c) => obj("line-suffix", vec![("contents", rec(c, defs))]),
            Doc::LineSuffixBoundary => obj("line-suffix-boundary", vec![]),
            Doc::Line {
                hard,
                soft,
                literal,
            } => {
                let mut f = Vec::new();
                for (k, v) in [("hard", hard), ("soft", soft), ("literal", literal)] {
                    if *v {
                        f.push((k, Value::Bool(true)));
                    }
                }
                obj("line", f)
            }
            Doc::Label(l, c) => obj(
                "label",
                vec![
                    ("label", Value::String(l.clone())),
                    ("contents", rec(c, defs)),
                ],
            ),
            Doc::BreakParent => obj("break-parent", vec![]),
            Doc::Shared(inner) => {
                let key = Rc::as_ptr(inner);
                if let Some(k) = defs.get(&key) {
                    return json!({ "$ref": k });
                }
                let k = defs.len();
                defs.insert(key, k);
                json!({ "$def": k, "doc": rec(inner, defs) })
            }
            Doc::Invalid(m, _) => obj("$invalid", vec![("message", Value::String(m.clone()))]),
        }
    }
    rec(d, &mut HashMap::new())
}

/// Prints a native doc as `printDocToString` would: propagate breaks, then print.
pub fn print_native(d: &Doc, o: &PrintOptions) -> Value {
    let r = port::propagate_breaks(d).and_then(|()| port::print(d, o));
    match r {
        Ok(p) => match p.cursor {
            None => json!(["ok", p.formatted, null, null]),
            Some((before, around)) => {
                json!(["ok", p.formatted, before.encode_utf16().count(), around])
            }
        },
        Err(e) => json!(["err", e]),
    }
}
