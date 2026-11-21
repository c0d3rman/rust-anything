//! `src/language-css/`: parser-postcss.js (`parseNestedCSS`), loc.js and printer-postcss.js with
//! its print/ and utils/ helpers, for the `css` parser.

mod media;
mod postcss;
mod selector;
mod u16;
mod units;
mod value;

use media::M;
use postcss::Kind;
use selector::{Ns, Tree};
use u16::{Idx, len16};
use value::V;

use crate::comments::is_js_whitespace;
use crate::doc::*;
use crate::options::{Options, Parser, TrailingComma};
use crate::printer::{R, bail};
use crate::text::{LINE_TERMINATORS, has_newline, is_next_line_empty};

fn trim(s: &str) -> &str {
    s.trim_matches(is_js_whitespace)
}

enum Sel {
    Parsed(Tree),
    Unknown(String),
}

enum Val {
    Rule(Vec<N>),
    Raw(String),
    Root(Box<V>),
    Unknown(String),
}

enum Params {
    Raw(String),
    Media(Box<M>),
    MediaUnknown(String),
    /// `parseMediaQuery`'s fallback when the parser throws.
    SelectorUnknown(String),
    Root(Box<V>),
    Unknown(String),
}

struct N {
    kind: Kind,
    start: i64,
    end: Option<i64>,
    raws: postcss::Raws,
    raws_selector: Option<String>,
    raws_params: Option<String>,
    selector: Option<Sel>,
    prop: String,
    name: String,
    value: Option<Val>,
    params: Option<Params>,
    important: bool,
    scss_default: bool,
    raws_scss_default: Option<String>,
    scss_global: bool,
    raws_scss_global: Option<String>,
    custom_selector: Option<String>,
    nested_prop: bool,
    mixin: bool,
    function: bool,
    variable: bool,
    extend: bool,
    text: String,
    nodes: Option<Vec<N>>,
}

fn parse_selector(s: &str) -> R<Sel> {
    if s.contains("//") || s.contains("/*") {
        return Ok(Sel::Unknown(trim(s).to_string()));
    }
    match selector::parse(s) {
        Ok(Some(t)) => Ok(Sel::Parsed(t)),
        Ok(None) => Ok(Sel::Unknown(s.to_string())),
        Err(()) => bail("postcss-selector-parser never returns"),
    }
}

fn parse_val(s: &str, root_offset: i64, p: Parser) -> R<Option<V>> {
    if p == Parser::Less && s.starts_with("~`") {
        return Ok(None);
    }
    value::parse_value(s, root_offset, p == Parser::Scss).or_else(|_| bail("css value"))
}

/// `value.match(/(\s*)(!default).*$/u)`: `.` stops at line terminators and `$` is the end, so
/// the first occurrence with no line break after it.
fn directive(value: &str, word: &str) -> Option<(usize, String)> {
    let i = value
        .match_indices(word)
        .map(|(i, _)| i)
        .find(|&i| !value[i + word.len()..].contains(LINE_TERMINATORS))?;
    let ws_start = value[..i].len() - value[..i].trim_end_matches(is_js_whitespace).len();
    let start = i - ws_start;
    Some((start, value[start..].to_string()))
}

/// The index of the text a postcss tree was parsed from, and the original text Prettier slices
/// (`options.originalText`); they differ under front matter and in custom-property blocks.
#[derive(Clone, Copy)]
struct Src<'t> {
    idx: &'t Idx,
    orig: &'t str,
    oidx: &'t Idx,
}

fn process(n: postcss::Node, src: Src, p: Parser) -> R<N> {
    let start = src.idx.b2u(n.start);
    let end = n.end.map(|e| src.idx.b2u(e));
    let mut children = Vec::new();
    if let Some(nodes) = n.nodes.clone() {
        for c in nodes {
            children.push(process(c, src, p)?);
        }
    }
    let mut out = N {
        kind: n.kind,
        start,
        end,
        raws: n.raws.clone(),
        raws_selector: None,
        raws_params: None,
        selector: None,
        prop: n.prop.clone(),
        name: n.name.clone(),
        value: None,
        params: None,
        important: n.important,
        scss_default: false,
        raws_scss_default: None,
        scss_global: false,
        raws_scss_global: None,
        custom_selector: None,
        nested_prop: false,
        mixin: n.mixin,
        function: n.function,
        variable: n.variable,
        extend: p == Parser::Less
            && n.kind == Kind::Decl
            && n.value.as_deref().is_some_and(has_extend_call),
        text: n.text.clone(),
        nodes: n.nodes.as_ref().map(|_| children),
    };
    if out.end.is_none()
        && let Some(last) = out.nodes.as_ref().and_then(|v| v.last())
    {
        out.end = last.end;
    }
    if n.kind == Kind::Decl
        && n.prop.starts_with("--")
        && n.value.as_deref().is_some_and(|v| v.starts_with('{'))
    {
        let v = n.value.as_deref().unwrap_or("");
        let mut rules = None;
        if v.trim_end_matches(is_js_whitespace).ends_with('}') {
            let prop_len = len16(&n.prop);
            let (Some(before), Some(body)) = (
                src.oidx.slice(src.orig, 0, Some(start)),
                src.oidx.slice(src.orig, start + prop_len, end),
            ) else {
                return bail("custom property splits a surrogate pair");
            };
            let before: String = before
                .chars()
                .map(|c| if c == '\n' { '\n' } else { ' ' })
                .collect();
            let fake = format!("{before}{}{body}", "a".repeat(prop_len as usize));
            if let Some(ast) = parse_root(&fake, p)
                && let Some(nodes) = &ast.nodes
                && nodes.len() == 1
                && nodes[0].kind == Kind::Rule
            {
                let fidx = Idx::new(&fake);
                let fsrc = Src { idx: &fidx, ..src };
                let mut kids = Vec::new();
                for c in nodes[0].nodes.clone().unwrap_or_default() {
                    kids.push(process(c, fsrc, p)?);
                }
                rules = Some(kids);
            }
        }
        out.value = Some(match rules {
            Some(r) => Val::Rule(r),
            None => match &n.raws.value {
                Some((raw, _)) => Val::Unknown(raw.clone()),
                None => return bail("custom property raw"),
            },
        });
        return Ok(out);
    }
    let mut selector = String::new();
    if let Some(sel) = &n.selector {
        selector = match &n.raws.selector {
            Some((raw, _)) => raw.clone(),
            None => sel.clone(),
        };
        if !trim(&n.raws.between).is_empty() {
            selector.push_str(&n.raws.between);
        }
        out.raws_selector = Some(selector.clone());
    }
    let mut value = String::new();
    if let Some(v) = &n.value {
        value = match &n.raws.value {
            Some((raw, _)) => raw.clone(),
            None => v.clone(),
        };
        out.value = Some(Val::Raw(v.clone()));
        out.raws.value = None;
    }
    let mut params = String::new();
    if let Some(p) = &n.params {
        params = match &n.raws.params {
            Some((raw, _)) => raw.clone(),
            None => p.clone(),
        };
        if !trim(&n.raws.after_name).is_empty() {
            params = format!("{}{params}", n.raws.after_name);
        }
        if !trim(&n.raws.between).is_empty() {
            params.push_str(&n.raws.between);
        }
        params = trim(&params).to_string();
        out.raws_params = Some(params.clone());
        out.params = Some(Params::Raw(p.clone()));
    }
    if !trim(&selector).is_empty() {
        if selector.starts_with('@') && selector.ends_with(':') {
            return Ok(out);
        }
        if p == Parser::Scss && is_nested_prop(n.selector.as_deref().unwrap_or("")) {
            out.nested_prop = true;
        }
        out.selector = Some(parse_selector(&selector)?);
        return Ok(out);
    }
    let value_offset = || {
        let mut r = start;
        if out_kind_is_decl(n.kind) {
            r += len16(&n.prop) + len16(&n.raws.between);
        }
        if n.kind == Kind::AtRule {
            r = atrule_offset(start, &n.name, &n.raws.after_name);
        }
        r
    };
    if !trim(&value).is_empty() {
        if let Some((i, m)) = directive(&value, "!default") {
            value.truncate(i);
            out.scss_default = true;
            if trim(&m) != "!default" {
                out.raws_scss_default = Some(m);
            }
        }
        if let Some((i, m)) = directive(&value, "!global") {
            value.truncate(i);
            out.scss_global = true;
            if trim(&m) != "!global" {
                out.raws_scss_global = Some(m);
            }
        }
        if value.starts_with("progid:") {
            return Ok(out);
        }
        out.value = Some(match parse_val(&value, value_offset(), p)? {
            Some(v) => Val::Root(Box::new(v)),
            None => Val::Unknown(value.clone()),
        });
    }
    if p == Parser::Less && n.kind == Kind::Decl && value.starts_with("extend(") {
        out.extend = out.extend || n.raws.between == ":";
        if out.extend && out.selector.is_none() {
            out.value = None;
            let mut inner = value["extend(".len()..].to_string();
            inner.pop();
            out.selector = Some(parse_selector(&inner)?);
        }
    }
    if p == Parser::Less && n.kind == Kind::AtRule {
        if n.mixin {
            let source = format!("{}{}{}{}", n.identifier, n.name, n.raws.after_name, params);
            out.selector = Some(parse_selector(&source)?);
            out.params = None;
            return Ok(out);
        }
        if n.function {
            return Ok(out);
        }
    }
    if p == Parser::Css && n.kind == Kind::AtRule && n.name == "custom-selector" {
        let p = n.params.clone().unwrap_or_default();
        // `params.match(/:--\S+\s+/)[0].trim()`: the first `:--` with a name and whitespace.
        let found = p.match_indices(":--").find_map(|(i, _)| {
            let rest = &p[i..];
            let name_end = rest.find(is_js_whitespace).unwrap_or(rest.len());
            let ws_end = rest[name_end..]
                .find(|c: char| !is_js_whitespace(c))
                .map_or(rest.len(), |x| x + name_end);
            (name_end > 3 && ws_end > name_end).then_some((rest, ws_end))
        });
        let Some((rest, ws_end)) = found else {
            return bail("custom-selector");
        };
        let custom = trim(&rest[..ws_end]).to_string();
        out.custom_selector = Some(custom.clone());
        // `node.params.slice(customSelector.length)`: from the start, not from the match.
        let Some(rest) = Idx::new(&p).slice(&p, len16(&custom), None) else {
            return bail("custom-selector splits a surrogate pair");
        };
        out.selector = Some(parse_selector(trim(rest))?);
        out.params = None;
        return Ok(out);
    }
    if p == Parser::Less && n.kind == Kind::AtRule {
        let raw_params = n.params.clone().unwrap_or_default();
        if out.name.contains(':') && raw_params.is_empty() {
            out.variable = true;
            let full = out.name.clone();
            let (head, rest) = full.split_once(':').unwrap_or((&full, ""));
            out.name = head.to_string();
            let off = atrule_offset(start, &out.name, &out.raws.after_name);
            out.value = Some(match parse_val(rest, off, p)? {
                Some(v) => Val::Root(Box::new(v)),
                None => Val::Unknown(rest.to_string()),
            });
        }
        if !["page", "nest", "keyframes"].contains(&out.name.as_str())
            && let Some(text) = raw_params.strip_prefix(':')
        {
            out.variable = true;
            if !text.is_empty() {
                let mut after = out.raws.after_name.clone();
                after.push(':');
                let off = atrule_offset(start, &out.name, &after);
                out.value = Some(match parse_val(text, off, p)? {
                    Some(v) => Val::Root(Box::new(v)),
                    None => Val::Unknown(text.to_string()),
                });
            }
            out.raws.after_name.push(':');
        }
        if out.variable {
            out.params = None;
            if matches!(&out.value, Some(Val::Raw(v)) if v.is_empty()) {
                out.value = None;
            }
            return Ok(out);
        }
    }
    if n.kind == Kind::AtRule && !params.is_empty() {
        let name = n.name.as_str();
        let lower = name.to_lowercase();
        let atrule_offset = atrule_offset(start, &n.name, &n.raws.after_name);
        if name == "warn" || name == "error" {
            out.params = Some(Params::MediaUnknown(params));
            return Ok(out);
        }
        if name == "extend" || name == "nest" {
            out.selector = Some(parse_selector(&params)?);
            out.params = None;
            return Ok(out);
        }
        if name == "at-root" {
            if at_root_with(&params) {
                out.params = Some(match parse_val(&params, atrule_offset, p)? {
                    Some(v) => Params::Root(Box::new(v)),
                    None => Params::Unknown(params),
                });
            } else {
                out.selector = Some(parse_selector(&params)?);
                out.params = None;
            }
            return Ok(out);
        }
        if matches!(lower.as_str(), "import" | "use" | "forward") {
            out.params = Some(match parse_val(&params, atrule_offset, p)? {
                Some(v) => Params::Root(Box::new(v)),
                None => Params::Unknown(params),
            });
            return Ok(out);
        }
        if [
            "namespace",
            "supports",
            "if",
            "else",
            "for",
            "each",
            "while",
            "debug",
            "mixin",
            "include",
            "function",
            "return",
            "define-mixin",
            "add-mixin",
        ]
        .contains(&name)
        {
            let params = scss_params(&params);
            out.value = Some(match parse_val(&params, atrule_offset, p)? {
                Some(v) => Val::Root(Box::new(v)),
                None => Val::Unknown(params),
            });
            out.params = None;
            return Ok(out);
        }
        if lower == "media" || lower == "custom-media" {
            if params.contains("#{") {
                return Ok(out);
            }
            out.params = Some(match media::parse(&params) {
                Ok(Some(m)) => Params::Media(Box::new(m)),
                Ok(None) => Params::SelectorUnknown(params),
                Err(()) => return bail("postcss-media-query-parser never returns"),
            });
            return Ok(out);
        }
        out.params = Some(Params::Raw(params));
        return Ok(out);
    }
    Ok(out)
}

