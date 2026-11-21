//! `src/language-js/print/jsx.js`.

use crate::value::Value;

use crate::comments::Flags;
use crate::doc::*;
use crate::node::*;
use crate::printer::{Args, Printer, R, bail};
use crate::utils::*;

fn is_jsx_ws(c: char) -> bool {
    matches!(c, ' ' | '\n' | '\r' | '\t')
}

fn raw(n: &Value) -> &str {
    n.get("extra")
        .and_then(|e| e.str_of("raw"))
        .or_else(|| n.str_of("raw"))
        .unwrap_or("")
}

pub fn is_meaningful_jsx_text(n: &Value) -> bool {
    n.is("JSXText") && {
        let r = raw(n);
        r.chars().any(|c| !is_jsx_ws(c)) || !r.contains('\n')
    }
}

fn is_jsx_whitespace_expression(p: &Printer<'_>, n: &Value) -> bool {
    n.is("JSXExpressionContainer")
        && n.field("expression").is_some_and(|e| {
            is_string_literal(e) && e.str_of("value") == Some(" ") && !p.has_any_comment(e)
        })
}

fn is_empty_jsx_element(n: &Value) -> bool {
    let children = n.list("children");
    match children.len() {
        0 => true,
        1 => children[0].is("JSXText") && !is_meaningful_jsx_text(&children[0]),
        _ => false,
    }
}

fn split_with_whitespace(text: &str) -> Vec<String> {
    let mut out = vec![String::new()];
    let mut in_ws = false;
    for c in text.chars() {
        let ws = is_jsx_ws(c);
        if ws != in_ws {
            out.push(String::new());
            in_ws = ws;
        }
        if let Some(last) = out.last_mut() {
            last.push(c);
        }
    }
    if in_ws {
        out.push(String::new());
    }
    out
}

fn utf16_len(s: &str) -> usize {
    s.encode_utf16().count()
}

/// A child of `printJsxChildren`, keeping the identity of the line docs JS compares with `===`.
#[derive(Clone)]
enum C {
    Str(String),
    Hard,
    Soft,
    Line,
    Ws,
    Doc(Doc),
}

impl C {
    fn is_empty_str(&self) -> bool {
        matches!(self, C::Str(s) if s.is_empty())
    }
}

struct Ctx {
    raw_ws: &'static str,
}

impl Ctx {
    fn doc(&self, c: &C) -> Doc {
        match c {
            C::Str(t) => s(t.clone()),
            C::Hard => hardline(),
            C::Soft => softline(),
            C::Line => line(),
            C::Ws => if_break(concat(vec![s(self.raw_ws), softline()]), s(" ")),
            C::Doc(d) => d.clone(),
        }
    }
}

fn is_empty_doc(d: &Doc) -> bool {
    match d {
        Doc::Str(t) => t.is_empty(),
        Doc::Array(parts) | Doc::Fill(parts) => parts.iter().all(is_empty_doc),
        Doc::Shared(inner) => is_empty_doc(inner),
        Doc::Group {
            contents, expanded, ..
        } => match expanded {
            Some(states) => states.iter().all(is_empty_doc),
            None => is_empty_doc(contents),
        },
        Doc::IfBreak { brk, flat, .. } => is_empty_doc(brk) && is_empty_doc(flat),
        Doc::Indent(c)
        | Doc::Align(_, c)
        | Doc::IndentIfBreak { contents: c, .. }
        | Doc::LineSuffix(c)
        | Doc::Label(_, c) => is_empty_doc(c),
        Doc::Cursor => true,
        Doc::Trim | Doc::LineSuffixBoundary | Doc::Line { .. } | Doc::BreakParent => false,
        Doc::Invalid(..) => false,
    }
}

fn separator_no_whitespace(fbt: bool, child: &str, node: &Value, next: Option<&Value>) -> C {
    if fbt {
        return C::Str(String::new());
    }
    let unclosed = |n: &Value| n.is("JSXElement") && n.field("closingElement").is_none();
    if unclosed(node) || next.is_some_and(unclosed) {
        return if utf16_len(child) == 1 {
            C::Soft
        } else {
            C::Hard
        };
    }
    C::Soft
}

