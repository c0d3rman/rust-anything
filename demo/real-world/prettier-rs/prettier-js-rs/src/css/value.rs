//! postcss-values-parser 2.0.1 (`lib/tokenize.js`, `lib/parser.js`; MIT, Copyright (c) Andrew
//! Powell) in loose mode, and Prettier's `parse-value.js` grouping and `loc.js` value offsets.
//! The tokenizer counts bytes; `build` turns positions into UTF-16 offsets as Prettier has them.
//! Positions that postcss-values-parser computes from a stale variable are kept so.

use super::u16::{Idx, len16};

#[derive(Clone, Debug)]
struct Tok {
    ty: &'static str,
    text: String,
    line: i64,
    col: i64,
    end_line: i64,
    end_col: Option<i64>,
    pos: i64,
}

fn c(s: &[u8], i: usize) -> u8 {
    s.get(i).copied().unwrap_or(0)
}

fn word_end(b: u8, next: u8) -> bool {
    matches!(
        b,
        b' ' | b'\n'
            | b'\t'
            | b'\r'
            | b'('
            | b')'
            | b'{'
            | b'}'
            | b'*'
            | b':'
            | b';'
            | b'@'
            | b'!'
            | b'&'
            | b'\''
            | b'"'
            | b'+'
            | b'|'
            | b'~'
            | b'>'
            | b','
            | b'['
            | b']'
            | b'\\'
    ) || (b == b'/' && next == b'*')
}

fn word_end_num(b: u8) -> bool {
    matches!(
        b,
        b' ' | b'\n'
            | b'\t'
            | b'\r'
            | b'('
            | b')'
            | b'{'
            | b'}'
            | b'*'
            | b':'
            | b';'
            | b'@'
            | b'!'
            | b'&'
            | b'\''
            | b'"'
            | b'-'
            | b'+'
            | b'|'
            | b'~'
            | b'>'
            | b','
            | b'['
            | b']'
            | b'\\'
            | b'/'
    )
}

