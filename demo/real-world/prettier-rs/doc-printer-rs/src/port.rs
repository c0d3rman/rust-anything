//! The Rust port of Prettier's doc printer (`src/document/printer.js`), with the string width it
//! measures by (`src/utils/get-string-width.js`, `get-east-asian-width`, `emoji-regex`).
//!
//! Prettier is MIT licensed, Copyright (c) James Long and contributors.
//! `get-east-asian-width` is MIT licensed, Copyright (c) Sindre Sorhus.
//! `emoji-regex` is MIT licensed, Copyright (c) Mathias Bynens.

#![forbid(unsafe_code)]

use std::cell::Cell;
use std::rc::Rc;
use std::sync::OnceLock;

use serde_json::{Map, Value};

#[path = "wide.rs"]
mod wide;

/// A Prettier document.
#[derive(Clone, Debug, PartialEq)]
pub enum Doc {
    Str(std::borrow::Cow<'static, str>),
    Array(Vec<Doc>),
    Cursor,
    Indent(Box<Doc>),
    Align(Align, Box<Doc>),
    Trim,
    Group {
        contents: Box<Doc>,
        brk: Cell<bool>,
        expanded: Option<Vec<Doc>>,
        id: Option<String>,
    },
    Fill(Vec<Doc>),
    IfBreak {
        brk: Box<Doc>,
        flat: Box<Doc>,
        group_id: Option<String>,
    },
    IndentIfBreak {
        contents: Box<Doc>,
        group_id: Option<String>,
        negate: bool,
    },
    LineSuffix(Box<Doc>),
    LineSuffixBoundary,
    Line {
        hard: bool,
        soft: bool,
        literal: bool,
    },
    Label(String, Box<Doc>),
    BreakParent,
    Shared(Rc<Doc>),
    /// A value that is not a doc (only `from_value` builds one): the error Prettier throws on
    /// reaching it, and whether the value is falsy in JS (`if (contents)` skips those).
    Invalid(String, bool),
}

/// A conditional group's `contents` and `expandedStates`, with `contents` the same node as
/// `states[0]` as `conditionalGroup` (and `mapDoc`) builds it: `states[0]` becomes a `Shared`
/// node and `contents` another handle to it. Break propagation walks only the states, so a
/// `contents` built any other way is not reached, as in Prettier.
pub fn conditional_group_parts(mut states: Vec<Doc>) -> (Doc, Vec<Doc>) {
    let Some(first) = states.first_mut() else {
        return (invalid(None), states);
    };
    if !matches!(first, Doc::Shared(_) | Doc::Str(_)) {
        let taken = std::mem::replace(first, Doc::Cursor);
        *first = Doc::Shared(Rc::new(taken));
    }
    (states[0].clone(), states)
}

/// The `n` of an `align` doc.
#[derive(Clone, Debug, PartialEq)]
pub enum Align {
    Width(f64),
    Text(String),
    DedentToRoot,
    Root,
}

/// The options the printer reads.
#[derive(Clone, Debug)]
pub struct PrintOptions {
    pub print_width: f64,
    pub tab_width: f64,
    pub use_tabs: bool,
    pub end_of_line: &'static str,
}

/// The printed text, and the cursor's place when the doc held two cursors.
#[derive(Clone, Debug, PartialEq)]
pub struct Printed {
    pub formatted: String,
    pub cursor: Option<(String, String)>,
}

/// Port of `printDocToString` (`src/document/printer.js:305`).
pub fn print_doc_to_string(doc: Value, options: Value) -> Result<Value, String> {
    let lone = Lone::scan(&doc);
    let doc = from_value(&doc, &lone);
    let options = read_options(&options)?;
    propagate_breaks(&doc)?;
    let printed = print(&doc, &options)?;
    let mut out = Map::new();
    match printed.cursor {
        None => {
            out.insert("formatted".into(), lone.text_value(&printed.formatted));
        }
        Some((before, around)) => {
            let start = lone.units_of(&before).len();
            out.insert("formatted".into(), lone.text_value(&printed.formatted));
            out.insert("cursorNodeStart".into(), Value::from(start));
            out.insert("cursorNodeText".into(), lone.text_value(&around));
        }
    }
    Ok(Value::Object(out))
}

/// How a doc's lone UTF-16 surrogates (`{"$utf16": [...]}` strings) are held in Rust strings.
/// Each distinct lone unit gets a stand-in character that occurs nowhere else in the doc, taken
/// from the private use areas (narrow, not part of any emoji sequence, so it measures 1 like a
/// lone surrogate does in `getStringWidth`; the BMP ones are also one UTF-16 unit long). The
/// printer only emits characters of the doc plus whitespace, so a stand-in in the output is
/// always the lone unit it stands for. Native docs (Rust strings) never hold lone surrogates.
#[derive(Clone, Debug, Default)]
pub struct Lone {
    units: Vec<(u16, char)>,
}

impl Lone {
    /// Picks stand-ins for every lone unit in `doc`'s `$utf16` strings.
    pub fn scan(doc: &Value) -> Lone {
        fn walk(v: &Value, used: &mut fx::FxHashSet<char>, lone: &mut Vec<u16>) {
            match v {
                Value::String(s) => used.extend(s.chars()),
                Value::Array(items) => items.iter().for_each(|i| walk(i, used, lone)),
                Value::Object(map) => {
                    if let Some(units) = utf16_marker(v) {
                        for r in char::decode_utf16(marker_units(units)) {
                            match r {
                                Ok(c) => {
                                    used.insert(c);
                                }
                                Err(e) => lone.push(e.unpaired_surrogate()),
                            }
                        }
                        return;
                    }
                    for (k, item) in map {
                        used.extend(k.chars());
                        walk(item, used, lone);
                    }
                }
                _ => {}
            }
        }
        let mut used = fx::FxHashSet::default();
        let mut found = Vec::new();
        walk(doc, &mut used, &mut found);
        if found.is_empty() {
            return Lone::default();
        }
        found.sort_unstable();
        found.dedup();
        let mut candidates = (0xE000u32..=0xF8FF)
            .chain(0xF_0000..=0xF_FFFD)
            .chain(0x10_0000..=0x10_FFFD)
            .filter_map(char::from_u32)
            .filter(|c| !used.contains(c));
        let units = found
            .into_iter()
            .map(|u| (u, candidates.next().unwrap_or('\u{FFFD}')))
            .collect();
        Lone { units }
    }

