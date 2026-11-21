use crate::source::{ParseOptions, SourceCode, SourceType};

fn check(code: &str, expected: &str) {
    let opts = ParseOptions {
        source_type: SourceType::Module,
        jsx: false,
        global_return: false,
    };
    let Ok(sc) = SourceCode::parse(code, &opts) else {
        panic!("parse failed");
    };
    let got = super::dump(&sc);
    if got != expected {
        let line = got.lines().zip(expected.lines()).position(|(a, b)| a != b);
        panic!(
            "event sequence differs from ESLint at line {line:?}\n--- got\n{got}\n--- expected\n{expected}"
        );
    }
}

macro_rules! fixture {
    ($name:ident) => {
        #[test]
        fn $name() {
            check(
                include_str!(concat!("fixtures/", stringify!($name), ".js")),
                include_str!(concat!("fixtures/", stringify!($name), ".txt")),
            );
        }
    };
}

fixture!(if_else);
fixture!(loops);
fixture!(try_finally);
fixture!(switch_fallthrough);
fixture!(logical);
fixture!(optional_chaining);
fixture!(classes);