fn tokenize(css: &str) -> Option<Vec<Tok>> {
    let s = css.as_bytes();
    let len = s.len();
    let mut tokens: Vec<Tok> = Vec::new();
    let mut offset: i64 = -1;
    let mut line: i64 = 1;
    let mut pos = 0usize;
    let mut paren = 0i64;
    let mut is_url_arg = false;
    let mut next: Option<i64> = None;
    // postcss-values-parser's columns are differences of UTF-16 indices.
    let idx = Idx::new(css);
    let pos16 = |x: i64| if x < 0 { x } else { idx.pos(x as usize) };
    let col = |x: i64, from: i64| pos16(x) - pos16(from);
    let slice = |a: usize, b: usize| css.get(a.min(len)..b.min(len)).unwrap_or("").to_string();
    while pos < len {
        let mut code = s[pos];
        if code == b'\n' {
            offset = pos as i64;
            line += 1;
        }
        let p = pos as i64;
        match code {
            b'\n' | b' ' | b'\t' | b'\r' | 0x0c => {
                let mut n = pos;
                loop {
                    n += 1;
                    code = c(s, n);
                    if code == b'\n' {
                        offset = n as i64;
                        line += 1;
                    }
                    if !matches!(code, b' ' | b'\n' | b'\t' | b'\r' | 0x0c) || n >= len {
                        break;
                    }
                }
                next = Some(n as i64);
                tokens.push(Tok {
                    ty: "space",
                    text: slice(pos, n),
                    line,
                    col: col(p, offset),
                    end_line: line,
                    end_col: Some(col(n as i64, offset)),
                    pos: p,
                });
                pos = n - 1;
            }
            b':' | b',' => {
                let n = pos + 1;
                next = Some(n as i64);
                tokens.push(Tok {
                    ty: if code == b':' { "colon" } else { "comma" },
                    text: slice(pos, n),
                    line,
                    col: col(p, offset),
                    end_line: line,
                    end_col: Some(col(n as i64, offset)),
                    pos: p,
                });
            }
            b'{' | b'}' | b'(' | b')' => {
                if code == b'(' {
                    paren += 1;
                    is_url_arg = !is_url_arg
                        && paren == 1
                        && tokens
                            .last()
                            .is_some_and(|t| t.ty == "word" && t.text == "url");
                } else if code == b')' {
                    paren -= 1;
                    is_url_arg = is_url_arg && paren > 0;
                }
                let ty = match code {
                    b'{' => "{",
                    b'}' => "}",
                    b'(' => "(",
                    _ => ")",
                };
                tokens.push(Tok {
                    ty,
                    text: (code as char).to_string(),
                    line,
                    col: col(p, offset),
                    end_line: line,
                    end_col: next.map(|n| col(n, offset)),
                    pos: p,
                });
            }
            b'\'' | b'"' => {
                let mut n = pos;
                loop {
                    let mut escaped = false;
                    n = pos_of(s, code, n + 1)?;
                    let mut ep = n;
                    while ep > 0 && s[ep - 1] == b'\\' {
                        ep -= 1;
                        escaped = !escaped;
                    }
                    if !escaped {
                        break;
                    }
                }
                next = Some(n as i64);
                tokens.push(Tok {
                    ty: "string",
                    text: slice(pos, n + 1),
                    line,
                    col: col(p, offset),
                    end_line: line,
                    end_col: Some(col(n as i64, offset)),
                    pos: p,
                });
                pos = n;
            }
            b'@' => {
                let mut i = pos + 1;
                while i < len
                    && !matches!(
                        s[i],
                        b' ' | b'\n'
                            | b'\t'
                            | b'\r'
                            | b'{'
                            | b'('
                            | b')'
                            | b'\''
                            | b'"'
                            | b'\\'
                            | b';'
                            | b','
                            | b'/'
                    )
                {
                    i += 1;
                }
                let n = if i >= len { len - 1 } else { i - 1 };
                next = Some(n as i64);
                tokens.push(Tok {
                    ty: "atword",
                    text: slice(pos, n + 1),
                    line,
                    col: col(p, offset),
                    end_line: line,
                    end_col: Some(col(n as i64, offset)),
                    pos: p,
                });
                pos = n;
            }
            b'\\' => {
                next = Some(p);
                tokens.push(Tok {
                    ty: "word",
                    text: slice(pos, pos + 1),
                    line,
                    col: col(p, offset),
                    end_line: line,
                    end_col: Some(col(p, offset)),
                    pos: p,
                });
            }
            b'+' | b'-' | b'*' => {
                let mut n = pos + 1;
                if code == b'-' && c(s, pos + 1) == b'-' {
                    n += 1;
                    next = Some(n as i64);
                    tokens.push(Tok {
                        ty: "word",
                        text: slice(pos, n),
                        line,
                        col: col(p, offset),
                        end_line: line,
                        end_col: Some(col(n as i64, offset)),
                        pos: p,
                    });
                    pos = n - 1;
                } else {
                    next = Some(n as i64);
                    tokens.push(Tok {
                        ty: "operator",
                        text: slice(pos, n),
                        line,
                        col: col(p, offset),
                        end_line: line,
                        end_col: Some(col(n as i64, offset)),
                        pos: p,
                    });
                    n -= 1;
                    pos = n;
                }
            }
            _ => {
                if code == b'/' && (c(s, pos + 1) == b'*' || (!is_url_arg && c(s, pos + 1) == b'/'))
                {
                    let n = if c(s, pos + 1) == b'*' {
                        pos_seq(s, b"*/", pos + 2)? + 1
                    } else {
                        match pos_of(s, b'\n', pos + 2) {
                            Some(nl) => nl - 1,
                            None => len,
                        }
                    };
                    let content = slice(pos, n + 1);
                    let lines: Vec<&str> = content.split('\n').collect();
                    let last = lines.len() - 1;
                    let (next_line, next_offset) = if last > 0 {
                        (line + last as i64, n as i64 - lines[last].len() as i64)
                    } else {
                        (line, offset)
                    };
                    next = Some(n as i64);
                    tokens.push(Tok {
                        ty: "comment",
                        text: content,
                        line,
                        col: col(p, offset),
                        end_line: next_line,
                        end_col: Some(col(n as i64, next_offset)),
                        pos: p,
                    });
                    offset = next_offset;
                    line = next_line;
                    pos = n;
                } else if code == b'#' && !c(s, pos + 1).is_ascii_alphanumeric() {
                    let n = pos + 1;
                    next = Some(n as i64);
                    tokens.push(Tok {
                        ty: "#",
                        text: slice(pos, n),
                        line,
                        col: col(p, offset),
                        end_line: line,
                        end_col: Some(col(n as i64, offset)),
                        pos: p,
                    });
                } else if (code == b'u' || code == b'U') && c(s, pos + 1) == b'+' {
                    let mut n = pos + 2;
                    loop {
                        n += 1;
                        if !(n < len
                            && (c(s, n).is_ascii_hexdigit() || matches!(c(s, n), b'?' | b'-')))
                        {
                            break;
                        }
                    }
                    next = Some(n as i64);
                    tokens.push(Tok {
                        ty: "unicoderange",
                        text: slice(pos, n),
                        line,
                        col: col(p, offset),
                        end_line: line,
                        end_col: Some(col(n as i64, offset)),
                        pos: p,
                    });
                    pos = n - 1;
                } else if code == b'/' {
                    let n = pos + 1;
                    next = Some(n as i64);
                    tokens.push(Tok {
                        ty: "operator",
                        text: slice(pos, n),
                        line,
                        col: col(p, offset),
                        end_line: line,
                        end_col: Some(col(n as i64, offset)),
                        pos: p,
                    });
                } else {
                    let num = code.is_ascii_digit();
                    let mut i = pos + 1;
                    while i < len
                        && !(if num {
                            word_end_num(s[i])
                        } else {
                            word_end(s[i], c(s, i + 1))
                        })
                    {
                        i += 1;
                    }
                    let mut n = if i >= len { len - 1 } else { i - 1 };
                    if num || code == b'.' {
                        let (n0, n1, n2) = (c(s, n), c(s, n + 1), c(s, n + 2));
                        if (n0 == b'e' || n0 == b'E')
                            && (n1 == b'-' || n1 == b'+')
                            && n2.is_ascii_digit()
                        {
                            let mut j = n + 2 + 1;
                            while j < len && !word_end_num(s[j]) {
                                j += 1;
                            }
                            n = if j >= len { len - 1 } else { j - 1 };
                        }
                    }
                    next = Some(n as i64);
                    tokens.push(Tok {
                        ty: "word",
                        text: slice(pos, n + 1),
                        line,
                        col: col(p, offset),
                        end_line: line,
                        end_col: Some(col(n as i64, offset)),
                        pos: p,
                    });
                    pos = n;
                }
            }
        }
        pos += 1;
    }
    Some(tokens)
}