    fn stand_in(&self, unit: u16) -> char {
        self.units
            .iter()
            .find(|(u, _)| *u == unit)
            .map_or('\u{FFFD}', |(_, c)| *c)
    }

    fn unit_of(&self, c: char) -> Option<u16> {
        self.units.iter().find(|(_, s)| *s == c).map(|(u, _)| *u)
    }

    fn units_of(&self, text: &str) -> Vec<u16> {
        let mut units = Vec::with_capacity(text.len());
        for c in text.chars() {
            match self.unit_of(c) {
                Some(u) => units.push(u),
                None => {
                    let mut buf = [0u16; 2];
                    units.extend_from_slice(c.encode_utf16(&mut buf));
                }
            }
        }
        units
    }

    fn text_value(&self, text: &str) -> Value {
        if !self.units.is_empty() && text.chars().any(|c| self.unit_of(c).is_some()) {
            let units: Vec<Value> = self.units_of(text).into_iter().map(Value::from).collect();
            let mut marker = Map::new();
            marker.insert("$utf16".into(), Value::Array(units));
            Value::Object(marker)
        } else {
            Value::String(text.to_string())
        }
    }
}

fn js_type(v: &Value) -> &'static str {
    match v {
        Value::Null => "null",
        Value::Bool(_) => "boolean",
        Value::Number(_) => "number",
        Value::String(_) => "string",
        Value::Array(_) | Value::Object(_) => "object",
    }
}

fn js_string(v: &Value) -> String {
    match v {
        Value::Null => "null".into(),
        Value::Bool(b) => b.to_string(),
        Value::Number(n) => crate::js::number_to_string(n.as_f64().unwrap_or(f64::NAN)),
        Value::String(s) => s.clone(),
        Value::Array(items) => items
            .iter()
            .map(|i| match i {
                Value::Null => String::new(),
                other => js_string(other),
            })
            .collect::<Vec<_>>()
            .join(","),
        Value::Object(_) => "[object Object]".into(),
    }
}

fn truthy(v: Option<&Value>) -> bool {
    match v {
        None | Some(Value::Null) => false,
        Some(Value::Bool(b)) => *b,
        Some(Value::Number(n)) => n.as_f64().is_some_and(|x| x != 0.0 && !x.is_nan()),
        Some(Value::String(s)) => !s.is_empty(),
        Some(_) => true,
    }
}

fn utf16_marker(v: &Value) -> Option<&Vec<Value>> {
    let Value::Object(map) = v else { return None };
    if map.len() != 1 {
        return None;
    }
    map.get("$utf16").and_then(Value::as_array)
}

fn undefined_marker(v: &Value) -> bool {
    matches!(v, Value::Object(map) if map.len() == 1 && map.get("$undefined") == Some(&Value::Bool(true)))
}

fn number_marker(v: &Value) -> Option<f64> {
    let Value::Object(map) = v else { return None };
    if map.len() != 1 {
        return None;
    }
    match map.get("$num")?.as_str()? {
        "Infinity" => Some(f64::INFINITY),
        "-Infinity" => Some(f64::NEG_INFINITY),
        "NaN" => Some(f64::NAN),
        "-0" => Some(-0.0),
        _ => None,
    }
}

fn marker_units(units: &[Value]) -> Vec<u16> {
    units
        .iter()
        .map(|u| u.as_u64().unwrap_or(0) as u16)
        .collect()
}

fn decode_units(units: &[Value], lone: &Lone) -> String {
    let units = marker_units(units);
    let mut out = String::with_capacity(units.len());
    for r in char::decode_utf16(units.iter().copied()) {
        match r {
            Ok(c) => out.push(c),
            Err(e) => out.push(lone.stand_in(e.unpaired_surrogate())),
        }
    }
    out
}

fn invalid(v: Option<&Value>) -> Doc {
    let message = match v {
        None => "Unexpected doc 'undefined', \nExpected it to be 'string' or 'object'.".into(),
        Some(v) if undefined_marker(v) => {
            "Unexpected doc 'undefined', \nExpected it to be 'string' or 'object'.".into()
        }
        Some(v) if js_type(v) != "object" || number_marker(v).is_some() => {
            let t = if number_marker(v).is_some() {
                "number"
            } else {
                js_type(v)
            };
            format!("Unexpected doc '{t}', \nExpected it to be 'string' or 'object'.")
        }
        Some(v) => {
            let t = match v.get("type") {
                None => "undefined".to_string(),
                Some(t) if undefined_marker(t) => "undefined".to_string(),
                Some(t) => js_string(t),
            };
            format!(
                "Unexpected doc.type '{t}'.\nExpected it to be 'cursor', 'indent', 'align', 'trim', \
                 'group', 'fill', 'if-break', 'indent-if-break', 'line-suffix', \
                 'line-suffix-boundary', 'line', 'label', or 'break-parent'."
            )
        }
    };
    let falsy = match v.and_then(number_marker) {
        Some(n) => n == 0.0 || n.is_nan(),
        None => !truthy(v) || v.is_some_and(undefined_marker),
    };
    Doc::Invalid(message, falsy)
}

fn group_id(v: Option<&Value>) -> Option<String> {
    if !truthy(v) {
        return None;
    }
    v.map(js_string)
}

