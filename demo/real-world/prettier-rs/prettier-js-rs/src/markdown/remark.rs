//! remark-parse 8.0.3 (MIT, Copyright (c) Titus Wormer) with the plugins Prettier's `markdown`
//! parser adds: remark-footnotes 2.0.0 (MIT, Copyright (c) Titus Wormer), remark-math 3.0.1 (MIT,
//! Copyright (c) Junyoung Choi), and Prettier's front matter,
//! liquid and wiki link tokenizers; parse-entities 2.0.0 for character references. Text is held as
//! `char`s, so offsets and columns count characters.

use std::collections::HashMap;
use std::rc::Rc;

use super::entities::{INVALID, LEGACY, NAMED};
use crate::comments::is_js_whitespace;

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Pt {
    pub line: usize,
    pub column: usize,
    pub offset: i64,
}

#[derive(Clone, Debug, Default)]
pub struct Node {
    pub ty: &'static str,
    pub start: Pt,
    pub end: Pt,
    pub has_pos: bool,
    pub children: Vec<Node>,
    pub value: String,
    pub depth: usize,
    pub ordered: bool,
    pub start_num: Option<f64>,
    pub spread: bool,
    pub checked: Option<bool>,
    pub lang: Option<String>,
    pub meta: Option<String>,
    pub url: String,
    pub title: Option<String>,
    pub alt: Option<String>,
    pub identifier: String,
    pub label: String,
    pub reference_type: &'static str,
    pub align: Vec<Option<&'static str>>,
    pub fm_language: String,
    pub explicit_language: String,
    pub start_delimiter: String,
    pub end_delimiter: String,
    pub is_indented: bool,
    pub is_aligned: bool,
    pub has_indented_codeblock: bool,
    pub kind: u8,
    pub leading_punct: bool,
    pub trailing_punct: bool,
}

pub fn node(ty: &'static str) -> Node {
    Node {
        ty,
        ..Default::default()
    }
}

type R<T = ()> = Result<T, ()>;

pub struct TokState {
    buf: Rc<Vec<char>>,
    pos: usize,
    line: usize,
    column: usize,
    tokens: Vec<Node>,
}

struct Eaten {
    before: Pt,
    line: usize,
    column: usize,
    pos: usize,
}

#[derive(Clone, Copy, PartialEq, Debug)]
enum B {
    FrontMatter,
    BlankLine,
    IndentedCode,
    FencedCode,
    Math,
    Blockquote,
    AtxHeading,
    ThematicBreak,
    List,
    SetextHeading,
    Html,
    FootnoteDefinition,
    Definition,
    Table,
    Paragraph,
}

const BLOCK_METHODS: [B; 15] = [
    B::FrontMatter,
    B::BlankLine,
    B::IndentedCode,
    B::FencedCode,
    B::Math,
    B::Blockquote,
    B::AtxHeading,
    B::ThematicBreak,
    B::List,
    B::SetextHeading,
    B::Html,
    B::FootnoteDefinition,
    B::Definition,
    B::Table,
    B::Paragraph,
];

const INTERRUPT_PARAGRAPH: [B; 7] = [
    B::Math,
    B::ThematicBreak,
    B::List,
    B::AtxHeading,
    B::FencedCode,
    B::Blockquote,
    B::Html,
];
const INTERRUPT_LIST: [B; 4] = [B::Math, B::AtxHeading, B::FencedCode, B::ThematicBreak];
const INTERRUPT_BLOCKQUOTE: [B; 8] = [
    B::Math,
    B::IndentedCode,
    B::FencedCode,
    B::AtxHeading,
    B::SetextHeading,
    B::ThematicBreak,
    B::Html,
    B::List,
];
const INTERRUPT_FOOTNOTE: [B; 11] = [
    B::BlankLine,
    B::FencedCode,
    B::Blockquote,
    B::AtxHeading,
    B::ThematicBreak,
    B::List,
    B::SetextHeading,
    B::Html,
    B::Definition,
    B::Table,
    B::FootnoteDefinition,
];

#[derive(Clone, Copy, PartialEq, Debug)]
enum I {
    Escape,
    AutoLink,
    Url,
    Email,
    Html,
    WikiLink,
    Link,
    FootnoteCall,
    Reference,
    Strong,
    Emphasis,
    Deletion,
    Code,
    Break,
    Math,
    Liquid,
    Text,
}

const INLINE_METHODS: [I; 17] = [
    I::Escape,
    I::AutoLink,
    I::Url,
    I::Email,
    I::Html,
    I::WikiLink,
    I::Link,
    I::FootnoteCall,
    I::Reference,
    I::Strong,
    I::Emphasis,
    I::Deletion,
    I::Code,
    I::Break,
    I::Math,
    I::Liquid,
    I::Text,
];

const ESCAPES: &str = "\\`*{}[]()#+-.!_>~|\n\"$%&',/:;<=?@^";

fn at(v: &[char], i: usize) -> Option<char> {
    v.get(i).copied()
}

fn is(v: &[char], i: usize, c: char) -> bool {
    v.get(i) == Some(&c)
}

fn ws(c: Option<char>) -> bool {
    c.is_some_and(is_js_whitespace)
}

fn s(v: &[char]) -> String {
    v.iter().collect()
}

fn slice(v: &[char], a: usize, b: usize) -> &[char] {
    let b = b.min(v.len());
    let a = a.min(b);
    &v[a..b]
}

fn index_of(v: &[char], pat: &str, from: usize) -> Option<usize> {
    let p: Vec<char> = pat.chars().collect();
    if p.is_empty() {
        return Some(from.min(v.len()));
    }
    let mut i = from;
    while i + p.len() <= v.len() {
        if v[i..i + p.len()] == p[..] {
            return Some(i);
        }
        i += 1;
    }
    None
}

fn js_trim(s: &str) -> &str {
    s.trim_matches(is_js_whitespace)
}

fn trim_chars(v: &[char]) -> &[char] {
    let mut a = 0;
    let mut b = v.len();
    while a < b && is_js_whitespace(v[a]) {
        a += 1;
    }
    while b > a && is_js_whitespace(v[b - 1]) {
        b -= 1;
    }
    &v[a..b]
}

fn trim_trailing_lines(v: &[char]) -> &[char] {
    let mut b = v.len();
    while b > 0 && v[b - 1] == '\n' {
        b -= 1;
    }
    &v[..b]
}

pub fn collapse_white_space(s: &str) -> String {
    let mut out = String::new();
    let mut in_ws = false;
    for c in s.chars() {
        if is_js_whitespace(c) {
            if !in_ws {
                out.push(' ');
            }
            in_ws = true;
        } else {
            out.push(c);
            in_ws = false;
        }
    }
    out
}

fn normalize(s: &str) -> String {
    collapse_white_space(s).to_lowercase()
}

fn decimal(c: Option<char>) -> bool {
    c.is_some_and(|c| c.is_ascii_digit())
}

fn alphabetical(c: Option<char>) -> bool {
    c.is_some_and(|c| c.is_ascii_alphabetic())
}

fn word_char(c: Option<char>) -> bool {
    c.is_some_and(|c| c.is_ascii_alphanumeric() || c == '_')
}

#[derive(Clone, Copy, PartialEq)]
enum RefKind {
    Named,
    Decimal,
    Hex,
}

/// parse-entities: the text and reference pieces of `v`, with each reference's source.
pub fn decode_pieces(v: &[char], non_terminated: bool) -> Vec<(String, Option<String>)> {
    let mut out: Vec<(String, Option<String>)> = Vec::new();
    let mut queue = String::new();
    let mut reference: Option<String> = None;
    let len = v.len() + 1;
    let mut index = 0;
    while index < len {
        let ch = at(v, index);
        if ch == Some('&') {
            let mut following = at(v, index + 1);
            if matches!(
                following,
                None | Some('\t' | '\n' | '\x0c' | ' ' | '&' | '<')
            ) {
                queue.push('&');
                index += 1;
                continue;
            }
            let start = index + 1;
            let mut begin = start;
            let mut end;
            let kind;
            if following == Some('#') {
                begin += 1;
                end = begin;
                following = at(v, end);
                if matches!(following, Some('X' | 'x')) {
                    kind = RefKind::Hex;
                    begin += 1;
                } else {
                    kind = RefKind::Decimal;
                }
            } else {
                kind = RefKind::Named;
            }
            end = begin;
            let mut entity_chars = String::new();
            let mut entity: Option<String> = None;
            let mut characters = String::new();
            while end < len {
                let f = at(v, end);
                let ok = match kind {
                    RefKind::Named => f.is_some_and(|c| c.is_ascii_alphanumeric()),
                    RefKind::Decimal => f.is_some_and(|c| c.is_ascii_digit()),
                    RefKind::Hex => f.is_some_and(|c| c.is_ascii_hexdigit()),
                };
                if !ok {
                    break;
                }
                characters.push(f.unwrap_or_default());
                if kind == RefKind::Named
                    && let Ok(i) = LEGACY.binary_search_by(|(k, _)| (*k).cmp(characters.as_str()))
                {
                    entity_chars = characters.clone();
                    entity = Some(LEGACY[i].1.to_string());
                }
                end += 1;
            }
            let mut terminated = at(v, end) == Some(';');
            if terminated {
                end += 1;
                if kind == RefKind::Named
                    && let Ok(i) = NAMED.binary_search_by(|(k, _)| (*k).cmp(characters.as_str()))
                {
                    entity_chars = characters.clone();
                    entity = Some(NAMED[i].1.to_string());
                }
            }
            if (!terminated && !non_terminated) || characters.is_empty() {
            } else if kind == RefKind::Named {
                if !(terminated && entity.is_none()) && entity_chars != characters {
                    end = begin + entity_chars.chars().count();
                    terminated = false;
                }
                let _ = terminated;
                reference = entity.clone();
            } else {
                let base = if kind == RefKind::Hex { 16 } else { 10 };
                let code = u64::from_str_radix(&characters, base).unwrap_or(u64::MAX);
                let prohibited = (0xd800..=0xdfff).contains(&code) || code > 0x10ffff;
                reference = Some(if prohibited {
                    "\u{FFFD}".to_string()
                } else if let Some((_, r)) = INVALID.iter().find(|(k, _)| u64::from(*k) == code) {
                    (*r).to_string()
                } else {
                    char::from_u32(code as u32)
                        .map(String::from)
                        .unwrap_or_default()
                });
            }
            match reference.as_ref().filter(|r| !r.is_empty()) {
                Some(r) => {
                    if !queue.is_empty() {
                        out.push((std::mem::take(&mut queue), None));
                    }
                    out.push((r.clone(), Some(s(slice(v, start - 1, end)))));
                }
                None => queue.extend(slice(v, start - 1, end)),
            }
            index = end;
            continue;
        }
        match ch {
            Some(c) => queue.push(c),
            None => {
                if !queue.is_empty() {
                    out.push((std::mem::take(&mut queue), None));
                }
            }
        }
        index += 1;
    }
    out
}

pub fn decode_raw(v: &[char], non_terminated: bool) -> String {
    decode_pieces(v, non_terminated)
        .into_iter()
        .map(|p| p.0)
        .collect()
}

fn unescape(v: &[char]) -> Vec<char> {
    let mut out = Vec::new();
    let mut i = 0;
    while i < v.len() {
        if v[i] == '\\' {
            match at(v, i + 1) {
                Some(c) if ESCAPES.contains(c) => {
                    out.push(c);
                    i += 2;
                }
                _ => {
                    out.push('\\');
                    i += 1;
                }
            }
            continue;
        }
        out.push(v[i]);
        i += 1;
    }
    out
}

fn get_indentation(v: &[char]) -> (usize, HashMap<usize, usize>) {
    let mut index = 0;
    let mut indent = 0;
    let mut stops = HashMap::new();
    let mut last = 0;
    while let Some(c @ ('\t' | ' ')) = at(v, index) {
        let size = if c == '\t' { 4 } else { 1 };
        indent += size;
        if size > 1 {
            indent = indent / size * size;
        }
        while last < indent {
            last += 1;
            stops.insert(last, index);
        }
        index += 1;
    }
    (indent, stops)
}

