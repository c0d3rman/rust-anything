//! postcss 8.5.6's tokenizer and parser (`lib/tokenize.js`, `lib/parser.js`; MIT, Copyright 2013
//! Andrey Sitnik). Offsets are byte offsets; every error postcss throws returns `None`.

#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Ty {
    Space,
    Word,
    String,
    AtWord,
    Brackets,
    Comment,
    Open,
    Ch(u8),
}

#[derive(Clone, Copy, Debug)]
pub struct Tok {
    pub ty: Ty,
    pub start: usize,
    pub end: usize,
    pub p2: Option<usize>,
    pub p3: Option<usize>,
    pub inline: bool,
}

impl Tok {
    pub fn text<'a>(&self, css: &'a str) -> &'a str {
        &css[self.start..self.end]
    }
}

struct Tokenizer<'a> {
    scss: bool,
    css: &'a [u8],
    pos: usize,
    buffer: Vec<Tok>,
    returned: Vec<Tok>,
}

fn c(css: &[u8], i: usize) -> u8 {
    css.get(i).copied().unwrap_or(0)
}

/// The last byte of the character at `i`: postcss steps one UTF-16 unit, the port one character.
pub fn last_byte(css: &[u8], i: usize) -> usize {
    let mut j = i;
    while j + 1 < css.len() && (css[j + 1] & 0xc0) == 0x80 {
        j += 1;
    }
    j
}

fn is_space(b: u8) -> bool {
    matches!(b, b' ' | b'\n' | b'\t' | b'\r' | 0x0c)
}

