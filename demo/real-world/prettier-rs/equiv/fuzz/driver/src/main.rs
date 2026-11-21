//! Differential driver: runs every case through prettier-js-rs (in a child `rust-worker`
//! process, so hangs, aborts and stack overflows are caught) and through Prettier (in a
//! long-lived Node oracle), and classifies the pair.
//!
//! Subcommands:
//!   run --gen NAME --in CASES.jsonl --out DIR [--workers 4] [--feedback] [--limit N]
//!   minimize --in FINDINGS.jsonl --out DIR [--workers 4] [--budget 3000]
//!   one FILE ARGS...         (prints both outputs for one input)
//!   rust-worker              (internal)

use serde_json::{Value, json};
use std::collections::BTreeMap;
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::mpsc::{Receiver, RecvTimeoutError, channel};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

fn main() {
    let args: Vec<String> = std::env::args().collect();
    // The oracle exits at once without it, which would look like Prettier failing every input.
    if matches!(args.get(1).map(String::as_str), Some("run" | "minimize" | "one"))
        && std::env::var_os("PRETTIER_DIST").is_none_or(|v| v.is_empty())
    {
        eprintln!("Set PRETTIER_DIST to Prettier 3.6.2's built dist/prettier directory.");
        std::process::exit(2);
    }
    match args.get(1).map(String::as_str) {
        Some("rust-worker") => rust_worker(),
        Some("run") => run(&args[2..]),
        Some("minimize") => minimize(&args[2..]),
        Some("one") => one(&args[2..]),
        Some("cov-run") => cov_run(),
        Some("encode-seeds") => encode_seeds(&args[2..]),
        Some("decode-corpus") => decode_corpus(&args[2..]),
        _ => {
            eprintln!("usage: equiv-fuzz run|minimize|one ...");
            std::process::exit(2);
        }
    }
}

#[path = "../../header.rs"]
mod header;

/// cov-run < CASES.jsonl: formats every case (and its output again) in this process, Rust only,
/// for line coverage under `cargo llvm-cov run`.
fn cov_run() {
    std::panic::set_hook(Box::new(|_| {}));
    let mut n = 0u64;
    for line in std::io::stdin().lock().lines() {
        let Ok(line) = line else { break };
        let Some((_, cargs, text)) = parse_case(&line) else { continue };
        let Some(opts) = prettier_js_rs::Options::from_cli(&cargs) else { continue };
        if let Ok(out) = fmt(&text, &opts) {
            let _ = fmt(&out, &opts);
        }
        n += 1;
    }
    eprintln!("cov-run: {n} cases");
}

/// encode-seeds CASES.jsonl DIR: writes each case as a libFuzzer input (default options).
fn encode_seeds(args: &[String]) {
    let dir = Path::new(&args[1]);
    std::fs::create_dir_all(dir).unwrap();
    let mut n = 0;
    for line in std::fs::read_to_string(&args[0]).unwrap().lines() {
        let Some((_, cargs, text)) = parse_case(line) else { continue };
        if cargs.iter().any(|a| a == "--parser") {
            continue;
        }
        let Some(path) = cargs.iter().position(|a| a == "--stdin-filepath").map(|i| &cargs[i + 1]) else {
            continue;
        };
        let Some(t) = header::type_byte(path) else { continue };
        if text.len() > 8000 {
            continue;
        }
        let mut bytes = vec![t, 0, 0];
        bytes.extend_from_slice(text.as_bytes());
        std::fs::write(dir.join(format!("seed{n:05}")), bytes).unwrap();
        n += 1;
    }
    eprintln!("wrote {n} seeds");
}

/// decode-corpus DIR: prints each libFuzzer corpus file as a case line.
fn decode_corpus(args: &[String]) {
    let mut entries: Vec<_> = std::fs::read_dir(&args[0]).unwrap().flatten().map(|e| e.path()).collect();
    entries.sort();
    let mut out = std::io::stdout().lock();
    for p in entries {
        let data = std::fs::read(&p).unwrap();
        if let Some((cargs, text)) = header::decode(&data) {
            let name = format!("lf-{}", p.file_name().unwrap().to_string_lossy());
            writeln!(out, "{}", json!({"name": name, "args": cargs, "stdin": text})).unwrap();
        }
    }
}

// ---------------------------------------------------------------- rust worker