/// Reads a doc from its JSON form (group ids as strings).
pub fn from_value(v: &Value, lone: &Lone) -> Doc {
    let child = |key: &str, lone: &Lone| -> Box<Doc> {
        Box::new(match v.get(key) {
            Some(c) => from_value(c, lone),
            None => invalid(None),
        })
    };
    match v {
        Value::String(s) => Doc::Str(s.clone().into()),
        Value::Array(items) => Doc::Array(items.iter().map(|i| from_value(i, lone)).collect()),
        Value::Object(map) => {
            if let Some(units) = utf16_marker(v) {
                return Doc::Str(decode_units(units, lone).into());
            }
            match map.get("type").and_then(Value::as_str) {
                Some("cursor") => Doc::Cursor,
                Some("indent") => Doc::Indent(child("contents", lone)),
                Some("align") => {
                    let n = match map.get("n") {
                        Some(Value::String(s)) if !s.is_empty() => Align::Text(s.clone()),
                        Some(n) if utf16_marker(n).is_some_and(|u| !u.is_empty()) => {
                            Align::Text(decode_units(utf16_marker(n).unwrap_or(&Vec::new()), lone))
                        }
                        Some(Value::Number(n)) => Align::Width(n.as_f64().unwrap_or(f64::NAN)),
                        Some(n) if number_marker(n) == Some(f64::NEG_INFINITY) => {
                            Align::DedentToRoot
                        }
                        Some(n) if number_marker(n).is_some() => {
                            Align::Width(number_marker(n).unwrap_or(f64::NAN))
                        }
                        Some(n @ Value::Object(_))
                            if n.get("type").and_then(Value::as_str) == Some("root") =>
                        {
                            Align::Root
                        }
                        Some(Value::Bool(true)) => Align::Width(1.0),
                        Some(Value::Object(_) | Value::Array(_)) => Align::Width(f64::NAN),
                        _ => Align::Width(0.0),
                    };
                    Doc::Align(n, child("contents", lone))
                }
                Some("trim") => Doc::Trim,
                Some("group") => {
                    let expanded: Option<Vec<Doc>> = map.get("expandedStates").and_then(|s| {
                        s.as_array()
                            .map(|states| states.iter().map(|d| from_value(d, lone)).collect())
                    });
                    // `"cg": 1` marks a group whose `contents` is the same object as
                    // `expandedStates[0]` (as `conditionalGroup` builds it); JSON alone cannot
                    // say so.
                    let (contents, expanded) = match expanded {
                        Some(states) if truthy(map.get("cg")) => {
                            let (contents, states) = conditional_group_parts(states);
                            (Box::new(contents), Some(states))
                        }
                        other => (child("contents", lone), other),
                    };
                    Doc::Group {
                        contents,
                        brk: Cell::new(truthy(map.get("break"))),
                        expanded,
                        id: group_id(map.get("id")),
                    }
                }
                Some("fill") => Doc::Fill(match map.get("parts") {
                    Some(Value::Array(parts)) => {
                        parts.iter().map(|p| from_value(p, lone)).collect()
                    }
                    _ => vec![invalid(None)],
                }),
                Some("if-break") => Doc::IfBreak {
                    brk: child("breakContents", lone),
                    flat: child("flatContents", lone),
                    group_id: group_id(map.get("groupId")),
                },
                Some("indent-if-break") => Doc::IndentIfBreak {
                    contents: child("contents", lone),
                    group_id: group_id(map.get("groupId")),
                    negate: truthy(map.get("negate")),
                },
                Some("line-suffix") => Doc::LineSuffix(child("contents", lone)),
                Some("line-suffix-boundary") => Doc::LineSuffixBoundary,
                Some("line") => Doc::Line {
                    hard: truthy(map.get("hard")),
                    soft: truthy(map.get("soft")),
                    literal: truthy(map.get("literal")),
                },
                Some("label") => Doc::Label(
                    map.get("label").map(js_string).unwrap_or_default(),
                    child("contents", lone),
                ),
                Some("break-parent") => Doc::BreakParent,
                _ => invalid(Some(v)),
            }
        }
        other => invalid(Some(other)),
    }
}

fn to_number(v: Option<&Value>) -> f64 {
    match v {
        None => f64::NAN,
        Some(Value::Null) => 0.0,
        Some(Value::Bool(b)) => f64::from(u8::from(*b)),
        Some(Value::Number(n)) => n.as_f64().unwrap_or(f64::NAN),
        Some(Value::String(s)) => {
            let t = s.trim();
            if t.is_empty() {
                0.0
            } else {
                t.parse().unwrap_or(f64::NAN)
            }
        }
        Some(v) => number_marker(v).unwrap_or(f64::NAN),
    }
}

fn read_options(v: &Value) -> Result<PrintOptions, String> {
    if v.is_null() {
        return Err("Cannot read properties of null (reading 'printWidth')".into());
    }
    if undefined_marker(v) {
        return Err("Cannot read properties of undefined (reading 'printWidth')".into());
    }
    let end_of_line = match v.get("endOfLine").and_then(Value::as_str) {
        Some("cr") => "\r",
        Some("crlf") => "\r\n",
        _ => "\n",
    };
    Ok(PrintOptions {
        print_width: to_number(v.get("printWidth")),
        tab_width: to_number(v.get("tabWidth")),
        use_tabs: truthy(v.get("useTabs")),
        end_of_line,
    })
}

/// Port of `propagateBreaks` (`src/document/utils.js:144`): a `break-parent`, or a group that
/// breaks, breaks every enclosing group that has no expanded states.
pub fn propagate_breaks(doc: &Doc) -> Result<(), String> {
    propagate(doc, &mut fx::FxHashMap::default()).map(|_| ())
}

