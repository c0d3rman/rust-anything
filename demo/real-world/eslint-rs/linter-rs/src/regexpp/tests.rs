use super::parser::{K, RegExpAst};
use super::validator::Ref;
use super::{parse_pattern, validate_flags, validate_pattern};

fn u16s(s: &str) -> Vec<u16> {
    s.encode_utf16().collect()
}

fn err(p: &str, u: bool, v: bool) -> Option<String> {
    validate_pattern(&u16s(p), u, v).err().map(|e| e.message)
}

fn js_num(n: f64) -> String {
    if n.is_infinite() {
        return if n > 0.0 {
            "Infinity".into()
        } else {
            "-Infinity".into()
        };
    }
    if n.fract() == 0.0 && n.abs() < 1e21 {
        return format!("{}", n as i128);
    }
    format!("{n}")
}

fn ser(ast: &RegExpAst, id: usize) -> String {
    let n = ast.node(id);
    let mut s = format!("{}@{}-{}", n.type_name(), n.start, n.end);
    match &n.k {
        K::Character { value } => s += &format!(":{value}"),
        K::CharacterSet {
            kind,
            negate,
            key,
            value,
            strings,
        } => {
            s += &format!(
                ":{kind}:{negate}:{}:{}:{strings}",
                key.as_deref().unwrap_or(""),
                value.as_deref().unwrap_or("")
            )
        }
        K::Quantifier {
            min, max, greedy, ..
        } => s += &format!(":{}:{}:{greedy}", js_num(*min), js_num(*max)),
        K::Assertion { kind, negate, .. } => s += &format!(":{kind}:{negate}"),
        K::CapturingGroup {
            name, references, ..
        } => {
            let refs: Vec<String> = references
                .iter()
                .map(|r| ast.node(*r).start.to_string())
                .collect();
            s += &format!(":{}:{}", name.as_deref().unwrap_or(""), refs.join(","))
        }
        K::Backreference {
            reference,
            ambiguous,
            resolved,
        } => {
            let r = match reference {
                Ref::Num(n) => js_num(*n),
                Ref::Name(n) => n.clone(),
            };
            let g: Vec<String> = resolved
                .iter()
                .map(|r| ast.node(*r).start.to_string())
                .collect();
            s += &format!(":{r}:{ambiguous}:{}", g.join(","))
        }
        K::CharacterClass {
            negate,
            unicode_sets,
            ..
        } => s += &format!(":{negate}:{unicode_sets}"),
        K::ExpressionCharacterClass { negate, .. } => s += &format!(":{negate}"),
        K::ModifierFlags {
            ignore_case,
            multiline,
            dot_all,
        } => s += &format!(":{ignore_case}:{multiline}:{dot_all}"),
        _ => {}
    }
    if let Some(p) = n.parent {
        let p = ast.node(p);
        s += &format!("^{}@{}", p.type_name(), p.start);
    }
    s
}

fn dump(ast: &RegExpAst) -> String {
    let mut out = Vec::new();
    ast.visit(&mut |a, id, enter| {
        if enter {
            out.push(ser(a, id));
        }
    });
    out.join("\n")
}

#[test]
fn messages() {
    assert_eq!(err("a", false, false), None);
    assert_eq!(
        err("(", false, false).as_deref(),
        Some("Invalid regular expression: /(/: Unterminated group")
    );
    assert_eq!(
        err("a)", true, false).as_deref(),
        Some("Invalid regular expression: /a)/u: Unmatched ')'")
    );
    assert_eq!(
        err("[a", false, true).as_deref(),
        Some("Invalid regular expression: /[a/v: Unterminated character class")
    );
    assert_eq!(
        err("a", true, true).as_deref(),
        Some("Invalid regular expression: /a/uv: Invalid regular expression flags")
    );
    assert_eq!(
        err("\\", false, false).as_deref(),
        Some("Invalid regular expression: /\\/: \\ at end of pattern")
    );
    assert_eq!(
        err("a{2,1}", false, false).as_deref(),
        Some("Invalid regular expression: /a{2,1}/: numbers out of order in {} quantifier")
    );
    assert_eq!(
        err("{1}", false, false).as_deref(),
        Some("Invalid regular expression: /{1}/: Nothing to repeat")
    );
    assert_eq!(err("a{1", false, false), None);
    assert_eq!(
        err("a{1", true, false).as_deref(),
        Some("Invalid regular expression: /a{1/u: Incomplete quantifier")
    );
    assert_eq!(
        err("\\p{Foo}", true, false).as_deref(),
        Some("Invalid regular expression: /\\p{Foo}/u: Invalid property name")
    );
    assert_eq!(err("\\p{Foo}", false, false), None);
    assert_eq!(err("[\\p{RGI_Emoji}]", false, true), None);
    assert_eq!(
        err("[^\\p{RGI_Emoji}]", false, true).as_deref(),
        Some(
            "Invalid regular expression: /[^\\p{RGI_Emoji}]/v: Negated character class may contain strings"
        )
    );
    assert_eq!(err("(?<a>x)|(?<a>y)", false, false), None);
    assert_eq!(
        err("(?<a>x)(?<a>y)", false, false).as_deref(),
        Some("Invalid regular expression: /(?<a>x)(?<a>y)/: Duplicate capture group name")
    );
    assert_eq!(err("\\k<a>", false, false), None);
    assert_eq!(
        err("\\k<a>(?<b>)", false, false).as_deref(),
        Some("Invalid regular expression: /\\k<a>(?<b>)/: Invalid named capture referenced")
    );
    assert_eq!(
        err("(?i-i:a)", false, false).as_deref(),
        Some("Invalid regular expression: /(?i-i:a)/: Duplicated flag 'i'")
    );
    assert_eq!(
        err("(?-:a)", false, false).as_deref(),
        Some("Invalid regular expression: /(?-:a)/: Invalid empty flags")
    );
    assert_eq!(
        err("[a&&&b]", false, true).as_deref(),
        Some("Invalid regular expression: /[a&&&b]/v: Invalid character in character class")
    );
    assert_eq!(
        err("[z-a]", false, false).as_deref(),
        Some("Invalid regular expression: /[z-a]/: Range out of order in character class")
    );
    assert_eq!(err("\\u{1F600}", true, false), None);
    assert_eq!(err("(?<\u{1d49c}>.)\\k<\u{1d49c}>", false, false), None);
}

