//! The checks yaml 1.10.2's `Document#parse` makes on the CST (`Document-*.js`,
//! `resolveSeq-*.js`: anchors, tags, pairs, duplicate keys), and yaml-unist-parser 2.0.5's
//! transform, comment attachment and position updates (`lib/transforms`, `lib/attach.js`,
//! `lib/utils`), which give the tree Prettier's YAML printer reads. yaml-unist-parser is MIT
//! licensed, Copyright (c) Ika. Anything yaml reports as an error returns `None`.

use super::cst::{Chomp, Cst, It, T, at_blank, at_document_boundary, c, fold_newline};
use crate::comments::is_js_whitespace;

#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Y {
    Root,
    Document,
    DocumentHead,
    DocumentBody,
    Directive,
    Alias,
    BlockLiteral,
    BlockFolded,
    Plain,
    QuoteSingle,
    QuoteDouble,
    Mapping,
    MappingItem,
    MappingKey,
    MappingValue,
    Sequence,
    SequenceItem,
    FlowMapping,
    FlowMappingItem,
    FlowSequence,
    FlowSequenceItem,
    Comment,
    Tag,
    Anchor,
}

#[derive(Clone, Copy, PartialEq, Debug, Default)]
pub struct Pt {
    pub offset: usize,
    pub line: usize,
    pub column: usize,
}

pub const LEAD: u8 = 1;
pub const TRAIL: u8 = 2;
pub const END: u8 = 4;

#[derive(Clone, Debug)]
pub struct U {
    pub ty: Y,
    pub start: Pt,
    pub end: Pt,
    pub kids: Vec<usize>,
    pub tag: Option<usize>,
    pub anchor: Option<usize>,
    pub middle: Vec<usize>,
    pub leading: Vec<usize>,
    pub trailing: Option<usize>,
    pub end_comments: Vec<usize>,
    pub indicator: Option<usize>,
    pub value: String,
    pub chomping: Chomp,
    pub indent: Option<i64>,
    pub name: String,
    pub params: Vec<String>,
    pub parent: Option<usize>,
    pub empty_pos: bool,
    pub attach: u8,
}

impl Y {
    fn attach(self) -> u8 {
        match self {
            Y::Alias | Y::Plain | Y::QuoteDouble | Y::QuoteSingle | Y::Directive => LEAD | TRAIL,
            Y::BlockFolded | Y::BlockLiteral | Y::MappingItem | Y::FlowMappingItem | Y::Mapping => {
                LEAD
            }
            Y::FlowMapping | Y::FlowSequence | Y::MappingValue | Y::SequenceItem => {
                LEAD | TRAIL | END
            }
            Y::MappingKey => TRAIL | END,
            Y::Sequence => LEAD | END,
            Y::DocumentBody => END,
            Y::DocumentHead => END | TRAIL,
            Y::Document => TRAIL,
            _ => 0,
        }
    }

    pub fn has_children(self) -> bool {
        matches!(
            self,
            Y::Root
                | Y::Document
                | Y::DocumentHead
                | Y::DocumentBody
                | Y::Mapping
                | Y::MappingItem
                | Y::MappingKey
                | Y::MappingValue
                | Y::Sequence
                | Y::SequenceItem
                | Y::FlowMapping
                | Y::FlowMappingItem
                | Y::FlowSequence
                | Y::FlowSequenceItem
        )
    }
}

#[derive(Clone, Debug, PartialEq)]
enum KeyVal {
    Null,
    Bool(bool),
    Num(f64),
    Str(String),
    Distinct,
    Unknown,
}

#[derive(Debug)]
enum Ast {
    Scalar {
        cst: usize,
        tag: Option<String>,
        key: KeyVal,
    },
    Alias {
        cst: usize,
    },
    Map {
        cst: usize,
        tag: Option<String>,
        pairs: Vec<Pair>,
    },
    Seq {
        cst: usize,
        tag: Option<String>,
        items: Vec<SeqEl>,
    },
}

#[derive(Debug)]
struct Pair {
    key: Option<Box<Ast>>,
    value: Option<Box<Ast>>,
}

#[derive(Debug)]
enum SeqEl {
    Node(Option<Box<Ast>>),
    Pair(Pair),
}

const STR: &str = "tag:yaml.org,2002:str";
const MAP: &str = "tag:yaml.org,2002:map";
const SEQ: &str = "tag:yaml.org,2002:seq";

fn js_trim(s: &str) -> &str {
    s.trim_matches(is_js_whitespace)
}

fn utf16_len(s: &[u8]) -> usize {
    String::from_utf8_lossy(s).encode_utf16().count()
}

struct Resolver<'c, 'a> {
    cst: &'c mut Cst<'a>,
    anchors: std::collections::HashSet<String>,
    prefixes: Vec<(String, String)>,
}

type O<T> = Option<T>;

impl Resolver<'_, '_> {
    fn src(&self) -> &[u8] {
        self.cst.src
    }

    fn text(&self, r: (usize, usize)) -> String {
        String::from_utf8_lossy(
            &self.cst.src[r.0.min(self.cst.src.len())..r.1.min(self.cst.src.len())],
        )
        .into_owned()
    }

    fn prop_value(&self, id: usize, key: u8, skip: bool) -> Option<String> {
        for p in &self.cst.n[id].props {
            if c(self.src(), p.0) == key {
                return Some(self.text((p.0 + usize::from(skip), p.1)));
            }
        }
        None
    }

    fn comment_has_required_whitespace(&self, id: usize, start: usize) -> bool {
        let n = &self.cst.n[id];
        if let Some(h) = n.header
            && start == h.1
        {
            return false;
        }
        let Some(vr) = n.vr else {
            return false;
        };
        start != vr.1 || at_blank(self.src(), vr.1.wrapping_sub(1), false)
    }

    fn plain_str(&self, id: usize) -> O<String> {
        plain_str(self.cst, id)
    }

    fn quote_str(&self, id: usize) -> O<String> {
        quote_str(self.cst, id)
    }

    fn resolve_opt(&mut self, id: Option<usize>) -> O<Option<Box<Ast>>> {
        match id {
            None => Some(None),
            Some(id) => Some(Some(Box::new(self.resolve(id)?))),
        }
    }

