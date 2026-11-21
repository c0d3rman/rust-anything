//! @eslint-community/eslint-utils 4.9.1: `getStaticValue`, `getStringIfConstant`,
//! `getPropertyName` and `ReferenceTracker`, on a model of the JavaScript values they compute.
//! Whatever the model cannot reproduce exactly comes back as `Unknown`.

use crate::ast::{Ast, NodeId, Prop};
use crate::eslint_utils::find_variable;
use crate::scope::{DefType, ScopeId, ScopeManager, VarId};
use std::rc::Rc;

/// A JavaScript value `getStaticValue` can produce.
#[derive(Debug, Clone)]
pub enum V {
    Undefined,
    Null,
    Bool(bool),
    Num(f64),
    Str(String),
    BigInt(i128),
    /// An array; `None` is a hole.
    Arr(Rc<Vec<Option<V>>>),
    /// A plain object with its own properties in JavaScript key order.
    Obj(Rc<Vec<(String, V)>>),
    /// A RegExp object made from a literal: pattern and flags.
    RegExp(Rc<(String, String)>),
    /// A built-in object or function, by its canonical path (`Math.max`, `String.prototype.slice`).
    Builtin(&'static str),
}

/// `{ value, optional }`.
#[derive(Debug, Clone)]
pub struct StaticValue {
    pub value: V,
    pub optional: bool,
}

/// The value cannot be computed faithfully: the caller must give up.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Unknown;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Stop {
    Throw,
    Unknown,
}

type Res<T> = Result<T, Stop>;

fn sv(value: V) -> Option<StaticValue> {
    Some(StaticValue {
        value,
        optional: false,
    })
}

impl V {
    pub fn type_of(&self) -> &'static str {
        match self {
            V::Undefined => "undefined",
            V::Null | V::Arr(_) | V::Obj(_) | V::RegExp(_) => "object",
            V::Bool(_) => "boolean",
            V::Num(_) => "number",
            V::Str(_) => "string",
            V::BigInt(_) => "bigint",
            V::Builtin(p) => match builtin_kind(p) {
                Some(BK::Obj) => "object",
                _ => "function",
            },
        }
    }

    fn is_nullish(&self) -> bool {
        matches!(self, V::Undefined | V::Null)
    }

    fn is_object(&self) -> bool {
        matches!(self, V::Arr(_) | V::Obj(_) | V::RegExp(_) | V::Builtin(_))
    }

    /// `Boolean(value)`.
    pub fn truthy(&self) -> bool {
        match self {
            V::Undefined | V::Null => false,
            V::Bool(b) => *b,
            V::Num(n) => !(n.is_nan() || *n == 0.0),
            V::Str(s) => !s.is_empty(),
            V::BigInt(b) => *b != 0,
            _ => true,
        }
    }
}

// ---------------------------------------------------------------------------------------------
// Conversions
// ---------------------------------------------------------------------------------------------

/// `Number#toString()` (radix 10).
pub fn num_to_string(n: f64) -> String {
    if n.is_nan() {
        return "NaN".into();
    }
    if n == 0.0 {
        return "0".into();
    }
    if n.is_infinite() {
        return if n > 0.0 {
            "Infinity".into()
        } else {
            "-Infinity".into()
        };
    }
    let e = format!("{:e}", n.abs());
    let (mant, exp) = match e.split_once('e') {
        Some((m, x)) => (m, x.parse::<i32>().unwrap_or(0)),
        None => (e.as_str(), 0),
    };
    let digits: String = mant.chars().filter(|c| *c != '.').collect();
    let k = digits.len() as i32;
    let n10 = exp + 1;
    let body = if k <= n10 && n10 <= 21 {
        format!("{}{}", digits, "0".repeat((n10 - k) as usize))
    } else if 0 < n10 && n10 <= 21 {
        format!("{}.{}", &digits[..n10 as usize], &digits[n10 as usize..])
    } else if -6 < n10 && n10 <= 0 {
        format!("0.{}{}", "0".repeat((-n10) as usize), digits)
    } else {
        let e = n10 - 1;
        let sign = if e >= 0 { "+" } else { "-" };
        if k == 1 {
            format!("{}e{}{}", digits, sign, e.abs())
        } else {
            format!("{}.{}e{}{}", &digits[..1], &digits[1..], sign, e.abs())
        }
    };
    if n < 0.0 { format!("-{body}") } else { body }
}

fn is_decimal_literal(s: &str) -> bool {
    let b = s.as_bytes();
    let mut i = 0;
    if i < b.len() && (b[i] == b'+' || b[i] == b'-') {
        i += 1;
    }
    let int_start = i;
    while i < b.len() && b[i].is_ascii_digit() {
        i += 1;
    }
    let int_digits = i - int_start;
    let mut frac_digits = 0;
    if i < b.len() && b[i] == b'.' {
        i += 1;
        let f = i;
        while i < b.len() && b[i].is_ascii_digit() {
            i += 1;
        }
        frac_digits = i - f;
    }
    if int_digits == 0 && frac_digits == 0 {
        return false;
    }
    if i < b.len() && (b[i] == b'e' || b[i] == b'E') {
        i += 1;
        if i < b.len() && (b[i] == b'+' || b[i] == b'-') {
            i += 1;
        }
        let e = i;
        while i < b.len() && b[i].is_ascii_digit() {
            i += 1;
        }
        if i == e {
            return false;
        }
    }
    i == b.len()
}

fn radix_digits_to_f64(digits: &str, radix: u32) -> Res<f64> {
    let mut acc: u128 = 0;
    for c in digits.chars() {
        let d = c.to_digit(radix).ok_or(Stop::Unknown)?;
        acc = acc
            .checked_mul(radix as u128)
            .and_then(|a| a.checked_add(d as u128))
            .ok_or(Stop::Unknown)?;
    }
    Ok(acc as f64)
}

/// `StringToNumber`.
pub fn string_to_number(s: &str) -> f64 {
    str_to_num(s).unwrap_or(f64::NAN)
}

fn str_to_num(s: &str) -> Option<f64> {
    let t = crate::utils::js_trim(s);
    if t.is_empty() {
        return Some(0.0);
    }
    if t.len() > 2 && t.as_bytes()[0] == b'0' {
        let radix = match t.as_bytes()[1] {
            b'x' | b'X' => 16,
            b'o' | b'O' => 8,
            b'b' | b'B' => 2,
            _ => 0,
        };
        if radix != 0 {
            let digits = &t[2..];
            if !digits.chars().all(|c| c.is_digit(radix)) {
                return Some(f64::NAN);
            }
            return radix_digits_to_f64(digits, radix).ok();
        }
    }
    match t {
        "Infinity" | "+Infinity" => return Some(f64::INFINITY),
        "-Infinity" => return Some(f64::NEG_INFINITY),
        _ => {}
    }
    if !is_decimal_literal(t) {
        return Some(f64::NAN);
    }
    t.parse::<f64>().ok()
}

fn to_primitive(v: &V) -> Res<V> {
    match v {
        V::Arr(_) | V::Obj(_) | V::RegExp(_) => Ok(V::Str(to_string(v)?)),
        V::Builtin(_) => Err(Stop::Unknown),
        _ => Ok(v.clone()),
    }
}

/// `ToString(value)`.
fn to_string(v: &V) -> Res<String> {
    Ok(match v {
        V::Undefined => "undefined".into(),
        V::Null => "null".into(),
        V::Bool(b) => b.to_string(),
        V::Num(n) => num_to_string(*n),
        V::Str(s) => s.clone(),
        V::BigInt(b) => b.to_string(),
        V::Arr(a) => join(a, ",")?,
        V::Obj(o) => {
            if o.iter()
                .any(|(k, _)| k == "toString" || k == "valueOf" || k == "toJSON")
            {
                return Err(Stop::Unknown);
            }
            "[object Object]".into()
        }
        V::RegExp(r) => format!("/{}/{}", regexp_source(&r.0), canonical_flags(&r.1)),
        V::Builtin(_) => return Err(Stop::Unknown),
    })
}

fn join(a: &[Option<V>], sep: &str) -> Res<String> {
    let mut out = String::new();
    for (i, e) in a.iter().enumerate() {
        if i > 0 {
            out.push_str(sep);
        }
        match e {
            None | Some(V::Undefined) | Some(V::Null) => {}
            Some(x) => out.push_str(&to_string(x)?),
        }
    }
    Ok(out)
}

fn regexp_source(p: &str) -> String {
    if p.is_empty() {
        "(?:)".into()
    } else {
        p.to_string()
    }
}

fn canonical_flags(f: &str) -> String {
    "dgimsuvy".chars().filter(|c| f.contains(*c)).collect()
}

/// `ToNumber(value)`; BigInts throw.
fn to_number(v: &V) -> Res<f64> {
    Ok(match v {
        V::Undefined => f64::NAN,
        V::Null => 0.0,
        V::Bool(b) => f64::from(u8::from(*b)),
        V::Num(n) => *n,
        V::Str(s) => str_to_num(s).ok_or(Stop::Unknown)?,
        V::BigInt(_) => return Err(Stop::Throw),
        _ => return to_number(&to_primitive(v)?),
    })
}

enum Numeric {
    N(f64),
    B(i128),
}

fn to_numeric(v: &V) -> Res<Numeric> {
    match to_primitive(v)? {
        V::BigInt(b) => Ok(Numeric::B(b)),
        p => Ok(Numeric::N(to_number(&p)?)),
    }
}

fn to_int32(n: f64) -> i32 {
    to_uint32(n) as i32
}

fn to_uint32(n: f64) -> u32 {
    if !n.is_finite() || n == 0.0 {
        return 0;
    }
    let t = n.trunc();
    let m = t.rem_euclid(4294967296.0);
    m as u32
}

fn to_integer_or_infinity(v: &V) -> Res<f64> {
    let n = to_number(v)?;
    if n.is_nan() {
        return Ok(0.0);
    }
    Ok(n.trunc() + 0.0)
}

fn utf16(s: &str) -> Vec<u16> {
    s.encode_utf16().collect()
}

fn from_utf16(u: &[u16]) -> Res<String> {
    String::from_utf16(u).map_err(|_| Stop::Unknown)
}

fn strict_eq(a: &V, b: &V) -> bool {
    match (a, b) {
        (V::Undefined, V::Undefined) | (V::Null, V::Null) => true,
        (V::Bool(x), V::Bool(y)) => x == y,
        (V::Num(x), V::Num(y)) => x == y,
        (V::Str(x), V::Str(y)) => x == y,
        (V::BigInt(x), V::BigInt(y)) => x == y,
        (V::Arr(x), V::Arr(y)) => Rc::ptr_eq(x, y),
        (V::Obj(x), V::Obj(y)) => Rc::ptr_eq(x, y),
        (V::RegExp(x), V::RegExp(y)) => Rc::ptr_eq(x, y),
        (V::Builtin(x), V::Builtin(y)) => x == y,
        _ => false,
    }
}

fn same_value_zero(a: &V, b: &V) -> bool {
    match (a, b) {
        (V::Num(x), V::Num(y)) if x.is_nan() && y.is_nan() => true,
        _ => strict_eq(a, b),
    }
}

fn same_value(a: &V, b: &V) -> bool {
    match (a, b) {
        (V::Num(x), V::Num(y)) => {
            (x.is_nan() && y.is_nan()) || (x == y && x.is_sign_negative() == y.is_sign_negative())
        }
        _ => strict_eq(a, b),
    }
}

