//! Doc builders and doc utilities: `src/document/builders.js` and `src/document/utils.js`.

use std::cell::Cell;
use std::rc::Rc;

pub use doc_printer_rs::port::{Align, Doc};

pub fn s(text: impl Into<std::borrow::Cow<'static, str>>) -> Doc {
    Doc::Str(text.into())
}

pub fn empty() -> Doc {
    Doc::Str(std::borrow::Cow::Borrowed(""))
}

pub fn concat(parts: Vec<Doc>) -> Doc {
    Doc::Array(parts)
}

pub fn group(contents: Doc) -> Doc {
    group_with(contents, false, None)
}

pub fn group_with(contents: Doc, should_break: bool, id: Option<String>) -> Doc {
    Doc::Group {
        contents: Box::new(contents),
        brk: Cell::new(should_break),
        expanded: None,
        id,
    }
}

pub fn conditional_group(states: Vec<Doc>, should_break: bool) -> Doc {
    cond_group(states, should_break, None)
}

/// A group with expanded states whose `contents` is the same node as `states[0]`, as
/// `conditionalGroup` and `mapDoc` build it.
pub fn cond_group(states: Vec<Doc>, should_break: bool, id: Option<String>) -> Doc {
    let (contents, states) = doc_printer_rs::port::conditional_group_parts(states);
    Doc::Group {
        contents: Box::new(contents),
        brk: Cell::new(should_break),
        expanded: Some(states),
        id,
    }
}

pub fn indent(contents: Doc) -> Doc {
    Doc::Indent(Box::new(contents))
}

pub fn align(n: f64, contents: Doc) -> Doc {
    if n == f64::NEG_INFINITY {
        return dedent_to_root(contents);
    }
    Doc::Align(Align::Width(n), Box::new(contents))
}

pub fn dedent_to_root(contents: Doc) -> Doc {
    Doc::Align(Align::DedentToRoot, Box::new(contents))
}

pub fn mark_as_root(contents: Doc) -> Doc {
    Doc::Align(Align::Root, Box::new(contents))
}

pub fn dedent(contents: Doc) -> Doc {
    Doc::Align(Align::Width(-1.0), Box::new(contents))
}

pub fn fill(parts: Vec<Doc>) -> Doc {
    Doc::Fill(parts)
}

pub fn if_break(break_contents: Doc, flat_contents: Doc) -> Doc {
    if_break_id(break_contents, flat_contents, None)
}

pub fn if_break_id(break_contents: Doc, flat_contents: Doc, group_id: Option<String>) -> Doc {
    Doc::IfBreak {
        brk: Box::new(break_contents),
        flat: Box::new(flat_contents),
        group_id,
    }
}

pub fn indent_if_break(contents: Doc, group_id: Option<String>, negate: bool) -> Doc {
    Doc::IndentIfBreak {
        contents: Box::new(contents),
        group_id,
        negate,
    }
}

pub fn line_suffix(contents: Doc) -> Doc {
    Doc::LineSuffix(Box::new(contents))
}

pub fn line_suffix_boundary() -> Doc {
    Doc::LineSuffixBoundary
}

pub fn break_parent() -> Doc {
    Doc::BreakParent
}

pub fn line() -> Doc {
    Doc::Line {
        hard: false,
        soft: false,
        literal: false,
    }
}

pub fn softline() -> Doc {
    Doc::Line {
        hard: false,
        soft: true,
        literal: false,
    }
}

pub fn hardline_without_break_parent() -> Doc {
    Doc::Line {
        hard: true,
        soft: false,
        literal: false,
    }
}

pub fn hardline() -> Doc {
    Doc::Array(vec![hardline_without_break_parent(), Doc::BreakParent])
}

pub fn literalline() -> Doc {
    Doc::Array(vec![
        Doc::Line {
            hard: true,
            soft: false,
            literal: true,
        },
        Doc::BreakParent,
    ])
}

pub fn label(name: &str, contents: Doc) -> Doc {
    if name.is_empty() {
        contents
    } else {
        Doc::Label(name.to_string(), Box::new(contents))
    }
}

pub fn label_of(doc: &Doc) -> Option<&str> {
    match doc {
        Doc::Label(name, _) => Some(name),
        Doc::Shared(inner) => label_of(inner),
        _ => None,
    }
}

pub fn inherit_label(doc: Doc, f: impl FnOnce(Doc) -> Doc) -> Doc {
    match doc {
        Doc::Label(name, contents) => Doc::Label(name, Box::new(f(*contents))),
        Doc::Shared(inner) if matches!(&*inner, Doc::Label(..)) => {
            let Doc::Label(name, contents) = (*inner).clone() else {
                return f(Doc::Shared(inner));
            };
            Doc::Label(name, Box::new(f(*contents)))
        }
        other => f(other),
    }
}

