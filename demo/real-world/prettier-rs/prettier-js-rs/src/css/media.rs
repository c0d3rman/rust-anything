//! postcss-media-query-parser 0.2.3 (`dist/parsers.js`; MIT, Copyright (c) 2015 dryoma), with
//! Prettier's `addMissingType`.

#[derive(Clone, Debug)]
pub struct M {
    pub ty: String,
    pub value: String,
    pub before: String,
    pub nodes: Option<Vec<M>>,
}

fn m(ty: &str, value: &str) -> M {
    M {
        ty: ty.to_string(),
        value: value.to_string(),
        before: String::new(),
        nodes: None,
    }
}

fn js_ws(c: char) -> bool {
    crate::comments::is_js_whitespace(c)
}

fn lead_ws(s: &str) -> &str {
    &s[..s.len() - s.trim_start_matches(js_ws).len()]
}

fn trim(s: &str) -> &str {
    s.trim_matches(js_ws)
}

/// `None` where postcss-media-query-parser throws: a `}` that leaves no mode to read.
fn parse_feature(string: &str) -> Option<Vec<M>> {
    let chars: Vec<char> = string.chars().collect();
    let inner: Vec<char> =
        if chars.first() == Some(&'(') && chars.last() == Some(&')') && chars.len() >= 2 {
            chars[1..chars.len() - 1].to_vec()
        } else {
            chars
        };
    let mut modes: Vec<(&str, Option<char>, bool)> = vec![("normal", None, false)];
    let mut feature = String::new();
    let mut result = Vec::new();
    let mut colon_value: Option<String> = None;
    for (i, &ch) in inner.iter().enumerate() {
        if ch == '\'' || ch == '"' {
            let top = *modes.last().unwrap_or(&("normal", None, false));
            if top.2 {
                modes.push(("string", Some(ch), false));
            } else if top.0 == "string" && top.1 == Some(ch) && (i == 0 || inner[i - 1] != '\\') {
                modes.pop();
            }
        }
        if ch == '{' {
            modes.push(("interpolation", None, true));
        } else if ch == '}' {
            modes.pop();
            modes.last()?;
        }
        if modes.last().is_some_and(|t| t.0 == "normal") && ch == ':' {
            colon_value = Some(inner[i + 1..].iter().collect());
            break;
        }
        feature.push(ch);
    }
    result.push(m("media-feature", trim(&feature)));
    if let Some(v) = colon_value {
        result.push(m("colon", ":"));
        result.push(m("value", trim(&v)));
    }
    Some(result)
}

fn is_ws(c: char) -> bool {
    js_ws(c)
}

