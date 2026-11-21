//! lib/rules/preserve-caught-error.js

use crate::ast::NodeId;
use crate::linter::Cx;
use crate::report::{Fix, SugVal};
use crate::rule::{Report, SuggestSpec, fixer};
use crate::rules::{Rule, RuleDef};
use crate::source::Token;
use crate::tokens::{N, Opts};
use crate::utils::{get_static_property_name, is_closing_paren, is_opening_paren};
use serde_json::Value;

pub static DEF: RuleDef = RuleDef {
    name: "preserve-caught-error",
    messages: &[
        (
            "missingCause",
            "There is no `cause` attached to the symptom error being thrown.",
        ),
        (
            "incorrectCause",
            "The symptom error is being thrown with an incorrect `cause`.",
        ),
        (
            "includeCause",
            "Include the original caught error as the `cause` of the symptom error.",
        ),
        (
            "missingCatchErrorParam",
            "The caught error is not accessible because the catch clause lacks the error parameter. Start referencing the caught error using the catch parameter.",
        ),
        (
            "partiallyLostError",
            "Re-throws cannot preserve the caught error as a part of it is being lost due to destructuring.",
        ),
        (
            "caughtErrorShadowed",
            "The caught error is being attached as `cause`, but is shadowed by a closer scoped redeclaration.",
        ),
    ],
    has_default_options: true,
    create,
};

const BUILT_IN_ERROR_TYPES: &[&str] = &[
    "Error",
    "EvalError",
    "RangeError",
    "ReferenceError",
    "SyntaxError",
    "TypeError",
    "URIError",
    "AggregateError",
];

struct R {
    require_catch_parameter: bool,
    class_names: Vec<(String, i64)>,
}

fn create(options: &[Value]) -> Result<Box<dyn Rule>, String> {
    let bad = || Err("invalid option".to_string());
    let mut r = R {
        require_catch_parameter: false,
        class_names: Vec::new(),
    };
    match options {
        [] => {}
        [Value::Object(o)] => {
            for (k, v) in o {
                match (k.as_str(), v) {
                    ("requireCatchParameter", Value::Bool(b)) => r.require_catch_parameter = *b,
                    ("errorClassNames", Value::Array(items)) => {
                        for (i, item) in items.iter().enumerate() {
                            if items[..i].contains(item) {
                                return bad();
                            }
                            let entry = match item {
                                Value::String(s) => (s.clone(), 2),
                                Value::Object(m) => {
                                    if m.keys().any(|k| k != "name" && k != "argumentPosition") {
                                        return bad();
                                    }
                                    let Some(Value::String(name)) = m.get("name") else {
                                        return bad();
                                    };
                                    let Some(pos) =
                                        m.get("argumentPosition").and_then(|p| p.as_f64())
                                    else {
                                        return bad();
                                    };
                                    if pos.fract() != 0.0 || pos < 1.0 {
                                        return bad();
                                    }
                                    (name.clone(), pos as i64)
                                }
                                _ => return bad(),
                            };
                            match r.class_names.iter_mut().find(|(n, _)| *n == entry.0) {
                                Some(slot) => slot.1 = entry.1,
                                None => r.class_names.push(entry),
                            }
                        }
                    }
                    _ => return bad(),
                }
            }
        }
        _ => return bad(),
    }
    Ok(Box::new(r))
}

enum Cause {
    Unknown,
    None,
    Found { value: NodeId, multiple: bool },
}

fn get_error_cause(cx: &Cx, expr: NodeId, options_index: i64) -> Cause {
    let ast = &cx.sc.ast;
    let args: Vec<NodeId> = ast.nodes_of(expr, "arguments").collect();
    if let Some(i) = args.iter().position(|a| ast.is(*a, "SpreadElement"))
        && (i as i64) <= options_index
    {
        return Cause::Unknown;
    }
    let Some(opts) = usize::try_from(options_index)
        .ok()
        .and_then(|i| args.get(i).copied())
    else {
        return Cause::None;
    };
    if !ast.is(opts, "ObjectExpression") {
        return Cause::Unknown;
    }
    let props: Vec<NodeId> = ast.nodes_of(opts, "properties").collect();
    if props.iter().any(|p| ast.is(*p, "SpreadElement")) {
        return Cause::Unknown;
    }
    let causes: Vec<NodeId> = props
        .into_iter()
        .filter(|p| get_static_property_name(ast, *p).as_deref() == Some("cause"))
        .collect();
    match causes.last().and_then(|p| ast.child(*p, "value")) {
        Some(value) => Cause::Found {
            value,
            multiple: causes.len() > 1,
        },
        None => Cause::None,
    }
}

