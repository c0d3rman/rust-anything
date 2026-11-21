use eslint_linter_rs::source::{ParseOptions, SourceCode, SourceType};
use std::io::BufRead;

fn walk(sc: &SourceCode, id: u32, out: &mut Vec<String>) {
    let n = sc.ast.node(id);
    let extra = match n.kind {
        "Identifier" | "PrivateIdentifier" | "JSXIdentifier" => {
            format!(" {}", sc.ast.name(id).unwrap_or(""))
        }
        "Literal" => format!(" {}", sc.ast.str(id, "raw").unwrap_or("")),
        _ => String::new(),
    };
    out.push(format!("{} {} {}{}", n.kind, n.start, n.end, extra));
    for c in sc.ast.children(id) {
        walk(sc, c, out);
    }
}

fn main() {
    let path = std::env::args().nth(1).unwrap();
    let f = std::io::BufReader::new(std::fs::File::open(path).unwrap());
    for line in f.lines() {
        let line = line.unwrap();
        let c: serde_json::Value = serde_json::from_str(&line).unwrap();
        let name = c["name"].as_str().unwrap();
        let args: Vec<&str> = c["args"]
            .as_array()
            .unwrap()
            .iter()
            .filter_map(|a| a.as_str())
            .collect();
        let code = c["stdin"].as_str().unwrap_or("");
        let opts = ParseOptions {
            source_type: if args.contains(&"input.cjs") {
                SourceType::CommonJs
            } else {
                SourceType::Module
            },
            jsx: args.iter().any(|a| a.contains("jsx")),
            global_return: false,
        };
        println!("## {name}");
        match SourceCode::parse(code, &opts) {
            Err(e) => println!("ERROR {:?}", e),
            Ok(sc) => {
                let mut out = Vec::new();
                walk(&sc, sc.root, &mut out);
                for t in &sc.tokens {
                    out.push(format!(
                        "T {} {} {} {}",
                        t.kind.as_str(),
                        t.start,
                        t.end,
                        serde_json::to_string(&t.value).unwrap()
                    ));
                }
                for t in &sc.comments {
                    out.push(format!(
                        "C {} {} {} {}",
                        t.kind.as_str(),
                        t.start,
                        t.end,
                        serde_json::to_string(&t.value).unwrap()
                    ));
                }
                println!("{}", out.join("\n"));
            }
        }
    }
}
