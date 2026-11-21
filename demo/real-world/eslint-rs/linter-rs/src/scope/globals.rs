//! conf/globals.js (es2026, commonjs) and `addDeclaredGlobals` / `markExportedVariables` of
//! lib/languages/js/source-code/source-code.js.

use super::{ScopeManager, VarId};
use crate::ast::Ast;
use crate::directives;
use crate::source::{SourceCode, SourceType};
use std::collections::HashSet;

pub const ES2026: &[&str] = &[
    "Array",
    "Boolean",
    "constructor",
    "Date",
    "decodeURI",
    "decodeURIComponent",
    "encodeURI",
    "encodeURIComponent",
    "Error",
    "escape",
    "eval",
    "EvalError",
    "Function",
    "hasOwnProperty",
    "Infinity",
    "isFinite",
    "isNaN",
    "isPrototypeOf",
    "Math",
    "NaN",
    "Number",
    "Object",
    "parseFloat",
    "parseInt",
    "propertyIsEnumerable",
    "RangeError",
    "ReferenceError",
    "RegExp",
    "String",
    "SyntaxError",
    "toLocaleString",
    "toString",
    "TypeError",
    "undefined",
    "unescape",
    "URIError",
    "valueOf",
    "JSON",
    "ArrayBuffer",
    "DataView",
    "Float32Array",
    "Float64Array",
    "Int16Array",
    "Int32Array",
    "Int8Array",
    "Intl",
    "Map",
    "Promise",
    "Proxy",
    "Reflect",
    "Set",
    "Symbol",
    "Uint16Array",
    "Uint32Array",
    "Uint8Array",
    "Uint8ClampedArray",
    "WeakMap",
    "WeakSet",
    "Atomics",
    "SharedArrayBuffer",
    "BigInt",
    "BigInt64Array",
    "BigUint64Array",
    "globalThis",
    "AggregateError",
    "FinalizationRegistry",
    "WeakRef",
    "Float16Array",
    "Iterator",
    "AsyncDisposableStack",
    "DisposableStack",
    "SuppressedError",
    "Temporal",
];

const COMMONJS: &[(&str, &str)] = &[
    ("exports", "writable"),
    ("global", "readonly"),
    ("module", "readonly"),
    ("require", "readonly"),
];

/// `applyInlineConfig`'s `inlineGlobals`: name, normalized value, comment indices, in order.
pub fn inline_global_comments(sc: &SourceCode) -> Vec<(String, String, Vec<usize>)> {
    let mut out: Vec<(String, String, Vec<usize>)> = Vec::new();
    for c in directives::inline_config_nodes(sc) {
        let Some(d) = directives::parse_directive(&c.value) else {
            continue;
        };
        if !matches!(d.label, "global" | "globals") {
            continue;
        }
        let ci = sc
            .comments
            .iter()
            .position(|x| std::ptr::eq(x, c))
            .unwrap_or(0);
        for (id, setting) in directives::parse_string_config(d.value) {
            let norm = match setting.as_deref() {
                Some("off") => "off",
                Some("true") | Some("writeable") | Some("writable") => "writable",
                None | Some("false") | Some("readable") | Some("readonly") => "readonly",
                Some(_) => continue,
            };
            match out.iter_mut().find(|(k, _, _)| *k == id) {
                Some(slot) => {
                    slot.2.push(ci);
                    slot.1 = norm.to_string();
                }
                None => out.push((id, norm.to_string(), vec![ci])),
            }
        }
    }
    out
}

/// `SourceCode#finalize`: `addDeclaredGlobals` then `markExportedVariables`.
pub fn finalize(
    sm: &mut ScopeManager,
    ast: &Ast,
    source_type: SourceType,
    configured: &[(String, String)],
    inline: &[(String, String, Vec<usize>)],
    exported: &[String],
) {
    let mut config: Vec<(String, String)> = ES2026
        .iter()
        .map(|n| (n.to_string(), "readonly".to_string()))
        .collect();
    let put = |list: &mut Vec<(String, String)>, k: &str, v: &str| match list
        .iter_mut()
        .find(|(n, _)| n == k)
    {
        Some(slot) => slot.1 = v.to_string(),
        None => list.push((k.to_string(), v.to_string())),
    };
    if source_type == SourceType::CommonJs {
        for (k, v) in COMMONJS {
            put(&mut config, k, v);
        }
    }
    for (k, v) in configured {
        put(&mut config, k, v);
    }
    let mut finals = config.clone();
    for (k, v, _) in inline {
        put(&mut finals, k, v);
    }
    let names: Vec<String> = finals
        .iter()
        .filter(|(_, v)| v != "off")
        .map(|(k, _)| k.clone())
        .collect();
    add_globals(sm, ast, &names);
    for name in &names {
        let Some(v) = sm.lookup(0, name) else {
            continue;
        };
        let var = &mut sm.vars[v as usize];
        var.eslint_implicit_global_setting = config
            .iter()
            .find(|(k, _)| k == name)
            .map(|(_, v)| v.clone());
        let il = inline.iter().find(|(k, _, _)| k == name);
        var.eslint_explicit_global = il.is_some();
        var.eslint_explicit_global_comments = il.map(|(_, _, c)| c.clone());
        var.writeable = Some(finals.iter().any(|(k, v)| k == name && v == "writable"));
    }
    for name in exported {
        if let Some(v) = sm.lookup(0, name) {
            let var = &mut sm.vars[v as usize];
            var.eslint_used.set(true);
            var.eslint_exported = true;
        }
    }
}

/// `GlobalScope#__addVariables`.
fn add_globals(sm: &mut ScopeManager, ast: &Ast, names: &[String]) {
    let set: HashSet<&str> = names.iter().map(String::as_str).collect();
    for name in names {
        if sm.scopes[0].set.contains_key(name) {
            continue;
        }
        let v = sm.vars.len() as VarId;
        sm.vars.push(super::Variable {
            name: name.clone(),
            identifiers: Vec::new(),
            references: Vec::new(),
            defs: Vec::new(),
            tainted: false,
            stack: true,
            scope: 0,
            eslint_implicit_global_setting: None,
            eslint_explicit_global: false,
            eslint_explicit_global_comments: None,
            writeable: None,
            eslint_used: std::cell::Cell::new(false),
            eslint_exported: false,
        });
        sm.scopes[0].set.insert(name.clone(), v);
        sm.scopes[0].variables.push(v);
    }
    let through = std::mem::take(&mut sm.scopes[0].through);
    let mut kept = Vec::new();
    for r in through {
        let name = ast.name(sm.refs[r as usize].identifier).unwrap_or("");
        if set.contains(name) {
            if let Some(&v) = sm.scopes[0].set.get(name) {
                sm.refs[r as usize].resolved = Some(v);
                sm.vars[v as usize].references.push(r);
            }
        } else {
            kept.push(r);
        }
    }
    sm.scopes[0].through = kept;
    let implicit_vars = std::mem::take(&mut sm.scopes[0].implicit.variables);
    let mut kept_vars = Vec::new();
    for v in implicit_vars {
        let name = sm.vars[v as usize].name.clone();
        if set.contains(name.as_str()) {
            sm.scopes[0].implicit.set.remove(&name);
        } else {
            kept_vars.push(v);
        }
    }
    sm.scopes[0].implicit.variables = kept_vars;
    let left = std::mem::take(&mut sm.scopes[0].implicit.left);
    sm.scopes[0].implicit.left = left
        .into_iter()
        .filter(|r| !set.contains(ast.name(sm.refs[*r as usize].identifier).unwrap_or("")))
        .collect();
}
