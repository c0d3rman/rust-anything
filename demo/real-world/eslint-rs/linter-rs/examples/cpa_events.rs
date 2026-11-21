//! Dumps the code path analysis steps of recorded cases: `cpa_events <list> <app-cases dir> <out dir>`
//! reads the case names in `<list>` and writes `<out dir>/<name>.txt` for each case that parses.

use eslint_linter_rs::source::{ParseOptions, SourceCode, SourceType};

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let (Some(list), Some(dir), Some(out)) = (args.get(1), args.get(2), args.get(3)) else {
        eprintln!("usage: cpa_events <list> <app-cases dir> <out dir>");
        return;
    };
    let _ = std::fs::create_dir_all(out);
    let names = std::fs::read_to_string(list).unwrap_or_default();
    let (mut ok, mut skipped) = (0, 0);
    for name in names.lines().filter(|l| !l.is_empty()) {
        let toml = std::fs::read_to_string(format!("{dir}/{name}.toml")).unwrap_or_default();
        let Ok(code) = std::fs::read_to_string(format!("{dir}/{name}.stdin")) else {
            continue;
        };
        let filename = toml
            .split("\"--stdin-filename\", \"")
            .nth(1)
            .and_then(|r| r.split('"').next())
            .unwrap_or("input.js")
            .to_string();
        let po = toml
            .split("\"--parser-options\", '")
            .nth(1)
            .and_then(|r| r.split('\'').next())
            .unwrap_or("");
        if po.contains("impliedStrict")
            || toml.contains("\"--parser\"")
            || toml.contains("\"--env\"")
        {
            skipped += 1;
            continue;
        }
        let jsx = po.contains("\"jsx\":true");
        let global_return = po.contains("\"globalReturn\":true");
        let source_type = if filename.ends_with(".cjs") {
            SourceType::CommonJs
        } else {
            SourceType::Module
        };
        let Ok(sc) = SourceCode::parse(
            &code,
            &ParseOptions {
                source_type,
                jsx,
                global_return,
            },
        ) else {
            skipped += 1;
            continue;
        };
        let text = format!(
            "{filename} {jsx} {global_return}\n{}",
            eslint_linter_rs::code_path::dump(&sc)
        );
        let _ = std::fs::write(format!("{out}/{name}.txt"), text);
        ok += 1;
    }
    println!("dumped {ok}, skipped {skipped}");
}