    fn resolve(&mut self, id: usize) -> O<Ast> {
        let parent = self.cst.n[id].ctx.parent;
        let mut props = Vec::new();
        if let Some(p) = parent
            && self.cst.is_item(p)
        {
            props.extend(self.cst.n[p].props.iter().copied());
        }
        props.extend(self.cst.n[id].props.iter().copied());
        let (mut anchors, mut tags) = (0, 0);
        for (start, _) in &props {
            match c(self.src(), *start) {
                b'#' => {
                    if !self.comment_has_required_whitespace(id, *start) {
                        return None;
                    }
                }
                b'&' => anchors += 1,
                b'!' => tags += 1,
                _ => {}
            }
        }
        if anchors > 1 || tags > 1 {
            return None;
        }
        if anchors == 1 {
            let name = self.prop_value(id, b'&', true)?;
            self.anchors.insert(name);
        }
        let ty = self.cst.n[id].ty;
        if ty == T::Alias {
            if anchors + tags > 0 {
                return None;
            }
            let vr = self.cst.n[id].vr?;
            let name = self.text(vr);
            if !self.anchors.contains(&name) {
                return None;
            }
            return Some(Ast::Alias { cst: id });
        }
        let explicit = self.prop_value(id, b'!', false);
        let mut non_specific = false;
        let mut tag_name = None;
        if let Some(tag) = &explicit {
            if tag.as_bytes().get(1) == Some(&b'<') {
                let verbatim = tag.get(2..tag.len().saturating_sub(1)).unwrap_or("");
                if verbatim == "!" || verbatim == "!!" || !tag.ends_with('>') {
                    return None;
                }
                tag_name = Some(verbatim.to_string());
            } else {
                let cut = tag.rfind('!')? + 1;
                let (handle, suffix) = tag.split_at(cut);
                if handle == "!" && suffix.is_empty() {
                    non_specific = true;
                } else {
                    let prefix = self
                        .prefixes
                        .iter()
                        .find(|(h, _)| h == handle)
                        .map(|(_, p)| p.clone())
                        .or(match handle {
                            "!" => Some("!".to_string()),
                            "!!" => Some("tag:yaml.org,2002:".to_string()),
                            _ => None,
                        })?;
                    if suffix.is_empty() {
                        return None;
                    }
                    tag_name = Some(format!("{prefix}{}", decode_uri_component(suffix)?));
                }
            }
        }
        let by_type = match ty {
            T::BlockFolded | T::BlockLiteral | T::QuoteDouble | T::QuoteSingle => Some(STR),
            T::FlowMap | T::Map => Some(MAP),
            T::FlowSeq | T::Seq => Some(SEQ),
            T::Plain if non_specific => Some(STR),
            _ => None,
        };
        let shown = if explicit.is_some() {
            tag_name.clone().or(by_type.map(str::to_string))
        } else {
            None
        };
        let name = tag_name.clone().or(by_type.map(str::to_string));
        let is_map = matches!(ty, T::Map | T::FlowMap);
        let is_seq = matches!(ty, T::Seq | T::FlowSeq);
        let scalar = matches!(
            ty,
            T::Plain | T::QuoteDouble | T::QuoteSingle | T::BlockFolded | T::BlockLiteral
        );
        match name.as_deref() {
            Some(MAP) if !is_map => None,
            Some(SEQ) if !is_seq => None,
            // yaml 1.10 `resolveByTagName` resolves a collection tagged with a core-schema scalar
            // tag through that tag (`resolveString` gives "" for a collection) and wraps the
            // result in a `Scalar`; yaml-unist-parser's `transformSeq` / `transformMap` then read
            // `items` of it and throw a TypeError. Tags outside the core schema (`!!binary`,
            // `!foo`, ...) match no tag and fall back to the collection, which Prettier formats.
            Some(t)
                if (is_map || is_seq)
                    && matches!(
                        t.strip_prefix("tag:yaml.org,2002:"),
                        Some("str" | "null" | "bool" | "int" | "float")
                    ) =>
            {
                None
            }
            _ if is_map => self.resolve_map(id, shown),
            _ if is_seq => self.resolve_seq(id, shown),
            _ if !scalar => None,
            _ => {
                let s = match ty {
                    T::Plain => self.plain_str(id)?,
                    T::QuoteDouble | T::QuoteSingle => self.quote_str(id)?,
                    _ => block_str(self.cst, id)?,
                };
                let key = match name.as_deref() {
                    None => core_value(&s),
                    Some(STR) => KeyVal::Str(s),
                    Some(t) => {
                        let (v, kind) = core_kind(&s);
                        match t.strip_prefix("tag:yaml.org,2002:") {
                            Some(k @ ("null" | "bool" | "int" | "float")) if k == kind => v,
                            _ => KeyVal::Str(s),
                        }
                    }
                };
                Some(Ast::Scalar {
                    cst: id,
                    tag: shown,
                    key,
                })
            }
        }
    }

    fn resolve_map(&mut self, id: usize, tag: Option<String>) -> O<Ast> {
        let pairs = if self.cst.n[id].ty == T::FlowMap {
            self.flow_map_items(id)?
        } else {
            self.block_map_items(id)?
        };
        let keys: Vec<KeyVal> = pairs
            .iter()
            .map(|p| match p.key.as_deref() {
                None => KeyVal::Null,
                Some(Ast::Scalar { key, .. }) => key.clone(),
                Some(_) => KeyVal::Distinct,
            })
            .collect();
        let null_key: Vec<bool> = pairs.iter().map(|p| p.key.is_none()).collect();
        for i in 0..keys.len() {
            for j in i + 1..keys.len() {
                if null_key[i] && null_key[j] {
                    return None;
                }
                if null_key[i] != null_key[j] {
                    if matches!(keys[i], KeyVal::Unknown) || matches!(keys[j], KeyVal::Unknown) {
                        return None;
                    }
                    continue;
                }
                match (&keys[i], &keys[j]) {
                    (KeyVal::Unknown, _) | (_, KeyVal::Unknown) => return None,
                    (KeyVal::Distinct, _) | (_, KeyVal::Distinct) => {}
                    (KeyVal::Str(a), KeyVal::Str(b)) if a == b && a == "<<" => {}
                    (a, b) if a == b => return None,
                    _ => {}
                }
            }
        }
        Some(Ast::Map {
            cst: id,
            tag,
            pairs,
        })
    }

    fn fake_plain(&mut self, item: usize) -> usize {
        let pos = self.cst.n[item].range.0 + 1;
        let mut node = self.cst.n[item].clone();
        node.ty = T::Plain;
        node.props = Vec::new();
        node.ctx = Default::default();
        node.ctx.parent = Some(item);
        node.range = (pos, pos);
        node.vr = Some((pos, pos));
        node.node = None;
        node.items = Vec::new();
        node.header = None;
        self.cst.n.push(node);
        self.cst.n.len() - 1
    }

    fn block_map_items(&mut self, id: usize) -> O<Vec<Pair>> {
        let items = self.cst.n[id].items.clone();
        let mut out = Vec::new();
        let mut key: Option<Option<Box<Ast>>> = None;
        let mut key_start: Option<usize> = None;
        for (i, it) in items.iter().enumerate() {
            let It::N(item) = *it else {
                return None;
            };
            match self.cst.n[item].ty {
                T::BlankLine | T::Comment => {}
                T::MapKey => {
                    if let Some(k) = key.take() {
                        out.push(Pair {
                            key: k,
                            value: None,
                        });
                    }
                    let node = self.cst.n[item].node;
                    key = Some(self.resolve_opt(node)?);
                    key_start = None;
                }
                T::MapValue => {
                    let k = key.take().unwrap_or(None);
                    let node = self.cst.n[item].node;
                    if !self.cst.n[item].ctx.at_line_start
                        && let Some(nd) = node
                        && self.cst.n[nd].ty == T::Map
                        && !self.cst.n[nd].ctx.at_line_start
                    {
                        return None;
                    }
                    let value_node = match node {
                        Some(n) => Some(n),
                        None if !self.cst.n[item].props.is_empty() => Some(self.fake_plain(item)),
                        None => None,
                    };
                    let value = self.resolve_opt(value_node)?;
                    if k.is_some()
                        && let Some(ks) = key_start
                        && self.cst.n[item].range.0 > ks
                        && utf16_len(&self.src()[ks..self.cst.n[item].range.0]) > 1024
                    {
                        return None;
                    }
                    out.push(Pair { key: k, value });
                    key_start = None;
                }
                _ => {
                    if let Some(k) = key.take() {
                        out.push(Pair {
                            key: k,
                            value: None,
                        });
                    }
                    key = Some(Some(Box::new(self.resolve(item)?)));
                    key_start = Some(self.cst.n[item].range.0);
                    let mut ok = false;
                    for next in &items[i + 1..] {
                        let It::N(nx) = *next else {
                            break;
                        };
                        match self.cst.n[nx].ty {
                            T::BlankLine | T::Comment => continue,
                            T::MapValue => {
                                ok = true;
                                break;
                            }
                            _ => break,
                        }
                    }
                    if !ok {
                        return None;
                    }
                    let vr = self.cst.n[item].vr?;
                    if self.src()[vr.0..vr.1.min(self.src().len())].contains(&b'\n') {
                        return None;
                    }
                }
            }
        }
        if let Some(k) = key {
            out.push(Pair {
                key: k,
                value: None,
            });
        }
        Some(out)
    }

    fn check_flow_comment(&self, id: usize) -> O<()> {
        let start = self.cst.n[id].range.0;
        let prev = if start > 0 {
            c(self.src(), start - 1)
        } else {
            0
        };
        if matches!(prev, b'\n' | b'\t' | b' ') {
            Some(())
        } else {
            None
        }
    }

    fn check_flow_end(&self, id: usize, close: u8) -> O<()> {
        for it in self.cst.n[id].items.iter().rev() {
            match *it {
                It::N(n) if self.cst.n[n].ty == T::Comment => continue,
                It::Ch(ch, _) => return if ch == close { Some(()) } else { None },
                It::N(_) => return None,
            }
        }
        Some(())
    }

