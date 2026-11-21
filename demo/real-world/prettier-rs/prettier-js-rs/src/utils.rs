//! `src/language-js/utils/index.js` and the string/number helpers of `src/utils/`.

use crate::value::Value;

use crate::node::*;
use crate::options::Options;
use crate::printer::Printer;

pub fn width(text: &str) -> usize {
    doc_printer_rs::port::string_width(text) as usize
}

pub fn precedence(op: &str) -> i32 {
    precedence_of(op)
}

pub fn precedence_of(op: &str) -> i32 {
    match op {
        "|>" => 0,
        "??" => 1,
        "||" => 2,
        "&&" => 3,
        "|" => 4,
        "^" => 5,
        "&" => 6,
        "==" | "===" | "!=" | "!==" => 7,
        "<" | ">" | "<=" | ">=" | "in" | "instanceof" => 8,
        ">>" | "<<" | ">>>" => 9,
        "+" | "-" => 10,
        "*" | "/" | "%" => 11,
        "**" => 12,
        _ => -100,
    }
}

fn is_equality(op: &str) -> bool {
    matches!(op, "==" | "!=" | "===" | "!==")
}

fn is_multiplicative(op: &str) -> bool {
    matches!(op, "*" | "/" | "%")
}

fn is_bitshift(op: &str) -> bool {
    matches!(op, ">>" | ">>>" | "<<")
}

pub fn should_flatten(parent_op: &str, node_op: &str) -> bool {
    if precedence_of(node_op) != precedence_of(parent_op) {
        return false;
    }
    if parent_op == "**" {
        return false;
    }
    if is_equality(parent_op) && is_equality(node_op) {
        return false;
    }
    if (node_op == "%" && is_multiplicative(parent_op))
        || (parent_op == "%" && is_multiplicative(node_op))
    {
        return false;
    }
    if node_op != parent_op && is_multiplicative(node_op) && is_multiplicative(parent_op) {
        return false;
    }
    if is_bitshift(parent_op) && is_bitshift(node_op) {
        return false;
    }
    true
}

pub fn is_bitwise_operator(op: &str) -> bool {
    is_bitshift(op) || matches!(op, "|" | "^" | "&")
}

pub fn is_binary_cast_expression(n: &Value) -> bool {
    matches!(
        n.ty(),
        "AsExpression"
            | "AsConstExpression"
            | "SatisfiesExpression"
            | "TSAsExpression"
            | "TSSatisfiesExpression"
    )
}

pub fn is_nullish_coalescing(n: &Value) -> bool {
    n.is("LogicalExpression") && n.str_of("operator") == Some("??")
}

pub fn is_signed_numeric_literal(n: &Value) -> bool {
    n.is("UnaryExpression")
        && matches!(n.str_of("operator"), Some("+" | "-"))
        && n.field("argument").is_some_and(is_numeric_literal)
}

pub fn is_memberish(n: &Value) -> bool {
    is_member_expression(n) || (n.is("BindExpression") && n.field("object").is_some())
}

pub fn is_call_like_expression(n: &Value) -> bool {
    is_call_expression(n) || matches!(n.ty(), "NewExpression" | "ImportExpression")
}

pub fn has_naked_left_side(n: &Value) -> bool {
    matches!(
        n.ty(),
        "AssignmentExpression"
            | "BinaryExpression"
            | "LogicalExpression"
            | "NGPipeExpression"
            | "ConditionalExpression"
            | "SequenceExpression"
            | "TaggedTemplateExpression"
            | "BindExpression"
            | "TSNonNullExpression"
            | "ChainExpression"
    ) || is_call_expression(n)
        || is_member_expression(n)
        || (n.is("UpdateExpression") && !n.flag("prefix"))
        || is_binary_cast_expression(n)
}

pub fn left_side(n: &Value) -> Option<&Value> {
    if let Some(Value::Array(e)) = n.get("expressions") {
        return e.first();
    }
    [
        "left",
        "test",
        "callee",
        "object",
        "tag",
        "argument",
        "expression",
    ]
    .iter()
    .find_map(|k| n.field(k))
}