fn parse_root(text: &str, p: Parser) -> Option<postcss::Node> {
    if p == Parser::Less {
        let masked = mask_inline_comments(text);
        let root = postcss::Parser::parse(&masked, false, true)?;
        return Some(root);
    }
    postcss::Parser::parse(text, p == Parser::Scss, false)
}

fn mask_inline_comments(text: &str) -> String {
    #[derive(Clone, Copy, PartialEq)]
    enum St {
        Init,
        Single,
        Double,
        Url,
        Block,
        Inline,
    }
    let chars: Vec<(usize, char)> = text.char_indices().collect();
    let mut state = St::Init;
    let mut back = St::Init;
    let mut start = 0;
    let mut has = false;
    let mut spans: Vec<(usize, usize)> = Vec::new();
    let prev = |i: usize| if i > 0 { Some(chars[i - 1].1) } else { None };
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i].1;
        match state {
            St::Init => {
                if c == '\'' {
                    state = St::Single;
                } else if c == '"' {
                    state = St::Double;
                } else if (c == 'u' || c == 'U')
                    && text[chars[i].0..].len() >= 4
                    && text[chars[i].0..].is_char_boundary(4)
                    && text[chars[i].0..chars[i].0 + 4].eq_ignore_ascii_case("url(")
                {
                    state = St::Url;
                    i += 3;
                } else if c == '*' && prev(i) == Some('/') {
                    state = St::Block;
                } else if c == '/' && prev(i) == Some('/') {
                    state = St::Inline;
                    start = i - 1;
                }
            }
            St::Single | St::Double => {
                let q = if state == St::Single { '\'' } else { '"' };
                if c == q && prev(i) != Some('\\') {
                    state = back;
                    back = St::Init;
                }
                if c == '\n' || c == '\r' {
                    return text.to_string();
                }
            }
            St::Url => {
                if c == ')' {
                    state = St::Init;
                }
                if c == '\n' || c == '\r' {
                    return text.to_string();
                }
                if c == '\'' {
                    state = St::Single;
                    back = St::Url;
                } else if c == '"' {
                    state = St::Double;
                    back = St::Url;
                }
            }
            St::Block => {
                if c == '/' && prev(i) == Some('*') {
                    state = St::Init;
                }
            }
            St::Inline => {
                if c == '"' || c == '\'' || c == '*' {
                    has = true;
                }
                if c == '\n' || c == '\r' {
                    if has {
                        spans.push((start, i));
                    }
                    state = St::Init;
                    has = false;
                }
            }
        }
        i += 1;
    }
    let mut out: Vec<char> = chars.iter().map(|x| x.1).collect();
    for (a, b) in spans {
        for ch in &mut out[a..b] {
            if matches!(*ch, '"' | '\'' | '*') {
                *ch = ' ';
            }
        }
    }
    out.into_iter().collect()
}

fn has_extend_call(v: &str) -> bool {
    let lower = v.to_ascii_lowercase();
    let mut from = 0;
    while let Some(i) = lower[from..].find("extend(") {
        let rest = &v[from + i + 7..];
        let line_end = rest.find(LINE_TERMINATORS).unwrap_or(rest.len());
        if rest[..line_end]
            .char_indices()
            .skip(1)
            .any(|(_, c)| c == ')')
        {
            return true;
        }
        from += i + 1;
    }
    false
}

/// `getValueRootOffset` for an at-rule: `1 + name.length + afterName.match(/^\s*:?\s*/)`.
fn atrule_offset(start: i64, name: &str, after_name: &str) -> i64 {
    let rest = after_name.trim_start_matches(is_js_whitespace);
    let rest = rest.strip_prefix(':').unwrap_or(rest);
    let rest = rest.trim_start_matches(is_js_whitespace);
    // Every character `\s` matches is one UTF-16 unit.
    let ws = after_name[..after_name.len() - rest.len()].chars().count() as i64;
    start + 1 + len16(name) + ws
}