fn remove_indentation(value: &[char], maximum: usize) -> Vec<char> {
    let mut values: Vec<Vec<char>> = value.split(|&c| c == '\n').map(<[char]>::to_vec).collect();
    let mut first: Vec<char> = vec![' '; maximum];
    first.push('!');
    values.insert(0, first);
    let mut min_indent = usize::MAX;
    let mut matrix: Vec<HashMap<usize, usize>> = vec![HashMap::new(); values.len()];
    let mut position = values.len();
    while position > 0 {
        position -= 1;
        let (indent, stops) = get_indentation(&values[position]);
        matrix[position] = stops;
        if trim_chars(&values[position]).is_empty() {
            continue;
        }
        if indent > 0 {
            if indent < min_indent {
                min_indent = indent;
            }
        } else {
            min_indent = usize::MAX;
            break;
        }
    }
    if min_indent != usize::MAX {
        for position in (0..values.len()).rev() {
            let stops = &matrix[position];
            let mut index = min_indent;
            while index > 0 && !stops.contains_key(&index) {
                index -= 1;
            }
            let cut = match stops.get(&index) {
                Some(&s) => s + 1,
                None => 0,
            };
            let line = &values[position];
            values[position] = line[cut.min(line.len())..].to_vec();
        }
    }
    values.remove(0);
    values.join(&'\n')
}

pub struct Parser {
    line_starts: Vec<usize>,
    total: usize,
    offset: HashMap<usize, i64>,
    in_list: bool,
    in_block: bool,
    in_link: bool,
    at_start: bool,
    text_only: bool,
    eof: Option<Pt>,
}

impl Parser {
    fn to_offset(&self, line: usize, column: usize) -> i64 {
        if line == 0 || line > self.line_starts.len() + 1 {
            return -1;
        }
        let base = if line >= 2 {
            self.line_starts
                .get(line - 2)
                .copied()
                .unwrap_or(self.total + 1)
        } else {
            0
        };
        let offset = base as i64 + column as i64 - 1;
        if offset >= 0 && (offset as usize) < self.total + 1 {
            offset
        } else {
            -1
        }
    }

    fn now(&self, st: &TokState) -> Pt {
        Pt {
            line: st.line,
            column: st.column,
            offset: self.to_offset(st.line, st.column),
        }
    }

    fn update_position(&self, st: &mut TokState, sub: &[char]) {
        let mut last: Option<usize> = None;
        for (i, &c) in sub.iter().enumerate() {
            if c == '\n' {
                st.line += 1;
                last = Some(i);
            }
        }
        match last {
            None => st.column += sub.len(),
            Some(l) => st.column = sub.len() - l,
        }
        if let Some(&o) = self.offset.get(&st.line) {
            if last.is_some() {
                st.column = (st.column as i64 + o).max(0) as usize;
            } else if (st.column as i64) <= o {
                st.column = (o + 1) as usize;
            }
        }
    }

    fn eat(&self, st: &mut TokState, n: usize) -> Eaten {
        let before = self.now(st);
        let e = Eaten {
            before,
            line: st.line,
            column: st.column,
            pos: st.pos,
        };
        let end = (st.pos + n).min(st.buf.len());
        let buf = st.buf.clone();
        st.pos = end;
        self.update_position(st, &buf[e.pos..end]);
        e
    }

    fn eat_str(&self, st: &mut TokState, sub: &str) -> Eaten {
        self.eat(st, sub.chars().count())
    }

    fn pos(&self, st: &TokState, ev: &Eaten, n: &mut Node) {
        if !n.has_pos {
            n.start = ev.before;
            n.has_pos = true;
        }
        n.end = self.now(st);
    }

    fn mergeable(n: &Node) -> bool {
        n.ty != "text"
            || !n.has_pos
            || n.start.line != n.end.line
            || n.end.column - n.start.column == n.value.chars().count()
    }

    fn add(&mut self, st: &mut TokState, ev: &Eaten, mut n: Node, parent: Option<usize>) -> usize {
        self.pos(st, ev, &mut n);
        let now = self.now(st);
        let children = match parent {
            Some(p) => &mut st.tokens[p].children,
            None => &mut st.tokens,
        };
        let merge = children.last().is_some_and(|prev| {
            prev.ty == "text" && n.ty == "text" && Self::mergeable(prev) && Self::mergeable(&n)
        });
        if merge {
            if let Some(prev) = children.last_mut() {
                prev.value.push_str(&n.value);
            }
        } else {
            children.push(n);
        }
        let idx = children.len() - 1;
        children[idx].end = now;
        if self.at_start && !st.tokens.is_empty() {
            self.at_start = false;
        }
        idx
    }

    fn apply(&mut self, st: &mut TokState, ev: &Eaten, n: Node) -> usize {
        self.add(st, ev, n, None)
    }

    fn reset(&mut self, st: &mut TokState, ev: &Eaten, n: Node) -> usize {
        let idx = self.add(st, ev, n, None);
        st.line = ev.line;
        st.column = ev.column;
        st.pos = ev.pos;
        idx
    }

    pub fn tokenize_block(&mut self, value: Vec<char>, loc: Pt) -> R<Vec<Node>> {
        self.tokenize(true, value, loc)
    }

    pub fn tokenize_inline(&mut self, value: Vec<char>, loc: Pt) -> R<Vec<Node>> {
        self.tokenize(false, value, loc)
    }

    fn tokenize(&mut self, block: bool, value: Vec<char>, loc: Pt) -> R<Vec<Node>> {
        if value.is_empty() {
            return Ok(Vec::new());
        }
        let mut st = TokState {
            buf: Rc::new(value),
            pos: 0,
            line: loc.line,
            column: loc.column,
            tokens: Vec::new(),
        };
        self.update_position(&mut st, &[]);
        while st.pos < st.buf.len() {
            let before = st.pos;
            let buf = st.buf.clone();
            let mut matched = false;
            if block {
                for m in BLOCK_METHODS {
                    if m == B::FrontMatter && !self.at_start {
                        continue;
                    }
                    self.block(m, &mut st, &buf[before..], false)?;
                    if st.pos != before {
                        matched = true;
                        break;
                    }
                }
            } else {
                for m in INLINE_METHODS {
                    if self.text_only && m != I::Text {
                        continue;
                    }
                    if matches!(m, I::AutoLink | I::Url | I::Email) && self.in_link {
                        continue;
                    }
                    self.inline(m, &mut st, &buf[before..])?;
                    if st.pos != before {
                        matched = true;
                        break;
                    }
                }
            }
            if !matched {
                return Err(());
            }
        }
        self.eof = Some(self.now(&st));
        Ok(st.tokens)
    }

    fn block(&mut self, m: B, st: &mut TokState, v: &[char], silent: bool) -> R<bool> {
        match m {
            B::FrontMatter => self.front_matter(st, v),
            B::BlankLine => self.blank_line(st, v, silent),
            B::IndentedCode => self.indented_code(st, v, silent),
            B::FencedCode => self.fenced_code(st, v, silent),
            B::Math => self.math_block(st, v, silent),
            B::Blockquote => self.blockquote(st, v, silent),
            B::AtxHeading => self.atx_heading(st, v, silent),
            B::ThematicBreak => self.thematic_break(st, v, silent),
            B::List => self.list(st, v, silent),
            B::SetextHeading => self.setext_heading(st, v, silent),
            B::Html => self.html_block(st, v, silent),
            B::FootnoteDefinition => self.footnote_definition(st, v, silent),
            B::Definition => self.definition(st, v, silent),
            B::Table => self.table(st, v, silent),
            B::Paragraph => self.paragraph(st, v, silent),
        }
    }

    fn interrupt(&mut self, list: &[B], st: &mut TokState, v: &[char]) -> R<bool> {
        for &m in list {
            if self.block(m, st, v, true)? {
                return Ok(true);
            }
        }
        Ok(false)
    }

    fn front_matter(&mut self, st: &mut TokState, v: &[char]) -> R<bool> {
        let text = s(v);
        let Some(fm) = parse_front_matter(&text) else {
            return Ok(false);
        };
        let ev = self.eat_str(st, &fm.raw);
        let mut n = node("front-matter");
        n.value = fm.value;
        n.fm_language = fm.language;
        n.explicit_language = fm.explicit_language;
        n.start_delimiter = fm.start_delimiter;
        n.end_delimiter = fm.end_delimiter;
        self.apply(st, &ev, n);
        Ok(true)
    }

    fn blank_line(&mut self, st: &mut TokState, v: &[char], silent: bool) -> R<bool> {
        let mut index = 0;
        loop {
            let mut j = index;
            while matches!(at(v, j), Some(' ' | '\t')) {
                j += 1;
            }
            match at(v, j) {
                Some('\n') => index = j + 1,
                None => {
                    index = j;
                    break;
                }
                _ => break,
            }
            if index >= v.len() {
                break;
            }
        }
        if index == 0 {
            return Ok(false);
        }
        if silent {
            return Ok(true);
        }
        self.eat(st, index);
        Ok(true)
    }

    fn indented_code(&mut self, st: &mut TokState, v: &[char], silent: bool) -> R<bool> {
        let length = v.len();
        let mut index: isize = -1;
        let mut subvalue = String::new();
        let mut content = String::new();
        let mut sub_q = String::new();
        let mut content_q = String::new();
        let mut indent = false;
        loop {
            index += 1;
            if index as usize >= length {
                break;
            }
            let mut i = index as usize;
            let mut ch = at(v, i);
            if indent {
                indent = false;
                subvalue.push_str(&sub_q);
                content.push_str(&content_q);
                sub_q.clear();
                content_q.clear();
                if ch == Some('\n') {
                    sub_q.push('\n');
                    content_q.push('\n');
                } else {
                    subvalue.extend(ch);
                    content.extend(ch);
                    loop {
                        i += 1;
                        if i >= length {
                            break;
                        }
                        ch = at(v, i);
                        if ch.is_none() || ch == Some('\n') {
                            content_q.extend(ch);
                            sub_q.extend(ch);
                            break;
                        }
                        subvalue.extend(ch);
                        content.extend(ch);
                    }
                }
                index = i as isize;
            } else if ch == Some(' ') && is(v, i + 1, ' ') && is(v, i + 2, ' ') && is(v, i + 3, ' ')
            {
                sub_q.push_str("    ");
                index += 3;
                indent = true;
            } else if ch == Some('\t') {
                sub_q.push('\t');
                indent = true;
            } else {
                let mut blank = String::new();
                while matches!(ch, Some('\t' | ' ')) {
                    blank.extend(ch);
                    i += 1;
                    ch = at(v, i);
                }
                index = i as isize;
                if ch != Some('\n') {
                    break;
                }
                sub_q.push_str(&blank);
                sub_q.push('\n');
                content_q.push('\n');
            }
        }
        if content.is_empty() {
            return Ok(false);
        }
        if silent {
            return Ok(true);
        }
        let ev = self.eat_str(st, &subvalue);
        let mut n = node("code");
        n.value = content.trim_end_matches('\n').to_string();
        self.apply(st, &ev, n);
        Ok(true)
    }

