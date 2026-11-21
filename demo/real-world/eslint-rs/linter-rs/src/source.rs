//! `SourceCode` (lib/languages/js/source-code/source-code.js) on oxc: text, lines, espree-style
//! tokens and comments, locations in UTF-16 code units.

use crate::ast::{Ast, NodeId};
use oxc_parser::Kind;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TokKind {
    Boolean,
    Identifier,
    Keyword,
    Null,
    Numeric,
    Punctuator,
    String,
    RegularExpression,
    Template,
    JSXIdentifier,
    JSXText,
    PrivateIdentifier,
    Line,
    Block,
    Shebang,
}

impl TokKind {
    pub fn as_str(self) -> &'static str {
        match self {
            TokKind::Boolean => "Boolean",
            TokKind::Identifier => "Identifier",
            TokKind::Keyword => "Keyword",
            TokKind::Null => "Null",
            TokKind::Numeric => "Numeric",
            TokKind::Punctuator => "Punctuator",
            TokKind::String => "String",
            TokKind::RegularExpression => "RegularExpression",
            TokKind::Template => "Template",
            TokKind::JSXIdentifier => "JSXIdentifier",
            TokKind::JSXText => "JSXText",
            TokKind::PrivateIdentifier => "PrivateIdentifier",
            TokKind::Line => "Line",
            TokKind::Block => "Block",
            TokKind::Shebang => "Shebang",
        }
    }

    pub fn is_comment(self) -> bool {
        matches!(self, TokKind::Line | TokKind::Block | TokKind::Shebang)
    }
}

