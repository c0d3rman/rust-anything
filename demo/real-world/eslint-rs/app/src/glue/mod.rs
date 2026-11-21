//! `eslint --no-config-lookup ... --stdin`: linted in Rust by `eslint-linter-rs` when every flag,
//! rule and construct in the file is ported; otherwise ESLint gets the case, with the stdin
//! bytes already read.

use std::ffi::OsString;
use std::io::{Read, Write};
use std::process::ExitCode;

use crate::original;

pub fn run(argv: &[OsString]) -> Option<ExitCode> {
    let args: Vec<String> = argv.iter().map(|a| a.to_str().map(str::to_string)).collect::<Option<_>>()?;
    eslint_linter_rs::cli::parse_args(&args).ok()?;
    let mut input = Vec::new();
    if std::io::stdin().read_to_end(&mut input).is_err() {
        return Some(original::run_with_stdin(argv, &input));
    }
    let Ok(text) = std::str::from_utf8(&input) else {
        return Some(original::run_with_stdin(argv, &input));
    };
    let cwd = std::env::current_dir().ok()?;
    let Some(cwd) = cwd.to_str() else {
        return Some(original::run_with_stdin(argv, &input));
    };
    match eslint_linter_rs::cli::run(&args, text, cwd) {
        Ok(r) => {
            let mut out = std::io::stdout().lock();
            let _ = out.write_all(r.stdout.as_bytes());
            let _ = out.flush();
            let mut err = std::io::stderr().lock();
            let _ = err.write_all(r.stderr.as_bytes());
            Some(ExitCode::from(r.code))
        }
        Err(_) => Some(original::run_with_stdin(argv, &input)),
    }
}
