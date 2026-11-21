//! lib/rules/utils/char-source.js, regular-expressions.js and unicode/*.

use crate::ast::NodeId;
use crate::linter::Cx;
use crate::regexpp::validator::{Options, Validator};
use crate::scope::ScopeId;
use crate::static_value::{
    ReferenceTracker, TraceMap, V, get_static_value, get_string_if_constant, num_to_string,
};

pub fn utf16(s: &str) -> Vec<u16> {
    s.encode_utf16().collect()
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CodeUnit {
    pub start: usize,
    pub len: usize,
    pub value: u16,
}

impl CodeUnit {
    pub fn end(&self) -> usize {
        self.start + self.len
    }
}

struct TextReader<'a> {
    source: &'a [u16],
    pos: usize,
}

impl TextReader<'_> {
    fn at(&self, offset: isize) -> Option<u16> {
        let i = self.pos as isize + offset;
        if i < 0 {
            return None;
        }
        self.source.get(i as usize).copied()
    }

    fn read(&self, offset: isize, length: usize) -> Vec<u16> {
        let start = (self.pos as isize + offset).max(0) as usize;
        let end = (start + length).min(self.source.len());
        if start >= end {
            Vec::new()
        } else {
            self.source[start..end].to_vec()
        }
    }
}

fn hex_value(units: &[u16]) -> Option<u32> {
    let s = String::from_utf16(units).ok()?;
    u32::from_str_radix(&s, 16).ok()
}

fn read_hex_sequence(reader: &mut TextReader, length: usize) -> Vec<u16> {
    let units = reader.read(0, length);
    reader.pos += length;
    vec![hex_value(&units).unwrap_or(0) as u16]
}

fn read_unicode_sequence(reader: &mut TextReader) -> Vec<u16> {
    if reader.at(0) == Some(u16::from(b'{')) {
        let mut i = reader.pos + 1;
        while i < reader.source.len()
            && reader.source[i] < 0x80
            && (reader.source[i] as u8).is_ascii_hexdigit()
        {
            i += 1;
        }
        if i > reader.pos + 1 && reader.source.get(i) == Some(&u16::from(b'}')) {
            let cp = hex_value(&reader.source[reader.pos + 1..i]).unwrap_or(0);
            reader.pos = i + 1;
            let mut buf = [0u16; 2];
            return match char::from_u32(cp) {
                Some(c) => c.encode_utf16(&mut buf).to_vec(),
                None => vec![cp as u16],
            };
        }
    }
    read_hex_sequence(reader, 4)
}

fn read_octal_sequence(reader: &mut TextReader, max_length: usize) -> Vec<u16> {
    let units = reader.read(-1, max_length);
    let n = units
        .iter()
        .take_while(|u| (u16::from(b'0')..=u16::from(b'7')).contains(u))
        .count();
    reader.pos += n.saturating_sub(1);
    let mut v = 0u32;
    for u in &units[..n] {
        v = v * 8 + u32::from(*u - u16::from(b'0'));
    }
    vec![v as u16]
}

fn read_escape_sequence_or_line_continuation(reader: &mut TextReader) -> Vec<u16> {
    let ch = reader.at(1);
    reader.pos += 2;
    let Some(ch) = ch else { return Vec::new() };
    let simple = match ch {
        0x62 => Some(0x08),
        0x66 => Some(0x0c),
        0x6e => Some(0x0a),
        0x72 => Some(0x0d),
        0x74 => Some(0x09),
        0x76 => Some(0x0b),
        _ => None,
    };
    if let Some(s) = simple {
        return vec![s];
    }
    match ch {
        0x78 => read_hex_sequence(reader, 2),
        0x75 => read_unicode_sequence(reader),
        0x0d => {
            if reader.at(0) == Some(0x0a) {
                reader.pos += 1;
            }
            Vec::new()
        }
        0x0a | 0x2028 | 0x2029 => Vec::new(),
        0x30..=0x33 => read_octal_sequence(reader, 3),
        0x34..=0x37 => read_octal_sequence(reader, 2),
        _ => vec![ch],
    }
}

fn map_escape_sequence_or_line_continuation(reader: &mut TextReader, out: &mut Vec<CodeUnit>) {
    let start = reader.pos;
    let units = read_escape_sequence_or_line_continuation(reader);
    let len = reader.pos - start;
    for u in units.iter().take(2) {
        out.push(CodeUnit {
            start,
            len,
            value: *u,
        });
    }
}