fn propagate(doc: &Doc, shared: &mut fx::FxHashMap<*const Doc, bool>) -> Result<bool, String> {
    Ok(match doc {
        Doc::Str(_) | Doc::Cursor | Doc::Trim | Doc::LineSuffixBoundary | Doc::Line { .. } => false,
        Doc::BreakParent => true,
        Doc::Invalid(message, _) => return Err(message.clone()),
        Doc::Shared(inner) => {
            let key = Rc::as_ptr(inner);
            if let Some(&done) = shared.get(&key) {
                return Ok(done);
            }
            let done = propagate(inner, shared)?;
            shared.insert(key, done);
            done
        }
        Doc::Array(parts) | Doc::Fill(parts) => {
            let mut any = false;
            for part in parts {
                any |= propagate(part, shared)?;
            }
            any
        }
        Doc::IfBreak { brk, flat, .. } => {
            let a = propagate(brk, shared)?;
            let b = propagate(flat, shared)?;
            a || b
        }
        Doc::Group {
            contents,
            brk,
            expanded,
            ..
        } => {
            match expanded {
                // `traverseDoc(..., shouldTraverseConditionalGroups = true)` walks the states and
                // not `contents`. The builders make `contents` the same object as `states[0]`
                // (here: the same `Shared` node, see `conditional_group_parts`), so it is reached
                // through the states; a `contents` that is a different object is never walked.
                Some(states) => {
                    for state in states {
                        propagate(state, shared)?;
                    }
                }
                None => {
                    if propagate(contents, shared)? {
                        brk.set(true);
                    }
                }
            }
            brk.get()
        }
        Doc::Indent(c)
        | Doc::Align(_, c)
        | Doc::IndentIfBreak { contents: c, .. }
        | Doc::Label(_, c)
        | Doc::LineSuffix(c) => propagate(c, shared)?,
    })
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Mode {
    Break,
    Flat,
}

#[derive(Clone, Debug)]
enum Part {
    Indent,
    Text(String),
    Spaces(f64),
}

#[derive(Clone, Debug, Default)]
struct Ind {
    value: String,
    length: f64,
    queue: Vec<Part>,
    root: Option<Rc<Ind>>,
}

fn repeat(s: &str, n: f64) -> String {
    if n.is_nan() || n < 1.0 {
        String::new()
    } else {
        s.repeat(n as usize)
    }
}

fn generate_ind(ind: &Ind, part: Option<Part>, options: &PrintOptions) -> Ind {
    let mut queue = ind.queue.clone();
    match part {
        Some(p) => queue.push(p),
        None => {
            queue.pop();
        }
    }
    let mut value = String::new();
    let mut length = 0.0;
    let mut last_tabs = 0.0;
    let mut last_spaces = 0.0;
    let add_tabs = |value: &mut String, length: &mut f64, count: f64| {
        value.push_str(&repeat("\t", count));
        *length += options.tab_width * count;
    };
    let add_spaces = |value: &mut String, length: &mut f64, count: f64| {
        value.push_str(&repeat(" ", count));
        *length += count;
    };
    for part in &queue {
        match part {
            Part::Indent | Part::Text(_) => {
                if options.use_tabs {
                    if last_tabs > 0.0 {
                        add_tabs(&mut value, &mut length, last_tabs);
                    }
                } else if last_spaces > 0.0 {
                    add_spaces(&mut value, &mut length, last_spaces);
                }
                last_tabs = 0.0;
                last_spaces = 0.0;
                match part {
                    Part::Indent => {
                        if options.use_tabs {
                            add_tabs(&mut value, &mut length, 1.0);
                        } else {
                            add_spaces(&mut value, &mut length, options.tab_width);
                        }
                    }
                    Part::Text(s) => {
                        value.push_str(s);
                        length += s.encode_utf16().count() as f64;
                    }
                    Part::Spaces(_) => {}
                }
            }
            Part::Spaces(n) => {
                last_tabs += 1.0;
                last_spaces += n;
            }
        }
    }
    if last_spaces > 0.0 {
        add_spaces(&mut value, &mut length, last_spaces);
    }
    Ind {
        value,
        length,
        queue,
        root: ind.root.clone(),
    }
}

fn make_align(ind: &Rc<Ind>, n: &Align, options: &PrintOptions) -> Rc<Ind> {
    match n {
        Align::DedentToRoot => ind.root.clone().unwrap_or_default(),
        Align::Width(w) if *w < 0.0 => Rc::new(generate_ind(ind, None, options)),
        // `if (!widthOrDoc) return ind;`: 0, NaN and "" are all falsy.
        Align::Width(w) if *w == 0.0 || w.is_nan() => ind.clone(),
        Align::Text(s) if s.is_empty() => ind.clone(),
        Align::Root => Rc::new(Ind {
            root: Some(ind.clone()),
            ..(**ind).clone()
        }),
        Align::Text(s) => Rc::new(generate_ind(ind, Some(Part::Text(s.clone())), options)),
        Align::Width(w) => Rc::new(generate_ind(ind, Some(Part::Spaces(*w)), options)),
    }
}

#[derive(Clone, Copy, Debug)]
enum Node<'d> {
    Doc(&'d Doc),
    FillFrom(&'d [Doc], usize),
    Indented(&'d Doc),
    Hardline,
    Three(&'d Doc, &'d Doc, &'d Doc),
}

#[derive(Clone, Debug)]
struct Cmd<'d> {
    ind: Rc<Ind>,
    mode: Mode,
    node: Node<'d>,
}

#[derive(Debug)]
enum Seg<'a> {
    Text(std::borrow::Cow<'a, str>),
    Cursor,
}

fn trim_segments(out: &mut Vec<Seg<'_>>) -> f64 {
    let mut trimmed = 0usize;
    let mut cursors = 0usize;
    let mut index = out.len();
    let mut cut: Option<(usize, usize)> = None;
    while index > 0 {
        index -= 1;
        match &out[index] {
            Seg::Cursor => cursors += 1,
            Seg::Text(s) => {
                let kept = s.trim_end_matches([' ', '\t']);
                trimmed += s.len() - kept.len();
                if !kept.is_empty() {
                    cut = Some((index, kept.len()));
                    break;
                }
            }
        }
    }
    if let Some((i, len)) = cut
        && let Seg::Text(s) = &mut out[i]
    {
        s.to_mut().truncate(len);
    }
    if trimmed > 0 || cursors > 0 {
        let keep = match cut {
            Some((i, _)) => i + 1,
            None => 0,
        };
        out.truncate(keep);
        for _ in 0..cursors {
            out.push(Seg::Cursor);
        }
    }
    trimmed as f64
}

fn trim_measured(out: &mut Vec<&str>) -> f64 {
    let mut trimmed = 0usize;
    while let Some(last) = out.last_mut() {
        let kept = last.trim_end_matches([' ', '\t']);
        trimmed += last.len() - kept.len();
        if kept.is_empty() {
            out.pop();
        } else {
            *last = kept;
            break;
        }
    }
    trimmed as f64
}

fn fits(
    next: (Mode, Node<'_>),
    rest: &[Cmd<'_>],
    mut width: f64,
    mut has_line_suffix: bool,
    group_modes: &fx::FxHashMap<String, Mode>,
    must_be_flat: bool,
) -> bool {
    if width == f64::INFINITY {
        return true;
    }
    let mut rest_idx = rest.len();
    let mut cmds = vec![next];
    let mut out: Vec<&str> = Vec::new();
    while width >= 0.0 {
        let Some((mode, node)) = cmds.pop() else {
            if rest_idx == 0 {
                return true;
            }
            rest_idx -= 1;
            cmds.push((rest[rest_idx].mode, rest[rest_idx].node));
            continue;
        };
        let doc = match node {
            Node::Doc(d) => d,
            Node::FillFrom(parts, from) => {
                for part in parts[from.min(parts.len())..].iter().rev() {
                    cmds.push((mode, Node::Doc(part)));
                }
                continue;
            }
            Node::Indented(d) => {
                cmds.push((mode, Node::Doc(d)));
                continue;
            }
            Node::Hardline => return true,
            Node::Three(a, b, c) => {
                cmds.push((mode, Node::Doc(c)));
                cmds.push((mode, Node::Doc(b)));
                cmds.push((mode, Node::Doc(a)));
                continue;
            }
        };
        match doc {
            Doc::Str(s) => {
                out.push(s);
                width -= string_width(s);
            }
            Doc::Array(parts) | Doc::Fill(parts) => {
                for part in parts.iter().rev() {
                    cmds.push((mode, Node::Doc(part)));
                }
            }
            Doc::Indent(c)
            | Doc::Align(_, c)
            | Doc::IndentIfBreak { contents: c, .. }
            | Doc::Label(_, c) => cmds.push((mode, Node::Doc(c))),
            Doc::Shared(inner) => cmds.push((mode, Node::Doc(inner))),
            Doc::Trim => width += trim_measured(&mut out),
            Doc::Group {
                contents,
                brk,
                expanded,
                ..
            } => {
                if must_be_flat && brk.get() {
                    return false;
                }
                let group_mode = if brk.get() { Mode::Break } else { mode };
                let contents = match expanded {
                    Some(states) if group_mode == Mode::Break => match states.last() {
                        Some(last) => last,
                        None => continue,
                    },
                    _ => &**contents,
                };
                cmds.push((group_mode, Node::Doc(contents)));
            }
            Doc::IfBreak {
                brk,
                flat,
                group_id,
            } => {
                let group_mode = match group_id {
                    Some(id) => group_modes.get(id).copied().unwrap_or(Mode::Flat),
                    None => mode,
                };
                let contents = if group_mode == Mode::Break { brk } else { flat };
                if !is_falsy(contents) {
                    cmds.push((mode, Node::Doc(contents)));
                }
            }
            Doc::Line { hard, soft, .. } => {
                if mode == Mode::Break || *hard {
                    return true;
                }
                if !soft {
                    out.push(" ");
                    width -= 1.0;
                }
            }
            Doc::LineSuffix(_) => has_line_suffix = true,
            Doc::LineSuffixBoundary => {
                if has_line_suffix {
                    return false;
                }
            }
            Doc::Cursor | Doc::BreakParent | Doc::Invalid(..) => {}
        }
    }
    false
}

/// A doc JS treats as falsy: `""`, or a falsy non-doc (`null`, `undefined`, `0`, `false`, NaN).
fn is_falsy(doc: &Doc) -> bool {
    matches!(doc, Doc::Str(s) if s.is_empty()) || matches!(doc, Doc::Invalid(_, true))
}

/// Prints a doc whose breaks were already propagated.
pub fn print(doc: &Doc, options: &PrintOptions) -> Result<Printed, String> {
    let mut group_modes: fx::FxHashMap<String, Mode> = fx::FxHashMap::default();
    let width = options.print_width;
    let new_line = options.end_of_line;
    let mut pos = 0.0;
    let mut cmds = vec![Cmd {
        ind: Rc::new(Ind::default()),
        mode: Mode::Break,
        node: Node::Doc(doc),
    }];
    let mut out: Vec<Seg> = Vec::new();
    let mut should_remeasure = false;
    let mut line_suffix: Vec<Cmd> = Vec::new();
    let mut printed_cursors = 0;
    let hardline = Doc::Line {
        hard: true,
        soft: false,
        literal: false,
    };

    while let Some(Cmd { ind, mode, node }) = cmds.pop() {
        let doc = match node {
            Node::Doc(d) => d,
            Node::FillFrom(parts, from) => {
                print_fill(
                    parts,
                    from,
                    &ind,
                    mode,
                    &mut cmds,
                    width - pos,
                    !line_suffix.is_empty(),
                    &group_modes,
                )?;
                flush_suffix(&mut cmds, &mut line_suffix);
                continue;
            }
            Node::Indented(d) => {
                cmds.push(Cmd {
                    ind: Rc::new(generate_ind(&ind, Some(Part::Indent), options)),
                    mode,
                    node: Node::Doc(d),
                });
                flush_suffix(&mut cmds, &mut line_suffix);
                continue;
            }
            Node::Hardline => &hardline,
            Node::Three(..) => return Err("unexpected command".into()),
        };
        match doc {
            Doc::Str(s) => {
                let formatted: std::borrow::Cow<'_, str> = if new_line == "\n" {
                    std::borrow::Cow::Borrowed(&**s)
                } else {
                    std::borrow::Cow::Owned(s.replace('\n', new_line))
                };
                if !cmds.is_empty() {
                    pos += string_width(&formatted);
                }
                out.push(Seg::Text(formatted));
            }
            Doc::Array(parts) => {
                for part in parts.iter().rev() {
                    cmds.push(Cmd {
                        ind: ind.clone(),
                        mode,
                        node: Node::Doc(part),
                    });
                }
            }
            Doc::Cursor => {
                if printed_cursors >= 2 {
                    return Err("There are too many 'cursor' in doc.".into());
                }
                out.push(Seg::Cursor);
                printed_cursors += 1;
            }
            Doc::Indent(c) => cmds.push(Cmd {
                ind: Rc::new(generate_ind(&ind, Some(Part::Indent), options)),
                mode,
                node: Node::Doc(c),
            }),
            Doc::Align(n, c) => cmds.push(Cmd {
                ind: make_align(&ind, n, options),
                mode,
                node: Node::Doc(c),
            }),
            Doc::Trim => pos -= trim_segments(&mut out),
            Doc::Group {
                contents,
                brk,
                expanded,
                id,
            } => {
                let mut handled = false;
                if mode == Mode::Flat && !should_remeasure {
                    cmds.push(Cmd {
                        ind: ind.clone(),
                        mode: if brk.get() { Mode::Break } else { Mode::Flat },
                        node: Node::Doc(contents),
                    });
                    handled = true;
                }
                if !handled {
                    should_remeasure = false;
                    let next = (Mode::Flat, Node::Doc(contents));
                    let rem = width - pos;
                    let has_line_suffix = !line_suffix.is_empty();
                    if !brk.get() && fits(next, &cmds, rem, has_line_suffix, &group_modes, false) {
                        cmds.push(Cmd {
                            ind: ind.clone(),
                            mode: Mode::Flat,
                            node: Node::Doc(contents),
                        });
                    } else if let Some(states) = expanded {
                        // `expandedStates.at(-1)` of `[]` is `undefined`: with `break` Prettier
                        // pushes it and throws when it is popped; without, the loop never runs and
                        // the group prints nothing.
                        if brk.get() {
                            let Some(most_expanded) = states.last() else {
                                return Err("Unexpected doc 'undefined', \nExpected it to be 'string' or 'object'.".into());
                            };
                            cmds.push(Cmd {
                                ind: ind.clone(),
                                mode: Mode::Break,
                                node: Node::Doc(most_expanded),
                            });
                        } else {
                            for i in 1..=states.len() {
                                if i >= states.len() {
                                    cmds.push(Cmd {
                                        ind: ind.clone(),
                                        mode: Mode::Break,
                                        node: Node::Doc(&states[i - 1]),
                                    });
                                    break;
                                }
                                let cmd = (Mode::Flat, Node::Doc(&states[i]));
                                if fits(cmd, &cmds, rem, has_line_suffix, &group_modes, false) {
                                    cmds.push(Cmd {
                                        ind: ind.clone(),
                                        mode: Mode::Flat,
                                        node: Node::Doc(&states[i]),
                                    });
                                    break;
                                }
                            }
                        }
                    } else {
                        cmds.push(Cmd {
                            ind: ind.clone(),
                            mode: Mode::Break,
                            node: Node::Doc(contents),
                        });
                    }
                }
                if let Some(id) = id {
                    // `groupModeMap[doc.id] = cmds.at(-1).mode`: the stack is empty only after a
                    // group with no expanded states.
                    let Some(last) = cmds.last() else {
                        return Err("Cannot read properties of undefined (reading 'mode')".into());
                    };
                    group_modes.insert(id.clone(), last.mode);
                }
            }
            Doc::Fill(parts) => print_fill(
                parts,
                0,
                &ind,
                mode,
                &mut cmds,
                width - pos,
                !line_suffix.is_empty(),
                &group_modes,
            )?,
            Doc::IfBreak {
                brk,
                flat,
                group_id,
            } => {
                let group_mode = match group_id {
                    Some(id) => group_modes.get(id).copied(),
                    None => Some(mode),
                };
                let contents = match group_mode {
                    Some(Mode::Break) => Some(&**brk),
                    Some(Mode::Flat) => Some(&**flat),
                    None => None,
                };
                if let Some(c) = contents
                    && !is_falsy(c)
                {
                    cmds.push(Cmd {
                        ind: ind.clone(),
                        mode,
                        node: Node::Doc(c),
                    });
                }
            }
            Doc::IndentIfBreak {
                contents,
                group_id,
                negate,
            } => {
                let group_mode = match group_id {
                    Some(id) => group_modes.get(id).copied(),
                    None => Some(mode),
                };
                let node = match (group_mode, *negate) {
                    (Some(Mode::Break), false) | (Some(Mode::Flat), true) => {
                        Some(Node::Indented(contents))
                    }
                    (Some(Mode::Break), true) | (Some(Mode::Flat), false) => {
                        (!is_falsy(contents)).then_some(Node::Doc(contents))
                    }
                    (None, _) => None,
                };
                if let Some(node) = node {
                    cmds.push(Cmd {
                        ind: ind.clone(),
                        mode,
                        node,
                    });
                }
            }
            Doc::LineSuffix(c) => line_suffix.push(Cmd {
                ind: ind.clone(),
                mode,
                node: Node::Doc(c),
            }),
            Doc::LineSuffixBoundary => {
                if !line_suffix.is_empty() {
                    cmds.push(Cmd {
                        ind: ind.clone(),
                        mode,
                        node: Node::Hardline,
                    });
                }
            }
            Doc::Line {
                hard,
                soft,
                literal,
            } => {
                let mut breaking = mode == Mode::Break;
                if mode == Mode::Flat {
                    if *hard {
                        should_remeasure = true;
                        breaking = true;
                    } else if !soft {
                        out.push(Seg::Text(" ".into()));
                        pos += 1.0;
                    }
                }
                if breaking {
                    if !line_suffix.is_empty() {
                        cmds.push(Cmd { ind, mode, node });
                        line_suffix.reverse();
                        cmds.append(&mut line_suffix);
                    } else if *literal {
                        match &ind.root {
                            Some(root) => {
                                out.push(Seg::Text(new_line.into()));
                                out.push(Seg::Text(root.value.clone().into()));
                                pos = root.length;
                            }
                            None => {
                                out.push(Seg::Text(new_line.into()));
                                pos = 0.0;
                            }
                        }
                    } else {
                        trim_segments(&mut out);
                        out.push(Seg::Text(format!("{new_line}{}", ind.value).into()));
                        pos = ind.length;
                    }
                }
            }
            Doc::Label(_, c) => cmds.push(Cmd {
                ind: ind.clone(),
                mode,
                node: Node::Doc(c),
            }),
            Doc::Shared(inner) => cmds.push(Cmd {
                ind: ind.clone(),
                mode,
                node: Node::Doc(inner),
            }),
            Doc::BreakParent => {}
            Doc::Invalid(message, _) => return Err(message.clone()),
        }
        flush_suffix(&mut cmds, &mut line_suffix);
    }

    let cursors: Vec<usize> = out
        .iter()
        .enumerate()
        .filter(|(_, s)| matches!(s, Seg::Cursor))
        .map(|(i, _)| i)
        .collect();
    let join = |segs: &[Seg<'_>]| {
        let mut text = String::new();
        for s in segs {
            if let Seg::Text(t) = s {
                text.push_str(t);
            }
        }
        text
    };
    if cursors.len() >= 2 {
        let before = join(&out[..cursors[0]]);
        let around = join(&out[cursors[0] + 1..cursors[1]]);
        let after = join(&out[cursors[1] + 1..]);
        return Ok(Printed {
            formatted: format!("{before}{around}{after}"),
            cursor: Some((before, around)),
        });
    }
    Ok(Printed {
        formatted: join(&out),
        cursor: None,
    })
}

fn flush_suffix<'d>(cmds: &mut Vec<Cmd<'d>>, line_suffix: &mut Vec<Cmd<'d>>) {
    if cmds.is_empty() && !line_suffix.is_empty() {
        line_suffix.reverse();
        cmds.append(line_suffix);
    }
}

#[allow(clippy::too_many_arguments)]
fn print_fill<'d>(
    parts: &'d [Doc],
    offset: usize,
    ind: &Rc<Ind>,
    mode: Mode,
    cmds: &mut Vec<Cmd<'d>>,
    rem: f64,
    has_line_suffix: bool,
    group_modes: &fx::FxHashMap<String, Mode>,
) -> Result<(), String> {
    let length = parts.len().saturating_sub(offset);
    if length == 0 {
        return Ok(());
    }
    let cmd = |mode: Mode, node: Node<'d>| Cmd {
        ind: ind.clone(),
        mode,
        node,
    };
    let content = &parts[offset];
    let content_fits = fits(
        (Mode::Flat, Node::Doc(content)),
        &[],
        rem,
        has_line_suffix,
        group_modes,
        true,
    );
    if length == 1 {
        let m = if content_fits {
            Mode::Flat
        } else {
            Mode::Break
        };
        cmds.push(cmd(m, Node::Doc(content)));
        return Ok(());
    }
    let whitespace = &parts[offset + 1];
    if length == 2 {
        let m = if content_fits {
            Mode::Flat
        } else {
            Mode::Break
        };
        cmds.push(cmd(m, Node::Doc(whitespace)));
        cmds.push(cmd(m, Node::Doc(content)));
        return Ok(());
    }
    let second = &parts[offset + 2];
    let both_fit = fits(
        (Mode::Flat, Node::Three(content, whitespace, second)),
        &[],
        rem,
        has_line_suffix,
        group_modes,
        true,
    );
    cmds.push(cmd(mode, Node::FillFrom(parts, offset + 2)));
    if both_fit {
        cmds.push(cmd(Mode::Flat, Node::Doc(whitespace)));
        cmds.push(cmd(Mode::Flat, Node::Doc(content)));
    } else if content_fits {
        cmds.push(cmd(Mode::Break, Node::Doc(whitespace)));
        cmds.push(cmd(Mode::Flat, Node::Doc(content)));
    } else {
        cmds.push(cmd(Mode::Break, Node::Doc(whitespace)));
        cmds.push(cmd(Mode::Break, Node::Doc(content)));
    }
    Ok(())
}

fn emoji_regex() -> Option<&'static regex::Regex> {
    static RE: OnceLock<Option<regex::Regex>> = OnceLock::new();
    RE.get_or_init(|| {
        let source = include_str!("emoji-regex.txt");
        let mut out = String::with_capacity(source.len() * 2);
        let mut chars = source.chars().peekable();
        while let Some(c) = chars.next() {
            if c != '\\' {
                out.push(c);
                continue;
            }
            let (len, kind) = match chars.next() {
                Some('u') => (4, 'u'),
                Some('x') => (2, 'x'),
                Some(other) => {
                    out.push('\\');
                    out.push(other);
                    continue;
                }
                None => break,
            };
            let hex: String = (0..len).filter_map(|_| chars.next()).collect();
            let unit = u32::from_str_radix(&hex, 16).ok()?;
            let mapped = if kind == 'u' { map_unit(unit) } else { unit };
            out.push_str(&format!("\\x{{{mapped:X}}}"));
        }
        regex::Regex::new(&out).ok()
    })
    .as_ref()
}