fn rust_worker() {
    std::panic::set_hook(Box::new(|_| {}));
    let stdin = std::io::stdin();
    let mut stdout = std::io::stdout().lock();
    for line in stdin.lock().lines() {
        let Ok(line) = line else { break };
        if line.is_empty() {
            continue;
        }
        let req: Value = serde_json::from_str(&line).expect("request json");
        let argv: Vec<String> = req["args"]
            .as_array()
            .unwrap()
            .iter()
            .map(|a| a.as_str().unwrap().to_string())
            .collect();
        let text = req["text"].as_str().unwrap();
        let res = match prettier_js_rs::Options::from_cli(&argv) {
            None => json!({"status": "badopts"}),
            Some(opts) => match fmt(text, &opts) {
                Ok(out) => {
                    let second = match fmt(&out, &opts) {
                        Ok(s) => json!({"status": "ok", "out": s}),
                        Err(e) => e,
                    };
                    json!({"status": "ok", "out": out, "second": second})
                }
                Err(e) => e,
            },
        };
        writeln!(stdout, "{res}").unwrap();
        stdout.flush().unwrap();
    }
}

fn fmt(text: &str, opts: &prettier_js_rs::Options) -> Result<String, Value> {
    match std::panic::catch_unwind(|| prettier_js_rs::format(text, opts)) {
        Ok(Ok(s)) => Ok(s),
        Ok(Err(e)) => Err(json!({"status": "refused", "reason": e.0.to_string()})),
        Err(p) => {
            let msg = p
                .downcast_ref::<String>()
                .cloned()
                .or_else(|| p.downcast_ref::<&str>().map(|s| s.to_string()))
                .unwrap_or_default();
            Err(json!({"status": "panic", "reason": msg}))
        }
    }
}

// ---------------------------------------------------------------- child processes

enum Fail {
    Timeout,
    Died,
}

struct Proc {
    cmd: Vec<String>,
    child: Option<Child>,
    stdin: Option<ChildStdin>,
    rx: Option<Receiver<Option<String>>>,
}

impl Proc {
    fn new(cmd: Vec<String>) -> Self {
        Proc {
            cmd,
            child: None,
            stdin: None,
            rx: None,
        }
    }

    fn start(&mut self) {
        let mut child = Command::new(&self.cmd[0])
            .args(&self.cmd[1..])
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .expect("spawn child");
        let stdout = child.stdout.take().unwrap();
        let (tx, rx) = channel();
        std::thread::spawn(move || {
            let mut r = BufReader::with_capacity(1 << 20, stdout);
            let mut line = String::new();
            loop {
                line.clear();
                match r.read_line(&mut line) {
                    Ok(0) | Err(_) => {
                        let _ = tx.send(None);
                        return;
                    }
                    Ok(_) => {
                        if tx.send(Some(line.trim_end_matches('\n').to_string())).is_err() {
                            return;
                        }
                    }
                }
            }
        });
        self.stdin = child.stdin.take();
        self.child = Some(child);
        self.rx = Some(rx);
    }

    fn kill(&mut self) {
        self.stdin = None;
        if let Some(mut c) = self.child.take() {
            let _ = c.kill();
            let _ = c.wait();
        }
        self.rx = None;
    }

    fn send(&mut self, req: &Value) -> bool {
        if self.child.is_none() {
            self.start();
        }
        let s = self.stdin.as_mut().unwrap();
        let ok = writeln!(s, "{req}").is_ok() && s.flush().is_ok();
        if !ok {
            self.kill();
        }
        ok
    }

    fn recv(&mut self, timeout: Duration) -> Result<Value, Fail> {
        let Some(rx) = self.rx.as_ref() else {
            return Err(Fail::Died);
        };
        match rx.recv_timeout(timeout) {
            Ok(Some(line)) => Ok(serde_json::from_str(&line).expect("child json")),
            Ok(None) | Err(RecvTimeoutError::Disconnected) => {
                self.kill();
                Err(Fail::Died)
            }
            Err(RecvTimeoutError::Timeout) => {
                self.kill();
                Err(Fail::Timeout)
            }
        }
    }
}

impl Drop for Proc {
    fn drop(&mut self) {
        self.kill();
    }
}

// ---------------------------------------------------------------- evaluation

#[derive(Clone, Debug)]
enum RustRes {
    Ok { out: String, second: Value },
    Refused(String),
    BadOpts,
    Panic(String),
    Crash,
    Timeout,
}