    fn flow_map_items(&mut self, id: usize) -> O<Vec<Pair>> {
        let items = self.cst.n[id].items.clone();
        let mut out = Vec::new();
        let mut key: Option<Option<Box<Ast>>> = None;
        let mut explicit = false;
        let mut next = b'{';
        let len = items.len();
        for (i, it) in items.iter().enumerate() {
            match *it {
                It::Ch(ch, _) => {
                    if ch == b'?' && key.is_none() && !explicit {
                        explicit = true;
                        next = b':';
                        continue;
                    }
                    if ch == b':' {
                        if key.is_none() {
                            key = Some(None);
                        }
                        if next == b':' {
                            next = b',';
                            continue;
                        }
                    } else {
                        if explicit {
                            if key.is_none() && ch != b',' {
                                key = Some(None);
                            }
                            explicit = false;
                        }
                        if let Some(k) = key.take() {
                            out.push(Pair {
                                key: k,
                                value: None,
                            });
                            if ch == b',' {
                                next = b':';
                                continue;
                            }
                        }
                    }
                    if ch == b'}' {
                        if i == len - 1 {
                            continue;
                        }
                    } else if ch == next {
                        next = b':';
                        continue;
                    }
                    return None;
                }
                It::N(n) => match self.cst.n[n].ty {
                    T::BlankLine => {}
                    T::Comment => self.check_flow_comment(n)?,
                    _ => {
                        if key.is_none() {
                            if next == b',' {
                                return None;
                            }
                            key = Some(Some(Box::new(self.resolve(n)?)));
                        } else {
                            if next != b',' {
                                return None;
                            }
                            let value = Some(Box::new(self.resolve(n)?));
                            out.push(Pair {
                                key: key.take().unwrap_or(None),
                                value,
                            });
                            explicit = false;
                        }
                    }
                },
            }
        }
        self.check_flow_end(id, b'}')?;
        if let Some(k) = key {
            out.push(Pair {
                key: k,
                value: None,
            });
        }
        Some(out)
    }

    fn resolve_seq(&mut self, id: usize, tag: Option<String>) -> O<Ast> {
        let items = if self.cst.n[id].ty == T::FlowSeq {
            self.flow_seq_items(id)?
        } else {
            let mut out = Vec::new();
            for it in self.cst.n[id].items.clone() {
                let It::N(item) = it else {
                    return None;
                };
                match self.cst.n[item].ty {
                    T::BlankLine | T::Comment => {}
                    T::SeqItem => {
                        let node = self.cst.n[item].node;
                        out.push(SeqEl::Node(self.resolve_opt(node)?));
                        if self.cst.has_props(item) {
                            return None;
                        }
                    }
                    _ => return None,
                }
            }
            out
        };
        Some(Ast::Seq {
            cst: id,
            tag,
            items,
        })
    }

    fn flow_seq_items(&mut self, id: usize) -> O<Vec<SeqEl>> {
        let items = self.cst.n[id].items.clone();
        let mut out: Vec<SeqEl> = Vec::new();
        let mut explicit = false;
        let mut key: Option<Option<Box<Ast>>> = None;
        let mut key_start: Option<usize> = None;
        let mut next: Option<u8> = Some(b'[');
        let len = items.len();
        for (i, it) in items.iter().enumerate() {
            match *it {
                It::Ch(ch, offset) => {
                    if ch != b':' && (explicit || key.is_some()) {
                        if explicit && key.is_none() {
                            key = Some(if next.is_some() {
                                match out.pop()? {
                                    SeqEl::Node(n) => n,
                                    SeqEl::Pair(_) => return None,
                                }
                            } else {
                                None
                            });
                        }
                        out.push(SeqEl::Pair(Pair {
                            key: key.take().unwrap_or(None),
                            value: None,
                        }));
                        explicit = false;
                        key_start = None;
                    }
                    if Some(ch) == next {
                        next = None;
                    } else if next.is_none() && ch == b'?' {
                        explicit = true;
                    } else if next != Some(b'[') && ch == b':' && key.is_none() {
                        if next == Some(b',') {
                            let popped = match out.pop()? {
                                SeqEl::Node(n) => n,
                                SeqEl::Pair(_) => return None,
                            };
                            if !explicit && let Some(ks) = key_start {
                                if offset > ks && utf16_len(&self.src()[ks..offset]) > 1024 {
                                    return None;
                                }
                                if self.src()[ks..offset.max(ks)].contains(&b'\n') {
                                    return None;
                                }
                            }
                            key = Some(popped);
                        } else {
                            key = Some(None);
                        }
                        key_start = None;
                        explicit = false;
                        next = None;
                    } else if next == Some(b'[') || ch != b']' || i < len - 1 {
                        return None;
                    }
                }
                It::N(n) => match self.cst.n[n].ty {
                    T::BlankLine => {}
                    T::Comment => self.check_flow_comment(n)?,
                    _ => {
                        if next.is_some() {
                            return None;
                        }
                        let value = Some(Box::new(self.resolve(n)?));
                        match key.take() {
                            None => out.push(SeqEl::Node(value)),
                            Some(k) => out.push(SeqEl::Pair(Pair { key: k, value })),
                        }
                        key_start = Some(self.cst.n[n].range.0);
                        next = Some(b',');
                    }
                },
            }
        }
        self.check_flow_end(id, b']')?;
        if let Some(k) = key {
            out.push(SeqEl::Pair(Pair {
                key: k,
                value: None,
            }));
        }
        Some(out)
    }
}

pub fn plain_str(cst: &Cst<'_>, id: usize) -> O<String> {
    let src = cst.src;
    let (start, mut end) = cst.n[id].vr?;
    let mut ch = if end > 0 { c(src, end - 1) } else { 0 };
    while start < end && matches!(ch, b'\n' | b'\t' | b' ') {
        end -= 1;
        ch = if end > 0 { c(src, end - 1) } else { 0 };
    }
    let mut out: Vec<u8> = Vec::new();
    let mut i = start;
    while i < end {
        let ch = src[i];
        if ch == b'\n' {
            let (fold, offset, _) = fold_newline(src, i, -1);
            out.extend_from_slice(fold.as_bytes());
            i = offset;
        } else if ch == b' ' || ch == b'\t' {
            let ws = i;
            let mut next = c(src, i + 1);
            while i < end && (next == b' ' || next == b'\t') {
                i += 1;
                next = c(src, i + 1);
            }
            if next != b'\n' {
                out.extend_from_slice(&src[ws..=i]);
            }
        } else {
            out.push(ch);
        }
        i += 1;
    }
    if matches!(c(src, start), b'\t' | b'@' | b'`') {
        return None;
    }
    String::from_utf8(out).ok()
}

pub fn quote_str(cst: &Cst<'_>, id: usize) -> O<String> {
    let src = cst.src;
    let n = &cst.n[id];
    let (start, end) = n.vr?;
    let indent = n.ctx.indent.unwrap_or(0);
    let double = n.ty == T::QuoteDouble;
    let q = if double { b'"' } else { b'\'' };
    if end == 0 || c(src, end - 1) != q || end < start + 2 {
        return None;
    }
    let mut out = String::new();
    let mut i = start + 1;
    while i < end - 1 {
        let ch = src[i];
        if ch == b'\n' {
            if at_document_boundary(src, i + 1, None) {
                return None;
            }
            let (fold, offset, error) = fold_newline(src, i, indent);
            if error {
                return None;
            }
            out.push_str(&fold);
            i = offset;
        } else if double && ch == b'\\' {
            i += 1;
            let e = c(src, i);
            let simple = match e {
                b'0' => Some('\0'),
                b'a' => Some('\x07'),
                b'b' => Some('\x08'),
                b'e' => Some('\x1b'),
                b'f' => Some('\x0c'),
                b'n' => Some('\n'),
                b'r' => Some('\r'),
                b't' | b'\t' => Some('\t'),
                b'v' => Some('\x0b'),
                b'N' => Some('\u{85}'),
                b'_' => Some('\u{a0}'),
                b'L' => Some('\u{2028}'),
                b'P' => Some('\u{2029}'),
                b' ' => Some(' '),
                b'"' => Some('"'),
                b'/' => Some('/'),
                b'\\' => Some('\\'),
                _ => None,
            };
            if let Some(s) = simple {
                out.push(s);
            } else {
                let len = match e {
                    b'x' => 2,
                    b'u' => 4,
                    b'U' => 8,
                    b'\n' => {
                        while matches!(c(src, i + 1), b' ' | b'\t') {
                            i += 1;
                        }
                        i += 1;
                        continue;
                    }
                    _ => return None,
                };
                let cc = src.get(i + 1..i + 1 + len)?;
                if !cc.iter().all(u8::is_ascii_hexdigit) {
                    return None;
                }
                let code = u32::from_str_radix(std::str::from_utf8(cc).ok()?, 16).ok()?;
                out.push(char::from_u32(code)?);
                i += len;
            }
        } else if !double && ch == b'\'' {
            out.push('\'');
            i += 1;
            if c(src, i) != b'\'' {
                return None;
            }
        } else if ch == b' ' || ch == b'\t' {
            let ws = i;
            let mut next = c(src, i + 1);
            while next == b' ' || next == b'\t' {
                i += 1;
                next = c(src, i + 1);
            }
            if next != b'\n' {
                out.push_str(&String::from_utf8_lossy(&src[ws..=i]));
            }
        } else {
            let len = utf8_len(ch);
            out.push_str(std::str::from_utf8(src.get(i..i + len)?).ok()?);
            i += len - 1;
        }
        i += 1;
    }
    Some(out)
}