/// `isSCSSNestedPropertyNode`: `node.selector` (postcss's, without `raws.between`) with the
/// first `/\/\*.*?\*\//u` and `/\/\/.*\n/u` match removed, ending in `:`.
fn is_nested_prop(selector: &str) -> bool {
    let mut s = selector.to_string();
    let block = s.match_indices("/*").find_map(|(i, _)| {
        let rest = &s[i + 2..];
        let j = rest.find("*/")?;
        (!rest[..j].contains(LINE_TERMINATORS)).then_some((i, i + 2 + j + 2))
    });
    if let Some((a, b)) = block {
        s.replace_range(a..b, "");
    }
    let line = s.match_indices("//").find_map(|(i, _)| {
        let rest = &s[i + 2..];
        let j = rest.find(LINE_TERMINATORS)?;
        rest[j..].starts_with('\n').then_some((i, i + 2 + j + 1))
    });
    if let Some((a, b)) = line {
        s.replace_range(a..b, "");
    }
    trim(&s).ends_with(':')
}

fn at_root_with(params: &str) -> bool {
    let Some(inner) = params.strip_prefix('(').and_then(|x| x.strip_suffix(')')) else {
        return false;
    };
    let rest = inner.trim_start_matches(is_js_whitespace);
    let rest = if let Some(r) = rest.strip_prefix("without") {
        r
    } else if let Some(r) = rest.strip_prefix("with") {
        r
    } else {
        return false;
    };
    let rest = rest.trim_start_matches(is_js_whitespace);
    rest.strip_prefix(':').is_some_and(|r| !r.is_empty())
}

fn out_kind_is_decl(k: Kind) -> bool {
    k == Kind::Decl
}

fn scss_params(params: &str) -> String {
    let mut p = params.to_string();
    if let Some((w_end, ws_end)) = rest_args(&p) {
        p = format!(
            "{}...{}{}",
            &p[..w_end],
            &p[w_end..ws_end],
            &p[ws_end + 3..]
        );
    }
    if !p.starts_with("if") {
        let first_end = p.find(is_js_whitespace).unwrap_or(p.len());
        let after = &p[first_end..];
        let ws = after.len() - after.trim_start_matches(is_js_whitespace).len();
        if first_end > 0 && ws > 0 && after[ws..].starts_with('(') {
            p = format!("{}({}{}", &p[..first_end], &after[..ws], &after[ws + 1..]);
        }
    }
    p
}

fn rest_args(p: &str) -> Option<(usize, usize)> {
    for (at, ch) in p.char_indices() {
        if ch != '$' {
            continue;
        }
        let rest = &p[at + 1..];
        for (k, c) in rest.char_indices() {
            if is_js_whitespace(c) {
                break;
            }
            let w_end = at + 1 + k + c.len_utf8();
            let after = &p[w_end..];
            let ws = after.len() - after.trim_start_matches(is_js_whitespace).len();
            if after[ws..].starts_with("...") {
                return Some((w_end, w_end + ws));
            }
        }
    }
    None
}

fn maybe_lower(v: &str) -> String {
    if v.contains('$')
        || v.contains('@')
        || v.contains('#')
        || v.starts_with('%')
        || v.starts_with("--")
        || v.starts_with(":--")
        || (v.contains('(') && v.contains(')'))
    {
        v.to_string()
    } else {
        v.to_lowercase()
    }
}

fn last_line_has_inline_comment(text: &str) -> bool {
    text.split(['\n', '\r'])
        .next_back()
        .is_some_and(|l| l.contains("//"))
}

fn units() -> &'static std::collections::HashMap<&'static str, &'static str> {
    static U: std::sync::OnceLock<std::collections::HashMap<&'static str, &'static str>> =
        std::sync::OnceLock::new();
    U.get_or_init(|| units::UNITS.iter().copied().collect())
}

fn print_unit(unit: &str) -> String {
    let lower = unit.to_lowercase();
    match units().get(lower.as_str()) {
        Some(u) => (*u).to_string(),
        None => unit.to_string(),
    }
}

fn print_css_number(raw: &str) -> String {
    let n = crate::utils::print_number(raw);
    match n.find(".0") {
        Some(i) if n[i + 2..].is_empty() || n[i + 2..].starts_with('e') => {
            format!("{}{}", &n[..i], &n[i + 2..])
        }
        _ => n,
    }
}

fn str_regex() -> Option<&'static regex::Regex> {
    static R: std::sync::OnceLock<Option<regex::Regex>> = std::sync::OnceLock::new();
    R.get_or_init(|| regex::Regex::new(r#"(?s)"(?:[^"\\]|\\.)*"|'(?:[^'\\]|\\.)*'"#).ok())
        .as_ref()
}

fn num_regex() -> Option<&'static regex::Regex> {
    static R: std::sync::OnceLock<Option<regex::Regex>> = std::sync::OnceLock::new();
    R.get_or_init(|| {
        regex::Regex::new(r#"(?is)("(?:[^"\\]|\\.)*"|'(?:[^'\\]|\\.)*')|([$@]?[_a-z\x{80}-\x{FFFF}][\w\x{80}-\x{FFFF}-]*)?((?:\d*\.\d+|\d+\.?)(?:e[+-]?\d+)?)([a-z]+)?"#).ok()
    })
    .as_ref()
}

fn adjust_strings(value: &str, opts: &Options) -> String {
    match str_regex() {
        Some(r) => r
            .replace_all(value, |c: &regex::Captures| {
                crate::utils::print_string(&c[0], opts)
            })
            .into_owned(),
        None => value.to_string(),
    }
}

fn adjust_numbers(value: &str) -> String {
    let Some(r) = num_regex() else {
        return value.to_string();
    };
    r.replace_all(value, |c: &regex::Captures| {
        if c.get(1).is_none()
            && c.get(2).is_none()
            && let Some(num) = c.get(3)
        {
            let unit = c
                .get(4)
                .map_or(String::new(), |u| u.as_str().to_lowercase());
            if unit.is_empty() || unit == "n" || units().contains_key(unit.as_str()) {
                return format!(
                    "{}{}",
                    print_css_number(num.as_str()),
                    if unit.is_empty() {
                        String::new()
                    } else {
                        print_unit(&unit)
                    }
                );
            }
        }
        c[0].to_string()
    })
    .into_owned()
}

fn quote_attribute_value(v: &str, opts: &Options) -> String {
    if v.contains('"') || v.contains('\'') {
        v.to_string()
    } else {
        let q = if opts.single_quote { "'" } else { "\"" };
        format!("{q}{v}{q}")
    }
}

fn is_wide_keyword(v: &str) -> bool {
    matches!(
        v.to_lowercase().as_str(),
        "initial" | "inherit" | "unset" | "revert"
    )
}

struct Pr<'a> {
    text: &'a str,
    idx: &'a Idx,
    opts: &'a Options,
}

#[derive(Clone, Copy)]
struct Top<'a> {
    decl: Option<&'a N>,
    atrule: Option<&'a N>,
    rule: Option<&'a N>,
    key_value: bool,
    variable: bool,
}

impl<'a> Pr<'a> {
    /// `options.originalText.slice(a, b)`.
    fn slice(&self, a: i64, b: Option<i64>) -> R<String> {
        match self.idx.slice(self.text, a, b) {
            Some(t) => Ok(t.to_string()),
            None => bail("slice splits a surrogate pair"),
        }
    }

    /// The byte offset of UTF-16 index `u` in `options.originalText`.
    fn byte(&self, u: i64) -> R<usize> {
        match self.idx.byte(u) {
            Some(b) => Ok(b),
            None => bail("index splits a surrogate pair"),
        }
    }

    fn sequence(&self, nodes: &'a [N], top: Top<'a>, parent: &'a N) -> R<Vec<Doc>> {
        let mut parts = Vec::new();
        for (i, n) in nodes.iter().enumerate() {
            let prev = if i > 0 { nodes.get(i - 1) } else { None };
            if prev.is_some_and(|p| p.kind == Kind::Comment && trim(&p.text) == "prettier-ignore") {
                parts.push(s(self.slice(n.start, n.end)?));
            } else {
                parts.push(self.print_css(n, top, parent, nodes, i)?);
            }
            let Some(next) = nodes.get(i + 1) else {
                continue;
            };
            if (next.kind == Kind::Comment && !has_newline(self.text, self.byte(next.start)?, true))
                || (next.kind == Kind::AtRule && next.name == "else" && n.kind != Kind::Comment)
            {
                parts.push(s(" "));
            } else {
                parts.push(hardline());
                if let Some(e) = n.end
                    && is_next_line_empty(self.text, self.byte(e)?)
                {
                    parts.push(hardline());
                }
            }
        }
        Ok(parts)
    }

