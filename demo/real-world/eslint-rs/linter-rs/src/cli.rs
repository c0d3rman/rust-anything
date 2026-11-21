//! The CLI's stdin path (lib/cli.js, lib/shared/translate-cli-options.js,
//! lib/eslint/eslint-helpers.js `verifyText`, the `json` and `stylish` formatters) for runs whose
//! whole config comes from flags (`--no-config-lookup`).

use crate::json;
use crate::levn;
use crate::linter::{self, Options, RuleConf, Unsupported};
use crate::report::Problem;
use crate::source::SourceType;
use serde_json::Value;

pub struct Run {
    pub stdout: String,
    pub stderr: String,
    pub code: u8,
}

pub struct Parsed {
    pub opts: Options,
    pub filename: Option<String>,
    pub format: Format,
    pub color: Option<bool>,
    pub max_warnings: i64,
    pub exit_on_fatal: bool,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Format {
    Json,
    Stylish,
}

fn merge_rules(into: &mut Vec<(String, Value)>, from: Vec<(String, Value)>) {
    for (k, v) in from {
        match into.iter_mut().find(|(x, _)| *x == k) {
            Some(slot) => slot.1 = v,
            None => into.push((k, v)),
        }
    }
}

fn cli_severity(v: &Value) -> Option<u8> {
    match v {
        Value::String(s) if matches!(s.as_str(), "0" | "1" | "2") => None,
        other => linter::severity_of(other),
    }
}

/// Reads argv (without the program name). `Err` means the case goes to ESLint.
pub fn parse_args(args: &[String]) -> Result<Parsed, Unsupported> {
    let mut config_lookup = true;
    let mut rules: Vec<(String, Value)> = Vec::new();
    let mut globals: Vec<String> = Vec::new();
    let mut parser_options: Option<Vec<(String, Value)>> = None;
    let mut format = Format::Stylish;
    let mut fix_dry_run = false;
    let mut fix_types: Option<Vec<String>> = None;
    let mut stdin = false;
    let mut filename: Option<String> = None;
    let mut quiet = false;
    let mut max_warnings: i64 = -1;
    let mut rudd = false;
    let mut rudd_sev: Option<String> = None;
    let mut ruic: Option<String> = None;
    let mut inline = true;
    let mut color: Option<bool> = None;
    let mut exit_on_fatal = false;
    let mut i = 0;
    while i < args.len() {
        let a = args[i].as_str();
        i += 1;
        let (flag, inline_value) = match a.split_once('=') {
            Some((f, v)) if f.starts_with("--") => (f, Some(v.to_string())),
            _ => (a, None),
        };
        let take = |i: &mut usize| -> Result<String, Unsupported> {
            if let Some(v) = &inline_value {
                return Ok(v.clone());
            }
            let v = args.get(*i).cloned().ok_or("missing value")?;
            *i += 1;
            Ok(v)
        };
        match flag {
            "--no-config-lookup" => config_lookup = false,
            "--rule" => {
                let v = take(&mut i)?;
                let parsed = levn::parse_object(&v).ok_or("rule not parsed")?;
                merge_rules(&mut rules, parsed);
            }
            "--global" => {
                let v = take(&mut i)?;
                if v.contains(['"', '\'', '[', ']']) {
                    return Err("quoted global".into());
                }
                for g in v.split(',') {
                    let g = g.trim();
                    if !g.is_empty() {
                        globals.push(g.to_string());
                    }
                }
            }
            "--parser-options" => {
                let v = take(&mut i)?;
                let parsed = levn::parse_object(&v).ok_or("parser options not parsed")?;
                let mut cur = parser_options.take().unwrap_or_default();
                merge_rules(&mut cur, parsed);
                parser_options = Some(cur);
            }
            "-f" | "--format" => {
                format = match take(&mut i)?.as_str() {
                    "json" => Format::Json,
                    "stylish" => Format::Stylish,
                    _ => return Err("format".into()),
                }
            }
            "--fix-dry-run" => fix_dry_run = true,
            "--fix-type" => {
                let v = take(&mut i)?;
                let list: Vec<String> = v.split(',').map(|s| s.trim().to_string()).collect();
                if list.iter().any(|t| {
                    !matches!(
                        t.as_str(),
                        "problem" | "suggestion" | "layout" | "directive"
                    )
                }) {
                    return Err("fix type".into());
                }
                let mut all = fix_types.take().unwrap_or_default();
                all.extend(list);
                fix_types = Some(all);
            }
            "--stdin" => stdin = true,
            "--stdin-filename" => filename = Some(take(&mut i)?),
            "--quiet" => quiet = true,
            "--max-warnings" => {
                max_warnings = take(&mut i)?.parse().map_err(|_| "max warnings")?;
            }
            "--report-unused-disable-directives" => rudd = true,
            "--report-unused-disable-directives-severity" => rudd_sev = Some(take(&mut i)?),
            "--report-unused-inline-configs" => ruic = Some(take(&mut i)?),
            "--no-inline-config" => inline = false,
            "--color" => color = Some(true),
            "--no-color" => color = Some(false),
            "--exit-on-fatal-error" => exit_on_fatal = true,
            _ => return Err(format!("argument {a}")),
        }
    }
    if config_lookup || !stdin {
        return Err("config lookup or files".into());
    }
    if fix_types.is_some() && !fix_dry_run {
        return Err("fix type without fix".into());
    }
    if rudd && rudd_sev.is_some() {
        return Err("both unused directive flags".into());
    }
    let sev_word = |s: &str| -> Result<&'static str, Unsupported> {
        match s {
            "off" | "0" => Ok("off"),
            "warn" | "1" => Ok("warn"),
            "error" | "2" => Ok("error"),
            _ => Err("severity word".into()),
        }
    };
    let report_unused_disable_directives = if rudd {
        "error"
    } else if let Some(s) = &rudd_sev {
        sev_word(s)?
    } else {
        "warn"
    };
    let report_unused_inline_configs = match &ruic {
        Some(s) => sev_word(s)?,
        None => "off",
    };
    let mut conf: Vec<(String, RuleConf)> = Vec::new();
    for (k, v) in rules {
        let arr = match v {
            Value::Array(a) => a,
            other => vec![other],
        };
        let severity = arr.first().and_then(cli_severity).ok_or("rule severity")?;
        if !crate::core_rules::CORE_RULES.contains(&k.as_str()) {
            return Err("unknown rule".into());
        }
        let options = arr[1..].to_vec();
        let def = crate::rules::find(&k).ok_or_else(|| format!("rule {k} not ported"))?;
        (def.create)(&options).map_err(|e| format!("{k}: {e}"))?;
        conf.push((k, RuleConf { severity, options }));
    }
    let mut jsx = false;
    let mut global_return = false;
    if let Some(po) = parser_options {
        for (k, v) in po {
            match (k.as_str(), v) {
                ("ecmaFeatures", Value::Object(o)) => {
                    for (fk, fv) in o {
                        match (fk.as_str(), fv) {
                            ("jsx", Value::Bool(b)) => jsx = b,
                            ("globalReturn", Value::Bool(b)) => global_return = b,
                            _ => return Err("parser option".into()),
                        }
                    }
                }
                _ => return Err("parser option".into()),
            }
        }
    }
    let source_type = match &filename {
        None => SourceType::Module,
        Some(f) => {
            let lower = f.as_str();
            if lower.ends_with(".cjs") {
                SourceType::CommonJs
            } else if lower.ends_with(".js") || lower.ends_with(".mjs") {
                SourceType::Module
            } else {
                return Err("file type".into());
            }
        }
    };
    if let Some(f) = &filename {
        let segs: Vec<&str> = f.split(['/', '\\']).collect();
        if f.starts_with('/')
            || f.contains('\\')
            || segs
                .iter()
                .any(|s| matches!(*s, ".." | "." | "node_modules" | ".git" | ""))
        {
            return Err("file path".into());
        }
    }
    let mut global_pairs = Vec::new();
    for g in globals {
        match g.strip_suffix(":true") {
            Some(n) => global_pairs.push((n.to_string(), "writable".to_string())),
            None => global_pairs.push((g, "readonly".to_string())),
        }
    }
    let mut dedup: Vec<(String, String)> = Vec::new();
    for (k, v) in global_pairs {
        match dedup.iter_mut().find(|(x, _)| *x == k) {
            Some(slot) => slot.1 = v,
            None => dedup.push((k, v)),
        }
    }
    Ok(Parsed {
        opts: Options {
            rules: conf,
            source_type,
            jsx,
            global_return,
            globals: dedup,
            allow_inline_config: inline,
            report_unused_disable_directives,
            report_unused_inline_configs,
            fix: fix_dry_run,
            fix_types,
            quiet,
        },
        filename,
        format,
        color,
        max_warnings,
        exit_on_fatal,
    })
}

