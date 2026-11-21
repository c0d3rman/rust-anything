//! Port of @eslint-community/regexpp 4.12.3 (index.js).

pub mod data;
pub mod parser;
pub mod validator;

#[cfg(test)]
mod tests;

pub use parser::{K, Nid, RegExpAst, parse_pattern};
pub use validator::{Handler, Options, SyntaxError, Validator};

pub fn validate_pattern(
    source: &[u16],
    unicode: bool,
    unicode_sets: bool,
) -> Result<(), SyntaxError> {
    Validator::new(Options::default(), ()).validate_pattern(
        source,
        0,
        source.len(),
        Some((unicode, unicode_sets)),
    )
}

pub fn validate_flags(source: &[u16]) -> Result<(), SyntaxError> {
    Validator::new(Options::default(), ()).validate_flags(source, 0, source.len())
}