    fn print_css(
        &self,
        n: &'a N,
        top: Top<'a>,
        parent: &'a N,
        siblings: &'a [N],
        index: usize,
    ) -> R {
        let _ = (siblings, index);
        Ok(match n.kind {
            Kind::Root => {
                let nodes = n.nodes.as_deref().unwrap_or(&[]);
                let seq = self.sequence(nodes, top, n)?;
                let mut after = trim(&n.raws.after).to_string();
                if let Some(rest) = after.strip_prefix(';') {
                    after = trim(rest).to_string();
                }
                concat(vec![
                    concat(seq),
                    if after.is_empty() {
                        empty()
                    } else {
                        s(format!(" {after}"))
                    },
                    if nodes.is_empty() {
                        empty()
                    } else {
                        hardline()
                    },
                ])
            }
            Kind::Comment => {
                let text = self.slice(n.start, n.end)?;
                if n.raws.inline {
                    s(text.trim_end_matches(is_js_whitespace).to_string())
                } else {
                    s(text)
                }
            }
            Kind::Rule => {
                let nodes = n.nodes.as_deref().unwrap_or(&[]);
                let sel = match &n.selector {
                    Some(sel) => self.print_selector_root(sel, n, top)?,
                    None => return bail("empty selector"),
                };
                let child_top = Top {
                    rule: Some(n),
                    ..top
                };
                let unknown_comment =
                    matches!(&n.selector, Some(Sel::Unknown(v)) if last_line_has_inline_comment(v));
                let detached = match &n.selector {
                    Some(Sel::Unknown(v)) => detached_ruleset(v),
                    _ => false,
                };
                concat(vec![
                    sel,
                    if n.important {
                        s(" !important")
                    } else {
                        empty()
                    },
                    if unknown_comment { line() } else { s(" ") },
                    s("{"),
                    if nodes.is_empty() {
                        empty()
                    } else {
                        indent(concat(vec![
                            hardline(),
                            concat(self.sequence(nodes, child_top, n)?),
                        ]))
                    },
                    hardline(),
                    s("}"),
                    if detached { s(";") } else { empty() },
                ])
            }
            Kind::Decl => {
                let between = trim(&n.raws.between);
                let is_colon = between == ":";
                let value_doc = match &n.value {
                    Some(Val::Rule(nodes)) => concat(vec![
                        s("{"),
                        if nodes.is_empty() {
                            empty()
                        } else {
                            indent(concat(vec![
                                hardline(),
                                concat(self.sequence(
                                    nodes,
                                    Top {
                                        decl: Some(n),
                                        ..top
                                    },
                                    n,
                                )?),
                            ]))
                        },
                        hardline(),
                        s("}"),
                    ]),
                    Some(Val::Raw(v)) => s(v.clone()),
                    Some(Val::Root(v)) => {
                        let t = Top {
                            decl: Some(n),
                            key_value: true,
                            variable: false,
                            ..top
                        };
                        self.print_value(v, &[], t)?
                    }
                    Some(Val::Unknown(v)) => s(v.clone()),
                    None => empty(),
                };
                let is_all_space =
                    matches!(&n.value, Some(Val::Raw(v)) if v.chars().all(|c| c == ' '));
                let composes = matches!(&n.value, Some(Val::Root(v)) if v.group.as_ref().is_some_and(|g| g.ty == "value") && n.prop.to_lowercase() == "composes");
                let mut value_doc = if composes {
                    remove_lines(&value_doc)
                } else {
                    value_doc
                };
                if !is_colon && last_line_has_inline_comment(between) {
                    let breaks = match &n.value {
                        Some(Val::Root(v)) => v
                            .group
                            .as_ref()
                            .and_then(|g| g.group.as_ref())
                            .is_some_and(|g| should_break_list_node(g, n)),
                        _ => false,
                    };
                    if !breaks {
                        value_doc = indent(concat(vec![hardline(), dedent(value_doc)]));
                    }
                }
                let icss = top
                    .rule
                    .and_then(|r| r.raws_selector.as_deref())
                    .is_some_and(|s| s.starts_with(":import") || s.starts_with(":export"));
                let before: String = n
                    .raws
                    .before
                    .chars()
                    .filter(|c| !(c.is_whitespace() || *c == ';'))
                    .collect();
                let important = match &n.raws.important {
                    Some(r) => replace_ci(r, "!", "important", " !important"),
                    None if n.important => " !important".to_string(),
                    None => String::new(),
                };
                let default = match &n.raws_scss_default {
                    Some(r) => replace_directive(r, "!default"),
                    None if n.scss_default => " !default".to_string(),
                    None => String::new(),
                };
                let global = match &n.raws_scss_global {
                    Some(r) => replace_directive(r, "!global"),
                    None if n.scss_global => " !global".to_string(),
                    None => String::new(),
                };
                let _ = parent;
                let less_var_parent = parent.kind == Kind::AtRule && parent.variable;
                let extend_doc = match &n.selector {
                    Some(sel) if self.opts.parser == Parser::Less && n.extend => concat(vec![
                        s("extend("),
                        self.print_selector_root(sel, n, top)?,
                        s(")"),
                    ]),
                    _ => empty(),
                };
                concat(vec![
                    s(before),
                    s(if icss || less_var_parent {
                        n.prop.clone()
                    } else {
                        maybe_lower(&n.prop)
                    }),
                    s(if between.starts_with("//") { " " } else { "" }),
                    s(between.to_string()),
                    s(if n.extend || is_all_space { "" } else { " " }),
                    extend_doc,
                    value_doc,
                    s(important),
                    s(default),
                    s(global),
                    match &n.nodes {
                        Some(nodes) => concat(vec![
                            s(" {"),
                            indent(concat(vec![
                                softline(),
                                concat(self.sequence(
                                    nodes,
                                    Top {
                                        decl: Some(n),
                                        ..top
                                    },
                                    n,
                                )?),
                            ])),
                            softline(),
                            s("}"),
                        ]),
                        None => s(";"),
                    },
                ])
            }
            Kind::AtRule => {
                let child_top = Top {
                    atrule: Some(n),
                    ..top
                };
                if self.opts.parser == Parser::Less {
                    if n.mixin {
                        let sel = match &n.selector {
                            Some(sel) => self.print_selector_root(sel, n, child_top)?,
                            None => empty(),
                        };
                        return Ok(concat(vec![
                            sel,
                            s(if n.important { " !important" } else { "" }),
                            s(";"),
                        ]));
                    }
                    if n.function {
                        let params = match &n.params {
                            Some(Params::Raw(p)) => p.clone(),
                            _ => return bail("less function params"),
                        };
                        return Ok(concat(vec![s(n.name.clone()), s(params), s(";")]));
                    }
                    if n.variable {
                        let value = match &n.value {
                            Some(Val::Root(v)) => self.print_value(
                                v,
                                &[],
                                Top {
                                    key_value: true,
                                    variable: true,
                                    ..child_top
                                },
                            )?,
                            Some(Val::Unknown(v)) | Some(Val::Raw(v)) => s(v.clone()),
                            Some(Val::Rule(_)) => return bail("less variable rule value"),
                            None => empty(),
                        };
                        let between = trim(&n.raws.between);
                        let nodes = match &n.nodes {
                            Some(nodes) => concat(vec![
                                s("{"),
                                indent(concat(vec![
                                    if nodes.is_empty() {
                                        empty()
                                    } else {
                                        softline()
                                    },
                                    concat(self.sequence(nodes, child_top, n)?),
                                ])),
                                softline(),
                                s("}"),
                            ]),
                            None => empty(),
                        };
                        return Ok(concat(vec![
                            s("@"),
                            s(n.name.clone()),
                            s(": "),
                            value,
                            s(if between.is_empty() {
                                String::new()
                            } else {
                                format!("{between} ")
                            }),
                            nodes,
                            s(";"),
                        ]));
                    }
                }
                let detached_call = n.raws_params.as_deref().is_some_and(|p| {
                    let inner = p.strip_prefix('(').and_then(|x| x.strip_suffix(')'));
                    inner.is_some_and(|i| i.chars().all(is_js_whitespace))
                });
                let import_unknown_semi = n.name == "import"
                    && matches!(&n.params, Some(Params::Unknown(v)) if v.ends_with(';'));
                let mut parts = vec![
                    s("@"),
                    s(if detached_call || n.name.ends_with(':') {
                        n.name.clone()
                    } else {
                        maybe_lower(&n.name)
                    }),
                ];
                let params_doc = match &n.params {
                    None => None,
                    Some(Params::Raw(p)) if p.is_empty() => None,
                    Some(Params::Raw(p)) => Some(s(p.clone())),
                    Some(Params::MediaUnknown(p)) | Some(Params::Unknown(p)) => Some(s(p.clone())),
                    Some(Params::SelectorUnknown(p)) if top.rule.is_some_and(|r| r.nested_prop) => {
                        Some(s(adjust_numbers(&adjust_strings(
                            &maybe_lower(p),
                            self.opts,
                        ))))
                    }
                    Some(Params::SelectorUnknown(p)) => Some(s(p.clone())),
                    Some(Params::Media(m)) => Some(self.print_media(m, false)?),
                    Some(Params::Root(v)) => Some(self.print_value(
                        v,
                        &[],
                        Top {
                            key_value: false,
                            ..child_top
                        },
                    )?),
                };
                if let Some(pd) = params_doc {
                    parts.push(if detached_call { empty() } else { s(" ") });
                    parts.push(pd);
                }
                if let Some(sel) = &n.selector {
                    parts.push(indent(concat(vec![
                        s(" "),
                        self.print_selector_root(sel, n, child_top)?,
                    ])));
                }
                let control = self.is_control(n);
                let control_tail = |v: Option<&V>| {
                    if !control {
                        empty()
                    } else if v
                        .and_then(|r| r.group.as_deref())
                        .and_then(|g| g.group.as_deref())
                        .is_some_and(|g| {
                            g.ty == "paren_group" && g.open.is_some() && g.close.is_some()
                        })
                    {
                        s(" ")
                    } else {
                        line()
                    }
                };
                match &n.value {
                    Some(Val::Root(v)) => {
                        parts.push(group(concat(vec![
                            s(" "),
                            self.print_value(
                                v,
                                &[],
                                Top {
                                    key_value: false,
                                    ..child_top
                                },
                            )?,
                            control_tail(Some(v)),
                        ])));
                    }
                    Some(Val::Unknown(v)) | Some(Val::Raw(v)) => parts.push(group(concat(vec![
                        s(" "),
                        s(v.clone()),
                        control_tail(None),
                    ]))),
                    Some(Val::Rule(_)) => return bail("atrule rule value"),
                    None if n.name == "else" => parts.push(s(" ")),
                    None => {}
                }
                match &n.nodes {
                    Some(nodes) => {
                        let comment_before = match (&n.selector, &n.params) {
                            (Some(Sel::Unknown(v)), _) => last_line_has_inline_comment(v),
                            (None, Some(Params::Raw(p))) => last_line_has_inline_comment(p),
                            _ => false,
                        };
                        parts.push(if control {
                            empty()
                        } else if comment_before {
                            line()
                        } else {
                            s(" ")
                        });
                        parts.push(s("{"));
                        parts.push(indent(concat(vec![
                            if nodes.is_empty() {
                                empty()
                            } else {
                                softline()
                            },
                            concat(self.sequence(nodes, child_top, n)?),
                        ])));
                        parts.push(softline());
                        parts.push(s("}"));
                    }
                    None => {
                        if !import_unknown_semi {
                            parts.push(s(";"));
                        }
                    }
                }
                concat(parts)
            }
        })
    }