pub fn left_side_path(n: &Value) -> Vec<(&'static str, Option<usize>)> {
    if matches!(n.get("expressions"), Some(Value::Array(_))) {
        return vec![("expressions", Some(0))];
    }
    for k in [
        "left",
        "test",
        "object",
        "callee",
        "tag",
        "argument",
        "expression",
    ] {
        if n.field(k).is_some() {
            return vec![(k, None)];
        }
    }
    Vec::new()
}

pub fn starts_with_no_lookahead_token(n: &Value, pred: &dyn Fn(&Value) -> bool) -> bool {
    let go = |k: &str| {
        n.field(k)
            .is_some_and(|c| starts_with_no_lookahead_token(c, pred))
    };
    match n.ty() {
        "BinaryExpression" | "LogicalExpression" | "AssignmentExpression" | "NGPipeExpression" => {
            go("left")
        }
        "MemberExpression" | "OptionalMemberExpression" => go("object"),
        "TaggedTemplateExpression" => {
            if n.field("tag").is_some_and(|t| t.is("FunctionExpression")) {
                return false;
            }
            go("tag")
        }
        "CallExpression" | "OptionalCallExpression" => {
            if n.field("callee")
                .is_some_and(|t| t.is("FunctionExpression"))
            {
                return false;
            }
            go("callee")
        }
        "ConditionalExpression" => go("test"),
        "UpdateExpression" => !n.flag("prefix") && go("argument"),
        "BindExpression" => go("object"),
        "SequenceExpression" => n
            .list("expressions")
            .first()
            .is_some_and(|c| starts_with_no_lookahead_token(c, pred)),
        "ChainExpression"
        | "TSSatisfiesExpression"
        | "TSAsExpression"
        | "TSNonNullExpression"
        | "AsExpression"
        | "AsConstExpression"
        | "SatisfiesExpression" => go("expression"),
        _ => pred(n),
    }
}

pub fn call_arguments(n: &Value) -> Vec<&Value> {
    let n = skip_chain(n);
    if n.is("ImportExpression") || n.is("TSImportType") {
        let key = if n.is("ImportExpression") {
            "source"
        } else {
            "argument"
        };
        let mut out: Vec<&Value> = n.field(key).into_iter().collect();
        if let Some(o) = n.field("options") {
            out.push(o);
        }
        return out;
    }
    n.list("arguments").iter().collect()
}

pub fn function_parameters(n: &Value) -> Vec<&Value> {
    let mut out = Vec::new();
    if let Some(t) = n.field("this") {
        out.push(t);
    }
    if let Some(Value::Array(p)) = n.get("parameters") {
        out.extend(p.iter());
    } else {
        out.extend(n.list("params").iter());
    }
    if let Some(r) = n.field("rest") {
        out.push(r);
    }
    out
}

pub fn has_rest_parameter(n: &Value) -> bool {
    n.field("rest").is_some()
        || function_parameters(n)
            .last()
            .is_some_and(|p| p.is("RestElement"))
}

pub fn is_single_word_type(n: &Value) -> bool {
    matches!(
        n.ty(),
        "Identifier" | "ThisExpression" | "Super" | "PrivateName" | "PrivateIdentifier"
    )
}

pub fn is_simple_call_argument(n: &Value, depth: i32) -> bool {
    if depth <= 0 {
        return false;
    }
    if n.is("ChainExpression") || n.is("TSNonNullExpression") {
        return n
            .field("expression")
            .is_some_and(|e| is_simple_call_argument(e, depth));
    }
    let child = |c: &Value| is_simple_call_argument(c, depth - 1);
    if is_regexp_literal(n) {
        let pattern = n
            .get("regex")
            .and_then(|r| r.str_of("pattern"))
            .unwrap_or("");
        return width(pattern) <= 5;
    }
    if is_literal(n) || is_single_word_type(n) || n.is("ArgumentPlaceholder") {
        return true;
    }
    if n.is("TemplateLiteral") {
        return n.list("quasis").iter().all(|q| {
            !q.get("value")
                .and_then(|v| v.str_of("raw"))
                .unwrap_or("")
                .contains('\n')
        }) && n.list("expressions").iter().all(child);
    }
    if n.is("ObjectExpression") {
        return n.list("properties").iter().all(|p| {
            !p.flag("computed") && (p.flag("shorthand") || p.field("value").is_some_and(child))
        });
    }
    if n.is("ArrayExpression") {
        return n.list("elements").iter().all(|x| x.is_null() || child(x));
    }
    if is_call_like_expression(n) {
        if n.is("ImportExpression")
            || n.field("callee")
                .is_some_and(|c| is_simple_call_argument(c, depth))
        {
            let args = call_arguments(n);
            return args.len() as i32 <= depth && args.iter().all(|a| child(a));
        }
        return false;
    }
    if is_member_expression(n) {
        return n
            .field("object")
            .is_some_and(|o| is_simple_call_argument(o, depth))
            && n.field("property")
                .is_some_and(|o| is_simple_call_argument(o, depth));
    }
    if (n.is("UnaryExpression") && matches!(n.str_of("operator"), Some("!" | "-" | "+" | "~")))
        || n.is("UpdateExpression")
    {
        return n
            .field("argument")
            .is_some_and(|a| is_simple_call_argument(a, depth));
    }
    false
}