impl Tokenizer<'_> {
    fn end_of_file(&self) -> bool {
        self.returned.is_empty() && self.pos >= self.css.len()
    }

    fn back(&mut self, t: Tok) {
        self.returned.push(t);
    }

    fn next_token(&mut self) -> Option<Option<Tok>> {
        if let Some(t) = self.returned.pop() {
            return Some(Some(t));
        }
        let css = self.css;
        let len = css.len();
        if self.pos >= len {
            return Some(None);
        }
        let pos = self.pos;
        let code = css[pos];
        let tok;
        let mut new_pos = pos;
        match code {
            b' ' | b'\n' | b'\t' | b'\r' | 0x0c => {
                let mut next = pos;
                loop {
                    next += 1;
                    if !is_space(c(css, next)) || next >= len {
                        break;
                    }
                }
                tok = Tok {
                    ty: Ty::Space,
                    start: pos,
                    end: next,
                    p2: None,
                    p3: None,
                    inline: false,
                };
                new_pos = next - 1;
            }
            b',' if self.scss => {
                tok = Tok {
                    ty: Ty::Word,
                    start: pos,
                    end: pos + 1,
                    p2: Some(pos),
                    p3: Some(pos + 1),
                    inline: false,
                };
            }
            b'[' | b']' | b'{' | b'}' | b':' | b';' | b')' => {
                tok = Tok {
                    ty: Ty::Ch(code),
                    start: pos,
                    end: pos + 1,
                    p2: Some(pos),
                    p3: None,
                    inline: false,
                };
            }
            b'(' => {
                let prev = self
                    .buffer
                    .pop()
                    .map(|t| &css[t.start..t.end])
                    .unwrap_or(b"");
                let n = c(css, pos + 1);
                if self.scss && prev == b"url" && n != b'\'' && n != b'"' {
                    let mut brackets = 1;
                    let mut next = pos + 1;
                    while next < len {
                        match css[next] {
                            b'(' => brackets += 1,
                            b')' => {
                                brackets -= 1;
                                if brackets == 0 {
                                    break;
                                }
                            }
                            _ => {}
                        }
                        next += 1;
                    }
                    tok = Tok {
                        ty: Ty::Brackets,
                        start: pos,
                        end: (next + 1).min(len),
                        p2: Some(pos),
                        p3: Some(next),
                        inline: false,
                    };
                    new_pos = next;
                } else if prev == b"url" && n != b'\'' && n != b'"' && !is_space(n) {
                    let mut next = pos;
                    loop {
                        let mut escaped = false;
                        next = find(css, b")", next + 1)?;
                        let mut ep = next;
                        while ep > 0 && css[ep - 1] == b'\\' {
                            ep -= 1;
                            escaped = !escaped;
                        }
                        if !escaped {
                            break;
                        }
                    }
                    tok = Tok {
                        ty: Ty::Brackets,
                        start: pos,
                        end: next + 1,
                        p2: Some(pos),
                        p3: Some(next),
                        inline: false,
                    };
                    new_pos = next;
                } else {
                    let next = find(css, b")", pos + 1);
                    let bad = match next {
                        None => true,
                        Some(n) => {
                            let content = &css[pos..=n];
                            (1..content.len()).any(|i| {
                                matches!(
                                    content[i],
                                    b'\r' | b'\n' | b'"' | b'\'' | b'(' | b'/' | b'\\'
                                ) && !matches!(content[i - 1], b'\n' | b'\r')
                            })
                        }
                    };
                    match next {
                        Some(n) if !bad => {
                            tok = Tok {
                                ty: Ty::Brackets,
                                start: pos,
                                end: n + 1,
                                p2: Some(pos),
                                p3: Some(n),
                                inline: false,
                            };
                            new_pos = n;
                        }
                        _ => {
                            tok = Tok {
                                ty: Ty::Open,
                                start: pos,
                                end: pos + 1,
                                p2: Some(pos),
                                p3: None,
                                inline: false,
                            };
                        }
                    }
                }
            }
            b'\'' | b'"' if self.scss => {
                let mut next = pos;
                let mut escaped = false;
                loop {
                    next += 1;
                    if next >= len {
                        return None;
                    }
                    let ch = css[next];
                    if !escaped && ch == code {
                        break;
                    } else if ch == b'\\' {
                        escaped = !escaped;
                    } else if escaped {
                        escaped = false;
                    } else if ch == b'#' && c(css, next + 1) == b'{' {
                        next = interpolation(css, next)?;
                    }
                }
                tok = Tok {
                    ty: Ty::String,
                    start: pos,
                    end: next + 1,
                    p2: Some(pos),
                    p3: Some(next),
                    inline: false,
                };
                new_pos = next;
            }
            b'\'' | b'"' => {
                let mut next = pos;
                loop {
                    let mut escaped = false;
                    next = find(css, &[code], next + 1)?;
                    let mut ep = next;
                    while ep > 0 && css[ep - 1] == b'\\' {
                        ep -= 1;
                        escaped = !escaped;
                    }
                    if !escaped {
                        break;
                    }
                }
                tok = Tok {
                    ty: Ty::String,
                    start: pos,
                    end: next + 1,
                    p2: Some(pos),
                    p3: Some(next),
                    inline: false,
                };
                new_pos = next;
            }
            b'@' => {
                let mut i = pos + 1;
                while i < len
                    && !matches!(
                        css[i],
                        b'\t'
                            | b'\n'
                            | 0x0c
                            | b'\r'
                            | b' '
                            | b'"'
                            | b'#'
                            | b'\''
                            | b'('
                            | b')'
                            | b'/'
                            | b';'
                            | b'['
                            | b'\\'
                            | b']'
                            | b'{'
                            | b'}'
                    )
                {
                    i += 1;
                }
                let next = if i >= len { len - 1 } else { i - 1 };
                tok = Tok {
                    ty: Ty::AtWord,
                    start: pos,
                    end: next + 1,
                    p2: Some(pos),
                    p3: Some(next),
                    inline: false,
                };
                new_pos = next;
            }
            b'\\' => {
                let mut next = pos;
                let mut escape = true;
                while c(css, next + 1) == b'\\' {
                    next += 1;
                    escape = !escape;
                }
                let code = c(css, next + 1);
                if escape && code != b'/' && !is_space(code) && next + 1 < len {
                    next = last_byte(css, next + 1);
                    if c(css, next).is_ascii_hexdigit() {
                        while c(css, next + 1).is_ascii_hexdigit() {
                            next += 1;
                        }
                        if c(css, next + 1) == b' ' {
                            next += 1;
                        }
                    }
                } else if escape && code != b'/' && !is_space(code) {
                    next += 1;
                }
                let end = (next + 1).min(len);
                tok = Tok {
                    ty: Ty::Word,
                    start: pos,
                    end,
                    p2: Some(pos),
                    p3: Some(next),
                    inline: false,
                };
                new_pos = next;
            }
            b'#' if self.scss && c(css, pos + 1) == b'{' => {
                let next = interpolation(css, pos)?;
                tok = Tok {
                    ty: Ty::Word,
                    start: pos,
                    end: next + 1,
                    p2: Some(pos),
                    p3: Some(next),
                    inline: false,
                };
                new_pos = next;
            }
            b'/' if self.scss && c(css, pos + 1) == b'/' => {
                let mut i = pos + 1;
                while i < len && !matches!(css[i], b'\n' | 0x0c | b'\r') {
                    i += 1;
                }
                let next = if i >= len { len - 1 } else { i - 1 };
                tok = Tok {
                    ty: Ty::Comment,
                    start: pos,
                    end: next + 1,
                    p2: Some(pos),
                    p3: Some(next),
                    inline: true,
                };
                new_pos = next;
            }
            _ => {
                if code == b'/' && c(css, pos + 1) == b'*' {
                    let close = find(css, b"*/", pos + 2)?;
                    let next = close + 1;
                    tok = Tok {
                        ty: Ty::Comment,
                        start: pos,
                        end: next + 1,
                        p2: Some(pos),
                        p3: Some(next),
                        inline: false,
                    };
                    new_pos = next;
                } else {
                    let mut i = pos + 1;
                    while i < len {
                        let b = css[i];
                        if matches!(
                            b,
                            b'\t'
                                | b'\n'
                                | 0x0c
                                | b'\r'
                                | b' '
                                | b'!'
                                | b'"'
                                | b'#'
                                | b'\''
                                | b'('
                                | b')'
                                | b':'
                                | b';'
                                | b'@'
                                | b'['
                                | b'\\'
                                | b']'
                                | b'{'
                                | b'}'
                        ) || (b == b'/' && c(css, i + 1) == b'*')
                            || (self.scss && b == b',')
                        {
                            break;
                        }
                        i += 1;
                    }
                    let next = if i >= len { len - 1 } else { i - 1 };
                    tok = Tok {
                        ty: Ty::Word,
                        start: pos,
                        end: next + 1,
                        p2: Some(pos),
                        p3: Some(next),
                        inline: false,
                    };
                    self.buffer.push(tok);
                    new_pos = next;
                }
            }
        }
        self.pos = new_pos + 1;
        Some(Some(tok))
    }
}

fn interpolation(css: &[u8], start: usize) -> Option<usize> {
    let mut next = start;
    let mut deep = 1;
    let mut quote: Option<u8> = None;
    let mut escaped = false;
    while deep > 0 {
        next += 1;
        if css.len() <= next {
            return None;
        }
        let code = css[next];
        let n = c(css, next + 1);
        if let Some(q) = quote {
            if !escaped && code == q {
                quote = None;
                escaped = false;
            } else if code == b'\\' {
                escaped = !escaped;
            } else if escaped {
                escaped = false;
            }
        } else if code == b'\'' || code == b'"' {
            quote = Some(code);
        } else if code == b'}' {
            deep -= 1;
        } else if code == b'#' && n == b'{' {
            deep += 1;
        }
    }
    Some(next)
}

