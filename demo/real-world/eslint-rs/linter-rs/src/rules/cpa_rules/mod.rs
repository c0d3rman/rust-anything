use crate::rules::RuleDef;

macro_rules! rules {
    ($($m:ident),* $(,)?) => {
        $(pub mod $m;)*
        pub static ALL: &[&RuleDef] = &[$(&$m::DEF),*];
    };
}

mod segs;

rules!(
    no_unreachable,
    no_fallthrough,
    getter_return,
    constructor_super,
    no_this_before_super,
    no_useless_return,
    no_useless_assignment
);