fn utf8_len(b: u8) -> usize {
    match b {
        0xF0..=0xFF => 4,
        0xE0..=0xEF => 3,
        0xC0..=0xDF => 2,
        _ => 1,
    }
}

fn decode_uri_component(s: &str) -> O<String> {
    let b = s.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'%' {
            let h = b.get(i + 1..i + 3)?;
            out.push(u8::from_str_radix(std::str::from_utf8(h).ok()?, 16).ok()?);
            i += 3;
        } else {
            out.push(b[i]);
            i += 1;
        }
    }
    String::from_utf8(out).ok()
}

fn core_kind(s: &str) -> (KeyVal, &'static str) {
    let v = core_value(s);
    let kind = match &v {
        KeyVal::Null => "null",
        KeyVal::Bool(_) => "bool",
        KeyVal::Num(_) => {
            let unsigned = s.strip_prefix(['-', '+']).unwrap_or(s);
            if unsigned.bytes().all(|c| c.is_ascii_digit())
                || s.starts_with("0o")
                || s.starts_with("0x")
            {
                "int"
            } else {
                "float"
            }
        }
        _ => "str",
    };
    (v, kind)
}

fn block_str(cst: &Cst<'_>, id: usize) -> O<String> {
    let n = &cst.n[id];
    let src = cst.src;
    let Some((start, mut end)) = n.vr else {
        return Some(String::new());
    };
    if end == 0 || end <= start {
        return Some(String::new());
    }
    let keep = n.chomping == Chomp::Keep;
    let mut last_newline: Option<usize> = None;
    let mut ch = c(src, end - 1);
    while matches!(ch, b'\n' | b'\t' | b' ') {
        end -= 1;
        if end <= start {
            if keep {
                break;
            }
            return Some(String::new());
        }
        if ch == b'\n' {
            last_newline = Some(end);
        }
        ch = c(src, end - 1);
    }
    let mut keep_start = end + 1;
    if let Some(ln) = last_newline {
        if keep {
            keep_start = ln;
            end = n.vr?.1;
        } else {
            end = ln;
        }
    }
    let bi = n.ctx.indent.unwrap_or(0) + n.block_indent.unwrap_or(0);
    let folded = n.ty == T::BlockFolded;
    let mut at_start = true;
    let mut out: Vec<u8> = Vec::new();
    let mut sep: Vec<u8> = Vec::new();
    let mut prev_more = false;
    let mut i = start;
    while i < end {
        let mut j = 0;
        while j < bi {
            if c(src, i) != b' ' {
                break;
            }
            i += 1;
            j += 1;
        }
        let ch = c(src, i);
        if ch == b'\n' {
            if sep == b"\n" {
                out.push(b'\n');
            } else {
                sep = b"\n".to_vec();
            }
        } else {
            let line_end = super::cst::end_of_line(src, i);
            let line = &src[i.min(src.len())..line_end.min(src.len())];
            i = line_end;
            if folded && (ch == b' ' || ch == b'\t') && i < keep_start {
                if sep == b" " {
                    sep = b"\n".to_vec();
                } else if !prev_more && !at_start && sep == b"\n" {
                    sep = b"\n\n".to_vec();
                }
                out.extend_from_slice(&sep);
                out.extend_from_slice(line);
                sep = if line_end < end && c(src, line_end) != 0 {
                    vec![c(src, line_end)]
                } else {
                    Vec::new()
                };
                prev_more = true;
            } else {
                out.extend_from_slice(&sep);
                out.extend_from_slice(line);
                sep = if folded && i < keep_start {
                    b" ".to_vec()
                } else {
                    b"\n".to_vec()
                };
                prev_more = false;
            }
            if at_start && !line.is_empty() {
                at_start = false;
            }
        }
        i += 1;
    }
    if n.chomping != Chomp::Strip {
        out.push(b'\n');
    }
    String::from_utf8(out).ok()
}

fn core_value(s: &str) -> KeyVal {
    if matches!(s, "" | "~" | "null" | "Null" | "NULL") {
        return KeyVal::Null;
    }
    if matches!(s, "true" | "True" | "TRUE") {
        return KeyVal::Bool(true);
    }
    if matches!(s, "false" | "False" | "FALSE") {
        return KeyVal::Bool(false);
    }
    let digits = |x: &[u8], f: fn(&u8) -> bool| !x.is_empty() && x.iter().all(f);
    if let Some(rest) = s.strip_prefix("0o")
        && digits(rest.as_bytes(), |c| (b'0'..=b'7').contains(c))
    {
        return big(rest, 8);
    }
    let unsigned = s.strip_prefix(['-', '+']).unwrap_or(s);
    if digits(unsigned.as_bytes(), u8::is_ascii_digit) {
        if unsigned.len() > 15 {
            return KeyVal::Unknown;
        }
        return s.parse::<f64>().map_or(KeyVal::Unknown, KeyVal::Num);
    }
    if let Some(rest) = s.strip_prefix("0x")
        && digits(rest.as_bytes(), u8::is_ascii_hexdigit)
    {
        return big(rest, 16);
    }
    let lower = s.to_ascii_lowercase();
    if matches!(lower.as_str(), ".inf" | "+.inf" | "-.inf" | ".nan") {
        return KeyVal::Num(match lower.as_str() {
            ".nan" => f64::NAN,
            "-.inf" => f64::NEG_INFINITY,
            _ => f64::INFINITY,
        });
    }
    static FLOAT: std::sync::OnceLock<Option<regex::Regex>> = std::sync::OnceLock::new();
    let re = FLOAT.get_or_init(|| {
        regex::Regex::new(
            r"^[-+]?(?:(?:\.[0-9]+|[0-9]+(?:\.[0-9]*)?)[eE][-+]?[0-9]+|\.[0-9]+|[0-9]+\.[0-9]*)$",
        )
        .ok()
    });
    if re.as_ref().is_some_and(|r| r.is_match(s)) {
        let mut norm = String::new();
        let bytes = s.as_bytes();
        for (i, &ch) in bytes.iter().enumerate() {
            if ch == b'.' && (i == 0 || !bytes[i - 1].is_ascii_digit()) {
                norm.push('0');
            }
            norm.push(ch as char);
            if ch == b'.' && !bytes.get(i + 1).is_some_and(u8::is_ascii_digit) {
                norm.push('0');
            }
        }
        return norm.parse::<f64>().map_or(KeyVal::Unknown, KeyVal::Num);
    }
    KeyVal::Str(s.to_string())
}

fn big(digits: &str, radix: u32) -> KeyVal {
    match u64::from_str_radix(digits, radix) {
        Ok(v) if v < (1u64 << 53) => KeyVal::Num(v as f64),
        _ => KeyVal::Unknown,
    }
}

pub struct Tree {
    pub n: Vec<U>,
    pub root: usize,
    pub comments: Vec<usize>,
}

