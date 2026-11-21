//! A backtracking matcher for the subset of `new RegExp(pattern, "u")` that rule options use.

use crate::utils::is_js_whitespace;

enum Node {
    Char(char),
    Any,
    Class(Vec<Item>, bool),
    Start,
    End,
    Group(Vec<Vec<Node>>),
    Repeat(Box<Node>, usize, usize),
}

#[derive(Clone, Copy)]
enum Item {
    Range(char, char),
    Digit(bool),
    Word(bool),
    Space(bool),
}

impl Item {
    fn matches(self, c: char) -> bool {
        match self {
            Item::Range(a, b) => a <= c && c <= b,
            Item::Digit(neg) => c.is_ascii_digit() != neg,
            Item::Word(neg) => (c.is_ascii_alphanumeric() || c == '_') != neg,
            Item::Space(neg) => is_js_whitespace(c) != neg,
        }
    }
}

pub struct Regex {
    alt: Vec<Vec<Node>>,
}

struct Parser {
    s: Vec<char>,
    i: usize,
}

impl Parser {
    fn peek(&self) -> Option<char> {
        self.s.get(self.i).copied()
    }

    fn alternation(&mut self) -> Option<Vec<Vec<Node>>> {
        let mut alt = vec![self.sequence()?];
        while self.peek() == Some('|') {
            self.i += 1;
            alt.push(self.sequence()?);
        }
        Some(alt)
    }

    fn sequence(&mut self) -> Option<Vec<Node>> {
        let mut seq = Vec::new();
        while let Some(c) = self.peek() {
            if c == '|' || c == ')' {
                break;
            }
            self.i += 1;
            let atom = match c {
                '^' => {
                    seq.push(Node::Start);
                    continue;
                }
                '$' => {
                    seq.push(Node::End);
                    continue;
                }
                '.' => Node::Any,
                '(' => {
                    if self.peek() == Some('?') {
                        if self.s.get(self.i + 1) != Some(&':') {
                            return None;
                        }
                        self.i += 2;
                    }
                    let alt = self.alternation()?;
                    if self.peek() != Some(')') {
                        return None;
                    }
                    self.i += 1;
                    Node::Group(alt)
                }
                '[' => self.class()?,
                '\\' => match self.escape()? {
                    Item::Range(a, _) => Node::Char(a),
                    item => Node::Class(vec![item], false),
                },
                '*' | '+' | '?' | '{' | '}' | ']' => return None,
                c => Node::Char(c),
            };
            let (min, max) = match self.peek() {
                Some('*') => (0, usize::MAX),
                Some('+') => (1, usize::MAX),
                Some('?') => (0, 1),
                Some('{') => return None,
                _ => {
                    seq.push(atom);
                    continue;
                }
            };
            self.i += 1;
            if self.peek() == Some('?') {
                self.i += 1;
            }
            seq.push(Node::Repeat(Box::new(atom), min, max));
        }
        Some(seq)
    }

    fn escape(&mut self) -> Option<Item> {
        let c = self.peek()?;
        self.i += 1;
        Some(match c {
            'd' => Item::Digit(false),
            'D' => Item::Digit(true),
            'w' => Item::Word(false),
            'W' => Item::Word(true),
            's' => Item::Space(false),
            'S' => Item::Space(true),
            't' => Item::Range('\t', '\t'),
            'n' => Item::Range('\n', '\n'),
            'r' => Item::Range('\r', '\r'),
            'f' => Item::Range('\u{c}', '\u{c}'),
            'v' => Item::Range('\u{b}', '\u{b}'),
            '^' | '$' | '\\' | '.' | '*' | '+' | '?' | '(' | ')' | '[' | ']' | '{' | '}' | '|'
            | '/' => Item::Range(c, c),
            _ => return None,
        })
    }