fn loose_eq(a: &V, b: &V) -> Res<bool> {
    let same_type = std::mem::discriminant(a) == std::mem::discriminant(b);
    if same_type {
        return Ok(strict_eq(a, b));
    }
    if a.is_nullish() && b.is_nullish() {
        return Ok(true);
    }
    match (a, b) {
        (V::Num(x), V::Str(s)) | (V::Str(s), V::Num(x)) => {
            Ok(*x == str_to_num(s).ok_or(Stop::Unknown)?)
        }
        (V::BigInt(_), V::Str(_)) | (V::Str(_), V::BigInt(_)) => Err(Stop::Unknown),
        (V::Bool(x), _) => loose_eq(&V::Num(f64::from(u8::from(*x))), b),
        (_, V::Bool(y)) => loose_eq(a, &V::Num(f64::from(u8::from(*y)))),
        (x, y) if x.is_object() && !y.is_object() && !y.is_nullish() => {
            loose_eq(&to_primitive(x)?, y)
        }
        (x, y) if y.is_object() && !x.is_object() && !x.is_nullish() => {
            loose_eq(x, &to_primitive(y)?)
        }
        (V::BigInt(x), V::Num(y)) | (V::Num(y), V::BigInt(x)) => {
            Ok(y.is_finite() && y.fract() == 0.0 && y.abs() < 1e38 && *y as i128 == *x)
        }
        _ => Ok(false),
    }
}

/// `left < right` (`IsLessThan`); `None` is `undefined`.
fn less_than(a: &V, b: &V, left_first: bool) -> Res<Option<bool>> {
    let (pa, pb) = if left_first {
        (to_primitive(a)?, to_primitive(b)?)
    } else {
        let pb = to_primitive(b)?;
        (to_primitive(a)?, pb)
    };
    if let (V::Str(x), V::Str(y)) = (&pa, &pb) {
        return Ok(Some(utf16(x) < utf16(y)));
    }
    match (to_numeric(&pa)?, to_numeric(&pb)?) {
        (Numeric::N(x), Numeric::N(y)) => {
            if x.is_nan() || y.is_nan() {
                Ok(None)
            } else {
                Ok(Some(x < y))
            }
        }
        (Numeric::B(x), Numeric::B(y)) => Ok(Some(x < y)),
        _ => Err(Stop::Unknown),
    }
}

fn js_pow(x: f64, y: f64) -> Res<f64> {
    if y.is_nan() {
        return Ok(f64::NAN);
    }
    if y == 0.0 {
        return Ok(1.0);
    }
    if x.is_nan() {
        return Ok(f64::NAN);
    }
    if y == 1.0 {
        return Ok(x);
    }
    if x.fract() == 0.0 && x.is_finite() && y.fract() == 0.0 && y > 0.0 && y <= 1100.0 {
        let mut r = 1.0f64;
        for _ in 0..(y as u32) {
            r *= x;
            if r.abs() > 9007199254740992.0 {
                return Err(Stop::Unknown);
            }
        }
        return Ok(r);
    }
    Err(Stop::Unknown)
}

fn bigint_op(op: &str, x: i128, y: i128) -> Res<V> {
    let r = match op {
        "+" => x.checked_add(y),
        "-" => x.checked_sub(y),
        "*" => x.checked_mul(y),
        "/" => {
            if y == 0 {
                return Err(Stop::Throw);
            }
            x.checked_div(y)
        }
        "%" => {
            if y == 0 {
                return Err(Stop::Throw);
            }
            x.checked_rem(y)
        }
        "&" => Some(x & y),
        "|" => Some(x | y),
        "^" => Some(x ^ y),
        _ => None,
    };
    let r = r.ok_or(Stop::Unknown)?;
    if r.unsigned_abs() > (1u128 << 100) {
        return Err(Stop::Unknown);
    }
    Ok(V::BigInt(r))
}

fn binary(op: &str, l: &V, r: &V) -> Res<V> {
    Ok(match op {
        "==" => V::Bool(loose_eq(l, r)?),
        "!=" => V::Bool(!loose_eq(l, r)?),
        "===" => V::Bool(strict_eq(l, r)),
        "!==" => V::Bool(!strict_eq(l, r)),
        "<" => V::Bool(less_than(l, r, true)? == Some(true)),
        ">" => V::Bool(less_than(r, l, false)? == Some(true)),
        "<=" => V::Bool(less_than(r, l, false)? == Some(false)),
        ">=" => V::Bool(less_than(l, r, true)? == Some(false)),
        "+" => {
            let (pl, pr) = (to_primitive(l)?, to_primitive(r)?);
            if matches!(pl, V::Str(_)) || matches!(pr, V::Str(_)) {
                V::Str(to_string(&pl)? + &to_string(&pr)?)
            } else {
                match (to_numeric(&pl)?, to_numeric(&pr)?) {
                    (Numeric::N(a), Numeric::N(b)) => V::Num(a + b),
                    (Numeric::B(a), Numeric::B(b)) => bigint_op("+", a, b)?,
                    _ => return Err(Stop::Throw),
                }
            }
        }
        _ => {
            let (a, b) = (to_numeric(l)?, to_numeric(r)?);
            match (a, b) {
                (Numeric::N(a), Numeric::N(b)) => V::Num(match op {
                    "-" => a - b,
                    "*" => a * b,
                    "/" => a / b,
                    "%" => {
                        if b.is_infinite() && a.is_finite() {
                            a
                        } else {
                            a % b
                        }
                    }
                    "**" => js_pow(a, b)?,
                    "<<" => f64::from(to_int32(a).wrapping_shl(to_uint32(b) & 31)),
                    ">>" => f64::from(to_int32(a) >> (to_uint32(b) & 31)),
                    ">>>" => f64::from(to_uint32(a) >> (to_uint32(b) & 31)),
                    "|" => f64::from(to_int32(a) | to_int32(b)),
                    "^" => f64::from(to_int32(a) ^ to_int32(b)),
                    "&" => f64::from(to_int32(a) & to_int32(b)),
                    _ => return Err(Stop::Unknown),
                }),
                (Numeric::B(a), Numeric::B(b)) => bigint_op(op, a, b)?,
                _ => return Err(Stop::Throw),
            }
        }
    })
}

// ---------------------------------------------------------------------------------------------
// Built-ins
// ---------------------------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq)]
enum BK {
    Fn,
    Obj,
}

const GLOBAL_FNS: &[&str] = &[
    "Array",
    "ArrayBuffer",
    "BigInt",
    "BigInt64Array",
    "BigUint64Array",
    "Boolean",
    "DataView",
    "Date",
    "decodeURI",
    "decodeURIComponent",
    "encodeURI",
    "encodeURIComponent",
    "escape",
    "Float32Array",
    "Float64Array",
    "Function",
    "Int16Array",
    "Int32Array",
    "Int8Array",
    "isFinite",
    "isNaN",
    "Map",
    "Number",
    "Object",
    "parseFloat",
    "parseInt",
    "Promise",
    "Proxy",
    "RegExp",
    "Set",
    "String",
    "Symbol",
    "Uint16Array",
    "Uint32Array",
    "Uint8Array",
    "Uint8ClampedArray",
    "unescape",
    "WeakMap",
    "WeakSet",
];

const MATH_FNS: &[&str] = &[
    "abs", "acos", "acosh", "asin", "asinh", "atan", "atanh", "atan2", "ceil", "cbrt", "expm1",
    "clz32", "cos", "cosh", "exp", "floor", "fround", "hypot", "imul", "log", "log1p", "log2",
    "log10", "max", "min", "pow", "random", "round", "sign", "sin", "sinh", "sqrt", "tan", "tanh",
    "trunc",
];

const OBJECT_PROTO: &[&str] = &[
    "__defineGetter__",
    "__defineSetter__",
    "hasOwnProperty",
    "__lookupGetter__",
    "__lookupSetter__",
    "isPrototypeOf",
    "propertyIsEnumerable",
    "toString",
    "valueOf",
    "toLocaleString",
];

const STRING_PROTO: &[&str] = &[
    "at",
    "charAt",
    "charCodeAt",
    "codePointAt",
    "concat",
    "endsWith",
    "includes",
    "indexOf",
    "lastIndexOf",
    "localeCompare",
    "match",
    "matchAll",
    "normalize",
    "padEnd",
    "padStart",
    "repeat",
    "replace",
    "replaceAll",
    "search",
    "slice",
    "split",
    "startsWith",
    "substr",
    "substring",
    "toLocaleLowerCase",
    "toLocaleUpperCase",
    "toLowerCase",
    "toString",
    "toUpperCase",
    "trim",
    "trimEnd",
    "trimStart",
    "valueOf",
    "anchor",
    "big",
    "blink",
    "bold",
    "fixed",
    "fontcolor",
    "fontsize",
    "italics",
    "link",
    "small",
    "strike",
    "sub",
    "sup",
];

const ARRAY_PROTO: &[&str] = &[
    "at",
    "concat",
    "copyWithin",
    "entries",
    "every",
    "fill",
    "filter",
    "find",
    "findIndex",
    "findLast",
    "findLastIndex",
    "flat",
    "flatMap",
    "forEach",
    "includes",
    "indexOf",
    "join",
    "keys",
    "lastIndexOf",
    "map",
    "pop",
    "push",
    "reduce",
    "reduceRight",
    "reverse",
    "shift",
    "slice",
    "some",
    "sort",
    "splice",
    "toLocaleString",
    "toReversed",
    "toSorted",
    "toSpliced",
    "toString",
    "unshift",
    "values",
    "with",
];

const NUMBER_PROTO: &[&str] = &[
    "toExponential",
    "toFixed",
    "toLocaleString",
    "toPrecision",
    "toString",
    "valueOf",
];
const BOOLEAN_PROTO: &[&str] = &["toString", "valueOf"];
const MAP_PROTO: &[&str] = &[
    "clear", "delete", "entries", "forEach", "get", "has", "keys", "set", "values",
];
const SET_PROTO: &[&str] = &[
    "add", "clear", "delete", "entries", "forEach", "has", "keys", "values",
];

const STATICS: &[(&str, &[&str])] = &[
    ("Math", MATH_FNS),
    (
        "Number",
        &[
            "isFinite",
            "isInteger",
            "isNaN",
            "isSafeInteger",
            "parseFloat",
            "parseInt",
        ],
    ),
    (
        "Object",
        &[
            "assign",
            "create",
            "defineProperties",
            "defineProperty",
            "entries",
            "freeze",
            "fromEntries",
            "getOwnPropertyDescriptor",
            "getOwnPropertyDescriptors",
            "getOwnPropertyNames",
            "getOwnPropertySymbols",
            "getPrototypeOf",
            "hasOwn",
            "is",
            "isExtensible",
            "isFrozen",
            "isSealed",
            "keys",
            "preventExtensions",
            "seal",
            "setPrototypeOf",
            "values",
        ],
    ),
    ("Array", &["from", "isArray", "of"]),
    ("String", &["fromCharCode", "fromCodePoint", "raw"]),
    ("JSON", &["parse", "stringify"]),
    (
        "Reflect",
        &[
            "apply",
            "construct",
            "defineProperty",
            "deleteProperty",
            "get",
            "getOwnPropertyDescriptor",
            "getPrototypeOf",
            "has",
            "isExtensible",
            "ownKeys",
            "preventExtensions",
            "set",
            "setPrototypeOf",
        ],
    ),
    ("Symbol", &["for", "keyFor"]),
    ("Date", &["now", "parse", "UTC"]),
    (
        "Promise",
        &["all", "allSettled", "any", "race", "reject", "resolve"],
    ),
];

const PROTOS: &[(&str, &[&str])] = &[
    ("String", STRING_PROTO),
    ("Array", ARRAY_PROTO),
    ("Number", NUMBER_PROTO),
    ("Boolean", BOOLEAN_PROTO),
    ("Object", OBJECT_PROTO),
    ("Map", MAP_PROTO),
    ("Set", SET_PROTO),
];

fn intern_path(p: String) -> &'static str {
    use std::cell::RefCell;
    use std::collections::HashMap;
    thread_local! {
        static PATHS: RefCell<HashMap<String, &'static str>> = RefCell::new(HashMap::new());
    }
    PATHS.with(|m| {
        let mut m = m.borrow_mut();
        if let Some(v) = m.get(&p) {
            return *v;
        }
        let leaked: &'static str = Box::leak(p.clone().into_boxed_str());
        m.insert(p, leaked);
        leaked
    })
}