pub fn is_lone_short_argument(
    n: &Value,
    print_width: usize,
    opts: &Options,
    c: &crate::comments::Comments,
) -> bool {
    if c.has_any(n) {
        return false;
    }
    let threshold = print_width as f64 * 0.25;
    if n.is("ThisExpression")
        || (n.is("Identifier")
            && (n.str_of("name").unwrap_or("").encode_utf16().count() as f64) <= threshold)
        || (is_signed_numeric_literal(n) && !n.field("argument").is_some_and(|a| c.has_any(a)))
    {
        return true;
    }
    if let Some(pattern) = n.get("regex").and_then(|r| r.str_of("pattern"))
        && n.is("Literal")
    {
        return pattern.encode_utf16().count() as f64 <= threshold;
    }
    if is_string_literal(n) {
        let raw = n.str_of("raw").unwrap_or("");
        return print_string(raw, opts).encode_utf16().count() as f64 <= threshold;
    }
    if n.is("TemplateLiteral") {
        let quasis = n.list("quasis");
        let raw = quasis
            .first()
            .and_then(|q| q.get("value"))
            .and_then(|v| v.str_of("raw"))
            .unwrap_or("");
        return n.list("expressions").is_empty()
            && raw.encode_utf16().count() as f64 <= threshold
            && !raw.contains('\n');
    }
    if n.is("UnaryExpression") {
        return n
            .field("argument")
            .is_some_and(|a| is_lone_short_argument(a, print_width, opts, c));
    }
    if n.is("CallExpression")
        && n.list("arguments").is_empty()
        && let Some(callee) = n.field("callee")
        && callee.is("Identifier")
    {
        return (callee.str_of("name").unwrap_or("").encode_utf16().count() as f64)
            <= threshold - 2.0;
    }
    is_literal(n)
}

fn is_node_matches_name_or_path(mut n: &Value, path: &str) -> bool {
    let names: Vec<&str> = path.split('.').collect();
    for (index, name) in names.iter().enumerate().rev() {
        if index == 0 {
            return n.is("Identifier") && n.str_of("name") == Some(name);
        }
        if !n.is("MemberExpression")
            || n.flag("optional")
            || n.flag("computed")
            || !n
                .field("property")
                .is_some_and(|p| p.is("Identifier") && p.str_of("name") == Some(name))
        {
            return false;
        }
        match n.field("object") {
            Some(o) => n = o,
            None => return false,
        }
    }
    false
}

const TEST_CALL_CALLEES: &[&str] = &[
    "it",
    "it.only",
    "it.skip",
    "describe",
    "describe.only",
    "describe.skip",
    "test",
    "test.only",
    "test.skip",
    "test.step",
    "test.describe",
    "test.describe.only",
    "test.describe.parallel",
    "test.describe.parallel.only",
    "test.describe.serial",
    "test.describe.serial.only",
    "skip",
    "xit",
    "xdescribe",
    "xtest",
    "fit",
    "fdescribe",
    "ftest",
];

fn is_angular_test_wrapper(n: &Value) -> bool {
    is_call_expression(n)
        && n.field("callee").is_some_and(|c| {
            c.is("Identifier")
                && matches!(
                    c.str_of("name"),
                    Some("async" | "inject" | "fakeAsync" | "waitForAsync")
                )
        })
}

fn is_unit_test_setup_identifier(n: &Value) -> bool {
    n.is("Identifier")
        && matches!(
            n.str_of("name"),
            Some("beforeEach" | "beforeAll" | "afterEach" | "afterAll")
        )
}

