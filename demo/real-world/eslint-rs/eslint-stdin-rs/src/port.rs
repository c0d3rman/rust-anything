//! The Rust port of `wrappers/eslint-stdin.js`: ESLint's CLI on piped-in text, backed by
//! `eslint-linter-rs`.

#![forbid(unsafe_code)]

/// Port of `lintStdin` (`wrappers/eslint-stdin.js:13`).
pub async fn lint_stdin(args: Vec<String>, text: String) -> Result<String, String> {
    let run = eslint_linter_rs::cli::run(&args, &text, "[CWD]")
        .map_err(|e| format!("Error: not ported: {e}"))?;
    let mut out = String::from("{\"stdout\":");
    eslint_linter_rs::json::quote(&run.stdout, &mut out);
    out.push_str(",\"stderr\":");
    eslint_linter_rs::json::quote(&run.stderr, &mut out);
    out.push_str(&format!(",\"code\":{}}}", run.code));
    Ok(out)
}