fn canonical(path: String) -> &'static str {
    let c = match path.as_str() {
        "Number.parseInt" => "parseInt".to_string(),
        "Number.parseFloat" => "parseFloat".to_string(),
        "String.prototype.trimLeft" => "String.prototype.trimStart".to_string(),
        "String.prototype.trimRight" => "String.prototype.trimEnd".to_string(),
        _ => path,
    };
    intern_path(c)
}

fn builtin_kind(p: &str) -> Option<BK> {
    if matches!(p, "Math" | "JSON" | "Reflect") || p.ends_with(".prototype") {
        return Some(BK::Obj);
    }
    Some(BK::Fn)
}

fn proto_of(p: &str) -> Option<&'static [&'static str]> {
    let ctor = p.strip_suffix(".prototype")?;
    PROTOS.iter().find(|(c, _)| *c == ctor).map(|(_, l)| *l)
}

enum Got {
    Val(V),
    Getter,
}

fn builtin_prop(p: &'static str, key: &str) -> Res<Got> {
    if key == "__proto__" {
        return Ok(Got::Getter);
    }
    if p == "Math" {
        let c = match key {
            "E" => Some(std::f64::consts::E),
            "LN10" => Some(std::f64::consts::LN_10),
            "LN2" => Some(std::f64::consts::LN_2),
            "LOG10E" => Some(std::f64::consts::LOG10_E),
            "LOG2E" => Some(std::f64::consts::LOG2_E),
            "PI" => Some(std::f64::consts::PI),
            "SQRT1_2" => Some(std::f64::consts::FRAC_1_SQRT_2),
            "SQRT2" => Some(std::f64::consts::SQRT_2),
            _ => None,
        };
        if let Some(c) = c {
            return Ok(Got::Val(V::Num(c)));
        }
    }
    if p == "Number" {
        let c = match key {
            "EPSILON" => Some(f64::EPSILON),
            "MAX_SAFE_INTEGER" => Some(9007199254740991.0),
            "MIN_SAFE_INTEGER" => Some(-9007199254740991.0),
            "MAX_VALUE" => Some(f64::MAX),
            "MIN_VALUE" => Some(5e-324),
            "NaN" => Some(f64::NAN),
            "NEGATIVE_INFINITY" => Some(f64::NEG_INFINITY),
            "POSITIVE_INFINITY" => Some(f64::INFINITY),
            _ => None,
        };
        if let Some(c) = c {
            return Ok(Got::Val(V::Num(c)));
        }
    }
    if let Some(list) = proto_of(p) {
        if list.contains(&key) {
            return Ok(Got::Val(V::Builtin(canonical(format!("{p}.{key}")))));
        }
        if matches!(p, "Map.prototype" | "Set.prototype") && key == "size" {
            return Ok(Got::Getter);
        }
        if p != "Object.prototype" && OBJECT_PROTO.contains(&key) {
            return Ok(Got::Val(V::Builtin(canonical(format!(
                "Object.prototype.{key}"
            )))));
        }
        return Err(Stop::Unknown);
    }
    if let Some((_, list)) = STATICS.iter().find(|(c, _)| *c == p)
        && list.contains(&key)
    {
        return Ok(Got::Val(V::Builtin(canonical(format!("{p}.{key}")))));
    }
    if key == "prototype" && PROTOS.iter().any(|(c, _)| *c == p) {
        return Ok(Got::Val(V::Builtin(canonical(format!("{p}.prototype")))));
    }
    if builtin_kind(p) == Some(BK::Fn) && matches!(key, "call" | "apply" | "bind") {
        return Ok(Got::Val(V::Builtin(canonical(format!(
            "Function.prototype.{key}"
        )))));
    }
    Err(Stop::Unknown)
}

fn is_array_index(key: &str) -> Option<usize> {
    if key == "0" {
        return Some(0);
    }
    if key.is_empty()
        || key.starts_with('0')
        || !key.bytes().all(|b| b.is_ascii_digit())
        || key.len() > 10
    {
        return None;
    }
    let n: u64 = key.parse().ok()?;
    if n >= 4294967295 {
        return None;
    }
    Some(n as usize)
}

fn proto_method(proto: &str, list: &[&str], key: &str) -> Res<Got> {
    if list.contains(&key) {
        return Ok(Got::Val(V::Builtin(canonical(format!(
            "{proto}.prototype.{key}"
        )))));
    }
    if OBJECT_PROTO.contains(&key) {
        return Ok(Got::Val(V::Builtin(canonical(format!(
            "Object.prototype.{key}"
        )))));
    }
    Err(Stop::Unknown)
}

/// `value[key]`, and whether the property is a getter (`isGetter`).
fn get_prop(v: &V, key: &str) -> Res<Got> {
    match v {
        V::Undefined | V::Null => Err(Stop::Throw),
        V::Bool(_) => proto_method("Boolean", BOOLEAN_PROTO, key),
        V::Num(_) => proto_method("Number", NUMBER_PROTO, key),
        V::BigInt(_) => Err(Stop::Unknown),
        V::Str(s) => {
            if key == "length" {
                return Ok(Got::Val(V::Num(s.encode_utf16().count() as f64)));
            }
            if let Some(i) = is_array_index(key) {
                let u = utf16(s);
                return Ok(Got::Val(match u.get(i) {
                    Some(c) => V::Str(from_utf16(&[*c])?),
                    None => V::Undefined,
                }));
            }
            proto_method("String", STRING_PROTO, key)
        }
        V::Arr(a) => {
            if key == "length" {
                return Ok(Got::Val(V::Num(a.len() as f64)));
            }
            if let Some(i) = is_array_index(key) {
                return Ok(Got::Val(
                    a.get(i).cloned().flatten().unwrap_or(V::Undefined),
                ));
            }
            if key == "__proto__" {
                return Ok(Got::Getter);
            }
            proto_method("Array", ARRAY_PROTO, key)
        }
        V::Obj(o) => {
            if let Some((_, x)) = o.iter().find(|(k, _)| k == key) {
                return Ok(Got::Val(x.clone()));
            }
            if key == "__proto__" {
                return Ok(Got::Getter);
            }
            if OBJECT_PROTO.contains(&key) {
                return Ok(Got::Val(V::Builtin(canonical(format!(
                    "Object.prototype.{key}"
                )))));
            }
            if key == "constructor" {
                return Err(Stop::Unknown);
            }
            Ok(Got::Val(V::Undefined))
        }
        V::RegExp(r) => Ok(match key {
            "source" => Got::Val(V::Str(regexp_source(&r.0))),
            "flags" => Got::Val(V::Str(canonical_flags(&r.1))),
            "global" => Got::Val(V::Bool(r.1.contains('g'))),
            "ignoreCase" => Got::Val(V::Bool(r.1.contains('i'))),
            "multiline" => Got::Val(V::Bool(r.1.contains('m'))),
            "dotAll" => Got::Val(V::Bool(r.1.contains('s'))),
            "sticky" => Got::Val(V::Bool(r.1.contains('y'))),
            "unicode" => Got::Val(V::Bool(r.1.contains('u'))),
            "hasIndices" => Got::Val(V::Bool(r.1.contains('d'))),
            "unicodeSets" | "__proto__" => Got::Getter,
            "lastIndex" => Got::Val(V::Num(0.0)),
            _ => return Err(Stop::Unknown),
        }),
        V::Builtin(p) => builtin_prop(p, key),
    }
}

fn is_call_allowed(p: &str) -> bool {
    if let Some(f) = p.strip_prefix("Math.") {
        return f != "random";
    }
    if let Some(m) = p.strip_prefix("Array.prototype.") {
        return matches!(
            m,
            "at" | "concat"
                | "entries"
                | "every"
                | "filter"
                | "find"
                | "findIndex"
                | "flat"
                | "includes"
                | "indexOf"
                | "join"
                | "keys"
                | "lastIndexOf"
                | "slice"
                | "some"
                | "toString"
                | "values"
        );
    }
    if let Some(m) = p.strip_prefix("String.prototype.") {
        return matches!(
            m,
            "at" | "charAt"
                | "charCodeAt"
                | "codePointAt"
                | "concat"
                | "endsWith"
                | "includes"
                | "indexOf"
                | "lastIndexOf"
                | "normalize"
                | "padEnd"
                | "padStart"
                | "slice"
                | "startsWith"
                | "substr"
                | "substring"
                | "toLowerCase"
                | "toString"
                | "toUpperCase"
                | "trim"
                | "trimEnd"
                | "trimStart"
        );
    }
    if let Some(m) = p.strip_prefix("Map.prototype.") {
        return matches!(m, "entries" | "get" | "has" | "keys" | "values");
    }
    if let Some(m) = p.strip_prefix("Set.prototype.") {
        return matches!(m, "entries" | "has" | "keys" | "values");
    }
    if let Some(m) = p.strip_prefix("Number.prototype.") {
        return matches!(m, "toExponential" | "toFixed" | "toPrecision" | "toString");
    }
    matches!(
        p,
        "Array.isArray"
            | "Array.of"
            | "BigInt"
            | "Boolean"
            | "Date"
            | "Date.parse"
            | "decodeURI"
            | "decodeURIComponent"
            | "encodeURI"
            | "encodeURIComponent"
            | "escape"
            | "isFinite"
            | "isNaN"
            | "Object.prototype.isPrototypeOf"
            | "Map"
            | "Number"
            | "Number.isFinite"
            | "Number.isNaN"
            | "parseFloat"
            | "parseInt"
            | "Object"
            | "Object.entries"
            | "Object.is"
            | "Object.isExtensible"
            | "Object.isFrozen"
            | "Object.isSealed"
            | "Object.keys"
            | "Object.values"
            | "RegExp"
            | "Set"
            | "String"
            | "String.fromCharCode"
            | "String.fromCodePoint"
            | "String.raw"
            | "Symbol.for"
            | "Symbol.keyFor"
            | "unescape"
    )
}

fn is_pass_through(p: &str) -> bool {
    matches!(
        p,
        "Object.freeze" | "Object.preventExtensions" | "Object.seal"
    )
}

fn arg(args: &[V], i: usize) -> V {
    args.get(i).cloned().unwrap_or(V::Undefined)
}

fn rel_index(v: &V, len: f64, default: f64) -> Res<f64> {
    if matches!(v, V::Undefined) {
        return Ok(default);
    }
    let r = to_integer_or_infinity(v)?;
    Ok(if r < 0.0 {
        (len + r).max(0.0)
    } else {
        r.min(len)
    })
}

fn this_string(this: &V) -> Res<Vec<u16>> {
    if this.is_nullish() {
        return Err(Stop::Throw);
    }
    Ok(utf16(&to_string(this)?))
}

fn find_u16(hay: &[u16], needle: &[u16], from: usize) -> Option<usize> {
    if needle.is_empty() {
        return if from <= hay.len() { Some(from) } else { None };
    }
    if needle.len() > hay.len() {
        return None;
    }
    (from..=hay.len() - needle.len()).find(|&i| hay[i..i + needle.len()] == *needle)
}

fn object_keys(v: &V) -> Res<Vec<(String, V)>> {
    match v {
        V::Undefined | V::Null => Err(Stop::Throw),
        V::Bool(_) | V::Num(_) | V::BigInt(_) => Ok(Vec::new()),
        V::Str(s) => {
            let u = utf16(s);
            let mut out = Vec::new();
            for (i, c) in u.iter().enumerate() {
                out.push((i.to_string(), V::Str(from_utf16(&[*c])?)));
            }
            Ok(out)
        }
        V::Arr(a) => Ok(a
            .iter()
            .enumerate()
            .filter_map(|(i, e)| e.clone().map(|e| (i.to_string(), e)))
            .collect()),
        V::Obj(o) => Ok(o.as_ref().clone()),
        V::RegExp(_) => Ok(Vec::new()),
        V::Builtin(_) => Err(Stop::Unknown),
    }
}