fn map_unit(unit: u32) -> u32 {
    if (0xD800..=0xDFFF).contains(&unit) {
        0xF0000 + unit - 0xD800
    } else {
        unit
    }
}

fn unmap_unit(cp: u32) -> u16 {
    if (0xF0000..=0xF07FF).contains(&cp) {
        (cp - 0xF0000 + 0xD800) as u16
    } else {
        cp as u16
    }
}

fn is_narrow(cp: u32) -> bool {
    wide::WIDE
        .binary_search_by(|&(lo, hi)| {
            if hi < cp {
                std::cmp::Ordering::Less
            } else if lo > cp {
                std::cmp::Ordering::Greater
            } else {
                std::cmp::Ordering::Equal
            }
        })
        .is_err()
}

/// Port of `getStringWidth` (`src/utils/get-string-width.js`): the columns `text` takes, with
/// emoji two wide and East Asian wide and fullwidth characters two wide.
pub fn string_width(text: &str) -> f64 {
    if text.is_empty() {
        return 0.0;
    }
    if text.bytes().all(|b| (0x20..=0x7F).contains(&b)) {
        return text.len() as f64;
    }
    let mapped: String = text
        .encode_utf16()
        .filter_map(|u| char::from_u32(map_unit(u32::from(u))))
        .collect();
    let replaced = match emoji_regex() {
        Some(re) => re.replace_all(&mapped, "  ").into_owned(),
        None => mapped,
    };
    let units: Vec<u16> = replaced.chars().map(|c| unmap_unit(c as u32)).collect();
    let mut width = 0.0;
    for r in char::decode_utf16(units) {
        let cp = match r {
            Ok(c) => c as u32,
            Err(e) => u32::from(e.unpaired_surrogate()),
        };
        if cp <= 0x1F || (0x7F..=0x9F).contains(&cp) || (0x300..=0x36F).contains(&cp) {
            continue;
        }
        width += if is_narrow(cp) { 1.0 } else { 2.0 };
    }
    width
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn run(doc: Value, width: f64) -> String {
        let out = print_doc_to_string(
            doc,
            json!({"printWidth": width, "tabWidth": 2, "useTabs": false}),
        )
        .unwrap();
        out["formatted"].as_str().unwrap().to_string()
    }

    #[test]
    fn widths_match_prettier() {
        assert_eq!(string_width("abc"), 3.0);
        assert_eq!(string_width("日本"), 4.0);
        assert_eq!(string_width("😀"), 2.0);
        assert_eq!(string_width("👨‍👩‍👧"), 2.0);
        assert_eq!(string_width("e\u{301}"), 1.0);
        assert_eq!(string_width("a\tb"), 2.0);
    }

    #[test]
    fn group_breaks_when_too_wide() {
        let doc = json!({"type": "group", "contents": ["[", {"type": "indent", "contents": [{"type": "line", "soft": true}, "aaaa", ",", {"type": "line"}, "bbbb"]}, {"type": "line", "soft": true}, "]"]});
        assert_eq!(run(doc.clone(), 80.0), "[aaaa, bbbb]");
        assert_eq!(run(doc, 6.0), "[\n  aaaa,\n  bbbb\n]");
    }

    #[test]
    fn empty_string_align_is_a_no_op() {
        // `align("")` is falsy in `makeAlign`; native printers build `Align::Text("")`.
        let doc = Doc::Align(
            Align::Text(String::new()),
            Box::new(Doc::Array(vec![
                Doc::Line {
                    hard: true,
                    soft: false,
                    literal: false,
                },
                Doc::Str("e".into()),
            ])),
        );
        let options = PrintOptions {
            print_width: 80.0,
            tab_width: 0.0,
            use_tabs: false,
            end_of_line: "\n",
        };
        assert_eq!(print(&doc, &options).unwrap().formatted, "\ne");
    }

    #[test]
    fn conditional_group_contents_apart_from_states_is_not_propagated() {
        let inner = json!({"type": "group", "contents": ["a", {"type": "break-parent"}, {"type": "line"}, "b"]});
        let apart = json!({"type": "group", "contents": inner, "expandedStates": ["c"]});
        assert_eq!(run(apart, 80.0), "a b");
        let same = json!({"type": "group", "cg": 1, "contents": inner, "expandedStates": [inner]});
        assert_eq!(run(same, 80.0), "a\nb");
    }

    #[test]
    fn empty_expanded_states() {
        assert_eq!(
            run(
                json!({"type": "group", "contents": "a", "expandedStates": []}),
                0.0
            ),
            ""
        );
        let err = print_doc_to_string(
            json!({"type": "group", "id": "A", "contents": "a", "expandedStates": []}),
            json!({"printWidth": 0, "tabWidth": 2}),
        )
        .unwrap_err();
        assert_eq!(err, "Cannot read properties of undefined (reading 'mode')");
    }

    #[test]
    fn lone_surrogates_do_not_collide_with_real_characters() {
        let out = print_doc_to_string(
            json!([{"$utf16": [0xD800]}, "\u{10FC00}\u{E000}"]),
            json!({"printWidth": 80, "tabWidth": 2}),
        )
        .unwrap();
        assert_eq!(
            out["formatted"],
            json!({"$utf16": [0xD800, 0xDBFF, 0xDC00, 0xE000]})
        );
    }

    #[test]
    fn falsy_non_doc_branch_is_skipped() {
        // `if (contents)`: a `null` branch of an if-break is skipped, not an error.
        let doc = json!({"type": "group", "id": "A", "contents": {"type": "if-break", "groupId": "A", "breakContents": "a", "flatContents": null}, "expandedStates": []});
        assert_eq!(run(doc, 0.0), "");
    }

    #[test]
    fn trims_trailing_whitespace() {
        let doc = json!(["a  ", {"type": "line", "hard": true}, {"type": "break-parent"}, "b"]);
        assert_eq!(run(doc, 80.0), "a\nb");
    }
}

