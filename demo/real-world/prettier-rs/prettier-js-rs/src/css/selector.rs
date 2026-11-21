//! postcss-selector-parser 2.2.3 (`dist/tokenize.js`, `dist/parser.js`; MIT, Copyright (c) Ben
//! Briggs) in its default lossless mode: the tree Prettier prints for selectors.

#[derive(Clone, Debug)]
struct Tok {
    ty: &'static str,
    text: String,
}

fn c(s: &[u8], i: usize) -> u8 {
    s.get(i).copied().unwrap_or(0)
}

fn tokenize(css: &str) -> Option<Vec<Tok>> {
    let s = css.as_bytes();
    let len = s.len();
    let mut out = Vec::new();
    let mut pos = 0;
    let sl = |a: usize, b: usize| css.get(a..b.min(len)).unwrap_or("").to_string();
    while pos < len {
        let code = s[pos];
        match code {
            b'\n' | b' ' | b'\t' | b'\r' | 0x0c => {
                let mut n = pos;
                loop {
                    n += 1;
                    if !matches!(c(s, n), b' ' | b'\n' | b'\t' | b'\r' | 0x0c) || n >= len {
                        break;
                    }
                }
                out.push(Tok {
                    ty: "space",
                    text: sl(pos, n),
                });
                pos = n - 1;
            }
            b'+' | b'>' | b'~' | b'|' => {
                let mut n = pos;
                loop {
                    n += 1;
                    if !matches!(c(s, n), b'+' | b'>' | b'~' | b'|') || n >= len {
                        break;
                    }
                }
                out.push(Tok {
                    ty: "combinator",
                    text: sl(pos, n),
                });
                pos = n - 1;
            }
            b'*' | b'&' | b',' | b'[' | b']' | b':' | b';' | b'(' | b')' => {
                let ty = match code {
                    b'*' => "*",
                    b'&' => "&",
                    b',' => ",",
                    b'[' => "[",
                    b']' => "]",
                    b':' => ":",
                    b';' => ";",
                    b'(' => "(",
                    _ => ")",
                };
                out.push(Tok {
                    ty,
                    text: (code as char).to_string(),
                });
            }
            b'\'' | b'"' => {
                let mut n = pos;
                loop {
                    let mut escaped = false;
                    n = s.get(n + 1..)?.iter().position(|&x| x == code)? + n + 1;
                    let mut ep = n;
                    while ep > 0 && s[ep - 1] == b'\\' {
                        ep -= 1;
                        escaped = !escaped;
                    }
                    if !escaped {
                        break;
                    }
                }
                out.push(Tok {
                    ty: "string",
                    text: sl(pos, n + 1),
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
                            | b'/'
                    )
                {
                    i += 1;
                }
                let n = if i >= len { len - 1 } else { i - 1 };
                out.push(Tok {
                    ty: "at-word",
                    text: sl(pos, n + 1),
                });
                pos = n;
            }
            b'\\' => {
                let mut n = pos;
                let mut escape = true;
                while c(s, n + 1) == b'\\' {
                    n += 1;
                    escape = !escape;
                }
                let code = c(s, n + 1);
                if escape
                    && !matches!(code, b'/' | b' ' | b'\n' | b'\t' | b'\r' | 0x0c)
                    && n + 1 < len
                {
                    n = super::postcss::last_byte(s, n + 1);
                }
                out.push(Tok {
                    ty: "word",
                    text: sl(pos, n + 1),
                });
                pos = n;
            }
            _ => {
                if code == b'/' && c(s, pos + 1) == b'*' {
                    let close = s.get(pos + 2..)?.windows(2).position(|w| w == b"*/")? + pos + 2;
                    let n = close + 1;
                    out.push(Tok {
                        ty: "comment",
                        text: sl(pos, n + 1),
                    });
                    pos = n;
                } else {
                    let mut i = pos + 1;
                    while i < len {
                        let b = s[i];
                        if matches!(
                            b,
                            b' ' | b'\n'
                                | b'\t'
                                | b'\r'
                                | b'('
                                | b')'
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
                        ) || (b == b'/' && c(s, i + 1) == b'*')
                        {
                            break;
                        }
                        i += 1;
                    }
                    let n = if i >= len { len - 1 } else { i - 1 };
                    out.push(Tok {
                        ty: "word",
                        text: sl(pos, n + 1),
                    });
                    pos = n;
                }
            }
        }
        pos += 1;
    }
    Some(out)
}

#[derive(Clone, Debug, PartialEq)]
pub enum Ns {
    None,
    True,
    Str(String),
}

#[derive(Clone, Debug)]
pub struct S {
    pub ty: &'static str,
    pub value: String,
    pub namespace: Ns,
    pub attribute: String,
    pub operator: Option<String>,
    pub attr_value: Option<String>,
    pub insensitive: bool,
    pub nodes: Option<Vec<usize>>,
}

fn s_node(ty: &'static str, value: String) -> S {
    S {
        ty,
        value,
        namespace: Ns::None,
        attribute: String::new(),
        operator: None,
        attr_value: None,
        insensitive: false,
        nodes: None,
    }
}

pub struct Tree {
    pub n: Vec<S>,
    pub parent: Vec<Option<usize>>,
}

struct P {
    t: Tree,
    tokens: Vec<Tok>,
    position: usize,
    current: usize,
    /// A `parse` step that consumed nothing: postcss-selector-parser loops forever there.
    hung: bool,
}

type O<T = ()> = Option<T>;

impl P {
    fn tok(&self) -> O<&Tok> {
        self.tokens.get(self.position)
    }
    fn next(&self) -> O<&Tok> {
        self.tokens.get(self.position + 1)
    }
    fn prev(&self) -> O<&Tok> {
        self.position
            .checked_sub(1)
            .and_then(|i| self.tokens.get(i))
    }

    fn append(&mut self, parent: usize, node: S) -> usize {
        self.t.n.push(node);
        let id = self.t.n.len() - 1;
        self.t.parent.push(Some(parent));
        if let Some(nodes) = self.t.n[parent].nodes.as_mut() {
            nodes.push(id);
        }
        id
    }

    fn new_node(&mut self, mut node: S, namespace: Option<Ns>) -> usize {
        if let Some(ns) = namespace
            && ns != Ns::None
        {
            node.namespace = ns;
        }
        self.append(self.current, node)
    }

    fn last(&self, id: usize) -> O<usize> {
        self.t.n[id].nodes.as_ref()?.last().copied()
    }

    fn step(&mut self, throw_on_paren: bool) -> O {
        let at = self.position;
        self.parse(throw_on_paren)?;
        if self.position == at {
            self.hung = true;
            return None;
        }
        Some(())
    }

    fn parse(&mut self, throw_on_paren: bool) -> O {
        let ty = self.tok()?.ty;
        match ty {
            "space" => self.space(),
            "comment" => {
                let v = self.tok()?.text.clone();
                self.new_node(s_node("comment", v), None);
                self.position += 1;
                Some(())
            }
            "(" => self.parentheses(),
            ")" => {
                if throw_on_paren {
                    return None;
                }
                Some(())
            }
            "[" => self.attribute(),
            "]" | ";" => None,
            "at-word" | "word" => self.word(None),
            ":" => self.pseudo(),
            "," => {
                if self.position == self.tokens.len() - 1 {
                    self.position += 1;
                    return Some(());
                }
                let parent = self.t.parent[self.current]?;
                let sel = s_node("selector", String::new());
                let mut sel = sel;
                sel.nodes = Some(Vec::new());
                self.current = self.append(parent, sel);
                self.position += 1;
                Some(())
            }
            "*" => self.universal(None),
            "&" => {
                let v = self.tok()?.text.clone();
                self.new_node(s_node("nesting", v), None);
                self.position += 1;
                Some(())
            }
            "combinator" => self.combinator(),
            "string" => {
                let v = self.tok()?.text.clone();
                self.new_node(s_node("string", v), None);
                self.position += 1;
                Some(())
            }
            _ => Some(()),
        }
    }

    fn space(&mut self) -> O {
        let prev_ty = self.prev().map(|t| t.ty);
        let next_ty = self.next().map(|t| t.ty);
        if self.position == 0 || prev_ty == Some(",") || prev_ty == Some("(") {
            self.position += 1;
        } else if self.position == self.tokens.len() - 1
            || next_ty == Some(",")
            || next_ty == Some(")")
        {
            self.last(self.current)?;
            self.position += 1;
        } else {
            self.combinator()?;
        }
        Some(())
    }

    fn combinator(&mut self) -> O {
        if self.tok()?.text == "|" {
            return self.namespace();
        }
        let mut value = String::new();
        while self.position < self.tokens.len() {
            let Some(cur) = self.tok() else {
                break;
            };
            if cur.ty != "space" && cur.ty != "combinator" {
                break;
            }
            let next_comb = self.next().is_some_and(|t| t.ty == "combinator");
            let prev_comb = self.prev().is_some_and(|t| t.ty == "combinator");
            if !next_comb && !prev_comb && (cur.ty == "combinator" || cur.ty == "space") {
                value = cur.text.clone();
            }
            self.position += 1;
        }
        self.new_node(s_node("combinator", value), None);
        Some(())
    }

    fn namespace(&mut self) -> O {
        let before = match self.prev().map(|t| t.text.clone()) {
            Some(s) if !s.is_empty() => Ns::Str(s),
            _ => Ns::True,
        };
        let next_ty = self.next()?.ty;
        if next_ty == "word" {
            self.position += 1;
            return self.word(Some(before));
        } else if next_ty == "*" {
            self.position += 1;
            return self.universal(Some(before));
        }
        Some(())
    }

    fn universal(&mut self, namespace: Option<Ns>) -> O {
        if self.next().is_some_and(|t| t.text == "|") {
            self.position += 1;
            return self.namespace();
        }
        let v = self.tok()?.text.clone();
        self.new_node(s_node("universal", v), namespace);
        self.position += 1;
        Some(())
    }

    fn word(&mut self, namespace: Option<Ns>) -> O {
        if self.next().is_some_and(|t| t.text == "|") {
            self.position += 1;
            return self.namespace();
        }
        self.split_word(namespace, None)
    }

    fn split_word(&mut self, namespace: Option<Ns>, pseudo: Option<String>) -> O {
        let mut word = self.tok()?.text.clone();
        while self.next().is_some_and(|t| t.ty == "word") {
            self.position += 1;
            let current = self.tok()?.text.clone();
            word.push_str(&current);
            if current.ends_with('\\') && self.next().is_some_and(|t| t.ty == "space") {
                let sp = self.next()?.text.clone();
                word.push_str(&sp);
                self.position += 1;
            }
        }
        let class: Vec<usize> = word.match_indices('.').map(|(i, _)| i).collect();
        let interp: Vec<usize> = word.match_indices("#{").map(|(i, _)| i).collect();
        let id: Vec<usize> = word
            .match_indices('#')
            .map(|(i, _)| i)
            .filter(|i| !interp.contains(i))
            .collect();
        let mut indices: Vec<usize> = vec![0];
        indices.extend(class.iter().copied());
        indices.extend(id.iter().copied());
        indices.sort_unstable();
        indices.dedup();
        let count = indices.len();
        for (i, &ind) in indices.iter().enumerate() {
            let index = indices
                .get(i + 1)
                .copied()
                .filter(|&x| x != 0)
                .unwrap_or(word.len());
            let value = word.get(ind..index)?.to_string();
            if i == 0
                && let Some(prefix) = &pseudo
            {
                let mut node = s_node("pseudo", format!("{prefix}{value}"));
                node.nodes = Some(Vec::new());
                self.new_node(node, None);
                if count > 1 && self.next().is_some_and(|t| t.ty == "(") {
                    return None;
                }
                continue;
            }
            let node = if class.contains(&ind) {
                s_node("class", value[1..].to_string())
            } else if id.contains(&ind) {
                s_node("id", value[1..].to_string())
            } else {
                s_node("tag", value)
            };
            self.new_node(node, namespace.clone());
        }
        self.position += 1;
        Some(())
    }

    fn pseudo(&mut self) -> O {
        let mut s = String::new();
        while self.tok().is_some_and(|t| t.ty == ":") {
            s.push(':');
            self.position += 1;
        }
        if self.tok()?.ty != "word" {
            return None;
        }
        self.split_word(None, Some(s))
    }

    fn parentheses(&mut self) -> O {
        let last = self.last(self.current);
        if let Some(l) = last
            && self.t.n[l].ty == "pseudo"
        {
            let mut sel = s_node("selector", String::new());
            sel.nodes = Some(Vec::new());
            let cache = self.current;
            let sid = self.append(l, sel);
            self.current = sid;
            let mut balanced = 1;
            self.position += 1;
            while self.position < self.tokens.len() && balanced > 0 {
                let ty = self.tok()?.ty;
                if ty == "(" {
                    balanced += 1;
                }
                if ty == ")" {
                    balanced -= 1;
                }
                if balanced > 0 && ty == ")" {
                    // Consumes nothing; the next turn counts the `)` again.
                    self.parse(false)?;
                } else if balanced > 0 {
                    self.step(false)?;
                } else {
                    self.position += 1;
                }
            }
            if balanced > 0 {
                return None;
            }
            self.current = cache;
        } else {
            let l = last?;
            let mut balanced = 1;
            self.position += 1;
            // `last.value += "("`; an attribute's `value` is its value, `undefined` if absent.
            paren_target(&mut self.t.n[l]).push('(');
            while self.position < self.tokens.len() && balanced > 0 {
                let t = self.tok()?.clone();
                if t.ty == "(" {
                    balanced += 1;
                }
                if t.ty == ")" {
                    balanced -= 1;
                }
                paren_target(&mut self.t.n[l]).push_str(&t.text);
                self.position += 1;
            }
            if balanced > 0 {
                return None;
            }
        }
        Some(())
    }

    fn attribute(&mut self) -> O {
        let mut s = String::new();
        self.position += 1;
        while self.position < self.tokens.len() && self.tok()?.ty != "]" {
            s.push_str(&self.tok()?.text.clone());
            self.position += 1;
        }
        // Without a `]` token postcss-selector-parser throws: explicitly, or on reading
        // `this.currToken[2]` past the end when the string holds a `]`.
        if self.position == self.tokens.len() {
            return None;
        }
        let (name_part, operator, value) = split_attribute(&s);
        let mut node = s_node("attribute", String::new());
        node.operator = operator;
        let ns_parts: Vec<&str> = name_part.splitn(2, '|').collect();
        if ns_parts.len() > 1 {
            node.namespace = if ns_parts[0].is_empty() {
                Ns::True
            } else {
                Ns::Str(ns_parts[0].to_string())
            };
            // `parts[0].split(/(\|)/g)[2]`: up to a second `|`.
            node.attribute = ns_parts[1].split('|').next().unwrap_or("").to_string();
        } else {
            node.attribute = name_part.to_string();
        }
        // `value` is `parts[2]`: absent without an operator, possibly empty after one.
        match value {
            Some(v) if !v.is_empty() => match insensitive_split(&v) {
                Some((head, _)) => {
                    node.attr_value = Some(head);
                    node.insensitive = true;
                }
                None => node.attr_value = Some(v),
            },
            v => node.attr_value = v,
        }
        self.new_node(node, None);
        self.position += 1;
        Some(())
    }
}

/// The string postcss-selector-parser's `parentheses` appends to: `value`, which on an
/// attribute is its value (`undefined` while absent).
fn paren_target(n: &mut S) -> &mut String {
    if n.ty == "attribute" {
        n.attr_value.get_or_insert_with(|| "undefined".to_string())
    } else {
        &mut n.value
    }
}

fn split_attribute(s: &str) -> (&str, Option<String>, Option<String>) {
    let b = s.as_bytes();
    for i in 0..b.len() {
        if b[i] == b'=' {
            let op_start = if i > 0 && matches!(b[i - 1], b'*' | b'~' | b'^' | b'$' | b'|') {
                i - 1
            } else {
                i
            };
            return (
                &s[..op_start],
                Some(s[op_start..=i].to_string()),
                Some(s[i + 1..].to_string()),
            );
        }
    }
    (s, None, None)
}

fn insensitive_split(v: &str) -> Option<(String, String)> {
    let t = v.trim_end_matches(|c: char| c.is_whitespace());
    let rest_ws = &v[t.len()..];
    let _ = rest_ws;
    let without_i = t.strip_suffix('i')?;
    let head = without_i.trim_end_matches(|c: char| c.is_whitespace());
    if head.len() == without_i.len() {
        return None;
    }
    Some((head.to_string(), v[head.len()..].to_string()))
}

/// postcss-selector-parser's result for `selector`, or `None` where it throws.
/// `Ok(None)` where postcss-selector-parser throws; `Err` where it never returns.
pub fn parse(selector: &str) -> Result<Option<Tree>, ()> {
    let Some(tokens) = tokenize(selector) else {
        return Ok(None);
    };
    let mut root = s_node("root", String::new());
    root.nodes = Some(vec![1]);
    let mut sel = s_node("selector", String::new());
    sel.nodes = Some(Vec::new());
    let mut p = P {
        t: Tree {
            n: vec![root, sel],
            parent: vec![None, Some(0)],
        },
        tokens,
        position: 0,
        current: 1,
        hung: false,
    };
    while p.position < p.tokens.len() {
        if p.step(true).is_none() {
            return if p.hung { Err(()) } else { Ok(None) };
        }
    }
    Ok(Some(p.t))
}
