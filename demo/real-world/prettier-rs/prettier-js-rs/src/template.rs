//! `src/language-js/print/template-literal.js`, with the embed detection of
//! `src/language-js/embed/*.js` (embedded languages are not ported, so those templates bail).

use crate::value::Value;

use crate::doc::*;
use crate::node::*;
use crate::printer::{Printer, R, bail};

fn render_flat(p: &Printer<'_>, doc: &Doc, end_of_line: &'static str) -> R<String> {
    let opts = doc_printer_rs::port::PrintOptions {
        print_width: f64::INFINITY,
        tab_width: p.opts.tab_width as f64,
        use_tabs: p.opts.use_tabs,
        end_of_line,
    };
    doc_printer_rs::port::propagate_breaks(doc).or_else(|e| bail(format!("doc: {e}")))?;
    match doc_printer_rs::port::print(doc, &opts) {
        Ok(printed) => Ok(printed.formatted),
        Err(e) => bail(format!("doc: {e}")),
    }
}

/// `None` stands for the `NaN` Prettier computes when a tab meets `tabWidth: 0`.
fn get_indent_size(value: &str, tab_width: usize) -> Option<usize> {
    let Some(i) = value.rfind('\n') else {
        return Some(0);
    };
    let mut size = Some(0);
    for c in value[i + 1..].chars() {
        match c {
            '\t' => {
                size = size
                    .filter(|_| tab_width > 0)
                    .map(|n| n + tab_width - (n % tab_width))
            }
            ' ' => size = size.map(|n| n + 1),
            _ => break,
        }
    }
    size
}

fn add_alignment_to_doc(doc: Doc, size: Option<usize>, tab_width: usize) -> R<Doc> {
    let Some(size) = size.filter(|&n| n > 0) else {
        return Ok(doc);
    };
    if tab_width == 0 {
        // `Math.floor(size / 0)` is `Infinity`: Prettier never finishes this loop.
        return bail("template alignment with tabWidth 0");
    }
    let mut aligned = doc;
    for _ in 0..size / tab_width {
        aligned = indent(aligned);
    }
    aligned = align((size % tab_width) as f64, aligned);
    Ok(dedent_to_root(aligned))
}

fn raw(q: &Value) -> &str {
    q.get("value").and_then(|v| v.str_of("raw")).unwrap_or("")
}

fn name_is(n: Option<&Value>, ty: &str, name: &str) -> bool {
    n.is_some_and(|n| n.is(ty) && n.str_of("name") == Some(name))
}

fn is_styled_extend(tag: &Value) -> bool {
    tag.field("object")
        .and_then(|o| o.str_of("name"))
        .is_some_and(|n| n.starts_with(|c: char| c.is_ascii_uppercase()))
        && tag.field("property").and_then(|p| p.str_of("name")) == Some("extend")
}

