use crate::ast::NodeId;
use crate::linter::Cx;
use crate::rules::RuleDef;
use serde_json::Value;

macro_rules! rules {
    ($($m:ident),* $(,)?) => {
        $(pub mod $m;)*
        pub static ALL: &[&RuleDef] = &[$(&$m::DEF),*];
    };
}

rules!(
    no_octal,
    no_self_assign,
    no_setter_return,
    no_sparse_arrays,
    no_unexpected_multiline,
    no_unneeded_ternary,
    no_unsafe_finally,
    no_unsafe_negation,
    no_unsafe_optional_chaining,
    no_unused_labels,
    no_unused_private_class_members,
    no_useless_catch,
    no_useless_constructor,
    no_with,
    prefer_template,
    require_yield,
    yoda,
);

/// `schema: []`.
fn no_options(options: &[Value]) -> Result<(), String> {
    if options.is_empty() {
        Ok(())
    } else {
        Err("invalid option".into())
    }
}

/// An object of boolean properties with `additionalProperties: false`, at `options[index]`.
fn bool_props(
    options: &[Value],
    index: usize,
    names: &[&str],
) -> Result<Vec<Option<bool>>, String> {
    let mut out = vec![None; names.len()];
    match options.get(index) {
        None => {}
        Some(Value::Object(o)) => {
            for (k, v) in o {
                let Some(i) = names.iter().position(|n| n == k) else {
                    return Err("invalid option".into());
                };
                let Value::Bool(b) = v else {
                    return Err("invalid option".into());
                };
                out[i] = Some(*b);
            }
        }
        Some(_) => return Err("invalid option".into()),
    }
    Ok(out)
}

/// `sourceCode.isGlobalReference(node)` for an identifier naming a builtin global, when it can be
/// decided without scope analysis: `None` when it cannot.
fn builtin_global_reference(cx: &Cx, ident: NodeId) -> Option<bool> {
    Some(cx.sm().is_global_reference(&cx.sc.ast, ident))
}

/// `\s` of a JavaScript regular expression.
fn is_js_whitespace(c: char) -> bool {
    matches!(
        c,
        '\t' | '\n' | '\u{b}' | '\u{c}' | '\r' | ' ' | '\u{a0}' | '\u{1680}' | '\u{2000}'
            ..='\u{200a}'
                | '\u{2028}'
                | '\u{2029}'
                | '\u{202f}'
                | '\u{205f}'
                | '\u{3000}'
                | '\u{feff}'
    )
}

#[cfg(test)]
mod tests {
    #[test]
    fn js_whitespace() {
        assert!(super::is_js_whitespace('\u{feff}'));
        assert!(!super::is_js_whitespace('\u{85}'));
        assert!(!super::is_js_whitespace('\u{180e}'));
    }
}