fn pos_of(s: &[u8], b: u8, from: usize) -> Option<usize> {
    s.get(from..)?
        .iter()
        .position(|&x| x == b)
        .map(|i| i + from)
}

fn pos_seq(s: &[u8], pat: &[u8], from: usize) -> Option<usize> {
    s.get(from..)?
        .windows(pat.len())
        .position(|w| w == pat)
        .map(|i| i + from)
}

/// A node of postcss-values-parser's tree, after Prettier's `parse-value.js`.
#[derive(Clone, Debug, Default)]
pub struct V {
    pub ty: &'static str,
    pub value: String,
    pub before: String,
    pub after: String,
    pub quote: String,
    pub quoted: bool,
    pub unit: String,
    pub is_hex: bool,
    pub is_color: bool,
    pub inline: bool,
    pub has_source: bool,
    pub start: i64,
    pub end: Option<i64>,
    pub start_line: i64,
    pub group: Option<Box<V>>,
    pub groups: Vec<V>,
    pub open: Option<Box<V>>,
    pub close: Option<Box<V>>,
    sl: i64,
    el: i64,
    ec: Option<i64>,
    source_index: i64,
    nodes: Option<Vec<usize>>,
    unbalanced: i64,
}

struct P {
    arena: Vec<V>,
    parent: Vec<Option<usize>>,
    tokens: Vec<Tok>,
    position: usize,
    current: usize,
    cache: Vec<usize>,
    spaces: String,
}

impl P {
    fn tok(&self) -> Option<&Tok> {
        self.tokens.get(self.position)
    }