    fn fenced_code(&mut self, st: &mut TokState, v: &[char], silent: bool) -> R<bool> {
        let mut length = v.len() + 1;
        let mut index = 0;
        let mut subvalue = String::new();
        while index < length {
            let c = at(v, index);
            if c != Some(' ') && c != Some('\t') {
                break;
            }
            subvalue.extend(c);
            index += 1;
        }
        let indent = index;
        let marker = at(v, index);
        if marker != Some('~') && marker != Some('`') {
            return Ok(false);
        }
        let marker = marker.unwrap_or('`');
        index += 1;
        let mut fence = 1;
        subvalue.push(marker);
        while index < length && at(v, index) == Some(marker) {
            subvalue.push(marker);
            fence += 1;
            index += 1;
        }
        if fence < 3 {
            return Ok(false);
        }
        while index < length && matches!(at(v, index), Some(' ' | '\t')) {
            subvalue.extend(at(v, index));
            index += 1;
        }
        let mut flag = String::new();
        let mut queue = String::new();
        while index < length {
            let c = at(v, index);
            if c == Some('\n') || (marker == '`' && c == Some(marker)) {
                break;
            }
            if matches!(c, Some(' ' | '\t')) {
                queue.extend(c);
            } else {
                flag.push_str(&queue);
                flag.extend(c);
                queue.clear();
            }
            index += 1;
        }
        let c = at(v, index);
        if c.is_some() && c != Some('\n') {
            return Ok(false);
        }
        if silent {
            return Ok(true);
        }
        subvalue.push_str(&flag);
        let flag_chars: Vec<char> = flag.chars().collect();
        let flag = decode_raw(&unescape(&flag_chars), true);
        if !queue.is_empty() {
            subvalue.push_str(&queue);
        }
        let mut closing = String::new();
        let mut ex_closing = String::new();
        let mut content = String::new();
        let mut ex_content = String::new();
        let mut skip = true;
        let mut character = None;
        while index < length {
            character = at(v, index);
            content.push_str(&closing);
            ex_content.push_str(&ex_closing);
            closing.clear();
            ex_closing.clear();
            if character != Some('\n') {
                content.extend(character);
                ex_closing.extend(character);
                index += 1;
                continue;
            }
            if skip {
                subvalue.push('\n');
                skip = false;
            } else {
                closing.push('\n');
                ex_closing.push('\n');
            }
            let mut q = String::new();
            index += 1;
            while index < length {
                character = at(v, index);
                if character != Some(' ') {
                    break;
                }
                q.push(' ');
                index += 1;
            }
            closing.push_str(&q);
            ex_closing.extend(q.chars().skip(indent));
            if q.chars().count() >= 4 {
                continue;
            }
            q.clear();
            while index < length {
                character = at(v, index);
                if character != Some(marker) {
                    break;
                }
                q.push(marker);
                index += 1;
            }
            closing.push_str(&q);
            ex_closing.push_str(&q);
            if q.chars().count() < fence {
                continue;
            }
            while index < length {
                character = at(v, index);
                if character != Some(' ') && character != Some('\t') {
                    break;
                }
                closing.extend(character);
                ex_closing.extend(character);
                index += 1;
            }
            if character.is_none() || character == Some('\n') {
                break;
            }
        }
        let _ = character;
        subvalue.push_str(&content);
        subvalue.push_str(&closing);
        let fc: Vec<char> = flag.chars().collect();
        let mut lang: Option<String> = None;
        let mut meta: Option<String> = None;
        length = fc.len();
        let mut i = 0;
        while i < length {
            let c = fc[i];
            if c == ' ' || c == '\t' {
                if lang.as_deref().is_none_or(str::is_empty) {
                    lang = Some(s(&fc[..i]));
                }
            } else if lang.as_deref().is_some_and(|l| !l.is_empty()) {
                meta = Some(s(&fc[i..]));
                break;
            }
            i += 1;
        }
        let ev = self.eat_str(st, &subvalue);
        let mut n = node("code");
        n.lang = lang
            .filter(|l| !l.is_empty())
            .or(if flag.is_empty() { None } else { Some(flag) });
        n.meta = meta.filter(|m| !m.is_empty());
        n.value = ex_content;
        self.apply(st, &ev, n);
        Ok(true)
    }

    fn math_block(&mut self, st: &mut TokState, v: &[char], silent: bool) -> R<bool> {
        let length = v.len();
        let mut index = 0;
        while index < length && v[index] == ' ' {
            index += 1;
        }
        let indent_size = index;
        while index < length && v[index] == '$' {
            index += 1;
        }
        let open = index - indent_size;
        if open < 2 {
            return Ok(false);
        }
        while index < length && v[index] == ' ' {
            index += 1;
        }
        let content_start = index;
        while index < length {
            let c = v[index];
            if c == '$' {
                return Ok(false);
            }
            if c == '\n' {
                break;
            }
            index += 1;
        }
        if at(v, index) != Some('\n') {
            return Ok(false);
        }
        if silent {
            return Ok(true);
        }
        let mut content: Vec<String> = Vec::new();
        if content_start != index {
            content.push(s(&v[content_start..index]));
        }
        index += 1;
        let next_lf = |from: usize| index_of(v, "\n", from).unwrap_or(length);
        let mut line_end = next_lf(index + 1);
        while index < length {
            let mut closing = false;
            let mut lcs = index;
            let mut lce = line_end;
            let mut li = line_end;
            let mut close_size = 0;
            while li > lcs && v[li - 1] == ' ' {
                li -= 1;
            }
            while li > lcs && v[li - 1] == '$' {
                close_size += 1;
                li -= 1;
            }
            if open <= close_size && index_of(v, "$", lcs) == Some(li) {
                closing = true;
                lce = li;
            }
            while lcs <= lce && lcs - index < indent_size && at(v, lcs) == Some(' ') {
                lcs += 1;
            }
            if closing {
                while lce > lcs && v[lce - 1] == ' ' {
                    lce -= 1;
                }
            }
            if !closing || lcs != lce {
                content.push(s(slice(v, lcs, lce)));
            }
            if closing {
                break;
            }
            index = line_end + 1;
            line_end = next_lf(index + 1);
        }
        let ev = self.eat(st, line_end.min(length));
        let mut n = node("math");
        n.value = content.join("\n");
        self.apply(st, &ev, n);
        Ok(true)
    }

    fn blockquote(&mut self, st: &mut TokState, v: &[char], silent: bool) -> R<bool> {
        let now = self.now(st);
        let mut current_line = now.line;
        let length = v.len();
        let mut index = 0;
        while index < length && matches!(v[index], ' ' | '\t') {
            index += 1;
        }
        if at(v, index) != Some('>') {
            return Ok(false);
        }
        if silent {
            return Ok(true);
        }
        let mut values: Vec<Vec<char>> = Vec::new();
        let mut contents: Vec<Vec<char>> = Vec::new();
        let mut indents: Vec<usize> = Vec::new();
        index = 0;
        while index < length {
            let next = index_of(v, "\n", index).unwrap_or(length);
            let start = index;
            let mut prefixed = false;
            while index < length && matches!(v[index], ' ' | '\t') {
                index += 1;
            }
            if at(v, index) == Some('>') {
                index += 1;
                prefixed = true;
                if at(v, index) == Some(' ') {
                    index += 1;
                }
            } else {
                index = start;
            }
            let content = slice(v, index, next).to_vec();
            if !prefixed && trim_chars(&content).is_empty() {
                break;
            }
            if !prefixed {
                let rest = v[index..].to_vec();
                if self.interrupt(&INTERRUPT_BLOCKQUOTE, st, &rest)? {
                    break;
                }
            }
            let line = if start == index {
                content.clone()
            } else {
                slice(v, start, next).to_vec()
            };
            indents.push(index - start);
            values.push(line);
            contents.push(content);
            index = next + 1;
        }
        let ev = self.eat(st, values.join(&'\n').len());
        for ind in indents {
            *self.offset.entry(current_line).or_insert(0) += ind as i64;
            current_line += 1;
        }
        let prev = self.in_block;
        self.in_block = true;
        let children = self.tokenize_block(contents.join(&'\n'), now)?;
        self.in_block = prev;
        let mut n = node("blockquote");
        n.children = children;
        self.apply(st, &ev, n);
        Ok(true)
    }

    fn atx_heading(&mut self, st: &mut TokState, v: &[char], silent: bool) -> R<bool> {
        let mut now = self.now(st);
        let mut length = v.len() + 1;
        let mut index: isize = -1;
        let mut subvalue = String::new();
        loop {
            index += 1;
            if index as usize >= length {
                break;
            }
            let c = at(v, index as usize);
            if c != Some(' ') && c != Some('\t') {
                index -= 1;
                break;
            }
            subvalue.extend(c);
        }
        let mut depth = 0;
        loop {
            index += 1;
            if index as usize > length {
                break;
            }
            let c = at(v, index as usize);
            if c != Some('#') {
                index -= 1;
                break;
            }
            subvalue.push('#');
            depth += 1;
        }
        if depth > 6 || depth == 0 || at(v, (index + 1) as usize) == Some('#') {
            return Ok(false);
        }
        length = v.len() + 1;
        let mut queue = String::new();
        let mut character = None;
        loop {
            index += 1;
            if index as usize >= length {
                break;
            }
            character = at(v, index as usize);
            if character != Some(' ') && character != Some('\t') {
                index -= 1;
                break;
            }
            queue.extend(character);
        }
        if queue.is_empty() && character.is_some() && character != Some('\n') {
            return Ok(false);
        }
        if silent {
            return Ok(true);
        }
        subvalue.push_str(&queue);
        queue.clear();
        let mut content = String::new();
        loop {
            index += 1;
            if index as usize >= length {
                break;
            }
            let mut i = index as usize;
            let mut c = at(v, i);
            if c.is_none() || c == Some('\n') {
                break;
            }
            if c != Some(' ') && c != Some('\t') && c != Some('#') {
                content.push_str(&queue);
                content.extend(c);
                queue.clear();
                continue;
            }
            while c == Some(' ') || c == Some('\t') {
                queue.extend(c);
                i += 1;
                c = at(v, i);
            }
            if !content.is_empty() && queue.is_empty() && c == Some('#') {
                content.push('#');
                index = i as isize;
                continue;
            }
            while c == Some('#') {
                queue.push('#');
                i += 1;
                c = at(v, i);
            }
            while c == Some(' ') || c == Some('\t') {
                queue.extend(c);
                i += 1;
                c = at(v, i);
            }
            index = i as isize - 1;
        }
        let pre = subvalue.chars().count();
        now.column += pre;
        now.offset += pre as i64;
        subvalue.push_str(&content);
        subvalue.push_str(&queue);
        let ev = self.eat_str(st, &subvalue);
        let children = self.tokenize_inline(content.chars().collect(), now)?;
        let mut n = node("heading");
        n.depth = depth;
        n.children = children;
        self.apply(st, &ev, n);
        Ok(true)
    }

    fn thematic_break(&mut self, st: &mut TokState, v: &[char], silent: bool) -> R<bool> {
        let length = v.len() + 1;
        let mut index = 0;
        let mut subvalue = String::new();
        let mut c = at(v, 0);
        while index < length {
            c = at(v, index);
            if c != Some('\t') && c != Some(' ') {
                break;
            }
            subvalue.extend(c);
            index += 1;
        }
        let Some(marker @ ('*' | '-' | '_')) = c else {
            return Ok(false);
        };
        subvalue.push(marker);
        let mut count = 1;
        let mut queue = String::new();
        index += 1;
        while index < length {
            let c = at(v, index);
            if c == Some(marker) {
                count += 1;
                subvalue.push_str(&queue);
                subvalue.push(marker);
                queue.clear();
            } else if c == Some(' ') {
                queue.push(' ');
            } else if count >= 3 && (c.is_none() || c == Some('\n')) {
                subvalue.push_str(&queue);
                if silent {
                    return Ok(true);
                }
                let ev = self.eat_str(st, &subvalue);
                self.apply(st, &ev, node("thematicBreak"));
                return Ok(true);
            } else {
                return Ok(false);
            }
            index += 1;
        }
        Ok(false)
    }