fn fix_comment_marks(s: &str) -> String {
    let b = s.as_bytes();
    let mut out = String::new();
    let mut i = 0;
    let mut last = 0;
    while i + 1 < b.len() {
        if (b[i] == b'*' && b[i + 1] == b'/') || (b[i] == b'/' && b[i + 1] == b'*') {
            out.push_str(&s[last..i]);
            out.push_str("*//*");
            i += 2;
            last = i;
        } else {
            i += 1;
        }
    }
    out.push_str(&s[last..]);
    out
}

fn find(hay: &[u8], needle: &[u8], from: usize) -> Option<usize> {
    if from > hay.len() {
        return None;
    }
    hay[from..]
        .windows(needle.len())
        .position(|w| w == needle)
        .map(|i| i + from)
}

#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Kind {
    Root,
    Rule,
    AtRule,
    Decl,
    Comment,
}

#[derive(Clone, Debug, Default)]
pub struct Raws {
    pub before: String,
    pub after: String,
    pub between: String,
    pub after_name: String,
    pub semicolon: bool,
    pub important: Option<String>,
    pub own_semicolon: Option<String>,
    pub inline: bool,
    pub left: String,
    pub right: String,
    pub selector: Option<(String, String)>,
    pub value: Option<(String, String)>,
    pub params: Option<(String, String)>,
}

#[derive(Clone, Debug)]
pub struct Node {
    pub kind: Kind,
    pub start: usize,
    pub end: Option<usize>,
    pub raws: Raws,
    pub selector: Option<String>,
    pub prop: String,
    pub value: Option<String>,
    pub name: String,
    pub params: Option<String>,
    pub text: String,
    pub important: bool,
    pub mixin: bool,
    pub function: bool,
    pub variable: bool,
    pub identifier: String,
    pub nodes: Option<Vec<Node>>,
}

#[derive(Default)]
struct AtExtra {
    function_params: Option<String>,
    mixin: Option<(String, Option<String>)>,
}

fn new_node(kind: Kind) -> Node {
    Node {
        kind,
        start: 0,
        end: None,
        raws: Raws::default(),
        selector: None,
        prop: String::new(),
        value: None,
        name: String::new(),
        params: None,
        text: String::new(),
        important: false,
        mixin: false,
        function: false,
        variable: false,
        identifier: String::new(),
        nodes: if matches!(kind, Kind::Root | Kind::Rule) {
            Some(Vec::new())
        } else {
            None
        },
    }
}

pub struct Parser<'a> {
    css: &'a str,
    t: Tokenizer<'a>,
    stack: Vec<Node>,
    spaces: String,
    semicolon: bool,
    less: bool,
}

type O<T = ()> = Option<T>;

fn has_positions(t: &Tok) -> Option<usize> {
    t.p3.filter(|&x| x != 0).or(t.p2.filter(|&x| x != 0))
}