    fn next_tok(&self) -> Option<&Tok> {
        self.tokens.get(self.position + 1)
    }

    fn add(&mut self, mut v: V) -> usize {
        if !self.spaces.is_empty() {
            v.before.push_str(&std::mem::take(&mut self.spaces));
        }
        self.arena.push(v);
        let id = self.arena.len() - 1;
        self.parent.push(Some(self.current));
        if let Some(n) = self.arena[self.current].nodes.as_mut() {
            n.push(id);
        }
        id
    }

    fn last(&self, id: usize) -> Option<usize> {
        self.arena[id].nodes.as_ref()?.last().copied()
    }

    fn leaf(&self, ty: &'static str, t: &Tok) -> V {
        V {
            ty,
            value: t.text.clone(),
            has_source: true,
            sl: t.line,
            el: t.end_line,
            ec: t.end_col,
            source_index: t.pos,
            ..Default::default()
        }
    }

    fn parse(&mut self) -> Option<()> {
        while self.position < self.tokens.len() {
            let t = self.tok()?.clone();
            match t.ty {
                "space" => {
                    let at_end = self.position == self.tokens.len() - 1;
                    let next_ty = self.next_tok().map(|t| t.ty);
                    if at_end || next_ty == Some(",") || next_ty == Some(")") {
                        let last = self.last(self.current)?;
                        self.arena[last].after.push_str(&t.text);
                    } else {
                        self.spaces = t.text.clone();
                    }
                    self.position += 1;
                }
                "colon" | "comma" => {
                    let v = self.leaf(if t.ty == "colon" { "colon" } else { "comma" }, &t);
                    self.add(v);
                    self.position += 1;
                }
                "comment" => {
                    let mut v = self.leaf("comment", &t);
                    let mut value = t.text.replace("/*", "").replace("*/", "");
                    if value.starts_with("//") {
                        value = value[2..].to_string();
                        v.inline = true;
                    }
                    v.value = value;
                    self.add(v);
                    self.position += 1;
                }
                "(" => self.paren_open(&t)?,
                ")" => {
                    let v = self.leaf("paren", &t);
                    self.add(v);
                    self.position += 1;
                    if self.position >= self.tokens.len() - 1
                        && self.arena[self.current].unbalanced == 0
                    {
                        continue;
                    }
                    self.arena[self.current].unbalanced -= 1;
                    if self.arena[self.current].unbalanced < 0 {
                        return None;
                    }
                    if self.arena[self.current].unbalanced == 0
                        && let Some(c) = self.cache.pop()
                    {
                        self.current = c;
                    }
                }
                "operator" => {
                    let ch = t.text.as_str();
                    if (ch == "+" || ch == "-") && self.next_tok().is_some_and(|n| n.ty == "word") {
                        let cur = &self.arena[self.current];
                        let empty = cur.nodes.as_ref().is_none_or(Vec::is_empty);
                        let last_op = self
                            .last(self.current)
                            .is_some_and(|l| self.arena[l].ty == "operator");
                        if empty || last_op {
                            self.split_word()?;
                            continue;
                        }
                    }
                    let v = V {
                        ty: "operator",
                        value: t.text.clone(),
                        has_source: true,
                        sl: t.line,
                        el: t.line,
                        ec: Some(t.col),
                        source_index: t.end_line,
                        ..Default::default()
                    };
                    self.position += 1;
                    self.add(v);
                }
                "string" => {
                    let mut v = self.leaf("string", &t);
                    let q = t.text.chars().next();
                    if matches!(q, Some('"' | '\'')) {
                        v.quoted = true;
                        v.quote = q.unwrap_or('"').to_string();
                        v.value = t.text[1..t.text.len().saturating_sub(1).max(1)].to_string();
                    }
                    self.add(v);
                    self.position += 1;
                }
                "unicoderange" => {
                    let v = self.leaf("unicode-range", &t);
                    self.add(v);
                    self.position += 1;
                }
                _ => self.split_word()?,
            }
        }
        if !self.spaces.is_empty() {
            match self.last(self.current) {
                None => self.arena[self.current]
                    .before
                    .push_str(&self.spaces.clone()),
                Some(l) => {
                    let sp = self.spaces.clone();
                    self.arena[l].after.push_str(&sp);
                }
            }
        }
        Some(())
    }