#[derive(Clone, Debug)]
enum PRes {
    Ok(String),
    Err(String),
    Timeout,
}

struct Outcome {
    rust: RustRes,
    prettier: PRes,
    category: &'static str,
    idem: Option<&'static str>,
    /// Prettier's output of its own output, when the idempotence check needed it.
    prettier2: Option<PRes>,
}

struct Slot {
    rust: Proc,
    node: Proc,
    timeout: Duration,
}

fn fuzz_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap().to_path_buf()
}

impl Slot {
    fn new() -> Self {
        let exe = std::env::current_exe().unwrap().to_string_lossy().to_string();
        let oracle = fuzz_dir().join("oracle.mjs").to_string_lossy().to_string();
        Slot {
            rust: Proc::new(vec![exe, "rust-worker".into()]),
            node: Proc::new(vec![
                "node".into(),
                "--stack-size=984".into(),
                "--max-old-space-size=1536".into(),
                oracle,
            ]),
            timeout: Duration::from_secs(
                std::env::var("FUZZ_TIMEOUT")
                    .ok()
                    .and_then(|v| v.parse().ok())
                    .unwrap_or(20),
            ),
        }
    }

    fn prettier(&mut self, args: &[String], text: &str) -> PRes {
        let req = json!({"id": 0, "args": args, "text": text});
        if !self.node.send(&req) {
            return PRes::Timeout;
        }
        self.read_prettier()
    }

    fn read_prettier(&mut self) -> PRes {
        match self.node.recv(self.timeout) {
            Ok(v) if v["ok"].as_bool() == Some(true) => PRes::Ok(v["out"].as_str().unwrap().into()),
            Ok(v) => PRes::Err(v["err"].as_str().unwrap_or_default().into()),
            Err(_) => PRes::Timeout,
        }
    }

    fn eval(&mut self, args: &[String], text: &str, want_prettier: bool) -> Outcome {
        let req = json!({"id": 0, "args": args, "text": text});
        let rust_sent = self.rust.send(&req);
        let node_sent = want_prettier && self.node.send(&req);
        let rust = if !rust_sent {
            RustRes::Crash
        } else {
            match self.rust.recv(self.timeout) {
                Ok(v) => match v["status"].as_str().unwrap() {
                    "ok" => RustRes::Ok {
                        out: v["out"].as_str().unwrap().into(),
                        second: v["second"].clone(),
                    },
                    "refused" => RustRes::Refused(v["reason"].as_str().unwrap_or_default().into()),
                    "badopts" => RustRes::BadOpts,
                    _ => RustRes::Panic(v["reason"].as_str().unwrap_or_default().into()),
                },
                Err(Fail::Timeout) => RustRes::Timeout,
                Err(Fail::Died) => RustRes::Crash,
            }
        };
        let prettier = if !want_prettier {
            PRes::Err("not run".into())
        } else if !node_sent {
            PRes::Timeout
        } else {
            self.read_prettier()
        };
        let category = categorize(&rust, &prettier);
        let mut idem = None;
        let mut prettier2 = None;
        if let RustRes::Ok { out, second } = &rust {
            idem = Some(match second["status"].as_str().unwrap() {
                "ok" if second["out"].as_str() == Some(out.as_str()) => "idem_ok",
                "ok" => {
                    if let PRes::Ok(pout) = &prettier {
                        let p2 = self.prettier(args, pout);
                        let same = matches!(&p2, PRes::Ok(s) if Some(s.as_str()) == second["out"].as_str());
                        prettier2 = Some(p2);
                        match (category, same) {
                            ("equal", true) => "nonidem_same_as_prettier",
                            ("equal", false) => "nonidem_diverges",
                            _ => "nonidem_in_other",
                        }
                    } else {
                        "nonidem_in_other"
                    }
                }
                "refused" => "idem_second_refused",
                _ => "idem_second_panic",
            });
        }
        Outcome {
            rust,
            prettier,
            category,
            idem,
            prettier2,
        }
    }
}

fn categorize(rust: &RustRes, prettier: &PRes) -> &'static str {
    match (rust, prettier) {
        (RustRes::BadOpts, _) => "refused_opts",
        (RustRes::Refused(_), PRes::Err(_)) => "both_error",
        (RustRes::Refused(_), _) => "refused",
        (RustRes::Panic(_), _) => "RUST_PANIC",
        (RustRes::Crash, _) => "RUST_CRASH",
        (RustRes::Timeout, _) => "RUST_TIMEOUT",
        (RustRes::Ok { out, .. }, PRes::Ok(p)) if out == p => "equal",
        (RustRes::Ok { .. }, PRes::Ok(_)) => "MISMATCH",
        (RustRes::Ok { .. }, PRes::Err(_)) => "SOUNDNESS",
        (RustRes::Ok { .. }, PRes::Timeout) => "prettier_timeout",
    }
}