    fn print_media(&self, m: &M, is_last: bool) -> R {
        Ok(match m.ty.as_str() {
            "media-query-list" => {
                let nodes = m.nodes.as_deref().unwrap_or(&[]);
                let mut parts = Vec::new();
                for (i, q) in nodes.iter().enumerate() {
                    if q.ty == "media-query" && q.value.is_empty() {
                        continue;
                    }
                    parts.push(self.print_media(q, i + 1 == nodes.len())?);
                }
                group(indent(join(&line(), parts)))
            }
            "media-query" => {
                let nodes = m.nodes.as_deref().unwrap_or(&[]);
                let mut parts = Vec::new();
                for (i, q) in nodes.iter().enumerate() {
                    parts.push(self.print_media(q, i + 1 == nodes.len())?);
                }
                concat(vec![
                    join(&s(" "), parts),
                    s(if is_last { "" } else { "," }),
                ])
            }
            "media-type" => s(adjust_numbers(&adjust_strings(&m.value, self.opts))),
            "media-feature-expression" => match &m.nodes {
                None => s(m.value.clone()),
                Some(nodes) => {
                    let mut parts = vec![s("(")];
                    for (i, q) in nodes.iter().enumerate() {
                        parts.push(self.print_media(q, i + 1 == nodes.len())?);
                    }
                    parts.push(s(")"));
                    concat(parts)
                }
            },
            "media-feature" => {
                let mut v = m.value.clone();
                while v.contains("  ") {
                    v = v.replace("  ", " ");
                }
                s(maybe_lower(&adjust_strings(&v, self.opts)))
            }
            "media-colon" => concat(vec![s(m.value.clone()), s(" ")]),
            "media-value" => s(adjust_numbers(&adjust_strings(&m.value, self.opts))),
            "media-keyword" => s(adjust_strings(&m.value, self.opts)),
            "media-url" => {
                let v = m.value.clone();
                let lower = v.to_lowercase();
                let v = if lower.starts_with("url(") {
                    let rest = &v[4..];
                    format!("url({}", rest.trim_start_matches(is_js_whitespace))
                } else {
                    v
                };
                let v = if v.ends_with(')') {
                    let body = &v[..v.len() - 1];
                    format!("{})", body.trim_end_matches(is_js_whitespace))
                } else {
                    v
                };
                s(adjust_strings(&v, self.opts))
            }
            "media-unknown" => s(m.value.clone()),
            _ => return bail(format!("media node {}", m.ty)),
        })
    }

    fn print_selector_root(&self, sel: &Sel, owner: &'a N, top: Top<'a>) -> R {
        match sel {
            Sel::Unknown(v) => {
                let rule = if owner.kind == Kind::Rule {
                    Some(owner)
                } else {
                    top.rule
                };
                if rule.is_some_and(|r| r.nested_prop) {
                    return Ok(s(adjust_numbers(&adjust_strings(
                        &maybe_lower(v),
                        self.opts,
                    ))));
                }
                if owner.raws_selector.is_some() {
                    let start = owner.start;
                    let end = start + owner.raws_selector.as_deref().map_or(0, len16);
                    return Ok(s(trim(&self.slice(start, Some(end))?).to_string()));
                }
                Ok(s(v.clone()))
            }
            Sel::Parsed(t) => self.print_sel(t, 0, top),
        }
    }

    fn is_control(&self, n: &N) -> bool {
        self.opts.parser == Parser::Scss
            && n.kind == Kind::AtRule
            && matches!(n.name.as_str(), "if" | "else" | "for" | "each" | "while")
    }

    fn atrule_name_in(&self, top: Top<'a>, names: &[&str]) -> bool {
        top.atrule
            .is_some_and(|a| names.contains(&a.name.to_lowercase().as_str()))
    }

    fn print_sel(&self, t: &Tree, id: usize, top: Top<'a>) -> R {
        let n = &t.n[id];
        let kids = n.nodes.clone().unwrap_or_default();
        let print_kids =
            |p: &Self| -> R<Vec<Doc>> { kids.iter().map(|&k| p.print_sel(t, k, top)).collect() };
        let ns = |n: &selector::S| match &n.namespace {
            Ns::None => empty(),
            Ns::True => s("|"),
            Ns::Str(x) => s(format!("{}|", trim(x))),
        };
        Ok(match n.ty {
            "root" => {
                let sep = if self.atrule_name_in(top, &["extend", "custom-selector", "nest"]) {
                    line()
                } else {
                    hardline()
                };
                let custom = if self.atrule_name_in(top, &["custom-selector"]) {
                    let cs = top
                        .atrule
                        .and_then(|a| a.custom_selector.clone())
                        .unwrap_or_default();
                    concat(vec![s(cs), line()])
                } else {
                    empty()
                };
                group(concat(vec![
                    custom,
                    join(&concat(vec![s(","), sep]), print_kids(self)?),
                ]))
            }
            "selector" => {
                let printed = concat(print_kids(self)?);
                group(if kids.len() > 2 {
                    indent(printed)
                } else {
                    printed
                })
            }
            "comment" => s(n.value.clone()),
            "string" => s(adjust_strings(&n.value, self.opts)),
            "tag" => {
                let parent = t.parent[id];
                let prev = parent.and_then(|p| {
                    let sib = t.n[p].nodes.as_ref()?;
                    let i = sib.iter().position(|&x| x == id)?;
                    if i == 0 { None } else { Some(sib[i - 1]) }
                });
                let after_nesting = prev.is_some_and(|p| t.n[p].ty == "nesting");
                let keyframes = top
                    .atrule
                    .is_some_and(|a| a.name.to_lowercase().ends_with("keyframes"))
                    && matches!(n.value.to_lowercase().as_str(), "from" | "to");
                let v = if after_nesting {
                    n.value.clone()
                } else {
                    adjust_numbers(&if keyframes {
                        n.value.to_lowercase()
                    } else {
                        n.value.clone()
                    })
                };
                concat(vec![ns(n), s(v)])
            }
            "id" => s(format!("#{}", n.value)),
            "class" => s(format!(
                ".{}",
                adjust_numbers(&adjust_strings(&n.value, self.opts))
            )),
            "attribute" => concat(vec![
                s("["),
                ns(n),
                s(trim(&n.attribute).to_string()),
                s(n.operator.clone().unwrap_or_default()),
                match &n.attr_value {
                    Some(v) if !v.is_empty() => s(quote_attribute_value(
                        &adjust_strings(trim(v), self.opts),
                        self.opts,
                    )),
                    _ => empty(),
                },
                s(if n.insensitive { " i" } else { "" }),
                s("]"),
            ]),
            "combinator" => {
                let parent = t.parent[id];
                let sib = parent
                    .and_then(|p| t.n[p].nodes.clone())
                    .unwrap_or_default();
                let is_last = sib.last() == Some(&id);
                if matches!(n.value.as_str(), "+" | ">" | "~" | ">>>") {
                    let first =
                        parent.is_some_and(|p| t.n[p].ty == "selector") && sib.first() == Some(&id);
                    return Ok(concat(vec![
                        if first { empty() } else { line() },
                        s(n.value.clone()),
                        s(if is_last { "" } else { " " }),
                    ]));
                }
                let tv = trim(&n.value);
                let leading = if tv.starts_with('(') { line() } else { empty() };
                let v = adjust_numbers(&adjust_strings(tv, self.opts));
                concat(vec![leading, if v.is_empty() { line() } else { s(v) }])
            }
            "universal" => concat(vec![ns(n), s(n.value.clone())]),
            "pseudo" => {
                let printed = print_kids(self)?;
                concat(vec![
                    s(maybe_lower(&n.value)),
                    if printed.is_empty() {
                        empty()
                    } else {
                        group(concat(vec![
                            s("("),
                            indent(concat(vec![
                                softline(),
                                join(&concat(vec![s(","), line()]), printed),
                            ])),
                            softline(),
                            s(")"),
                        ]))
                    },
                ])
            }
            "nesting" => s(n.value.clone()),
            other => return bail(format!("selector node {other}")),
        })
    }