    fn paren_open(&mut self, t: &Tok) -> Option<()> {
        let mut unbalanced = 1;
        let mut pos = self.position + 1;
        while pos < self.tokens.len() && unbalanced > 0 {
            match self.tokens[pos].ty {
                "(" => unbalanced += 1,
                ")" => unbalanced -= 1,
                _ => {}
            }
            pos += 1;
        }
        if unbalanced > 0 {
            return None;
        }
        if let Some(last) = self.last(self.current)
            && self.arena[last].ty == "func"
            && self.arena[last].unbalanced < 0
        {
            self.arena[last].unbalanced = 0;
            self.current = last;
        }
        self.arena[self.current].unbalanced += 1;
        let v = self.leaf("paren", t);
        self.add(v);
        self.position += 1;
        Some(())
    }

    fn split_word(&mut self) -> Option<()> {
        let mut next = self.next_tok().cloned();
        let first = self.tok()?.clone();
        let mut word = first.text.clone();
        let no_follow = {
            let b = word.as_bytes();
            matches!(b.first(), Some(b'#' | b'{' | b'}'))
                && !(b[0] == b'#' && b.get(1).is_some_and(|x| x.is_ascii_alphanumeric()))
        };
        if !no_follow {
            while let Some(n) = &next
                && n.ty == "word"
            {
                self.position += 1;
                word.push_str(&self.tok()?.text.clone());
                next = self.next_tok().cloned();
            }
        }
        let cur = self.tok()?.clone();
        let at: Vec<usize> = word.match_indices('@').map(|(i, _)| i).collect();
        let mut indices: Vec<usize> = vec![0];
        indices.extend(at.iter().copied());
        indices.sort_unstable();
        indices.dedup();
        let num_re = number_prefix(&cur.text).is_some();
        for (i, &ind) in indices.iter().enumerate() {
            let index = indices
                .get(i + 1)
                .copied()
                .filter(|&x| x != 0)
                .unwrap_or(word.len());
            let value = word.get(ind..index).unwrap_or("").to_string();
            let base = V {
                has_source: true,
                sl: cur.line,
                el: cur.end_line,
                ec: Some(cur.col + len16(word.get(..index).unwrap_or(&word)) - 1),
                source_index: cur.pos + ind as i64,
                ..Default::default()
            };
            let node = if at.contains(&ind) {
                let mut v = base;
                v.ty = "atword";
                v.value = value[1..].to_string();
                v.nodes = Some(Vec::new());
                v
            } else if num_re {
                let mut v = base;
                v.ty = "number";
                let unit = match number_prefix(&value) {
                    Some(n) => value[n..].to_string(),
                    None => value.clone(),
                };
                v.value = if unit.is_empty() {
                    value.clone()
                } else {
                    value.replacen(&unit, "", 1)
                };
                v.unit = unit;
                v
            } else {
                let mut v = base;
                v.value = value.clone();
                if next.as_ref().is_some_and(|n| n.ty == "(") {
                    v.ty = "func";
                    v.unbalanced = -1;
                    v.nodes = Some(Vec::new());
                } else {
                    v.ty = "word";
                    v.is_hex = value.len() > 1 && value.starts_with('#');
                    let h = &value.as_bytes()[1.min(value.len())..];
                    v.is_color = value.starts_with('#')
                        && matches!(h.len(), 3 | 4 | 6 | 8)
                        && h.iter().all(u8::is_ascii_hexdigit);
                }
                v
            };
            let is_func = node.ty == "func";
            if is_func {
                self.cache.push(self.current);
            }
            self.add(node);
        }
        self.position += 1;
        Some(())
    }
}