fn separator_with_whitespace(fbt: bool, child: &str, node: &Value, next: Option<&Value>) -> C {
    if fbt {
        return C::Hard;
    }
    let unclosed = |n: &Value| n.is("JSXElement") && n.field("closingElement").is_none();
    if utf16_len(child) == 1 {
        return if unclosed(node) || next.is_some_and(unclosed) {
            C::Hard
        } else {
            C::Soft
        };
    }
    C::Hard
}

fn print_jsx_children(p: &mut Printer<'_>, cx: &Ctx, fbt: bool) -> R<Vec<C>> {
    let node = p.node();
    let children = node.list("children");
    let mut parts: Vec<C> = vec![C::Str(String::new())];
    let mut prev: String = String::new();
    fn push(parts: &mut Vec<C>, cx: &Ctx, d: C) {
        let last = parts.pop().unwrap_or(C::Str(String::new()));
        parts.push(C::Doc(concat(vec![cx.doc(&last), cx.doc(&d)])));
    }
    fn push_line(parts: &mut Vec<C>, d: C) {
        if d.is_empty_str() {
            return;
        }
        parts.push(d);
        parts.push(C::Str(String::new()));
    }
    for (i, child) in children.iter().enumerate() {
        let next = children.get(i + 1);
        let as_text = if is_jsx_whitespace_expression(p, child) {
            Some(" ".to_string())
        } else if child.is("JSXText") {
            Some(raw(child).to_string())
        } else {
            None
        };
        if let Some(text) = as_text {
            let is_meaningful = text.chars().any(|c| !is_jsx_ws(c)) || !text.contains('\n');
            if is_meaningful {
                let mut words = split_with_whitespace(&text);
                if words.first().is_some_and(String::is_empty) {
                    words.remove(0);
                    if words.first().is_some_and(|w| w.contains('\n')) {
                        let w1 = words.get(1).cloned().unwrap_or_default();
                        push_line(&mut parts, separator_with_whitespace(fbt, &w1, child, next));
                    } else {
                        push_line(&mut parts, C::Ws);
                    }
                    if !words.is_empty() {
                        words.remove(0);
                    }
                }
                let mut end_ws = None;
                if words.last().is_some_and(String::is_empty) {
                    words.pop();
                    end_ws = words.pop();
                }
                if words.is_empty() {
                    continue;
                }
                for (j, word) in words.iter().enumerate() {
                    if j % 2 == 1 {
                        push_line(&mut parts, C::Line);
                    } else {
                        prev = word.clone();
                        push(&mut parts, cx, C::Str(word.clone()));
                    }
                }
                match end_ws {
                    Some(ws) if ws.contains('\n') => push_line(
                        &mut parts,
                        separator_with_whitespace(fbt, &prev, child, next),
                    ),
                    Some(_) => push_line(&mut parts, C::Ws),
                    None => push_line(&mut parts, separator_no_whitespace(fbt, &prev, child, next)),
                }
            } else if text.contains('\n') {
                if text.matches('\n').count() > 1 {
                    push_line(&mut parts, C::Hard);
                }
            } else {
                push_line(&mut parts, C::Ws);
            }
        } else {
            let printed = p.print_index("children", i)?;
            prev = String::new();
            push(&mut parts, cx, C::Doc(printed));
            if let Some(n) =
                next.filter(|n| is_meaningful_jsx_text(n) || is_jsx_whitespace_expression(p, n))
            {
                let next_raw = if n.is("JSXText") { raw(n) } else { " " };
                let trimmed = next_raw.trim_matches(is_jsx_ws);
                let first_word = trimmed.split(is_jsx_ws).next().unwrap_or("");
                push_line(
                    &mut parts,
                    separator_no_whitespace(fbt, first_word, child, Some(n)),
                );
            } else {
                push_line(&mut parts, C::Hard);
            }
        }
    }
    Ok(parts)
}

