//! Byte layout shared by the libFuzzer target and the driver's `decode`/`encode` commands:
//! byte 0 picks the file type, bytes 1-2 pick options, the rest is the UTF-8 input text.

pub const PATHS: [&str; 12] = [
    "fuzz.js", "fuzz.jsx", "fuzz.ts", "fuzz.tsx", "fuzz.json", "fuzz.json5", "fuzz.graphql",
    "fuzz.yaml", "fuzz.md", "fuzz.css", "fuzz.scss", "fuzz.less",
];
const WIDTHS: [&str; 8] = ["80", "40", "20", "10", "120", "5", "1", "60"];

pub fn decode(data: &[u8]) -> Option<(Vec<String>, &str)> {
    if data.len() < 3 {
        return None;
    }
    let text = std::str::from_utf8(&data[3..]).ok()?;
    let (b0, b1, b2) = (data[0], data[1], data[2]);
    let mut args = vec![
        "--stdin-filepath".to_string(),
        PATHS[b0 as usize % PATHS.len()].to_string(),
    ];
    let mut push = |a: &str| args.push(a.to_string());
    if b1 & 7 != 0 {
        push("--print-width");
        push(WIDTHS[(b1 & 7) as usize]);
    }
    if b1 & 8 != 0 {
        push("--use-tabs");
    }
    if b1 & 16 != 0 {
        push("--no-semi");
    }
    if b1 & 32 != 0 {
        push("--single-quote");
    }
    match b1 >> 6 {
        1 => {
            push("--trailing-comma");
            push("es5")
        }
        2 => {
            push("--trailing-comma");
            push("none")
        }
        _ => {}
    }
    if b2 & 1 != 0 {
        push("--arrow-parens");
        push("avoid");
    }
    match (b2 >> 1) & 3 {
        1 => {
            push("--quote-props");
            push("consistent")
        }
        2 => {
            push("--quote-props");
            push("preserve")
        }
        _ => {}
    }
    if b2 & 8 != 0 {
        push("--object-wrap");
        push("collapse");
    }
    match (b2 >> 4) & 3 {
        1 => {
            push("--prose-wrap");
            push("always")
        }
        2 => {
            push("--prose-wrap");
            push("never")
        }
        _ => {}
    }
    if b2 & 64 != 0 {
        push("--experimental-ternaries");
    }
    if b2 & 128 != 0 {
        push("--no-bracket-spacing");
    }
    Some((args, text))
}

/// Header byte 0 for a seed file path, if it is one of the fuzzed types.
pub fn type_byte(path: &str) -> Option<u8> {
    let lower = path.to_ascii_lowercase();
    let ext = lower.rsplit_once('.')?.1;
    let want = match ext {
        "js" | "mjs" | "cjs" => "js",
        "gql" | "graphqls" => "graphql",
        "yml" => "yaml",
        "markdown" => "md",
        "jsonc" => "json",
        e => e,
    };
    PATHS
        .iter()
        .position(|p| p.rsplit_once('.').unwrap().1 == want)
        .map(|i| i as u8)
}