fn number_prefix(s: &str) -> Option<usize> {
    let b = s.as_bytes();
    let mut i = 0;
    if matches!(b.first(), Some(b'+' | b'-')) {
        i += 1;
    }
    let ds = i;
    while b.get(i).is_some_and(u8::is_ascii_digit) {
        i += 1;
    }
    if i > ds {
        if b.get(i) == Some(&b'.') {
            i += 1;
            while b.get(i).is_some_and(u8::is_ascii_digit) {
                i += 1;
            }
        }
    } else if b.get(i) == Some(&b'.') && b.get(i + 1).is_some_and(u8::is_ascii_digit) {
        i += 1;
        while b.get(i).is_some_and(u8::is_ascii_digit) {
            i += 1;
        }
    } else {
        return None;
    }
    if matches!(b.get(i), Some(b'e' | b'E')) {
        let mut j = i + 1;
        if matches!(b.get(j), Some(b'+' | b'-')) {
            j += 1;
        }
        let ds = j;
        while b.get(j).is_some_and(u8::is_ascii_digit) {
            j += 1;
        }
        if j > ds {
            i = j;
        }
    }
    Some(i)
}

/// `lineColumnToIndex`, in UTF-16 units.
fn line_col_to_index(line: i64, col: Option<i64>, text: &str, idx: &Idx) -> Option<i64> {
    let col = col?;
    let mut index: i64 = 0;
    for _ in 0..(line - 1).max(0) {
        index = match text.get(index.max(0) as usize..).and_then(|t| t.find('\n')) {
            Some(i) => index + i as i64 + 1,
            None => 0,
        };
    }
    Some(idx.b2u(index as usize) + col)
}

fn fix_word(v: &V, i: i64) -> i64 {
    let val = &v.value;
    if val == "-" || val == "--" || !val.starts_with('-') {
        return i;
    }
    i - if val.as_bytes().get(1) == Some(&b'-') {
        2
    } else {
        1
    }
}

fn build(p: &P, id: usize, text: &str, idx: &Idx, root_offset: i64, is_scss: bool) -> Option<V> {
    let mut v = p.arena[id].clone();
    if v.has_source {
        // The tokenizer counts bytes; Prettier's `sourceIndex` counts UTF-16 units.
        let u = |b: i64| if b < 0 { b } else { idx.b2u(b as usize) };
        let start = if v.ty == "word" {
            fix_word(&v, u(v.source_index))
        } else {
            u(v.source_index)
        };
        v.start = start + root_offset;
        v.end = line_col_to_index(v.el, v.ec, text, idx)
            .map(|e| (if v.ty == "word" { fix_word(&v, e) } else { e }) + root_offset);
        v.start_line = v.sl;
    }
    if is_scss && v.ty == "number" && v.unit == ".." && v.value.ends_with('.') {
        v.value.pop();
        v.unit = "...".to_string();
    }
    if let Some(nodes) = v.nodes.take() {
        let mut children: Vec<V> = Vec::new();
        for &c in &nodes {
            let mut child = build(p, c, text, idx, root_offset, is_scss)?;
            if child.ty == "func"
                && child.value == "selector"
                && let Some(g) = child.group.as_mut()
            {
                let (Some(open), Some(close)) = (&g.open, &g.close) else {
                    return None;
                };
                let a = (open.source_index + 1).max(0) as usize;
                let b = close.source_index.max(0) as usize;
                let raw = text.get(a.min(b)..b).unwrap_or("").to_string();
                g.groups = vec![V {
                    ty: "selector",
                    value: raw,
                    ..Default::default()
                }];
            }
            if child.ty == "func" && child.value == "url" {
                url_args(&mut child, text, is_scss);
            }
            children.push(child);
        }
        v.group = Some(Box::new(flatten(group_nodes(children)?)));
    }
    Some(v)
}

fn url_args(node: &mut V, text: &str, is_scss: bool) {
    let Some(group) = node.group.as_mut() else {
        return;
    };
    let mut list: Vec<&V> = Vec::new();
    for g in &group.groups {
        if g.ty == "comma_group" {
            list.extend(g.groups.iter());
        } else {
            list.push(g);
        }
    }
    let interpolation = (1..list.len()).rev().any(|i| {
        list[i].ty == "word"
            && list[i].value == "{"
            && list[i - 1].ty == "word"
            && list[i - 1].value.ends_with('#')
    });
    let string_or_func = list
        .iter()
        .any(|g| g.ty == "string" || (g.ty == "func" && !g.value.ends_with('\\')));
    let scss_var = is_scss
        && list
            .first()
            .is_some_and(|g| g.ty == "word" && g.value.starts_with('$'));
    if interpolation || (!string_or_func && !scss_var) {
        let (Some(open), Some(close)) = (&group.open, &group.close) else {
            return;
        };
        let a = (open.source_index + 1).max(0) as usize;
        let b = close.source_index.max(0) as usize;
        let raw = text
            .get(a.min(b)..b)
            .unwrap_or("")
            .trim_matches(super::postcss::js_ws)
            .to_string();
        group.groups = vec![V {
            ty: "raw",
            value: raw,
            ..Default::default()
        }];
    }
}