pub fn parse_string_literal(source: &[u16]) -> Vec<CodeUnit> {
    let mut reader = TextReader { source, pos: 0 };
    let quote = reader.at(0);
    reader.pos += 1;
    let mut out = Vec::new();
    loop {
        let ch = reader.at(0);
        if ch == quote || ch.is_none() {
            break;
        }
        if ch == Some(0x5c) {
            map_escape_sequence_or_line_continuation(&mut reader, &mut out);
        } else {
            out.push(CodeUnit {
                start: reader.pos,
                len: 1,
                value: ch.unwrap_or(0),
            });
            reader.pos += 1;
        }
    }
    out
}

pub fn parse_template_token(source: &[u16]) -> Vec<CodeUnit> {
    let mut reader = TextReader { source, pos: 0 };
    reader.pos += 1;
    let mut out = Vec::new();
    loop {
        let ch = reader.at(0);
        if ch.is_none() || ch == Some(0x60) || (ch == Some(0x24) && reader.at(1) == Some(0x7b)) {
            break;
        }
        if ch == Some(0x5c) {
            map_escape_sequence_or_line_continuation(&mut reader, &mut out);
        } else if ch == Some(0x0d) {
            let len = if reader.at(1) == Some(0x0a) { 2 } else { 1 };
            out.push(CodeUnit {
                start: reader.pos,
                len,
                value: 0x0a,
            });
            reader.pos += len;
        } else {
            out.push(CodeUnit {
                start: reader.pos,
                len: 1,
                value: ch.unwrap_or(0),
            });
            reader.pos += 1;
        }
    }
    out
}

pub fn is_combining_character(cp: u32) -> bool {
    crate::regexpp::validator::is_mark(cp)
}

pub fn is_emoji_modifier(code: u32) -> bool {
    (0x1f3fb..=0x1f3ff).contains(&code)
}

pub fn is_regional_indicator_symbol(code: u32) -> bool {
    (0x1f1e6..=0x1f1ff).contains(&code)
}

pub fn is_surrogate_pair(lead: u32, tail: u32) -> bool {
    (0xd800..0xdc00).contains(&lead) && (0xdc00..0xe000).contains(&tail)
}

pub const REGEXPP_LATEST_ECMA_VERSION: u32 = 2025;

pub fn is_valid_with_unicode_flag(ecma_version: u32, pattern: &[u16]) -> bool {
    if ecma_version <= 5 {
        return false;
    }
    let options = Options {
        strict: false,
        ecma_version: Some(ecma_version.min(REGEXPP_LATEST_ECMA_VERSION)),
    };
    Validator::new(options, ())
        .validate_pattern(pattern, 0, pattern.len(), Some((true, false)))
        .is_ok()
}

pub fn node_text16(cx: &Cx, node: NodeId) -> Vec<u16> {
    utf16(cx.sc.node_text(node))
}

pub fn regex_of(cx: &Cx, node: NodeId) -> Option<(Vec<u16>, String)> {
    let ast = &cx.sc.ast;
    if !ast.is(node, "Literal") || ast.prop(node, "regex").is_none() {
        return None;
    }
    let raw = cx.sc.node_text(node);
    let slash = raw.rfind('/')?;
    if slash == 0 {
        return None;
    }
    Some((utf16(&raw[1..slash]), raw[slash + 1..].to_string()))
}

pub fn string_value(cx: &Cx, node: NodeId) -> Option<Vec<u16>> {
    let ast = &cx.sc.ast;
    if !ast.is(node, "Literal")
        || !matches!(ast.prop(node, "value"), Some(crate::ast::Prop::Str(_)))
    {
        return None;
    }
    Some(
        parse_string_literal(&node_text16(cx, node))
            .iter()
            .map(|u| u.value)
            .collect(),
    )
}

pub fn static_template_value(cx: &Cx, node: NodeId) -> Option<Vec<u16>> {
    if !crate::utils::is_static_template_literal(&cx.sc.ast, node) {
        return None;
    }
    Some(
        parse_template_token(&node_text16(cx, node))
            .iter()
            .map(|u| u.value)
            .collect(),
    )
}