struct Counts {
    errors: usize,
    fatal: usize,
    warnings: usize,
    fixable_errors: usize,
    fixable_warnings: usize,
}

fn counts(messages: &[Problem]) -> Counts {
    let mut c = Counts {
        errors: 0,
        fatal: 0,
        warnings: 0,
        fixable_errors: 0,
        fixable_warnings: 0,
    };
    for m in messages {
        if m.fatal || m.severity == 2 {
            c.errors += 1;
            if m.fatal {
                c.fatal += 1;
            }
            if m.fix.is_some() {
                c.fixable_errors += 1;
            }
        } else {
            c.warnings += 1;
            if m.fix.is_some() {
                c.fixable_warnings += 1;
            }
        }
    }
    c
}

struct FileResult {
    file_path: String,
    messages: Vec<Problem>,
    suppressed: Vec<Problem>,
    counts: Counts,
    output: Option<String>,
    source: Option<String>,
}

fn write_json(results: &[FileResult]) -> String {
    let mut out = String::from("[");
    for (i, r) in results.iter().enumerate() {
        if i > 0 {
            out.push(',');
        }
        out.push_str("{\"filePath\":");
        json::quote(&r.file_path, &mut out);
        out.push_str(",\"messages\":[");
        for (j, m) in r.messages.iter().enumerate() {
            if j > 0 {
                out.push(',');
            }
            m.write_json(&mut out);
        }
        out.push_str("],\"suppressedMessages\":[");
        for (j, m) in r.suppressed.iter().enumerate() {
            if j > 0 {
                out.push(',');
            }
            m.write_json(&mut out);
        }
        let c = &r.counts;
        out.push_str(&format!(
            "],\"errorCount\":{},\"fatalErrorCount\":{},\"warningCount\":{},\"fixableErrorCount\":{},\"fixableWarningCount\":{}",
            c.errors, c.fatal, c.warnings, c.fixable_errors, c.fixable_warnings
        ));
        if let Some(o) = &r.output {
            out.push_str(",\"output\":");
            json::quote(o, &mut out);
        }
        if let Some(s) = &r.source {
            out.push_str(",\"source\":");
            json::quote(s, &mut out);
        }
        out.push_str(",\"usedDeprecatedRules\":[]}");
    }
    out.push(']');
    out
}

