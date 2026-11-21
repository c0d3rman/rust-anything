use crate::rules::RuleDef;

macro_rules! rules {
    ($($m:ident),* $(,)?) => {
        $(pub mod $m;)*
        pub static ALL: &[&RuleDef] = &[$(&$m::DEF),*];
    };
}

pub mod mini_regex;

rules!(
    curly,
    default_case,
    dot_notation,
    no_case_declarations,
    no_cond_assign,
    no_dupe_class_members,
    no_dupe_else_if,
    no_dupe_keys,
    no_duplicate_case,
    no_empty_function,
    no_empty_pattern,
    no_empty_static_block,
    no_irregular_whitespace,
    no_lonely_if,
    no_loss_of_precision,
    no_nested_ternary,
    no_nonoctal_decimal_escape,
);