fn parse_int(s: &str, radix: &V) -> Res<f64> {
    let t = s.trim_start_matches(crate::utils::is_js_whitespace);
    let (neg, mut t) = match t.as_bytes().first() {
        Some(b'-') => (true, &t[1..]),
        Some(b'+') => (false, &t[1..]),
        _ => (false, t),
    };
    let mut r = to_int32(to_number(radix)?);
    let mut strip = true;
    if r != 0 {
        if !(2..=36).contains(&r) {
            return Ok(f64::NAN);
        }
        if r != 16 {
            strip = false;
        }
    } else {
        r = 10;
    }
    if strip && (t.starts_with("0x") || t.starts_with("0X")) {
        t = &t[2..];
        r = 16;
    }
    let digits: String = t.chars().take_while(|c| c.is_digit(r as u32)).collect();
    if digits.is_empty() {
        return Ok(f64::NAN);
    }
    let v = if r == 10 {
        if digits.len() > 20 {
            return Err(Stop::Unknown);
        }
        digits.parse::<f64>().map_err(|_| Stop::Unknown)?
    } else {
        let v = radix_digits_to_f64(&digits, r as u32)?;
        if v > 9007199254740992.0 && !(r as u32).is_power_of_two() {
            return Err(Stop::Unknown);
        }
        v
    };
    Ok(if neg { -v } else { v })
}

fn parse_float(s: &str) -> Res<f64> {
    let t = s.trim_start_matches(crate::utils::is_js_whitespace);
    for inf in ["Infinity", "+Infinity"] {
        if t.starts_with(inf) {
            return Ok(f64::INFINITY);
        }
    }
    if t.starts_with("-Infinity") {
        return Ok(f64::NEG_INFINITY);
    }
    let mut best = None;
    let mut end = 0;
    for (i, c) in t.char_indices() {
        if !(c.is_ascii_digit() || matches!(c, '+' | '-' | '.' | 'e' | 'E')) {
            break;
        }
        end = i + 1;
        if is_decimal_literal(&t[..end]) {
            best = Some(end);
        }
    }
    let _ = end;
    match best {
        Some(e) => t[..e].parse::<f64>().map_err(|_| Stop::Unknown),
        None => Ok(f64::NAN),
    }
}

fn math(f: &str, args: &[V]) -> Res<V> {
    let n = |i: usize| to_number(&arg(args, i));
    Ok(V::Num(match f {
        "abs" => n(0)?.abs(),
        "ceil" => n(0)?.ceil(),
        "floor" => n(0)?.floor(),
        "trunc" => n(0)?.trunc(),
        "sqrt" => n(0)?.sqrt(),
        "fround" => f64::from(n(0)? as f32),
        "sign" => {
            let x = n(0)?;
            if x.is_nan() || x == 0.0 {
                x
            } else {
                x.signum()
            }
        }
        "round" => {
            let x = n(0)?;
            if !x.is_finite() || x == 0.0 {
                x
            } else {
                let f = x.floor();
                let r = if x - f >= 0.5 { f + 1.0 } else { f };
                if r == 0.0 && x < 0.0 { -0.0 } else { r }
            }
        }
        "max" | "min" => {
            let mut acc = if f == "max" {
                f64::NEG_INFINITY
            } else {
                f64::INFINITY
            };
            let mut nan = false;
            for a in args {
                let x = to_number(a)?;
                if x.is_nan() {
                    nan = true;
                } else if f == "max" {
                    if x > acc || (x == 0.0 && acc == 0.0 && !x.is_sign_negative()) {
                        acc = x;
                    }
                } else if x < acc || (x == 0.0 && acc == 0.0 && x.is_sign_negative()) {
                    acc = x;
                }
            }
            if nan { f64::NAN } else { acc }
        }
        "clz32" => f64::from(to_uint32(n(0)?).leading_zeros()),
        "imul" => f64::from(to_int32(n(0)?).wrapping_mul(to_int32(n(1)?))),
        "pow" => js_pow(n(0)?, n(1)?)?,
        _ => return Err(Stop::Unknown),
    }))
}

fn string_method(m: &str, this: &V, args: &[V]) -> Res<V> {
    if m == "toString" {
        return match this {
            V::Str(s) => Ok(V::Str(s.clone())),
            _ => Err(Stop::Unknown),
        };
    }
    let s = this_string(this)?;
    let len = s.len() as f64;
    let str_arg = |i: usize| -> Res<Vec<u16>> {
        let a = arg(args, i);
        if matches!(a, V::RegExp(_)) && matches!(m, "includes" | "startsWith" | "endsWith") {
            return Err(Stop::Throw);
        }
        Ok(utf16(&to_string(&a)?))
    };
    let out = |u: &[u16]| -> Res<V> { Ok(V::Str(from_utf16(u)?)) };
    match m {
        "at" => {
            let r = to_integer_or_infinity(&arg(args, 0))?;
            let k = if r >= 0.0 { r } else { len + r };
            if k < 0.0 || k >= len {
                return Ok(V::Undefined);
            }
            out(&s[k as usize..k as usize + 1])
        }
        "charAt" | "charCodeAt" | "codePointAt" => {
            let p = to_integer_or_infinity(&arg(args, 0))?;
            if p < 0.0 || p >= len {
                return Ok(match m {
                    "charAt" => V::Str(String::new()),
                    "charCodeAt" => V::Num(f64::NAN),
                    _ => V::Undefined,
                });
            }
            let i = p as usize;
            match m {
                "charAt" => out(&s[i..i + 1]),
                "charCodeAt" => Ok(V::Num(f64::from(s[i]))),
                _ => {
                    let c = s[i];
                    if (0xD800..0xDC00).contains(&c)
                        && let Some(&d) = s.get(i + 1)
                        && (0xDC00..0xE000).contains(&d)
                    {
                        return Ok(V::Num(f64::from(
                            (u32::from(c) - 0xD800) * 0x400 + (u32::from(d) - 0xDC00) + 0x10000,
                        )));
                    }
                    Ok(V::Num(f64::from(c)))
                }
            }
        }
        "concat" => {
            let mut r = from_utf16(&s)?;
            for a in args {
                r.push_str(&to_string(a)?);
            }
            Ok(V::Str(r))
        }
        "includes" | "indexOf" => {
            let needle = str_arg(0)?;
            let pos = to_integer_or_infinity(&arg(args, 1))?.clamp(0.0, len) as usize;
            let f = find_u16(&s, &needle, pos);
            Ok(if m == "includes" {
                V::Bool(f.is_some())
            } else {
                V::Num(f.map(|x| x as f64).unwrap_or(-1.0))
            })
        }
        "lastIndexOf" => {
            let needle = str_arg(0)?;
            let num = to_number(&arg(args, 1))?;
            let pos = if num.is_nan() {
                f64::INFINITY
            } else {
                num.trunc()
            };
            let start = pos.clamp(0.0, len) as usize;
            if needle.len() > s.len() {
                return Ok(V::Num(-1.0));
            }
            let top = start.min(s.len() - needle.len());
            let f = (0..=top).rev().find(|&i| s[i..i + needle.len()] == *needle);
            Ok(V::Num(f.map(|x| x as f64).unwrap_or(-1.0)))
        }
        "startsWith" | "endsWith" => {
            let needle = str_arg(0)?;
            if m == "startsWith" {
                let p = to_integer_or_infinity(&arg(args, 1))?.clamp(0.0, len) as usize;
                Ok(V::Bool(
                    s.len() >= p + needle.len() && s[p..p + needle.len()] == *needle,
                ))
            } else {
                let e = if matches!(arg(args, 1), V::Undefined) {
                    len
                } else {
                    to_integer_or_infinity(&arg(args, 1))?
                };
                let e = e.clamp(0.0, len) as usize;
                Ok(V::Bool(
                    e >= needle.len() && s[e - needle.len()..e] == *needle,
                ))
            }
        }
        "padEnd" | "padStart" => {
            let max = to_integer_or_infinity(&arg(args, 0))?;
            if max <= len {
                return out(&s);
            }
            let fill = match arg(args, 1) {
                V::Undefined => vec![0x20u16],
                f => utf16(&to_string(&f)?),
            };
            if fill.is_empty() {
                return out(&s);
            }
            if max > 1e7 {
                return Err(Stop::Unknown);
            }
            let need = max as usize - s.len();
            let pad: Vec<u16> = fill.iter().copied().cycle().take(need).collect();
            let r: Vec<u16> = if m == "padEnd" {
                [s.clone(), pad].concat()
            } else {
                [pad, s.clone()].concat()
            };
            out(&r)
        }
        "slice" => {
            let from = rel_index(&arg(args, 0), len, 0.0)?;
            let to = rel_index(&arg(args, 1), len, len)?;
            if from >= to {
                return Ok(V::Str(String::new()));
            }
            out(&s[from as usize..to as usize])
        }
        "substring" => {
            let a = to_integer_or_infinity(&arg(args, 0))?.clamp(0.0, len);
            let b = if matches!(arg(args, 1), V::Undefined) {
                len
            } else {
                to_integer_or_infinity(&arg(args, 1))?.clamp(0.0, len)
            };
            let (x, y) = if a < b { (a, b) } else { (b, a) };
            out(&s[x as usize..y as usize])
        }
        "substr" => {
            let st = rel_index(&arg(args, 0), len, 0.0)?;
            let l = if matches!(arg(args, 1), V::Undefined) {
                f64::INFINITY
            } else {
                to_integer_or_infinity(&arg(args, 1))?
            };
            let end = (st + l.clamp(0.0, len)).min(len);
            if st >= end {
                return Ok(V::Str(String::new()));
            }
            out(&s[st as usize..end as usize])
        }
        "toLowerCase" | "toUpperCase" | "normalize" => {
            let st = from_utf16(&s)?;
            if !st.is_ascii() {
                return Err(Stop::Unknown);
            }
            if m == "normalize" && !matches!(arg(args, 0), V::Undefined) {
                return Err(Stop::Unknown);
            }
            Ok(V::Str(match m {
                "toLowerCase" => st.to_ascii_lowercase(),
                "toUpperCase" => st.to_ascii_uppercase(),
                _ => st,
            }))
        }
        "trim" | "trimStart" | "trimEnd" => {
            let st = from_utf16(&s)?;
            let ws = crate::utils::is_js_whitespace;
            Ok(V::Str(match m {
                "trim" => st.trim_matches(ws).to_string(),
                "trimStart" => st.trim_start_matches(ws).to_string(),
                _ => st.trim_end_matches(ws).to_string(),
            }))
        }
        _ => Err(Stop::Unknown),
    }
}

fn array_method(m: &str, this: &V, args: &[V]) -> Res<V> {
    let V::Arr(a) = this else {
        return Err(Stop::Unknown);
    };
    let len = a.len() as f64;
    match m {
        "at" => {
            let r = to_integer_or_infinity(&arg(args, 0))?;
            let k = if r >= 0.0 { r } else { len + r };
            if k < 0.0 || k >= len {
                return Ok(V::Undefined);
            }
            Ok(a[k as usize].clone().unwrap_or(V::Undefined))
        }
        "concat" => {
            let mut out: Vec<Option<V>> = a.as_ref().clone();
            for x in args {
                match x {
                    V::Arr(b) => out.extend(b.iter().cloned()),
                    V::Builtin(_) => return Err(Stop::Unknown),
                    other => out.push(Some(other.clone())),
                }
            }
            Ok(V::Arr(Rc::new(out)))
        }
        "includes" => {
            let x = arg(args, 0);
            let from = rel_index(&arg(args, 1), len, 0.0)? as usize;
            Ok(V::Bool(a.iter().skip(from).any(|e| {
                same_value_zero(e.as_ref().unwrap_or(&V::Undefined), &x)
            })))
        }
        "indexOf" => {
            let x = arg(args, 0);
            let from = rel_index(&arg(args, 1), len, 0.0)? as usize;
            let f = a
                .iter()
                .enumerate()
                .skip(from)
                .find(|(_, e)| e.as_ref().is_some_and(|e| strict_eq(e, &x)));
            Ok(V::Num(f.map(|(i, _)| i as f64).unwrap_or(-1.0)))
        }
        "lastIndexOf" if args.len() < 2 => {
            let x = arg(args, 0);
            let f = a
                .iter()
                .enumerate()
                .rev()
                .find(|(_, e)| e.as_ref().is_some_and(|e| strict_eq(e, &x)));
            Ok(V::Num(f.map(|(i, _)| i as f64).unwrap_or(-1.0)))
        }
        "join" => {
            let sep = match arg(args, 0) {
                V::Undefined => ",".to_string(),
                s => to_string(&s)?,
            };
            Ok(V::Str(join(a, &sep)?))
        }
        "toString" => Ok(V::Str(join(a, ",")?)),
        "slice" => {
            let from = rel_index(&arg(args, 0), len, 0.0)?;
            let to = rel_index(&arg(args, 1), len, len)?;
            let v: Vec<Option<V>> = if from < to {
                a[from as usize..to as usize].to_vec()
            } else {
                Vec::new()
            };
            Ok(V::Arr(Rc::new(v)))
        }
        _ => Err(Stop::Unknown),
    }
}