struct Tx<'c, 'a> {
    cst: &'c Cst<'a>,
    text: &'a str,
    line_starts: Vec<usize>,
    t: Tree,
}

impl Tx<'_, '_> {
    fn pt(&self, offset: usize) -> Pt {
        let o = offset.min(self.text.len());
        let line = self.line_starts.partition_point(|&s| s <= o);
        let ls = self.line_starts[line - 1];
        Pt {
            offset,
            line,
            column: utf16_len(&self.text.as_bytes()[ls..o]) + 1,
        }
    }

    fn make(&mut self, ty: Y, start: Pt, end: Pt) -> usize {
        self.t.n.push(U {
            ty,
            start,
            end,
            kids: Vec::new(),
            tag: None,
            anchor: None,
            middle: Vec::new(),
            leading: Vec::new(),
            trailing: None,
            end_comments: Vec::new(),
            indicator: None,
            value: String::new(),
            chomping: Chomp::Clip,
            indent: None,
            name: String::new(),
            params: Vec::new(),
            parent: None,
            empty_pos: false,
            attach: ty.attach(),
        });
        self.t.n.len() - 1
    }

    fn range(&mut self, ty: Y, start: usize, end: usize) -> usize {
        let (s, e) = (self.pt(start), self.pt(end));
        self.make(ty, s, e)
    }

    fn slice(&self, a: usize, b: usize) -> String {
        let bytes = self.text.as_bytes();
        String::from_utf8_lossy(&bytes[a.min(bytes.len())..b.min(bytes.len())]).into_owned()
    }

    fn comment_node(&mut self, start: usize, end: usize) -> usize {
        let id = self.range(Y::Comment, start, end);
        self.t.n[id].value = self.slice(start + 1, end);
        self.t.comments.push(id);
        id
    }

    fn cst_comment(&mut self, id: usize) -> usize {
        let r = self.cst.n[id].range;
        let p = self.cst.n[id].props.first().copied().unwrap_or(r);
        let cid = self.range(Y::Comment, r.0, r.1);
        self.t.n[cid].value = self.slice(p.0 + 1, p.1);
        cid
    }

    fn extract_prop_comments(&mut self, id: usize) -> O<()> {
        for p in self.cst.n[id].props.clone() {
            if c(self.cst.src, p.0) != b'#' {
                return None;
            }
            self.comment_node(p.0, p.1);
        }
        Some(())
    }

    fn extract_comments(&mut self, items: &[It]) -> Vec<It> {
        let mut rest = Vec::new();
        for it in items {
            if let It::N(n) = *it
                && self.cst.n[n].ty == T::Comment
            {
                let cid = self.cst_comment(n);
                self.t.comments.push(cid);
            } else if let It::N(n) = *it
                && self.cst.n[n].ty == T::BlankLine
            {
            } else {
                rest.push(*it);
            }
        }
        rest
    }

    fn content(
        &mut self,
        uid: usize,
        cst: usize,
        tag: &Option<String>,
        indicator_range: Option<(usize, usize)>,
    ) -> O<()> {
        let props = self.cst.n[cst].props.clone();
        let vr_start = self.cst.n[cst].vr.map_or(0, |v| v.0);
        let mut first: Option<(usize, usize)> = None;
        for p in props {
            match c(self.cst.src, p.0) {
                b'!' => {
                    first.get_or_insert(p);
                    let t = self.range(Y::Tag, p.0, p.1);
                    self.t.n[t].value = tag.clone().unwrap_or_default();
                    self.t.n[uid].tag = Some(t);
                }
                b'&' => {
                    first.get_or_insert(p);
                    let a = self.range(Y::Anchor, p.0, p.1);
                    let name = self.cst.n[cst]
                        .props
                        .iter()
                        .find(|q| c(self.cst.src, q.0) == b'&')
                        .map(|q| self.slice(q.0 + 1, q.1))
                        .unwrap_or_default();
                    self.t.n[a].value = name;
                    self.t.n[uid].anchor = Some(a);
                }
                b'#' => {
                    let cm = self.comment_node(p.0, p.1);
                    let is_indicator = indicator_range.is_some_and(|(s, e)| s < p.0 && p.1 < e);
                    if is_indicator {
                        if self.t.n[uid].indicator.is_some() {
                            return None;
                        }
                        self.t.n[uid].indicator = Some(cm);
                    } else if let Some(f) = first
                        && f.1 <= p.0
                        && p.1 <= vr_start
                    {
                        self.t.n[uid].middle.push(cm);
                    }
                }
                _ => return None,
            }
        }
        Some(())
    }

    fn node(&mut self, ast: Option<&Ast>) -> O<Option<usize>> {
        let Some(ast) = ast else {
            return Some(None);
        };
        Some(Some(match ast {
            Ast::Alias { cst } => {
                let vr = self.cst.n[*cst].vr?;
                let id = self.range(Y::Alias, vr.0 - 1, vr.1);
                self.content(id, *cst, &None, None)?;
                self.t.n[id].value = self.slice(vr.0, vr.1);
                id
            }
            Ast::Scalar { cst, tag, .. } => {
                let n = &self.cst.n[*cst];
                let vr = n.vr?;
                match n.ty {
                    T::Plain => {
                        let mut end = vr.1;
                        let bytes = self.text.as_bytes();
                        while end > 0 && end - 1 < bytes.len() && is_js_space_byte_end(bytes, end) {
                            end = prev_char_start(bytes, end);
                        }
                        let id = self.range(Y::Plain, vr.0, end);
                        self.content(id, *cst, tag, None)?;
                        self.t.n[id].value = plain_str(self.cst, *cst)?;
                        id
                    }
                    T::QuoteDouble | T::QuoteSingle => {
                        let ty = if n.ty == T::QuoteDouble {
                            Y::QuoteDouble
                        } else {
                            Y::QuoteSingle
                        };
                        let id = self.range(ty, vr.0, vr.1);
                        self.content(id, *cst, tag, None)?;
                        self.t.n[id].value = quote_str(self.cst, *cst)?;
                        id
                    }
                    T::BlockFolded | T::BlockLiteral => {
                        let ty = if n.ty == T::BlockFolded {
                            Y::BlockFolded
                        } else {
                            Y::BlockLiteral
                        };
                        let header = n.header?;
                        let chomp_len = if n.chomping == Chomp::Clip { 0 } else { 1 };
                        let explicit = header.1 as i64 - header.0 as i64 - 1 - chomp_len != 0;
                        let id = self.range(ty, header.0, vr.1);
                        self.content(id, *cst, tag, Some((header.0, vr.1)))?;
                        self.t.n[id].chomping = n.chomping;
                        self.t.n[id].indent = if explicit { n.block_indent } else { None };
                        id
                    }
                    _ => return None,
                }
            }
            Ast::Map { cst, tag, pairs } => self.map(*cst, tag, pairs)?,
            Ast::Seq { cst, tag, items } => {
                if self.cst.n[*cst].ty == T::FlowSeq {
                    self.flow_seq(*cst, tag, items)?
                } else {
                    self.seq(*cst, tag, items)?
                }
            }
        }))
    }

    fn pair(
        &mut self,
        pair: &Pair,
        flow: bool,
        key_range: Option<(usize, usize)>,
        value_range: Option<(usize, usize)>,
    ) -> O<usize> {
        let kc = self.node(pair.key.as_deref())?;
        let vc = self.node(pair.value.as_deref())?;
        let mk = if kc.is_some() || key_range.is_some() {
            let start = match key_range {
                Some(r) => self.pt(r.0),
                None => self.t.n[kc?].start,
            };
            let end = match kc {
                Some(k) => self.t.n[k].end,
                None => self.pt(key_range?.0 + 1),
            };
            let id = self.make(Y::MappingKey, start, end);
            self.t.n[id].kids = kc.into_iter().collect();
            Some(id)
        } else {
            None
        };
        let mv = if vc.is_some() || value_range.is_some() {
            let start = match value_range {
                Some(r) => self.pt(r.0),
                None => self.t.n[vc?].start,
            };
            let end = match vc {
                Some(v) => self.t.n[v].end,
                None => self.pt(value_range?.0 + 1),
            };
            let id = self.make(Y::MappingValue, start, end);
            self.t.n[id].kids = vc.into_iter().collect();
            Some(id)
        } else {
            None
        };
        let start = match mk {
            Some(k) => self.t.n[k].start,
            None => self.t.n[mv?].start,
        };
        let end = match mv {
            Some(v) => self.t.n[v].end,
            None => self.t.n[mk?].end,
        };
        let mk = match mk {
            Some(k) => k,
            None => {
                let p = self.t.n[mv?].start;
                let id = self.make(Y::MappingKey, p, p);
                self.t.n[id].empty_pos = true;
                id
            }
        };
        let mv = match mv {
            Some(v) => v,
            None => {
                let p = self.t.n[mk].end;
                let id = self.make(Y::MappingValue, p, p);
                self.t.n[id].empty_pos = true;
                id
            }
        };
        let id = self.make(
            if flow {
                Y::FlowMappingItem
            } else {
                Y::MappingItem
            },
            start,
            end,
        );
        self.t.n[id].kids = vec![mk, mv];
        Some(id)
    }