fn is_finding(o: &Outcome) -> bool {
    matches!(
        o.category,
        "MISMATCH" | "SOUNDNESS" | "RUST_PANIC" | "RUST_CRASH" | "RUST_TIMEOUT" | "prettier_timeout"
    ) || matches!(
        o.idem,
        Some("nonidem_diverges" | "nonidem_same_as_prettier" | "idem_second_panic")
    )
}

fn lang_of(args: &[String]) -> String {
    let mut path = "";
    let mut parser = None;
    let mut i = 0;
    while i < args.len() {
        let a = &args[i];
        if a == "--stdin-filepath" {
            path = args.get(i + 1).map(String::as_str).unwrap_or("");
            i += 1;
        } else if let Some(v) = a.strip_prefix("--stdin-filepath=") {
            path = v;
        } else if a == "--parser" {
            parser = args.get(i + 1).cloned();
            i += 1;
        } else if let Some(v) = a.strip_prefix("--parser=") {
            parser = Some(v.to_string());
        }
        i += 1;
    }
    if let Some(p) = parser {
        return p;
    }
    let base = path.rsplit('/').next().unwrap_or(path).to_ascii_lowercase();
    let ext = base.rsplit_once('.').map(|(_, e)| e).unwrap_or("");
    match ext {
        "js" | "mjs" | "cjs" => "js",
        "jsx" => "jsx",
        "ts" | "mts" | "cts" => "ts",
        "tsx" => "tsx",
        "json" | "jsonc" | "json5" => "json",
        "graphql" | "gql" | "graphqls" => "graphql",
        "yml" | "yaml" => "yaml",
        "md" | "markdown" | "mdown" | "mkdn" | "lr" => "markdown",
        "css" => "css",
        "scss" => "scss",
        "less" => "less",
        _ => {
            if base.starts_with('.') || base == "readme" {
                "other"
            } else {
                "other"
            }
        }
    }
    .to_string()
}

fn rust_json(r: &RustRes) -> Value {
    match r {
        RustRes::Ok { out, second } => json!({"status": "ok", "out": out, "second": second}),
        RustRes::Refused(s) => json!({"status": "refused", "reason": s}),
        RustRes::BadOpts => json!({"status": "badopts"}),
        RustRes::Panic(s) => json!({"status": "panic", "reason": s}),
        RustRes::Crash => json!({"status": "crash"}),
        RustRes::Timeout => json!({"status": "timeout"}),
    }
}

fn p_json(r: &PRes) -> Value {
    match r {
        PRes::Ok(s) => json!({"ok": true, "out": s}),
        PRes::Err(e) => json!({"ok": false, "err": e}),
        PRes::Timeout => json!({"ok": false, "timeout": true}),
    }
}

fn finding_json(gen_name: &str, name: &str, args: &[String], text: &str, o: &Outcome) -> Value {
    json!({
        "gen": gen_name,
        "name": name,
        "lang": lang_of(args),
        "category": o.category,
        "idem": o.idem,
        "args": args,
        "stdin": text,
        "rust": rust_json(&o.rust),
        "prettier": p_json(&o.prettier),
        "prettier2": o.prettier2.as_ref().map(p_json),
    })
}

// ---------------------------------------------------------------- run

fn flag<'a>(args: &'a [String], name: &str) -> Option<&'a str> {
    args.iter()
        .position(|a| a == name)
        .and_then(|i| args.get(i + 1))
        .map(String::as_str)
}

#[derive(Default)]
struct Stats {
    counts: BTreeMap<String, BTreeMap<String, BTreeMap<String, u64>>>,
    idem: BTreeMap<String, BTreeMap<String, BTreeMap<String, u64>>>,
    done: u64,
}

impl Stats {
    fn add(&mut self, gen_name: &str, lang: &str, o: &Outcome) {
        *self
            .counts
            .entry(gen_name.into())
            .or_default()
            .entry(lang.into())
            .or_default()
            .entry(o.category.into())
            .or_default() += 1;
        if let Some(i) = o.idem {
            *self
                .idem
                .entry(gen_name.into())
                .or_default()
                .entry(lang.into())
                .or_default()
                .entry(i.into())
                .or_default() += 1;
        }
    }