fn call_builtin(p: &str, this: &V, args: &[V]) -> Res<V> {
    if let Some(f) = p.strip_prefix("Math.") {
        return math(f, args);
    }
    if let Some(m) = p.strip_prefix("String.prototype.") {
        return string_method(m, this, args);
    }
    if let Some(m) = p.strip_prefix("Array.prototype.") {
        return array_method(m, this, args);
    }
    if p.starts_with("Map.prototype.") || p.starts_with("Set.prototype.") {
        return match this {
            V::Builtin(_) => Err(Stop::Unknown),
            _ => Err(Stop::Throw),
        };
    }
    match p {
        "Number" => Ok(V::Num(match args.first() {
            None => 0.0,
            Some(a) => match to_numeric(a)? {
                Numeric::N(n) => n,
                Numeric::B(b) => b as f64,
            },
        })),
        "String" => Ok(V::Str(match args.first() {
            None => String::new(),
            Some(a) => to_string(a)?,
        })),
        "Boolean" => Ok(V::Bool(arg(args, 0).truthy())),
        "isNaN" => Ok(V::Bool(to_number(&arg(args, 0))?.is_nan())),
        "isFinite" => Ok(V::Bool(to_number(&arg(args, 0))?.is_finite())),
        "Number.isNaN" => Ok(V::Bool(matches!(arg(args, 0), V::Num(n) if n.is_nan()))),
        "Number.isFinite" => Ok(V::Bool(matches!(arg(args, 0), V::Num(n) if n.is_finite()))),
        "parseInt" => {
            let s = to_string(&arg(args, 0))?;
            Ok(V::Num(parse_int(&s, &arg(args, 1))?))
        }
        "parseFloat" => {
            let s = to_string(&arg(args, 0))?;
            Ok(V::Num(parse_float(&s)?))
        }
        "Object.is" => Ok(V::Bool(same_value(&arg(args, 0), &arg(args, 1)))),
        "Object.keys" => Ok(V::Arr(Rc::new(
            object_keys(&arg(args, 0))?
                .into_iter()
                .map(|(k, _)| Some(V::Str(k)))
                .collect(),
        ))),
        "Object.values" => Ok(V::Arr(Rc::new(
            object_keys(&arg(args, 0))?
                .into_iter()
                .map(|(_, v)| Some(v))
                .collect(),
        ))),
        "Object.entries" => Ok(V::Arr(Rc::new(
            object_keys(&arg(args, 0))?
                .into_iter()
                .map(|(k, v)| Some(V::Arr(Rc::new(vec![Some(V::Str(k)), Some(v)]))))
                .collect(),
        ))),
        "Object.isExtensible" | "Object.isFrozen" | "Object.isSealed" => {
            let a = arg(args, 0);
            match a {
                V::Builtin(_) => Err(Stop::Unknown),
                x if x.is_object() => Ok(V::Bool(p == "Object.isExtensible")),
                _ => Ok(V::Bool(p != "Object.isExtensible")),
            }
        }
        "Array.isArray" => Ok(V::Bool(matches!(
            arg(args, 0),
            V::Arr(_) | V::Builtin("Array.prototype")
        ))),
        "Array.of" => Ok(V::Arr(Rc::new(args.iter().cloned().map(Some).collect()))),
        "String.fromCharCode" => {
            let mut u = Vec::new();
            for a in args {
                u.push(to_uint32(to_number(a)?) as u16);
            }
            Ok(V::Str(from_utf16(&u)?))
        }
        "Number.prototype.toString" => match this {
            V::Num(n)
                if matches!(arg(args, 0), V::Undefined)
                    || matches!(arg(args, 0), V::Num(r) if r == 10.0) =>
            {
                Ok(V::Str(num_to_string(*n)))
            }
            _ => Err(Stop::Unknown),
        },
        "Object.prototype.isPrototypeOf" => {
            if !arg(args, 0).is_object() {
                return Ok(V::Bool(false));
            }
            if this.is_nullish() {
                return Err(Stop::Throw);
            }
            Err(Stop::Unknown)
        }
        "Map" | "Set" => Err(Stop::Throw),
        _ => Err(Stop::Unknown),
    }
}

const BUILTIN_NAMES: &[&str] = &[
    "Array",
    "ArrayBuffer",
    "BigInt",
    "BigInt64Array",
    "BigUint64Array",
    "Boolean",
    "DataView",
    "Date",
    "decodeURI",
    "decodeURIComponent",
    "encodeURI",
    "encodeURIComponent",
    "escape",
    "Float32Array",
    "Float64Array",
    "Function",
    "Infinity",
    "Int16Array",
    "Int32Array",
    "Int8Array",
    "isFinite",
    "isNaN",
    "isPrototypeOf",
    "JSON",
    "Map",
    "Math",
    "NaN",
    "Number",
    "Object",
    "parseFloat",
    "parseInt",
    "Promise",
    "Proxy",
    "Reflect",
    "RegExp",
    "Set",
    "String",
    "Symbol",
    "Uint16Array",
    "Uint32Array",
    "Uint8Array",
    "Uint8ClampedArray",
    "undefined",
    "unescape",
    "WeakMap",
    "WeakSet",
];

fn global_value(name: &str) -> Option<V> {
    Some(match name {
        "Infinity" => V::Num(f64::INFINITY),
        "NaN" => V::Num(f64::NAN),
        "undefined" => V::Undefined,
        "isPrototypeOf" => V::Builtin("Object.prototype.isPrototypeOf"),
        "JSON" | "Math" | "Reflect" => V::Builtin(intern_path(name.to_string())),
        n if GLOBAL_FNS.contains(&n) => V::Builtin(intern_path(n.to_string())),
        _ => return None,
    })
}

// ---------------------------------------------------------------------------------------------
// The evaluator
// ---------------------------------------------------------------------------------------------

struct Ev<'a> {
    ast: &'a Ast,
    sm: &'a ScopeManager,
    depth: u32,
}

const MAX_DEPTH: u32 = 150;

fn literal_value(ast: &Ast, node: NodeId) -> Res<V> {
    if let Some(r) = ast.other(node, "regex") {
        let p = r
            .get("pattern")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();
        let f = r
            .get("flags")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();
        return Ok(V::RegExp(Rc::new((p, f))));
    }
    if let Some(b) = ast.str(node, "bigint") {
        return parse_bigint(b).map(V::BigInt).ok_or(Stop::Unknown);
    }
    Ok(match ast.prop(node, "value") {
        Some(Prop::Str(s)) => V::Str(s.clone()),
        Some(Prop::Num(n)) => V::Num(*n),
        Some(Prop::Bool(b)) => V::Bool(*b),
        Some(Prop::Null) | None => V::Null,
        _ => return Err(Stop::Unknown),
    })
}

fn parse_bigint(s: &str) -> Option<i128> {
    let s = s.replace('_', "");
    let (radix, digits) = match s.get(..2) {
        Some("0x") | Some("0X") => (16, &s[2..]),
        Some("0o") | Some("0O") => (8, &s[2..]),
        Some("0b") | Some("0B") => (2, &s[2..]),
        _ => (10, s.as_str()),
    };
    let v = i128::from_str_radix(digits, radix).ok()?;
    if v.unsigned_abs() > (1u128 << 100) {
        return None;
    }
    Some(v)
}

