//! Option validation shared by the rules of this group.

use serde_json::Value;

/// `schema: []`.
pub fn no_options(options: &[Value]) -> Result<(), String> {
    if options.is_empty() {
        Ok(())
    } else {
        Err("invalid option".into())
    }
}

/// `schema: [{ type: "object", properties: { <key>: { type: "boolean" } }, additionalProperties: false }]`.
pub fn bool_object(options: &[Value], keys: &[&str]) -> Result<Vec<Option<bool>>, String> {
    let mut out = vec![None; keys.len()];
    match options {
        [] => {}
        [Value::Object(o)] => {
            for (k, v) in o {
                let Some(i) = keys.iter().position(|x| x == k) else {
                    return Err("invalid option".into());
                };
                match v {
                    Value::Bool(b) => out[i] = Some(*b),
                    _ => return Err("invalid option".into()),
                }
            }
        }
        _ => return Err("invalid option".into()),
    }
    Ok(out)
}