fn print_jsx_element_internal(p: &mut Printer<'_>) -> R {
    let node = p.node();
    if node.is("JSXElement") && is_empty_jsx_element(node) {
        let opening = p.print("openingElement")?;
        let closing = p.print("closingElement")?;
        return Ok(concat(vec![opening, closing]));
    }
    let (open_key, close_key) = if node.is("JSXElement") {
        ("openingElement", "closingElement")
    } else {
        ("openingFragment", "closingFragment")
    };
    let opening = shared(p.print(open_key)?);
    let closing = shared(p.print(close_key)?);
    let children = node.list("children");
    if children.len() == 1
        && children[0].is("JSXExpressionContainer")
        && children[0]
            .field("expression")
            .is_some_and(|e| e.is("TemplateLiteral") || e.is("TaggedTemplateExpression"))
    {
        let mut parts = vec![opening];
        parts.extend(p.print_all("children")?);
        parts.push(closing);
        return Ok(concat(parts));
    }
    let contains_tag = children.iter().any(is_jsx_element);
    let contains_multiple_expressions = children
        .iter()
        .filter(|c| c.is("JSXExpressionContainer") && !is_jsx_whitespace_expression(p, c))
        .count()
        > 1;
    let contains_multiple_attributes = node.is("JSXElement")
        && node
            .field("openingElement")
            .is_some_and(|o| o.list("attributes").len() > 1);
    let mut forced_break = will_break(&opening)
        || contains_tag
        || contains_multiple_attributes
        || contains_multiple_expressions;
    let raw_ws = if p.opts.single_quote {
        "{' '}"
    } else {
        "{\" \"}"
    };
    let cx = Ctx { raw_ws };
    let fbt = node
        .field("openingElement")
        .and_then(|o| o.field("name"))
        .and_then(|n| n.str_of("name"))
        == Some("fbt");
    let mut children_c = print_jsx_children(p, &cx, fbt)?;
    let contains_text = children
        .iter()
        .any(|c| is_meaningful_jsx_text(c) || is_jsx_whitespace_expression(p, c));

    let mut i = children_c.len() as isize - 2;
    while i >= 0 {
        let u = i as usize;
        let at = |k: usize| children_c.get(k);
        let is = |k: usize, f: &dyn Fn(&C) -> bool| at(k).is_some_and(f);
        let empty = |c: &C| c.is_empty_str();
        let hard = |c: &C| matches!(c, C::Hard);
        let soft = |c: &C| matches!(c, C::Soft);
        let ws = |c: &C| matches!(c, C::Ws);
        let pair_of_empty = is(u, &empty) && is(u + 1, &empty);
        let pair_of_hard = is(u, &hard) && is(u + 1, &empty) && is(u + 2, &hard);
        let line_then_ws = (is(u, &soft) || is(u, &hard)) && is(u + 1, &empty) && is(u + 2, &ws);
        let ws_then_line =
            is(u, &ws) && is(u + 1, &empty) && (is(u + 2, &soft) || is(u + 2, &hard));
        let double_ws = is(u, &ws) && is(u + 1, &empty) && is(u + 2, &ws);
        let hard_soft = (is(u, &soft) && is(u + 1, &empty) && is(u + 2, &hard))
            || (is(u, &hard) && is(u + 1, &empty) && is(u + 2, &soft));
        if (pair_of_hard && contains_text)
            || pair_of_empty
            || line_then_ws
            || double_ws
            || hard_soft
        {
            children_c.drain(u..u + 2);
        } else if ws_then_line {
            children_c.drain(u + 1..u + 3);
        }
        i -= 1;
    }
    let is_empty_or_line = |c: &C| c.is_empty_str() || matches!(c, C::Line | C::Hard | C::Soft);
    while children_c.last().is_some_and(is_empty_or_line) {
        children_c.pop();
    }
    while children_c.len() > 1
        && is_empty_or_line(&children_c[0])
        && is_empty_or_line(&children_c[1])
    {
        children_c.drain(0..2);
    }

    let mut multiline: Vec<Doc> = vec![empty()];
    let n = children_c.len();
    for (i, child) in children_c.iter().enumerate() {
        if matches!(child, C::Ws) {
            if i == 1 && is_empty_doc(&cx.doc(&children_c[0])) {
                if n == 2 {
                    let last = multiline.pop().unwrap_or_else(empty);
                    multiline.push(concat(vec![last, s(raw_ws)]));
                    continue;
                }
                multiline.push(concat(vec![s(raw_ws), hardline()]));
                multiline.push(empty());
                continue;
            } else if i == n - 1
                || (i >= 2
                    && children_c[i - 1].is_empty_str()
                    && matches!(children_c[i - 2], C::Hard))
            {
                let last = multiline.pop().unwrap_or_else(empty);
                multiline.push(concat(vec![last, s(raw_ws)]));
                continue;
            }
        }
        let d = cx.doc(child);
        if i % 2 == 0 {
            let last = multiline.pop().unwrap_or_else(empty);
            multiline.push(concat(vec![last, d.clone()]));
        } else {
            multiline.push(d.clone());
            multiline.push(empty());
        }
        if will_break(&d) {
            forced_break = true;
        }
    }
    let content = if contains_text {
        fill(multiline)
    } else {
        group_with(concat(multiline), true, None)
    };
    let multi_line_elem = group(concat(vec![
        opening.clone(),
        indent(concat(vec![hardline(), content])),
        hardline(),
        closing.clone(),
    ]));
    if forced_break {
        return Ok(multi_line_elem);
    }
    let mut flat = vec![opening];
    flat.extend(children_c.iter().map(|c| cx.doc(c)));
    flat.push(closing);
    Ok(conditional_group(
        vec![group(concat(flat)), multi_line_elem],
        false,
    ))
}