pub fn is_embedded(p: &Printer<'_>) -> bool {
    let node = p.node();
    if node.list("quasis").iter().any(|q| {
        q.get("value")
            .and_then(|v| v.get("cooked"))
            .is_none_or(Value::is_null)
    }) {
        return false;
    }
    let Some(parent) = p.parent() else {
        return false;
    };
    let language_comment = |name: &str| {
        let want = format!(" {name} ");
        let has = |n: &Value| {
            p.comments.has_with(
                n,
                crate::comments::Flags::BLOCK.with(crate::comments::Flags::LEADING),
                &|c| c.value == want,
            )
        };
        has(node)
            || (parent.is("ExpressionStatement") && has(parent))
            || ((parent.is("AsConstExpression")
                || (parent.is("TSAsExpression")
                    && parent.field("typeAnnotation").is_some_and(|t| {
                        t.is("TSTypeReference")
                            && name_is(t.field("typeName"), "Identifier", "const")
                    })))
                && has(parent))
    };
    if language_comment("GraphQL") || language_comment("HTML") {
        return true;
    }
    if is_angular_component_part(p) {
        return true;
    }
    if parent.is("JSXExpressionContainer")
        && let Some(gp) = p.grandparent()
    {
        let style_jsx = gp.is("JSXElement")
            && gp.field("openingElement").is_some_and(|o| {
                o.field("name").and_then(|n| n.str_of("name")) == Some("style")
                    && o.list("attributes").iter().any(|a| {
                        a.is("JSXAttribute")
                            && a.field("name").and_then(|n| n.str_of("name")) == Some("jsx")
                    })
            });
        let css_prop = gp.is("JSXAttribute")
            && gp
                .field("name")
                .is_some_and(|n| n.is("JSXIdentifier") && n.str_of("name") == Some("css"));
        if style_jsx || css_prop {
            return true;
        }
    }
    if parent.is("CallExpression") && name_is(parent.field("callee"), "Identifier", "graphql") {
        return true;
    }
    if !parent.is("TaggedTemplateExpression") {
        return false;
    }
    let Some(mut tag) = parent.field("tag") else {
        return false;
    };
    if tag.is("ParenthesizedExpression") {
        tag = tag.field("expression").unwrap_or(tag);
    }
    let styled = |n: Option<&Value>| name_is(n, "Identifier", "styled");
    match tag.ty() {
        "Identifier" => matches!(
            tag.str_of("name"),
            Some("css" | "gql" | "graphql" | "html" | "md" | "markdown")
        ),
        "MemberExpression" => {
            let obj = tag.field("object").and_then(|o| o.str_of("name"));
            let prop = tag.field("property").and_then(|o| o.str_of("name"));
            (obj == Some("css") && matches!(prop, Some("global" | "resolve")))
                || (obj == Some("graphql") && prop == Some("experimental"))
                || styled(tag.field("object"))
                || is_styled_extend(tag)
        }
        "CallExpression" => {
            let callee = tag.field("callee");
            styled(callee)
                || callee.is_some_and(|c| {
                    c.is("MemberExpression")
                        && c.field("object").is_some_and(|o| {
                            (o.is("MemberExpression")
                                && (styled(o.field("object")) || is_styled_extend(o)))
                                || (o.is("CallExpression") && styled(o.field("callee")))
                        })
                })
        }
        _ => false,
    }
}

fn is_jest_each(p: &Printer<'_>) -> bool {
    let node = p.node();
    let Some(parent) = p.parent() else {
        return false;
    };
    let trigger = |n: Option<&Value>| {
        n.is_some_and(|n| n.is("Identifier"))
            && n.and_then(|n| n.str_of("name")).is_some_and(|name| {
                let base = |s: &str| matches!(s, "describe" | "it" | "test");
                base(name) || name.strip_prefix(['f', 'x']).is_some_and(base)
            })
    };
    let Some(tag) = parent.field("tag") else {
        return false;
    };
    parent.is("TaggedTemplateExpression")
        && parent.field("quasi").is_some_and(|q| q.same(node))
        && tag.is("MemberExpression")
        && name_is(tag.field("property"), "Identifier", "each")
        && (trigger(tag.field("object"))
            || tag.field("object").is_some_and(|o| {
                o.is("MemberExpression")
                    && o.field("property").is_some_and(|pr| {
                        pr.is("Identifier") && matches!(pr.str_of("name"), Some("only" | "skip"))
                    })
                    && trigger(o.field("object"))
            }))
}