pub mod fx {
    use std::hash::{BuildHasherDefault, Hasher};

    #[derive(Default, Clone, Copy)]
    pub struct FxHasher {
        hash: u64,
    }

    const SEED: u64 = 0x51_7c_c1_b7_27_22_0a_95;

    impl FxHasher {
        fn add(&mut self, word: u64) {
            self.hash = (self.hash.rotate_left(5) ^ word).wrapping_mul(SEED);
        }
    }

    impl Hasher for FxHasher {
        fn write(&mut self, bytes: &[u8]) {
            let (chunks, rest) = bytes.as_chunks::<8>();
            for c in chunks {
                self.add(u64::from_le_bytes(*c));
            }
            for &b in rest {
                self.add(u64::from(b));
            }
        }
        fn write_u8(&mut self, i: u8) {
            self.add(u64::from(i));
        }
        fn write_u32(&mut self, i: u32) {
            self.add(u64::from(i));
        }
        fn write_u64(&mut self, i: u64) {
            self.add(i);
        }
        fn write_usize(&mut self, i: usize) {
            self.add(i as u64);
        }
        fn finish(&self) -> u64 {
            self.hash
        }
    }

    pub type FxBuild = BuildHasherDefault<FxHasher>;
    pub type FxHashMap<K, V> = std::collections::HashMap<K, V, FxBuild>;
    pub type FxHashSet<K> = std::collections::HashSet<K, FxBuild>;
}