    fn map(&mut self, cst: usize, tag: &Option<String>, pairs: &[Pair]) -> O<usize> {
        let flow = self.cst.n[cst].ty == T::FlowMap;
        let items = self.cst.n[cst].items.clone();
        if flow {
            let rest = self.extract_comments(&items);
            let groups = flow_groups(&rest);
            if groups.len() != pairs.len() {
                return None;
            }
            let mut kids = Vec::new();
            for (pair, g) in pairs.iter().zip(&groups) {
                let (kr, vr) = char_ranges(g);
                kids.push(self.pair(pair, true, kr, vr)?);
            }
            let (It::Ch(_, open), It::Ch(_, close)) = (*rest.first()?, *rest.last()?) else {
                return None;
            };
            let id = self.range(Y::FlowMapping, open, close + 1);
            self.content(id, cst, tag, None)?;
            self.t.n[id].kids = kids;
            return Some(id);
        }
        for it in &items {
            if let It::N(n) = *it
                && matches!(self.cst.n[n].ty, T::MapKey | T::MapValue)
            {
                self.extract_prop_comments(n)?;
            }
        }
        let rest = self.extract_comments(&items);
        let mut groups: Vec<Vec<usize>> = Vec::new();
        let mut cur: Vec<usize> = Vec::new();
        let mut has_key = false;
        for it in &rest {
            let It::N(n) = *it else {
                return None;
            };
            if self.cst.n[n].ty == T::MapValue {
                cur.push(n);
                groups.push(std::mem::take(&mut cur));
                has_key = false;
                continue;
            }
            if has_key {
                groups.push(std::mem::take(&mut cur));
            }
            cur.push(n);
            has_key = true;
        }
        if has_key {
            groups.push(cur);
        }
        if groups.len() != pairs.len() {
            return None;
        }
        let mut kids = Vec::new();
        for (pair, g) in pairs.iter().zip(&groups) {
            let (kr, vr) = if self.cst.n[g[0]].ty == T::MapValue {
                (None, Some(self.cst.n[g[0]].range))
            } else {
                (
                    Some(self.cst.n[g[0]].range),
                    g.get(1).map(|&x| self.cst.n[x].range),
                )
            };
            kids.push(self.pair(pair, false, kr, vr)?);
        }
        let start = self.t.n[*kids.first()?].start;
        let end = self.t.n[*kids.last()?].end;
        let id = self.make(Y::Mapping, start, end);
        self.content(id, cst, tag, None)?;
        self.t.n[id].kids = kids;
        Some(id)
    }

    fn flow_seq(&mut self, cst: usize, tag: &Option<String>, items: &[SeqEl]) -> O<usize> {
        let all = self.cst.n[cst].items.clone();
        let rest = self.extract_comments(&all);
        let groups = flow_groups(&rest);
        let mut kids = Vec::new();
        for (i, item) in items.iter().enumerate() {
            match item {
                SeqEl::Node(n) => {
                    let node = self.node(n.as_deref())??;
                    let (s, e) = (self.t.n[node].start, self.t.n[node].end);
                    let id = self.make(Y::FlowSequenceItem, s, e);
                    self.t.n[id].kids = vec![node];
                    kids.push(id);
                }
                SeqEl::Pair(p) => {
                    let (kr, vr) = char_ranges(groups.get(i)?);
                    kids.push(self.pair(p, true, kr, vr)?);
                }
            }
        }
        let (It::Ch(_, open), It::Ch(_, close)) = (*rest.first()?, *rest.last()?) else {
            return None;
        };
        let id = self.range(Y::FlowSequence, open, close + 1);
        self.content(id, cst, tag, None)?;
        self.t.n[id].kids = kids;
        Some(id)
    }

    fn seq(&mut self, cst: usize, tag: &Option<String>, items: &[SeqEl]) -> O<usize> {
        let all = self.cst.n[cst].items.clone();
        let rest = self.extract_comments(&all);
        if rest.len() != items.len() {
            return None;
        }
        let mut kids = Vec::new();
        for (it, el) in rest.iter().zip(items) {
            let It::N(ci) = *it else {
                return None;
            };
            self.extract_prop_comments(ci)?;
            let SeqEl::Node(n) = el else {
                return None;
            };
            let item = self.node(n.as_deref())?;
            let vs = self.cst.n[ci].vr?.0;
            let start = self.pt(vs);
            let end = match item {
                Some(x) => self.t.n[x].end,
                None => self.pt(vs + 1),
            };
            let id = self.make(Y::SequenceItem, start, end);
            self.t.n[id].kids = item.into_iter().collect();
            kids.push(id);
        }
        let start = self.t.n[*kids.first()?].start;
        let end = self.t.n[*kids.last()?].end;
        let id = self.make(Y::Sequence, start, end);
        self.content(id, cst, tag, None)?;
        self.t.n[id].kids = kids;
        Some(id)
    }