pub fn is_test_call(n: &Value, parent: Option<&Value>) -> bool {
    if !n.is("CallExpression") || n.flag("optional") {
        return false;
    }
    let args = call_arguments(n);
    let callee = n.field("callee");
    if args.len() == 1 {
        if is_angular_test_wrapper(n) && parent.is_some_and(|p| is_test_call(p, None)) {
            return is_function_or_arrow(args[0]);
        }
        if callee.is_some_and(is_unit_test_setup_identifier) {
            return is_angular_test_wrapper(args[0]);
        }
    } else if (args.len() == 2 || args.len() == 3)
        && (args[0].is("TemplateLiteral") || is_string_literal(args[0]))
        && callee.is_some_and(|c| {
            TEST_CALL_CALLEES
                .iter()
                .any(|p| is_node_matches_name_or_path(c, p))
        })
    {
        if args.len() == 3 && !is_numeric_literal(args[2]) {
            return false;
        }
        let with_body = |a: &Value| {
            a.is("FunctionExpression")
                || (a.is("ArrowFunctionExpression")
                    && a.field("body").is_some_and(|b| b.is("BlockStatement")))
        };
        return (if args.len() == 2 {
            is_function_or_arrow(args[1])
        } else {
            with_body(args[1]) && function_parameters(args[1]).len() <= 1
        }) || is_angular_test_wrapper(args[1]);
    }
    false
}

/// Whether `isTestCall(node)` throws: typescript-estree wraps `b?.()` in a `ChainExpression`,
/// which `isCallExpression` sees through and `isAngularTestWrapper` then reads `.callee` of.
pub fn test_call_crashes(n: &Value) -> bool {
    if !n.is("CallExpression") || n.flag("optional") {
        return false;
    }
    let chained_call = |a: &Value| a.is("ChainExpression") && is_call_expression(a);
    let args = call_arguments(n);
    let callee = n.field("callee");
    if args.len() == 1 {
        return !is_angular_test_wrapper(n)
            && callee.is_some_and(is_unit_test_setup_identifier)
            && chained_call(args[0]);
    }
    (args.len() == 2 || (args.len() == 3 && is_numeric_literal(args[2])))
        && (args[0].is("TemplateLiteral") || is_string_literal(args[0]))
        && callee.is_some_and(|c| {
            TEST_CALL_CALLEES
                .iter()
                .any(|p| is_node_matches_name_or_path(c, p))
        })
        && chained_call(args[1])
}

pub fn template_literal_has_newlines(n: &Value) -> bool {
    n.list("quasis").iter().any(|q| {
        q.get("value")
            .and_then(|v| v.str_of("raw"))
            .unwrap_or("")
            .contains('\n')
    })
}

pub fn is_template_on_its_own_line(n: &Value, text: &str) -> bool {
    ((n.is("TemplateLiteral") && template_literal_has_newlines(n))
        || (n.is("TaggedTemplateExpression")
            && n.field("quasi").is_some_and(template_literal_has_newlines)))
        && !crate::text::has_newline(text, n.start(), true)
}

pub fn is_function_composition_args(args: &[&Value]) -> bool {
    if args.len() <= 1 {
        return false;
    }
    let mut count = 0;
    for arg in args {
        if is_function_or_arrow(arg) {
            count += 1;
            if count > 1 {
                return true;
            }
        } else if is_call_expression(arg)
            && call_arguments(arg).iter().any(|c| is_function_or_arrow(c))
        {
            return true;
        }
    }
    false
}

pub fn is_long_curried_call_expression(p: &Printer<'_>) -> bool {
    let node = p.node();
    let Some(parent) = p.parent() else {
        return false;
    };
    p.key() == Some("callee")
        && is_call_expression(node)
        && is_call_expression(parent)
        && !parent.list("arguments").is_empty()
        && node.list("arguments").len() > parent.list("arguments").len()
}

pub fn is_next_line_empty(n: &Value, text: &str) -> bool {
    crate::text::is_next_line_empty(text, n.end())
}

