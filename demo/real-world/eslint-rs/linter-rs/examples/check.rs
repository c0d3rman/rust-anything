//! Runs every recorded CLI case through the port in process: cases it claims must match the
//! recording byte for byte. `check <app-cases dir> [name prefix]`; `MISMATCHES=<file>` writes
//! the differing cases, `CLAIMED=<file>` the names of matching ones.

use std::collections::BTreeMap;

fn toml_args(toml: &str) -> Option<(Vec<String>, i64)> {
    let mut args = None;
    let mut code = 0;
    for line in toml.lines() {
        if let Some(rest) = line.strip_prefix("args = ") {
            args = Some(parse_array(rest)?);
        } else if let Some(rest) = line.strip_prefix("status.code = ") {
            code = rest.trim().parse().ok()?;
        }
    }
    Some((args?, code))
}

fn parse_array(s: &str) -> Option<Vec<String>> {
    let b: Vec<char> = s.trim().chars().collect();
    let mut out = Vec::new();
    let mut i = 1;
    while i < b.len() {
        match b[i] {
            '"' => {
                let mut v = String::new();
                i += 1;
                while b[i] != '"' {
                    if b[i] == '\\' {
                        i += 1;
                        match b[i] {
                            'n' => v.push('\n'),
                            't' => v.push('\t'),
                            'u' => {
                                let h: String = b[i + 1..i + 5].iter().collect();
                                v.push(char::from_u32(u32::from_str_radix(&h, 16).ok()?)?);
                                i += 4;
                            }
                            c => v.push(c),
                        }
                    } else {
                        v.push(b[i]);
                    }
                    i += 1;
                }
                out.push(v);
            }
            '\'' => {
                let mut v = String::new();
                i += 1;
                while b[i] != '\'' {
                    v.push(b[i]);
                    i += 1;
                }
                out.push(v);
            }
            _ => {}
        }
        i += 1;
    }
    Some(out)
}

fn main() {
    let dir = std::env::args().nth(1).expect("app-cases dir");
    let prefix = std::env::args().nth(2).unwrap_or_default();
    let mut names: Vec<String> = std::fs::read_dir(&dir)
        .unwrap()
        .filter_map(|e| e.ok()?.file_name().into_string().ok())
        .filter_map(|n| n.strip_suffix(".toml").map(str::to_string))
        .filter(|n| n.starts_with(&prefix))
        .collect();
    names.sort();
    let (mut ok, mut bad, mut declined) = (0, 0, 0);
    let mut reasons: BTreeMap<String, usize> = BTreeMap::new();
    let mut mism = String::new();
    let mut claimed = String::new();
    let start = std::time::Instant::now();
    for n in &names {
        let read =
            |ext: &str| std::fs::read_to_string(format!("{dir}/{n}.{ext}")).unwrap_or_default();
        let Some((args, code)) = toml_args(&read("toml")) else {
            continue;
        };
        let stdin = read("stdin");
        match eslint_linter_rs::cli::run(&args, &stdin, "[CWD]") {
            Err(e) => {
                declined += 1;
                let key: String = e.chars().take(60).collect();
                *reasons.entry(key).or_default() += 1;
            }
            Ok(r) => {
                let (eo, ee) = (read("stdout"), read("stderr"));
                if r.stdout == eo && r.stderr == ee && r.code as i64 == code {
                    ok += 1;
                    claimed.push_str(n);
                    claimed.push('\n');
                } else {
                    bad += 1;
                    mism.push_str(&format!(
                        "## {n}\nexpected ({code}):\n{eo}{ee}\nactual ({}):\n{}{}\n",
                        r.code, r.stdout, r.stderr
                    ));
                }
            }
        }
    }
    println!(
        "{} cases: {ok} in Rust and identical, {bad} mismatches, {declined} declined ({:.2?})",
        names.len(),
        start.elapsed()
    );
    let mut rs: Vec<_> = reasons.into_iter().collect();
    rs.sort_by_key(|r| std::cmp::Reverse(r.1));
    for (r, c) in rs.iter().take(40) {
        println!("  {c:6} {r}");
    }
    if let Ok(p) = std::env::var("MISMATCHES") {
        std::fs::write(p, mism).unwrap();
    }
    if let Ok(p) = std::env::var("CLAIMED") {
        std::fs::write(p, claimed).unwrap();
    }
}
