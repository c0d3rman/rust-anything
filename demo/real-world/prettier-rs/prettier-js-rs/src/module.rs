//! `src/language-js/print/module.js`.

use crate::value::Value;

use crate::doc::*;
use crate::node::*;
use crate::printer::{Args, Printer, R, bail};

/// `getTextWithoutComments`. Prettier splices each comment out with offsets relative to `start`
/// in the text as spliced so far, in UTF-16 units; a comment that ends exactly at `start` gives a
/// negative offset, and `String#slice` then counts it from the end. That case is ported as is.
pub fn text_without_comments(p: &Printer<'_>, start: usize, end: usize) -> String {
    let straddles = p
        .comments
        .list
        .iter()
        .any(|c| c.start < start && c.end >= start && c.start <= end);
    if !straddles {
        let mut bytes: Vec<u8> = p.text.as_bytes()[start..end].to_vec();
        for c in &p.comments.list {
            if c.start > end {
                break;
            }
            if c.end < start {
                continue;
            }
            for i in c.start.max(start)..c.end.min(end) {
                bytes[i - start] = b' ';
            }
        }
        return String::from_utf8_lossy(&bytes).into_owned();
    }
    let units = |b: usize| p.text[..b].encode_utf16().count() as i64;
    let slice = |t: &[u16], from: i64, to: Option<i64>| -> Vec<u16> {
        let len = t.len() as i64;
        let norm = |i: i64| if i < 0 { (len + i).max(0) } else { i.min(len) };
        let (a, b) = (norm(from), to.map_or(len, norm));
        if a >= b {
            Vec::new()
        } else {
            t[a as usize..b as usize].to_vec()
        }
    };
    let start16 = units(start);
    let mut text: Vec<u16> = p.text[start..end].encode_utf16().collect();
    for c in &p.comments.list {
        if c.start > end {
            break;
        }
        if c.end < start {
            continue;
        }
        let (cs, ce) = (units(c.start), units(c.end));
        let mut next = slice(&text, 0, Some(cs - start16));
        next.extend(std::iter::repeat_n(b' ' as u16, (ce - cs) as usize));
        next.extend(slice(&text, ce - start16, None));
        text = next;
    }
    String::from_utf16_lossy(&text)
}

fn kind(k: Option<&str>) -> Doc {
    match k {
        Some(k) if k != "value" => s(format!(" {k}")),
        _ => empty(),
    }
}

pub fn print_import_declaration(p: &mut Printer<'_>) -> R {
    let node = p.node();
    Ok(concat(vec![
        s("import"),
        match node.str_of("phase") {
            Some(phase) => s(format!(" {phase}")),
            None => empty(),
        },
        kind(node.str_of("importKind")),
        print_module_specifiers(p)?,
        print_module_source(p)?,
        print_import_attributes(p)?,
        s(if p.opts.semi { ";" } else { "" }),
    ]))
}

pub fn print_export_declaration(p: &mut Printer<'_>) -> R {
    let node = p.node();
    let is_default = node.is("ExportDefaultDeclaration");
    let mut parts = vec![
        crate::class::print_decorators_before_export(p)?,
        crate::typescript::print_declare_token(p),
        s("export"),
        s(if is_default { " default" } else { "" }),
    ];
    if p.has_comment(node, crate::comments::Flags::DANGLING) {
        parts.push(s(" "));
        parts.push(p.print_dangling(node, false, None));
        if crate::estree::needs_hardline_after_dangling_comment(p, node) {
            parts.push(hardline());
        }
    }
    if let Some(decl) = node.field("declaration") {
        parts.push(s(" "));
        parts.push(p.print("declaration")?);
        let omit = matches!(
            decl.ty(),
            "ClassDeclaration"
                | "FunctionDeclaration"
                | "TSInterfaceDeclaration"
                | "TSDeclareFunction"
                | "EnumDeclaration"
        );
        if p.opts.semi && is_default && !omit {
            parts.push(s(";"));
        }
        return Ok(concat(parts));
    }
    parts.push(kind(node.str_of("exportKind")));
    if node.is("ExportAllDeclaration") {
        parts.push(s(" *"));
        if node.field("exported").is_some() {
            parts.push(s(" as "));
            parts.push(p.print("exported")?);
        }
    } else {
        parts.push(print_module_specifiers(p)?);
    }
    parts.push(print_module_source(p)?);
    parts.push(print_import_attributes(p)?);
    if p.opts.semi {
        parts.push(s(";"));
    }
    Ok(concat(parts))
}

fn should_print_specifiers(p: &Printer<'_>, node: &Value) -> bool {
    if !node.is("ImportDeclaration")
        || !node.list("specifiers").is_empty()
        || node.str_of("importKind") == Some("type")
    {
        return true;
    }
    let source_start = node.field("source").map_or(node.end(), |s| s.start());
    text_without_comments(p, node.start(), source_start)
        .trim_end()
        .ends_with("from")
}