fn find_parent_catch(cx: &Cx, node: NodeId) -> Option<NodeId> {
    let ast = &cx.sc.ast;
    let mut cur = Some(node);
    while let Some(n) = cur {
        if ast.is(n, "CatchClause") {
            return Some(n);
        }
        if matches!(
            ast.kind(n),
            "FunctionDeclaration"
                | "FunctionExpression"
                | "ArrowFunctionExpression"
                | "StaticBlock"
        ) {
            return None;
        }
        cur = ast.parent(n);
    }
    None
}

fn insert_after(t: &Token, text: String) -> Fix {
    fixer::insert_after_range((t.start, t.end), text)
}

fn insert_cause_into_options(cx: &Cx, options: NodeId, name: &str) -> Option<Fix> {
    let ast = &cx.sc.ast;
    match ast.nodes_of(options, "properties").last() {
        None => Some(insert_after(
            cx.sc.first_token(options, N)?,
            format!("cause: {name}"),
        )),
        Some(last) => Some(fixer::insert_after_range(
            ast.range(last),
            format!(", cause: {name}"),
        )),
    }
}

fn insertion_token_after_parens<'a>(cx: &'a Cx, first: &'a Token, last_index: u32) -> &'a Token {
    let mut token = first;
    while let Some(next) = cx.sc.token_after_tok(token, N) {
        if !(next.end <= last_index && is_closing_paren(next)) {
            break;
        }
        token = next;
    }
    token
}

fn add_arguments_to_empty_call(cx: &Cx, expr: NodeId, text: &str) -> Option<Fix> {
    let sc = cx.sc;
    let closing = sc.last_token(expr, N)?;
    let callee = sc.ast.child(expr, "callee")?;
    let last_callee = sc.last_token(callee, N)?;
    if let Some(p) = sc
        .tokens_between(last_callee.end, closing.start, N)
        .iter()
        .find(|t| is_opening_paren(t))
    {
        return Some(insert_after(p, text.to_string()));
    }
    let t = insertion_token_after_parens(cx, last_callee, closing.end);
    Some(insert_after(t, format!("({text})")))
}

fn append_arguments(cx: &Cx, expr: NodeId, text: &str) -> Option<Fix> {
    let sc = cx.sc;
    let last_arg = sc.ast.nodes_of(expr, "arguments").last()?;
    let last_arg_token = sc.last_token(last_arg, N)?;
    let before_paren = sc.last_token(
        expr,
        Opts {
            include_comments: false,
            skip: 1,
        },
    )?;
    let t = insertion_token_after_parens(cx, last_arg_token, before_paren.end);
    Some(insert_after(t, text.to_string()))
}

impl R {
    fn is_built_in_global_error(&self, cx: &Cx, callee: NodeId) -> bool {
        let ast = &cx.sc.ast;
        ast.is(callee, "Identifier")
            && ast
                .name(callee)
                .is_some_and(|n| BUILT_IN_ERROR_TYPES.contains(&n))
            && cx.sm().is_global_reference(ast, callee)
    }

    fn is_throwing_new_error(&self, cx: &Cx, arg: NodeId) -> bool {
        let ast = &cx.sc.ast;
        if !matches!(ast.kind(arg), "NewExpression" | "CallExpression") {
            return false;
        }
        let Some(callee) = ast.child(arg, "callee") else {
            return false;
        };
        if self.is_built_in_global_error(cx, callee) {
            return true;
        }
        let target = if ast.is(callee, "MemberExpression") && !ast.bool(callee, "computed") {
            ast.child(callee, "property")
        } else {
            Some(callee)
        };
        target.is_some_and(|t| {
            ast.is(t, "Identifier")
                && ast
                    .name(t)
                    .is_some_and(|n| self.class_names.iter().any(|(c, _)| c == n))
        })
    }