    fn print_value(&self, v: &V, anc: &[&V], top: Top<'a>) -> R {
        let mut stack: Vec<&V> = anc.to_vec();
        stack.push(v);
        let child = |p: &Self, c: &V| p.print_value(c, &stack, top);
        Ok(match v.ty {
            "root" | "value" => match &v.group {
                Some(g) => child(self, g)?,
                None => empty(),
            },
            "comment" => s(self.slice(v.start, v.end)?),
            "comma_group" => self.print_comma_group(v, &stack, top)?,
            "paren_group" => self.print_paren_group(v, &stack, top)?,
            "func" => {
                let supports = self.atrule_name_in(top, &["supports"])
                    && matches!(v.value.to_lowercase().as_str(), "not" | "and" | "or");
                concat(vec![
                    s(v.value.clone()),
                    s(if supports { " " } else { "" }),
                    match &v.group {
                        Some(g) => child(self, g)?,
                        None => empty(),
                    },
                ])
            }
            "paren" | "operator" | "unicode-range" | "raw" => s(v.value.clone()),
            "number" => s(format!(
                "{}{}",
                print_css_number(&v.value),
                print_unit(&v.unit)
            )),
            "word" => {
                if (v.is_color && v.is_hex) || is_wide_keyword(&v.value) {
                    s(v.value.to_lowercase())
                } else {
                    s(v.value.clone())
                }
            }
            "colon" => {
                let prev = previous_sibling(anc, v);
                let no_line =
                    prev.is_some_and(|p| p.value.ends_with('\\')) || inside_func(anc, "url");
                group(concat(vec![
                    s(v.value.clone()),
                    if no_line { empty() } else { line() },
                ]))
            }
            "string" => s(crate::utils::print_string(
                &format!("{}{}{}", v.quote, v.value, v.quote),
                self.opts,
            )),
            "atword" => s(format!("@{}", v.value)),
            other => return bail(format!("value node {other}")),
        })
    }

    fn print_comma_group(&self, node: &V, stack: &[&V], top: Top<'a>) -> R {
        let anc = &stack[..stack.len() - 1];
        let parent = anc.last().copied();
        let grand = anc.len().checked_sub(2).map(|i| anc[i]);
        let prop = top.decl.map(|d| d.prop.to_lowercase());
        let is_grid = prop.as_ref().is_some_and(|p| {
            (p == "grid" || p.starts_with("grid-template"))
                && parent.is_some_and(|x| x.ty == "value")
        });
        let has_inline_comment = node.groups.iter().any(|g| g.ty == "comment" && g.inline);
        let is_control = top.atrule.is_some_and(|a| self.is_control(a));
        let scss = self.opts.parser == Parser::Scss;
        let scss_or_less = scss || self.opts.parser == Parser::Less;
        let mut printed = Vec::new();
        for g in &node.groups {
            printed.push(self.print_value(g, stack, top)?);
        }
        let in_url = inside_func(stack, "url");
        let in_calc = inside_func(stack, "calc");
        let mut parts: Vec<Doc> = vec![s("")];
        let mut did_break = false;
        let mut in_interp_string = false;
        let push_cat = |parts: &mut Vec<Doc>, d: Doc| {
            let prev = parts.pop().unwrap_or_else(|| s(""));
            parts.push(concat(vec![prev, d]));
        };
        let gs = &node.groups;
        for i in 0..gs.len() {
            let prev = if i > 0 { gs.get(i - 1) } else { None };
            let cur = &gs[i];
            let next = gs.get(i + 1);
            let next2 = gs.get(i + 2);
            if cur.ty == "comment" && cur.inline && next.is_none() {
                push_cat(
                    &mut parts,
                    line_suffix(concat(vec![s(" "), printed[i].clone()])),
                );
                continue;
            }
            push_cat(&mut parts, printed[i].clone());
            if in_url {
                if next.is_some_and(is_add) || is_add(cur) {
                    push_cat(&mut parts, s(" "));
                }
                continue;
            }
            if next.is_some_and(|n| n.ty == "colon")
                && cur.ty == "word"
                && parts.len() > 2
                && gs[..i].iter().all(|g| g.ty == "comment")
                && !prev.is_some_and(|p| p.ty == "comment" && p.inline)
            {
                let idx = parts.len() - 2;
                let d = std::mem::replace(&mut parts[idx], empty());
                parts[idx] = dedent(d);
            }
            if self.atrule_name_in(top, &["forward"])
                && cur.ty == "word"
                && !cur.value.is_empty()
                && prev.is_some_and(|p| p.ty == "word" && p.value == "as")
                && next.is_some_and(|n| n.ty == "operator" && n.value == "*")
            {
                continue;
            }
            if self.atrule_name_in(top, &["utility"])
                && cur.ty == "word"
                && next.is_some_and(|n| n.ty == "operator" && n.value == "*")
            {
                continue;
            }
            let Some(next) = next else {
                continue;
            };
            if cur.ty == "word" && is_placeholder(next) && cur.end == Some(next.start) {
                continue;
            }
            if cur.ty == "string" && cur.quoted {
                let open = cur.value.rfind("#{");
                let close = cur.value.rfind('}');
                match (open, close) {
                    (Some(o), Some(c)) => in_interp_string = o > c,
                    (Some(_), None) => in_interp_string = true,
                    (None, Some(_)) => in_interp_string = false,
                    _ => {}
                }
            }
            if in_interp_string {
                continue;
            }
            if cur.ty == "colon" || next.ty == "colon" {
                continue;
            }
            if cur.ty == "atword" && (cur.value.is_empty() || cur.value.ends_with('[')) {
                continue;
            }
            if next.ty == "word" && next.value.starts_with(']') {
                continue;
            }
            if cur.value == "~" {
                continue;
            }
            if cur.ty != "string"
                && !cur.value.is_empty()
                && cur.value.contains('\\')
                && next.ty != "comment"
            {
                continue;
            }
            if prev.is_some_and(|p| {
                // `indexOf`: the first backslash is the last character.
                !p.value.is_empty() && p.value.find('\\') == Some(p.value.len() - 1)
            }) && cur.ty == "operator"
                && cur.value == "/"
            {
                continue;
            }
            if cur.value == "\\" {
                continue;
            }
            if cur.value == "$$" && cur.ty == "func" && next.ty == "word" && next.before.is_empty()
            {
                continue;
            }
            let hash = |x: &V| x.ty == "word" && x.value == "#";
            let lbrace = |x: &V| x.ty == "word" && x.value == "{";
            let rbrace = |x: &V| x.ty == "word" && x.value == "}";
            let empty_before = |x: &V| x.before.is_empty() && raw_before_known(x);
            if hash(cur)
                || lbrace(cur)
                || rbrace(next)
                || (lbrace(next) && empty_before(next))
                || (rbrace(cur) && empty_before(next))
            {
                continue;
            }
            if cur.value == "--" && hash(next) {
                continue;
            }
            let is_math = is_math_op(cur);
            let next_math = is_math_op(next);
            if ((is_math && hash(next)) || (next_math && rbrace(cur))) && empty_before(next) {
                continue;
            }
            if prev.is_none() && is_div(cur) {
                continue;
            }
            if in_calc
                && (is_add(cur) || is_add(next) || is_sub(cur) || is_sub(next))
                && empty_before(next)
            {
                continue;
            }
            let color_adjuster = (is_add(cur) || is_sub(cur))
                && i == 0
                && (next.ty == "number" || next.is_hex)
                && grand.is_some_and(is_color_adjuster)
                && !empty_before(next);
            let is_word = |x: &V| x.ty == "word" || x.ty == "atword";
            let space_before_op = next2.is_some_and(|n| n.ty == "func" || is_word(n))
                || cur.ty == "func"
                || is_word(cur);
            let space_after_op = next.ty == "func"
                || is_word(next)
                || prev.is_some_and(|p| p.ty == "func" || is_word(p));
            if scss
                && is_math
                && cur.value == "-"
                && next.ty == "func"
                && cur.end != Some(next.start)
            {
                push_cat(&mut parts, s(" "));
                continue;
            }
            if !(is_mul(next) || is_mul(cur))
                && !in_calc
                && !color_adjuster
                && ((is_div(next) && !space_before_op)
                    || (is_div(cur) && !space_after_op)
                    || (is_add(next) && !space_before_op)
                    || (is_add(cur) && !space_after_op)
                    || is_sub(next)
                    || is_sub(cur))
                && (empty_before(next)
                    || (is_math && (prev.is_none() || prev.is_some_and(is_math_op))))
            {
                continue;
            }
            if scss_or_less
                && is_math
                && cur.value == "-"
                && is_paren_group(next)
                && next.open.as_ref().is_some_and(|o| cur.end == Some(o.start))
            {
                continue;
            }
            if cur.ty == "comment" && cur.inline {
                if parent.is_some_and(|p| p.ty == "paren_group") {
                    parts.push(dedent(hardline()));
                    parts.push(s(""));
                    continue;
                }
                parts.push(hardline());
                parts.push(s(""));
                continue;
            }
            let w = |x: &V, set: &[&str]| x.ty == "word" && set.contains(&x.value.as_str());
            if is_control
                && (w(next, &["==", "!="])
                    || w(next, &["<", ">", "<=", ">="])
                    || w(next, &["and", "or", "not"])
                    || w(cur, &["in"])
                    || w(cur, &["from", "through", "end"]))
            {
                push_cat(&mut parts, s(" "));
                continue;
            }
            if self.atrule_name_in(top, &["namespace"]) {
                push_cat(&mut parts, s(" "));
                continue;
            }
            if is_grid {
                if cur.has_source && next.has_source && cur.start_line != next.start_line {
                    parts.push(hardline());
                    parts.push(s(""));
                    did_break = true;
                } else {
                    push_cat(&mut parts, s(" "));
                }
                continue;
            }
            if next_math {
                push_cat(&mut parts, s(" "));
                continue;
            }
            if next.value == "..." {
                continue;
            }
            if is_placeholder(cur) && is_placeholder(next) && cur.end == Some(next.start) {
                continue;
            }
            if is_placeholder(cur)
                && is_paren_group(next)
                && cur.end == next.open.as_ref().map(|o| o.start)
            {
                parts.push(softline());
                parts.push(s(""));
                continue;
            }
            if cur.value == "with" && is_paren_group(next) {
                let taken = std::mem::take(&mut parts);
                parts = vec![concat(vec![fill(taken), s(" ")])];
                continue;
            }
            if cur.value.ends_with('#') && next.value == "{" {
                // `isParenGroupNode(iNextNode.group)` reads `.type` of `undefined` and throws.
                let Some(g) = next.group.as_deref() else {
                    return bail("printCommaSeparatedValueGroup throws on `#` before `{`");
                };
                if is_paren_group(g) {
                    continue;
                }
            }
            if next.ty == "comment" && next.inline && next2.is_none() {
                continue;
            }
            parts.push(line());
            parts.push(s(""));
        }
        if has_inline_comment {
            push_cat(&mut parts, break_parent());
        }
        if did_break {
            parts.insert(0, hardline());
            parts.insert(0, s(""));
        }
        if is_control {
            return Ok(group(indent(concat(parts))));
        }
        if node.groups.is_empty() {
            return bail("empty comma group");
        }
        let import_url = node.groups.first().is_some_and(|g| g.value == "url")
            && node.groups.len() == 2
            && top.atrule.is_some_and(|a| a.name == "import");
        if import_url {
            return Ok(group(fill(parts)));
        }
        Ok(group(indent(fill(parts))))
    }