fn should_break_jsx_element(p: &Printer<'_>) -> bool {
    p.matches(&[
        None,
        Some(&|n, _| n.is("ArrowFunctionExpression")),
        Some(&|n, _| is_call_expression(n)),
    ]) && (p.matches(&[
        None,
        None,
        None,
        Some(&|n, _| n.is("JSXExpressionContainer")),
    ]) || p.matches(&[
        None,
        None,
        None,
        Some(&|n, _| n.is("ChainExpression")),
        Some(&|n, _| n.is("JSXExpressionContainer")),
    ]))
}

fn print_jsx_element(p: &mut Printer<'_>) -> R {
    let node = p.node();
    let inner = print_jsx_element_internal(p)?;
    let elem = crate::comments::print_comments(&p.comments, node, inner, p.text);
    let parent = p.parent();
    if parent.is_some_and(|pa| {
        matches!(
            pa.ty(),
            "ArrayExpression"
                | "JSXAttribute"
                | "JSXElement"
                | "JSXExpressionContainer"
                | "JSXFragment"
                | "ExpressionStatement"
                | "CallExpression"
                | "OptionalCallExpression"
                | "ConditionalExpression"
                | "JsExpressionRoot"
        )
    }) {
        return Ok(elem);
    }
    let should_break = should_break_jsx_element(p);
    let needs_parens = crate::parens::needs_parens(p)?;
    Ok(group_with(
        concat(vec![
            if needs_parens {
                empty()
            } else {
                if_break(s("("), empty())
            },
            indent(concat(vec![softline(), elem])),
            softline(),
            if needs_parens {
                empty()
            } else {
                if_break(s(")"), empty())
            },
        ]),
        should_break,
        None,
    ))
}

fn print_jsx_attribute(p: &mut Printer<'_>) -> R {
    let node = p.node();
    let mut parts = vec![p.print("name")?];
    if let Some(value) = node.field("value") {
        let res = if is_string_literal(value) {
            let r = raw(value);
            let inner = &r[1..r.len() - 1];
            let final_text = inner.replace("&apos;", "'").replace("&quot;", "\"");
            let quote = get_preferred_quote(&final_text, p.opts.jsx_single_quote);
            let final_text = if quote == '"' {
                final_text.replace('"', "&quot;")
            } else {
                final_text.replace('\'', "&apos;")
            };
            crate::comments::print_comments(
                &p.comments,
                value,
                replace_end_of_line(&format!("{quote}{final_text}{quote}")),
                p.text,
            )
        } else {
            p.print("value")?
        };
        parts.push(s("="));
        parts.push(res);
    }
    Ok(concat(parts))
}