pub fn join(sep: &Doc, docs: Vec<Doc>) -> Doc {
    let mut parts = Vec::with_capacity(docs.len() * 2);
    for (i, d) in docs.into_iter().enumerate() {
        if i > 0 {
            parts.push(sep.clone());
        }
        parts.push(d);
    }
    Doc::Array(parts)
}

pub fn shared(doc: Doc) -> Doc {
    match doc {
        Doc::Shared(_) | Doc::Str(_) => doc,
        other => Doc::Shared(Rc::new(other)),
    }
}

fn find(doc: &Doc, f: &dyn Fn(&Doc) -> bool) -> bool {
    if f(doc) {
        return true;
    }
    match doc {
        Doc::Array(parts) | Doc::Fill(parts) => parts.iter().any(|p| find(p, f)),
        Doc::IfBreak { brk, flat, .. } => find(brk, f) || find(flat, f),
        Doc::Group { contents, .. }
        | Doc::Indent(contents)
        | Doc::Align(_, contents)
        | Doc::IndentIfBreak { contents, .. }
        | Doc::Label(_, contents)
        | Doc::LineSuffix(contents) => find(contents, f),
        Doc::Shared(inner) => find(inner, f),
        _ => false,
    }
}

pub fn will_break(doc: &Doc) -> bool {
    find(doc, &|d| match d {
        Doc::Group { brk, .. } => brk.get(),
        Doc::Line { hard, .. } => *hard,
        Doc::BreakParent => true,
        _ => false,
    })
}

pub fn can_break(doc: &Doc) -> bool {
    find(doc, &|d| matches!(d, Doc::Line { .. }))
}

/// `cleanDoc(doc)` when it reduces to a string.
pub fn clean_string(doc: &Doc) -> Option<String> {
    match doc {
        Doc::Str(text) => Some(text.to_string()),
        Doc::Array(parts) => {
            let mut out = String::new();
            for p in parts {
                out.push_str(&clean_string(p)?);
            }
            Some(out)
        }
        Doc::Shared(inner) => clean_string(inner),
        Doc::Fill(parts)
            if parts
                .iter()
                .all(|p| matches!(p, Doc::Str(t) if t.is_empty())) =>
        {
            Some(String::new())
        }
        _ => None,
    }
}

pub fn is_empty(doc: &Doc) -> bool {
    match doc {
        Doc::Str(text) => text.is_empty(),
        Doc::Array(parts) => parts.iter().all(is_empty),
        Doc::Shared(inner) => is_empty(inner),
        _ => false,
    }
}

/// `replaceEndOfLine(doc)`: every `\n` in a string becomes a `literalline`.
pub fn replace_end_of_line(text: &str) -> Doc {
    if !text.contains('\n') {
        return s(text.to_string());
    }
    join(
        &literalline(),
        text.split('\n').map(|part| s(part.to_string())).collect(),
    )
}

/// `removeLines(doc)`: soft and plain lines become `""` and `" "`, `if-break` its flat contents.
pub fn remove_lines(doc: &Doc) -> Doc {
    match doc {
        Doc::Line {
            hard: false, soft, ..
        } => s(if *soft { "" } else { " " }),
        Doc::IfBreak { flat, .. } => remove_lines(flat),
        Doc::Array(parts) => Doc::Array(parts.iter().map(remove_lines).collect()),
        Doc::Fill(parts) => Doc::Fill(parts.iter().map(remove_lines).collect()),
        Doc::Group {
            contents,
            brk,
            expanded,
            id,
        } => match expanded {
            Some(states) => cond_group(
                states.iter().map(remove_lines).collect(),
                brk.get(),
                id.clone(),
            ),
            None => Doc::Group {
                contents: Box::new(remove_lines(contents)),
                brk: Cell::new(brk.get()),
                expanded: None,
                id: id.clone(),
            },
        },
        Doc::Indent(c) => Doc::Indent(Box::new(remove_lines(c))),
        Doc::Align(n, c) => Doc::Align(n.clone(), Box::new(remove_lines(c))),
        Doc::IndentIfBreak {
            contents,
            group_id,
            negate,
        } => Doc::IndentIfBreak {
            contents: Box::new(remove_lines(contents)),
            group_id: group_id.clone(),
            negate: *negate,
        },
        Doc::LineSuffix(c) => Doc::LineSuffix(Box::new(remove_lines(c))),
        Doc::Label(name, c) => Doc::Label(name.clone(), Box::new(remove_lines(c))),
        Doc::Shared(inner) => remove_lines(inner),
        other => other.clone(),
    }
}