    fn setext_heading(&mut self, st: &mut TokState, v: &[char], silent: bool) -> R<bool> {
        let mut now = self.now(st);
        let length = v.len();
        let mut index: isize = -1;
        let mut subvalue = String::new();
        loop {
            index += 1;
            if index as usize >= length {
                break;
            }
            let c = at(v, index as usize);
            if c != Some(' ') || index >= 3 {
                index -= 1;
                break;
            }
            subvalue.push(' ');
        }
        let mut content = String::new();
        let mut queue = String::new();
        loop {
            index += 1;
            if index as usize >= length {
                break;
            }
            let c = at(v, index as usize);
            if c == Some('\n') {
                index -= 1;
                break;
            }
            if c == Some(' ') || c == Some('\t') {
                queue.extend(c);
            } else {
                content.push_str(&queue);
                content.extend(c);
                queue.clear();
            }
        }
        let pre = subvalue.chars().count();
        now.column += pre;
        now.offset += pre as i64;
        subvalue.push_str(&content);
        subvalue.push_str(&queue);
        index += 1;
        let c = at(v, index as usize);
        index += 1;
        let marker = at(v, index as usize);
        if c != Some('\n') || !matches!(marker, Some('=' | '-')) {
            return Ok(false);
        }
        let marker = marker.unwrap_or('=');
        subvalue.push('\n');
        let mut q = String::from(marker);
        let depth = if marker == '=' { 1 } else { 2 };
        loop {
            index += 1;
            if index as usize >= length {
                break;
            }
            let c = at(v, index as usize);
            if c != Some(marker) {
                if c != Some('\n') {
                    return Ok(false);
                }
                break;
            }
            q.push(marker);
        }
        if silent {
            return Ok(true);
        }
        subvalue.push_str(&q);
        let ev = self.eat_str(st, &subvalue);
        let children = self.tokenize_inline(content.chars().collect(), now)?;
        let mut n = node("heading");
        n.depth = depth;
        n.children = children;
        self.apply(st, &ev, n);
        Ok(true)
    }

    fn html_block(&mut self, st: &mut TokState, v: &[char], silent: bool) -> R<bool> {
        let length = v.len();
        let mut index = 0;
        while index < length && matches!(v[index], '\t' | ' ') {
            index += 1;
        }
        if at(v, index) != Some('<') {
            return Ok(false);
        }
        let mut next = index_of(v, "\n", index + 1).unwrap_or(length);
        let line = s(slice(v, index, next));
        let Some(seq) = html_sequences()
            .iter()
            .position(|(open, _, _)| open.is_match(&line))
        else {
            return Ok(false);
        };
        let (_, close, interrupts) = &html_sequences()[seq];
        if silent {
            return Ok(*interrupts);
        }
        index = next;
        if !close.is_match(&line) {
            while index < length {
                next = index_of(v, "\n", index + 1).unwrap_or(length);
                let line = s(slice(v, index + 1, next));
                if close.is_match(&line) {
                    if !line.is_empty() {
                        index = next;
                    }
                    break;
                }
                index = next;
            }
        }
        let sub = s(slice(v, 0, index));
        let ev = self.eat(st, index.min(length));
        let mut n = node("html");
        n.value = sub;
        self.apply(st, &ev, n);
        Ok(true)
    }

    fn footnote_definition(&mut self, st: &mut TokState, v: &[char], silent: bool) -> R<bool> {
        let length = v.len() + 1;
        let mut index = 0;
        while index < length && matches!(at(v, index), Some('\t' | ' ')) {
            index += 1;
        }
        if at(v, index) != Some('[') {
            return Ok(false);
        }
        index += 1;
        if at(v, index) != Some('^') {
            return Ok(false);
        }
        index += 1;
        let label_start = index;
        let mut label_end = None;
        while index < length {
            let c = at(v, index);
            if matches!(c, None | Some('\n' | '\t' | ' ')) {
                return Ok(false);
            }
            if c == Some(']') {
                label_end = Some(index);
                index += 1;
                break;
            }
            index += 1;
        }
        let Some(label_end) = label_end else {
            return Ok(false);
        };
        if label_start == label_end || at(v, index) != Some(':') {
            return Ok(false);
        }
        index += 1;
        if silent {
            return Ok(true);
        }
        let label = s(&v[label_start..label_end]);
        let now = self.now(st);
        struct Line {
            start: usize,
            content_start: usize,
            content_end: usize,
            end: usize,
        }
        let mut start = 0;
        let mut indent: Option<usize> = Some(0);
        let mut content_start: Option<usize> = Some(index);
        let mut lines: Vec<Line> = Vec::new();
        while index < length {
            let c = at(v, index);
            if c.is_none() || c == Some('\n') {
                let mut line = Line {
                    start,
                    content_start: content_start.filter(|&x| x != 0).unwrap_or(index),
                    content_end: index,
                    end: index,
                };
                if c == Some('\n') {
                    start = index + 1;
                    indent = Some(0);
                    content_start = None;
                    line.end = start;
                }
                lines.push(line);
            } else if let Some(ind) = indent {
                if c == Some(' ') || c == Some('\t') {
                    let ni = ind + if c == Some(' ') { 1 } else { 4 - ind % 4 };
                    if ni > 4 {
                        indent = None;
                        content_start = Some(index);
                    } else {
                        indent = Some(ni);
                    }
                } else {
                    if ind < 4
                        && let Some(last) = lines.last()
                    {
                        let rest: Vec<char> = slice(v, index, 1024).to_vec();
                        if last.content_start == last.content_end
                            || self.interrupt(&INTERRUPT_FOOTNOTE, st, &rest)?
                        {
                            break;
                        }
                    }
                    indent = None;
                    content_start = Some(index);
                }
            }
            index += 1;
        }
        let mut n_lines = lines.len();
        while n_lines > 0 {
            let l = &lines[n_lines - 1];
            if l.content_start != l.content_end {
                break;
            }
            n_lines -= 1;
        }
        let end = if n_lines > 0 {
            lines[n_lines - 1].content_end
        } else {
            lines.first().ok_or(())?.content_end
        };
        let ev = self.eat(st, end);
        let mut content = Vec::new();
        for (i, l) in lines.iter().take(n_lines).enumerate() {
            *self.offset.entry(now.line + i).or_insert(0) += (l.content_start - l.start) as i64;
            content.extend_from_slice(slice(v, l.content_start, l.end));
        }
        let prev = self.in_block;
        self.in_block = true;
        let children = self.tokenize_block(content, now)?;
        self.in_block = prev;
        let mut n = node("footnoteDefinition");
        n.identifier = label.to_lowercase();
        n.label = label;
        n.children = children;
        self.apply(st, &ev, n);
        Ok(true)
    }

    fn definition(&mut self, st: &mut TokState, v: &[char], silent: bool) -> R<bool> {
        let mut i = 0;
        while matches!(at(v, i), Some(' ' | '\t')) {
            i += 1;
        }
        if at(v, i) != Some('[') || at(v, i + 1) == Some('^') {
            return Ok(false);
        }
        let length = v.len();
        let mut index = 0;
        let mut subvalue = String::new();
        while index < length && matches!(v[index], ' ' | '\t') {
            subvalue.push(v[index]);
            index += 1;
        }
        if at(v, index) != Some('[') {
            return Ok(false);
        }
        index += 1;
        subvalue.push('[');
        let mut queue = String::new();
        while index < length {
            let mut c = v[index];
            if c == ']' {
                break;
            } else if c == '\\' {
                queue.push('\\');
                index += 1;
                match at(v, index) {
                    Some(x) => c = x,
                    None => {
                        index += 1;
                        continue;
                    }
                }
            }
            queue.push(c);
            index += 1;
        }
        if queue.is_empty() || at(v, index) != Some(']') || at(v, index + 1) != Some(':') {
            return Ok(false);
        }
        let identifier = queue.clone();
        subvalue.push_str(&queue);
        subvalue.push_str("]:");
        index = subvalue.chars().count();
        while index < length && matches!(v[index], '\t' | ' ' | '\n') {
            subvalue.push(v[index]);
            index += 1;
        }
        let mut queue: Vec<char> = Vec::new();
        if at(v, index) == Some('<') {
            index += 1;
            while index < length && !matches!(v[index], '>' | '[' | ']') {
                queue.push(v[index]);
                index += 1;
            }
            if at(v, index) == Some('>') {
                subvalue.push('<');
                subvalue.extend(queue.iter());
                subvalue.push('>');
                index += 1;
            } else {
                return Ok(false);
            }
        }
        if queue.is_empty() {
            while index < length && !matches!(v[index], '[' | ']') && !is_js_whitespace(v[index]) {
                queue.push(v[index]);
                index += 1;
            }
            subvalue.extend(queue.iter());
        }
        if queue.is_empty() {
            return Ok(false);
        }
        let url = queue.clone();
        let mut q = String::new();
        while index < length && matches!(v[index], '\t' | ' ' | '\n') {
            q.push(v[index]);
            index += 1;
        }
        let c = at(v, index);
        let test = match c {
            Some('"') => Some('"'),
            Some('\'') => Some('\''),
            Some('(') => Some(')'),
            _ => None,
        };
        let mut title: Option<Vec<char>> = None;
        match test {
            None => {
                index = subvalue.chars().count();
            }
            Some(t) if !q.is_empty() => {
                subvalue.push_str(&q);
                subvalue.extend(c);
                index = subvalue.chars().count();
                let mut tq: Vec<char> = Vec::new();
                while index < length {
                    let mut ch = v[index];
                    if ch == t {
                        break;
                    }
                    if ch == '\n' {
                        index += 1;
                        match at(v, index) {
                            None => return Ok(false),
                            Some(x) if x == '\n' || x == t => return Ok(false),
                            Some(x) => ch = x,
                        }
                        tq.push('\n');
                    }
                    tq.push(ch);
                    index += 1;
                }
                if at(v, index) != Some(t) {
                    return Ok(false);
                }
                subvalue.extend(tq.iter());
                subvalue.push(t);
                index += 1;
                title = Some(tq);
            }
            Some(_) => return Ok(false),
        }
        while index < length && matches!(v[index], '\t' | ' ') {
            subvalue.push(v[index]);
            index += 1;
        }
        let c = at(v, index);
        if c.is_none() || c == Some('\n') {
            if silent {
                return Ok(true);
            }
            let url = decode_raw(&unescape(&url), false);
            let title = title
                .filter(|t| !t.is_empty())
                .map(|t| decode_raw(&unescape(&t), true));
            let ev = self.eat_str(st, &subvalue);
            let mut n = node("definition");
            n.identifier = normalize(&identifier);
            n.label = identifier;
            n.title = title.filter(|t| !t.is_empty());
            n.url = url;
            self.apply(st, &ev, n);
            return Ok(true);
        }
        Ok(false)
    }

