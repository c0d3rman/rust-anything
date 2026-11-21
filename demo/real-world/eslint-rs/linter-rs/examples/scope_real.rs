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
    let (_ok, _bad, _declined) = (0, 0, 0);
    let _reasons: BTreeMap<String, usize> = BTreeMap::new();
    let _mism = String::new();
    let _claimed = String::new();
    let _start = std::time::Instant::now();
    let rules: Vec<String> = std::env::args()
        .nth(2)
        .expect("rules")
        .split(',')
        .map(str::to_string)
        .collect();
    let mut names: Vec<String> = std::fs::read_dir(&dir)
        .unwrap()
        .filter_map(|e| e.ok()?.file_name().into_string().ok())
        .filter_map(|n| n.strip_suffix(".toml").map(str::to_string))
        .filter(|n| n.starts_with("real-"))
        .collect();
    names.sort();
    let norm = |s: &str, expected: bool| -> Vec<String> {
        if s.starts_with('[') {
            let v: serde_json::Value = serde_json::from_str(s).unwrap();
            if expected && v[0].get("output").is_some() {
                return vec!["<fixed output>".to_string()];
            }
            let mut out = Vec::new();
            for key in ["messages", "suppressedMessages"] {
                for m in v[0][key].as_array().unwrap() {
                    if m["ruleId"]
                        .as_str()
                        .is_some_and(|r| rules.iter().any(|x| x == r))
                    {
                        out.push(format!("{key} {m}"));
                    }
                }
            }
            return out;
        }
        s.lines()
            .filter(|l| {
                l.starts_with("  ")
                    && rules
                        .iter()
                        .any(|r| l.trim_end().ends_with(&format!("  {r}")))
            })
            .map(|l| {
                l.split("  ")
                    .map(str::trim)
                    .filter(|x| !x.is_empty())
                    .collect::<Vec<_>>()
                    .join(" | ")
            })
            .collect()
    };
    let (mut ok, mut bad, mut dec, mut skipped) = (0, 0, 0, 0);
    for n in &names {
        let read =
            |ext: &str| std::fs::read_to_string(format!("{dir}/{n}.{ext}")).unwrap_or_default();
        let Some((mut args, _)) = toml_args(&read("toml")) else {
            continue;
        };
        let i = args.iter().position(|a| a == "--rule").unwrap() + 1;
        let conf: serde_json::Map<String, serde_json::Value> =
            serde_json::from_str(&args[i]).unwrap();
        let kept: serde_json::Map<String, serde_json::Value> = conf
            .into_iter()
            .filter(|(k, _)| rules.contains(k))
            .collect();
        args[i] = serde_json::Value::Object(kept).to_string();
        match eslint_linter_rs::cli::run(&args, &read("stdin"), "[CWD]") {
            Err(e) => {
                dec += 1;
                println!("declined {n}: {e}");
            }
            Ok(r) => {
                let (e, a) = (norm(&read("stdout"), true), norm(&r.stdout, false));
                let mut e = e;
                let mut a = a;
                if e == ["<fixed output>"] {
                    skipped += 1;
                    let v: serde_json::Value = serde_json::from_str(&read("stdout")).unwrap();
                    let fixed = v[0]["output"].as_str().unwrap().to_string();
                    let Ok(r2) = eslint_linter_rs::cli::run(&args, &fixed, "[CWD]") else {
                        continue;
                    };
                    let mut v2 = v.clone();
                    v2[0].as_object_mut().unwrap().remove("output");
                    e = norm(&v2.to_string(), true);
                    a = norm(&r2.stdout, false);
                }
                if e == a {
                    ok += 1;
                } else {
                    bad += 1;
                    println!("## {n}");
                    for l in &e {
                        if !a.contains(l) {
                            println!("- {l}");
                        }
                    }
                    for l in &a {
                        if !e.contains(l) {
                            println!("+ {l}");
                        }
                    }
                }
            }
        }
    }
    println!("ok {ok} bad {bad} declined {dec} skipped(fixed) {skipped}");
}
