//! `src/language-json/parser-json.js` and `printer-estree-json.js`.

use crate::comments::Comment;
use crate::doc::*;
use crate::node::*;
use crate::options::Parser;
use crate::printer::{R, bail};
use crate::value::{Map, Value};

const PREFIX: &str = "(\n";

fn shift(v: &mut Value, by: usize) {
    match v {
        Value::Array(items) => items.iter_mut().for_each(|x| shift(x, by)),
        Value::Object(map) => {
            for (k, x) in map.iter_mut() {
                match k {
                    "start" | "end" => {
                        if let Some(n) = x.as_u64() {
                            *x = Value::from(n.saturating_sub(by as u64));
                        }
                    }
                    _ => shift(x, by),
                }
            }
        }
        _ => {}
    }
}

fn assert_json_node(n: &Value) -> bool {
    match n.ty() {
        "ArrayExpression" => n
            .list("elements")
            .iter()
            .all(|e| e.is_null() || assert_json_node(e)),
        "ObjectExpression" => n.list("properties").iter().all(|p| {
            p.is("Property")
                && !p.flag("computed")
                && !p.flag("shorthand")
                && !p.flag("method")
                && p.str_of("kind") == Some("init")
                && p.field("key")
                    .is_some_and(|k| k.is("Identifier") || assert_json_node(k))
                && p.field("value").is_some_and(assert_json_node)
        }),
        "UnaryExpression" => {
            matches!(n.str_of("operator"), Some("+" | "-"))
                && n.field("argument").is_some_and(|a| {
                    is_numeric_literal(a)
                        || (a.is("Identifier")
                            && matches!(a.str_of("name"), Some("Infinity" | "NaN")))
                })
        }
        "Identifier" => matches!(n.str_of("name"), Some("Infinity" | "NaN" | "undefined")),
        "TemplateLiteral" => n.list("expressions").is_empty(),
        "Literal" => n.get("regex").is_none() && n.get("bigint").is_none(),
        _ => false,
    }
}

/// `parseJson`: the root `JsonRoot` node and the comments, positions in `text`.
pub fn parse(text: &str, parser: Parser) -> R<(Value, Vec<Comment>)> {
    if parser == Parser::Jsonc
        && let Some(only_comments) = parse_only_comments(text)
    {
        let mut root = Map::new();
        root.insert("type".into(), Value::from("JsonRoot"));
        root.insert("start".into(), Value::from(0usize));
        root.insert("end".into(), Value::from(text.len()));
        return Ok((Value::Object(root), only_comments));
    }
    let wrapped = format!("{PREFIX}{text}\n)");
    let alloc = oxc_allocator::Allocator::default();
    let source_type = oxc_span::SourceType::cjs();
    let ret = oxc_parser::Parser::new(&alloc, &wrapped, source_type)
        .with_options(oxc_parser::ParseOptions {
            preserve_parens: true,
            ..Default::default()
        })
        .parse();
    if ret.diagnostics.has_errors() || ret.diagnostics.has_warnings() {
        return bail("parse error");
    }
    if ret.program.hashbang.is_some() {
        return bail("hashbang");
    }
    let mut comments = crate::comments::from_oxc(&ret.program, &wrapped);
    for c in &mut comments {
        if c.start < PREFIX.len() || c.end > PREFIX.len() + text.len() {
            return bail("json comment outside the value");
        }
        c.start -= PREFIX.len();
        c.end -= PREFIX.len();
    }
    if parser == Parser::JsonStringify && !comments.is_empty() {
        return bail("comment in json-stringify");
    }
    let json = ret.program.to_estree_json(false, false);
    let mut program: Value = match crate::value::from_estree_json(&json) {
        Ok(v) => v,
        Err(e) => return bail(format!("estree json: {e}")),
    };
    crate::soundness::check_json_identifiers(&program)?;
    let body = match program.get_mut("body") {
        Some(Value::Array(body)) if body.len() == 1 => body,
        _ => return bail("json shape"),
    };
    let statement = body[0].take();
    if !statement.is("ExpressionStatement") {
        return bail("json shape");
    }
    let Some(paren) = statement
        .get("expression")
        .filter(|e| e.is("ParenthesizedExpression"))
    else {
        return bail("json shape");
    };
    let Some(mut expression) = paren.get("expression").cloned() else {
        return bail("json shape");
    };
    shift(&mut expression, PREFIX.len());
    if !assert_json_node(&expression) {
        return bail("not json");
    }
    let mut root = Map::new();
    root.insert("type".into(), Value::from("JsonRoot"));
    root.insert("node".into(), expression);
    root.insert("start".into(), Value::from(0usize));
    root.insert("end".into(), Value::from(text.len()));
    Ok((Value::Object(root), comments))
}