impl Ev<'_> {
    fn r(&mut self, node: Option<NodeId>, scope: Option<ScopeId>) -> Res<Option<StaticValue>> {
        let Some(node) = node else { return Ok(None) };
        if self.depth >= MAX_DEPTH {
            return Err(Stop::Unknown);
        }
        self.depth += 1;
        let out = self.op(node, scope);
        self.depth -= 1;
        out
    }

    fn elements(
        &mut self,
        list: &[Option<NodeId>],
        scope: Option<ScopeId>,
    ) -> Res<Option<Vec<Option<V>>>> {
        let ast = self.ast;
        let mut out: Vec<Option<V>> = Vec::new();
        for (i, el) in list.iter().enumerate() {
            match el {
                None => out.resize(i + 1, None),
                Some(e) if ast.is(*e, "SpreadElement") => {
                    let Some(a) = self.r(ast.child(*e, "argument"), scope)? else {
                        return Ok(None);
                    };
                    out.extend(iterate(&a.value)?.into_iter().map(Some));
                }
                Some(e) => {
                    let Some(v) = self.r(Some(*e), scope)? else {
                        return Ok(None);
                    };
                    out.push(Some(v.value));
                }
            }
        }
        Ok(Some(out))
    }

    fn property_name_value(
        &mut self,
        node: NodeId,
        scope: Option<ScopeId>,
    ) -> Res<Option<StaticValue>> {
        let ast = self.ast;
        let name = if ast.is(node, "Property") {
            ast.child(node, "key")
        } else {
            ast.child(node, "property")
        };
        let Some(name) = name else { return Ok(None) };
        if ast.bool(node, "computed") {
            return self.r(Some(name), scope);
        }
        if ast.is(name, "Identifier") {
            return Ok(sv(V::Str(ast.name(name).unwrap_or("").to_string())));
        }
        if ast.is(name, "Literal") {
            if let Some(b) = ast.str(name, "bigint") {
                return Ok(sv(V::Str(b.to_string())));
            }
            return Ok(sv(V::Str(to_string(&literal_value(ast, name)?)?)));
        }
        Ok(None)
    }

    fn has_mutation_in_property(&mut self, var: VarId, scope: Option<ScopeId>) -> Res<bool> {
        let ast = self.ast;
        let sm = self.sm;
        for &r in &sm.var(var).references {
            let mut node = sm.reference(r).identifier;
            while let Some(p) = ast.parent(node)
                && ast.is(p, "MemberExpression")
            {
                node = p;
            }
            let Some(parent) = ast.parent(node) else {
                continue;
            };
            if (ast.is(parent, "AssignmentExpression") && ast.child(parent, "left") == Some(node))
                || (ast.is(parent, "UpdateExpression")
                    && ast.child(parent, "argument") == Some(node))
            {
                return Ok(true);
            }
            if ast.is(parent, "CallExpression")
                && ast.child(parent, "callee") == Some(node)
                && ast.is(node, "MemberExpression")
            {
                let name = self.property_name_value(node, scope)?;
                if let Some(StaticValue {
                    value: V::Str(n), ..
                }) = name
                    && matches!(
                        n.as_str(),
                        "copyWithin"
                            | "fill"
                            | "pop"
                            | "push"
                            | "reverse"
                            | "shift"
                            | "sort"
                            | "splice"
                            | "unshift"
                    )
                {
                    return Ok(true);
                }
            }
        }
        Ok(false)
    }

    fn identifier(&mut self, node: NodeId, scope: Option<ScopeId>) -> Res<Option<StaticValue>> {
        let Some(initial) = scope else {
            return Ok(None);
        };
        let ast = self.ast;
        let sm = self.sm;
        let Some(var) = find_variable(sm, ast, initial, node) else {
            return Ok(None);
        };
        let v = sm.var(var);
        if v.defs.is_empty() && BUILTIN_NAMES.contains(&v.name.as_str()) {
            return Ok(global_value(&v.name).map(|value| StaticValue {
                value,
                optional: false,
            }));
        }
        if v.defs.len() != 1 {
            return Ok(None);
        }
        let def = &v.defs[0];
        if def.parent.is_none() || def.kind != DefType::Variable {
            return Ok(None);
        }
        let is_const = def.decl_kind.as_deref() == Some("const") || {
            let refs = &v.references;
            let inits = refs
                .iter()
                .filter(|r| sm.reference(**r).init == Some(true))
                .count();
            let reads = refs
                .iter()
                .filter(|r| sm.reference(**r).is_read_only())
                .count();
            inits == 1 && reads + inits == refs.len()
        };
        if !is_const {
            return Ok(None);
        }
        let decl = def.node;
        if !ast
            .child(decl, "id")
            .is_some_and(|id| ast.is(id, "Identifier"))
        {
            return Ok(None);
        }
        let init = self.r(ast.child(decl, "init"), scope)?;
        if let Some(i) = &init
            && matches!(
                i.value,
                V::Arr(_) | V::Obj(_) | V::RegExp(_) | V::Builtin(_)
            )
            && i.value.type_of() == "object"
            && self.has_mutation_in_property(var, scope)?
        {
            return Ok(None);
        }
        Ok(init)
    }

    fn op(&mut self, node: NodeId, scope: Option<ScopeId>) -> Res<Option<StaticValue>> {
        let ast = self.ast;
        let op = ast.str(node, "operator").unwrap_or("");
        match ast.kind(node) {
            "ArrayExpression" => {
                Ok(self
                    .elements(ast.list(node, "elements"), scope)?
                    .map(|e| StaticValue {
                        value: V::Arr(Rc::new(e)),
                        optional: false,
                    }))
            }
            "AssignmentExpression" => {
                if op == "=" {
                    self.r(ast.child(node, "right"), scope)
                } else {
                    Ok(None)
                }
            }
            "BinaryExpression" => {
                if op == "in" || op == "instanceof" {
                    return Ok(None);
                }
                let l = self.r(ast.child(node, "left"), scope)?;
                let r = self.r(ast.child(node, "right"), scope)?;
                match (l, r) {
                    (Some(l), Some(r)) => Ok(sv(binary(op, &l.value, &r.value)?)),
                    _ => Ok(None),
                }
            }
            "CallExpression" => self.call(node, scope),
            "ConditionalExpression" => {
                let Some(t) = self.r(ast.child(node, "test"), scope)? else {
                    return Ok(None);
                };
                let branch = if t.value.truthy() {
                    "consequent"
                } else {
                    "alternate"
                };
                self.r(ast.child(node, branch), scope)
            }
            "ExpressionStatement" => self.r(ast.child(node, "expression"), scope),
            "Identifier" => self.identifier(node, scope),
            "Literal" => Ok(sv(literal_value(ast, node)?)),
            "LogicalExpression" => {
                let Some(l) = self.r(ast.child(node, "left"), scope)? else {
                    return Ok(None);
                };
                if (op == "||" && l.value.truthy())
                    || (op == "&&" && !l.value.truthy())
                    || (op == "??" && !l.value.is_nullish())
                {
                    return Ok(Some(l));
                }
                self.r(ast.child(node, "right"), scope)
            }
            "MemberExpression" => {
                if ast
                    .child(node, "property")
                    .is_some_and(|p| ast.is(p, "PrivateIdentifier"))
                {
                    return Ok(None);
                }
                let Some(obj) = self.r(ast.child(node, "object"), scope)? else {
                    return Ok(None);
                };
                if obj.value.is_nullish() && (obj.optional || ast.bool(node, "optional")) {
                    return Ok(Some(StaticValue {
                        value: V::Undefined,
                        optional: true,
                    }));
                }
                let Some(prop) = self.property_name_value(node, scope)? else {
                    return Ok(None);
                };
                let key = to_string(&prop.value)?;
                match get_prop(&obj.value, &key)? {
                    Got::Val(v) => Ok(sv(v)),
                    Got::Getter => Ok(None),
                }
            }
            "ChainExpression" => {
                Ok(self
                    .r(ast.child(node, "expression"), scope)?
                    .map(|e| StaticValue {
                        value: e.value,
                        optional: false,
                    }))
            }
            "NewExpression" => {
                let callee = self.r(ast.child(node, "callee"), scope)?;
                let args = self.elements(ast.list(node, "arguments"), scope)?;
                if let (Some(c), Some(_)) = (callee, args)
                    && let V::Builtin(p) = c.value
                    && is_call_allowed(p)
                {
                    return Err(Stop::Unknown);
                }
                Ok(None)
            }
            "ObjectExpression" => self.object(node, scope),
            "SequenceExpression" => self.r(
                ast.list(node, "expressions").last().copied().flatten(),
                scope,
            ),
            "TaggedTemplateExpression" => {
                let tag = self.r(ast.child(node, "tag"), scope)?;
                let Some(quasi) = ast.child(node, "quasi") else {
                    return Ok(None);
                };
                let exprs = self.elements(ast.list(quasi, "expressions"), scope)?;
                if let (Some(t), Some(exprs)) = (tag, exprs)
                    && matches!(t.value, V::Builtin("String.raw"))
                {
                    let raws: Vec<&str> = ast
                        .nodes_of(quasi, "quasis")
                        .map(|q| crate::utils::template_element_raw(ast, q).unwrap_or(""))
                        .collect();
                    let mut out = String::new();
                    for (i, r) in raws.iter().enumerate() {
                        out.push_str(r);
                        if i + 1 < raws.len()
                            && let Some(e) = exprs.get(i)
                        {
                            out.push_str(&to_string(e.as_ref().unwrap_or(&V::Undefined))?);
                        }
                    }
                    return Ok(sv(V::Str(out)));
                }
                Ok(None)
            }
            "TemplateLiteral" => {
                let Some(exprs) = self.elements(ast.list(node, "expressions"), scope)? else {
                    return Ok(None);
                };
                let quasis: Vec<NodeId> = ast.nodes_of(node, "quasis").collect();
                let cooked = |i: usize| -> Res<&str> {
                    quasis
                        .get(i)
                        .and_then(|q| crate::utils::template_element_cooked(ast, *q))
                        .ok_or(Stop::Unknown)
                };
                let mut value = cooked(0)?.to_string();
                for (i, e) in exprs.iter().enumerate() {
                    value.push_str(&to_string(&to_primitive(
                        e.as_ref().unwrap_or(&V::Undefined),
                    )?)?);
                    value.push_str(cooked(i + 1)?);
                }
                Ok(sv(V::Str(value)))
            }
            "UnaryExpression" => {
                if op == "delete" {
                    return Ok(None);
                }
                if op == "void" {
                    return Ok(sv(V::Undefined));
                }
                let Some(a) = self.r(ast.child(node, "argument"), scope)? else {
                    return Ok(None);
                };
                let v = a.value;
                Ok(match op {
                    "-" => sv(match to_numeric(&v)? {
                        Numeric::N(n) => V::Num(-n),
                        Numeric::B(b) => V::BigInt(-b),
                    }),
                    "+" => sv(V::Num(to_number(&v)?)),
                    "!" => sv(V::Bool(!v.truthy())),
                    "~" => sv(match to_numeric(&v)? {
                        Numeric::N(n) => V::Num(f64::from(!to_int32(n))),
                        Numeric::B(b) => V::BigInt(!b),
                    }),
                    "typeof" => sv(V::Str(v.type_of().to_string())),
                    _ => None,
                })
            }
            _ => Ok(None),
        }
    }

    fn call(&mut self, node: NodeId, scope: Option<ScopeId>) -> Res<Option<StaticValue>> {
        let ast = self.ast;
        let Some(args) = self.elements(ast.list(node, "arguments"), scope)? else {
            return Ok(None);
        };
        let args: Vec<V> = args
            .into_iter()
            .map(|a| a.unwrap_or(V::Undefined))
            .collect();
        let Some(callee) = ast.child(node, "callee") else {
            return Ok(None);
        };
        let optional = ast.bool(node, "optional");
        let invoke = |f: &V, this: &V| -> Res<Option<StaticValue>> {
            let V::Builtin(p) = f else { return Ok(None) };
            if is_call_allowed(p) {
                return Ok(sv(call_builtin(p, this, &args)?));
            }
            if is_pass_through(p) {
                return Ok(sv(arg(&args, 0)));
            }
            Ok(None)
        };
        if ast.is(callee, "MemberExpression") {
            if ast
                .child(callee, "property")
                .is_some_and(|p| ast.is(p, "PrivateIdentifier"))
            {
                return Ok(None);
            }
            let Some(obj) = self.r(ast.child(callee, "object"), scope)? else {
                return Ok(None);
            };
            if obj.value.is_nullish() && (obj.optional || optional) {
                return Ok(Some(StaticValue {
                    value: V::Undefined,
                    optional: true,
                }));
            }
            let Some(prop) = self.property_name_value(callee, scope)? else {
                return Ok(None);
            };
            let key = to_string(&prop.value)?;
            return match get_prop(&obj.value, &key)? {
                Got::Val(f) => invoke(&f, &obj.value),
                Got::Getter => Ok(None),
            };
        }
        let Some(c) = self.r(Some(callee), scope)? else {
            return Ok(None);
        };
        if c.value.is_nullish() && optional {
            return Ok(Some(StaticValue {
                value: V::Undefined,
                optional: true,
            }));
        }
        invoke(&c.value, &V::Undefined)
    }

    fn object(&mut self, node: NodeId, scope: Option<ScopeId>) -> Res<Option<StaticValue>> {
        let ast = self.ast;
        let mut obj: Vec<(String, V)> = Vec::new();
        for p in ast.nodes_of(node, "properties") {
            if ast.is(p, "Property") {
                if ast.str(p, "kind") != Some("init") {
                    return Ok(None);
                }
                let key = self.property_name_value(p, scope)?;
                let value = self.r(ast.child(p, "value"), scope)?;
                let (Some(k), Some(v)) = (key, value) else {
                    return Ok(None);
                };
                let k = to_string(&k.value)?;
                if k == "__proto__" {
                    return Err(Stop::Unknown);
                }
                set_key(&mut obj, k, v.value);
            } else if ast.is(p, "SpreadElement") {
                let Some(a) = self.r(ast.child(p, "argument"), scope)? else {
                    return Ok(None);
                };
                match &a.value {
                    V::Undefined
                    | V::Null
                    | V::Bool(_)
                    | V::Num(_)
                    | V::BigInt(_)
                    | V::RegExp(_) => {}
                    V::Builtin(_) => return Err(Stop::Unknown),
                    other => {
                        for (k, v) in object_keys(other)? {
                            set_key(&mut obj, k, v);
                        }
                    }
                }
            } else {
                return Ok(None);
            }
        }
        Ok(sv(V::Obj(Rc::new(obj))))
    }
}

fn set_key(obj: &mut Vec<(String, V)>, k: String, v: V) {
    if let Some(slot) = obj.iter_mut().find(|(x, _)| *x == k) {
        slot.1 = v;
        return;
    }
    if let Some(i) = is_array_index(&k) {
        let pos = obj
            .iter()
            .position(|(x, _)| is_array_index(x).is_none_or(|j| j > i))
            .unwrap_or(obj.len());
        obj.insert(pos, (k, v));
    } else {
        obj.push((k, v));
    }
}

fn iterate(v: &V) -> Res<Vec<V>> {
    match v {
        V::Arr(a) => Ok(a
            .iter()
            .map(|e| e.clone().unwrap_or(V::Undefined))
            .collect()),
        V::Str(s) => Ok(s.chars().map(|c| V::Str(c.to_string())).collect()),
        V::Builtin(_) => Err(Stop::Unknown),
        _ => Err(Stop::Throw),
    }
}

// ---------------------------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------------------------