    fn json(&self) -> Value {
        json!({"cases": self.done, "categories": self.counts, "idempotence": self.idem})
    }
}

fn parse_case(line: &str) -> Option<(String, Vec<String>, String)> {
    let v: Value = serde_json::from_str(line).ok()?;
    let args = v["args"]
        .as_array()?
        .iter()
        .map(|a| a.as_str().map(String::from))
        .collect::<Option<Vec<_>>>()?;
    Some((
        v["name"].as_str().unwrap_or("").to_string(),
        args,
        v["stdin"].as_str()?.to_string(),
    ))
}

fn run(args: &[String]) {
    let gen_name = flag(args, "--gen").expect("--gen").to_string();
    let input = flag(args, "--in").expect("--in").to_string();
    let out_dir = PathBuf::from(flag(args, "--out").expect("--out"));
    let workers: usize = flag(args, "--workers").map_or(4, |v| v.parse().unwrap());
    let limit: u64 = flag(args, "--limit").map_or(u64::MAX, |v| v.parse().unwrap());
    let feedback = args.iter().any(|a| a == "--feedback");
    // --log-all FILE: one {name, category, idem} line per case (used by triage.mjs)
    let log_all = flag(args, "--log-all").map(|p| {
        Arc::new(Mutex::new(std::fs::File::create(p).expect("log-all file")))
    });
    std::fs::create_dir_all(&out_dir).unwrap();
    let reader: Box<dyn BufRead + Send> = if input == "-" {
        Box::new(BufReader::new(std::io::stdin()))
    } else {
        Box::new(BufReader::new(std::fs::File::open(&input).expect("open input")))
    };
    let lines = Arc::new(Mutex::new((reader.lines(), 0u64)));
    let stats = Arc::new(Mutex::new(Stats::default()));
    let findings = Arc::new(Mutex::new(
        std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(out_dir.join("findings.jsonl"))
            .unwrap(),
    ));
    let summary_path = out_dir.join("summary.json");
    let started = Instant::now();
    let mut handles = Vec::new();
    for _ in 0..workers {
        let lines = lines.clone();
        let stats = stats.clone();
        let findings = findings.clone();
        let gen_name = gen_name.clone();
        let log_all = log_all.clone();
        let summary_path = summary_path.clone();
        handles.push(std::thread::spawn(move || {
            let mut slot = Slot::new();
            loop {
                let line = {
                    let mut g = lines.lock().unwrap();
                    if g.1 >= limit {
                        break;
                    }
                    match g.0.next() {
                        Some(Ok(l)) => {
                            g.1 += 1;
                            l
                        }
                        _ => break,
                    }
                };
                let Some((name, cargs, text)) = parse_case(&line) else {
                    continue;
                };
                let lang = lang_of(&cargs);
                let o = slot.eval(&cargs, &text, true);
                let mut extra = Vec::new();
                if let Some(l) = &log_all {
                    writeln!(
                        l.lock().unwrap(),
                        "{}",
                        json!({"name": name, "category": o.category, "idem": o.idem})
                    )
                    .unwrap();
                }
                if is_finding(&o) {
                    extra.push(finding_json(&gen_name, &name, &cargs, &text, &o));
                }
                let mut fb = None;
                if feedback {
                    if let PRes::Ok(pout) = &o.prettier {
                        if pout != &text {
                            let o2 = slot.eval(&cargs, pout, true);
                            if is_finding(&o2) {
                                extra.push(finding_json(
                                    &format!("{gen_name}+fb"),
                                    &format!("{name}+fb"),
                                    &cargs,
                                    pout,
                                    &o2,
                                ));
                            }
                            fb = Some(o2);
                        }
                    }
                }
                if !extra.is_empty() {
                    let mut f = findings.lock().unwrap();
                    for e in extra {
                        writeln!(f, "{e}").unwrap();
                    }
                }
                let mut s = stats.lock().unwrap();
                s.add(&gen_name, &lang, &o);
                if let Some(o2) = &fb {
                    s.add(&format!("{gen_name}+fb"), &lang, o2);
                }
                s.done += 1;
                if s.done % 2000 == 0 {
                    let _ = std::fs::write(&summary_path, serde_json::to_string_pretty(&s.json()).unwrap());
                    eprintln!(
                        "[{gen_name}] {} cases, {:.0}s",
                        s.done,
                        started.elapsed().as_secs_f64()
                    );
                }
            }
        }));
    }
    for h in handles {
        h.join().unwrap();
    }
    let s = stats.lock().unwrap();
    let text = serde_json::to_string_pretty(&s.json()).unwrap();
    std::fs::write(&summary_path, &text).unwrap();
    println!("{text}");
    eprintln!("[{gen_name}] done {} cases in {:.0}s", s.done, started.elapsed().as_secs_f64());
}