    fn table(&mut self, st: &mut TokState, v: &[char], silent: bool) -> R<bool> {
        let mut index = 0;
        let mut line_count = 0;
        let length = v.len() + 1;
        let mut lines: Vec<Vec<char>> = Vec::new();
        while index < length {
            let line_index = index_of(v, "\n", index).unwrap_or(v.len());
            let pipe = index_of(v, "|", index + 1);
            if pipe.is_none_or(|p| p > line_index) {
                if line_count < 2 {
                    return Ok(false);
                }
                break;
            }
            lines.push(slice(v, index, line_index).to_vec());
            line_count += 1;
            index = line_index + 1;
        }
        let subvalue = lines.join(&'\n');
        let alignments = if lines.len() > 1 {
            lines.remove(1)
        } else {
            Vec::new()
        };
        line_count -= 1;
        let mut alignment: Option<Option<&'static str>> = None;
        let mut align: Vec<Option<&'static str>> = Vec::new();
        let mut has_dash = false;
        let mut first: Option<bool> = None;
        for &c in &alignments {
            if c == '|' {
                has_dash = false;
                match alignment.take() {
                    None => {
                        if first == Some(false) {
                            return Ok(false);
                        }
                    }
                    Some(a) => align.push(a),
                }
                first = Some(false);
            } else if c == '-' {
                has_dash = true;
                alignment = Some(alignment.flatten());
            } else if c == ':' {
                alignment = Some(Some(if alignment == Some(Some("left")) {
                    "center"
                } else if has_dash && alignment == Some(None) {
                    "right"
                } else {
                    "left"
                }));
            } else if !is_js_whitespace(c) {
                return Ok(false);
            }
        }
        if let Some(a) = alignment {
            align.push(a);
        }
        if align.is_empty() {
            return Ok(false);
        }
        if silent {
            return Ok(true);
        }
        let ev = self.eat(st, subvalue.len());
        let mut t = node("table");
        t.align = align;
        let table_idx = self.reset(st, &ev, t);
        for (position, line) in lines.iter().take(line_count).enumerate() {
            if position > 0 {
                self.eat(st, 1);
            }
            let ev = self.eat(st, line.len());
            let row_idx = {
                let mut r = node("tableRow");
                self.pos(st, &ev, &mut r);
                st.tokens[table_idx].children.push(r);
                let i = st.tokens[table_idx].children.len() - 1;
                let now = self.now(st);
                st.tokens[table_idx].children[i].end = now;
                i
            };
            st.line = ev.line;
            st.column = ev.column;
            st.pos = ev.pos;
            let len = line.len() + 1;
            let mut index = 0;
            let mut queue = String::new();
            let mut cell = String::new();
            let mut preamble = true;
            while index < len {
                let c = at(line, index);
                if c == Some('\t') || c == Some(' ') {
                    if !cell.is_empty() {
                        queue.extend(c);
                    } else {
                        self.eat(st, 1);
                    }
                    index += 1;
                    continue;
                }
                if c.is_none() || c == Some('|') {
                    if preamble {
                        self.eat(st, usize::from(c.is_some()));
                    } else {
                        if !cell.is_empty() || c.is_some() {
                            let mut sub = cell.clone();
                            let qc: Vec<char> = queue.chars().collect();
                            if qc.len() > 1 {
                                if c.is_some() {
                                    sub.extend(&qc[..qc.len() - 1]);
                                    queue = qc[qc.len() - 1].to_string();
                                } else {
                                    sub.push_str(&queue);
                                    queue.clear();
                                }
                            }
                            let now = self.now(st);
                            let ev = self.eat_str(st, &sub);
                            let children = self.tokenize_inline(cell.chars().collect(), now)?;
                            let mut cn = node("tableCell");
                            cn.children = children;
                            self.pos(st, &ev, &mut cn);
                            let row = &mut st.tokens[table_idx].children[row_idx];
                            row.children.push(cn);
                        }
                        let n = queue.chars().count() + usize::from(c.is_some());
                        self.eat(st, n);
                        queue.clear();
                        cell.clear();
                    }
                } else {
                    if !queue.is_empty() {
                        cell.push_str(&queue);
                        queue.clear();
                    }
                    cell.extend(c);
                    if c == Some('\\') && index != len - 2 {
                        cell.extend(at(line, index + 1));
                        index += 1;
                    }
                }
                preamble = false;
                index += 1;
            }
            if position == 0 {
                self.eat(st, 1 + alignments.len());
            }
        }
        if self.at_start && !st.tokens.is_empty() {
            self.at_start = false;
        }
        Ok(true)
    }

    fn paragraph(&mut self, st: &mut TokState, v: &[char], silent: bool) -> R<bool> {
        let length = v.len();
        let mut index = index_of(v, "\n", 0);
        loop {
            let Some(i) = index else {
                index = Some(length);
                break;
            };
            if i >= length {
                break;
            }
            if at(v, i + 1) == Some('\n') {
                break;
            }
            let mut size = 0;
            let mut position = i + 1;
            let mut c = None;
            while position < length {
                c = at(v, position);
                if c == Some('\t') {
                    size = 4;
                    break;
                } else if c == Some(' ') {
                    size += 1;
                } else {
                    break;
                }
                position += 1;
            }
            if size >= 4 && c != Some('\n') {
                index = index_of(v, "\n", i + 1);
                continue;
            }
            let sub = v[i + 1..].to_vec();
            if self.interrupt(&INTERRUPT_PARAGRAPH, st, &sub)? {
                break;
            }
            let position = i;
            index = index_of(v, "\n", i + 1);
            if let Some(ni) = index
                && trim_chars(slice(v, position, ni)).is_empty()
            {
                index = Some(position);
                break;
            }
        }
        let end = index.unwrap_or(length).min(length);
        if silent {
            return Ok(true);
        }
        let now = self.now(st);
        let sub = trim_trailing_lines(&v[..end]).to_vec();
        let ev = self.eat(st, sub.len());
        let children = self.tokenize_inline(sub, now)?;
        let mut n = node("paragraph");
        n.children = children;
        self.apply(st, &ev, n);
        Ok(true)
    }

    fn list(&mut self, st: &mut TokState, v: &[char], silent: bool) -> R<bool> {
        let length = v.len();
        let mut index = 0;
        while index < length && matches!(v[index], '\t' | ' ') {
            index += 1;
        }
        let c = at(v, index);
        let ordered;
        let marker;
        let mut start = None;
        if matches!(c, Some('*' | '+' | '-')) {
            marker = c;
            ordered = false;
        } else {
            ordered = true;
            let mut queue = String::new();
            while index < length && v[index].is_ascii_digit() {
                queue.push(v[index]);
                index += 1;
            }
            let c = at(v, index);
            if queue.is_empty() || !matches!(c, Some('.' | ')')) {
                return Ok(false);
            }
            if silent && queue != "1" {
                return Ok(false);
            }
            // `parseInt(queue, 10)`: a double, rounded past 2^53 and Infinity past ~1e308.
            start = Some(queue.parse::<f64>().unwrap_or(f64::INFINITY));
            marker = c;
        }
        index += 1;
        let c = at(v, index);
        if c != Some(' ') && c != Some('\t') && c != Some('\n') && c.is_some() {
            return Ok(false);
        }
        if silent {
            return Ok(true);
        }
        struct Item {
            value: Vec<Vec<char>>,
            indent: usize,
            trail: Vec<Vec<char>>,
        }
        index = 0;
        let mut items: Vec<Item> = Vec::new();
        let mut all_lines: Vec<Vec<char>> = Vec::new();
        let mut empty_lines: Vec<Vec<char>> = Vec::new();
        let mut spread = false;
        let mut empty = false;
        while index < length {
            let next = index_of(v, "\n", index).unwrap_or(length);
            let start_index = index;
            let prefixed;
            let mut indented = false;
            let mut size = 0;
            while index < length {
                let c = v[index];
                if c == '\t' {
                    size += 4 - size % 4;
                } else if c == ' ' {
                    size += 1;
                } else {
                    break;
                }
                index += 1;
            }
            if let Some(item) = items.last()
                && size >= item.indent
            {
                indented = true;
            }
            let mut current_marker = None;
            if !indented {
                let c = at(v, index);
                if matches!(c, Some('*' | '+' | '-')) {
                    current_marker = c;
                    index += 1;
                    size += 1;
                } else {
                    let mut queue = String::new();
                    while index < length && v[index].is_ascii_digit() {
                        queue.push(v[index]);
                        index += 1;
                    }
                    let c = at(v, index);
                    index += 1;
                    if !queue.is_empty() && matches!(c, Some('.' | ')')) {
                        current_marker = c;
                        size += queue.len() + 1;
                    }
                }
                if current_marker.is_some() {
                    let c = at(v, index);
                    if c == Some('\t') {
                        size += 4 - size % 4;
                        index += 1;
                    } else if c == Some(' ') {
                        let end = index + 4;
                        while index < end {
                            if at(v, index) != Some(' ') {
                                break;
                            }
                            index += 1;
                            size += 1;
                        }
                        if index == end && at(v, index) == Some(' ') {
                            index -= 3;
                            size -= 3;
                        }
                    } else if c != Some('\n') && c.is_some() {
                        current_marker = None;
                    }
                }
            }
            if current_marker.is_some() {
                if marker != current_marker {
                    break;
                }
                prefixed = true;
            } else {
                if let Some(item) = items.last() {
                    indented = size >= item.indent || size > 4;
                }
                prefixed = false;
                index = start_index;
            }
            let line = slice(v, start_index, next).to_vec();
            let content = if start_index == index {
                line.clone()
            } else {
                slice(v, index, next).to_vec()
            };
            if matches!(current_marker, Some('*' | '_' | '-'))
                && self.thematic_break(st, &line, true)?
            {
                break;
            }
            let previous_empty = empty;
            empty = !prefixed && trim_chars(&content).is_empty();
            if indented && !items.is_empty() {
                let item = items.last_mut().ok_or(())?;
                item.value.append(&mut empty_lines.clone());
                item.value.push(line.clone());
                all_lines.append(&mut empty_lines);
                all_lines.push(line);
            } else if prefixed {
                if !empty_lines.is_empty() {
                    spread = true;
                    let item = items.last_mut().ok_or(())?;
                    item.value.push(Vec::new());
                    item.trail = empty_lines.clone();
                }
                items.push(Item {
                    value: vec![line.clone()],
                    indent: size,
                    trail: Vec::new(),
                });
                all_lines.append(&mut empty_lines);
                all_lines.push(line);
            } else if empty {
                empty_lines.push(line);
            } else {
                if previous_empty {
                    break;
                }
                if self.interrupt(&INTERRUPT_LIST, st, &line)? {
                    break;
                }
                let item = items.last_mut().ok_or(())?;
                item.value.append(&mut empty_lines.clone());
                item.value.push(line.clone());
                all_lines.append(&mut empty_lines);
                all_lines.push(line);
            }
            index = next + 1;
        }
        let ev = self.eat(st, all_lines.join(&'\n').len());
        let mut list = node("list");
        list.ordered = ordered;
        list.start_num = start;
        list.spread = spread;
        let list_idx = self.reset(st, &ev, list);
        let prev_list = self.in_list;
        let prev_block = self.in_block;
        self.in_list = true;
        self.in_block = true;
        let count = items.len();
        for (i, item) in items.iter().enumerate() {
            let value = item.value.join(&'\n');
            let now = self.now(st);
            let ev = self.eat(st, value.len());
            let li = self.list_item(value, now)?;
            let mut li = li;
            self.pos(st, &ev, &mut li);
            st.tokens[list_idx].children.push(li);
            let mut trail = item.trail.join(&'\n');
            if i != count - 1 {
                trail.push('\n');
            }
            self.eat(st, trail.len());
        }
        self.in_list = prev_list;
        self.in_block = prev_block;
        Ok(true)
    }

    fn list_item(&mut self, value: Vec<char>, pos: Pt) -> R<Node> {
        let mut value = self.normal_list_item(value, pos)?;
        let mut checked = None;
        if value.len() >= 4
            && value[0] == '['
            && matches!(value[1], ' ' | 'X' | '\t' | 'x')
            && value[2] == ']'
            && matches!(value[3], ' ' | '\t')
        {
            checked = Some(value[1].eq_ignore_ascii_case(&'x'));
            *self.offset.entry(pos.line).or_insert(0) += 4;
            value = value[4..].to_vec();
        }
        let spread = {
            let mut found = false;
            for i in 0..value.len().saturating_sub(1) {
                if value[i] == '\n'
                    && value[i + 1] == '\n'
                    && !value[i + 2..].iter().all(|c| is_js_whitespace(*c))
                {
                    found = true;
                    break;
                }
            }
            found
        };
        let children = self.tokenize_block(value, pos)?;
        let mut n = node("listItem");
        n.spread = spread;
        n.checked = checked;
        n.children = children;
        Ok(n)
    }

    fn normal_list_item(&mut self, value: Vec<char>, pos: Pt) -> R<Vec<char>> {
        let mut line = pos.line;
        let mut i = 0;
        while i < value.len() && matches!(value[i], ' ' | '\t') {
            i += 1;
        }
        let p1 = s(&value[..i]);
        let ms = i;
        if matches!(at(&value, i), Some('*' | '+' | '-')) {
            i += 1;
        } else {
            while i < value.len() && value[i].is_ascii_digit() {
                i += 1;
            }
            if i == ms || !matches!(at(&value, i), Some('.' | ')')) {
                return Err(());
            }
            i += 1;
        }
        let p2 = s(&value[ms..i]);
        let ss = i;
        let spaces = value[i..].iter().take_while(|&&c| c == ' ').count();
        let p3: String = if (1..=4).contains(&spaces) {
            " ".repeat(spaces)
        } else if spaces > 4 {
            " ".to_string()
        } else if at(&value, i) == Some('\t') {
            "\t".to_string()
        } else if i == value.len() || at(&value, i) == Some('\n') {
            String::new()
        } else {
            return Err(());
        };
        i = ss + p3.chars().count();
        let line_end = index_of(&value, "\n", i).unwrap_or(value.len());
        let rest = s(&value[i..line_end]);
        let bullet = format!("{p1}{p2}{p3}");
        let mut p2b = p2.clone();
        if p2.parse::<f64>().is_ok_and(|n| n < 10.0) && bullet.chars().count() % 2 == 1 {
            p2b = format!(" {p2}");
        }
        let max = format!("{p1}{}{p3}", " ".repeat(p2b.chars().count()));
        let mut replaced: Vec<char> = format!("{max}{rest}").chars().collect();
        replaced.extend_from_slice(&value[line_end..]);
        let lines: Vec<&[char]> = replaced.split(|&c| c == '\n').collect();
        let max_chars: Vec<char> = max.chars().collect();
        let (max_indent, _) = get_indentation(&max_chars);
        let trimmed = remove_indentation(&replaced, max_indent);
        let mut trimmed_lines: Vec<Vec<char>> = trimmed
            .split(|&c| c == '\n')
            .map(<[char]>::to_vec)
            .collect();
        trimmed_lines[0] = rest.chars().collect();
        *self.offset.entry(line).or_insert(0) += bullet.chars().count() as i64;
        line += 1;
        for (idx, l) in lines.iter().enumerate().skip(1) {
            let diff = l.len() as i64 - trimmed_lines.get(idx).map_or(0, Vec::len) as i64;
            *self.offset.entry(line).or_insert(0) += diff;
            line += 1;
        }
        Ok(trimmed_lines.join(&'\n'))
    }

