//! `src/language-markdown/`: print-preprocess.js, printer-markdown.js, print-paragraph.js,
//! print-sentence.js, print-whitespace.js, print/table.js, utils.js and embed.js, on the tree
//! `remark.rs` builds. Offsets count characters.

mod constants;
mod entities;
mod languages;
pub(crate) mod remark;

use remark::Node;

use crate::comments::is_js_whitespace;
use crate::doc::*;
use crate::options::{Options, Parser, ProseWrap};
use crate::printer::{R, bail};

const NON_CJK: u8 = 0;
const CJ_LETTER: u8 = 1;
const K_LETTER: u8 = 2;
const CJK_PUNCT: u8 = 3;

fn re(pattern: &str) -> Option<regex::Regex> {
    regex::Regex::new(pattern).ok()
}

struct Res {
    cjk_split: regex::Regex,
    punct: regex::Regex,
    hangul: regex::Regex,
    word_underscore: regex::Regex,
}

fn res() -> Option<&'static Res> {
    static R: std::sync::OnceLock<Option<Res>> = std::sync::OnceLock::new();
    R.get_or_init(|| {
        let p = constants::PUNCTUATION;
        Some(Res {
            cjk_split: re(constants::CJK)?,
            punct: re(&format!("^{p}"))?,
            hangul: re(r"\p{Script_Extensions=Hangul}")?,
            word_underscore: re(&format!("(^|{p})(_+)|(_+)({p}|$)"))?,
        })
    })
    .as_ref()
}

fn is_punct(c: Option<char>) -> bool {
    let Some(c) = c else {
        return false;
    };
    res().is_some_and(|r| r.punct.is_match(&c.to_string()))
}

fn html_ws(c: char) -> bool {
    matches!(c, '\t' | '\n' | '\x0c' | '\r' | ' ')
}

fn word(value: &str, kind: u8, is_cj: bool, lead: bool, trail: bool) -> Node {
    let mut n = remark::node("word");
    n.value = value.to_string();
    n.kind = kind;
    n.has_pos = is_cj;
    n.leading_punct = lead;
    n.trailing_punct = trail;
    n
}

fn whitespace(value: &str) -> Node {
    let mut n = remark::node("whitespace");
    n.value = value.to_string();
    n
}

fn is_cj(n: &Node) -> bool {
    n.ty == "word" && n.has_pos
}

/// `splitText`: words and whitespace of a text value.
fn split_text(text: &str) -> R<Vec<Node>> {
    let Some(r) = res() else {
        return bail("markdown regex");
    };
    let mut nodes: Vec<Node> = Vec::new();
    let mut tokens: Vec<(bool, &str)> = Vec::new();
    let mut last = 0;
    let bytes = text.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if matches!(bytes[i], b'\t' | b'\n' | b' ') {
            let st = i;
            while i < bytes.len() && matches!(bytes[i], b'\t' | b'\n' | b' ') {
                i += 1;
            }
            tokens.push((false, &text[last..st]));
            tokens.push((true, &text[st..i]));
            last = i;
        } else {
            i += 1;
        }
    }
    tokens.push((false, &text[last..]));
    let n_tokens = tokens.len();
    for (index, (is_ws, token)) in tokens.into_iter().enumerate() {
        if is_ws {
            nodes.push(whitespace(if token.contains('\n') { "\n" } else { " " }));
            continue;
        }
        if (index == 0 || index == n_tokens - 1) && token.is_empty() {
            continue;
        }
        let mut inner: Vec<(bool, &str)> = Vec::new();
        let mut last = 0;
        for m in r.cjk_split.find_iter(token) {
            inner.push((false, &token[last..m.start()]));
            inner.push((true, m.as_str()));
            last = m.end();
        }
        inner.push((false, &token[last..]));
        let n_inner = inner.len();
        for (ii, (is_cjk, t)) in inner.into_iter().enumerate() {
            if (ii == 0 || ii == n_inner - 1) && t.is_empty() {
                continue;
            }
            let node = if !is_cjk {
                if t.is_empty() {
                    continue;
                }
                word(
                    t,
                    NON_CJK,
                    false,
                    is_punct(t.chars().next()),
                    is_punct(t.chars().last()),
                )
            } else if r.punct.is_match(t) {
                word(t, CJK_PUNCT, true, true, true)
            } else if r.hangul.is_match(t) {
                word(t, K_LETTER, false, false, false)
            } else {
                word(t, CJ_LETTER, true, false, false)
            };
            if let Some(last) = nodes.last()
                && last.ty == "word"
            {
                let between = (last.kind == NON_CJK && node.kind == CJK_PUNCT)
                    || (last.kind == CJK_PUNCT && node.kind == NON_CJK);
                if !between && !last.value.contains('\u{3000}') && !node.value.contains('\u{3000}')
                {
                    nodes.push(whitespace(""));
                }
            }
            nodes.push(node);
        }
    }
    Ok(nodes)
}

fn chars_slice(text: &[char], a: i64, b: i64) -> String {
    let len = text.len() as i64;
    let a = a.clamp(0, len) as usize;
    let b = b.clamp(0, len) as usize;
    if a >= b {
        return String::new();
    }
    text[a..b].iter().collect()
}

struct Pre<'a> {
    text: &'a [char],
    tab_width: usize,
}