pub fn print_template_literal(p: &mut Printer<'_>) -> R {
    if is_embedded(p) {
        return bail("embedded language");
    }
    if is_jest_each(p)
        && let Some(printed) = print_jest_each(p)?
    {
        return Ok(printed);
    }
    let node = p.node();
    let exprs_key: &'static str = if p.node().is("TSTemplateLiteralType") {
        "types"
    } else {
        "expressions"
    };
    let expression_docs = p.print_all(exprs_key)?;
    let tab_width = p.opts.tab_width;
    let text = p.text;
    let quasis = node.list("quasis");
    let expressions = node.list(exprs_key);
    let mut parts = vec![line_suffix_boundary(), s("`")];
    let mut previous_indent = Some(0);
    let printed_quasis = p.print_all("quasis")?;
    let mut expression_docs = expression_docs.into_iter();
    for (index, (quasi, printed)) in quasis.iter().zip(printed_quasis).enumerate() {
        parts.push(printed);
        if quasi.flag("tail") {
            continue;
        }
        let raw = raw(quasi);
        let indent_size = if raw.contains('\n') {
            get_indent_size(raw, tab_width)
        } else {
            previous_indent
        };
        previous_indent = indent_size;
        let Some(mut expression_doc) = expression_docs.next() else {
            return bail("template expressions");
        };
        let Some(expression) = expressions.get(index) else {
            return bail("template expressions");
        };
        let next_start = quasis.get(index + 1).map_or(quasi.end(), |q| q.start());
        let mut has_newline = crate::text::has_newline_in_range(text, quasi.end(), next_start);
        if !has_newline {
            let rendered = render_flat(p, &expression_doc, p.opts.end_of_line.as_str())?;
            if rendered.contains('\n') {
                has_newline = true;
            } else {
                expression_doc = s(rendered);
            }
        }
        if has_newline
            && (p.has_any_comment(expression)
                || expression.is("Identifier")
                || is_member_expression(expression)
                || expression.is("ConditionalExpression")
                || expression.is("SequenceExpression")
                || matches!(
                    expression.ty(),
                    "TSAsExpression"
                        | "TSSatisfiesExpression"
                        | "AsExpression"
                        | "AsConstExpression"
                        | "SatisfiesExpression"
                )
                || is_binaryish(expression))
        {
            expression_doc = concat(vec![
                indent(concat(vec![softline(), expression_doc])),
                softline(),
            ]);
        }
        let aligned = if indent_size == Some(0) && raw.ends_with('\n') {
            dedent_to_root(expression_doc)
        } else {
            add_alignment_to_doc(expression_doc, indent_size, tab_width)?
        };
        parts.push(group(concat(vec![
            s("${"),
            aligned,
            line_suffix_boundary(),
            s("}"),
        ])));
    }
    parts.push(s("`"));
    Ok(concat(parts))
}

pub fn print_tagged_template_literal(p: &mut Printer<'_>) -> R {
    let quasi = p.print("quasi")?;
    let node = p.node();
    let mut space = empty();
    if let Some(q) = node.field("quasi")
        && let Some(first) = p
            .comments
            .get_comments(q, crate::comments::Flags::LEADING, None)
            .first()
    {
        let c = p.comments.get(first.index);
        let after = node
            .field("typeArguments")
            .or_else(|| node.field("typeParameters"))
            .or_else(|| node.field("tag"))
            .map_or(0, |n| n.end());
        space = if crate::text::has_newline_in_range(p.text, after, c.start) {
            softline()
        } else {
            s(" ")
        };
    }
    let tag = p.print("tag")?;
    let type_args = if node.field("typeArguments").is_some() {
        p.print("typeArguments")?
    } else {
        p.print("typeParameters")?
    };
    Ok(concat(vec![
        tag,
        type_args,
        space,
        line_suffix_boundary(),
        quasi,
    ]))
}