    fn missing_cause_fix(
        &self,
        cx: &Cx,
        expr: NodeId,
        builtin: bool,
        class: &str,
        options_index: i64,
        caught: &str,
    ) -> Option<Fix> {
        let ast = &cx.sc.ast;
        let args: Vec<NodeId> = ast.nodes_of(expr, "arguments").collect();
        let existing = |o: NodeId| {
            if ast.is(o, "ObjectExpression") {
                insert_cause_into_options(cx, o, caught)
            } else {
                None
            }
        };
        if builtin && class == "AggregateError" {
            if args.is_empty() {
                return add_arguments_to_empty_call(
                    cx,
                    expr,
                    &format!("[], \"\", {{ cause: {caught} }}"),
                );
            }
            if args.len() < 2 {
                return append_arguments(cx, expr, &format!(", \"\", {{ cause: {caught} }}"));
            }
            if args.len() < 3 {
                return append_arguments(cx, expr, &format!(", {{ cause: {caught} }}"));
            }
            return existing(args[2]);
        }
        if builtin {
            if args.is_empty() {
                return add_arguments_to_empty_call(
                    cx,
                    expr,
                    &format!("\"\", {{ cause: {caught} }}"),
                );
            }
            if args.len() < 2 {
                return append_arguments(cx, expr, &format!(", {{ cause: {caught} }}"));
            }
            return existing(args[1]);
        }
        if (args.len() as i64) < options_index {
            return None;
        }
        match usize::try_from(options_index)
            .ok()
            .and_then(|i| args.get(i).copied())
        {
            Some(o) => existing(o),
            None => {
                if args.is_empty() {
                    add_arguments_to_empty_call(cx, expr, &format!("{{ cause: {caught} }}"))
                } else {
                    append_arguments(cx, expr, &format!(", {{ cause: {caught} }}"))
                }
            }
        }
    }
}

impl Rule for R {
    fn listeners(&self) -> &'static [&'static str] {
        &["ThrowStatement"]
    }

    fn on(&mut self, _: usize, node: NodeId, cx: &mut Cx) {
        let ast = &cx.sc.ast;
        let Some(catch) = find_parent_catch(cx, node) else {
            return;
        };
        let Some(expr) = ast.child(node, "argument") else {
            return;
        };
        if !self.is_throwing_new_error(cx, expr) {
            return;
        }
        let param = ast.child(catch, "param");
        if let Some(p) = param
            && !ast.is(p, "Identifier")
        {
            cx.report(Report::node(catch, "partiallyLostError"));
            return;
        }
        let Some(caught) = param else {
            if self.require_catch_parameter {
                cx.report(Report::node(node, "missingCatchErrorParam"));
            }
            return;
        };
        let caught_name = ast.name(caught).unwrap_or("").to_string();
        let Some(callee) = ast.child(expr, "callee") else {
            return;
        };
        let class = if ast.is(callee, "Identifier") {
            ast.name(callee)
        } else {
            ast.child(callee, "property").and_then(|p| ast.name(p))
        }
        .unwrap_or("")
        .to_string();
        let builtin = self.is_built_in_global_error(cx, callee);
        let options_index = if builtin {
            if class == "AggregateError" { 2 } else { 1 }
        } else {
            match self.class_names.iter().find(|(c, _)| *c == class) {
                Some((_, p)) => p - 1,
                None => return,
            }
        };
        match get_error_cause(cx, expr, options_index) {
            Cause::Unknown => {}
            Cause::None => {
                let mut r = Report::node(node, "missingCause");
                r.suggest.push(SuggestSpec {
                    keys: vec![
                        ("messageId", SugVal::Str("includeCause".into())),
                        ("fix", SugVal::Fix),
                    ],
                    fix: self.missing_cause_fix(
                        cx,
                        expr,
                        builtin,
                        &class,
                        options_index,
                        &caught_name,
                    ),
                });
                cx.report(r);
            }
            Cause::Found { value, multiple } => {
                if ast.is(value, "Identifier") && ast.name(value) == Some(caught_name.as_str()) {
                    let sm = cx.sm();
                    let mut scope = Some(cx.get_scope(node));
                    while let Some(s) = scope {
                        if sm.lookup(s, &caught_name).is_some() {
                            break;
                        }
                        scope = sm.scope(s).upper;
                    }
                    if scope.map(|s| sm.scope(s).block) != Some(catch) {
                        cx.report(Report::node(node, "caughtErrorShadowed"));
                    }
                    return;
                }
                let mut r = Report::node(value, "incorrectCause");
                if !multiple {
                    let fix = ast.parent(value).map(|p| {
                        if ast.bool(p, "method")
                            || ast.bool(p, "shorthand")
                            || ast.str(p, "kind") != Some("init")
                        {
                            fixer::replace(cx.sc, p, format!("cause: {caught_name}"))
                        } else {
                            fixer::replace(cx.sc, value, caught_name.as_str())
                        }
                    });
                    r.suggest.push(SuggestSpec {
                        keys: vec![
                            ("messageId", SugVal::Str("includeCause".into())),
                            ("fix", SugVal::Fix),
                        ],
                        fix,
                    });
                }
                cx.report(r);
            }
        }
    }
}