#[test]
fn flags() {
    assert_eq!(validate_flags(&u16s("gimsuyvd")), Ok(()));
    assert_eq!(
        validate_flags(&u16s("gg")).map_err(|e| e.message),
        Err("Invalid regular expression: Duplicated flag 'g'".into())
    );
    assert_eq!(
        validate_flags(&u16s("x")).map_err(|e| e.message),
        Err("Invalid regular expression: Invalid flag 'x'".into())
    );
}

#[test]
fn lone_surrogate_is_lossy() {
    let p: Vec<u16> = vec![0xd83d, 0x29];
    let e = validate_pattern(&p, false, false).err();
    assert!(e.is_some_and(|e| e.lossy));
}

#[test]
fn ast_shapes() {
    let ast = parse_pattern(&u16s("[a&&b]"), false, true).ok();
    let d = ast.as_ref().map(dump).unwrap_or_default();
    assert!(d.contains("ExpressionCharacterClass@0-6:false^Alternative@0"));
    assert!(d.contains("ClassIntersection@1-5^ExpressionCharacterClass@0"));
    let ast = parse_pattern(&u16s("(a)\\1*"), false, false).ok();
    let d = ast.as_ref().map(dump).unwrap_or_default();
    assert!(d.contains("Quantifier@3-6:0:Infinity:true^Alternative@0"));
    assert!(d.contains("Backreference@3-5:1:false:0^Quantifier@3"));
    let ast = parse_pattern(&u16s("[\\u{1F600}-\\u{1F64F}]"), true, false).ok();
    let d = ast.as_ref().map(dump).unwrap_or_default();
    assert!(d.contains("CharacterClassRange@1-20^CharacterClass@0"));
}

#[test]
fn corpus() {
    let Ok(path) = std::env::var("REGEXPP_CORPUS") else {
        return;
    };
    let Ok(text) = std::fs::read_to_string(&path) else {
        return;
    };
    let (mut same, mut diff, mut ast_same, mut ast_diff, mut flag_same, mut flag_diff) =
        (0, 0, 0, 0, 0, 0);
    let mut shown = 0;
    for line in text.lines() {
        let Ok(v) = serde_json::from_str::<serde_json::Value>(line) else {
            continue;
        };
        let units = |k: &str| -> Vec<u16> {
            v[k].as_array()
                .map(|a| {
                    a.iter()
                        .filter_map(|x| x.as_u64())
                        .map(|x| x as u16)
                        .collect()
                })
                .unwrap_or_default()
        };
        if v.get("f").is_some() {
            let got = validate_flags(&units("f")).err().map(|e| e.message);
            let want = v["err"]
                .as_array()
                .map(|_| String::from_utf16_lossy(&units("err")));
            if got == want {
                flag_same += 1;
            } else {
                flag_diff += 1;
                eprintln!(
                    "FLAGS {:?}: want {:?} got {:?}",
                    String::from_utf16_lossy(&units("f")),
                    v["err"],
                    got
                );
            }
            continue;
        }
        let p = units("p");
        let (u, vv) = (
            v["u"].as_bool().unwrap_or(false),
            v["v"].as_bool().unwrap_or(false),
        );
        let got = validate_pattern(&p, u, vv).err();
        let want = v["err"]
            .as_array()
            .map(|_| String::from_utf16_lossy(&units("err")));
        let exact = want
            .as_ref()
            .is_none_or(|w| w.encode_utf16().eq(units("err")));
        let lossy = got.as_ref().is_some_and(|e| e.lossy);
        if got.as_ref().map(|e| e.message.clone()) == want && lossy != exact {
            same += 1;
        } else {
            diff += 1;
            if shown < 30 {
                shown += 1;
                eprintln!(
                    "PATTERN {:?} u={u} v={vv}: want {:?} got {:?}",
                    String::from_utf16_lossy(&p),
                    want,
                    got
                );
            }
        }
        let got_ast = parse_pattern(&p, u, vv).ok().map(|a| dump(&a));
        if got_ast.as_deref() == v["ast"].as_str() {
            ast_same += 1;
        } else {
            ast_diff += 1;
            if shown < 30 {
                shown += 1;
                eprintln!(
                    "AST {:?} u={u} v={vv}:\nwant {:?}\ngot  {:?}\nperr {:?}",
                    String::from_utf16_lossy(&p),
                    v["ast"],
                    got_ast,
                    v["perr"]
                        .as_array()
                        .map(|_| String::from_utf16_lossy(&units("perr")))
                );
            }
        }
    }
    eprintln!(
        "validatePattern same={same} diff={diff}; parsePattern AST same={ast_same} diff={ast_diff}; validateFlags same={flag_same} diff={flag_diff}"
    );
    assert_eq!(diff + ast_diff + flag_diff, 0);
}
