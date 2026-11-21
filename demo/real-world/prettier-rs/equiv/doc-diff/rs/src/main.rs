//! Line-oriented server around the port, for `../driver.mjs`.
//!
//! Request (one JSON object per line):
//!   {"args": [CLI flags], "stdin": "...", "width": optional print width override,
//!    "doc": bool, "fmt": bool}
//! Response (one line):
//!   {"status": "ok" | "cli" | "empty" | "bail" | "panic", "err"?: "...",
//!    "doc"?: <doc>, "fmt"?: {"ok": "..."} | {"err": "..."}}
//!
//! `doc` is `text_to_doc` after `propagate_breaks`, in Prettier's wire JSON (the form
//! `doc_printer_rs::port::from_value` reads), except that each `Shared` subtree is written once as
//! `{"$def": k, "doc": ...}` and later as `{"$ref": k}`, so the driver can rebuild the sharing
//! and nothing is expanded here.

use std::collections::HashMap;
use std::io::{BufRead, Write};
use std::rc::Rc;

use doc_printer_rs::port::{Align, Doc};
use prettier_js_rs::Options;
use serde_json::{Map, Value, json};

/// Native docs hold Rust strings, which cannot hold lone surrogates, so every string is plain
/// JSON. (This used to read U+10F800..U+10FFFF as lone surrogates, the old `from_value` encoding,
/// which corrupted real characters in that range.)
fn str_value(s: &str) -> Value {
    Value::String(s.to_string())
}

fn num(f: f64) -> Value {
    if f.is_finite() {
        serde_json::Number::from_f64(f).map_or(Value::Null, Value::Number)
    } else if f.is_nan() {
        json!({"$num": "NaN"})
    } else if f > 0.0 {
        json!({"$num": "Infinity"})
    } else {
        json!({"$num": "-Infinity"})
    }
}

struct Ser {
    shared: HashMap<*const Doc, usize>,
}

impl Ser {
    fn obj(&mut self, ty: &str, fields: Vec<(&str, Value)>) -> Value {
        let mut m = Map::new();
        m.insert("type".into(), Value::String(ty.into()));
        for (k, v) in fields {
            m.insert(k.into(), v);
        }
        Value::Object(m)
    }

    fn doc(&mut self, d: &Doc) -> Value {
        match d {
            Doc::Str(s) => str_value(s),
            Doc::Array(items) => Value::Array(items.iter().map(|i| self.doc(i)).collect()),
            Doc::Cursor => self.obj("cursor", vec![]),
            Doc::Indent(c) => {
                let c = self.doc(c);
                self.obj("indent", vec![("contents", c)])
            }
            Doc::Align(n, c) => {
                let n = match n {
                    Align::Width(f) => num(*f),
                    Align::Text(s) => Value::String(s.clone()),
                    Align::DedentToRoot => num(f64::NEG_INFINITY),
                    Align::Root => json!({"type": "root"}),
                };
                let c = self.doc(c);
                self.obj("align", vec![("n", n), ("contents", c)])
            }
            Doc::Trim => self.obj("trim", vec![]),
            Doc::Group {
                contents,
                brk,
                expanded,
                id,
            } => {
                let mut f = vec![
                    ("contents", self.doc(contents)),
                    ("break", Value::Bool(brk.get())),
                ];
                if let Some(states) = expanded {
                    let s = states.iter().map(|s| self.doc(s)).collect();
                    f.push(("expandedStates", Value::Array(s)));
                }
                if let Some(id) = id {
                    f.push(("id", Value::String(id.clone())));
                }
                self.obj("group", f)
            }
            Doc::Fill(parts) => {
                let p = parts.iter().map(|p| self.doc(p)).collect();
                self.obj("fill", vec![("parts", Value::Array(p))])
            }
            Doc::IfBreak {
                brk,
                flat,
                group_id,
            } => {
                let mut f = vec![
                    ("breakContents", self.doc(brk)),
                    ("flatContents", self.doc(flat)),
                ];
                if let Some(id) = group_id {
                    f.push(("groupId", Value::String(id.clone())));
                }
                self.obj("if-break", f)
            }
            Doc::IndentIfBreak {
                contents,
                group_id,
                negate,
            } => {
                let mut f = vec![
                    ("contents", self.doc(contents)),
                    ("negate", Value::Bool(*negate)),
                ];
                if let Some(id) = group_id {
                    f.push(("groupId", Value::String(id.clone())));
                }
                self.obj("indent-if-break", f)
            }
            Doc::LineSuffix(c) => {
                let c = self.doc(c);
                self.obj("line-suffix", vec![("contents", c)])
            }
            Doc::LineSuffixBoundary => self.obj("line-suffix-boundary", vec![]),
            Doc::Line {
                hard,
                soft,
                literal,
            } => self.obj(
                "line",
                vec![
                    ("hard", Value::Bool(*hard)),
                    ("soft", Value::Bool(*soft)),
                    ("literal", Value::Bool(*literal)),
                ],
            ),
            Doc::Label(l, c) => {
                let c = self.doc(c);
                self.obj(
                    "label",
                    vec![("label", Value::String(l.clone())), ("contents", c)],
                )
            }
            Doc::BreakParent => self.obj("break-parent", vec![]),
            Doc::Shared(rc) => {
                let key = Rc::as_ptr(rc);
                if let Some(k) = self.shared.get(&key) {
                    return json!({ "$ref": k });
                }
                let k = self.shared.len();
                self.shared.insert(key, k);
                let inner = self.doc(rc);
                json!({ "$def": k, "doc": inner })
            }
            Doc::Invalid(m, _) => self.obj("$invalid", vec![("message", Value::String(m.clone()))]),
        }
    }
}