    fn inline(&mut self, m: I, st: &mut TokState, v: &[char]) -> R<bool> {
        match m {
            I::Escape => self.escape(st, v),
            I::AutoLink => self.auto_link(st, v),
            I::Url => self.url(st, v),
            I::Email => self.email(st, v),
            I::Html => self.inline_html(st, v),
            I::WikiLink => self.wiki_link(st, v),
            I::Link => self.link(st, v),
            I::FootnoteCall => self.footnote_call(st, v),
            I::Reference => self.reference(st, v),
            I::Strong => self.strong(st, v),
            I::Emphasis => self.emphasis(st, v),
            I::Deletion => self.deletion(st, v),
            I::Code => self.inline_code(st, v),
            I::Break => self.hard_break(st, v),
            I::Math => self.inline_math(st, v),
            I::Liquid => self.liquid(st, v),
            I::Text => self.text(st, v),
        }
    }

    fn locate(&self, m: I, v: &[char], from: usize) -> Option<usize> {
        match m {
            I::Escape => index_of(v, "\\", from),
            I::AutoLink | I::Html => index_of(v, "<", from),
            I::Url => ["www.", "http://", "https://"]
                .iter()
                .filter_map(|p| index_of(v, p, from))
                .min(),
            I::Email => locate_email(v, from),
            I::WikiLink | I::FootnoteCall => index_of(v, "[", from),
            I::Link | I::Reference => {
                let link = index_of(v, "[", from);
                let image = index_of(v, "![", from);
                match (link, image) {
                    (l, None) => l,
                    (Some(l), Some(i)) => Some(if l < i { l } else { i }),
                    (None, Some(i)) => Some(i),
                }
            }
            I::Strong => min_found(index_of(v, "**", from), index_of(v, "__", from)),
            I::Emphasis => min_found(index_of(v, "*", from), index_of(v, "_", from)),
            I::Deletion => index_of(v, "~~", from),
            I::Code => index_of(v, "`", from),
            I::Break => {
                let mut index = index_of(v, "\n", from)?;
                while index > from {
                    if v[index - 1] != ' ' {
                        break;
                    }
                    index -= 1;
                }
                Some(index)
            }
            I::Math => index_of(v, "$", from),
            I::Liquid => index_of(v, "{", from),
            I::Text => None,
        }
    }

    fn text(&mut self, st: &mut TokState, v: &[char]) -> R<bool> {
        let mut min = v.len();
        if !self.text_only {
            for m in INLINE_METHODS {
                if m == I::Text {
                    continue;
                }
                if let Some(p) = self.locate(m, v, 1)
                    && p < min
                {
                    min = p;
                }
            }
        }
        let sub = &v[..min];
        for (content, source) in decode_pieces(sub, true) {
            let n_eat = source.as_ref().unwrap_or(&content).chars().count();
            let ev = self.eat(st, n_eat);
            let mut n = node("text");
            n.value = content;
            self.apply(st, &ev, n);
        }
        Ok(true)
    }

    fn escape(&mut self, st: &mut TokState, v: &[char]) -> R<bool> {
        if at(v, 0) == Some('\\')
            && let Some(c) = at(v, 1)
            && ESCAPES.contains(c)
        {
            let ev = self.eat(st, 2);
            let n = if c == '\n' {
                node("break")
            } else {
                let mut n = node("text");
                n.value = c.to_string();
                n
            };
            self.apply(st, &ev, n);
            return Ok(true);
        }
        Ok(false)
    }

    fn inline_children_text_only(&mut self, content: Vec<char>, now: Pt) -> R<Vec<Node>> {
        let prev_tokenizers = self.text_only;
        let prev_link = self.in_link;
        self.in_link = true;
        self.text_only = true;
        let r = self.tokenize_inline(content, now);
        self.text_only = prev_tokenizers;
        self.in_link = prev_link;
        r
    }

    fn auto_link(&mut self, st: &mut TokState, v: &[char]) -> R<bool> {
        if at(v, 0) != Some('<') {
            return Ok(false);
        }
        let length = v.len();
        let mut index = 1;
        let mut queue = String::new();
        while index < length {
            let c = v[index];
            if is_js_whitespace(c)
                || c == '>'
                || c == '@'
                || (c == ':' && at(v, index + 1) == Some('/'))
            {
                break;
            }
            queue.push(c);
            index += 1;
        }
        if queue.is_empty() {
            return Ok(false);
        }
        let mut link = queue;
        let mut queue = String::new();
        let c = at(v, index);
        link.extend(c);
        index += 1;
        let has_at = c == Some('@');
        if !has_at {
            if c != Some(':') || at(v, index + 1) != Some('/') {
                return Ok(false);
            }
            link.push('/');
            index += 1;
        }
        while index < length {
            let c = v[index];
            if is_js_whitespace(c) || c == '>' {
                break;
            }
            queue.push(c);
            index += 1;
        }
        if queue.is_empty() || at(v, index) != Some('>') {
            return Ok(false);
        }
        link.push_str(&queue);
        let mut content = link.clone();
        let subvalue = format!("<{link}>");
        let mut now = self.now(st);
        now.column += 1;
        now.offset += 1;
        if has_at {
            if link.chars().take(7).collect::<String>().to_lowercase() == "mailto:" {
                content = content.chars().skip(7).collect();
                now.column += 7;
                now.offset += 7;
            } else {
                link = format!("mailto:{link}");
            }
        }
        let children = self.inline_children_text_only(content.chars().collect(), now)?;
        let ev = self.eat_str(st, &subvalue);
        let mut n = node("link");
        n.url = decode_raw(&link.chars().collect::<Vec<_>>(), false);
        n.children = children;
        self.apply(st, &ev, n);
        Ok(true)
    }

    fn url(&mut self, st: &mut TokState, v: &[char]) -> R<bool> {
        let length = v.len();
        let head4 = s(slice(v, 0, 4));
        let protocolless;
        let mut index;
        if head4 == "www." {
            protocolless = true;
            index = 4;
        } else if s(slice(v, 0, 7)).to_lowercase() == "http://" {
            protocolless = false;
            index = 7;
        } else if s(slice(v, 0, 8)).to_lowercase() == "https://" {
            protocolless = false;
            index = 8;
        } else {
            return Ok(false);
        }
        let mut previous_dot = index as isize - 1;
        let start = index;
        let mut dots: Vec<usize> = Vec::new();
        let mut code = None;
        while index < length {
            code = at(v, index);
            if code == Some('.') {
                if previous_dot == index as isize - 1 {
                    break;
                }
                dots.push(index);
                previous_dot = index as isize;
                index += 1;
                continue;
            }
            if decimal(code) || alphabetical(code) || code == Some('-') || code == Some('_') {
                index += 1;
                continue;
            }
            break;
        }
        if code == Some('.') {
            dots.pop();
            index -= 1;
        }
        if dots.is_empty() {
            return Ok(false);
        }
        let last_two = if dots.len() < 2 {
            start
        } else {
            dots[dots.len() - 2] + 1
        };
        if slice(v, last_two, index).contains(&'_') {
            return Ok(false);
        }
        let mut end = index;
        let path_start = index;
        while index < length {
            let c = v[index];
            if is_js_whitespace(c) || c == '<' {
                break;
            }
            index += 1;
            if !matches!(c, '!' | '*' | ',' | '.' | ':' | '?' | '_' | '~') {
                end = index;
            }
        }
        index = end;
        if index > 0 && v[index - 1] == ')' {
            let mut path = slice(v, path_start, index).to_vec();
            let left = path.iter().filter(|&&c| c == '(').count();
            let mut right = path.iter().filter(|&&c| c == ')').count();
            while right > left {
                let last = path.iter().rposition(|&c| c == ')').unwrap_or(0);
                index = path_start + last;
                path = slice(v, path_start, index).to_vec();
                right -= 1;
            }
        }
        if index > 0 && v[index - 1] == ';' {
            index -= 1;
            if index > 0 && alphabetical(at(v, index - 1)) {
                let mut e = index as isize - 2;
                while e >= 0 && alphabetical(at(v, e as usize)) {
                    e -= 1;
                }
                if e >= 0 && at(v, e as usize) == Some('&') {
                    index = e as usize;
                }
            }
        }
        let content = slice(v, 0, index).to_vec();
        let mut url = decode_raw(&content, false);
        if protocolless {
            url = format!("http://{url}");
        }
        let now = self.now(st);
        let children = self.inline_children_text_only(content.clone(), now)?;
        let ev = self.eat(st, content.len());
        let mut n = node("link");
        n.url = url;
        n.children = children;
        self.apply(st, &ev, n);
        Ok(true)
    }

    fn email(&mut self, st: &mut TokState, v: &[char]) -> R<bool> {
        let length = v.len();
        let mut index = 0;
        let mut code = at(v, 0);
        while decimal(code) || alphabetical(code) || matches!(code, Some('+' | '-' | '.' | '_')) {
            index += 1;
            code = at(v, index);
        }
        if index == 0 || code != Some('@') {
            return Ok(false);
        }
        index += 1;
        let mut first_dot: Option<usize> = None;
        while index < length {
            code = at(v, index);
            if decimal(code) || alphabetical(code) || matches!(code, Some('-' | '.' | '_')) {
                index += 1;
                if first_dot.is_none() && code == Some('.') {
                    first_dot = Some(index);
                }
                continue;
            }
            break;
        }
        let Some(fd) = first_dot else {
            return Ok(false);
        };
        if fd == index || code == Some('-') || code == Some('_') {
            return Ok(false);
        }
        if code == Some('.') {
            index -= 1;
        }
        let content = slice(v, 0, index).to_vec();
        let now = self.now(st);
        let children = self.inline_children_text_only(content.clone(), now)?;
        let ev = self.eat(st, content.len());
        let mut n = node("link");
        n.url = format!("mailto:{}", decode_raw(&content, false));
        n.children = children;
        self.apply(st, &ev, n);
        Ok(true)
    }

    fn inline_html(&mut self, st: &mut TokState, v: &[char]) -> R<bool> {
        if at(v, 0) != Some('<') || v.len() < 3 {
            return Ok(false);
        }
        let c = at(v, 1);
        if !alphabetical(c) && !matches!(c, Some('?' | '!' | '/')) {
            return Ok(false);
        }
        let text = s(v);
        let Some(m) = html_tag().and_then(|r| r.find(&text)) else {
            return Ok(false);
        };
        let sub = m.as_str().to_string();
        let lower = sub.to_ascii_lowercase();
        if !self.in_link && lower.starts_with("<a ") {
            self.in_link = true;
        } else if self.in_link && lower.starts_with("</a>") {
            self.in_link = false;
        }
        let ev = self.eat_str(st, &sub);
        let mut n = node("html");
        n.value = sub;
        self.apply(st, &ev, n);
        Ok(true)
    }