#[derive(Debug, Clone)]
pub struct Token {
    pub kind: TokKind,
    pub start: u32,
    pub end: u32,
    pub value: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct Pos {
    pub line: u32,
    pub column: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Loc {
    pub start: Pos,
    pub end: Pos,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SourceType {
    Module,
    CommonJs,
}

pub struct ParseOptions {
    pub source_type: SourceType,
    pub jsx: bool,
    pub global_return: bool,
}

pub struct SourceCode {
    pub text: String,
    pub has_bom: bool,
    pub ast: Ast,
    pub root: NodeId,
    pub tokens: Vec<Token>,
    pub comments: Vec<Token>,
    pub line_starts: Vec<u32>,
    pub lines: Vec<String>,
    pub source_type: SourceType,
    /// `parserOptions.ecmaFeatures.globalReturn` (forced off for modules).
    pub global_return: bool,
    byte_of_u16: Vec<u32>,
    u16_of_byte: Vec<u32>,
    tokens_and_comments: Vec<Token>,
}

#[derive(Debug)]
pub enum ParseFail {
    Syntax(String),
    Unsupported(String),
}

impl SourceCode {
    pub fn parse(input: &str, opts: &ParseOptions) -> Result<SourceCode, ParseFail> {
        let (has_bom, text) = match input.strip_prefix('\u{FEFF}') {
            Some(t) => (true, t.to_string()),
            None => (false, input.to_string()),
        };
        let mut to_parse = text.clone();
        let shebang = text.starts_with("#!");
        if shebang {
            to_parse.replace_range(0..2, "//");
        }
        let mut u16_of_byte = vec![0u32; text.len() + 1];
        let mut byte_of_u16 = Vec::with_capacity(text.len() + 1);
        let mut u = 0u32;
        for (b, ch) in text.char_indices() {
            let n = ch.len_utf16() as u32;
            for i in 0..ch.len_utf8() {
                u16_of_byte[b + i] = u;
            }
            for _ in 0..n {
                byte_of_u16.push(b as u32);
            }
            u += n;
        }
        u16_of_byte[text.len()] = u;
        byte_of_u16.push(text.len() as u32);

        let alloc = oxc_allocator::Allocator::default();
        let st = match opts.source_type {
            SourceType::Module => oxc_span::SourceType::mjs(),
            SourceType::CommonJs => oxc_span::SourceType::cjs(),
        }
        .with_jsx(opts.jsx);
        let ret = oxc_parser::Parser::new(&alloc, &to_parse, st)
            .with_options(oxc_parser::ParseOptions {
                preserve_parens: false,
                allow_return_outside_function: opts.global_return
                    || opts.source_type == SourceType::CommonJs,
                ..Default::default()
            })
            .with_config(oxc_parser::config::TokensParserConfig)
            .parse();
        if ret.fatal_error
            || ret.diagnostics.has_errors()
            || ret.diagnostics.warnings().next().is_some()
        {
            let msg = ret
                .diagnostics
                .errors()
                .next()
                .map(|d| d.to_string())
                .unwrap_or_default();
            return Err(ParseFail::Syntax(msg));
        }
        if ret.program.hashbang.is_some() {
            return Err(ParseFail::Unsupported("hashbang".into()));
        }
        let semantic = oxc_semantic::SemanticBuilder::new()
            .with_check_syntax_error(true)
            .build(&ret.program);
        if let Some(e) = semantic.diagnostics.first() {
            return Err(ParseFail::Syntax(e.to_string()));
        }
        let json = ret.program.to_estree_json(false, false);
        let (mut ast, root) = match Ast::from_json(&json, &u16_of_byte) {
            Ok(r) => r,
            Err(_) => Ast::from_json(&mask_infinite_numbers(&json), &u16_of_byte)
                .map_err(|e| ParseFail::Unsupported(format!("estree json: {e}")))?,
        };
        fix_ast(&mut ast, root, &text);

        let mut comments = Vec::new();
        for c in ret.program.comments.iter() {
            let s = c.span.start as usize;
            let e = c.span.end as usize;
            let (kind, value) = if c.is_line() {
                let raw = &to_parse[s..e];
                let skip = if raw.starts_with("<!--") {
                    4
                } else if raw.starts_with("-->") {
                    3
                } else {
                    2
                };
                (TokKind::Line, &to_parse[s + skip..e])
            } else {
                (TokKind::Block, &to_parse[s + 2..e - 2])
            };
            comments.push(Token {
                kind,
                start: u16_of_byte[s],
                end: u16_of_byte[e],
                value: value.to_string(),
            });
        }
        if shebang
            && let Some(first) = comments.first_mut()
            && first.start == 0
        {
            first.kind = TokKind::Shebang;
        }

        let mut sc = SourceCode {
            text,
            has_bom,
            ast,
            root,
            tokens: Vec::new(),
            comments,
            line_starts: Vec::new(),
            lines: Vec::new(),
            source_type: opts.source_type,
            global_return: opts.global_return && opts.source_type != SourceType::Module,
            byte_of_u16,
            u16_of_byte,
            tokens_and_comments: Vec::new(),
        };
        sc.tokens = sc.convert_tokens(&ret.tokens)?;
        for (i, n) in sc.ast.nodes.iter().enumerate() {
            if matches!(
                n.kind,
                "ImportDeclaration" | "ExportNamedDeclaration" | "ExportAllDeclaration"
            ) && !sc.ast.list(i as NodeId, "attributes").is_empty()
                && let Some(src) = sc.ast.child(i as NodeId, "source")
            {
                let end = sc.ast.range(src).1;
                if let Some(t) = sc.tokens.iter().find(|t| t.start >= end)
                    && t.value == "assert"
                {
                    return Err(ParseFail::Syntax("import assertions".into()));
                }
            }
        }
        sc.compute_lines();
        fix_program_range(&mut sc);
        let mut all: Vec<Token> = sc
            .tokens
            .iter()
            .chain(sc.comments.iter())
            .cloned()
            .collect();
        all.sort_by_key(|t| t.start);
        sc.tokens_and_comments = all;
        Ok(sc)
    }

    fn convert_tokens(&self, toks: &[oxc_parser::Token]) -> Result<Vec<Token>, ParseFail> {
        let mut jsx_ident_ranges = Vec::new();
        let mut jsx_attr_strings = Vec::new();
        let mut ident_starts = Vec::new();
        let mut jsx_texts: Vec<(u32, &str)> = Vec::new();
        for (i, n) in self.ast.nodes.iter().enumerate() {
            match n.kind {
                "JSXIdentifier" => jsx_ident_ranges.push((n.start, n.end)),
                "JSXText" => {
                    if let Some(v) = self.ast.str(i as NodeId, "value") {
                        if v.contains('&') {
                            return Err(ParseFail::Unsupported("JSX entity".into()));
                        }
                        jsx_texts.push((n.start, v));
                    }
                }
                "Identifier" => {
                    let is_meta = n.parent.is_some_and(|p| {
                        self.ast.is(p, "MetaProperty")
                            && self.ast.child(p, "meta") == Some(i as NodeId)
                    });
                    if !is_meta {
                        ident_starts.push(n.start);
                    }
                }
                "Literal" => {
                    if let Some(p) = n.parent
                        && self.ast.is(p, "JSXAttribute")
                        && self.ast.child(p, "value") == Some(i as NodeId)
                    {
                        if self
                            .ast
                            .str(i as NodeId, "raw")
                            .is_some_and(|r| r.contains('&'))
                        {
                            return Err(ParseFail::Unsupported("JSX entity".into()));
                        }
                        jsx_attr_strings.push(n.start);
                    }
                }
                _ => {}
            }
        }
        jsx_ident_ranges.sort_unstable();
        ident_starts.sort_unstable();
        let mut out = Vec::with_capacity(toks.len());
        let mut iter = toks.iter().peekable();
        while let Some(t) = iter.next() {
            let s = t.start() as usize;
            let e = t.end() as usize;
            let start = self.u16_of_byte[s];
            let mut end = self.u16_of_byte[e];
            let raw = &self.text[s..e];
            let kind = t.kind();
            let tk = match kind {
                Kind::Eof | Kind::Skip | Kind::HashbangComment => continue,
                Kind::Undetermined => {
                    return Err(ParseFail::Unsupported("undetermined token".into()));
                }
                Kind::Decimal
                | Kind::Float
                | Kind::PositiveExponential
                | Kind::NegativeExponential
                | Kind::Binary
                | Kind::Octal
                | Kind::Hex
                | Kind::DecimalBigInt
                | Kind::BinaryBigInt
                | Kind::OctalBigInt
                | Kind::HexBigInt => TokKind::Numeric,
                Kind::Str => {
                    if jsx_attr_strings.binary_search(&start).is_ok()
                        || jsx_attr_strings.contains(&start)
                    {
                        TokKind::JSXText
                    } else {
                        TokKind::String
                    }
                }
                Kind::RegExp => TokKind::RegularExpression,
                Kind::NoSubstitutionTemplate | Kind::TemplateTail => TokKind::Template,
                Kind::TemplateHead | Kind::TemplateMiddle => TokKind::Template,
                Kind::PrivateIdentifier => TokKind::PrivateIdentifier,
                Kind::JSXText => TokKind::JSXText,
                Kind::At => return Err(ParseFail::Unsupported("decorator".into())),
                k if k.is_identifier_name() => {
                    if jsx_ident_ranges
                        .binary_search_by(|r| r.0.cmp(&start))
                        .is_ok()
                    {
                        TokKind::JSXIdentifier
                    } else if raw.contains('\\') {
                        TokKind::Identifier
                    } else {
                        match word_kind(raw) {
                            TokKind::Identifier => TokKind::Identifier,
                            _ if matches!(raw, "let" | "static" | "yield") => TokKind::Keyword,
                            _ if ident_starts.binary_search(&start).is_ok() => TokKind::Identifier,
                            k => k,
                        }
                    }
                }
                _ => TokKind::Punctuator,
            };
            if tk == TokKind::JSXIdentifier {
                // `<a-b>`: a JSX name may span several lexer tokens.
                if let Ok(i) = jsx_ident_ranges.binary_search_by(|r| r.0.cmp(&start)) {
                    let target = jsx_ident_ranges[i].1;
                    while end < target {
                        match iter.next() {
                            Some(n) => end = self.u16_of_byte[n.end() as usize],
                            None => break,
                        }
                    }
                }
            }
            let text_slice = self.slice16(start, end);
            let value = match tk {
                TokKind::Identifier if raw.contains('\\') => decode_escapes(raw),
                TokKind::PrivateIdentifier => text_slice[1..].to_string(),
                TokKind::JSXText if kind == Kind::JSXText => {
                    match jsx_texts.iter().find(|(s, _)| *s == start) {
                        Some((_, v)) => v.to_string(),
                        None => text_slice.to_string(),
                    }
                }
                _ => text_slice.to_string(),
            };
            out.push(Token {
                kind: tk,
                start,
                end,
                value,
            });
        }
        Ok(out)
    }

    fn compute_lines(&mut self) {
        let mut starts = vec![0u32];
        let mut lines = Vec::new();
        let bytes = self.text.as_bytes();
        let mut line_start_b = 0usize;
        let mut i = 0usize;
        while i < bytes.len() {
            let c = bytes[i];
            let brk = if c == b'\r' {
                if bytes.get(i + 1) == Some(&b'\n') {
                    2
                } else {
                    1
                }
            } else if c == b'\n' {
                1
            } else if c == 0xE2
                && bytes.get(i + 1) == Some(&0x80)
                && matches!(bytes.get(i + 2), Some(0xA8) | Some(0xA9))
            {
                3
            } else {
                0
            };
            if brk > 0 {
                lines.push(self.text[line_start_b..i].to_string());
                i += brk;
                line_start_b = i;
                starts.push(self.u16_of_byte[i]);
            } else {
                i += 1;
            }
        }
        lines.push(self.text[line_start_b..].to_string());
        self.line_starts = starts;
        self.lines = lines;
    }

    pub fn len16(&self) -> u32 {
        self.u16_of_byte[self.text.len()]
    }

    pub fn byte(&self, u16_index: u32) -> usize {
        self.byte_of_u16
            .get(u16_index as usize)
            .copied()
            .unwrap_or(self.text.len() as u32) as usize
    }

    pub fn u16_of(&self, byte: usize) -> u32 {
        self.u16_of_byte[byte]
    }

    pub fn slice16(&self, start: u32, end: u32) -> &str {
        &self.text[self.byte(start)..self.byte(end)]
    }

    pub fn node_text(&self, id: NodeId) -> &str {
        let (s, e) = self.ast.range(id);
        self.slice16(s, e)
    }

    pub fn loc_of(&self, index: u32) -> Pos {
        let line = match self.line_starts.binary_search(&index) {
            Ok(i) => i,
            Err(i) => i - 1,
        };
        Pos {
            line: line as u32 + 1,
            column: index - self.line_starts[line],
        }
    }

    pub fn index_of(&self, pos: Pos) -> u32 {
        self.line_starts[(pos.line - 1) as usize] + pos.column
    }

    pub fn node_loc(&self, id: NodeId) -> Loc {
        let (s, e) = self.ast.range(id);
        Loc {
            start: self.loc_of(s),
            end: self.loc_of(e),
        }
    }

    pub fn tok_loc(&self, t: &Token) -> Loc {
        Loc {
            start: self.loc_of(t.start),
            end: self.loc_of(t.end),
        }
    }

    pub fn tokens_and_comments(&self) -> &[Token] {
        &self.tokens_and_comments
    }
}

const INFINITE: &str = "\u{0}infinite";

/// Number literals beyond f64 (`1e400`) come out of oxc as `1e+400`, which serde_json rejects:
/// written as a marker string here, read back as infinity in `fix_ast`.
fn mask_infinite_numbers(json: &str) -> String {
    let b = json.as_bytes();
    let mut out = String::with_capacity(json.len());
    let mut i = 0;
    let mut last = 0;
    let mut in_str = false;
    while i < b.len() {
        let c = b[i];
        if in_str {
            if c == b'\\' {
                i += 2;
                continue;
            }
            if c == b'"' {
                in_str = false;
            }
            i += 1;
            continue;
        }
        if c == b'"' {
            in_str = true;
            i += 1;
            continue;
        }
        if c.is_ascii_digit() || c == b'-' {
            let st = i;
            while i < b.len()
                && (b[i].is_ascii_digit() || matches!(b[i], b'-' | b'+' | b'.' | b'e' | b'E'))
            {
                i += 1;
            }
            if json[st..i].parse::<f64>().is_ok_and(f64::is_infinite) {
                out.push_str(&json[last..st]);
                out.push('"');
                out.push_str("\\u0000infinite");
                out.push('"');
                last = i;
            }
            continue;
        }
        i += 1;
    }
    out.push_str(&json[last..]);
    out
}

fn word_kind(w: &str) -> TokKind {
    match w {
        "true" | "false" => TokKind::Boolean,
        "null" => TokKind::Null,
        "break" | "case" | "catch" | "continue" | "debugger" | "default" | "do" | "else"
        | "finally" | "for" | "function" | "if" | "return" | "switch" | "throw" | "try" | "var"
        | "while" | "with" | "instanceof" | "typeof" | "void" | "delete" | "new" | "in"
        | "this" | "const" | "class" | "extends" | "export" | "import" | "super" | "static"
        | "yield" | "let" => TokKind::Keyword,
        _ => TokKind::Identifier,
    }
}

fn decode_escapes(raw: &str) -> String {
    let mut out = String::new();
    let mut chars = raw.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '\\' && chars.peek() == Some(&'u') {
            chars.next();
            let hex: String = if chars.peek() == Some(&'{') {
                chars.next();
                let mut h = String::new();
                for c in chars.by_ref() {
                    if c == '}' {
                        break;
                    }
                    h.push(c);
                }
                h
            } else {
                (0..4).filter_map(|_| chars.next()).collect()
            };
            if let Some(ch) = u32::from_str_radix(&hex, 16).ok().and_then(char::from_u32) {
                out.push(ch);
            }
        } else {
            out.push(c);
        }
    }
    out
}

fn fix_ast(ast: &mut Ast, _root: NodeId, _text: &str) {
    for n in ast.nodes.iter_mut() {
        for (_, p) in n.props.iter_mut() {
            if matches!(p, crate::ast::Prop::Str(v) if v == INFINITE) {
                *p = crate::ast::Prop::Num(f64::INFINITY);
            }
        }
        if n.kind == "TemplateElement" {
            let tail = n
                .props
                .iter()
                .any(|(k, p)| *k == "tail" && matches!(p, crate::ast::Prop::Bool(true)));
            n.start -= 1;
            n.end += if tail { 1 } else { 2 };
        }
    }
}

fn fix_program_range(sc: &mut SourceCode) {
    let _ = sc;
}