    fn document(&mut self, doc: usize, body_ast: Option<&Ast>) -> O<usize> {
        let d = &self.cst.n[doc];
        let directives = d.directives.clone();
        let contents = d.contents.clone();
        let vr = d.vr?;
        let mut dir_nodes = Vec::new();
        let mut head_comments = Vec::new();
        let mut head_end = Vec::new();
        let mut has_directive = false;
        for &x in directives.iter().rev() {
            match self.cst.n[x].ty {
                T::BlankLine => {}
                T::Comment => {
                    let cm = self.cst_comment(x);
                    if has_directive {
                        head_comments.insert(0, cm);
                    } else {
                        head_end.insert(0, cm);
                    }
                }
                T::Directive => {
                    self.extract_prop_comments(x)?;
                    let r = self.cst.n[x].range;
                    let id = self.range(Y::Directive, r.0, r.1);
                    self.t.n[id].name = self.cst.n[x].name.clone();
                    let raw = self.cst.n[x]
                        .vr
                        .map(|v| self.slice(v.0, v.1))
                        .unwrap_or_default();
                    self.t.n[id].params = split_params(&raw);
                    dir_nodes.insert(0, id);
                    has_directive = true;
                }
                _ => return None,
            }
        }
        let before = &self.text.as_bytes()[..vr.0.min(self.text.len())];
        let mut marker: Option<usize> = None;
        for i in 0..before.len().saturating_sub(2) {
            if &before[i..i + 3] == b"---"
                && std::str::from_utf8(&before[i + 3..])
                    .is_ok_and(|r| r.chars().all(is_js_whitespace))
            {
                marker = Some(i);
                break;
            }
        }
        if let Some(m) = marker
            && m > 0
            && !matches!(before[m - 1], b'\r' | b'\n')
        {
            marker = None;
        }
        let (mut hs, he) = match marker {
            None => (vr.0, vr.0),
            Some(m) => (m, m + 3),
        };
        if let Some(&first) = dir_nodes.first() {
            hs = self.t.n[first].start.offset;
        }
        let head_pos = (self.pt(hs), self.pt(he));
        let head_marker_line = marker.map(|m| self.pt(m).line);
        for &cm in head_comments.iter().chain(&head_end) {
            self.t.comments.push(cm);
        }
        let mut body_comments = Vec::new();
        let mut doc_trailing = Vec::new();
        let mut head_trailing = Vec::new();
        let mut has_content = false;
        for &x in contents.iter().rev() {
            match self.cst.n[x].ty {
                T::BlankLine => {}
                T::Comment => {
                    let cm = self.cst_comment(x);
                    if head_marker_line == Some(self.t.n[cm].start.line) {
                        head_trailing.insert(0, cm);
                    } else if has_content {
                        body_comments.insert(0, cm);
                    } else if self.t.n[cm].start.offset >= vr.1 {
                        doc_trailing.insert(0, cm);
                    } else {
                        body_comments.insert(0, cm);
                    }
                }
                _ => has_content = true,
            }
        }
        if doc_trailing.len() > 1 || head_trailing.len() > 1 {
            return None;
        }
        let content = self.node(body_ast)?;
        let has_end_marker = self
            .text
            .as_bytes()
            .get(vr.1..)
            .is_some_and(|r| r.starts_with(b"..."));
        let mut orig_end = if has_end_marker {
            vr.1.saturating_sub(1)
        } else {
            vr.1
        };
        if orig_end > 0 && self.text.as_bytes().get(orig_end - 1) == Some(&b'\r') {
            orig_end -= 1;
        }
        let body_start = match content {
            Some(x) => self.t.n[x].start.offset,
            None => orig_end,
        };
        let (bs, be) = (self.pt(body_start), self.pt(orig_end));
        let doc_end = if has_end_marker {
            self.pt(vr.1 + 3)
        } else {
            be
        };
        for &cm in &body_comments {
            self.t.comments.push(cm);
        }
        let body = self.make(Y::DocumentBody, bs, be);
        self.t.n[body].kids = content.into_iter().collect();
        let head = self.make(Y::DocumentHead, head_pos.0, head_pos.1);
        self.t.n[head].kids = dir_nodes;
        self.t.n[head].end_comments = head_end;
        if let Some(&ht) = head_trailing.first() {
            self.t.comments.push(ht);
            self.t.n[head].trailing = Some(ht);
        }
        let dt = doc_trailing.first().copied();
        if let Some(d) = dt {
            self.t.comments.push(d);
        }
        let hstart = self.t.n[head].start;
        let id = self.make(Y::Document, hstart, doc_end);
        self.t.n[id].trailing = dt;
        self.t.n[id].kids = vec![head, body];
        Some(id)
    }
}

fn split_params(raw: &str) -> Vec<String> {
    if raw.is_empty() {
        return Vec::new();
    }
    let t = js_trim(raw);
    let mut out = Vec::new();
    let mut cur = String::new();
    let mut in_sep = false;
    for ch in t.chars() {
        if ch == ' ' || ch == '\t' {
            if !in_sep {
                out.push(std::mem::take(&mut cur));
                in_sep = true;
            }
        } else {
            in_sep = false;
            cur.push(ch);
        }
    }
    out.push(cur);
    out
}

fn is_js_space_byte_end(bytes: &[u8], end: usize) -> bool {
    let start = prev_char_start(bytes, end);
    std::str::from_utf8(&bytes[start..end]).is_ok_and(|s| s.chars().all(is_js_whitespace))
}

fn prev_char_start(bytes: &[u8], end: usize) -> usize {
    let mut i = end - 1;
    while i > 0 && (bytes[i] & 0xC0) == 0x80 {
        i -= 1;
    }
    i
}

fn flow_groups(items: &[It]) -> Vec<Vec<It>> {
    let mut groups = Vec::new();
    let mut start = 1;
    let mut has_item = false;
    let len = items.len();
    for i in 1..len.saturating_sub(1) {
        if let It::Ch(b',', _) = items[i] {
            groups.push(items[start..i].to_vec());
            start = i + 1;
            has_item = false;
            continue;
        }
        has_item = true;
    }
    if has_item {
        groups.push(items[start..len - 1].to_vec());
    }
    groups
}

type Span = Option<(usize, usize)>;

fn char_ranges(group: &[It]) -> (Span, Span) {
    let find = |ch: u8| {
        group.iter().find_map(|it| match *it {
            It::Ch(c, o) if c == ch => Some((o, o + 1)),
            _ => None,
        })
    };
    (find(b'?'), find(b':'))
}

fn remove_blank_lines(cst: &mut Cst<'_>) {
    for i in 0..cst.n.len() {
        let n = &mut cst.n[i];
        let items: Vec<It> = std::mem::take(&mut n.items);
        let contents = std::mem::take(&mut n.contents);
        let directives = std::mem::take(&mut n.directives);
        let keep = |id: &usize, all: &[super::cst::C]| all[*id].ty != T::BlankLine;
        let all = &cst.n;
        let items: Vec<It> = items
            .into_iter()
            .filter(|it| match it {
                It::N(x) => keep(x, all),
                It::Ch(..) => true,
            })
            .collect();
        let contents: Vec<usize> = contents.into_iter().filter(|x| keep(x, all)).collect();
        let directives: Vec<usize> = directives.into_iter().filter(|x| keep(x, all)).collect();
        let n = &mut cst.n[i];
        n.items = items;
        n.contents = contents;
        n.directives = directives;
    }
}

/// yaml-unist-parser's `parse`: the tree, or `None` where yaml or the transform would throw.
pub fn parse(text: &str) -> O<Tree> {
    if text.as_bytes().contains(&0) {
        return None;
    }
    let mut cst = Cst::parse(text.as_bytes());
    if cst.has_error {
        return None;
    }
    let docs = cst.docs.clone();
    let mut bodies = Vec::new();
    for &doc in &docs {
        let mut r = Resolver {
            cst: &mut cst,
            anchors: Default::default(),
            prefixes: Vec::new(),
        };
        let mut version_seen = false;
        for d in r.cst.n[doc].directives.clone() {
            if r.cst.n[d].ty != T::Directive {
                continue;
            }
            let name = r.cst.n[d].name.clone();
            let raw = r.cst.n[d].vr.map(|v| r.text(v)).unwrap_or_default();
            let params = split_params(&raw);
            match name.as_str() {
                "TAG" => {
                    let (Some(h), Some(p)) = (
                        params.first().filter(|s| !s.is_empty()),
                        params.get(1).filter(|s| !s.is_empty()),
                    ) else {
                        return None;
                    };
                    if r.prefixes.iter().any(|(x, _)| x == h) {
                        return None;
                    }
                    r.prefixes.push((h.clone(), p.clone()));
                }
                "YAML" | "YAML:1.0" => {
                    if version_seen {
                        return None;
                    }
                    version_seen = true;
                    let v = if name == "YAML:1.0" {
                        "1.0".to_string()
                    } else {
                        params.first().cloned().unwrap_or_default()
                    };
                    if v.is_empty() || v == "1.0" || v == "1.1" {
                        return None;
                    }
                }
                _ => {}
            }
        }
        let mut body = None;
        for x in r.cst.n[doc].contents.clone() {
            match r.cst.n[x].ty {
                T::Comment | T::BlankLine => {}
                _ => {
                    if body.is_some() {
                        return None;
                    }
                    body = Some(r.resolve(x)?);
                }
            }
        }
        bodies.push(body);
    }
    remove_blank_lines(&mut cst);
    let mut line_starts = vec![0];
    for (i, b) in text.bytes().enumerate() {
        if b == b'\n' {
            line_starts.push(i + 1);
        }
    }
    let mut tx = Tx {
        cst: &cst,
        text,
        line_starts,
        t: Tree {
            n: Vec::new(),
            root: 0,
            comments: Vec::new(),
        },
    };
    let mut doc_nodes = Vec::new();
    for (doc, body) in docs.iter().zip(&bodies) {
        doc_nodes.push(tx.document(*doc, body.as_ref())?);
    }
    let root = tx.range(Y::Root, 0, text.len());
    tx.t.n[root].kids = doc_nodes;
    tx.t.root = root;
    let mut t = tx.t;
    attach(&mut t)?;
    update_positions(&mut t, root);
    remove_fake_nodes(&mut t, root);
    Some(t)
}