fn should_inline_container_expression(p: &Printer<'_>, n: &Value, parent: Option<&Value>) -> bool {
    n.is("JSXEmptyExpression")
        || (!p.has_any_comment(n)
            && (n.is("ArrayExpression")
                || n.is("ObjectExpression")
                || n.is("ArrowFunctionExpression")
                || (n.is("AwaitExpression")
                    && n.field("argument").is_some_and(|a| {
                        should_inline_container_expression(p, a, Some(n)) || a.is("JSXElement")
                    }))
                || is_call_expression(n)
                || (n.is("ChainExpression")
                    && n.field("expression").is_some_and(is_call_expression))
                || n.is("FunctionExpression")
                || n.is("TemplateLiteral")
                || n.is("TaggedTemplateExpression")
                || n.is("DoExpression")
                || (parent.is_some_and(is_jsx_element)
                    && (n.is("ConditionalExpression") || is_binaryish(n)))))
}

fn print_jsx_expression_container(p: &mut Printer<'_>) -> R {
    let node = p.node();
    let expr = node.field("expression").unwrap_or(node);
    let inline = should_inline_container_expression(p, expr, p.parent());
    let printed = p.print("expression")?;
    Ok(if inline {
        group(concat(vec![
            s("{"),
            printed,
            line_suffix_boundary(),
            s("}"),
        ]))
    } else {
        group(concat(vec![
            s("{"),
            indent(concat(vec![softline(), printed])),
            softline(),
            line_suffix_boundary(),
            s("}"),
        ]))
    })
}

fn print_jsx_opening_element(p: &mut Printer<'_>) -> R {
    let node = p.node();
    let type_args_key = if node.field("typeArguments").is_some() {
        "typeArguments"
    } else {
        "typeParameters"
    };
    let name_has_comments = ["name", "typeParameters", "typeArguments"]
        .iter()
        .any(|k| node.field(k).is_some_and(|n| p.has_any_comment(n)));
    let attrs = node.list("attributes");
    let self_closing = node.flag("selfClosing");
    if self_closing && attrs.is_empty() && !name_has_comments {
        return Ok(concat(vec![
            s("<"),
            p.print("name")?,
            p.print(type_args_key)?,
            s(" />"),
        ]));
    }
    if attrs.len() == 1
        && attrs[0].field("value").is_some_and(|v| {
            is_string_literal(v) && !v.str_of("value").unwrap_or("").contains('\n')
        })
        && !name_has_comments
        && !p.has_any_comment(&attrs[0])
    {
        let name = p.print("name")?;
        let mut parts = vec![s("<"), name, p.print(type_args_key)?, s(" ")];
        parts.extend(p.print_all("attributes")?);
        parts.push(s(if self_closing { " />" } else { ">" }));
        return Ok(group(concat(parts)));
    }
    let should_break = attrs.iter().any(|a| {
        a.field("value")
            .is_some_and(|v| is_string_literal(v) && v.str_of("value").unwrap_or("").contains('\n'))
    });
    let name = p.print("name")?;
    let type_args = p.print(type_args_key)?;
    let printed_attrs = p.map("attributes", |p, _| {
        Ok(concat(vec![line(), p.print_current(Args::default())?]))
    })?;
    let end = if self_closing {
        concat(vec![line(), s("/>")])
    } else {
        let last_attr_trailing = attrs
            .last()
            .is_some_and(|a| p.has_comment(a, Flags::TRAILING));
        let same_line = (attrs.is_empty() && !name_has_comments)
            || (p.opts.bracket_same_line
                && (!name_has_comments || !attrs.is_empty())
                && !last_attr_trailing);
        if same_line {
            s(">")
        } else {
            concat(vec![softline(), s(">")])
        }
    };
    Ok(group_with(
        concat(vec![
            s("<"),
            name,
            type_args,
            indent(concat(printed_attrs)),
            end,
        ]),
        should_break,
        None,
    ))
}