/// `getStaticValue(node, initialScope)`. `Ok(None)` is `null`.
pub fn get_static_value(
    ast: &Ast,
    sm: &ScopeManager,
    node: NodeId,
    initial_scope: Option<ScopeId>,
) -> Result<Option<StaticValue>, Unknown> {
    let mut ev = Ev { ast, sm, depth: 0 };
    match ev.r(Some(node), initial_scope) {
        Ok(v) => Ok(v),
        Err(Stop::Throw) => Ok(None),
        Err(Stop::Unknown) => Err(Unknown),
    }
}

/// `getStringIfConstant(node, initialScope)`.
pub fn get_string_if_constant(
    ast: &Ast,
    sm: &ScopeManager,
    node: Option<NodeId>,
    initial_scope: Option<ScopeId>,
) -> Result<Option<String>, Unknown> {
    let Some(node) = node else { return Ok(None) };
    match get_static_value(ast, sm, node, initial_scope)? {
        Some(v) => match to_string(&v.value) {
            Ok(s) => Ok(Some(s)),
            Err(Stop::Throw) => Ok(None),
            Err(Stop::Unknown) => Err(Unknown),
        },
        None => Ok(None),
    }
}

/// `getPropertyName(node, initialScope)` (`None` is `null` or `undefined`).
pub fn get_property_name(
    ast: &Ast,
    sm: &ScopeManager,
    node: NodeId,
    initial_scope: Option<ScopeId>,
) -> Result<Option<String>, Unknown> {
    match ast.kind(node) {
        "MemberExpression" => {
            let Some(p) = ast.child(node, "property") else {
                return Ok(None);
            };
            if ast.bool(node, "computed") {
                return get_string_if_constant(ast, sm, Some(p), initial_scope);
            }
            if ast.is(p, "PrivateIdentifier") {
                return Ok(None);
            }
            Ok(ast.name(p).map(str::to_string))
        }
        "Property" | "MethodDefinition" | "PropertyDefinition" => {
            let Some(k) = ast.child(node, "key") else {
                return Ok(None);
            };
            if ast.bool(node, "computed") {
                return get_string_if_constant(ast, sm, Some(k), initial_scope);
            }
            if ast.is(k, "Literal") {
                return match to_string(&literal_value(ast, k).map_err(|_| Unknown)?) {
                    Ok(s) => Ok(Some(s)),
                    Err(_) => Err(Unknown),
                };
            }
            if ast.is(k, "PrivateIdentifier") {
                return Ok(None);
            }
            Ok(ast.name(k).map(str::to_string))
        }
        _ => Ok(None),
    }
}

// ---------------------------------------------------------------------------------------------
// ReferenceTracker
// ---------------------------------------------------------------------------------------------

/// A trace map: `[READ]`, `[CALL]`, `[CONSTRUCT]` infos, `[ESM]`, and the string keys in order.
#[derive(Debug, Clone, Default)]
pub struct TraceMap {
    pub read: Option<usize>,
    pub call: Option<usize>,
    pub construct: Option<usize>,
    pub esm: bool,
    pub keys: Vec<(String, TraceMap)>,
}

impl TraceMap {
    pub fn get(&self, key: &str) -> Option<&TraceMap> {
        self.keys.iter().find(|(k, _)| k == key).map(|(_, v)| v)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RefType {
    Read,
    Call,
    Construct,
}

#[derive(Debug, Clone)]
pub struct TrackedRef {
    pub node: NodeId,
    pub path: Vec<String>,
    pub kind: RefType,
    pub info: usize,
}

pub struct ReferenceTracker<'a> {
    ast: &'a Ast,
    sm: &'a ScopeManager,
    global_scope: ScopeId,
    legacy: bool,
    global_object_names: Vec<String>,
    variable_stack: Vec<VarId>,
    out: Vec<TrackedRef>,
    unknown: bool,
}

impl<'a> ReferenceTracker<'a> {
    /// `new ReferenceTracker(globalScope)` with the default options (`mode: "strict"`).
    pub fn new(ast: &'a Ast, sm: &'a ScopeManager, global_scope: ScopeId) -> Self {
        ReferenceTracker {
            ast,
            sm,
            global_scope,
            legacy: false,
            global_object_names: ["global", "globalThis", "self", "window"]
                .iter()
                .map(|s| s.to_string())
                .collect(),
            variable_stack: Vec::new(),
            out: Vec::new(),
            unknown: false,
        }
    }

    pub fn with_options(mut self, legacy: bool, global_object_names: Option<Vec<String>>) -> Self {
        self.legacy = legacy;
        if let Some(g) = global_object_names {
            self.global_object_names = g;
        }
        self
    }

    fn finish(&mut self) -> Result<Vec<TrackedRef>, Unknown> {
        if self.unknown {
            return Err(Unknown);
        }
        Ok(std::mem::take(&mut self.out))
    }

    fn is_modified_global(&self, v: Option<VarId>) -> bool {
        match v {
            None => true,
            Some(v) => {
                let var = self.sm.var(v);
                !var.defs.is_empty()
                    || var
                        .references
                        .iter()
                        .any(|r| self.sm.reference(*r).is_write())
            }
        }
    }

    /// `iterateGlobalReferences(traceMap)`.
    pub fn iterate_global_references(
        &mut self,
        trace_map: &TraceMap,
    ) -> Result<Vec<TrackedRef>, Unknown> {
        self.global_refs(trace_map);
        self.finish()
    }

    fn global_refs(&mut self, trace_map: &TraceMap) {
        for (key, next) in &trace_map.keys {
            let v = self.sm.lookup(self.global_scope, key);
            if self.is_modified_global(v) {
                continue;
            }
            if let Some(v) = v {
                self.variable_refs(v, vec![key.clone()], next, true);
            }
        }
        for key in self.global_object_names.clone() {
            let v = self.sm.lookup(self.global_scope, &key);
            if self.is_modified_global(v) {
                continue;
            }
            if let Some(v) = v {
                self.variable_refs(v, Vec::new(), trace_map, false);
            }
        }
    }

    /// `iterateCjsReferences(traceMap)`.
    pub fn iterate_cjs_references(
        &mut self,
        trace_map: &TraceMap,
    ) -> Result<Vec<TrackedRef>, Unknown> {
        let require = TraceMap {
            keys: vec![(
                "require".into(),
                TraceMap {
                    call: Some(0),
                    ..Default::default()
                },
            )],
            ..Default::default()
        };
        self.global_refs(&require);
        let calls = std::mem::take(&mut self.out);
        for c in calls {
            let first = self
                .ast
                .list(c.node, "arguments")
                .first()
                .copied()
                .flatten();
            let key = match get_string_if_constant(self.ast, self.sm, first, None) {
                Ok(k) => k,
                Err(_) => {
                    self.unknown = true;
                    None
                }
            };
            let Some(key) = key else { continue };
            let Some(next) = trace_map.get(&key) else {
                continue;
            };
            let path = vec![key.clone()];
            if let Some(info) = next.read {
                self.out.push(TrackedRef {
                    node: c.node,
                    path: path.clone(),
                    kind: RefType::Read,
                    info,
                });
            }
            self.property_refs(c.node, path, next);
        }
        self.finish()
    }

    /// `iterateEsmReferences(traceMap)`.
    pub fn iterate_esm_references(
        &mut self,
        trace_map: &TraceMap,
    ) -> Result<Vec<TrackedRef>, Unknown> {
        let ast = self.ast;
        let program = self.sm.scope(self.global_scope).block;
        for node in ast.nodes_of(program, "body") {
            if !matches!(
                ast.kind(node),
                "ImportDeclaration"
                    | "ExportAllDeclaration"
                    | "ExportDefaultDeclaration"
                    | "ExportNamedDeclaration"
            ) {
                continue;
            }
            let Some(src) = ast.child(node, "source") else {
                continue;
            };
            let Some(module_id) = ast.str(src, "value") else {
                continue;
            };
            let Some(next) = trace_map.get(module_id) else {
                continue;
            };
            let path = vec![module_id.to_string()];
            if let Some(info) = next.read {
                self.out.push(TrackedRef {
                    node,
                    path: path.clone(),
                    kind: RefType::Read,
                    info,
                });
            }
            if ast.is(node, "ExportAllDeclaration") {
                for (key, export) in &next.keys {
                    if let Some(info) = export.read {
                        let mut p = path.clone();
                        p.push(key.clone());
                        self.out.push(TrackedRef {
                            node,
                            path: p,
                            kind: RefType::Read,
                            info,
                        });
                    }
                }
                continue;
            }
            for spec in ast.nodes_of(node, "specifiers") {
                if next.esm {
                    self.import_refs(spec, path.clone(), next);
                } else {
                    let mut wrapped = TraceMap {
                        keys: vec![("default".into(), next.clone())],
                        ..Default::default()
                    };
                    if self.legacy {
                        wrapped.read = next.read;
                        wrapped.call = next.call;
                        wrapped.construct = next.construct;
                        for (k, v) in &next.keys {
                            if k == "default" {
                                wrapped.keys[0].1 = v.clone();
                            } else {
                                wrapped.keys.push((k.clone(), v.clone()));
                            }
                        }
                    }
                    let start = self.out.len();
                    self.import_refs(spec, path.clone(), &wrapped);
                    let produced: Vec<TrackedRef> = self.out.drain(start..).collect();
                    for mut r in produced {
                        r.path = r
                            .path
                            .into_iter()
                            .enumerate()
                            .filter(|(i, n)| !(*i == 1 && n == "default"))
                            .map(|(_, n)| n)
                            .collect();
                        if r.path.len() >= 2 || r.kind != RefType::Read {
                            self.out.push(r);
                        }
                    }
                }
            }
        }
        self.finish()
    }

    fn variable_refs(
        &mut self,
        var: VarId,
        path: Vec<String>,
        trace_map: &TraceMap,
        should_report: bool,
    ) {
        if self.variable_stack.contains(&var) {
            return;
        }
        self.variable_stack.push(var);
        let refs = self.sm.var(var).references.clone();
        for r in refs {
            let reference = self.sm.reference(r);
            if !reference.is_read() {
                continue;
            }
            let node = reference.identifier;
            if should_report && let Some(info) = trace_map.read {
                self.out.push(TrackedRef {
                    node,
                    path: path.clone(),
                    kind: RefType::Read,
                    info,
                });
            }
            self.property_refs(node, path.clone(), trace_map);
        }
        self.variable_stack.pop();
    }

    fn is_pass_through(&self, node: NodeId) -> bool {
        let ast = self.ast;
        let Some(parent) = ast.parent(node) else {
            return false;
        };
        match ast.kind(parent) {
            "ConditionalExpression" => {
                ast.child(parent, "consequent") == Some(node)
                    || ast.child(parent, "alternate") == Some(node)
            }
            "LogicalExpression" | "ChainExpression" => true,
            "SequenceExpression" => {
                ast.list(parent, "expressions").last().copied().flatten() == Some(node)
            }
            _ => false,
        }
    }