fn style(color: bool, open: &str, close: &str, text: &str) -> String {
    if color {
        format!("\u{1b}[{open}m{text}\u{1b}[{close}m")
    } else {
        text.to_string()
    }
}

fn js_len(s: &str) -> usize {
    let stripped;
    let s = if s.contains('\u{1b}') || s.contains('\u{9b}') {
        stripped = strip_vt(s);
        stripped.as_str()
    } else {
        s
    };
    s.encode_utf16().count()
}

fn strip_vt(s: &str) -> String {
    let mut out = String::new();
    let mut it = s.chars().peekable();
    while let Some(c) = it.next() {
        if c == '\u{1b}' && it.peek() == Some(&'[') {
            it.next();
            for d in it.by_ref() {
                if d.is_ascii_alphabetic() {
                    break;
                }
            }
        } else {
            out.push(c);
        }
    }
    out
}

fn stylish(results: &[FileResult], color: bool) -> String {
    let mut output = String::from("\n");
    let (mut errors, mut warnings, mut fe, mut fw) = (0, 0, 0, 0);
    let mut summary_red = false;
    for r in results {
        if r.messages.is_empty() {
            continue;
        }
        errors += r.counts.errors;
        warnings += r.counts.warnings;
        fe += r.counts.fixable_errors;
        fw += r.counts.fixable_warnings;
        output.push_str(&style(color, "4", "24", &r.file_path));
        output.push('\n');
        let rows: Vec<Vec<String>> = r
            .messages
            .iter()
            .map(|m| {
                let kind = if m.fatal || m.severity == 2 {
                    summary_red = true;
                    style(color, "31", "39", "error")
                } else {
                    style(color, "33", "39", "warning")
                };
                let mut text = m.message.clone();
                let u: Vec<u16> = text.encode_utf16().collect();
                if u.len() > 1 && text.ends_with('.') && u[u.len() - 2] != b' ' as u16 {
                    text.pop();
                }
                vec![
                    String::new(),
                    m.line.to_string(),
                    m.column.to_string(),
                    kind,
                    text,
                    m.rule_id
                        .as_ref()
                        .map(|r| style(color, "2", "22", r))
                        .unwrap_or_default(),
                ]
            })
            .collect();
        let table = text_table(&rows);
        let lines: Vec<String> = table.split('\n').map(|l| dim_position(l, color)).collect();
        output.push_str(&lines.join("\n"));
        output.push_str("\n\n");
    }
    let total = errors + warnings;
    if total > 0 {
        let pl = |w: &str, n: usize| {
            if n == 1 {
                w.to_string()
            } else {
                format!("{w}s")
            }
        };
        let (co, cc) = if summary_red {
            ("31", "39")
        } else {
            ("33", "39")
        };
        let line = format!(
            "\u{2716} {}{} ({}{}, {}{})",
            total,
            pl(" problem", total),
            errors,
            pl(" error", errors),
            warnings,
            pl(" warning", warnings)
        );
        output.push_str(&style(color, co, cc, &style(color, "1", "22", &line)));
        output.push('\n');
        if fe > 0 || fw > 0 {
            let line = format!(
                "  {}{} and {}{} potentially fixable with the `--fix` option.",
                fe,
                pl(" error", fe),
                fw,
                pl(" warning", fw)
            );
            output.push_str(&style(color, co, cc, &style(color, "1", "22", &line)));
            output.push('\n');
        }
        style(color, "0", "0", &output)
    } else {
        String::new()
    }
}