fn print_jest_each(p: &mut Printer<'_>) -> R<Option<Doc>> {
    let node = p.node();
    let quasis = node.list("quasis");
    let header = quasis.first().map_or("", raw).trim();
    let header_names: Vec<String> = split_bars(header);
    if !(header_names.len() > 1 || header_names.iter().any(|h| !h.is_empty())) {
        return Ok(None);
    }
    p.in_jest_each = true;
    let expressions = p.print_all("expressions");
    p.in_jest_each = false;
    let mut stringified = Vec::new();
    for doc in expressions? {
        stringified.push(format!("${{{}}}", render_flat(p, &doc, "\n")?));
    }
    struct Row {
        has_line_break: bool,
        cells: Vec<String>,
    }
    let mut body = vec![Row {
        has_line_break: false,
        cells: Vec::new(),
    }];
    for (i, quasi) in quasis.iter().enumerate().skip(1) {
        let cell = stringified.get(i - 1).cloned().unwrap_or_default();
        if let Some(row) = body.last_mut() {
            if cell.contains('\n') {
                row.has_line_break = true;
            }
            row.cells.push(cell);
        }
        if raw(quasi).contains('\n') {
            body.push(Row {
                has_line_break: false,
                cells: Vec::new(),
            });
        }
    }
    let mut table = vec![Row {
        has_line_break: false,
        cells: header_names,
    }];
    table.extend(body.into_iter().filter(|r| !r.cells.is_empty()));
    let max_columns = table.iter().map(|r| r.cells.len()).max().unwrap_or(0);
    let mut widths = vec![0f64; max_columns];
    for row in table.iter().filter(|r| !r.has_line_break) {
        for (i, cell) in row.cells.iter().enumerate() {
            widths[i] = widths[i].max(doc_printer_rs::port::string_width(cell));
        }
    }
    let rows: Vec<Doc> = table
        .iter()
        .map(|row| {
            join(
                &s(" | "),
                row.cells
                    .iter()
                    .enumerate()
                    .map(|(i, cell)| {
                        if row.has_line_break {
                            s(cell.clone())
                        } else {
                            let pad =
                                (widths[i] - doc_printer_rs::port::string_width(cell)) as usize;
                            s(format!("{cell}{}", " ".repeat(pad)))
                        }
                    })
                    .collect(),
            )
        })
        .collect();
    Ok(Some(concat(vec![
        line_suffix_boundary(),
        s("`"),
        indent(concat(vec![hardline(), join(&hardline(), rows)])),
        hardline(),
        s("`"),
    ])))
}

fn split_bars(header: &str) -> Vec<String> {
    header
        .split('|')
        .enumerate()
        .map(|(i, part)| {
            let part = if i == 0 {
                part
            } else {
                part.trim_start_matches(char::is_whitespace)
            };
            part.trim_end_matches(char::is_whitespace).to_string()
        })
        .collect()
}

fn is_angular_component_part(p: &Printer<'_>) -> bool {
    let tpl = |n: &Value, _: Option<&str>| n.is("TemplateLiteral");
    let named = |name: &'static str| {
        move |n: &Value, k: Option<&str>| {
            is_object_property(n)
                && !n.flag("computed")
                && n.field("key")
                    .is_some_and(|key| key.is("Identifier") && key.str_of("name") == Some(name))
                && k == Some("value")
        }
    };
    let object = |n: &Value, k: Option<&str>| n.is("ObjectExpression") && k == Some("properties");
    let component = |n: &Value, k: Option<&str>| {
        n.is("CallExpression")
            && n.field("callee")
                .is_some_and(|c| c.is("Identifier") && c.str_of("name") == Some("Component"))
            && k == Some("arguments")
    };
    let decorator = |n: &Value, k: Option<&str>| n.is("Decorator") && k == Some("expression");
    let array = |n: &Value, k: Option<&str>| n.is("ArrayExpression") && k == Some("elements");
    let template = named("template");
    let styles = named("styles");
    p.matches(&[
        Some(&tpl),
        Some(&template),
        Some(&object),
        Some(&component),
        Some(&decorator),
    ]) || p.matches(&[
        Some(&tpl),
        Some(&array),
        Some(&styles),
        Some(&object),
        Some(&component),
        Some(&decorator),
    ]) || p.matches(&[
        Some(&tpl),
        Some(&styles),
        Some(&object),
        Some(&component),
        Some(&decorator),
    ])
}