fn define_parents(t: &mut Tree, id: usize, parent: Option<usize>) {
    t.n[id].parent = parent;
    let n = t.n[id].clone();
    for k in n.kids {
        define_parents(t, k, Some(id));
    }
    for x in n
        .anchor
        .into_iter()
        .chain(n.tag)
        .chain(n.leading)
        .chain(n.middle)
        .chain(n.indicator)
        .chain(n.trailing)
        .chain(n.end_comments)
    {
        define_parents(t, x, Some(id));
    }
}

#[derive(Clone, Default)]
struct Row {
    comment: Option<usize>,
    leading: Option<usize>,
    trailing_attach: Option<usize>,
    trailing: Option<usize>,
}

fn init_table(t: &Tree, table: &mut [Row], id: usize) {
    let n = &t.n[id];
    if n.start.offset == n.end.offset {
        return;
    }
    if n.attach & LEAD != 0 {
        let row = &mut table[n.start.line - 1];
        if row
            .leading
            .is_none_or(|l| n.start.column < t.n[l].start.column)
        {
            row.leading = Some(id);
        }
    }
    if n.attach & TRAIL != 0 && n.end.column > 1 && n.ty != Y::Document && n.ty != Y::DocumentHead {
        let row = &mut table[n.end.line - 1];
        if row
            .trailing_attach
            .is_none_or(|l| n.end.column >= t.n[l].end.column)
        {
            row.trailing_attach = Some(id);
        }
    }
    if !matches!(
        n.ty,
        Y::Root | Y::Document | Y::DocumentHead | Y::DocumentBody
    ) {
        let mut lines = vec![n.end.line];
        if n.start.line != n.end.line {
            lines.push(n.start.line);
        }
        for line in lines {
            let row = &mut table[line - 1];
            if row
                .trailing
                .is_none_or(|l| n.end.column >= t.n[l].end.column)
            {
                row.trailing = Some(id);
            }
        }
    }
    for &k in &t.n[id].kids {
        init_table(t, table, k);
    }
}

fn is_explicit_key(t: &Tree, id: usize) -> bool {
    let n = &t.n[id];
    !n.empty_pos && (n.kids.is_empty() || n.start.offset != t.n[n.kids[0]].start.offset)
}

fn should_own_end_comment(t: &Tree, id: usize, cm: usize) -> bool {
    let n = &t.n[id];
    let c = &t.n[cm];
    if n.start.offset < c.start.offset
        && n.end.offset > c.end.offset
        && matches!(n.ty, Y::FlowMapping | Y::FlowSequence)
    {
        return n.kids.is_empty() || c.start.line > t.n[*n.kids.last().unwrap_or(&id)].end.line;
    }
    if c.end.offset < n.end.offset {
        return false;
    }
    match n.ty {
        Y::SequenceItem => c.start.column > n.start.column,
        Y::MappingKey | Y::MappingValue => {
            let Some(p) = n.parent else {
                return false;
            };
            c.start.column > t.n[p].start.column
                && (n.kids.is_empty()
                    || (n.kids.len() == 1
                        && !matches!(t.n[n.kids[0]].ty, Y::BlockFolded | Y::BlockLiteral)))
                && (n.ty == Y::MappingValue || is_explicit_key(t, id))
        }
        _ => false,
    }
}

fn attach(t: &mut Tree) -> O<()> {
    let root = t.root;
    define_parents(t, root, None);
    let lines = t.n[root].end.line;
    let mut table = vec![Row::default(); lines];
    for &cm in &t.comments {
        let l = t.n[cm].start.line;
        table[l - 1].comment = Some(cm);
    }
    init_table(t, &mut table, root);
    let mut docs: std::collections::VecDeque<usize> = t.n[root].kids.iter().copied().collect();
    let mut comments = t.comments.clone();
    comments.sort_by_key(|&c| t.n[c].start.offset);
    for cm in comments {
        if t.n[cm].parent.is_some() {
            continue;
        }
        while docs.len() > 1 && t.n[cm].start.line > t.n[docs[0]].end.line {
            docs.pop_front();
        }
        let doc = *docs.front()?;
        attach_comment(t, &table, cm, doc)?;
    }
    Some(())
}

fn attach_comment(t: &mut Tree, table: &[Row], cm: usize, doc: usize) -> O<()> {
    let line = t.n[cm].start.line;
    if let Some(ta) = table[line - 1].trailing_attach {
        if t.n[ta].trailing.is_some() {
            return None;
        }
        t.n[cm].parent = Some(ta);
        t.n[ta].trailing = Some(cm);
        return Some(());
    }
    let doc_start = t.n[doc].start.line;
    let mut l = line;
    while l >= doc_start && l >= 1 {
        let row = &table[l - 1];
        let mut current = match row.trailing {
            Some(x) => x,
            None => {
                if l != line
                    && let Some(c2) = row.comment
                {
                    t.n[c2].parent?
                } else {
                    l -= 1;
                    continue;
                }
            }
        };
        if matches!(t.n[current].ty, Y::Sequence | Y::Mapping) {
            current = *t.n[current].kids.first()?;
        }
        if t.n[current].ty == Y::MappingItem {
            let k = t.n[current].kids.clone();
            current = if is_explicit_key(t, k[0]) { k[0] } else { k[1] };
        }
        loop {
            if should_own_end_comment(t, current, cm) {
                t.n[cm].parent = Some(current);
                t.n[current].end_comments.push(cm);
                return Some(());
            }
            match t.n[current].parent {
                Some(p) => current = p,
                None => break,
            }
        }
        break;
    }
    let doc_end = t.n[doc].end.line;
    for l in line + 1..=doc_end {
        if let Some(la) = table.get(l - 1).and_then(|r| r.leading) {
            t.n[cm].parent = Some(la);
            t.n[la].leading.push(cm);
            return Some(());
        }
    }
    let body = t.n[doc].kids[1];
    t.n[cm].parent = Some(body);
    t.n[body].end_comments.push(cm);
    Some(())
}

fn update_positions(t: &mut Tree, id: usize) {
    if !t.n[id].ty.has_children() {
        return;
    }
    for k in t.n[id].kids.clone() {
        update_positions(t, k);
    }
    if t.n[id].ty == Y::Document {
        let (head, body) = (t.n[id].kids[0], t.n[id].kids[1]);
        if t.n[head].start.offset == t.n[head].end.offset {
            let p = t.n[body].start;
            t.n[head].start = p;
            t.n[head].end = p;
        } else if t.n[body].start.offset == t.n[body].end.offset {
            let p = t.n[head].end;
            t.n[body].start = p;
            t.n[body].end = p;
        }
    }
    let mut start = t.n[id].start;
    let mut end = t.n[id].end;
    let mut upd_start = |p: Pt| {
        if p.offset < start.offset {
            start = p;
        }
    };
    let ec = t.n[id].end_comments.clone();
    if let (Some(&f), Some(&l)) = (ec.first(), ec.last()) {
        upd_start(t.n[f].start);
        if t.n[l].end.offset > end.offset {
            end = t.n[l].end;
        }
    }
    let kids = t.n[id].kids.clone();
    if let (Some(&f), Some(&l)) = (kids.first(), kids.last()) {
        upd_start(t.n[f].start);
        if t.n[l].end.offset > end.offset {
            end = t.n[l].end;
        }
        if let Some(&lc) = t.n[f].leading.first() {
            upd_start(t.n[lc].start);
        }
        if let Some(tg) = t.n[f].tag {
            upd_start(t.n[tg].start);
        }
        if let Some(an) = t.n[f].anchor {
            upd_start(t.n[an].start);
        }
        if let Some(tc) = t.n[l].trailing
            && t.n[tc].end.offset > end.offset
        {
            end = t.n[tc].end;
        }
    }
    t.n[id].start = start;
    t.n[id].end = end;
}

fn remove_fake_nodes(t: &mut Tree, id: usize) {
    if !t.n[id].ty.has_children() {
        return;
    }
    if t.n[id].kids.len() == 1 {
        let k = t.n[id].kids[0];
        let c = &t.n[k];
        if c.ty == Y::Plain && c.tag.is_none() && c.anchor.is_none() && c.value.is_empty() {
            t.n[id].kids.clear();
            return;
        }
    }
    for k in t.n[id].kids.clone() {
        remove_fake_nodes(t, k);
    }
}