    fn print_paren_group(&self, node: &V, stack: &[&V], top: Top<'a>) -> R {
        let anc = &stack[..stack.len() - 1];
        let parent = anc.last().copied();
        let mut docs = Vec::new();
        for g in &node.groups {
            docs.push(match g.ty {
                "raw" => s(g.value.clone()),
                "selector" => match parse_selector(&g.value)? {
                    Sel::Parsed(t) => self.print_sel(&t, 0, top)?,
                    Sel::Unknown(_) => {
                        let (Some(open), Some(close)) = (&node.open, &node.close) else {
                            return bail("selector parens");
                        };
                        let Some(open_end) = open.end else {
                            return bail("selector paren end");
                        };
                        let sel = trim(&self.slice(open_end + 1, Some(close.start))?).to_string();
                        if last_line_has_inline_comment(&sel) {
                            concat(vec![break_parent(), s(sel)])
                        } else {
                            s(sel)
                        }
                    }
                },
                _ => self.print_value(g, stack, top)?,
            });
        }
        let print_paren = |p: &Option<Box<V>>| p.as_ref().map_or(empty(), |x| s(x.value.clone()));
        if parent.is_some_and(|p| p.ty == "func" && p.value.to_lowercase() == "url")
            && (node.groups.len() == 1
                || (!node.groups.is_empty()
                    && node.groups[0].ty == "comma_group"
                    && node.groups[0]
                        .groups
                        .first()
                        .is_some_and(|g| g.ty == "word" && g.value.starts_with("data:"))))
        {
            return Ok(concat(vec![
                print_paren(&node.open),
                join(&s(","), docs),
                print_paren(&node.close),
            ]));
        }
        if node.open.is_none() {
            let decl_top =
                top.key_value && anc.len() == 2 && anc[1].ty == "value" && anc[0].ty == "root";
            let force = decl_top
                && node.groups.iter().any(|g| g.ty == "comma_group")
                && (top.variable || top.decl.is_some_and(|d| !d.prop.starts_with("--")));
            let softline_first = decl_top && !top.variable && top.decl.is_some();
            let mut with_comma: Vec<Doc> = Vec::new();
            let n = docs.len();
            for (i, d) in docs.into_iter().enumerate() {
                if i + 1 < n {
                    with_comma.push(concat(vec![d, s(",")]));
                } else {
                    with_comma.push(d);
                }
            }
            let sep = if force { hardline() } else { line() };
            let mut parts = Vec::new();
            for (i, d) in with_comma.into_iter().enumerate() {
                if i > 0 {
                    parts.push(sep.clone());
                }
                parts.push(d);
            }
            return Ok(indent(if force {
                concat(vec![hardline(), concat(parts)])
            } else {
                group(concat(vec![
                    if softline_first { softline() } else { empty() },
                    fill(parts),
                ]))
            }));
        }
        let n = node.groups.len();
        let mut parts = Vec::new();
        for (i, (child, doc)) in node.groups.iter().zip(docs).enumerate() {
            let is_last = i + 1 == n;
            let key_value_paren = child.ty == "comma_group"
                && child.groups.get(1).is_some_and(|g| g.ty == "colon")
                && child.groups.first().is_some_and(|g| g.ty != "paren_group")
                && child.groups.get(2).is_some_and(|g| g.ty == "paren_group");
            let doc = match doc {
                Doc::Group { contents, .. }
                    if key_value_paren
                        && matches!(&*contents, Doc::Indent(inner) if matches!(&**inner, Doc::Fill(_))) =>
                {
                    group(dedent(group(*contents)))
                }
                other => other,
            };
            let mut p = vec![doc];
            p.push(if is_last {
                self.trailing_comma(node, child, anc, top)?
            } else {
                s(",")
            });
            if !is_last && child.ty == "comma_group" && !child.groups.is_empty() {
                let mut last = child.groups.last();
                if let Some(l) = last
                    && !l.has_source
                    && l.close.is_some()
                {
                    last = l.close.as_deref();
                }
                if let Some(l) = last
                    && l.has_source
                    && let Some(e) = l.end
                    && e >= 0
                    && is_next_line_empty(self.text, self.byte(e)?)
                {
                    p.push(hardline());
                }
            }
            parts.push(concat(p));
        }
        let is_key = parent.is_some_and(|p| {
            p.ty == "comma_group"
                && p.groups.get(1).is_some_and(|g| g.ty == "colon")
                && p.groups
                    .iter()
                    .position(|g| std::ptr::eq(g, node))
                    .is_some_and(|i| p.groups.get(i + 1).is_some_and(|g| g.ty == "colon"))
        });
        let is_config = node.open.as_ref().is_some_and(|o| o.value == "(")
            && node.close.as_ref().is_some_and(|c| c.value == ")")
            && node.groups.iter().all(|g| g.ty == "comma_group")
            && parent.is_some_and(|p| {
                p.ty == "comma_group"
                    && p.groups
                        .iter()
                        .position(|g| std::ptr::eq(g, node))
                        .is_some_and(|i| {
                            i > 0
                                && p.groups
                                    .get(i - 1)
                                    .is_some_and(|w| w.ty == "word" && w.value == "with")
                        })
            });
        let map_item = self.is_scss_map_item(node, anc, top);
        let doc = group_with(
            concat(vec![
                print_paren(&node.open),
                indent(concat(vec![softline(), join(&line(), parts)])),
                softline(),
                print_paren(&node.close),
            ]),
            is_config || (map_item && !is_key),
            None,
        );
        Ok(if is_config || is_key {
            dedent(doc)
        } else {
            doc
        })
    }