fn parse_only_comments(text: &str) -> Option<Vec<Comment>> {
    let alloc = oxc_allocator::Allocator::default();
    let ret = oxc_parser::Parser::new(&alloc, text, oxc_span::SourceType::cjs()).parse();
    if ret.diagnostics.has_errors()
        || ret.diagnostics.has_warnings()
        || !ret.program.body.is_empty()
        || !ret.program.directives.is_empty()
        || ret.program.hashbang.is_some()
        || ret.program.comments.is_empty()
    {
        return None;
    }
    Some(crate::comments::from_oxc(&ret.program, text))
}

fn js_string(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 2);
    out.push('"');
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\u{8}' => out.push_str("\\b"),
            '\u{c}' => out.push_str("\\f"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if (c as u32) < 0x20 => out.push_str(&format!("\\u{:04x}", c as u32)),
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

fn js_number(v: &Value) -> String {
    match v.as_f64() {
        Some(f) if f.is_finite() => doc_printer_rs::js::number_to_string(f),
        _ => "null".to_string(),
    }
}

/// `printer-estree-json.js`, for the `json-stringify` parser.
pub fn print_stringify(root: &Value) -> R {
    let node = root.get("node").unwrap_or(root);
    Ok(concat(vec![print_node(node, false)?, hardline()]))
}

fn print_node(node: &Value, is_key: bool) -> R {
    match node.ty() {
        "ArrayExpression" => {
            let elements = node.list("elements");
            if elements.is_empty() {
                return Ok(s("[]"));
            }
            let mut printed = Vec::new();
            for e in elements {
                printed.push(if e.is_null() {
                    s("null")
                } else {
                    print_node(e, false)?
                });
            }
            Ok(concat(vec![
                s("["),
                indent(concat(vec![
                    hardline(),
                    join(&concat(vec![s(","), hardline()]), printed),
                ])),
                hardline(),
                s("]"),
            ]))
        }
        "ObjectExpression" => {
            let props = node.list("properties");
            if props.is_empty() {
                return Ok(s("{}"));
            }
            let mut printed = Vec::new();
            for p in props {
                let key = p.get("key").unwrap_or(p);
                let value = p.get("value").unwrap_or(p);
                printed.push(concat(vec![
                    print_node(key, true)?,
                    s(": "),
                    print_node(value, false)?,
                ]));
            }
            Ok(concat(vec![
                s("{"),
                indent(concat(vec![
                    hardline(),
                    join(&concat(vec![s(","), hardline()]), printed),
                ])),
                hardline(),
                s("}"),
            ]))
        }
        "UnaryExpression" => {
            let op = node.str_of("operator").unwrap_or("");
            Ok(concat(vec![
                s(if op == "+" { "" } else { "-" }),
                print_node(node.get("argument").unwrap_or(node), false)?,
            ]))
        }
        "Identifier" => {
            let name = node.str_of("name").unwrap_or("");
            Ok(s(if is_key {
                js_string(name)
            } else {
                name.to_string()
            }))
        }
        "TemplateLiteral" => {
            let cooked = node
                .list("quasis")
                .first()
                .and_then(|q| q.get("value"))
                .and_then(|v| v.str_of("cooked"));
            match cooked {
                Some(c) => Ok(s(js_string(c))),
                None => bail("template without cooked value"),
            }
        }
        "Literal" => {
            let value = node.get("value").unwrap_or(node);
            Ok(s(match value {
                Value::Null => "null".to_string(),
                Value::Bool(b) => b.to_string(),
                Value::String(text) => js_string(text),
                // `JSON.stringify(String(node.value))`: `String(Infinity)` is "Infinity".
                Value::Number(_) if is_key => js_string(
                    &value
                        .as_f64()
                        .map_or_else(|| "null".to_string(), doc_printer_rs::js::number_to_string),
                ),
                Value::Number(_) => js_number(value),
                _ => return bail("json literal"),
            }))
        }
        other => bail(format!("json node {other}")),
    }
}