impl<'a> Parser<'a> {
    fn text(&self, t: &Tok) -> &'a str {
        t.text(self.css)
    }

    fn current(&mut self) -> &mut Node {
        let n = self.stack.len() - 1;
        &mut self.stack[n]
    }

    fn push(&mut self, mut node: Node, offset: usize) -> usize {
        node.start = offset;
        node.raws.before = std::mem::take(&mut self.spaces);
        if node.kind != Kind::Comment {
            self.semicolon = false;
        }
        let cur = self.current();
        let nodes = cur.nodes.get_or_insert_with(Vec::new);
        nodes.push(node);
        nodes.len() - 1
    }

    fn last_child(&mut self) -> O<&mut Node> {
        self.current().nodes.as_mut()?.last_mut()
    }

    pub fn parse(css: &'a str, scss: bool, less: bool) -> O<Node> {
        let mut p = Parser {
            css,
            t: Tokenizer {
                scss,
                css: css.as_bytes(),
                pos: 0,
                buffer: Vec::new(),
                returned: Vec::new(),
            },
            stack: vec![new_node(Kind::Root)],
            spaces: String::new(),
            semicolon: false,
            less,
        };
        while !p.t.end_of_file() {
            let Some(token) = p.t.next_token()? else {
                break;
            };
            match token.ty {
                Ty::Space => {
                    let s = p.text(&token);
                    p.spaces.push_str(s);
                }
                Ty::Ch(b';') => p.free_semicolon(&token),
                Ty::Ch(b'}') => p.end(&token)?,
                Ty::Comment => p.comment(&token),
                Ty::AtWord if p.less => p.less_atrule(token)?,
                Ty::AtWord => p.scss_atrule(token)?,
                Ty::Ch(b'{') => p.empty_rule(&token),
                _ if p.less => p.less_other(token)?,
                _ => p.other(token)?,
            }
        }
        if p.stack.len() > 1 {
            return None;
        }
        let semicolon = p.semicolon;
        let spaces = std::mem::take(&mut p.spaces);
        let len = css.len();
        let root = p.current();
        if root.nodes.as_ref().is_some_and(|n| !n.is_empty()) {
            root.raws.semicolon = semicolon;
        }
        root.raws.after.push_str(&spaces);
        root.start = 0;
        root.end = Some(len);
        p.stack.pop()
    }

    fn free_semicolon(&mut self, token: &Tok) {
        let s = self.text(token);
        self.spaces.push_str(s);
        let spaces = self.spaces.clone();
        let mut take = false;
        if let Some(prev) = self.last_child()
            && prev.kind == Kind::Rule
            && prev.raws.own_semicolon.is_none()
        {
            prev.raws.own_semicolon = Some(spaces.clone());
            prev.end = Some(token.start + spaces.len());
            take = true;
        }
        if take {
            self.spaces.clear();
        }
    }

    fn end(&mut self, token: &Tok) -> O {
        let semicolon = self.semicolon;
        let spaces = std::mem::take(&mut self.spaces);
        self.semicolon = false;
        let cur = self.current();
        if cur.nodes.as_ref().is_some_and(|n| !n.is_empty()) {
            cur.raws.semicolon = semicolon;
        }
        cur.raws.after.push_str(&spaces);
        if self.stack.len() == 1 {
            return None;
        }
        let mut node = self.stack.pop()?;
        node.end = Some(token.start + 1);
        let parent = self.current();
        let nodes = parent.nodes.as_mut()?;
        let last = nodes.last_mut()?;
        *last = node;
        Some(())
    }

    fn comment(&mut self, token: &Tok) {
        if token.inline {
            let mut node = new_node(Kind::Comment);
            node.raws.inline = true;
            node.end = Some(token.end);
            let text = &self.css[token.start + 2..token.end];
            if text.trim_matches(js_ws).is_empty() {
                node.raws.left = text.to_string();
            } else {
                let start = text.len() - text.trim_start_matches(js_ws).len();
                let end = text.trim_end_matches(js_ws).len();
                node.raws.left = text[..start].to_string();
                node.text = fix_comment_marks(&text[start..end]);
                node.raws.right = text[end..].to_string();
            }
            self.push(node, token.start);
            return;
        }
        let mut node = new_node(Kind::Comment);
        node.end = Some(token.end);
        let text = &self.css[token.start + 2..token.end - 2];
        if text.trim_matches(js_ws).is_empty() {
            node.raws.left = text.to_string();
        } else {
            let start = text.len() - text.trim_start_matches(js_ws).len();
            let end = text.trim_end_matches(js_ws).len();
            node.raws.left = text[..start].to_string();
            node.text = text[start..end].to_string();
            node.raws.right = text[end..].to_string();
        }
        self.push(node, token.start);
    }

    fn empty_rule(&mut self, token: &Tok) {
        let mut node = new_node(Kind::Rule);
        node.selector = Some(String::new());
        self.push(node, token.start);
        self.open_last();
    }

    fn open_last(&mut self) {
        let cur = self.current();
        if let Some(nodes) = cur.nodes.as_mut()
            && let Some(last) = nodes.last()
        {
            let n = last.clone();
            self.stack.push(n);
        }
    }

    fn raw(&self, tokens: &[Tok], custom: bool) -> (String, Option<(String, String)>) {
        let mut value = String::new();
        let mut clean = true;
        let n = tokens.len();
        for (i, t) in tokens.iter().enumerate() {
            match t.ty {
                Ty::Space if i == n - 1 && !custom => clean = false,
                Ty::Comment => {
                    let safe = |x: Option<&Tok>| x.is_none_or(|t| t.ty == Ty::Space);
                    let prev = if i > 0 { tokens.get(i - 1) } else { None };
                    if !safe(prev) && !safe(tokens.get(i + 1)) {
                        if value.ends_with(',') {
                            clean = false;
                        } else {
                            value.push_str(self.text(t));
                        }
                    } else {
                        clean = false;
                    }
                }
                _ => value.push_str(self.text(t)),
            }
        }
        if clean {
            (value, None)
        } else {
            let raw: String = tokens.iter().map(|t| self.text(t)).collect();
            (value.clone(), Some((raw, value)))
        }
    }

    fn spaces_and_comments_from_end(&self, tokens: &mut Vec<Tok>) -> String {
        let mut s = String::new();
        while let Some(t) = tokens.last()
            && matches!(t.ty, Ty::Space | Ty::Comment)
        {
            s.insert_str(0, self.text(t));
            tokens.pop();
        }
        s
    }

    fn spaces_and_comments_from_start(&self, tokens: &mut Vec<Tok>) -> String {
        let mut s = String::new();
        while let Some(t) = tokens.first()
            && matches!(t.ty, Ty::Space | Ty::Comment)
        {
            s.push_str(self.text(t));
            tokens.remove(0);
        }
        s
    }

    fn rule(&mut self, tokens: Vec<Tok>) -> O {
        if self.t.scss {
            let mut with_colon = false;
            let mut brackets = 0i64;
            let mut value = String::new();
            for t in &tokens {
                if with_colon {
                    if !matches!(t.ty, Ty::Comment | Ty::Ch(b'{')) {
                        value.push_str(self.text(t));
                    }
                } else if t.ty == Ty::Space && self.text(t).contains('\n') {
                    break;
                } else if t.ty == Ty::Open {
                    brackets += 1;
                } else if t.ty == Ty::Ch(b')') {
                    brackets -= 1;
                } else if brackets == 0 && t.ty == Ty::Ch(b':') {
                    with_colon = true;
                }
            }
            let plain = !with_colon
                || value.trim_matches(js_ws).is_empty()
                || value.bytes().next().is_some_and(|b| {
                    b == b'#' || b == b':' || b == b'-' || b.is_ascii_alphabetic()
                });
            if !plain {
                return self.nested_decl(tokens);
            }
        }
        if self.less && tokens.len() >= 2 {
            let last = tokens[tokens.len() - 1];
            let prev = tokens[tokens.len() - 2];
            if prev.ty == Ty::AtWord && last.ty == Ty::Ch(b'{') {
                self.t.back(last);
                if self.less_interpolation(prev)? {
                    let new_token = self.t.next_token()??;
                    let mut ts = tokens[..tokens.len() - 2].to_vec();
                    ts.push(new_token);
                    for t in ts.into_iter().rev() {
                        self.t.back(t);
                    }
                    return Some(());
                }
            }
        }
        self.plain_rule(tokens);
        Some(())
    }

    fn nested_decl(&mut self, mut tokens: Vec<Tok>) -> O {
        tokens.pop();
        let mut node = new_node(Kind::Decl);
        node.nodes = Some(Vec::new());
        let start0 = tokens.first()?.p2.unwrap_or(0);
        let last = *tokens.iter().rev().find(|t| t.ty != Ty::Space)?;
        node.end = Some(match last.p3 {
            Some(e) if e != 0 => e + 1,
            _ => last.p2.unwrap_or(0) + 1,
        });
        let idx = self.push(node, start0);
        let mut before_extra = String::new();
        while tokens.first()?.ty != Ty::Word {
            before_extra.push_str(self.text(&tokens.remove(0)));
        }
        let start = match tokens[0].p2 {
            Some(s) if s != 0 => s,
            _ => start0,
        };
        let mut prop = String::new();
        while let Some(t) = tokens.first() {
            if matches!(t.ty, Ty::Ch(b':') | Ty::Space | Ty::Comment) {
                break;
            }
            prop.push_str(self.text(t));
            tokens.remove(0);
        }
        let mut between = String::new();
        while !tokens.is_empty() {
            let t = tokens.remove(0);
            between.push_str(self.text(&t));
            if t.ty == Ty::Ch(b':') {
                break;
            }
        }
        if prop.starts_with('_') || prop.starts_with('*') {
            before_extra.push_str(&prop[..1]);
            prop = prop[1..].to_string();
        }
        between.push_str(&self.spaces_and_comments_from_start(&mut tokens));
        let mut important = false;
        let mut raws_important = None;
        let mut i = tokens.len();
        while i > 1 {
            i -= 1;
            let t = tokens[i];
            let text = self.text(&t);
            if text == "!important" {
                important = true;
                let s: String = tokens[i..].iter().map(|t| self.text(t)).collect();
                tokens.truncate(i);
                let mut sp = String::new();
                while let Some(t) = tokens.last()
                    && t.ty == Ty::Space
                {
                    sp.insert_str(0, self.text(t));
                    tokens.pop();
                }
                let s = sp + &s;
                if s != " !important" {
                    raws_important = Some(s);
                }
                break;
            } else if text == "important" {
                let mut cache = tokens.clone();
                let mut s = String::new();
                let mut j = i;
                while j > 0 {
                    let ty = cache[j].ty;
                    if s.trim_matches(js_ws).starts_with('!') && ty != Ty::Space {
                        break;
                    }
                    let popped = cache.pop()?;
                    s.insert_str(0, self.text(&popped));
                    j -= 1;
                }
                if s.trim_matches(js_ws).starts_with('!') {
                    important = true;
                    raws_important = Some(s);
                    tokens = cache;
                }
            }
            if !matches!(t.ty, Ty::Space | Ty::Comment) {
                break;
            }
        }
        let (value, raw) = self.raw(&tokens, false);
        if value.contains(':') && self.has_colon(&tokens) {
            return None;
        }
        {
            let cur = self.current();
            let node = &mut cur.nodes.as_mut()?[idx];
            node.raws.before.push_str(&before_extra);
            node.start = start;
            node.prop = prop;
            node.raws.between = between;
            node.important = important;
            node.raws.important = raws_important;
            node.value = Some(value);
            node.raws.value = raw;
        }
        self.open_last();
        Some(())
    }

    fn plain_rule(&mut self, mut tokens: Vec<Tok>) {
        tokens.pop();
        let mut node = new_node(Kind::Rule);
        let start = tokens[0].p2.unwrap_or(0);
        node.raws.between = self.spaces_and_comments_from_end(&mut tokens);
        let (v, r) = self.raw(&tokens, false);
        node.selector = Some(v);
        node.raws.selector = r;
        self.push(node, start);
        self.open_last();
    }

    fn other(&mut self, start: Tok) -> O {
        let mut end = false;
        let mut colon = false;
        let mut brackets: Vec<u8> = Vec::new();
        let custom = self.text(&start).starts_with("--");
        let mut tokens: Vec<Tok> = Vec::new();
        let mut token = Some(start);
        while let Some(t) = token {
            tokens.push(t);
            match t.ty {
                Ty::Open | Ty::Ch(b'[') => {
                    brackets.push(if t.ty == Ty::Open { b')' } else { b']' })
                }
                Ty::Ch(b'{') if custom && colon => brackets.push(b'}'),
                _ if brackets.is_empty() => match t.ty {
                    Ty::Ch(b';') => {
                        if colon {
                            return self.decl(tokens, custom);
                        }
                        break;
                    }
                    Ty::Ch(b'{') => {
                        return self.rule(tokens);
                    }
                    Ty::Ch(b'}') => {
                        let last = tokens.pop()?;
                        self.t.back(last);
                        end = true;
                        break;
                    }
                    Ty::Ch(b':') => colon = true,
                    _ => {}
                },
                Ty::Ch(ch) if Some(&ch) == brackets.last() => {
                    brackets.pop();
                }
                _ => {}
            }
            token = self.t.next_token()?;
        }
        if self.t.end_of_file() {
            end = true;
        }
        if !brackets.is_empty() {
            return None;
        }
        if end && colon {
            if !custom {
                while let Some(t) = tokens.last()
                    && matches!(t.ty, Ty::Space | Ty::Comment)
                {
                    let t = tokens.pop()?;
                    self.t.back(t);
                }
            }
            return self.decl(tokens, custom);
        }
        self.unknown_word(tokens)
    }

    fn unknown_word(&mut self, tokens: Vec<Tok>) -> O {
        if !self.less {
            return None;
        }
        let first = self.text(tokens.first()?);
        if first == "each" && tokens.get(1)?.ty == Ty::Open {
            return self.less_each(tokens);
        }
        let b = first.as_bytes();
        let hex = b.first() == Some(&b'#')
            && (b.len() == 7 || b.len() == 4)
            && b[1..].iter().all(u8::is_ascii_hexdigit);
        let fraction = b.windows(2).any(|w| w[0] == b'.' && w[1].is_ascii_digit());
        if matches!(b.first(), Some(b'.' | b'#')) && !hex && !fraction {
            return self.less_mixin(tokens);
        }
        None
    }

    fn less_each(&mut self, mut tokens: Vec<Tok>) -> O {
        let first_paren = tokens.iter().position(|t| t.ty == Ty::Open)?;
        let last_paren = tokens
            .iter()
            .rposition(|t| t.ty == Ty::Ch(b')'))
            .unwrap_or(0);
        let count = last_paren.min(tokens.len() - first_paren);
        let params: String = tokens
            .drain(first_paren..first_paren + count)
            .map(|t| self.text(&t).to_string())
            .collect();
        for t in tokens.into_iter().rev() {
            self.t.back(t);
        }
        let token = self.t.next_token()??;
        let text = format!(" {}", self.text(&token));
        self.less_atrule_with(
            token,
            text,
            AtExtra {
                function_params: Some(params),
                mixin: None,
            },
        )
    }

    fn less_mixin(&mut self, mut tokens: Vec<Tok>) -> O {
        let identifier = self.text(&tokens[0])[..1].to_string();
        let brackets_index = tokens.iter().position(|t| t.ty == Ty::Brackets);
        let first_paren = tokens.iter().position(|t| t.ty == Ty::Open);
        if let Some(fp) = first_paren
            && fp > 0
            && brackets_index.is_none_or(|b| b > 3)
        {
            let lp = (1..tokens.len())
                .rev()
                .find(|&i| tokens[i].ty == Ty::Ch(b')'))?;
            let end = (lp + fp).min(tokens.len());
            if end <= fp {
                return None;
            }
            let paren = tokens[fp];
            let merged = Tok {
                ty: Ty::Brackets,
                start: tokens[fp].start,
                end: tokens[end - 1].end,
                p2: paren.p2,
                p3: paren.p3,
                inline: false,
            };
            let mut next: Vec<Tok> = tokens[..fp].to_vec();
            next.push(merged);
            next.extend_from_slice(&tokens[lp + 1..]);
            tokens = next;
        }
        let mut bang: Option<usize> = None;
        let mut bang_end = 0;
        for (i, t) in tokens.iter().enumerate() {
            let text = self.text(t);
            if text == "!" || bang.is_some() {
                bang.get_or_insert(i);
                bang_end = i;
            }
            if text == "important" {
                break;
            }
        }
        if let Some(b) = bang {
            // postcss-less joins the tokens' texts. After the paren merge above overshot (it
            // slices to `lastParenIndex + firstParenIndex`), tokens after the last `)` appear
            // twice and the joined text is longer than its span, which a port token can't hold.
            let joined: usize = tokens[b..=bang_end].iter().map(|t| t.end - t.start).sum();
            if joined != tokens[bang_end].end - tokens[b].start {
                return None;
            }
            let first = tokens[b];
            let merged = Tok {
                ty: Ty::Word,
                start: first.start,
                end: tokens[bang_end].end,
                p2: first.p2,
                p3: first.p3,
                inline: false,
            };
            tokens.splice(b..=bang_end, [merged]);
        }
        let mut important = None;
        let imp = tokens.iter().position(|t| {
            let s = self.text(t).to_lowercase();
            s.strip_suffix("important")
                .and_then(|x| x.trim_end_matches(js_ws).strip_suffix('!'))
                .is_some()
        });
        if let Some(i) = imp
            && i > 0
        {
            important = Some(self.text(&tokens[i]).to_string());
            tokens.remove(i);
        }
        for t in tokens.into_iter().rev() {
            self.t.back(t);
        }
        let token = self.t.next_token()??;
        let text = self.text(&token).to_string();
        self.less_atrule_with(
            token,
            text,
            AtExtra {
                function_params: None,
                mixin: Some((identifier, important)),
            },
        )
    }

    fn less_other(&mut self, token: Tok) -> O {
        let text = self.text(&token);
        if token.ty == Ty::Word && text.starts_with("//") {
            return self.less_inline_comment(token);
        }
        if text == "/" {
            // postcss-less issue #135: `/` then `/*...` is `//` then the word `*...`.
            let next = self.t.next_token()??;
            if next.ty == Ty::Comment && self.text(&next).starts_with("/*") {
                let word = Tok {
                    ty: Ty::Word,
                    start: next.start + 1,
                    ..next
                };
                // The word keeps the comment's start offset in postcss-less; a port token's
                // text is its span, so one that would be handed back to the parser is refused.
                if self.text(&word).contains('\n') {
                    return None;
                }
                self.t.back(word);
                return self.less_inline_comment(token);
            }
        }
        self.other(token)
    }

    fn less_inline_comment(&mut self, first: Tok) -> O {
        let mut last = first;
        let mut tok = Some(first);
        while let Some(t) = tok {
            let text = self.text(&t);
            if text.contains('\n') {
                let quote = text.find(['\'', '"']);
                if quote.is_some_and(|q| text[q..].contains('\n')) {
                    return None;
                }
                self.t.back(t);
                break;
            }
            last = t;
            tok = self.t.next_token()?;
        }
        let mut node = new_node(Kind::Comment);
        node.raws.inline = true;
        let css = self.css.as_bytes();
        let mut end = first.start;
        while end < css.len() && css[end] != b'\n' && css[end] != b'\r' {
            end += 1;
        }
        node.end = Some(end);
        let text = &self.css[first.start + 2..last.end.max(first.start + 2)];
        if text.trim_matches(js_ws).is_empty() {
            node.raws.left = text.to_string();
        } else {
            let start = text.len() - text.trim_start_matches(js_ws).len();
            let end = text.trim_end_matches(js_ws).len();
            node.raws.left = text[..start].to_string();
            node.text = text[start..end].to_string();
            node.raws.right = text[end..].to_string();
        }
        self.push(node, first.start);
        Some(())
    }

    fn less_interpolation(&mut self, token: Tok) -> O<bool> {
        let next = self.t.next_token()??;
        if self.text(&token).len() > 1 || next.ty != Ty::Ch(b'{') {
            self.t.back(next);
            return Some(false);
        }
        let mut last = next;
        let mut tok = self.t.next_token()?;
        while let Some(t) = tok
            && matches!(t.ty, Ty::Word | Ty::Ch(b'}'))
        {
            last = t;
            tok = self.t.next_token()?;
        }
        let merged = Tok {
            ty: Ty::Word,
            start: token.start,
            end: last.end,
            p2: token.p2,
            p3: last.p2,
            inline: false,
        };
        self.t.back(tok?);
        self.t.back(merged);
        Some(true)
    }

    fn less_atrule(&mut self, token: Tok) -> O {
        let text = self.text(&token).to_string();
        self.less_atrule_with(token, text, AtExtra::default())
    }

    fn less_atrule_with(&mut self, token: Tok, text: String, extra: AtExtra) -> O {
        if self.less_interpolation(token)? {
            return Some(());
        }
        self.atrule_full(token, text, extra)
    }

    fn decl(&mut self, mut tokens: Vec<Tok>, custom: bool) -> O {
        let mut node = new_node(Kind::Decl);
        let start0 = tokens[0].p2.unwrap_or(0);
        let last = *tokens.last()?;
        if last.ty == Ty::Ch(b';') {
            self.semicolon = true;
            tokens.pop();
        }
        let end_pos = has_positions(&last)
            .or_else(|| tokens.iter().rev().find_map(has_positions))
            .unwrap_or(0);
        node.end = Some(end_pos + 1);
        let idx = self.push(node, start0);
        let mut before_extra = String::new();
        while tokens.first()?.ty != Ty::Word {
            if tokens.len() == 1 {
                return None;
            }
            before_extra.push_str(self.text(&tokens.remove(0)));
        }
        let start = tokens[0].p2.unwrap_or(0);
        let mut prop = String::new();
        while let Some(t) = tokens.first() {
            if matches!(t.ty, Ty::Ch(b':') | Ty::Space | Ty::Comment) {
                break;
            }
            prop.push_str(self.text(t));
            tokens.remove(0);
        }
        let mut between = String::new();
        while !tokens.is_empty() {
            let t = tokens.remove(0);
            if t.ty == Ty::Ch(b':') {
                between.push(':');
                break;
            }
            if t.ty == Ty::Word
                && self
                    .text(&t)
                    .bytes()
                    .any(|b| b.is_ascii_alphanumeric() || b == b'_')
            {
                return None;
            }
            between.push_str(self.text(&t));
        }
        if prop.starts_with('_') || prop.starts_with('*') {
            before_extra.push_str(&prop[..1]);
            prop = prop[1..].to_string();
        }
        let mut first_spaces = Vec::new();
        while let Some(t) = tokens.first()
            && matches!(t.ty, Ty::Space | Ty::Comment)
        {
            first_spaces.push(tokens.remove(0));
        }
        let mut important = false;
        let mut raws_important = None;
        let mut i = tokens.len();
        while i > 0 {
            i -= 1;
            let t = tokens[i];
            let lower = self.text(&t).to_lowercase();
            if lower == "!important" {
                important = true;
                let s: String = tokens[i..].iter().map(|t| self.text(t)).collect();
                tokens.truncate(i);
                let mut sp = String::new();
                while let Some(t) = tokens.last()
                    && t.ty == Ty::Space
                {
                    sp.insert_str(0, self.text(t));
                    tokens.pop();
                }
                let s = sp + &s;
                if s != " !important" {
                    raws_important = Some(s);
                }
                break;
            } else if lower == "important" {
                let mut cache = tokens.clone();
                let mut s = String::new();
                let mut j = i;
                while j > 0 {
                    let ty = cache[j].ty;
                    if s.trim_matches(js_ws).starts_with('!') && ty != Ty::Space {
                        break;
                    }
                    let popped = cache.pop()?;
                    s.insert_str(0, self.text(&popped));
                    j -= 1;
                }
                if s.trim_matches(js_ws).starts_with('!') {
                    important = true;
                    raws_important = Some(s);
                    tokens = cache;
                }
            }
            if !matches!(t.ty, Ty::Space | Ty::Comment) {
                break;
            }
        }
        let has_word = tokens
            .iter()
            .any(|t| !matches!(t.ty, Ty::Space | Ty::Comment));
        if has_word {
            for t in &first_spaces {
                between.push_str(self.text(t));
            }
            first_spaces.clear();
        }
        let mut all = first_spaces;
        all.extend(tokens.iter().copied());
        let (value, raw) = self.raw(&all, custom);
        if value.contains(':') && !custom && self.has_colon(&tokens) {
            return None;
        }
        let cur = self.current();
        let node = &mut cur.nodes.as_mut()?[idx];
        node.raws.before.push_str(&before_extra);
        node.start = start;
        node.prop = prop;
        node.raws.between = between;
        node.important = important;
        node.raws.important = raws_important;
        node.value = Some(value);
        node.raws.value = raw;
        Some(())
    }

    fn has_colon(&self, tokens: &[Tok]) -> bool {
        let mut brackets: i64 = 0;
        let mut prev: Option<&Tok> = None;
        for t in tokens {
            if t.ty == Ty::Open {
                brackets += 1;
            }
            if t.ty == Ty::Ch(b')') {
                brackets -= 1;
            }
            if brackets == 0 && t.ty == Ty::Ch(b':') {
                match prev {
                    None => return true,
                    Some(p) if p.ty == Ty::Word && self.text(p) == "progid" => continue,
                    Some(_) => return true,
                }
            }
            prev = Some(t);
        }
        false
    }

    fn scss_atrule(&mut self, token: Tok) -> O {
        let mut name = self.text(&token).to_string();
        if !self.t.scss {
            return self.atrule(token, name);
        }
        let mut prev = token;
        while !self.t.end_of_file() {
            let Some(next) = self.t.next_token()? else {
                break;
            };
            if next.ty == Ty::Word && next.p2.is_some() && next.p2 == prev.p3.map(|x| x + 1) {
                name.push_str(self.text(&next));
                prev = next;
            } else {
                self.t.back(next);
                break;
            }
        }
        let merged = Tok {
            ty: Ty::AtWord,
            start: token.start,
            end: prev.end,
            p2: token.p2,
            p3: prev.p3,
            inline: false,
        };
        self.atrule(merged, name)
    }

    fn atrule(&mut self, token: Tok, text: String) -> O {
        self.atrule_full(token, text, AtExtra::default())
    }

    fn atrule_full(&mut self, token: Tok, text: String, extra: AtExtra) -> O {
        let mut node = new_node(Kind::AtRule);
        node.name = text[1..].to_string();
        if node.name.is_empty() {
            return None;
        }
        let idx = self.push(node, token.start);
        let mut last = false;
        let mut open = false;
        let mut params: Vec<Tok> = Vec::new();
        let mut brackets: Vec<u8> = Vec::new();
        let mut end: Option<usize> = None;
        let mut closed_by_brace: Option<Tok> = None;
        while !self.t.end_of_file() {
            let Some(t) = self.t.next_token()? else {
                break;
            };
            match t.ty {
                Ty::Open | Ty::Ch(b'[') => {
                    brackets.push(if t.ty == Ty::Open { b')' } else { b']' })
                }
                Ty::Ch(b'{') if !brackets.is_empty() => brackets.push(b'}'),
                Ty::Ch(ch) if Some(&ch) == brackets.last() => {
                    brackets.pop();
                }
                _ => {}
            }
            if brackets.is_empty() {
                match t.ty {
                    Ty::Ch(b';') => {
                        end = Some(t.start + 1);
                        self.semicolon = true;
                        break;
                    }
                    Ty::Ch(b'{') => {
                        open = true;
                        break;
                    }
                    Ty::Ch(b'}') => {
                        if !params.is_empty()
                            && let Some(p) = params.iter().rev().find(|p| p.ty != Ty::Space)
                        {
                            end = Some(has_positions(p).unwrap_or(0) + 1);
                        }
                        closed_by_brace = Some(t);
                        break;
                    }
                    _ => params.push(t),
                }
            } else {
                params.push(t);
            }
            if self.t.end_of_file() {
                last = true;
                break;
            }
        }
        let self_less = self.less;
        let between = self.spaces_and_comments_from_end(&mut params);
        let mut after_name = String::new();
        let mut params_value = String::new();
        let mut params_raw = None;
        let mut between_final = between.clone();
        if !params.is_empty() {
            after_name = self.spaces_and_comments_from_start(&mut params);
            let (v, r) = self.raw(&params, false);
            params_value = v;
            params_raw = r;
            if last {
                let t = params.last()?;
                end = Some(has_positions(t).unwrap_or(0) + 1);
                self.spaces = between.clone();
                between_final.clear();
            }
        }
        {
            let cur = self.current();
            let node = &mut cur.nodes.as_mut()?[idx];
            node.raws.between = between_final;
            node.raws.after_name = after_name;
            node.params = Some(params_value);
            node.raws.params = params_raw;
            if end.is_some() {
                node.end = end;
            }
            if open {
                node.nodes = Some(Vec::new());
            }
            if self_less {
                let params = node.params.clone().unwrap_or_default();
                let named = node.name.ends_with(':');
                if named {
                    node.name = node.name.replacen(':', "", 1);
                    node.raws.after_name = format!(":{}", node.raws.after_name);
                    node.variable = true;
                    node.value = node.params.clone();
                }
                if named && let Some(rest) = params.strip_prefix(':') {
                    let ws = rest.len() - rest.trim_start_matches(js_ws).len();
                    node.value = Some(rest[ws..].to_string());
                    node.raws.after_name.push_str(&params[..1 + ws]);
                    node.variable = true;
                }
                if let Some(p) = extra.function_params {
                    node.function = true;
                    node.params = Some(p);
                }
                if let Some((identifier, important)) = extra.mixin {
                    node.mixin = true;
                    node.identifier = identifier;
                    if let Some(i) = important {
                        node.important = true;
                        node.raws.important = Some(i);
                    }
                }
            }
        }
        if let Some(t) = closed_by_brace {
            self.end(&t)?;
        }
        if open {
            self.open_last();
        }
        Some(())
    }
}

pub fn js_ws(c: char) -> bool {
    crate::comments::is_js_whitespace(c)
}