    fn wiki_link(&mut self, st: &mut TokState, v: &[char]) -> R<bool> {
        if at(v, 0) != Some('[') || at(v, 1) != Some('[') {
            return Ok(false);
        }
        let Some(close) = index_of(v, "]]", 3) else {
            return Ok(false);
        };
        let contents = js_trim(&s(&v[2..close])).to_string();
        let ev = self.eat(st, close + 2);
        let mut n = node("wikiLink");
        n.value = contents;
        self.apply(st, &ev, n);
        Ok(true)
    }

    fn link(&mut self, st: &mut TokState, v: &[char]) -> R<bool> {
        let mut index = 0;
        let mut c = at(v, 0);
        let mut subvalue = String::new();
        let mut is_image = false;
        if c == Some('!') {
            is_image = true;
            subvalue.push('!');
            index += 1;
            c = at(v, index);
        }
        if c != Some('[') {
            return Ok(false);
        }
        if !is_image && self.in_link {
            return Ok(false);
        }
        subvalue.push('[');
        let mut queue = String::new();
        index += 1;
        let length = v.len();
        let mut now = self.now(st);
        let mut depth: i64 = 0;
        now.column += index;
        now.offset += index as i64;
        let mut opening: usize = 0;
        let mut closed = false;
        let mut subqueue = String::new();
        while index < length {
            let ch = v[index];
            subqueue = ch.to_string();
            if ch == '`' {
                let mut count = 1;
                while at(v, index + 1) == Some('`') {
                    subqueue.push('`');
                    index += 1;
                    count += 1;
                }
                if opening == 0 {
                    opening = count;
                } else if count >= opening {
                    opening = 0;
                }
            } else if ch == '\\' {
                index += 1;
                subqueue.extend(at(v, index));
            } else if ch == '[' {
                depth += 1;
            } else if ch == ']' {
                if depth > 0 {
                    depth -= 1;
                } else {
                    if at(v, index + 1) != Some('(') {
                        return Ok(false);
                    }
                    subqueue.push('(');
                    closed = true;
                    index += 1;
                    break;
                }
            }
            queue.push_str(&subqueue);
            subqueue.clear();
            index += 1;
        }
        if !closed {
            return Ok(false);
        }
        let content = queue;
        subvalue.push_str(&content);
        subvalue.push_str(&subqueue);
        index += 1;
        while index < length && is_js_whitespace(v[index]) {
            subvalue.push(v[index]);
            index += 1;
        }
        let url: String;
        let mut queue = String::new();
        if at(v, index) == Some('<') {
            index += 1;
            while index < length {
                let ch = v[index];
                if ch == '>' {
                    break;
                }
                if ch == '\n' {
                    return Ok(false);
                }
                queue.push(ch);
                index += 1;
            }
            if at(v, index) != Some('>') {
                return Ok(false);
            }
            subvalue.push('<');
            subvalue.push_str(&queue);
            subvalue.push('>');
            url = queue.clone();
            index += 1;
        } else {
            let mut subq = String::new();
            while index < length {
                let mut ch = v[index];
                if !subq.is_empty() && matches!(ch, '"' | '\'' | '(') {
                    break;
                }
                if is_js_whitespace(ch) {
                    break;
                }
                if ch == '(' {
                    depth += 1;
                } else if ch == ')' {
                    if depth == 0 {
                        break;
                    }
                    depth -= 1;
                }
                queue.push_str(&subq);
                subq.clear();
                if ch == '\\' {
                    queue.push('\\');
                    index += 1;
                    match at(v, index) {
                        Some(x) => ch = x,
                        None => {
                            index += 1;
                            continue;
                        }
                    }
                }
                queue.push(ch);
                index += 1;
            }
            subvalue.push_str(&queue);
            url = queue.clone();
            index = subvalue.chars().count();
        }
        let mut q = String::new();
        while index < length && is_js_whitespace(v[index]) {
            q.push(v[index]);
            index += 1;
        }
        let c = at(v, index);
        subvalue.push_str(&q);
        let mut title: Option<String> = None;
        if !q.is_empty() && matches!(c, Some('"' | '\'' | '(')) {
            index += 1;
            subvalue.extend(c);
            let marker = if c == Some('(') {
                ')'
            } else {
                c.unwrap_or('"')
            };
            let mut tq = String::new();
            while index < length {
                let mut ch = v[index];
                if ch == marker {
                    break;
                }
                if ch == '\\' {
                    tq.push('\\');
                    index += 1;
                    match at(v, index) {
                        Some(x) => ch = x,
                        None => {
                            index += 1;
                            continue;
                        }
                    }
                }
                index += 1;
                tq.push(ch);
            }
            if at(v, index) != Some(marker) {
                return Ok(false);
            }
            subvalue.push_str(&tq);
            subvalue.push(marker);
            title = Some(tq);
            index += 1;
            while index < length && is_js_whitespace(v[index]) {
                subvalue.push(v[index]);
                index += 1;
            }
        }
        if at(v, index) != Some(')') {
            return Ok(false);
        }
        subvalue.push(')');
        let url = decode_raw(&unescape(&url.chars().collect::<Vec<_>>()), false);
        let title = title
            .filter(|t| !t.is_empty())
            .map(|t| decode_raw(&unescape(&t.chars().collect::<Vec<_>>()), true));
        let mut n = node(if is_image { "image" } else { "link" });
        n.title = title.filter(|t| !t.is_empty());
        n.url = url;
        let content_chars: Vec<char> = content.chars().collect();
        if is_image {
            let alt = decode_raw(&unescape(&content_chars), true);
            n.alt = if alt.is_empty() { None } else { Some(alt) };
        } else {
            let prev = self.in_link;
            self.in_link = true;
            n.children = self.tokenize_inline(content_chars, now)?;
            self.in_link = prev;
        }
        let ev = self.eat_str(st, &subvalue);
        self.apply(st, &ev, n);
        Ok(true)
    }

    fn footnote_call(&mut self, st: &mut TokState, v: &[char]) -> R<bool> {
        if at(v, 0) != Some('[') || at(v, 1) != Some('^') {
            return Ok(false);
        }
        let mut index = 2;
        let start = index;
        let mut end = None;
        while index < v.len() + 1 {
            let c = at(v, index);
            if matches!(c, None | Some('\n' | '\t' | ' ')) {
                return Ok(false);
            }
            if c == Some(']') {
                end = Some(index);
                index += 1;
                break;
            }
            index += 1;
        }
        let Some(end) = end else {
            return Ok(false);
        };
        if start == end {
            return Ok(false);
        }
        let label = s(&v[start..end]);
        let ev = self.eat(st, index);
        let mut n = node("footnoteReference");
        n.identifier = label.to_lowercase();
        n.label = label;
        self.apply(st, &ev, n);
        Ok(true)
    }

    fn reference(&mut self, st: &mut TokState, v: &[char]) -> R<bool> {
        let mut index = 0;
        if at(v, 0) == Some('!') {
            index = 1;
        }
        if at(v, index) != Some('[') || at(v, index + 1) == Some('^') {
            return Ok(false);
        }
        let mut c = at(v, 0);
        let mut index = 0;
        let length = v.len();
        let mut intro = String::new();
        let mut is_link = true;
        if c == Some('!') {
            is_link = false;
            intro.push('!');
            index += 1;
            c = at(v, index);
        }
        if c != Some('[') {
            return Ok(false);
        }
        index += 1;
        intro.push('[');
        let mut queue = String::new();
        let mut depth = 0;
        let mut bracketed = false;
        while index < length {
            let mut ch = v[index];
            if ch == '[' {
                bracketed = true;
                depth += 1;
            } else if ch == ']' {
                if depth == 0 {
                    break;
                }
                depth -= 1;
            }
            if ch == '\\' {
                queue.push('\\');
                index += 1;
                match at(v, index) {
                    Some(x) => ch = x,
                    None => {
                        index += 1;
                        continue;
                    }
                }
            }
            queue.push(ch);
            index += 1;
        }
        let mut subvalue = queue.clone();
        let content = queue;
        if at(v, index) != Some(']') {
            return Ok(false);
        }
        index += 1;
        subvalue.push(']');
        let mut q = String::new();
        let mut reference_type = "shortcut";
        let identifier: String = if at(v, index) == Some('[') {
            let mut ident = String::new();
            q.push('[');
            index += 1;
            while index < length {
                let mut ch = v[index];
                if ch == '[' || ch == ']' {
                    break;
                }
                if ch == '\\' {
                    ident.push('\\');
                    index += 1;
                    match at(v, index) {
                        Some(x) => ch = x,
                        None => {
                            index += 1;
                            continue;
                        }
                    }
                }
                ident.push(ch);
                index += 1;
            }
            if at(v, index) == Some(']') {
                reference_type = if ident.is_empty() {
                    "collapsed"
                } else {
                    "full"
                };
                q.push_str(&ident);
                q.push(']');
            } else {
                ident.clear();
            }
            subvalue.push_str(&q);
            ident
        } else {
            if content.is_empty() {
                return Ok(false);
            }
            content.clone()
        };
        if reference_type != "full" && bracketed {
            return Ok(false);
        }
        let subvalue = format!("{intro}{subvalue}");
        if is_link && self.in_link {
            return Ok(false);
        }
        let mut now = self.now(st);
        let il = intro.chars().count();
        now.column += il;
        now.offset += il as i64;
        let identifier = if reference_type == "full" {
            identifier
        } else {
            content.clone()
        };
        let mut n = node(if is_link {
            "linkReference"
        } else {
            "imageReference"
        });
        n.identifier = normalize(&identifier);
        n.label = identifier;
        n.reference_type = reference_type;
        let cc: Vec<char> = content.chars().collect();
        if is_link {
            let prev = self.in_link;
            self.in_link = true;
            n.children = self.tokenize_inline(cc, now)?;
            self.in_link = prev;
        } else {
            let alt = decode_raw(&unescape(&cc), true);
            n.alt = if alt.is_empty() { None } else { Some(alt) };
        }
        let ev = self.eat_str(st, &subvalue);
        self.apply(st, &ev, n);
        Ok(true)
    }

    fn strong(&mut self, st: &mut TokState, v: &[char]) -> R<bool> {
        let c = at(v, 0);
        if !matches!(c, Some('*' | '_')) || at(v, 1) != c {
            return Ok(false);
        }
        let marker = c.unwrap_or('*');
        let length = v.len();
        let mut index = 2;
        let mut queue = String::new();
        while index < length {
            let mut ch = v[index];
            if ch == marker && at(v, index + 1) == Some(marker) && at(v, index + 2) != Some(marker)
            {
                if js_trim(&queue).is_empty() {
                    return Ok(false);
                }
                let mut now = self.now(st);
                now.column += 2;
                now.offset += 2;
                let sub = format!("{marker}{marker}{queue}{marker}{marker}");
                let children = self.tokenize_inline(queue.chars().collect(), now)?;
                let ev = self.eat_str(st, &sub);
                let mut n = node("strong");
                n.children = children;
                self.apply(st, &ev, n);
                return Ok(true);
            }
            if ch == '\\' {
                queue.push('\\');
                index += 1;
                match at(v, index) {
                    Some(x) => ch = x,
                    None => {
                        index += 1;
                        continue;
                    }
                }
            }
            queue.push(ch);
            index += 1;
        }
        Ok(false)
    }

    fn emphasis(&mut self, st: &mut TokState, v: &[char]) -> R<bool> {
        let c = at(v, 0);
        if !matches!(c, Some('*' | '_')) {
            return Ok(false);
        }
        let marker = c.unwrap_or('*');
        let length = v.len();
        let mut index = 1;
        let mut queue = String::new();
        let mut character: Option<char> = None;
        while index < length {
            let previous = character;
            character = at(v, index);
            if character == Some(marker) {
                index += 1;
                character = at(v, index);
                if character != Some(marker) {
                    if js_trim(&queue).is_empty() || previous == Some(marker) {
                        return Ok(false);
                    }
                    if marker == '_' && word_char(character) {
                        queue.push(marker);
                        continue;
                    }
                    let mut now = self.now(st);
                    now.column += 1;
                    now.offset += 1;
                    let sub = format!("{marker}{queue}{marker}");
                    let children = self.tokenize_inline(queue.chars().collect(), now)?;
                    let ev = self.eat_str(st, &sub);
                    let mut n = node("emphasis");
                    n.children = children;
                    self.apply(st, &ev, n);
                    return Ok(true);
                }
                queue.push(marker);
            }
            if character == Some('\\') {
                queue.push('\\');
                index += 1;
                character = at(v, index);
            }
            queue.extend(character);
            index += 1;
        }
        Ok(false)
    }