/// `.replace(/(\d+)\s+(\d+)/u, ...)` on one table line.
fn dim_position(line: &str, color: bool) -> String {
    let b = line.as_bytes();
    let mut i = 0;
    while i < b.len() {
        if b[i].is_ascii_digit() {
            let s = i;
            while i < b.len() && b[i].is_ascii_digit() {
                i += 1;
            }
            let d1 = i;
            let mut j = i;
            while j < line.len()
                && line[j..]
                    .chars()
                    .next()
                    .is_some_and(crate::directives::is_ws)
            {
                j += line[j..].chars().next().map_or(1, char::len_utf8);
            }
            if j > d1 && j < b.len() && b[j].is_ascii_digit() {
                let mut k = j;
                while k < b.len() && b[k].is_ascii_digit() {
                    k += 1;
                }
                let rep = style(
                    color,
                    "2",
                    "22",
                    &format!("{}:{}", &line[s..d1], &line[j..k]),
                );
                return format!("{}{}{}", &line[..s], rep, &line[k..]);
            }
            i = d1;
            continue;
        }
        i += 1;
    }
    line.to_string()
}

fn text_table(rows: &[Vec<String>]) -> String {
    let lens: Vec<Vec<usize>> = rows
        .iter()
        .map(|r| r.iter().map(|c| js_len(c)).collect())
        .collect();
    let mut sizes: Vec<usize> = Vec::new();
    for r in &lens {
        for (i, n) in r.iter().enumerate() {
            if sizes.len() <= i {
                sizes.push(*n);
            } else if *n > sizes[i] {
                sizes[i] = *n;
            }
        }
    }
    let align = ["", "r", "l"];
    rows.iter()
        .enumerate()
        .map(|(ri, row)| {
            let cells: Vec<String> = row
                .iter()
                .enumerate()
                .map(|(ix, c)| {
                    let n = sizes[ix].saturating_sub(lens[ri][ix]);
                    let s = " ".repeat(n);
                    if align.get(ix) == Some(&"r") {
                        format!("{s}{c}")
                    } else {
                        format!("{c}{s}")
                    }
                })
                .collect();
            cells
                .join("  ")
                .trim_end_matches(crate::directives::is_ws)
                .to_string()
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// Runs the CLI on stdin text. `Err` means the case goes to ESLint unchanged.
pub fn run(args: &[String], stdin: &str, cwd: &str) -> Result<Run, Unsupported> {
    let p = parse_args(args)?;
    let outcome = linter::verify_and_fix(stdin, &p.opts)?;
    let file_path = match &p.filename {
        None => "<text>".to_string(),
        Some(f) => format!("{}/{}", cwd.trim_end_matches('/'), f),
    };
    let c = counts(&outcome.messages);
    let output = if outcome.fixed {
        Some(outcome.output.clone())
    } else {
        None
    };
    let source = if c.errors + c.warnings > 0 && output.is_none() {
        Some(stdin.to_string())
    } else {
        None
    };
    let mut result = FileResult {
        file_path,
        messages: outcome.messages,
        suppressed: outcome.suppressed,
        counts: c,
        output,
        source,
    };
    let total_errors = result.counts.errors;
    let total_warnings = result.counts.warnings;
    let total_fatal = result.counts.fatal;
    let mut results = vec![];
    if p.opts.quiet {
        let fixable_errors = result.counts.fixable_errors;
        let keep: Vec<Problem> = result
            .messages
            .drain(..)
            .filter(|m| m.fatal || m.severity == 2)
            .collect();
        if !keep.is_empty() {
            let sup: Vec<Problem> = result
                .suppressed
                .drain(..)
                .filter(|m| m.fatal || m.severity == 2)
                .collect();
            result.counts = Counts {
                errors: keep.len(),
                fatal: result.counts.fatal,
                warnings: 0,
                fixable_errors,
                fixable_warnings: 0,
            };
            result.messages = keep;
            result.suppressed = sup;
            results.push(result);
        }
    } else {
        results.push(result);
    }
    let too_many = p.max_warnings >= 0 && (total_warnings as i64) > p.max_warnings;
    let formatted = match p.format {
        Format::Json => write_json(&results),
        Format::Stylish => {
            if too_many && p.color.is_none() {
                stylish(&results, false)
            } else {
                stylish(&results, p.color.unwrap_or(false))
            }
        }
    };
    let mut stdout = String::new();
    if !formatted.is_empty() {
        stdout.push_str(&formatted);
        stdout.push('\n');
    }
    let mut stderr = String::new();
    if total_errors == 0 && too_many {
        stderr.push_str(&format!(
            "ESLint found too many warnings (maximum: {}).\n",
            p.max_warnings
        ));
    }
    let code = if p.exit_on_fatal && total_fatal > 0 {
        2
    } else if total_errors > 0 || too_many {
        1
    } else {
        0
    };
    Ok(Run {
        stdout,
        stderr,
        code,
    })
}