impl Pre<'_> {
    fn restore_unescaped(&self, n: &mut Node) {
        if n.ty == "text" {
            let v = &n.value;
            let count = v.chars().count();
            let single = count == 1 || (count == 2 && v.starts_with('\\'));
            if v != "*" && v != "_" && single && (n.end.offset - n.start.offset) as usize != count {
                let text = chars_slice(self.text, n.start.offset, n.end.offset);
                let newline_bq = text.starts_with('\n')
                    && text[1..].trim_start_matches(' ').starts_with('>')
                    && text[1..]
                        .trim_start_matches(' ')
                        .chars()
                        .all(|c| c == ' ' || c == '>');
                if !newline_bq {
                    n.value = text;
                }
            }
        }
        for c in &mut n.children {
            self.restore_unescaped(c);
        }
    }

    fn merge_texts(n: &mut Node) {
        if !n.children.is_empty() {
            let old = std::mem::take(&mut n.children);
            for c in old {
                if let Some(last) = n.children.last_mut()
                    && last.ty == "text"
                    && c.ty == "text"
                {
                    last.value.push_str(&c.value);
                    last.end = c.end;
                    continue;
                }
                n.children.push(c);
            }
        }
        for c in &mut n.children {
            Self::merge_texts(c);
        }
    }

    fn mark_indented_code(&self, n: &mut Node) -> bool {
        let mut found = false;
        if n.ty == "code" {
            let t = chars_slice(self.text, n.start.offset, n.end.offset);
            let t2 = t.strip_prefix('\n').unwrap_or(&t);
            n.is_indented = t2.starts_with("    ") || t2.starts_with('\t');
            found = n.is_indented;
        }
        let mut any = false;
        for c in &mut n.children {
            if self.mark_indented_code(c) {
                any = true;
            }
        }
        if any && n.ty == "list" {
            n.has_indented_codeblock = true;
        }
        found || any
    }

    /// `getOrderedListItemInfo`: `Number(numberText)` is a double.
    fn ordered_info(&self, item: &Node) -> (f64, usize) {
        let t = chars_slice(self.text, item.start.offset, item.end.offset);
        let t = t.trim_start_matches(is_js_whitespace);
        let digits: String = t.chars().take_while(char::is_ascii_digit).collect();
        let rest = &t[digits.len()..];
        let rest = rest.strip_prefix(['.', ')']).unwrap_or(rest);
        let spaces = rest.chars().take_while(|c| is_js_whitespace(*c)).count();
        (digits.parse().unwrap_or(f64::NAN), spaces)
    }

    fn item_start(item: &Node) -> i64 {
        match item.children.first() {
            None => -1,
            Some(c) => c.start.column as i64 - 1,
        }
    }

    fn is_aligned(&self, list: &Node) -> bool {
        if !list.ordered {
            return true;
        }
        let first = &list.children[0];
        if self.ordered_info(first).1 > 1 {
            return true;
        }
        let first_start = Self::item_start(first);
        if first_start == -1 {
            return false;
        }
        let tw = self.tab_width as i64;
        if list.children.len() == 1 {
            return tw != 0 && first_start % tw == 0;
        }
        let second_start = Self::item_start(&list.children[1]);
        if first_start != second_start {
            return false;
        }
        if tw != 0 && first_start % tw == 0 {
            return true;
        }
        self.ordered_info(&list.children[1]).1 > 1
    }

    fn mark_aligned(&self, n: &mut Node, parent_unaligned: bool) {
        let mut unaligned = parent_unaligned;
        if n.ty == "list" && !n.children.is_empty() {
            n.is_aligned = if parent_unaligned {
                false
            } else {
                self.is_aligned(n)
            };
            if !n.is_aligned {
                unaligned = true;
            }
        }
        for c in &mut n.children {
            self.mark_aligned(c, unaligned);
        }
    }

    fn sentences(n: &mut Node) -> R<()> {
        let is_para = n.ty == "paragraph";
        let count = n.children.len();
        for (i, c) in n.children.iter_mut().enumerate() {
            if c.ty == "text" {
                let mut v: &str = &c.value;
                if is_para {
                    if i == 0 {
                        v = v.trim_start_matches(html_ws);
                    }
                    if i == count - 1 {
                        v = v.trim_end_matches(html_ws);
                    }
                }
                let children = split_text(v)?;
                c.ty = "sentence";
                c.children = children;
                c.value.clear();
            } else {
                Self::sentences(c)?;
            }
        }
        Ok(())
    }
}

fn is_inline_type(t: &str) -> bool {
    matches!(
        t,
        "liquidNode"
            | "inlineCode"
            | "emphasis"
            | "esComment"
            | "strong"
            | "delete"
            | "wikiLink"
            | "link"
            | "linkReference"
            | "image"
            | "imageReference"
            | "footnote"
            | "footnoteReference"
            | "sentence"
            | "whitespace"
            | "word"
            | "break"
            | "inlineMath"
    )
}

fn is_inline_wrapper(t: &str) -> bool {
    is_inline_type(t) || matches!(t, "tableCell" | "paragraph" | "heading")
}

fn is_autolink(n: Option<&Node>) -> bool {
    let Some(n) = n else {
        return false;
    };
    n.ty == "link"
        && n.children.len() == 1
        && n.start.offset == n.children[0].start.offset
        && n.end.offset == n.children[0].end.offset
}

fn ignore_kind(n: &Node) -> Option<&'static str> {
    if n.ty == "html" {
        let v = n.value.as_str();
        if v.len() < 7 {
            return None;
        }
        let inner = v.strip_prefix("<!--")?.strip_suffix("-->")?;
        return match inner.trim_matches(is_js_whitespace) {
            "prettier-ignore" => Some("next"),
            "prettier-ignore-start" => Some("start"),
            "prettier-ignore-end" => Some("end"),
            _ => None,
        };
    }
    None
}

fn get_preferred_quote(text: &str, single: bool) -> char {
    let (pref, alt) = if single { ('\'', '"') } else { ('"', '\'') };
    let p = text.chars().filter(|&c| c == pref).count();
    let a = text.chars().filter(|&c| c == alt).count();
    if p > a { alt } else { pref }
}

fn runs(text: &str, ch: char) -> Vec<usize> {
    let mut out = Vec::new();
    let mut count = 0;
    for c in text.chars() {
        if c == ch {
            count += 1;
        } else if count > 0 {
            out.push(count);
            count = 0;
        }
    }
    if count > 0 {
        out.push(count);
    }
    out
}

fn min_not_present(text: &str, ch: char) -> usize {
    let r = runs(text, ch);
    if r.is_empty() {
        return 0;
    }
    let max = r.iter().copied().max().unwrap_or(0);
    (1..max).find(|i| !r.contains(i)).unwrap_or(max + 1)
}

fn max_continuous(text: &str, ch: char) -> usize {
    runs(text, ch).into_iter().max().unwrap_or(0)
}

fn print_url(url: &str, dangerous: &[char]) -> String {
    if url.contains(' ') || dangerous.iter().any(|c| url.contains(*c)) {
        format!("<{}>", url.replace('<', "%3C").replace('>', "%3E"))
    } else {
        url.to_string()
    }
}

fn print_title(title: Option<&str>, single: bool, space: bool) -> String {
    let Some(title) = title.filter(|t| !t.is_empty()) else {
        return String::new();
    };
    if space {
        return format!(" {}", print_title(Some(title), single, false));
    }
    let mut t = String::new();
    let chars: Vec<char> = title.chars().collect();
    for (i, &c) in chars.iter().enumerate() {
        if c == '\\' && matches!(chars.get(i + 1), Some('"' | '\'' | ')')) {
            continue;
        }
        t.push(c);
    }
    if t.contains('"') && t.contains('\'') && !t.contains(')') {
        return format!("({t})");
    }
    let q = get_preferred_quote(&t, single);
    let t = t.replace('\\', "\\\\").replace(q, &format!("\\{q}"));
    format!("{q}{t}{q}")
}