    fn property_refs(&mut self, root: NodeId, path: Vec<String>, trace_map: &TraceMap) {
        let ast = self.ast;
        let mut node = root;
        while self.is_pass_through(node) {
            let Some(p) = ast.parent(node) else { break };
            node = p;
        }
        let Some(parent) = ast.parent(node) else {
            return;
        };
        match ast.kind(parent) {
            "MemberExpression" => {
                if ast.child(parent, "object") == Some(node) {
                    let key = match get_property_name(ast, self.sm, parent, None) {
                        Ok(k) => k,
                        Err(_) => {
                            self.unknown = true;
                            return;
                        }
                    };
                    let Some(key) = key else { return };
                    let Some(next) = trace_map.get(&key) else {
                        return;
                    };
                    let mut path = path;
                    path.push(key);
                    if let Some(info) = next.read {
                        self.out.push(TrackedRef {
                            node: parent,
                            path: path.clone(),
                            kind: RefType::Read,
                            info,
                        });
                    }
                    self.property_refs(parent, path, next);
                }
            }
            "CallExpression" => {
                if ast.child(parent, "callee") == Some(node)
                    && let Some(info) = trace_map.call
                {
                    self.out.push(TrackedRef {
                        node: parent,
                        path,
                        kind: RefType::Call,
                        info,
                    });
                }
            }
            "NewExpression" => {
                if ast.child(parent, "callee") == Some(node)
                    && let Some(info) = trace_map.construct
                {
                    self.out.push(TrackedRef {
                        node: parent,
                        path,
                        kind: RefType::Construct,
                        info,
                    });
                }
            }
            "AssignmentExpression" => {
                if ast.child(parent, "right") == Some(node) {
                    if let Some(left) = ast.child(parent, "left") {
                        self.lhs_refs(left, path.clone(), trace_map);
                    }
                    self.property_refs(parent, path, trace_map);
                }
            }
            "AssignmentPattern" => {
                if ast.child(parent, "right") == Some(node)
                    && let Some(left) = ast.child(parent, "left")
                {
                    self.lhs_refs(left, path, trace_map);
                }
            }
            "VariableDeclarator" => {
                if ast.child(parent, "init") == Some(node)
                    && let Some(id) = ast.child(parent, "id")
                {
                    self.lhs_refs(id, path, trace_map);
                }
            }
            _ => {}
        }
    }

    fn lhs_refs(&mut self, pattern: NodeId, path: Vec<String>, trace_map: &TraceMap) {
        let ast = self.ast;
        match ast.kind(pattern) {
            "Identifier" => {
                if let Some(v) = find_variable(self.sm, ast, self.global_scope, pattern) {
                    self.variable_refs(v, path, trace_map, false);
                }
            }
            "ObjectPattern" => {
                for prop in ast.nodes_of(pattern, "properties") {
                    let key = match get_property_name(ast, self.sm, prop, None) {
                        Ok(k) => k,
                        Err(_) => {
                            self.unknown = true;
                            continue;
                        }
                    };
                    let Some(key) = key else { continue };
                    let Some(next) = trace_map.get(&key) else {
                        continue;
                    };
                    let mut next_path = path.clone();
                    next_path.push(key);
                    if let Some(info) = next.read {
                        self.out.push(TrackedRef {
                            node: prop,
                            path: next_path.clone(),
                            kind: RefType::Read,
                            info,
                        });
                    }
                    if let Some(v) = ast.child(prop, "value") {
                        self.lhs_refs(v, next_path, next);
                    }
                }
            }
            "AssignmentPattern" => {
                if let Some(left) = ast.child(pattern, "left") {
                    self.lhs_refs(left, path, trace_map);
                }
            }
            _ => {}
        }
    }

    fn import_refs(&mut self, spec: NodeId, path: Vec<String>, trace_map: &TraceMap) {
        let ast = self.ast;
        let name_of = |n: Option<NodeId>| -> Option<String> {
            let n = n?;
            if ast.is(n, "Identifier") {
                ast.name(n).map(str::to_string)
            } else {
                ast.str(n, "value").map(str::to_string)
            }
        };
        match ast.kind(spec) {
            "ImportSpecifier" | "ImportDefaultSpecifier" => {
                let key = if ast.is(spec, "ImportDefaultSpecifier") {
                    Some("default".to_string())
                } else {
                    name_of(ast.child(spec, "imported"))
                };
                let Some(key) = key else { return };
                let Some(next) = trace_map.get(&key) else {
                    return;
                };
                let mut path = path;
                path.push(key);
                if let Some(info) = next.read {
                    self.out.push(TrackedRef {
                        node: spec,
                        path: path.clone(),
                        kind: RefType::Read,
                        info,
                    });
                }
                if let Some(local) = ast.child(spec, "local")
                    && let Some(v) = find_variable(self.sm, ast, self.global_scope, local)
                {
                    self.variable_refs(v, path, next, false);
                }
            }
            "ImportNamespaceSpecifier" => {
                if let Some(local) = ast.child(spec, "local")
                    && let Some(v) = find_variable(self.sm, ast, self.global_scope, local)
                {
                    self.variable_refs(v, path, trace_map, false);
                }
            }
            "ExportSpecifier" => {
                let Some(key) = name_of(ast.child(spec, "local")) else {
                    return;
                };
                let Some(next) = trace_map.get(&key) else {
                    return;
                };
                let mut path = path;
                path.push(key);
                if let Some(info) = next.read {
                    self.out.push(TrackedRef {
                        node: spec,
                        path,
                        kind: RefType::Read,
                        info,
                    });
                }
            }
            _ => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::source::{ParseOptions, SourceCode, SourceType};

    fn eval(code: &str) -> Result<Option<String>, Unknown> {
        let sc = SourceCode::parse(
            code,
            &ParseOptions {
                source_type: SourceType::Module,
                jsx: false,
                global_return: false,
            },
        )
        .ok()
        .unwrap_or_else(|| panic!("parse {code}"));
        let o = crate::linter::Options {
            rules: Vec::new(),
            source_type: SourceType::Module,
            jsx: false,
            global_return: false,
            globals: Vec::new(),
            allow_inline_config: true,
            report_unused_disable_directives: "off",
            report_unused_inline_configs: "off",
            fix: false,
            fix_types: None,
            quiet: false,
        };
        let data = crate::scope::ScopeData::new(&sc, &o, &[], &[], &[])
            .unwrap_or_else(|_| panic!("scope"));
        let sm = data.manager(&sc);
        let body: Vec<NodeId> = sc.ast.nodes_of(sc.root, "body").collect();
        let last = *body.last().unwrap_or_else(|| panic!("empty"));
        let scope = sm.get_scope(&sc.ast, last);
        let v = get_static_value(&sc.ast, sm, last, Some(scope))?;
        Ok(v.map(|v| match &v.value {
            V::Str(s) => format!("{}:{s}", v.value.type_of()),
            other => format!(
                "{}:{}",
                other.type_of(),
                to_string(other).unwrap_or_else(|_| "?".into())
            ),
        }))
    }

    fn ok(code: &str) -> String {
        match eval(code) {
            Ok(Some(s)) => s,
            Ok(None) => "null".into(),
            Err(_) => "unknown".into(),
        }
    }

    #[test]
    fn numbers_and_strings() {
        assert_eq!(ok("1 + 2"), "number:3");
        assert_eq!(ok("'a' + 1"), "string:a1");
        assert_eq!(ok("1 + '2' - 1"), "number:11");
        assert_eq!(ok("0.1 + 0.2"), "number:0.30000000000000004");
        assert_eq!(ok("1e21 + 0"), "number:1e+21");
        assert_eq!(
            ok("123456789012345680000 + 0"),
            "number:123456789012345680000"
        );
        assert_eq!(ok("1 / 3"), "number:0.3333333333333333");
        assert_eq!(ok("-0 + ''"), "string:0");
        assert_eq!(ok("0.000001 + ''"), "string:0.000001");
        assert_eq!(ok("1e-7 + ''"), "string:1e-7");
        assert_eq!(ok("0 / 0"), "number:NaN");
        assert_eq!(ok("-1 / 0 + ''"), "string:-Infinity");
        assert_eq!(ok("5 % -3"), "number:2");
        assert_eq!(ok("-1 >>> 0"), "number:4294967295");
        assert_eq!(ok("1 << 33"), "number:2");
        assert_eq!(ok("2 ** 10"), "number:1024");
        assert_eq!(ok("~2"), "number:-3");
    }

    #[test]
    fn coercions_and_comparisons() {
        assert_eq!(ok("[1, 2] + ''"), "string:1,2");
        assert_eq!(ok("[] + {}"), "string:[object Object]");
        assert_eq!(ok("+[]"), "number:0");
        assert_eq!(ok("+[5]"), "number:5");
        assert_eq!(ok("+' 0x1F '"), "number:31");
        assert_eq!(ok("+'1e'"), "number:NaN");
        assert_eq!(ok("null == undefined"), "boolean:true");
        assert_eq!(ok("'1' == 1"), "boolean:true");
        assert_eq!(ok("[] == false"), "boolean:true");
        assert_eq!(ok("NaN == NaN"), "boolean:false");
        assert_eq!(ok("'b' > 'a'"), "boolean:true");
        assert_eq!(ok("'10' < '9'"), "boolean:true");
        assert_eq!(ok("undefined <= 0"), "boolean:false");
        assert_eq!(ok("[] === []"), "boolean:false");
        assert_eq!(ok("typeof null"), "string:object");
        assert_eq!(ok("typeof Math.max"), "string:function");
        assert_eq!(ok("typeof Math"), "string:object");
        assert_eq!(ok("typeof 1n"), "string:bigint");
        assert_eq!(ok("-1n + ''"), "string:-1");
        assert_eq!(ok("1n + 1"), "null");
        assert_eq!(ok("void foo"), "undefined:undefined");
    }

    #[test]
    fn templates_members_and_calls() {
        assert_eq!(ok("`a${1}b${[2, 3]}`"), "string:a1b2,3");
        assert_eq!(ok("'abc'.length"), "number:3");
        assert_eq!(ok("'abc'[1]"), "string:b");
        assert_eq!(ok("'abc'.slice(-2)"), "string:bc");
        assert_eq!(ok("'a-b'.indexOf('-')"), "number:1");
        assert_eq!(ok("[1, 2, 3].join('-')"), "string:1-2-3");
        assert_eq!(ok("({a: 1}).a"), "number:1");
        assert_eq!(ok("({a: 1}).b"), "undefined:undefined");
        assert_eq!(ok("({}).__proto__"), "null");
        assert_eq!(ok("String(1.50)"), "string:1.5");
        assert_eq!(ok("Math.max(1, 3, 2)"), "number:3");
        assert_eq!(ok("Math.random()"), "null");
        assert_eq!(ok("Object.freeze([1])[0]"), "number:1");
        assert_eq!(ok("parseInt('08')"), "number:8");
        assert_eq!(ok("parseFloat('3.5e1x')"), "number:35");
        assert_eq!(ok("String.raw`a\\n${1}`"), "string:a\\n1");
        assert_eq!(ok("undefined.foo"), "null");
        assert_eq!(ok("undefined?.foo"), "undefined:undefined");
        assert_eq!(ok("[...'ab', ...[1]].length"), "number:3");
        assert_eq!(ok("[...[1, 2], , 3].length"), "number:3");
        assert_eq!(ok("/a/ig + ''"), "string:/a/gi");
        assert_eq!(ok("this.x"), "null");
    }

    #[test]
    fn variables() {
        assert_eq!(ok("const a = 'x'; a + a"), "string:xx");
        assert_eq!(ok("let a = 1; a"), "number:1");
        assert_eq!(ok("let a = 1; a = 2; a"), "null");
        assert_eq!(ok("const a = [1]; a.push(2); a"), "null");
        assert_eq!(ok("const a = [1]; a[0]"), "number:1");
        assert_eq!(ok("const {a} = {a: 1}; a"), "null");
        assert_eq!(ok("NaN"), "number:NaN");
        assert_eq!(ok("const a = b, b = a; a"), "unknown");
        assert_eq!(ok("const a = [3]; a.sort(); a[0]"), "null");
        assert_eq!(ok("const a = {x: 1}; a.x = 2; a.x"), "null");
        assert_eq!(ok("let Infinity = 2; Infinity"), "number:2");
        assert_eq!(
            ok("const o = {b: 2, 1: 'x', a: 1, 0: 'y'}; Object.keys(o) + ''"),
            "string:0,1,b,a"
        );
        assert_eq!(ok("const s = 'ab'; `${s}${s.length}`"), "string:ab2");
        assert_eq!(ok("Number.MAX_SAFE_INTEGER + 2"), "number:9007199254740992");
        assert_eq!(
            ok("typeof Number.parseInt === typeof parseInt && Number.parseInt === parseInt"),
            "boolean:true"
        );
        assert_eq!(ok("'abc'.foo"), "unknown");
        assert_eq!(ok("null ?? 'd'"), "string:d");
        assert_eq!(ok("0 || [] && 'x'"), "string:x");
    }
}