fn print_module_source(p: &mut Printer<'_>) -> R {
    let node = p.node();
    if node.field("source").is_none() {
        return Ok(empty());
    }
    let mut parts = Vec::new();
    if should_print_specifiers(p, node) {
        parts.push(s(" from"));
    }
    parts.push(s(" "));
    parts.push(p.print("source")?);
    Ok(concat(parts))
}

fn print_module_specifiers(p: &mut Printer<'_>) -> R {
    let node = p.node();
    if !should_print_specifiers(p, node) {
        return Ok(empty());
    }
    let mut parts = vec![s(" ")];
    if node.list("specifiers").is_empty() {
        parts.push(s("{}"));
        return Ok(concat(parts));
    }
    let mut standalone = Vec::new();
    let mut grouped = Vec::new();
    p.each("specifiers", |p, _| {
        let ty = p.node().ty();
        let d = p.print_current(Args::default())?;
        match ty {
            "ExportNamespaceSpecifier"
            | "ExportDefaultSpecifier"
            | "ImportNamespaceSpecifier"
            | "ImportDefaultSpecifier" => standalone.push(d),
            "ExportSpecifier" | "ImportSpecifier" => grouped.push(d),
            _ => return bail("specifier"),
        }
        Ok(())
    })?;
    let standalone_len = standalone.len();
    parts.push(join(&s(", "), standalone));
    if !grouped.is_empty() {
        if standalone_len > 0 {
            parts.push(s(", "));
        }
        let can_break = grouped.len() > 1
            || standalone_len > 0
            || node
                .list("specifiers")
                .iter()
                .any(|sp| p.has_any_comment(sp));
        let bracket = p.opts.bracket_spacing;
        if can_break {
            parts.push(group(concat(vec![
                s("{"),
                indent(concat(vec![
                    if bracket { line() } else { softline() },
                    join(&concat(vec![s(","), line()]), grouped),
                ])),
                if_break(
                    s(if p.opts.should_print_comma(true) {
                        ","
                    } else {
                        ""
                    }),
                    empty(),
                ),
                if bracket { line() } else { softline() },
                s("}"),
            ])));
        } else {
            let mut v = vec![s("{"), s(if bracket { " " } else { "" })];
            v.extend(grouped);
            v.push(s(if bracket { " " } else { "" }));
            v.push(s("}"));
            parts.push(concat(v));
        }
    }
    Ok(concat(parts))
}

fn print_import_attributes(p: &mut Printer<'_>) -> R {
    let node = p.node();
    let Some(source) = node.field("source") else {
        return Ok(empty());
    };
    let attrs = node.list("attributes");
    let end = attrs.first().map_or(node.end(), |a| a.start());
    let between_owned = text_without_comments(p, source.end(), end);
    let between = between_owned.trim_start();
    let keyword = if between.starts_with("assert") {
        "assert"
    } else if between.starts_with("with") || !attrs.is_empty() {
        "with"
    } else {
        return Ok(empty());
    };
    let mut parts = vec![s(format!(" {keyword} {{"))];
    if !attrs.is_empty() {
        if p.opts.bracket_spacing {
            parts.push(s(" "));
        }
        let printed = p.print_all("attributes")?;
        parts.push(join(&s(", "), printed));
        if p.opts.bracket_spacing {
            parts.push(s(" "));
        }
    }
    parts.push(s("}"));
    Ok(concat(parts))
}

pub fn print_module_specifier(p: &mut Printer<'_>) -> R {
    let node = p.node();
    let ty = node.ty();
    let is_import = ty.starts_with("Import");
    let (left_key, right_key) = if is_import {
        ("imported", "local")
    } else {
        ("local", "exported")
    };
    let left = if ty == "ExportNamespaceSpecifier" || ty == "ImportNamespaceSpecifier" {
        s("*")
    } else if node.field(left_key).is_some() {
        p.print(left_key)?
    } else {
        empty()
    };
    let right = if node.field(right_key).is_some() && !is_shorthand_specifier(node) {
        p.print(right_key)?
    } else {
        empty()
    };
    let both = !is_empty(&left) && !is_empty(&right);
    Ok(concat(vec![
        match if ty == "ImportSpecifier" {
            node.str_of("importKind")
        } else {
            node.str_of("exportKind")
        } {
            Some(k) if k != "value" => s(format!("{k} ")),
            _ => empty(),
        },
        left,
        s(if both { " as " } else { "" }),
        right,
    ]))
}

fn is_shorthand_specifier(n: &Value) -> bool {
    if !n.is("ImportSpecifier") && !n.is("ExportSpecifier") {
        return false;
    }
    let other_key = if n.is("ImportSpecifier") {
        "imported"
    } else {
        "exported"
    };
    let (Some(local), Some(other)) = (n.field("local"), n.field(other_key)) else {
        return false;
    };
    if local.ty() != other.ty() || local.start() != other.start() || local.end() != other.end() {
        return false;
    }
    if is_string_literal(local) {
        return local.str_of("value") == other.str_of("value")
            && local.str_of("raw") == other.str_of("raw");
    }
    local.is("Identifier") && local.str_of("name") == other.str_of("name")
}