// ---------------------------------------------------------------- one

fn one(args: &[String]) {
    let text = std::fs::read_to_string(&args[0]).unwrap();
    let cargs = args[1..].to_vec();
    let mut slot = Slot::new();
    let o = slot.eval(&cargs, &text, true);
    println!(
        "{}",
        serde_json::to_string_pretty(&finding_json("one", &args[0], &cargs, &text, &o)).unwrap()
    );
}

// ---------------------------------------------------------------- minimize

fn same_class(target: &Value, o: &Outcome) -> bool {
    let cat = target["category"].as_str().unwrap();
    let idem = target["idem"].as_str();
    match idem {
        Some(i @ ("nonidem_diverges" | "nonidem_same_as_prettier" | "idem_second_panic"))
            if !matches!(cat, "MISMATCH" | "SOUNDNESS" | "RUST_PANIC" | "RUST_CRASH" | "RUST_TIMEOUT") =>
        {
            o.idem == Some(i)
        }
        _ => o.category == cat,
    }
}

fn ddmin(units: Vec<String>, budget: &mut i64, test: &mut dyn FnMut(&str) -> bool) -> Vec<String> {
    let mut units = units;
    let mut n = 2usize;
    while units.len() >= 2 && *budget > 0 {
        let chunk = units.len().div_ceil(n);
        let mut reduced = false;
        let mut start = 0;
        while start < units.len() && *budget > 0 {
            let end = (start + chunk).min(units.len());
            let cand: Vec<String> = units[..start].iter().chain(&units[end..]).cloned().collect();
            *budget -= 1;
            if !cand.is_empty() && test(&cand.concat()) {
                units = cand;
                reduced = true;
                n = (n - 1).max(2);
                break;
            }
            start = end;
        }
        if !reduced {
            if n >= units.len() {
                break;
            }
            n = (n * 2).min(units.len());
        }
    }
    units
}

/// First line of Prettier's error without `(line:col)` positions, used to keep a soundness
/// finding's root cause while minimizing.
fn err_sig(p: &PRes) -> String {
    match p {
        PRes::Err(e) => {
            let first = e.lines().next().unwrap_or("");
            let mut out = String::new();
            let mut rest = first;
            while let Some(i) = rest.find('(') {
                out.push_str(&rest[..i]);
                let after = &rest[i + 1..];
                match after.find(')') {
                    Some(j) if after[..j].chars().all(|c| c.is_ascii_digit() || c == ':') => {
                        rest = &after[j + 1..];
                    }
                    _ => {
                        out.push('(');
                        rest = after;
                    }
                }
            }
            out.push_str(rest);
            out.trim().to_string()
        }
        _ => String::new(),
    }
}

/// A panic message without the numbers and quoted characters that vary with the input.
fn panic_sig(m: &str) -> String {
    let mut out = String::new();
    let mut quoted = false;
    for c in m.chars() {
        if c == '\'' {
            quoted = !quoted;
            out.push(c);
        } else if !quoted && !c.is_ascii_digit() {
            out.push(c);
        }
    }
    out
}

fn split_flags(cargs: &[String]) -> Vec<Vec<String>> {
    // groups of [flag] or [flag, value]; values never start with "--"
    let mut groups: Vec<Vec<String>> = Vec::new();
    for a in cargs {
        if a.starts_with("--") || groups.is_empty() {
            groups.push(vec![a.clone()]);
        } else {
            groups.last_mut().unwrap().push(a.clone());
        }
    }
    groups
}

