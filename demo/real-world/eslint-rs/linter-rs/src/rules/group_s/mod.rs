use crate::rules::RuleDef;

macro_rules! rules {
    ($($m:ident),* $(,)?) => {
        $(pub mod $m;)*
        pub static ALL: &[&RuleDef] = &[$(&$m::DEF),*];
    };
}

pub mod common;

rules!(
    use_isnan,
    valid_typeof,
    no_compare_neg_zero,
    no_dupe_args,
    no_async_promise_executor,
    no_throw_literal,
    no_new_wrappers,
    no_new_native_nonconstructor,
    no_alert,
    no_console,
    radix,
    no_eval,
    no_prototype_builtins,
    for_direction,
    no_implied_eval,
    no_obj_calls,
    no_constant_condition,
    no_constant_binary_expression,
    no_extra_boolean_cast,
    no_else_return,
    object_shorthand,
    preserve_caught_error,
);
