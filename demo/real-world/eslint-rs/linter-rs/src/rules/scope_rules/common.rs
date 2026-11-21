//! Option validation and reference helpers shared by the scope rules.

use crate::scope::{RefId, ScopeManager};
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

/// ast-utils `getModifyingReferences(references)`.
pub fn modifying_references(sm: &ScopeManager, refs: &[RefId]) -> Vec<RefId> {
    let mut out = Vec::new();
    for (i, &r) in refs.iter().enumerate() {
        let rf = sm.reference(r);
        let different = i == 0 || sm.reference(refs[i - 1]).identifier != rf.identifier;
        if rf.init == Some(false) && rf.is_write() && different {
            out.push(r);
        }
    }
    out
}

/// A `new RegExp(source, "u")` the scope rules support: `^lit`, `^lit.*$`, `^lit$`, `^(a|b)$`.
pub enum Pat {
    Prefix(String),
    OneOf(Vec<String>),
    Re(crate::rules::group_a::mini_regex::Regex),
}

fn is_lit(s: &str) -> bool {
    s.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
}

pub fn simple_pattern(src: &str) -> Option<Pat> {
    literal_pattern(src).or_else(|| crate::rules::group_a::mini_regex::Regex::new(src).map(Pat::Re))
}

fn literal_pattern(src: &str) -> Option<Pat> {
    let rest = src.strip_prefix('^')?;
    let Some(body) = rest.strip_suffix('$') else {
        return (is_lit(rest) && !rest.is_empty()).then(|| Pat::Prefix(rest.to_string()));
    };
    if let Some(prefix) = body.strip_suffix(".*")
        && is_lit(prefix)
    {
        return Some(Pat::Prefix(prefix.to_string()));
    }
    let inner = body
        .strip_prefix('(')
        .and_then(|b| b.strip_suffix(')'))
        .unwrap_or(body);
    let alts: Vec<String> = inner.split('|').map(str::to_string).collect();
    if alts.iter().all(|a| !a.is_empty() && is_lit(a))
        && (inner.len() < body.len() || alts.len() == 1)
    {
        return Some(Pat::OneOf(alts));
    }
    None
}

impl Pat {
    pub fn test(&self, name: &str) -> bool {
        match self {
            Pat::Prefix(p) => name.starts_with(p.as_str()),
            Pat::OneOf(l) => l.iter().any(|x| x == name),
            Pat::Re(r) => r.test(name),
        }
    }
}