fn parse_query(string: &str) -> Option<Vec<M>> {
    let chars: Vec<char> = string.chars().collect();
    let mut result: Vec<M> = Vec::new();
    let mut level = 0i64;
    let mut inside = false;
    let mut node = m("", "");
    let mut has_type = false;
    for i in 0..chars.len() {
        let ch = chars[i];
        if !inside {
            if is_ws(ch) {
                node.before.push(ch);
            } else {
                if ch == '(' {
                    node.ty = "media-feature-expression".to_string();
                    has_type = true;
                    level += 1;
                }
                node.value = ch.to_string();
                inside = true;
            }
        } else {
            node.value.push(ch);
            if ch == '{' || ch == '(' {
                level += 1;
            }
            if ch == ')' || ch == '}' {
                level -= 1;
            }
        }
        if inside
            && level == 0
            && (ch == ')' || i == chars.len() - 1 || chars.get(i + 1).is_some_and(|c| is_ws(*c)))
        {
            if ["not", "only", "and"].contains(&node.value.as_str()) {
                node.ty = "keyword".to_string();
                has_type = true;
            }
            if node.ty == "media-feature-expression" {
                node.nodes = Some(parse_feature(&node.value)?);
            }
            if !has_type {
                node.ty.clear();
            }
            result.push(std::mem::replace(&mut node, m("", "")));
            has_type = false;
            inside = false;
        }
    }
    let n = result.len();
    for i in 0..n {
        if !result[i].ty.is_empty() {
            continue;
        }
        if i > 0 {
            if result[i - 1].ty == "media-feature-expression" {
                result[i].ty = "keyword".to_string();
                continue;
            }
            if result[i - 1].value == "not" || result[i - 1].value == "only" {
                result[i].ty = "media-type".to_string();
                continue;
            }
            if result[i - 1].value == "and" {
                result[i].ty = "media-feature-expression".to_string();
                continue;
            }
            if result[i - 1].ty == "media-type" {
                result[i].ty = if i + 1 >= n {
                    "media-feature-expression".to_string()
                } else if result[i + 1].ty == "media-feature-expression" {
                    "keyword".to_string()
                } else {
                    "media-feature-expression".to_string()
                };
            }
        }
        if i == 0 {
            if i + 1 >= n {
                result[i].ty = "media-type".to_string();
                continue;
            }
            if result[i + 1].ty == "media-feature-expression" || result[i + 1].ty == "keyword" {
                result[i].ty = "media-type".to_string();
                continue;
            }
            if i + 2 < n {
                if result[i + 2].ty == "media-feature-expression" {
                    result[i].ty = "media-type".to_string();
                    result[i + 1].ty = "keyword".to_string();
                    continue;
                }
                if result[i + 2].ty == "keyword" {
                    result[i].ty = "keyword".to_string();
                    result[i + 1].ty = "media-type".to_string();
                    continue;
                }
            }
            if i + 3 < n && result[i + 3].ty == "media-feature-expression" {
                result[i].ty = "keyword".to_string();
                result[i + 1].ty = "media-type".to_string();
                result[i + 2].ty = "keyword".to_string();
                continue;
            }
        }
    }
    Some(result)
}

/// `parseMedia(params)` with Prettier's missing types and `media-` prefixes. `Ok(None)` where
/// the parser throws (Prettier keeps the params as `selector-unknown`); `Err` where it never
/// returns (an unclosed `url(`).
pub fn parse(string: &str) -> Result<Option<M>, ()> {
    let mut result: Vec<M> = Vec::new();
    let mut interim = 0usize;
    let mut level = 0i64;
    let lw = lead_ws(string);
    let after_ws = &string[lw.len()..];
    if after_ws.starts_with("url") && after_ws[3..].trim_start_matches(js_ws).starts_with('(') {
        let open = string.find('(').ok_or(())?;
        let mut i = open + 1;
        let b = string.as_bytes();
        let mut lv = 1;
        while lv > 0 {
            let ch = *b.get(i).ok_or(())?;
            if ch == b'(' {
                lv += 1;
            }
            if ch == b')' {
                lv -= 1;
            }
            i += 1;
        }
        result.push(m("url", trim(&string[..i])));
        interim = i;
    }
    let b = string.as_bytes();
    let mut queries = Vec::new();
    let scan_start = interim;
    for i in scan_start..b.len() {
        match b[i] {
            b'(' => level += 1,
            b')' => level -= 1,
            b',' if level == 0 => {
                queries.push(&string[interim..i]);
                interim = i + 1;
            }
            _ => {}
        }
    }
    queries.push(&string[interim..]);
    for q in queries {
        let Some(nodes) = parse_query(q) else {
            return Ok(None);
        };
        let mut node = m("media-query", trim(q));
        node.nodes = Some(nodes);
        result.push(node);
    }
    let mut list = m("media-query-list", trim(string));
    list.nodes = Some(result);
    Ok(Some(fix_types(list)))
}

fn fix_types(mut n: M) -> M {
    if let Some(nodes) = n.nodes.take() {
        n.nodes = Some(nodes.into_iter().map(fix_types).collect());
    }
    if n.ty.is_empty() && !n.value.is_empty() {
        n.ty = "unknown".to_string();
    }
    if !n.ty.is_empty() && !n.ty.starts_with("media-") {
        n.ty = format!("media-{}", n.ty);
    }
    n
}
