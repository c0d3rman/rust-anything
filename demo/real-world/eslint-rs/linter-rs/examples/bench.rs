//! `bench <inputs.jsonl> <names.txt> [iterations]`: the port's `verify_and_fix` in process on
//! the same cases as `bench/linter-node.mjs`; per-file microseconds (median) as JSON lines.

use std::io::BufRead;

fn main() {
    let mut a = std::env::args().skip(1);
    let (inputs, names) = (a.next().expect("inputs"), a.next().expect("names"));
    let iterations: usize = a.next().and_then(|s| s.parse().ok()).unwrap_or(10);
    let names: std::collections::HashSet<String> = std::fs::read_to_string(names)
        .expect("names")
        .lines()
        .map(str::to_string)
        .collect();
    let f = std::io::BufReader::new(std::fs::File::open(inputs).expect("inputs"));
    for line in f.lines().map_while(Result::ok) {
        let c: serde_json::Value = serde_json::from_str(&line).expect("json");
        let name = c["name"].as_str().unwrap_or("").to_string();
        if !names.contains(&name) && !names.contains(&name.to_lowercase()) {
            continue;
        }
        let args: Vec<String> = c["args"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|v| v.as_str().map(str::to_string))
            .collect();
        let text = c["stdin"].as_str().unwrap_or("").to_string();
        let Ok(p) = eslint_linter_rs::cli::parse_args(&args) else {
            continue;
        };
        let run = || eslint_linter_rs::linter::verify_and_fix(&text, &p.opts).is_ok();
        for _ in 0..3 {
            run();
        }
        let mut t = Vec::with_capacity(iterations);
        for _ in 0..iterations {
            let s = std::time::Instant::now();
            std::hint::black_box(run());
            t.push(s.elapsed().as_secs_f64() * 1e6);
        }
        t.sort_by(f64::total_cmp);
        println!("{{\"name\":\"{}\",\"us\":{:.1}}}", name, t[t.len() / 2]);
    }
}