/// What `prettier_js_rs::format` does to the text before `text_to_doc`.
fn prepare(text: &str) -> String {
    let text = text.strip_prefix('\u{FEFF}').unwrap_or(text);
    if text.contains('\r') {
        text.replace("\r\n", "\n").replace('\r', "\n")
    } else {
        text.to_string()
    }
}

fn handle(req: &Value) -> Value {
    let args: Vec<String> = req["args"]
        .as_array()
        .map(|a| {
            a.iter()
                .filter_map(|x| x.as_str().map(String::from))
                .collect()
        })
        .unwrap_or_default();
    let stdin = req["stdin"].as_str().unwrap_or("");
    let Some(mut opts) = Options::from_cli(&args) else {
        return json!({"status": "cli"});
    };
    if let Some(w) = req.get("width").and_then(Value::as_u64) {
        opts.print_width = w as usize;
    }
    let mut out = Map::new();
    if req["fmt"].as_bool() == Some(true) {
        let r = std::panic::catch_unwind(|| prettier_js_rs::format(stdin, &opts));
        out.insert(
            "fmt".into(),
            match r {
                Ok(Ok(s)) => json!({ "ok": str_value(&s) }),
                Ok(Err(e)) => json!({ "err": e.0 }),
                Err(_) => json!({ "err": "panic" }),
            },
        );
    }
    let text = prepare(stdin);
    if text.trim().is_empty() {
        out.insert("status".into(), "empty".into());
        return Value::Object(out);
    }
    let want_doc = req["doc"].as_bool() == Some(true);
    let r = std::panic::catch_unwind(|| {
        let doc = prettier_js_rs::text_to_doc(&text, &opts)?;
        doc_printer_rs::port::propagate_breaks(&doc).map_err(prettier_js_rs::Unsupported)?;
        Ok::<_, prettier_js_rs::Unsupported>(if want_doc {
            Some(
                Ser {
                    shared: HashMap::new(),
                }
                .doc(&doc),
            )
        } else {
            None
        })
    });
    match r {
        Ok(Ok(doc)) => {
            out.insert("status".into(), "ok".into());
            if let Some(d) = doc {
                out.insert("doc".into(), d);
            }
        }
        Ok(Err(e)) => {
            out.insert("status".into(), "bail".into());
            out.insert("err".into(), e.0.into());
        }
        Err(_) => {
            out.insert("status".into(), "panic".into());
        }
    }
    Value::Object(out)
}

fn main() {
    std::panic::set_hook(Box::new(|_| {}));
    let worker = std::thread::Builder::new()
        .stack_size(1 << 30)
        .spawn(|| {
            let stdin = std::io::stdin();
            let stdout = std::io::stdout();
            let mut out = std::io::BufWriter::new(stdout.lock());
            for line in stdin.lock().lines() {
                let Ok(line) = line else { break };
                let mut de = serde_json::Deserializer::from_str(&line);
                de.disable_recursion_limit();
                let resp = match serde::Deserialize::deserialize(&mut de) {
                    Ok(req) => handle(&req),
                    Err(e) => json!({"status": "badreq", "err": e.to_string()}),
                };
                let s = serde_json::to_string(&resp)
                    .unwrap_or_else(|_| "{\"status\":\"panic\"}".into());
                let _ = writeln!(out, "{s}");
                let _ = out.flush();
            }
        })
        .expect("spawn");
    let _ = worker.join();
}