fn fill_parts_from(docs: Vec<Doc>) -> Doc {
    let mut parts: Vec<Doc> = vec![s("")];
    fn rec(parts: &mut Vec<Doc>, docs: Vec<Doc>) {
        for d in docs {
            match d {
                Doc::Array(inner) => rec(parts, inner),
                Doc::Fill(fp) => {
                    let mut it = fp.into_iter();
                    let head = it.next().unwrap_or_else(|| s(""));
                    let prev = parts.pop().unwrap_or_else(|| s(""));
                    parts.push(concat(vec![prev, head]));
                    parts.extend(it);
                }
                other => {
                    let prev = parts.pop().unwrap_or_else(|| s(""));
                    parts.push(concat(vec![prev, other]));
                }
            }
        }
    }
    rec(&mut parts, docs);
    fill(parts)
}

fn map_strings(doc: &Doc) -> Doc {
    match doc {
        Doc::Str(t) if t.contains('\n') => join(
            &literalline(),
            t.split('\n').map(|p| s(p.to_string())).collect(),
        ),
        Doc::Array(v) => Doc::Array(v.iter().map(map_strings).collect()),
        Doc::Fill(v) => Doc::Fill(v.iter().map(map_strings).collect()),
        Doc::Indent(c) => Doc::Indent(Box::new(map_strings(c))),
        Doc::Align(a, c) => Doc::Align(a.clone(), Box::new(map_strings(c))),
        Doc::Group {
            contents,
            brk,
            expanded,
            id,
        } => match expanded {
            Some(states) => cond_group(
                states.iter().map(map_strings).collect(),
                brk.get(),
                id.clone(),
            ),
            None => Doc::Group {
                contents: Box::new(map_strings(contents)),
                brk: brk.clone(),
                expanded: None,
                id: id.clone(),
            },
        },
        Doc::IfBreak {
            brk,
            flat,
            group_id,
        } => Doc::IfBreak {
            brk: Box::new(map_strings(brk)),
            flat: Box::new(map_strings(flat)),
            group_id: group_id.clone(),
        },
        Doc::IndentIfBreak {
            contents,
            group_id,
            negate,
        } => Doc::IndentIfBreak {
            contents: Box::new(map_strings(contents)),
            group_id: group_id.clone(),
            negate: *negate,
        },
        Doc::LineSuffix(c) => Doc::LineSuffix(Box::new(map_strings(c))),
        Doc::Label(l, c) => Doc::Label(l.clone(), Box::new(map_strings(c))),
        Doc::Shared(inner) => map_strings(inner),
        other => other.clone(),
    }
}

pub(crate) fn strip_trailing_hardline(doc: Doc) -> Doc {
    strip_cleaned(clean_doc(&doc))
}

fn strip_cleaned(doc: Doc) -> Doc {
    fn parts(mut v: Vec<Doc>) -> Vec<Doc> {
        while v.len() >= 2
            && matches!(v[v.len() - 2], Doc::Line { .. })
            && matches!(v[v.len() - 1], Doc::BreakParent)
        {
            v.truncate(v.len() - 2);
        }
        if let Some(last) = v.pop() {
            v.push(strip_cleaned(last));
        }
        v
    }
    match doc {
        Doc::Indent(c) => Doc::Indent(Box::new(strip_cleaned(*c))),
        Doc::IndentIfBreak {
            contents,
            group_id,
            negate,
        } => Doc::IndentIfBreak {
            contents: Box::new(strip_cleaned(*contents)),
            group_id,
            negate,
        },
        Doc::Group {
            contents,
            brk,
            expanded,
            id,
        } => Doc::Group {
            contents: Box::new(strip_cleaned(*contents)),
            brk,
            expanded,
            id,
        },
        Doc::LineSuffix(c) => Doc::LineSuffix(Box::new(strip_cleaned(*c))),
        Doc::Label(l, c) => Doc::Label(l, Box::new(strip_cleaned(*c))),
        Doc::IfBreak {
            brk,
            flat,
            group_id,
        } => Doc::IfBreak {
            brk: Box::new(strip_cleaned(*brk)),
            flat: Box::new(strip_cleaned(*flat)),
            group_id,
        },
        Doc::Fill(v) => Doc::Fill(parts(v)),
        Doc::Array(v) => Doc::Array(parts(v)),
        Doc::Str(t) => s(t.trim_end_matches(['\n', '\r']).to_string()),
        Doc::Shared(inner) => strip_cleaned((*inner).clone()),
        other => other,
    }
}

fn embed_parser(lang: &str) -> Option<&'static str> {
    let l = languages::LANGUAGES;
    l.iter()
        .find(|x| x.name == lang)
        .or_else(|| l.iter().find(|x| x.aliases.contains(&lang)))
        .or_else(|| {
            let ext = format!(".{lang}");
            l.iter().find(|x| x.extensions.contains(&ext.as_str()))
        })
        .map(|x| x.parser)
}

struct Pr<'a> {
    text: &'a [char],
    opts: &'a Options,
}

type Path<'a> = Vec<(&'a Node, usize)>;

impl<'a> Pr<'a> {
    fn slice(&self, n: &Node) -> String {
        chars_slice(self.text, n.start.offset, n.end.offset)
    }

    fn char_at(&self, offset: i64) -> Option<char> {
        if offset < 0 {
            return None;
        }
        self.text.get(offset as usize).copied()
    }

    fn parent<'p>(path: &'p Path<'a>) -> Option<&'a Node> {
        path.len().checked_sub(2).map(|i| path[i].0)
    }