    fn deletion(&mut self, st: &mut TokState, v: &[char]) -> R<bool> {
        if at(v, 0) != Some('~') || at(v, 1) != Some('~') || ws(at(v, 2)) {
            return Ok(false);
        }
        let length = v.len();
        let mut index = 1;
        let mut now = self.now(st);
        now.column += 2;
        now.offset += 2;
        let mut previous: Option<char> = None;
        let mut preceding: Option<char> = None;
        let mut subvalue = String::new();
        loop {
            index += 1;
            if index >= length {
                break;
            }
            let c = at(v, index);
            if c == Some('~') && previous == Some('~') && (preceding.is_none() || !ws(preceding)) {
                let children = self.tokenize_inline(subvalue.chars().collect(), now)?;
                let ev = self.eat_str(st, &format!("~~{subvalue}~~"));
                let mut n = node("delete");
                n.children = children;
                self.apply(st, &ev, n);
                return Ok(true);
            }
            subvalue.extend(previous);
            preceding = previous;
            previous = c;
        }
        Ok(false)
    }

    fn inline_code(&mut self, st: &mut TokState, v: &[char]) -> R<bool> {
        let length = v.len();
        let mut index = 0;
        while index < length && v[index] == '`' {
            index += 1;
        }
        if index == 0 || index == length {
            return Ok(false);
        }
        let mut open_end = index;
        let mut close_start: Option<usize> = None;
        let mut close_end = 0;
        let mut found = false;
        let mut next = at(v, index);
        while index < length {
            let code = next;
            next = at(v, index + 1);
            if code == Some('`') {
                let cs = *close_start.get_or_insert(index);
                close_end = index + 1;
                if next != Some('`') && close_end - cs == open_end {
                    found = true;
                    break;
                }
            } else if close_start.is_some() {
                close_start = None;
            }
            index += 1;
        }
        if !found {
            return Ok(false);
        }
        let mut cs = close_start.unwrap_or(0);
        let mut i = open_end;
        let mut len = cs;
        let code = at(v, i);
        let nxt = at(v, len.wrapping_sub(1));
        if len > i + 2 && matches!(code, Some(' ' | '\n')) && matches!(nxt, Some(' ' | '\n')) {
            i += 1;
            len -= 1;
            let mut f = false;
            while i < len {
                if !matches!(v[i], ' ' | '\n') {
                    f = true;
                    break;
                }
                i += 1;
            }
            if f {
                open_end += 1;
                cs -= 1;
            }
        }
        let ev = self.eat(st, close_end);
        let mut n = node("inlineCode");
        n.value = s(slice(v, open_end, cs));
        self.apply(st, &ev, n);
        Ok(true)
    }

    fn hard_break(&mut self, st: &mut TokState, v: &[char]) -> R<bool> {
        for (i, &c) in v.iter().enumerate() {
            if c == '\n' {
                if i < 2 {
                    return Ok(false);
                }
                let ev = self.eat(st, i + 1);
                self.apply(st, &ev, node("break"));
                return Ok(true);
            }
            if c != ' ' {
                return Ok(false);
            }
        }
        Ok(false)
    }

    fn inline_math(&mut self, st: &mut TokState, v: &[char]) -> R<bool> {
        let length = v.len();
        let mut index = 0;
        let mut escaped = false;
        if at(v, 0) == Some('\\') {
            escaped = true;
            index += 1;
        }
        if at(v, index) != Some('$') {
            return Ok(false);
        }
        index += 1;
        if escaped {
            let ev = self.eat(st, index);
            let mut n = node("text");
            n.value = "$".to_string();
            self.apply(st, &ev, n);
            return Ok(true);
        }
        let mut double = false;
        if at(v, index) == Some('$') {
            double = true;
            index += 1;
        }
        let mut next = at(v, index);
        if matches!(next, Some(' ' | '\t')) {
            return Ok(false);
        }
        let content_start = index;
        let mut content_end = 0;
        let mut value_end = None;
        while index < length {
            let code = next;
            next = at(v, index + 1);
            if code == Some('$') {
                let previous = at(v, index.wrapping_sub(1));
                if !matches!(previous, Some(' ' | '\t'))
                    && !next.is_some_and(|c| c.is_ascii_digit())
                    && (!double || next == Some('$'))
                {
                    content_end = index - 1;
                    index += 1;
                    if double {
                        index += 1;
                    }
                    value_end = Some(index);
                    break;
                }
            } else if code == Some('\\') {
                index += 1;
                next = at(v, index + 1);
            }
            index += 1;
        }
        let Some(value_end) = value_end else {
            return Ok(false);
        };
        let content = s(slice(v, content_start, content_end + 1));
        let ev = self.eat(st, value_end);
        let mut n = node("inlineMath");
        n.value = content;
        self.apply(st, &ev, n);
        Ok(true)
    }

    fn liquid(&mut self, st: &mut TokState, v: &[char]) -> R<bool> {
        if at(v, 0) != Some('{') {
            return Ok(false);
        }
        let close = match at(v, 1) {
            Some('%') => "%}",
            Some('{') => "}}",
            _ => return Ok(false),
        };
        let Some(end) = index_of(v, close, 2) else {
            return Ok(false);
        };
        let len = end + 2;
        let ev = self.eat(st, len);
        let mut n = node("liquidNode");
        n.value = s(&v[..len]);
        self.apply(st, &ev, n);
        Ok(true)
    }
}

fn min_found(a: Option<usize>, b: Option<usize>) -> Option<usize> {
    match (a, b) {
        (a, None) => a,
        (None, b) => b,
        (Some(x), Some(y)) => Some(x.min(y)),
    }
}

fn is_atext(c: Option<char>) -> bool {
    decimal(c) || alphabetical(c) || matches!(c, Some('+' | '-' | '.' | '_'))
}

fn locate_email(v: &[char], from: usize) -> Option<usize> {
    let at_i = index_of(v, "@", from)?;
    if at_i == from || !is_atext(at(v, at_i - 1)) {
        return locate_email(v, at_i + 1);
    }
    let mut position = at_i;
    while position > from && is_atext(at(v, position - 1)) {
        position -= 1;
    }
    Some(position)
}

fn html_sequences() -> &'static Vec<(regex::Regex, regex::Regex, bool)> {
    static SEQ: std::sync::OnceLock<Vec<(regex::Regex, regex::Regex, bool)>> =
        std::sync::OnceLock::new();
    SEQ.get_or_init(|| {
        let blocks = [
            "address",
            "article",
            "aside",
            "base",
            "basefont",
            "blockquote",
            "body",
            "caption",
            "center",
            "col",
            "colgroup",
            "dd",
            "details",
            "dialog",
            "dir",
            "div",
            "dl",
            "dt",
            "fieldset",
            "figcaption",
            "figure",
            "footer",
            "form",
            "frame",
            "frameset",
            "h1",
            "h2",
            "h3",
            "h4",
            "h5",
            "h6",
            "head",
            "header",
            "hgroup",
            "hr",
            "html",
            "iframe",
            "legend",
            "li",
            "link",
            "main",
            "menu",
            "menuitem",
            "meta",
            "nav",
            "noframes",
            "ol",
            "optgroup",
            "option",
            "p",
            "param",
            "pre",
            "section",
            "source",
            "title",
            "summary",
            "table",
            "tbody",
            "td",
            "tfoot",
            "th",
            "thead",
            "title",
            "tr",
            "track",
            "ul",
        ]
        .join("|");
        let r = |p: &str| regex::Regex::new(p).ok();
        let list = [
            (
                r(r"(?i)^<(script|pre|style)(\s|>|$)"),
                r(r"(?i)</(script|pre|style)>"),
                true,
            ),
            (r(r"^<!--"), r(r"-->"), true),
            (r(r"^<\?"), r(r"\?>"), true),
            (r(r"^<![A-Za-z]"), r(r">"), true),
            (r(r"^<!\[CDATA\["), r(r"\]\]>"), true),
            (r(&format!(r"(?i)^</?({blocks})(\s|/?>|$)")), r(r"^$"), true),
            (
                r(&format!(r"^(?:{OPEN_TAG}|{CLOSE_TAG})\s*$")),
                r(r"^$"),
                false,
            ),
        ];
        list.into_iter()
            .filter_map(|(a, b, c)| Some((a?, b?, c)))
            .collect()
    })
}

const OPEN_TAG: &str = r#"<[A-Za-z][A-Za-z0-9\-]*(?:\s+[a-zA-Z_:][a-zA-Z0-9:._-]*(?:\s*=\s*(?:[^"'=<>`\x00-\x20]+|'[^']*'|"[^"]*"))?)*\s*/?>"#;
const CLOSE_TAG: &str = r"</[A-Za-z][A-Za-z0-9\-]*\s*>";

fn html_tag() -> Option<&'static regex::Regex> {
    static TAG: std::sync::OnceLock<Option<regex::Regex>> = std::sync::OnceLock::new();
    TAG.get_or_init(|| {
        regex::Regex::new(&format!(
            r"^(?:{OPEN_TAG}|{CLOSE_TAG}|<!---->|<!--(?:-?[^>-])(?:-?[^-])*-->|<[?].*?[?]>|<![A-Za-z]+\s+[^>]*>|<!\[CDATA\[[\s\S]*?\]\]>)"
        ))
        .ok()
    })
    .as_ref()
}

pub struct FrontMatter {
    pub language: String,
    pub explicit_language: String,
    pub value: String,
    pub start_delimiter: String,
    pub end_delimiter: String,
    pub raw: String,
}

pub fn parse_front_matter(text: &str) -> Option<FrontMatter> {
    let start = text.get(..3)?;
    if start != "---" && start != "+++" {
        return None;
    }
    let first_lb = text[3..].find('\n')? + 3;
    let explicit = js_trim(&text[3..first_lb]).to_string();
    let mut end = text[first_lb..]
        .find(&format!("\n{start}"))
        .map(|i| i + first_lb);
    let language = if explicit.is_empty() {
        if start == "+++" {
            "toml".to_string()
        } else {
            "yaml".to_string()
        }
    } else {
        explicit.clone()
    };
    if end.is_none() && start == "---" && language == "yaml" {
        end = text[first_lb..].find("\n...").map(|i| i + first_lb);
    }
    let end = end?;
    let fm_end = end + 1 + 3;
    let raw = text.get(..fm_end)?.to_string();
    Some(FrontMatter {
        language,
        explicit_language: explicit,
        value: text.get(first_lb + 1..end).unwrap_or("").to_string(),
        start_delimiter: start.to_string(),
        end_delimiter: raw[raw.len() - 3..].to_string(),
        raw,
    })
}

/// remark's `parse` for Prettier's `markdown` parser: the mdast root.
pub fn parse(text: &str) -> Option<Node> {
    let chars: Vec<char> = text.chars().collect();
    let mut line_starts = Vec::new();
    for (i, &c) in chars.iter().enumerate() {
        if c == '\n' {
            line_starts.push(i + 1);
        }
    }
    line_starts.push(chars.len() + 1);
    let mut p = Parser {
        line_starts: line_starts.clone(),
        total: chars.len(),
        offset: HashMap::new(),
        in_list: false,
        in_block: false,
        in_link: false,
        at_start: true,
        text_only: false,
        eof: None,
    };
    p.line_starts.pop();
    let start = Pt {
        line: 1,
        column: 1,
        offset: 0,
    };
    let children = p.tokenize_block(chars, start).ok()?;
    let mut root = node("root");
    root.children = children;
    root.start = start;
    root.end = p.eof.unwrap_or(start);
    root.has_pos = true;
    Some(root)
}