pub fn get_preferred_quote(text: &str, prefer_single: bool) -> char {
    let (preferred, alternate) = if prefer_single {
        ('\'', '"')
    } else {
        ('"', '\'')
    };
    let mut p = 0;
    let mut a = 0;
    for c in text.chars() {
        if c == preferred {
            p += 1;
        } else if c == alternate {
            a += 1;
        }
    }
    if p > a { alternate } else { preferred }
}

pub fn make_string(raw: &str, enclosing: char) -> String {
    let other = if enclosing == '"' { '\'' } else { '"' };
    let mut out = String::with_capacity(raw.len() + 2);
    out.push(enclosing);
    let mut chars = raw.chars();
    while let Some(c) = chars.next() {
        if c == '\\' {
            match chars.next() {
                Some(e) if e == other => out.push(e),
                Some(e) => {
                    out.push('\\');
                    out.push(e);
                }
                None => out.push('\\'),
            }
        } else if c == enclosing {
            out.push('\\');
            out.push(c);
        } else {
            out.push(c);
        }
    }
    out.push(enclosing);
    out
}

pub fn print_string(raw: &str, opts: &Options) -> String {
    let content = &raw[1..raw.len() - 1];
    let enclosing = if matches!(
        opts.parser,
        crate::options::Parser::Json | crate::options::Parser::Jsonc
    ) || (opts.parser == crate::options::Parser::Json5
        && opts.quote_props == crate::options::QuoteProps::Preserve
        && !opts.single_quote)
    {
        '"'
    } else {
        get_preferred_quote(content, opts.single_quote)
    };
    if raw.starts_with(enclosing) {
        return raw.to_string();
    }
    make_string(content, enclosing)
}

pub fn print_number(raw: &str) -> String {
    if raw.len() == 1 {
        return raw.to_string();
    }
    let mut s = raw.to_ascii_lowercase();
    s = re_replace(&s, r"^([+-]?[\d.]+e)(?:\+|(-))?0*(\d)", "${1}${2}${3}");
    s = re_replace(&s, r"^([+-]?[\d.]+)e[+-]?0+$", "${1}");
    s = re_replace(&s, r"^([+-])?\.", "${1}0.");
    s = trim_fraction_zeros(&s);
    s = re_replace(&s, r"\.(e|$)", "${1}");
    s
}

fn trim_fraction_zeros(s: &str) -> String {
    let Some(dot) = s.find('.') else {
        return s.to_string();
    };
    let tail_start = s[dot..].find('e').map_or(s.len(), |e| dot + e);
    let digits = &s[dot + 1..tail_start];
    if digits.is_empty() || !digits.bytes().all(|b| b.is_ascii_digit()) || !digits.ends_with('0') {
        return s.to_string();
    }
    let trimmed = digits.trim_end_matches('0');
    let kept = if trimmed.is_empty() {
        &digits[..1]
    } else {
        trimmed
    };
    format!("{}.{}{}", &s[..dot], kept, &s[tail_start..])
}

fn re_replace(s: &str, pattern: &'static str, rep: &str) -> String {
    thread_local! {
        static CACHE: std::cell::RefCell<std::collections::HashMap<&'static str, Option<regex::Regex>>> =
            std::cell::RefCell::new(std::collections::HashMap::new());
    }
    CACHE.with(|cache| {
        let mut cache = cache.borrow_mut();
        match cache
            .entry(pattern)
            .or_insert_with(|| regex::Regex::new(pattern).ok())
        {
            Some(re) => re.replace(s, rep).into_owned(),
            None => s.to_string(),
        }
    })
}

pub fn is_simple_expression_by_node_count(n: &Value, max: usize) -> bool {
    fn count(n: &Value, max: isize) -> isize {
        let mut c = 0;
        if let Value::Object(map) = n {
            for v in map.values() {
                if v.get("type").is_some_and(Value::is_string) {
                    c += 1;
                    c += count(v, max - c);
                }
                if c > max {
                    return c;
                }
            }
        }
        c
    }
    count(n, max as isize) <= max as isize
}

pub fn has_leading_own_line_comment(c: &crate::comments::Comments, text: &str, n: &Value) -> bool {
    if is_jsx_element(n) {
        return c.has(n, crate::comments::Flags::PRETTIER_IGNORE);
    }
    c.has_with(n, crate::comments::Flags::LEADING, &|cm| {
        crate::text::has_newline(text, cm.end, false)
    })
}
