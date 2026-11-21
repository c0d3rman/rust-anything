//! Text scanning on the original source: `src/utils/skip*.js`, `has-newline*.js`,
//! `is-next-line-empty.js`, `get-next-non-space-non-comment-character-index.js`. Offsets are
//! byte offsets.

/// JavaScript's line terminators, the characters a regular expression's `.` does not match.
pub const LINE_TERMINATORS: [char; 4] = ['\n', '\r', '\u{2028}', '\u{2029}'];

fn char_at(text: &str, i: usize) -> Option<char> {
    text.get(i..).and_then(|t| t.chars().next())
}

fn char_before(text: &str, i: usize) -> Option<(usize, char)> {
    text.get(..i)?.char_indices().next_back()
}

fn skip(
    text: &str,
    start: Option<usize>,
    backwards: bool,
    keep: &dyn Fn(char) -> bool,
) -> Option<usize> {
    let mut cursor = start?;
    if backwards {
        loop {
            if cursor == usize::MAX {
                return Some(usize::MAX);
            }
            match char_at(text, cursor) {
                Some(c) if keep(c) => match char_before(text, cursor) {
                    Some((p, _)) => cursor = p,
                    None => return Some(usize::MAX),
                },
                Some(_) => return Some(cursor),
                None => {
                    if cursor >= text.len() {
                        return if cursor == text.len() {
                            Some(cursor)
                        } else {
                            None
                        };
                    }
                    return Some(cursor);
                }
            }
        }
    }
    loop {
        match char_at(text, cursor) {
            Some(c) if keep(c) => cursor += c.len_utf8(),
            Some(_) => return Some(cursor),
            None => {
                return if cursor == text.len() {
                    Some(cursor)
                } else {
                    None
                };
            }
        }
    }
}

pub fn skip_spaces(text: &str, start: Option<usize>, backwards: bool) -> Option<usize> {
    skip(text, start, backwards, &|c| c == ' ' || c == '\t')
}

pub fn skip_whitespace(text: &str, start: Option<usize>, backwards: bool) -> Option<usize> {
    skip(text, start, backwards, &|c| {
        c.is_whitespace() || c == '\u{FEFF}'
    })
}

pub fn skip_to_line_end(text: &str, start: Option<usize>) -> Option<usize> {
    skip(text, start, false, &|c| matches!(c, ',' | ';' | ' ' | '\t'))
}

pub fn skip_everything_but_newline(
    text: &str,
    start: Option<usize>,
    backwards: bool,
) -> Option<usize> {
    skip(text, start, backwards, &|c| c != '\n' && c != '\r')
}

pub fn skip_inline_comment(text: &str, start: Option<usize>) -> Option<usize> {
    let start = start?;
    if text.get(start..).is_some_and(|t| t.starts_with("/*"))
        && let Some(end) = text[start + 2..].find("*/")
    {
        return Some(start + 2 + end + 2);
    }
    Some(start)
}

pub fn skip_trailing_comment(text: &str, start: Option<usize>) -> Option<usize> {
    let start = start?;
    if text.get(start..).is_some_and(|t| t.starts_with("//")) {
        return skip_everything_but_newline(text, Some(start), false);
    }
    Some(start)
}

pub fn skip_newline(text: &str, start: Option<usize>, backwards: bool) -> Option<usize> {
    let start = start?;
    if backwards {
        if start == usize::MAX {
            return Some(start);
        }
        let c = char_at(text, start);
        if c == Some('\n') && start > 0 && text.as_bytes()[start - 1] == b'\r' {
            return Some(start.wrapping_sub(2));
        }
        if matches!(c, Some('\n' | '\r' | '\u{2028}' | '\u{2029}')) {
            return Some(match char_before(text, start) {
                Some((p, _)) => p,
                None => usize::MAX,
            });
        }
        return Some(start);
    }
    match char_at(text, start) {
        Some('\r') if text.as_bytes().get(start + 1) == Some(&b'\n') => Some(start + 2),
        Some(c @ ('\n' | '\r' | '\u{2028}' | '\u{2029}')) => Some(start + c.len_utf8()),
        _ => Some(start),
    }
}

pub fn has_newline(text: &str, start: usize, backwards: bool) -> bool {
    let from = if backwards {
        match char_before(text, start) {
            Some((p, _)) => p,
            None => return false,
        }
    } else {
        start
    };
    let idx = skip_spaces(text, Some(from), backwards);
    let idx2 = skip_newline(text, idx, backwards);
    idx != idx2
}

pub fn has_newline_in_range(text: &str, start: usize, end: usize) -> bool {
    text.get(start..end.min(text.len()))
        .is_some_and(|t| t.contains('\n'))
}

pub fn is_next_line_empty(text: &str, start: usize) -> bool {
    let mut old = None;
    let mut idx = Some(start);
    while idx != old {
        old = idx;
        idx = skip_to_line_end(text, idx);
        idx = skip_inline_comment(text, idx);
        idx = skip_spaces(text, idx, false);
    }
    idx = skip_trailing_comment(text, idx);
    idx = skip_newline(text, idx, false);
    match idx {
        Some(i) => has_newline(text, i, false),
        None => false,
    }
}

pub fn next_non_space_non_comment_index(text: &str, start: usize) -> Option<usize> {
    let mut old = None;
    let mut next = Some(start);
    while next != old {
        old = next;
        next = skip_spaces(text, next, false);
        next = skip_inline_comment(text, next);
        next = skip_trailing_comment(text, next);
        next = skip_newline(text, next, false);
    }
    next
}

pub fn next_non_space_non_comment_char(text: &str, start: usize) -> Option<char> {
    next_non_space_non_comment_index(text, start).and_then(|i| char_at(text, i))
}