fn paren_group(open: Option<V>) -> V {
    V {
        ty: "paren_group",
        open: open.map(Box::new),
        ..Default::default()
    }
}

fn comma_group() -> V {
    V {
        ty: "comma_group",
        ..Default::default()
    }
}

fn group_nodes(nodes: Vec<V>) -> Option<V> {
    let mut paren_stack: Vec<V> = vec![paren_group(None)];
    let mut comma_stack: Vec<V> = vec![comma_group()];
    let n = nodes.len();
    let snapshot: Vec<(&'static str, String)> =
        nodes.iter().map(|x| (x.ty, x.value.clone())).collect();
    for (i, node) in nodes.into_iter().enumerate() {
        if node.ty == "paren" && node.value == "(" {
            paren_stack.push(paren_group(Some(node)));
            comma_stack.push(comma_group());
        } else if node.ty == "paren" && node.value == ")" {
            let cg = comma_stack.pop()?;
            let mut pg = paren_stack.pop()?;
            if !cg.groups.is_empty() {
                pg.groups.push(cg);
            }
            pg.close = Some(Box::new(node));
            if comma_stack.is_empty() {
                return None;
            }
            comma_stack.last_mut()?.groups.push(pg);
        } else if node.ty == "comma" {
            if i + 3 == n
                && snapshot[i + 1].0 == "comment"
                && snapshot[i + 2].0 == "paren"
                && snapshot[i + 2].1 == ")"
            {
                continue;
            }
            let cg = std::mem::replace(comma_stack.last_mut()?, comma_group());
            paren_stack.last_mut()?.groups.push(cg);
        } else {
            comma_stack.last_mut()?.groups.push(node);
        }
    }
    if paren_stack.len() != 1 || comma_stack.len() != 1 {
        return None;
    }
    let cg = comma_stack.pop()?;
    let mut root = paren_stack.pop()?;
    if !cg.groups.is_empty() {
        root.groups.push(cg);
    }
    Some(root)
}

fn flatten(node: V) -> V {
    if node.ty == "paren_group"
        && node.open.is_none()
        && node.close.is_none()
        && node.groups.len() == 1
    {
        let mut node = node;
        return flatten(node.groups.remove(0));
    }
    if node.ty == "comma_group" && node.groups.len() == 1 {
        let mut node = node;
        return flatten(node.groups.remove(0));
    }
    if node.ty == "paren_group" || node.ty == "comma_group" {
        let mut node = node;
        node.groups = std::mem::take(&mut node.groups)
            .into_iter()
            .map(flatten)
            .collect();
        return node;
    }
    node
}

/// Prettier's `parseValue`: the `value-root` of `value`, offsets shifted by `root_offset`.
/// `Err` where Prettier throws; `Ok(None)` where it falls back to `value-unknown`.
pub fn parse_value(value: &str, root_offset: i64, is_scss: bool) -> Result<Option<V>, ()> {
    let Some(tokens) = tokenize(value) else {
        return Ok(None);
    };
    let root = V {
        ty: "root",
        nodes: Some(vec![1]),
        ..Default::default()
    };
    let val = V {
        ty: "value",
        nodes: Some(Vec::new()),
        ..Default::default()
    };
    let mut p = P {
        arena: vec![root, val],
        parent: vec![None, Some(0)],
        tokens,
        position: 0,
        current: 1,
        cache: Vec::new(),
        spaces: String::new(),
    };
    if p.parse().is_none() {
        return Ok(None);
    }
    let _ = &p.parent;
    let idx = Idx::new(value);
    let mut root = build(&p, 0, value, &idx, root_offset, is_scss).ok_or(())?;
    root.value = value.to_string();
    Ok(Some(root))
}