    fn class(&mut self) -> Option<Node> {
        let negated = self.peek() == Some('^');
        if negated {
            self.i += 1;
        }
        let mut items = Vec::new();
        loop {
            let c = self.peek()?;
            self.i += 1;
            if c == ']' {
                break;
            }
            let item = if c == '\\' {
                if self.peek() == Some('-') {
                    self.i += 1;
                    Item::Range('-', '-')
                } else {
                    self.escape()?
                }
            } else {
                Item::Range(c, c)
            };
            if self.peek() == Some('-') && self.s.get(self.i + 1).is_some_and(|n| *n != ']') {
                let Item::Range(lo, _) = item else {
                    return None;
                };
                self.i += 1;
                let hc = self.peek()?;
                self.i += 1;
                let hi = if hc == '\\' {
                    match self.escape()? {
                        Item::Range(h, _) => h,
                        _ => return None,
                    }
                } else {
                    hc
                };
                if lo > hi {
                    return None;
                }
                items.push(Item::Range(lo, hi));
            } else {
                items.push(item);
            }
        }
        Some(Node::Class(items, negated))
    }
}

fn is_line_terminator(c: char) -> bool {
    matches!(c, '\n' | '\r' | '\u{2028}' | '\u{2029}')
}

impl Regex {
    /// `None` when the pattern uses syntax this matcher does not implement (or is invalid).
    pub fn new(pattern: &str) -> Option<Regex> {
        let mut p = Parser {
            s: pattern.chars().collect(),
            i: 0,
        };
        let alt = p.alternation()?;
        if p.i != p.s.len() {
            return None;
        }
        Some(Regex { alt })
    }

    /// `regex.test(input)`.
    pub fn test(&self, input: &str) -> bool {
        let s: Vec<char> = input.chars().collect();
        (0..=s.len()).any(|start| alt_at(&self.alt, &s, start, &|_| true))
    }
}

fn alt_at(alt: &[Vec<Node>], s: &[char], pos: usize, k: &dyn Fn(usize) -> bool) -> bool {
    alt.iter().any(|seq| seq_at(seq, s, pos, k))
}

fn seq_at(seq: &[Node], s: &[char], pos: usize, k: &dyn Fn(usize) -> bool) -> bool {
    let Some((first, rest)) = seq.split_first() else {
        return k(pos);
    };
    let next = |p: usize| seq_at(rest, s, p, k);
    match first {
        Node::Repeat(n, min, max) => rep_at(n, *min, *max, 0, s, pos, &next),
        n => node_at(n, s, pos, &next),
    }
}

fn rep_at(
    n: &Node,
    min: usize,
    max: usize,
    count: usize,
    s: &[char],
    pos: usize,
    k: &dyn Fn(usize) -> bool,
) -> bool {
    if count < min {
        return node_at(n, s, pos, &|p| rep_at(n, min, max, count + 1, s, p, k));
    }
    let more = || {
        count < max
            && node_at(n, s, pos, &|p| {
                p != pos && rep_at(n, min, max, count + 1, s, p, k)
            })
    };
    more() || k(pos)
}

fn node_at(n: &Node, s: &[char], pos: usize, k: &dyn Fn(usize) -> bool) -> bool {
    let one = |ok: bool| ok && k(pos + 1);
    match n {
        Node::Char(c) => one(s.get(pos) == Some(c)),
        Node::Any => one(s.get(pos).is_some_and(|c| !is_line_terminator(*c))),
        Node::Class(items, neg) => one(s
            .get(pos)
            .is_some_and(|c| items.iter().any(|i| i.matches(*c)) != *neg)),
        Node::Start => pos == 0 && k(pos),
        Node::End => pos == s.len() && k(pos),
        Node::Group(alt) => alt_at(alt, s, pos, k),
        Node::Repeat(inner, min, max) => rep_at(inner, *min, *max, 0, s, pos, k),
    }
}

#[cfg(test)]
mod tests {
    use super::Regex;

    #[test]
    fn subset() {
        let r = Regex::new("^[a-z]+(_[a-z]+)+$").unwrap();
        assert!(r.test("foo_bar"));
        assert!(!r.test("fooBar"));
        assert!(!r.test("foo_"));
        assert!(Regex::new(".?").unwrap().test(""));
        assert!(Regex::new("default").unwrap().test("no default here"));
        assert!(!Regex::new("^skip default").unwrap().test("do skip default"));
        assert!(Regex::new("a|b").unwrap().test("xb"));
        assert!(Regex::new("a{2}").is_none());
        assert!(Regex::new("(?=a)").is_none());
        assert!(Regex::new(r"\bx").is_none());
    }
}