#[derive(Debug, Clone, PartialEq)]
pub enum Static {
    Null,
    Str(Vec<u16>),
    Regex,
    Unknown,
}

fn from_value(v: &V) -> Static {
    match v {
        V::Str(s) if s.contains('\u{fffd}') => Static::Unknown,
        V::Str(s) => Static::Str(utf16(s)),
        V::Undefined => Static::Str(utf16("undefined")),
        V::Null => Static::Str(utf16("null")),
        V::Bool(b) => Static::Str(utf16(if *b { "true" } else { "false" })),
        V::Num(n) => Static::Str(utf16(&num_to_string(*n))),
        V::RegExp(_) => Static::Regex,
        _ => Static::Unknown,
    }
}

fn exact_string(cx: &Cx, node: NodeId) -> Option<Vec<u16>> {
    string_value(cx, node).or_else(|| static_template_value(cx, node))
}

pub fn static_value(cx: &Cx, node: Option<NodeId>, scope: ScopeId) -> Static {
    let Some(node) = node else {
        return Static::Null;
    };
    if let Some(s) = exact_string(cx, node) {
        return Static::Str(s);
    }
    match get_static_value(&cx.sc.ast, cx.sm(), node, Some(scope)) {
        Ok(Some(v)) => from_value(&v.value),
        Ok(None) => Static::Null,
        Err(_) => Static::Unknown,
    }
}

pub fn string_if_constant(cx: &Cx, node: Option<NodeId>, scope: ScopeId) -> Static {
    let Some(node) = node else {
        return Static::Null;
    };
    if let Some(s) = exact_string(cx, node) {
        return Static::Str(s);
    }
    match get_string_if_constant(&cx.sc.ast, cx.sm(), Some(node), Some(scope)) {
        Ok(Some(s)) if s.contains('\u{fffd}') => Static::Unknown,
        Ok(Some(s)) => Static::Str(utf16(&s)),
        Ok(None) => Static::Null,
        Err(_) => Static::Unknown,
    }
}

pub fn regexp_global_calls(cx: &Cx) -> Result<Vec<NodeId>, &'static str> {
    let sm = cx.sm();
    let map = TraceMap {
        keys: vec![(
            "RegExp".into(),
            TraceMap {
                call: Some(0),
                construct: Some(0),
                ..TraceMap::default()
            },
        )],
        ..TraceMap::default()
    };
    let refs = ReferenceTracker::new(&cx.sc.ast, sm, sm.global_scope())
        .iterate_global_references(&map)
        .map_err(|_| "RegExp reference this port does not track")?;
    Ok(refs.into_iter().map(|r| r.node).collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn units(s: &str) -> Vec<(usize, usize, u16)> {
        parse_string_literal(&utf16(s))
            .iter()
            .map(|u| (u.start, u.len, u.value))
            .collect()
    }

    #[test]
    fn string_literal_code_units() {
        assert_eq!(units("'a\\x41'"), vec![(1, 1, 0x61), (2, 4, 0x41)]);
        assert_eq!(units("'\\u{1F44D}'"), vec![(1, 9, 0xd83d), (1, 9, 0xdc4d)]);
        assert_eq!(units("'\\08'"), vec![(1, 2, 0), (3, 1, 0x38)]);
        assert_eq!(units("'\\377\\\n'"), vec![(1, 4, 0xff)]);
        assert_eq!(units("'\\ud83d'"), vec![(1, 6, 0xd83d)]);
    }

    #[test]
    fn template_code_units() {
        let t: Vec<(usize, usize, u16)> = parse_template_token(&utf16("`a\r\nb${"))
            .iter()
            .map(|u| (u.start, u.len, u.value))
            .collect();
        assert_eq!(t, vec![(1, 1, 0x61), (2, 2, 0x0a), (4, 1, 0x62)]);
    }

    #[test]
    fn unicode_flag_validity() {
        assert!(is_valid_with_unicode_flag(2026, &utf16("[\u{1F44D}]")));
        assert!(!is_valid_with_unicode_flag(2026, &utf16("\\a")));
        assert!(!is_valid_with_unicode_flag(5, &utf16("a")));
        assert!(is_combining_character(0x301));
        assert!(!is_combining_character(0x41));
    }
}