fn minimize_one(slot: &mut Slot, f: &Value, budget: i64) -> Value {
    let mut cargs: Vec<String> = f["args"]
        .as_array()
        .unwrap()
        .iter()
        .map(|a| a.as_str().unwrap().to_string())
        .collect();
    let text = f["stdin"].as_str().unwrap().to_string();
    let rust_failure = matches!(
        f["category"].as_str(),
        Some("RUST_PANIC" | "RUST_CRASH" | "RUST_TIMEOUT")
    );
    // A Rust failure where Prettier succeeded must stay one; otherwise Prettier is not needed.
    let need_prettier_ok = rust_failure && f["prettier"]["ok"] == true;
    let want_prettier = !rust_failure || need_prettier_ok;
    let panic_msg = f["rust"]["reason"].as_str().map(String::from);
    let target_sig = if f["category"] == "SOUNDNESS" {
        let e = f["prettier"]["err"].as_str().unwrap_or("").to_string();
        Some(err_sig(&PRes::Err(e)))
    } else {
        None
    };
    let mut budget = budget;
    let mut tests = 0u64;
    let mut check = |slot: &mut Slot, args: &[String], t: &str| {
        tests += 1;
        let o = slot.eval(args, t, want_prettier);
        same_class(f, &o)
            && target_sig.as_ref().is_none_or(|s| *s == err_sig(&o.prettier))
            && (!need_prettier_ok || matches!(o.prettier, PRes::Ok(_)))
            && (!rust_failure
                || match &o.rust {
                    RustRes::Panic(m) => Some(panic_sig(m)) == panic_msg.as_deref().map(panic_sig),
                    _ => true,
                })
    };
    // lines (keeping terminators), then characters
    let lines: Vec<String> = text.split_inclusive('\n').map(String::from).collect();
    let lines = {
        let a = cargs.clone();
        ddmin(lines, &mut budget, &mut |t| check(slot, &a, t))
    };
    let chars: Vec<String> = lines.concat().chars().map(String::from).collect();
    let chars = {
        let a = cargs.clone();
        ddmin(chars, &mut budget, &mut |t| check(slot, &a, t))
    };
    let min = chars.concat();
    // then drop each option flag that is not needed (never --stdin-filepath)
    let mut groups = split_flags(&cargs);
    let mut i = 0;
    while i < groups.len() {
        if groups[i][0] == "--stdin-filepath" {
            i += 1;
            continue;
        }
        let cand: Vec<String> = groups
            .iter()
            .enumerate()
            .filter(|(j, _)| *j != i)
            .flat_map(|(_, g)| g.clone())
            .collect();
        if check(slot, &cand, &min) {
            groups.remove(i);
        } else {
            i += 1;
        }
    }
    cargs = groups.concat();
    let o = slot.eval(&cargs, &min, true);
    let mut out = finding_json(
        f["gen"].as_str().unwrap_or(""),
        f["name"].as_str().unwrap_or(""),
        &cargs,
        &min,
        &o,
    );
    out["original_len"] = json!(text.len());
    out["tests"] = json!(tests);
    out["still_same_class"] = json!(same_class(f, &o));
    out
}

fn minimize(args: &[String]) {
    let input = flag(args, "--in").expect("--in");
    let out_dir = PathBuf::from(flag(args, "--out").expect("--out"));
    let workers: usize = flag(args, "--workers").map_or(4, |v| v.parse().unwrap());
    let budget: i64 = flag(args, "--budget").map_or(3000, |v| v.parse().unwrap());
    std::fs::create_dir_all(&out_dir).unwrap();
    let findings: Vec<Value> = std::fs::read_to_string(input)
        .unwrap()
        .lines()
        .filter_map(|l| serde_json::from_str(l).ok())
        .collect();
    let queue = Arc::new(Mutex::new(findings.into_iter().enumerate()));
    let out = Arc::new(Mutex::new(
        std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(out_dir.join("minimized.jsonl"))
            .unwrap(),
    ));
    let mut handles = Vec::new();
    for _ in 0..workers {
        let queue = queue.clone();
        let out = out.clone();
        handles.push(std::thread::spawn(move || {
            let mut slot = Slot::new();
            loop {
                let Some((i, f)) = queue.lock().unwrap().next() else {
                    break;
                };
                let m = minimize_one(&mut slot, &f, budget);
                eprintln!(
                    "#{i} {} {} -> {} bytes",
                    f["category"],
                    f["stdin"].as_str().unwrap().len(),
                    m["stdin"].as_str().unwrap().len()
                );
                writeln!(out.lock().unwrap(), "{m}").unwrap();
            }
        }));
    }
    for h in handles {
        h.join().unwrap();
    }
}