    fn trailing_comma(&self, node: &V, child: &V, anc: &[&V], top: Top<'a>) -> R {
        let grand = anc.last().copied();
        if grand.is_some_and(|g| g.ty == "func" && g.value.to_lowercase() == "var")
            && child.has_source
            && let Some(close) = &node.close
        {
            let t = self.slice(child.start, Some(close.start))?;
            if t.trim_end_matches(is_js_whitespace).ends_with(',') {
                return Ok(s(","));
            }
        }
        if child.ty != "comment"
            && !(child.ty == "comma_group" && child.groups.iter().all(|g| g.ty == "comment"))
            && self.opts.trailing_comma != TrailingComma::None
            && self.is_scss_map_item(node, anc, top)
        {
            return Ok(if_break(s(","), empty()));
        }
        Ok(empty())
    }

    fn is_scss_map_item(&self, node: &V, anc: &[&V], top: Top<'a>) -> bool {
        if self.opts.parser != Parser::Scss || node.groups.is_empty() {
            return false;
        }
        let kv_paren = |x: &V| {
            x.ty == "paren_group"
                && x.groups.first().is_some_and(|g| {
                    g.ty == "comma_group" && g.groups.get(1).is_some_and(|c| c.ty == "colon")
                })
        };
        let grand = anc.len().checked_sub(2).map(|i| anc[i]);
        if !kv_paren(node) && !grand.is_some_and(kv_paren) {
            return false;
        }
        if top.decl.is_some_and(|d| d.prop.starts_with('$')) {
            return true;
        }
        if grand.is_some_and(kv_paren) {
            return true;
        }
        grand.is_some_and(|g| g.ty == "func")
    }
}

fn raw_before_known(v: &V) -> bool {
    !matches!(v.ty, "paren_group" | "comma_group")
}

fn is_placeholder(v: &V) -> bool {
    v.ty == "atword" && v.value.starts_with("prettier-placeholder-")
}

fn is_paren_group(v: &V) -> bool {
    v.ty == "paren_group"
        && v.open.as_ref().is_some_and(|o| o.value == "(")
        && v.close.as_ref().is_some_and(|c| c.value == ")")
}

fn is_op(v: &V, op: &str) -> bool {
    v.ty == "operator" && v.value == op
}

fn is_add(v: &V) -> bool {
    is_op(v, "+")
}
fn is_sub(v: &V) -> bool {
    is_op(v, "-")
}
fn is_mul(v: &V) -> bool {
    is_op(v, "*")
}
fn is_div(v: &V) -> bool {
    is_op(v, "/")
}
fn is_math_op(v: &V) -> bool {
    is_add(v) || is_sub(v) || is_mul(v) || is_div(v) || is_op(v, "%")
}

fn is_color_adjuster(v: &V) -> bool {
    v.ty == "func"
        && matches!(
            v.value.to_lowercase().as_str(),
            "red"
                | "green"
                | "blue"
                | "alpha"
                | "a"
                | "rgb"
                | "hue"
                | "h"
                | "saturation"
                | "s"
                | "lightness"
                | "l"
                | "whiteness"
                | "w"
                | "blackness"
                | "b"
                | "tint"
                | "shade"
                | "blend"
                | "blenda"
                | "contrast"
                | "hsl"
                | "hsla"
                | "hwb"
                | "hwba"
        )
}

fn inside_func(anc: &[&V], name: &str) -> bool {
    anc.iter()
        .rev()
        .find(|x| x.ty == "func")
        .is_some_and(|f| f.value.to_lowercase() == name)
}

fn previous_sibling<'v>(anc: &[&'v V], v: &V) -> Option<&'v V> {
    let parent = anc.last()?;
    let i = parent.groups.iter().position(|g| std::ptr::eq(g, v))?;
    if i == 0 {
        None
    } else {
        parent.groups.get(i - 1)
    }
}

fn should_break_list_node(g: &V, decl: &N) -> bool {
    g.ty == "paren_group"
        && g.open.is_none()
        && g.groups.iter().any(|x| x.ty == "comma_group")
        && decl.kind == Kind::Decl
        && !decl.prop.starts_with("--")
}

fn detached_ruleset(v: &str) -> bool {
    v.starts_with('@')
        && v.len() > 1
        && v[1..].contains(':')
        && !v[1..].starts_with(':')
        && !v.contains('\n')
}

fn replace_ci(raw: &str, _bang: &str, word: &str, rep: &str) -> String {
    let lower = raw.to_lowercase();
    let Some(wi) = lower.find(word) else {
        return raw.to_string();
    };
    let before = &raw[..wi];
    let trimmed = before.trim_end_matches(is_js_whitespace);
    let Some(bang) = trimmed.strip_suffix('!') else {
        return raw.to_string();
    };
    let start = bang.trim_end_matches(is_js_whitespace).len();
    format!("{}{rep}{}", &raw[..start], &raw[wi + word.len()..])
}

fn replace_directive(raw: &str, word: &str) -> String {
    let lower = raw.to_lowercase();
    match lower.find(word) {
        Some(i) => {
            let start = raw[..i].trim_end_matches(is_js_whitespace).len();
            format!("{} {}{}", &raw[..start], word, &raw[i + word.len()..])
        }
        None => raw.to_string(),
    }
}

/// Prettier's `css` parser and printer: the doc for `text`.
pub fn print(text: &str, opts: &Options) -> R {
    let orig = text;
    let fm = crate::markdown::remark::parse_front_matter(text);
    let content: String;
    let text = match &fm {
        Some(f) => {
            let spaced: String = f
                .raw
                .chars()
                .map(|c| if c == '\n' { '\n' } else { ' ' })
                .collect();
            content = format!("{spaced}{}", &text[f.raw.len()..]);
            content.as_str()
        }
        None => text,
    };
    let Some(root) = parse_root(text, opts.parser) else {
        return bail("css parse error");
    };
    let idx = Idx::new(text);
    let oidx = Idx::new(orig);
    let src = Src {
        idx: &idx,
        orig,
        oidx: &oidx,
    };
    let root = process(root, src, opts.parser)?;
    let fm_doc = match &fm {
        None => None,
        Some(f) if f.language == "yaml" => {
            let value = trim(&f.value);
            let doc = if value.is_empty() {
                None
            } else {
                let mut o = opts.clone();
                o.parser = Parser::Yaml;
                Some(crate::markdown::strip_trailing_hardline(
                    crate::text_to_doc(value, &o)?,
                ))
            };
            let has = doc.is_some();
            Some(concat(vec![
                mark_as_root(concat(vec![
                    s(f.start_delimiter.clone()),
                    s(f.explicit_language.clone()),
                    hardline(),
                    doc.unwrap_or_else(empty),
                    if has { hardline() } else { empty() },
                    s(f.end_delimiter.clone()),
                ])),
                hardline(),
            ]))
        }
        Some(f) => Some(concat(vec![s(f.raw.clone()), hardline()])),
    };
    let pr = Pr {
        text: orig,
        idx: &oidx,
        opts,
    };
    if let Some(fd) = fm_doc {
        let top = Top {
            decl: None,
            atrule: None,
            rule: None,
            key_value: false,
            variable: false,
        };
        let body = pr.print_css(&root, top, &root, &[], 0)?;
        return Ok(concat(vec![fd, hardline(), body]));
    }
    let top = Top {
        decl: None,
        atrule: None,
        rule: None,
        key_value: false,
        variable: false,
    };
    pr.print_css(&root, top, &root, &[], 0)
}
