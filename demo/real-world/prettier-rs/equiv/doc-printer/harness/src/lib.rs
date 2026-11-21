//! Exposes `doc_printer_rs::port` to Node for the bounded-exhaustive equivalence checks.
//! Results are encoded as `["ok", formatted, cursorNodeStart|null, cursorNodeText|null]` or
//! `["err", message]` so the JS side can compare them with Prettier's output as plain strings.

use doc_printer_rs::port;
use napi_derive::napi;
use serde_json::Value;

/// Parses JSON without serde_json's 128-level recursion limit (Prettier docs nest deeper).
fn parse<T: serde::de::DeserializeOwned>(text: &str) -> T {
    let mut de = serde_json::Deserializer::from_str(text);
    de.disable_recursion_limit();
    T::deserialize(&mut de).expect("json input")
}

fn encode(r: std::thread::Result<Result<Value, String>>) -> Value {
    match r {
        Ok(Ok(v)) => Value::Array(vec![
            "ok".into(),
            v.get("formatted").cloned().unwrap_or(Value::Null),
            v.get("cursorNodeStart").cloned().unwrap_or(Value::Null),
            v.get("cursorNodeText").cloned().unwrap_or(Value::Null),
        ]),
        Ok(Err(e)) => Value::Array(vec!["err".into(), e.into()]),
        Err(p) => {
            let m = p
                .downcast_ref::<&str>()
                .map(|s| (*s).to_string())
                .or_else(|| p.downcast_ref::<String>().cloned())
                .unwrap_or_else(|| "unknown".into());
            Value::Array(vec!["err".into(), format!("Rust panic: {m}").into()])
        }
    }
}

/// Prints one doc (JSON) under each options object in `options` (a JSON array) through the
/// port's public entry point `print_doc_to_string`, which re-reads the doc and re-propagates
/// breaks for every call, exactly like Prettier's `printDocToString`.
#[napi(js_name = "printMany")]
pub fn print_many(doc: String, options: String) -> String {
    let doc: Value = parse(&doc);
    let options: Vec<Value> = parse(&options);
    let out: Vec<Value> = options
        .into_iter()
        .map(|o| {
            let d = doc.clone();
            encode(std::panic::catch_unwind(move || {
                port::print_doc_to_string(d, o)
            }))
        })
        .collect();
    serde_json::to_string(&out).expect("encode")
}

/// Batched form: `docs` is a JSON array of docs; returns one result array per doc.
#[napi(js_name = "printBatch")]
pub fn print_batch(docs: String, options: String) -> String {
    let docs: Vec<Value> = parse(&docs);
    let options: Vec<Value> = parse(&options);
    let out: Vec<Value> = docs
        .into_iter()
        .map(|doc| {
            Value::Array(
                options
                    .iter()
                    .map(|o| {
                        let d = doc.clone();
                        let o = o.clone();
                        encode(std::panic::catch_unwind(move || {
                            port::print_doc_to_string(d, o)
                        }))
                    })
                    .collect(),
            )
        })
        .collect();
    serde_json::to_string(&out).expect("encode")
}

/// `string_width` of every Unicode scalar value, indexed by code point (surrogates get -1).
#[napi(js_name = "widthAllScalars")]
pub fn width_all_scalars() -> Vec<f64> {
    (0u32..=0x10FFFF)
        .map(|cp| match char::from_u32(cp) {
            Some(c) => port::string_width(c.encode_utf8(&mut [0; 4])),
            None => -1.0,
        })
        .collect()
}

/// `string_width` of each string.
#[napi(js_name = "widths")]
pub fn widths(texts: Vec<String>) -> Vec<f64> {
    texts.iter().map(|t| port::string_width(t)).collect()
}

mod native;

thread_local! {
    static NATIVE: std::cell::RefCell<native::Enumerator> =
        std::cell::RefCell::new(native::Enumerator::new(native::native()));
}