/// `cleanDoc(doc)`.
pub fn clean_doc(doc: &Doc) -> Doc {
    let mut cache: doc_printer_rs::port::fx::FxHashMap<*const Doc, Doc> = Default::default();
    clean_rec(doc, &mut cache)
}

fn is_falsy(d: &Doc) -> bool {
    matches!(d, Doc::Str(t) if t.is_empty())
}

type GroupParts<'a> = (&'a Doc, bool, &'a Option<Vec<Doc>>, &'a Option<String>);

fn as_group(d: &Doc) -> Option<GroupParts<'_>> {
    match d {
        Doc::Group {
            contents,
            brk,
            expanded,
            id,
        } => Some((contents, brk.get(), expanded, id)),
        Doc::Shared(inner) => as_group(inner),
        _ => None,
    }
}

fn clean_rec(doc: &Doc, cache: &mut doc_printer_rs::port::fx::FxHashMap<*const Doc, Doc>) -> Doc {
    match doc {
        Doc::Shared(inner) => {
            let key = Rc::as_ptr(inner);
            if let Some(d) = cache.get(&key) {
                return d.clone();
            }
            let cleaned = clean_rec(inner, cache);
            let result = match cleaned {
                Doc::Str(_) | Doc::Array(_) => cleaned,
                other => Doc::Shared(Rc::new(other)),
            };
            cache.insert(key, result.clone());
            result
        }
        Doc::Array(parts) => {
            let mut out: Vec<Doc> = Vec::new();
            for part in parts {
                let part = clean_rec(part, cache);
                if is_falsy(&part) {
                    continue;
                }
                let mut items = match part {
                    Doc::Array(items) => items.into_iter(),
                    other => vec![other].into_iter(),
                };
                if let Some(current) = items.next() {
                    match (&current, out.last_mut()) {
                        (Doc::Str(c), Some(Doc::Str(last))) => last.to_mut().push_str(c),
                        _ => out.push(current),
                    }
                }
                out.extend(items);
            }
            match out.len() {
                0 => s(""),
                1 => out.pop().unwrap_or_else(|| s("")),
                _ => Doc::Array(out),
            }
        }
        Doc::Fill(parts) => {
            let parts: Vec<Doc> = parts.iter().map(|p| clean_rec(p, cache)).collect();
            if parts.iter().all(is_falsy) {
                return s("");
            }
            Doc::Fill(parts)
        }
        Doc::IfBreak {
            brk,
            flat,
            group_id,
        } => {
            let b = clean_rec(brk, cache);
            let f = clean_rec(flat, cache);
            if is_falsy(&b) && is_falsy(&f) {
                return s("");
            }
            Doc::IfBreak {
                brk: Box::new(b),
                flat: Box::new(f),
                group_id: group_id.clone(),
            }
        }
        Doc::Group {
            contents,
            brk,
            expanded,
            id,
        } => {
            let (contents, expanded) = match expanded {
                Some(states) => {
                    let states = states.iter().map(|d| clean_rec(d, cache)).collect();
                    let (contents, states) = doc_printer_rs::port::conditional_group_parts(states);
                    (contents, Some(states))
                }
                None => (clean_rec(contents, cache), None),
            };
            if is_falsy(&contents) && id.is_none() && !brk.get() && expanded.is_none() {
                return s("");
            }
            if let Some((_, inner_brk, inner_expanded, inner_id)) = as_group(&contents)
                && inner_id == id
                && inner_brk == brk.get()
                && inner_expanded.is_none()
                && expanded.is_none()
            {
                return contents;
            }
            Doc::Group {
                contents: Box::new(contents),
                brk: Cell::new(brk.get()),
                expanded,
                id: id.clone(),
            }
        }
        Doc::Indent(c) => {
            let c = clean_rec(c, cache);
            if is_falsy(&c) {
                return s("");
            }
            Doc::Indent(Box::new(c))
        }
        Doc::Align(n, c) => {
            let c = clean_rec(c, cache);
            if is_falsy(&c) {
                return s("");
            }
            Doc::Align(n.clone(), Box::new(c))
        }
        Doc::IndentIfBreak {
            contents,
            group_id,
            negate,
        } => {
            let c = clean_rec(contents, cache);
            if is_falsy(&c) {
                return s("");
            }
            Doc::IndentIfBreak {
                contents: Box::new(c),
                group_id: group_id.clone(),
                negate: *negate,
            }
        }
        Doc::LineSuffix(c) => {
            let c = clean_rec(c, cache);
            if is_falsy(&c) {
                return s("");
            }
            Doc::LineSuffix(Box::new(c))
        }
        Doc::Label(name, c) => Doc::Label(name.clone(), Box::new(clean_rec(c, cache))),
        other => other.clone(),
    }
}