    fn sibling(path: &Path<'a>, delta: isize) -> Option<&'a Node> {
        let parent = Self::parent(path)?;
        let idx = path.last()?.1 as isize + delta;
        if idx < 0 {
            return None;
        }
        parent.children.get(idx as usize)
    }

    fn has_ancestor(path: &Path<'a>, f: impl Fn(&Node) -> bool) -> bool {
        path[..path.len() - 1].iter().any(|(n, _)| f(n))
    }

    fn print(&self, path: &mut Path<'a>) -> R {
        let (node, idx) = *path.last().ok_or(()).or_else(|_| bail("empty path"))?;
        if idx > 0
            && let Some(prev) = Self::sibling(path, -1)
            && ignore_kind(prev) == Some("next")
        {
            return Ok(s(self.slice(node)));
        }
        if let Some(d) = self.embed(node)? {
            return Ok(d);
        }
        self.generic(path)
    }

    fn embed(&self, node: &Node) -> R<Option<Doc>> {
        if node.ty == "code"
            && let Some(lang) = &node.lang
        {
            let Some(parser) = embed_parser(lang) else {
                return Ok(None);
            };
            let mut o = self.opts.clone();
            o.parent_markdown = true;
            match parser {
                "babel" => {
                    o.parser = Parser::Babel;
                    o.filepath = "embedded.js".to_string();
                }
                "typescript" if lang == "ts" || lang == "typescript" => {
                    o.parser = Parser::Typescript;
                    o.filepath = "dummy.ts".to_string();
                }
                "typescript" if lang == "tsx" => {
                    o.parser = Parser::Typescript;
                    o.filepath = "dummy.tsx".to_string();
                }
                "json" => o.parser = Parser::Json,
                "jsonc" => o.parser = Parser::Jsonc,
                "json5" => o.parser = Parser::Json5,
                "json-stringify" => o.parser = Parser::JsonStringify,
                "graphql" => o.parser = Parser::Graphql,
                "yaml" => o.parser = Parser::Yaml,
                "markdown" => o.parser = Parser::Markdown,
                "css" => o.parser = Parser::Css,
                "scss" => o.parser = Parser::Scss,
                "less" => o.parser = Parser::Less,
                _ => return bail(format!("embedded {parser}")),
            }
            let code = self.fenced_value(node);
            if o.parser.is_json() && js_trim(&code).is_empty() {
                return bail("empty embedded json");
            }
            let doc = strip_trailing_hardline(crate::text_to_doc(&code, &o)?);
            let style = "`".repeat(3.max(max_continuous(&node.value, '`') + 1));
            return Ok(Some(mark_as_root(concat(vec![
                s(style.clone()),
                s(lang.clone()),
                s(node
                    .meta
                    .as_ref()
                    .map(|m| format!(" {m}"))
                    .unwrap_or_default()),
                hardline(),
                map_strings(&doc),
                hardline(),
                s(style),
            ]))));
        }
        if node.ty == "front-matter" && node.fm_language == "yaml" {
            let value = js_trim(&node.value);
            let doc = if value.is_empty() {
                None
            } else {
                let mut o = self.opts.clone();
                o.parser = Parser::Yaml;
                Some(strip_trailing_hardline(crate::text_to_doc(value, &o)?))
            };
            let has = doc.is_some();
            return Ok(Some(mark_as_root(concat(vec![
                s(node.start_delimiter.clone()),
                s(node.explicit_language.clone()),
                hardline(),
                doc.unwrap_or_else(empty),
                if has { hardline() } else { empty() },
                s(node.end_delimiter.clone()),
            ]))));
        }
        Ok(None)
    }

    fn fenced_value(&self, node: &Node) -> String {
        if node.end.offset as usize == self.text.len()
            && node.value.ends_with('\n')
            && self.text.last() == Some(&'\n')
        {
            let mut v = node.value.clone();
            v.pop();
            return v;
        }
        node.value.clone()
    }

    fn children(
        &self,
        path: &mut Path<'a>,
        f: &mut dyn FnMut(&Self, &mut Path<'a>) -> R<Option<Doc>>,
    ) -> R<Vec<Doc>> {
        let node = path.last().ok_or(()).or_else(|_| bail("path"))?.0;
        let mut parts: Vec<Doc> = Vec::new();
        for (i, c) in node.children.iter().enumerate() {
            path.push((c, i));
            let result = f(self, path);
            let pre = if !parts.is_empty() {
                Some(self.separator(path))
            } else {
                None
            };
            path.pop();
            if let Some(doc) = result? {
                if let Some(sep) = pre {
                    parts.extend(sep);
                }
                parts.push(doc);
            }
        }
        Ok(parts)
    }

    fn separator(&self, path: &Path<'a>) -> Vec<Doc> {
        let node = path[path.len() - 1].0;
        let parent = path[path.len() - 2].0;
        let inline = is_inline_type(node.ty) || (node.ty == "html" && is_inline_wrapper(parent.ty));
        if inline {
            return Vec::new();
        }
        let Some(prev) = Self::sibling(path, -1) else {
            return Vec::new();
        };
        let mut out = vec![hardline()];
        let triple = prev.ty == "list" && node.ty == "code" && node.is_indented;
        if self.double_hardline(node, prev, parent) || triple {
            out.push(hardline());
        }
        if triple {
            out.push(hardline());
        }
        out
    }

    fn is_loose(&self, n: &Node) -> bool {
        n.ty == "listItem" && (n.spread || self.char_at(n.end.offset - 1) == Some('\n'))
    }

    fn double_hardline(&self, node: &Node, prev: &Node, parent: &Node) -> bool {
        if self.is_loose(prev)
            || (node.ty == "list" && parent.ty == "listItem" && prev.ty == "code")
        {
            return true;
        }
        let sibling = prev.ty == node.ty && matches!(node.ty, "listItem" | "definition");
        let tight = parent.ty == "listItem" && (node.ty == "list" || !self.is_loose(parent));
        let prev_ignore = ignore_kind(prev) == Some("next");
        let html_pair =
            node.ty == "html" && prev.ty == "html" && prev.end.line + 1 == node.start.line;
        let html_after = node.ty == "html"
            && parent.ty == "listItem"
            && prev.ty == "paragraph"
            && prev.end.line + 1 == node.start.line;
        !(sibling || tight || prev_ignore || html_pair || html_after)
    }

    fn print_children(&self, path: &mut Path<'a>) -> R<Doc> {
        Ok(concat(
            self.children(path, &mut |p, path| p.print(path).map(Some))?,
        ))
    }

    fn remain_same(path: &Path<'a>) -> bool {
        path[..path.len() - 1]
            .iter()
            .rev()
            .find(|(n, _)| n.ty == "linkReference" || n.ty == "imageReference")
            .is_some_and(|(n, _)| n.ty != "linkReference" || n.reference_type != "full")
    }

    fn generic(&self, path: &mut Path<'a>) -> R {
        let node = path.last().ok_or(()).or_else(|_| bail("path"))?.0;
        let pw = self.opts.prose_wrap;
        if Self::remain_same(path) {
            let mut parts: Vec<Doc> = vec![s("")];
            for t in split_text(&self.slice(node))? {
                if t.ty == "word" {
                    let prev = parts.pop().unwrap_or_else(|| s(""));
                    parts.push(concat(vec![prev, s(t.value)]));
                    continue;
                }
                let d = self.whitespace_doc(path, None, &t.value, pw, true);
                if matches!(d, Doc::Str(_)) {
                    let prev = parts.pop().unwrap_or_else(|| s(""));
                    parts.push(concat(vec![prev, d]));
                    continue;
                }
                parts.push(d);
                parts.push(s(""));
            }
            return Ok(fill(parts));
        }
        Ok(match node.ty {
            "front-matter" => s(self.slice(node)),
            "root" => {
                if node.children.is_empty() {
                    return Ok(s(""));
                }
                concat(vec![self.print_root(path)?, hardline()])
            }
            "paragraph" => {
                let mut docs = Vec::new();
                for (i, c) in node.children.iter().enumerate() {
                    path.push((c, i));
                    let d = self.print(path);
                    path.pop();
                    docs.push(d?);
                }
                fill_parts_from(docs)
            }
            "sentence" => {
                let mut parts: Vec<Doc> = vec![s("")];
                for (i, c) in node.children.iter().enumerate() {
                    path.push((c, i));
                    let d = self.print(path);
                    path.pop();
                    let d = d?;
                    if c.ty == "whitespace" && !matches!(d, Doc::Str(_)) {
                        parts.push(d);
                        parts.push(s(""));
                    } else {
                        let prev = parts.pop().unwrap_or_else(|| s(""));
                        parts.push(concat(vec![prev, d]));
                    }
                }
                fill(parts)
            }
            "word" => s(self.print_word(path, node)?),
            "whitespace" => {
                let next = Self::sibling(path, 1);
                let never = next.is_some_and(|n| {
                    let v = n.value.as_str();
                    v.starts_with('>')
                        || matches!(v, "*" | "+" | "-")
                        || (v.starts_with('#') && v.len() <= 6 && v.chars().all(|c| c == '#'))
                        || {
                            let d = v.trim_end_matches(['.', ')']);
                            !d.is_empty()
                                && d.len() + 1 == v.len()
                                && d.chars().all(|c| c.is_ascii_digit())
                        }
                });
                let pw = if never { ProseWrap::Never } else { pw };
                self.whitespace_doc(path, Some(node), &node.value, pw, false)
            }
            "emphasis" => {
                let style = if is_autolink(node.children.first()) {
                    self.char_at(node.start.offset).unwrap_or('_').to_string()
                } else {
                    let prev = Self::sibling(path, -1);
                    let next = Self::sibling(path, 1);
                    let word_near = prev.is_some_and(|p| {
                        p.ty == "sentence"
                            && p.children
                                .last()
                                .is_some_and(|w| w.ty == "word" && !w.trailing_punct)
                    }) || next.is_some_and(|n| {
                        n.ty == "sentence"
                            && n.children
                                .first()
                                .is_some_and(|w| w.ty == "word" && !w.leading_punct)
                    });
                    if word_near || Self::has_ancestor(path, |n| n.ty == "emphasis") {
                        "*".to_string()
                    } else {
                        "_".to_string()
                    }
                };
                concat(vec![s(style.clone()), self.print_children(path)?, s(style)])
            }
            "strong" => concat(vec![s("**"), self.print_children(path)?, s("**")]),
            "delete" => concat(vec![s("~~"), self.print_children(path)?, s("~~")]),
            "inlineCode" => {
                let code = if pw == ProseWrap::Preserve {
                    node.value.clone()
                } else {
                    node.value.replace('\n', " ")
                };
                let ticks = "`".repeat(min_not_present(&code, '`').max(1));
                let pad = if code.starts_with('`')
                    || code.ends_with('`')
                    || (code.starts_with(['\n', ' '])
                        && code.ends_with(['\n', ' '])
                        && code.chars().any(|c| c != '\n' && c != ' '))
                {
                    " "
                } else {
                    ""
                };
                concat(vec![s(ticks.clone()), s(pad), s(code), s(pad), s(ticks)])
            }
            "wikiLink" => {
                let contents = if pw == ProseWrap::Preserve {
                    node.value.clone()
                } else {
                    let mut out = String::new();
                    let mut in_run = false;
                    for c in node.value.chars() {
                        if c == '\t' || c == '\n' {
                            if !in_run {
                                out.push(' ');
                            }
                            in_run = true;
                        } else {
                            out.push(c);
                            in_run = false;
                        }
                    }
                    out
                };
                concat(vec![s("[["), s(contents), s("]]")])
            }
            "link" => match self.char_at(node.start.offset) {
                Some('<') => {
                    let mailto = "mailto:";
                    let after: String =
                        chars_slice(self.text, node.start.offset + 1, node.start.offset + 1 + 7);
                    let url = if node.url.starts_with(mailto) && after != mailto {
                        node.url[mailto.len()..].to_string()
                    } else {
                        node.url.clone()
                    };
                    concat(vec![s("<"), s(url), s(">")])
                }
                Some('[') => concat(vec![
                    s("["),
                    self.print_children(path)?,
                    s("]("),
                    s(print_url(&node.url, &[')'])),
                    s(print_title(
                        node.title.as_deref(),
                        self.opts.single_quote,
                        true,
                    )),
                    s(")"),
                ]),
                _ => s(self.slice(node)),
            },
            "image" => concat(vec![
                s("!["),
                s(node.alt.clone().unwrap_or_default()),
                s("]("),
                s(print_url(&node.url, &[')'])),
                s(print_title(
                    node.title.as_deref(),
                    self.opts.single_quote,
                    true,
                )),
                s(")"),
            ]),
            "blockquote" => concat(vec![
                s("> "),
                Doc::Align(
                    Align::Text("> ".to_string()),
                    Box::new(self.print_children(path)?),
                ),
            ]),
            "heading" => concat(vec![
                s(format!("{} ", "#".repeat(node.depth))),
                self.print_children(path)?,
            ]),
            "code" => {
                if node.is_indented {
                    let a = "    ".to_string();
                    return Ok(Doc::Align(
                        Align::Text(a.clone()),
                        Box::new(concat(vec![
                            s(a),
                            join(
                                &hardline(),
                                node.value.split('\n').map(|p| s(p.to_string())).collect(),
                            ),
                        ])),
                    ));
                }
                let style = "`".repeat(3.max(max_continuous(&node.value, '`') + 1));
                let value = self.fenced_value(node);
                concat(vec![
                    s(style.clone()),
                    s(node.lang.clone().unwrap_or_default()),
                    s(node
                        .meta
                        .as_ref()
                        .map(|m| format!(" {m}"))
                        .unwrap_or_default()),
                    hardline(),
                    join(
                        &hardline(),
                        value.split('\n').map(|p| s(p.to_string())).collect(),
                    ),
                    hardline(),
                    s(style),
                ])
            }
            "html" => {
                let parent = Self::parent(path);
                let is_last = parent
                    .is_some_and(|p| path.last().is_some_and(|l| l.1 + 1 == p.children.len()));
                let value = if parent.is_some_and(|p| p.ty == "root") && is_last {
                    node.value.trim_end_matches(is_js_whitespace).to_string()
                } else {
                    node.value.clone()
                };
                let is_comment =
                    value.starts_with("<!--") && value.ends_with("-->") && value.len() >= 7;
                let sep = if is_comment {
                    hardline()
                } else {
                    mark_as_root(literalline())
                };
                join(&sep, value.split('\n').map(|p| s(p.to_string())).collect())
            }
            "list" => self.print_list(path)?,
            "thematicBreak" => {
                let anc: Vec<&Node> = path[..path.len() - 1]
                    .iter()
                    .rev()
                    .map(|(n, _)| *n)
                    .collect();
                match anc.iter().position(|n| n.ty == "list") {
                    None => s("---"),
                    Some(c) => {
                        let nth = anc.get(c + 1).map_or(0, |p| nth_sibling(anc[c], p));
                        s(if nth % 2 == 0 { "***" } else { "---" })
                    }
                }
            }
            "linkReference" => concat(vec![
                s("["),
                self.print_children(path)?,
                s("]"),
                s(match node.reference_type {
                    "full" => format!("[{}]", remark::collapse_white_space(&node.label)),
                    "collapsed" => "[]".to_string(),
                    _ => String::new(),
                }),
            ]),
            "imageReference" => match node.reference_type {
                "full" => concat(vec![
                    s("!["),
                    s(node.alt.clone().unwrap_or_default()),
                    s("]"),
                    s(format!("[{}]", remark::collapse_white_space(&node.label))),
                ]),
                _ => concat(vec![
                    s("!["),
                    s(match &node.alt {
                        Some(a) => a.clone(),
                        None => return bail("image reference without alt"),
                    }),
                    s("]"),
                    s(if node.reference_type == "collapsed" {
                        "[]"
                    } else {
                        ""
                    }),
                ]),
            },
            "definition" => {
                let sep = if pw == ProseWrap::Always {
                    line()
                } else {
                    s(" ")
                };
                let mut inner = vec![sep.clone(), s(print_url(&node.url, &[]))];
                if let Some(t) = &node.title {
                    inner.push(concat(vec![
                        sep,
                        s(print_title(Some(t), self.opts.single_quote, false)),
                    ]));
                }
                group(concat(vec![
                    s(format!("[{}]", remark::collapse_white_space(&node.label))),
                    s(":"),
                    indent(concat(inner)),
                ]))
            }
            "footnoteReference" => s(format!("[^{}]", node.label)),
            "footnoteDefinition" => {
                let single = node.children.len() == 1
                    && node.children[0].ty == "paragraph"
                    && (pw == ProseWrap::Never
                        || (pw == ProseWrap::Preserve
                            && node.children[0].start.line == node.children[0].end.line));
                let body = if single {
                    self.print_children(path)?
                } else {
                    let parts = self.children(path, &mut |p, path| {
                        let d = p.print(path)?;
                        Ok(Some(if path.last().is_some_and(|l| l.1 == 0) {
                            group(concat(vec![softline(), d]))
                        } else {
                            d
                        }))
                    })?;
                    group(Doc::Align(
                        Align::Text("    ".to_string()),
                        Box::new(concat(parts)),
                    ))
                };
                concat(vec![s(format!("[^{}]", node.label)), s(": "), body])
            }
            "table" => self.print_table(path)?,
            "tableCell" => self.print_children(path)?,
            "break" => {
                if self
                    .char_at(node.start.offset)
                    .is_some_and(is_js_whitespace)
                {
                    concat(vec![s("  "), mark_as_root(literalline())])
                } else {
                    concat(vec![s("\\"), hardline()])
                }
            }
            "liquidNode" => join(
                &hardline(),
                node.value.split('\n').map(|p| s(p.to_string())).collect(),
            ),
            "math" => concat(vec![
                s("$$"),
                hardline(),
                if node.value.is_empty() {
                    empty()
                } else {
                    concat(vec![
                        join(
                            &hardline(),
                            node.value.split('\n').map(|p| s(p.to_string())).collect(),
                        ),
                        hardline(),
                    ])
                },
                s("$$"),
            ]),
            "inlineMath" => s(self.slice(node)),
            other => return bail(format!("markdown node {other}")),
        })
    }

    fn print_word(&self, path: &Path<'a>, node: &Node) -> R<String> {
        let Some(r) = res() else {
            return bail("regex");
        };
        let star = node.value.replace('*', "\\*");
        let escaped = r
            .word_underscore
            .replace_all(&star, |c: &regex::Captures| {
                let t = match c.get(2) {
                    Some(u1) => format!("{}{}", c.get(1).map_or("", |m| m.as_str()), u1.as_str()),
                    None => format!(
                        "{}{}",
                        c.get(3).map_or("", |m| m.as_str()),
                        c.get(4).map_or("", |m| m.as_str())
                    ),
                };
                t.replace('_', "\\_")
            })
            .into_owned();
        if escaped != node.value {
            let n = path.len();
            let first_in_sentence =
                |i: usize| i >= 2 && path[i - 1].0.ty == "sentence" && path[i].1 == 0;
            let a = first_in_sentence(n - 1)
                && n >= 3
                && is_autolink(
                    (path[n - 2].1)
                        .checked_sub(1)
                        .and_then(|j| path[n - 3].0.children.get(j)),
                );
            let b = first_in_sentence(n - 1)
                && n >= 4
                && path[n - 3].0.ty == "emphasis"
                && path[n - 2].1 == 0
                && is_autolink(
                    (path[n - 3].1)
                        .checked_sub(1)
                        .and_then(|j| path[n - 4].0.children.get(j)),
                );
            if a || b {
                let mut out = escaped.clone();
                let mut prefix_end = 0;
                let bytes = escaped.as_bytes();
                loop {
                    let mut j = prefix_end;
                    if bytes.get(j) == Some(&b'\\') {
                        j += 1;
                    }
                    if matches!(bytes.get(j), Some(b'*' | b'_')) {
                        prefix_end = j + 1;
                    } else {
                        break;
                    }
                }
                if prefix_end > 0 {
                    out = format!(
                        "{}{}",
                        escaped[..prefix_end].replace('\\', ""),
                        &escaped[prefix_end..]
                    );
                }
                return Ok(out);
            }
        }
        Ok(escaped)
    }

    fn whitespace_doc(
        &self,
        path: &Path<'a>,
        _node: Option<&Node>,
        value: &str,
        pw: ProseWrap,
        is_link: bool,
    ) -> Doc {
        if pw == ProseWrap::Preserve && value == "\n" {
            return hardline();
        }
        let prev = Self::sibling(path, -1);
        let next = Self::sibling(path, 1);
        let can_be_space =
            value == " " || (value == "\n" && self.line_break_to_space(path, prev, next, is_link));
        let breakable = self.is_breakable(path, prev, next, value, pw, is_link);
        match (breakable, can_be_space) {
            (true, true) => line(),
            (true, false) => softline(),
            (false, true) => s(" "),
            (false, false) => s(""),
        }
    }

    fn line_break_to_space(
        &self,
        path: &Path<'a>,
        prev: Option<&Node>,
        next: Option<&Node>,
        is_link: bool,
    ) -> bool {
        if is_link {
            return true;
        }
        let (Some(prev), Some(next)) = (prev, next) else {
            return true;
        };
        let (pk, nk) = (prev.kind, next.kind);
        let non_or_k = |k: u8| k == NON_CJK || k == K_LETTER;
        if (non_or_k(pk) && non_or_k(nk))
            || (pk == K_LETTER && nk == CJ_LETTER)
            || (nk == K_LETTER && pk == CJ_LETTER)
        {
            return true;
        }
        if pk == CJK_PUNCT || nk == CJK_PUNCT || (pk == CJ_LETTER && nk == CJ_LETTER) {
            return false;
        }
        let ascii_punct =
            |c: Option<char>| c.is_some_and(|c| "!\"#$%&'()*+,-./:;<=>?@[\\]^_`{|}~".contains(c));
        if ascii_punct(next.value.chars().next()) || ascii_punct(prev.value.chars().last()) {
            return true;
        }
        if prev.trailing_punct || next.leading_punct {
            return false;
        }
        let Some(sentence) = Self::parent(path) else {
            return false;
        };
        let (mut spaced, mut tight) = (0, 0);
        let ch = &sentence.children;
        for i in 1..ch.len().saturating_sub(1) {
            let n = &ch[i];
            if n.ty == "whitespace" && (n.value == " " || n.value.is_empty()) {
                let (a, b) = (ch[i - 1].kind, ch[i + 1].kind);
                if (a == CJ_LETTER && b == NON_CJK) || (a == NON_CJK && b == CJ_LETTER) {
                    if n.value == " " {
                        spaced += 1;
                    } else {
                        tight += 1;
                    }
                }
            }
        }
        spaced > tight
    }

    fn is_breakable(
        &self,
        path: &Path<'a>,
        prev: Option<&Node>,
        next: Option<&Node>,
        value: &str,
        pw: ProseWrap,
        is_link: bool,
    ) -> bool {
        if pw != ProseWrap::Always
            || Self::has_ancestor(path, |n| {
                matches!(n.ty, "heading" | "tableCell" | "link" | "wikiLink")
            })
        {
            return false;
        }
        if is_link {
            return !value.is_empty();
        }
        let (Some(prev), Some(next)) = (prev, next) else {
            return true;
        };
        if value.is_empty() {
            return false;
        }
        if (prev.kind == K_LETTER && next.kind == CJ_LETTER)
            || (next.kind == K_LETTER && prev.kind == CJ_LETTER)
        {
            return true;
        }
        !(is_cj(prev) || is_cj(next))
    }

    fn print_root(&self, path: &mut Path<'a>) -> R {
        let node = path.last().ok_or(()).or_else(|_| bail("path"))?.0;
        let mut ranges: Vec<(usize, i64, usize, i64)> = Vec::new();
        let mut start: Option<(usize, i64)> = None;
        for (i, c) in node.children.iter().enumerate() {
            match ignore_kind(c) {
                Some("start") => {
                    if start.is_none() {
                        start = Some((i, c.end.offset));
                    }
                }
                Some("end") => {
                    if let Some((si, so)) = start.take() {
                        ranges.push((si, so, i, c.start.offset));
                    }
                }
                _ => {}
            }
        }
        let mut ranges = std::collections::VecDeque::from(ranges);
        let text = self.text;
        let parts = self.children(path, &mut |p, path| {
            let index = path.last().map_or(0, |l| l.1);
            if let Some(&(si, so, ei, eo)) = ranges.front() {
                if index == si {
                    let parent = Pr::parent(path).ok_or(()).or_else(|_| bail("root"))?;
                    return Ok(Some(concat(vec![
                        s(parent.children[si].value.clone()),
                        s(chars_slice(text, so, eo)),
                        s(parent.children[ei].value.clone()),
                    ])));
                }
                if si < index && index < ei {
                    return Ok(None);
                }
                if index == ei {
                    ranges.pop_front();
                    return Ok(None);
                }
            }
            p.print(path).map(Some)
        })?;
        Ok(concat(parts))
    }

    fn print_list(&self, path: &mut Path<'a>) -> R {
        let node = path.last().ok_or(()).or_else(|_| bail("path"))?.0;
        let parent = Self::parent(path)
            .ok_or(())
            .or_else(|_| bail("list parent"))?;
        let nth = nth_sibling(node, parent);
        let git_friendly = self.git_diff_friendly(node);
        let tab_width = self.opts.tab_width;
        let parts = self.children(path, &mut |p, path| {
            let (child, index) = *path.last().ok_or(()).or_else(|_| bail("path"))?;
            let raw = if node.ordered {
                let start = node.start_num.unwrap_or(1.0);
                let number = if index == 0 {
                    start
                } else if git_friendly {
                    1.0
                } else {
                    start + index as f64
                };
                format!(
                    "{}{}",
                    doc_printer_rs::js::number_to_string(number),
                    if nth % 2 == 0 { ". " } else { ") " }
                )
            } else if nth % 2 == 0 {
                "- ".to_string()
            } else {
                "* ".to_string()
            };
            let prefix = if (node.is_aligned || node.has_indented_codeblock) && node.ordered {
                let rest = if tab_width == 0 {
                    0
                } else {
                    raw.len() % tab_width
                };
                let add = if rest == 0 { 0 } else { tab_width - rest };
                format!("{raw}{}", " ".repeat(if add >= 4 { 0 } else { add }))
            } else {
                raw
            };
            let item = p.print_list_item(path, &prefix)?;
            let no_align = child.children.len() == 2
                && child.children[1].ty == "html"
                && child.children[0].start.column != child.children[1].start.column;
            Ok(Some(if no_align {
                concat(vec![s(prefix), item])
            } else {
                let w = prefix.chars().count();
                concat(vec![
                    s(prefix),
                    Doc::Align(Align::Text(" ".repeat(w)), Box::new(item)),
                ])
            }))
        })?;
        Ok(concat(parts))
    }

    fn print_list_item(&self, path: &mut Path<'a>, list_prefix: &str) -> R {
        let node = path.last().ok_or(()).or_else(|_| bail("path"))?.0;
        let prefix = match node.checked {
            None => "",
            Some(true) => "[x] ",
            Some(false) => "[ ] ",
        };
        let tab_width = self.opts.tab_width as i64;
        let lp = list_prefix.chars().count() as i64;
        let parts = self.children(path, &mut |p, path| {
            let (child, index) = *path.last().ok_or(()).or_else(|_| bail("path"))?;
            let printed = p.print(path)?;
            if index == 0 && child.ty != "list" {
                return Ok(Some(Doc::Align(
                    Align::Text(" ".repeat(prefix.len())),
                    Box::new(printed),
                )));
            }
            let a = " ".repeat((tab_width - lp).clamp(0, 3) as usize);
            Ok(Some(concat(vec![
                s(a.clone()),
                Doc::Align(Align::Text(a), Box::new(printed)),
            ])))
        })?;
        Ok(concat(vec![s(prefix), concat(parts)]))
    }

    fn git_diff_friendly(&self, list: &Node) -> bool {
        if !list.ordered || list.children.len() < 2 {
            return false;
        }
        let pre = Pre {
            text: self.text,
            tab_width: self.opts.tab_width,
        };
        let second = pre.ordered_info(&list.children[1]).0;
        if second != 1.0 {
            return false;
        }
        let first = pre.ordered_info(&list.children[0]).0;
        if first != 0.0 {
            return true;
        }
        list.children.len() > 2 && pre.ordered_info(&list.children[2]).0 == 1.0
    }

    fn print_table(&self, path: &mut Path<'a>) -> R {
        let node = path.last().ok_or(()).or_else(|_| bail("path"))?.0;
        let print_opts = doc_printer_rs::port::PrintOptions {
            print_width: self.opts.print_width as f64,
            tab_width: self.opts.tab_width as f64,
            use_tabs: self.opts.use_tabs,
            end_of_line: "\n",
        };
        let mut widths: Vec<usize> = Vec::new();
        let mut contents: Vec<Vec<(String, usize)>> = Vec::new();
        for (ri, row) in node.children.iter().enumerate() {
            path.push((row, ri));
            let mut cells = Vec::new();
            for (ci, cell) in row.children.iter().enumerate() {
                path.push((cell, ci));
                let d = self.print(path);
                path.pop();
                let d = d?;
                if doc_printer_rs::port::propagate_breaks(&d).is_err() {
                    path.pop();
                    return bail("table doc");
                }
                let text = match doc_printer_rs::port::print(&d, &print_opts) {
                    Ok(p) => p.formatted,
                    Err(_) => {
                        path.pop();
                        return bail("table doc");
                    }
                };
                let w = doc_printer_rs::port::string_width(&text) as usize;
                if widths.len() <= ci {
                    widths.resize(ci + 1, 3);
                }
                widths[ci] = widths[ci].max(w);
                cells.push((text, w));
            }
            path.pop();
            contents.push(cells);
        }
        let row_text = |cols: &Vec<(String, usize)>, compact: bool| -> String {
            let parts: Vec<String> = cols
                .iter()
                .enumerate()
                .map(|(ci, (text, w))| {
                    if compact {
                        return text.clone();
                    }
                    let spaces = widths.get(ci).copied().unwrap_or(3).saturating_sub(*w);
                    let align = node.align.get(ci).copied().flatten();
                    let before = match align {
                        Some("right") => spaces,
                        Some("center") => spaces / 2,
                        _ => 0,
                    };
                    format!(
                        "{}{text}{}",
                        " ".repeat(before),
                        " ".repeat(spaces - before)
                    )
                })
                .collect();
            format!("| {} |", parts.join(" | "))
        };
        let align_text = |compact: bool| -> String {
            let parts: Vec<String> = widths
                .iter()
                .enumerate()
                .map(|(i, &w)| {
                    let a = node.align.get(i).copied().flatten();
                    let first = if matches!(a, Some("center" | "left")) {
                        ":"
                    } else {
                        "-"
                    };
                    let last = if matches!(a, Some("center" | "right")) {
                        ":"
                    } else {
                        "-"
                    };
                    let middle = if compact {
                        "-".to_string()
                    } else {
                        "-".repeat(w.saturating_sub(2))
                    };
                    format!("{first}{middle}{last}")
                })
                .collect();
            format!("| {} |", parts.join(" | "))
        };
        let table = |compact: bool| -> Doc {
            let mut rows = Vec::new();
            if let Some(first) = contents.first() {
                rows.push(s(row_text(first, compact)));
            }
            rows.push(s(align_text(compact)));
            for r in contents.iter().skip(1) {
                rows.push(s(row_text(r, compact)));
            }
            join(&hardline_without_break_parent(), rows)
        };
        let aligned = table(false);
        if self.opts.prose_wrap != ProseWrap::Never {
            return Ok(concat(vec![break_parent(), aligned]));
        }
        let compact = table(true);
        Ok(concat(vec![
            break_parent(),
            group(if_break(compact, aligned)),
        ]))
    }
}

fn js_trim(s: &str) -> &str {
    s.trim_matches(is_js_whitespace)
}

fn nth_sibling(node: &Node, parent: &Node) -> i64 {
    let mut index: i64 = -1;
    for c in &parent.children {
        if c.ty == node.ty && c.ordered == node.ordered {
            index += 1;
        } else {
            index = -1;
        }
        if std::ptr::eq(c, node) {
            return index;
        }
    }
    index
}

/// Prettier's `markdown` parser and printer: the doc for `text`.
pub fn print(text: &str, opts: &Options) -> R {
    let lower = opts.filepath.to_ascii_lowercase();
    if lower.ends_with(".mdx") {
        return bail("mdx");
    }
    let Some(mut root) = remark::parse(text) else {
        return bail("markdown parse");
    };
    let chars: Vec<char> = text.chars().collect();
    let pre = Pre {
        text: &chars,
        tab_width: opts.tab_width,
    };
    pre.restore_unescaped(&mut root);
    Pre::merge_texts(&mut root);
    pre.mark_indented_code(&mut root);
    pre.mark_aligned(&mut root, false);
    Pre::sentences(&mut root)?;
    let pr = Pr { text: &chars, opts };
    let mut path: Path = vec![(&root, 0)];
    pr.print(&mut path)
}