fn print_jsx_closing_element(p: &mut Printer<'_>) -> R {
    let node = p.node();
    let name_node = node.field("name");
    let printed = p.print("name")?;
    let mut parts = vec![s("</")];
    if name_node.is_some_and(|n| p.has_comment(n, Flags::LEADING.with(Flags::LINE))) {
        parts.push(indent(concat(vec![hardline(), printed])));
        parts.push(hardline());
    } else if name_node.is_some_and(|n| p.has_comment(n, Flags::LEADING.with(Flags::BLOCK))) {
        parts.push(s(" "));
        parts.push(printed);
    } else {
        parts.push(printed);
    }
    parts.push(s(">"));
    Ok(concat(parts))
}

fn print_jsx_fragment_tag(p: &mut Printer<'_>) -> R {
    let node = p.node();
    let has = p.has_any_comment(node);
    let own_line = p.has_comment(node, Flags::LINE);
    let opening = node.is("JSXOpeningFragment");
    Ok(concat(vec![
        s(if opening { "<" } else { "</" }),
        indent(concat(vec![
            if own_line {
                hardline()
            } else if has && !opening {
                s(" ")
            } else {
                empty()
            },
            p.print_dangling(node, false, None),
        ])),
        if own_line { hardline() } else { empty() },
        s(">"),
    ]))
}

fn print_jsx_empty_expression(p: &mut Printer<'_>) -> R {
    let node = p.node();
    let requires_hardline = p.has_comment(node, Flags::LINE);
    Ok(concat(vec![
        p.print_dangling(node, requires_hardline, None),
        if requires_hardline {
            hardline()
        } else {
            empty()
        },
    ]))
}

fn print_jsx_spread(p: &mut Printer<'_>) -> R {
    let node = p.node();
    let key = if node.is("JSXSpreadAttribute") {
        "argument"
    } else {
        "expression"
    };
    let inner = p
        .call(key, |p| {
            let printed = concat(vec![s("..."), p.print_current(Args::default())?]);
            let n = p.node();
            if !p.has_any_comment(n) || !p.will_print_own_comments() {
                return Ok(printed);
            }
            Ok(concat(vec![
                indent(concat(vec![
                    softline(),
                    crate::comments::print_comments(&p.comments, n, printed, p.text),
                ])),
                softline(),
            ]))
        })
        .unwrap_or_else(|| bail("spread"))?;
    Ok(concat(vec![s("{"), inner, s("}")]))
}

pub fn print_jsx(p: &mut Printer<'_>) -> R {
    let node = p.node();
    match node.ty() {
        "JSXAttribute" => print_jsx_attribute(p),
        "JSXIdentifier" => Ok(s(node.str_of("name").unwrap_or("").to_string())),
        "JSXNamespacedName" => Ok(join(&s(":"), vec![p.print("namespace")?, p.print("name")?])),
        "JSXMemberExpression" => Ok(join(
            &s("."),
            vec![p.print("object")?, p.print("property")?],
        )),
        "JSXSpreadAttribute" | "JSXSpreadChild" => print_jsx_spread(p),
        "JSXExpressionContainer" => print_jsx_expression_container(p),
        // A JSX element continued on the next line is refused in `soundness.rs`.
        "JSXFragment" | "JSXElement" => print_jsx_element(p),
        "JSXOpeningElement" => print_jsx_opening_element(p),
        "JSXClosingElement" => print_jsx_closing_element(p),
        "JSXOpeningFragment" | "JSXClosingFragment" => print_jsx_fragment_tag(p),
        "JSXEmptyExpression" => print_jsx_empty_expression(p),
        other => bail(format!("node {other}")),
    }
}
