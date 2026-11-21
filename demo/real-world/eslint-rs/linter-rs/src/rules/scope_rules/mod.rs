use crate::rules::RuleDef;

macro_rules! rules {
    ($($m:ident),* $(,)?) => {
        $(pub mod $m;)*
        pub static ALL: &[&RuleDef] = &[$(&$m::DEF),*];
    };
}

pub mod common;

rules!(
    no_class_assign,
    no_const_assign,
    no_delete_var,
    no_ex_assign,
    no_func_assign,
    no_global_assign,
    no_import_assign,
    no_param_reassign,
    no_redeclare,
    no_shadow,
    no_shadow_restricted_names,
    no_unassigned_vars,
    no_unused_vars,
    no_use_before_define,
    no_undef,
    no_var,
    prefer_const,
);
