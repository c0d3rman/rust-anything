//! UTF-16 offsets. Prettier and postcss index JavaScript strings, so every offset the CSS port
//! stores on a node, adds a length to, or compares is a UTF-16 code-unit index, as in Prettier.
//! The parsers below work in bytes; their positions are converted here, once, when a node is
//! built, and converted back only where the original text is read (`Idx::slice`, `Idx::byte`).

/// Byte <-> UTF-16 index for one string. Only non-ASCII characters are recorded.
pub struct Idx {
    /// Byte start, UTF-16 start, byte length and UTF-16 length of each non-ASCII character.
    wide: Vec<(usize, usize, usize, usize)>,
    len_b: usize,
    len_u: usize,
}

pub fn len16(s: &str) -> i64 {
    if s.is_ascii() {
        s.len() as i64
    } else {
        s.encode_utf16().count() as i64
    }
}

impl Idx {
    pub fn new(s: &str) -> Idx {
        let mut wide = Vec::new();
        let mut u = 0;
        if !s.is_ascii() {
            for (b, c) in s.char_indices() {
                if !c.is_ascii() {
                    wide.push((b, u, c.len_utf8(), c.len_utf16()));
                }
                u += c.len_utf16();
            }
        } else {
            u = s.len();
        }
        Idx {
            wide,
            len_b: s.len(),
            len_u: u,
        }
    }

    /// The UTF-16 index of byte offset `b`. A byte inside a character counts that character.
    pub fn b2u(&self, b: usize) -> i64 {
        let k = self.wide.partition_point(|w| w.0 < b);
        if k == 0 {
            return b as i64;
        }
        let (bs, us, bl, ul) = self.wide[k - 1];
        if b < bs + bl {
            return (us + ul) as i64;
        }
        (us + ul + (b - bs - bl)) as i64
    }

    /// The UTF-16 index of a tokenizer position: a character boundary, or the last byte of a
    /// character for an inclusive end, which JavaScript gives as that character's last unit.
    pub fn pos(&self, b: usize) -> i64 {
        let k = self.wide.partition_point(|w| w.0 < b);
        if k > 0 {
            let (bs, us, bl, ul) = self.wide[k - 1];
            if b < bs + bl {
                return (us + ul - 1) as i64;
            }
        }
        self.b2u(b)
    }

    /// The byte offset of UTF-16 index `u` (clamped to the string), or `None` when `u` falls
    /// between the two halves of a surrogate pair, which only JavaScript can split.
    pub fn byte(&self, u: i64) -> Option<usize> {
        let u = u.clamp(0, self.len_u as i64) as usize;
        let k = self.wide.partition_point(|w| w.1 <= u);
        if k == 0 {
            return Some(u);
        }
        let (bs, us, bl, ul) = self.wide[k - 1];
        if u == us {
            return Some(bs);
        }
        if u < us + ul {
            return None;
        }
        Some((bs + bl + (u - us - ul)).min(self.len_b))
    }

    /// `text.slice(a, b)` in JavaScript, for the string this index was built from.
    pub fn slice<'t>(&self, text: &'t str, a: i64, b: Option<i64>) -> Option<&'t str> {
        let len = self.len_u as i64;
        let a = a.clamp(0, len);
        let b = b.unwrap_or(len).clamp(0, len);
        if b <= a {
            return Some("");
        }
        text.get(self.byte(a)?..self.byte(b)?)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trip() {
        let s = "a→b🐶c";
        let i = Idx::new(s);
        assert_eq!(len16(s), 6);
        assert_eq!(i.b2u(1), 1);
        assert_eq!(i.b2u(4), 2);
        assert_eq!(i.b2u(5), 3);
        assert_eq!(i.b2u(9), 5);
        assert_eq!(i.b2u(10), 6);
        assert_eq!(i.byte(2), Some(4));
        assert_eq!(i.byte(4), None);
        assert_eq!(i.byte(5), Some(9));
        assert_eq!(i.slice(s, 1, Some(3)), Some("→b"));
        assert_eq!(i.slice(s, 3, Some(4)), None);
    }
}
