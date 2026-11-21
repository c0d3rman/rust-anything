//! The token store (lib/languages/js/source-code/token-store) as plain searches over the
//! sorted token and comment lists.

use crate::ast::NodeId;
use crate::source::{SourceCode, Token};

#[derive(Clone, Copy, Default)]
pub struct Opts {
    pub include_comments: bool,
    pub skip: usize,
}

pub const C: Opts = Opts {
    include_comments: true,
    skip: 0,
};
pub const N: Opts = Opts {
    include_comments: false,
    skip: 0,
};

impl SourceCode {
    fn list(&self, o: Opts) -> &[Token] {
        if o.include_comments {
            self.tokens_and_comments()
        } else {
            &self.tokens
        }
    }

    /// First index whose token starts at or after `offset`.
    fn first_from(&self, list: &[Token], offset: u32) -> usize {
        list.partition_point(|t| t.start < offset)
    }

    pub fn token_after_offset(
        &self,
        offset: u32,
        o: Opts,
        f: impl Fn(&Token) -> bool,
    ) -> Option<&Token> {
        let list = self.list(o);
        let i = self.first_from(list, offset);
        list[i..].iter().filter(|t| f(t)).nth(o.skip)
    }

    pub fn token_before_offset(
        &self,
        offset: u32,
        o: Opts,
        f: impl Fn(&Token) -> bool,
    ) -> Option<&Token> {
        let list = self.list(o);
        let i = list.partition_point(|t| t.end <= offset);
        list[..i].iter().rev().filter(|t| f(t)).nth(o.skip)
    }

    pub fn token_after(&self, node: NodeId, o: Opts) -> Option<&Token> {
        self.token_after_offset(self.ast.range(node).1, o, |_| true)
    }

    pub fn token_after_f(&self, node: NodeId, f: impl Fn(&Token) -> bool) -> Option<&Token> {
        self.token_after_offset(self.ast.range(node).1, N, f)
    }

    pub fn token_after_tok(&self, t: &Token, o: Opts) -> Option<&Token> {
        self.token_after_offset(t.end, o, |_| true)
    }

    pub fn token_before(&self, node: NodeId, o: Opts) -> Option<&Token> {
        self.token_before_offset(self.ast.range(node).0, o, |_| true)
    }

    pub fn token_before_f(&self, node: NodeId, f: impl Fn(&Token) -> bool) -> Option<&Token> {
        self.token_before_offset(self.ast.range(node).0, N, f)
    }

    pub fn token_before_tok(&self, t: &Token, o: Opts) -> Option<&Token> {
        self.token_before_offset(t.start, o, |_| true)
    }

    pub fn tokens_in(&self, start: u32, end: u32, o: Opts) -> &[Token] {
        let list = self.list(o);
        let i = self.first_from(list, start);
        let j = list.partition_point(|t| t.end <= end).max(i);
        &list[i..j]
    }

    pub fn first_token(&self, node: NodeId, o: Opts) -> Option<&Token> {
        let (s, e) = self.ast.range(node);
        self.tokens_in(s, e, o).get(o.skip)
    }

    pub fn first_token_f(&self, node: NodeId, f: impl Fn(&Token) -> bool) -> Option<&Token> {
        let (s, e) = self.ast.range(node);
        self.tokens_in(s, e, N).iter().find(|t| f(t))
    }

    pub fn last_token(&self, node: NodeId, o: Opts) -> Option<&Token> {
        let (s, e) = self.ast.range(node);
        self.tokens_in(s, e, o).iter().rev().nth(o.skip)
    }

    pub fn last_token_f(&self, node: NodeId, f: impl Fn(&Token) -> bool) -> Option<&Token> {
        let (s, e) = self.ast.range(node);
        self.tokens_in(s, e, N).iter().rev().find(|t| f(t))
    }

    pub fn tokens_of(&self, node: NodeId, o: Opts) -> &[Token] {
        let (s, e) = self.ast.range(node);
        self.tokens_in(s, e, o)
    }

    pub fn tokens_between(&self, left_end: u32, right_start: u32, o: Opts) -> &[Token] {
        self.tokens_in(left_end, right_start, o)
    }

    pub fn first_token_between(
        &self,
        a: NodeId,
        b: NodeId,
        f: impl Fn(&Token) -> bool,
    ) -> Option<&Token> {
        self.tokens_between(self.ast.range(a).1, self.ast.range(b).0, N)
            .iter()
            .find(|t| f(t))
    }

    pub fn comments_in(&self, start: u32, end: u32) -> impl Iterator<Item = &Token> {
        let i = self.comments.partition_point(|c| c.start < start);
        self.comments[i..].iter().take_while(move |c| c.end <= end)
    }

    pub fn comments_inside(&self, node: NodeId) -> Vec<&Token> {
        let (s, e) = self.ast.range(node);
        self.comments_in(s, e).collect()
    }

    pub fn comments_exist_between(&self, left_end: u32, right_start: u32) -> bool {
        self.comments_in(left_end, right_start).next().is_some()
    }

    /// `getCommentsBefore(nodeOrToken)`: the comments directly before, with nothing else between.
    pub fn comments_before(&self, start: u32) -> Vec<&Token> {
        let list = self.tokens_and_comments();
        let mut i = list.partition_point(|t| t.end <= start);
        let mut out = Vec::new();
        while i > 0 && list[i - 1].kind.is_comment() {
            out.push(&list[i - 1]);
            i -= 1;
        }
        out.reverse();
        out
    }

    pub fn comments_after(&self, end: u32) -> Vec<&Token> {
        let list = self.tokens_and_comments();
        let mut i = list.partition_point(|t| t.start < end);
        let mut out = Vec::new();
        while i < list.len() && list[i].kind.is_comment() {
            out.push(&list[i]);
            i += 1;
        }
        out
    }

    pub fn text_of_range(&self, r: (u32, u32)) -> &str {
        self.slice16(r.0, r.1)
    }
}

pub fn is_punct(t: &Token, v: &str) -> bool {
    t.kind == crate::source::TokKind::Punctuator && t.value == v
}