fn print_options(v: &Value) -> port::PrintOptions {
    let n = |k: &str| match v.get(k) {
        Some(Value::Number(x)) => x.as_f64().unwrap_or(f64::NAN),
        Some(Value::Object(m)) => match m.get("$num").and_then(Value::as_str) {
            Some("Infinity") => f64::INFINITY,
            Some("-Infinity") => f64::NEG_INFINITY,
            _ => f64::NAN,
        },
        _ => f64::NAN,
    };
    port::PrintOptions {
        print_width: n("printWidth"),
        tab_width: n("tabWidth"),
        use_tabs: v.get("useTabs").and_then(Value::as_bool).unwrap_or(false),
        end_of_line: match v.get("endOfLine").and_then(Value::as_str) {
            Some("cr") => "\r",
            Some("crlf") => "\r\n",
            _ => "\n",
        },
    }
}

/// Number of native docs of `size` (decimal string; it can pass 2^53).
#[napi(js_name = "nativeCount")]
pub fn native_count(size: u32) -> String {
    NATIVE.with(|e| e.borrow_mut().count(size as usize).to_string())
}

/// Native docs `start..start+count` of `size` (decimal strings), each printed natively under
/// `optsIndent` (docs with indentation) or `optsPlain`. Returns one entry per doc:
/// `null` when the group-id symmetry filter skips it, else `{doc, indent, rust}` with `doc` the
/// Prettier doc JSON (`$def`/`$ref` for shared nodes) and `rust` one result per option set.
#[napi(js_name = "nativeBatch")]
pub fn native_batch(
    size: u32,
    start: String,
    count: u32,
    opts_indent: String,
    opts_plain: String,
) -> String {
    let start: u128 = start.parse().expect("start");
    let oi: Vec<port::PrintOptions> = parse::<Vec<Value>>(&opts_indent)
        .iter()
        .map(print_options)
        .collect();
    let op: Vec<port::PrintOptions> = parse::<Vec<Value>>(&opts_plain)
        .iter()
        .map(print_options)
        .collect();
    let out: Vec<Value> = NATIVE.with(|e| {
        let mut e = e.borrow_mut();
        let total = e.count(size as usize);
        (0..u128::from(count))
            .map(|k| start + k)
            .take_while(|&i| i < total)
            .map(|i| {
                let d = e.nth(size as usize, i);
                if native::first_id(&d) == Some("B") {
                    return Value::Null;
                }
                let indent = native::has_indentation(&d);
                let json = native::to_value(&d).to_string();
                let opts = if indent { &oi } else { &op };
                let rust: Vec<Value> = opts
                    .iter()
                    .map(|o| {
                        match std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                            native::print_native(&d, o)
                        })) {
                            Ok(v) => v,
                            Err(_) => serde_json::json!(["err", "Rust panic"]),
                        }
                    })
                    .collect();
                serde_json::json!({"doc": json, "indent": indent, "rust": rust})
            })
            .collect()
    });
    serde_json::to_string(&out).expect("encode")
}

/// `from_value(to_value(d)) == d` for native doc `i` of `size`: is the JSON the exhaustive JSON
/// check reads an exact image of the native doc? (Sharing is not compared: `from_value` has no
/// `$def`.)
#[napi(js_name = "nativeRoundTrip")]
pub fn native_round_trip(size: u32, start: String, count: u32) -> Vec<String> {
    let start: u128 = start.parse().expect("start");
    NATIVE.with(|e| {
        let mut e = e.borrow_mut();
        let total = e.count(size as usize);
        (0..u128::from(count))
            .map(|k| start + k)
            .take_while(|&i| i < total)
            .filter_map(|i| {
                let d = e.nth(size as usize, i);
                let back = port::from_value(&native::to_value(&d), &port::Lone::default());
                (back != d).then(|| native::to_value(&d).to_string())
            })
            .collect()
    })
}
