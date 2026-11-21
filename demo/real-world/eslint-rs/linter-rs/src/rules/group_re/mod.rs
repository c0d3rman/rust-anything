use crate::rules::RuleDef;
use serde_json::Value;

macro_rules! rules {
    ($($m:ident),* $(,)?) => {
        $(pub mod $m;)*
        pub static ALL: &[&RuleDef] = &[$(&$m::DEF),*];
    };
}

pub mod util;

rules!(
    no_control_regex,
    no_empty_character_class,
    no_invalid_regexp,
    no_misleading_character_class,
    no_regex_spaces,
    no_useless_backreference,
    no_useless_escape,
);

fn no_options(options: &[Value]) -> Result<(), String> {
    if options.is_empty() {
        Ok(())
    } else {
        Err("invalid option".into())
    }
}

fn string_array_option(options: &[Value], name: &str) -> Result<Option<Vec<String>>, String> {
    match options {
        [] => Ok(None),
        [Value::Object(o)] => {
            let mut out = None;
            for (k, v) in o {
                if k != name {
                    return Err("invalid option".into());
                }
                let Value::Array(items) = v else {
                    return Err("invalid option".into());
                };
                let mut list: Vec<String> = Vec::new();
                for i in items {
                    let Value::String(s) = i else {
                        return Err("invalid option".into());
                    };
                    if list.contains(s) {
                        return Err("invalid option".into());
                    }
                    list.push(s.clone());
                }
                out = Some(list);
            }
            Ok(out)
        }
        _ => Err("invalid option".into()),
    }
}
