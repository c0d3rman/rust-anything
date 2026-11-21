//! lib/rules/object-shorthand.js

use crate::ast::{NodeId, Prop};
use crate::linter::Cx;
use crate::report::Fix;
use crate::rule::{Report, fixer};
use crate::rules::group_a::mini_regex::Regex;
use crate::rules::{Rule, RuleDef};
use crate::scope::ScopeType;
use crate::source::TokKind;
use crate::tokens::{N, Opts};
use crate::utils::{get_static_property_name, is_closing_bracket, is_opening_bracket};
use serde_json::Value;

pub static DEF: RuleDef = RuleDef {
    name: "object-shorthand",
    messages: &[
        (
            "expectedAllPropertiesShorthanded",
            "Expected shorthand for all properties.",
        ),
        (
            "expectedLiteralMethodLongform",
            "Expected longform method syntax for string literal keys.",
        ),
        ("expectedPropertyShorthand", "Expected property shorthand."),
        (
            "expectedPropertyLongform",
            "Expected longform property syntax.",
        ),
        ("expectedMethodShorthand", "Expected method shorthand."),
        ("expectedMethodLongform", "Expected longform method syntax."),
        (
            "unexpectedMix",
            "Unexpected mix of shorthand and non-shorthand properties.",
        ),
    ],
    has_default_options: true,
    create,
};

struct R {
    to_methods: bool,
    to_props: bool,
    never: bool,
    consistent: bool,
    consistent_as_needed: bool,
    ignore_constructors: bool,
    methods_ignore: Option<Regex>,
    avoid_quotes: bool,
    avoid_explicit_return_arrows: bool,
    lexical: Vec<Vec<NodeId>>,
    arrows_with_lexical: Vec<NodeId>,
    methods_with_super: Vec<NodeId>,
    arguments_ids: std::collections::HashSet<NodeId>,
    unsupported: bool,
}

fn create(options: &[Value]) -> Result<Box<dyn Rule>, String> {
    let bad = || Err("invalid option".to_string());
    let apply = match options.first() {
        None => "always",
        Some(Value::String(s))
            if matches!(
                s.as_str(),
                "always"
                    | "methods"
                    | "properties"
                    | "never"
                    | "consistent"
                    | "consistent-as-needed"
            ) =>
        {
            s.as_str()
        }
        _ => return bad(),
    };
    let mut r = R {
        to_methods: apply == "methods" || apply == "always",
        to_props: apply == "properties" || apply == "always",
        never: apply == "never",
        consistent: apply == "consistent",
        consistent_as_needed: apply == "consistent-as-needed",
        ignore_constructors: false,
        methods_ignore: None,
        avoid_quotes: false,
        avoid_explicit_return_arrows: false,
        lexical: Vec::new(),
        arrows_with_lexical: Vec::new(),
        methods_with_super: Vec::new(),
        arguments_ids: std::collections::HashSet::new(),
        unsupported: false,
    };
    match options.len() {
        0 | 1 => {}
        2 => {
            let Value::Object(o) = &options[1] else {
                return bad();
            };
            let only_quotes = o.keys().all(|k| k == "avoidQuotes");
            let second_ok = matches!(apply, "always" | "methods" | "properties") && only_quotes;
            let third_ok = matches!(apply, "always" | "methods")
                && o.keys().all(|k| {
                    matches!(
                        k.as_str(),
                        "ignoreConstructors"
                            | "methodsIgnorePattern"
                            | "avoidQuotes"
                            | "avoidExplicitReturnArrows"
                    )
                });
            if !second_ok && !third_ok {
                return bad();
            }
            for (k, v) in o {
                match (k.as_str(), v) {
                    ("methodsIgnorePattern", Value::String(p)) => {
                        if !p.is_empty() {
                            r.methods_ignore =
                                Some(Regex::new(p).ok_or("methodsIgnorePattern not supported")?);
                        }
                    }
                    ("methodsIgnorePattern", _) => return bad(),
                    (_, Value::Bool(b)) => match k.as_str() {
                        "ignoreConstructors" => r.ignore_constructors = *b,
                        "avoidQuotes" => r.avoid_quotes = *b,
                        _ => r.avoid_explicit_return_arrows = *b,
                    },
                    _ => return bad(),
                }
            }
        }
        _ => return bad(),
    }
    Ok(Box::new(r))
}

fn is_string_literal(cx: &Cx, n: NodeId) -> bool {
    let ast = &cx.sc.ast;
    ast.is(n, "Literal")
        && ast.prop(n, "regex").is_none()
        && matches!(ast.prop(n, "value"), Some(Prop::Str(_)))
}

fn is_proto_key(cx: &Cx, p: NodeId) -> bool {
    !cx.sc.ast.bool(p, "computed")
        && get_static_property_name(&cx.sc.ast, p).as_deref() == Some("__proto__")
}

fn can_have_shorthand(cx: &Cx, p: NodeId) -> bool {
    let ast = &cx.sc.ast;
    !matches!(ast.str(p, "kind"), Some("set" | "get")) && !ast.is(p, "SpreadElement")
}

fn is_shorthand(cx: &Cx, p: NodeId) -> bool {
    cx.sc.ast.bool(p, "shorthand") || cx.sc.ast.bool(p, "method")
}

fn is_redundant(cx: &Cx, p: NodeId) -> bool {
    let ast = &cx.sc.ast;
    let Some(value) = ast.child(p, "value") else {
        return false;
    };
    if is_proto_key(cx, p) {
        return false;
    }
    if ast.is(value, "FunctionExpression") {
        return ast.child(value, "id").is_none();
    }
    if ast.is(value, "Identifier") {
        return get_static_property_name(ast, p).as_deref() == ast.name(value);
    }
    false
}

fn make_function_shorthand(cx: &Cx, node: NodeId) -> Option<Fix> {
    let sc = cx.sc;
    let ast = &sc.ast;
    let key = ast.child(node, "key")?;
    let value = ast.child(node, "value")?;
    let (first_key, last_key) = if ast.bool(node, "computed") {
        (
            sc.first_token_f(node, is_opening_bracket)?,
            sc.first_token_between(key, value, is_closing_bracket)?,
        )
    } else {
        (sc.first_token(key, N)?, sc.last_token(key, N)?)
    };
    let key_text = sc.slice16(first_key.start, last_key.end);
    if sc.comments_exist_between(last_key.end, ast.range(value).0) {
        return None;
    }
    let mut prefix = String::new();
    if ast.bool(value, "async") {
        prefix.push_str("async ");
    }
    if ast.bool(value, "generator") {
        prefix.push('*');
    }
    let range = (first_key.start, ast.range(node).1);
    let method_prefix = format!("{prefix}{key_text}");
    let vend = ast.range(value).1;
    if ast.is(value, "FunctionExpression") {
        let func = sc
            .tokens_of(value, N)
            .iter()
            .find(|t| t.kind == TokKind::Keyword && t.value == "function")?;
        let before_params = if ast.bool(value, "generator") {
            sc.token_after_tok(func, N)?
        } else {
            func
        };
        return Some(fixer::replace_range(
            range,
            format!("{method_prefix}{}", sc.slice16(before_params.end, vend)),
        ));
    }
    let body = ast.child(value, "body")?;
    let arrow = sc.token_before_f(body, |t| t.kind == TokKind::Punctuator && t.value == "=>")?;
    let fn_body = sc.slice16(arrow.end, vend);
    let first_value = sc.first_token(
        value,
        Opts {
            include_comments: false,
            skip: usize::from(ast.bool(value, "async")),
        },
    )?;
    let slice_start = first_value.start;
    let slice_end = sc.token_before_tok(arrow, N)?.end;
    let params: Vec<NodeId> = ast.nodes_of(value, "params").collect();
    let add_parens = params.len() == 1 && ast.range(params[0]).0 == slice_start;
    let old = sc.slice16(slice_start, slice_end);
    let new = if add_parens {
        format!("({old})")
    } else {
        old.to_string()
    };
    Some(fixer::replace_range(
        range,
        format!("{method_prefix}{new}{fn_body}"),
    ))
}

fn make_function_longform(cx: &Cx, node: NodeId) -> Option<Fix> {
    let sc = cx.sc;
    let ast = &sc.ast;
    let key = ast.child(node, "key")?;
    let value = ast.child(node, "value")?;
    let (first_key, last_key) = if ast.bool(node, "computed") {
        (
            sc.tokens_of(node, N).iter().find(|t| t.value == "[")?,
            sc.tokens_between(ast.range(key).1, ast.range(value).0, N)
                .iter()
                .find(|t| t.value == "]")?,
        )
    } else {
        (sc.first_token(key, N)?, sc.last_token(key, N)?)
    };
    let key_text = sc.slice16(first_key.start, last_key.end);
    let mut header = "function".to_string();
    if ast.bool(value, "async") {
        header = format!("async {header}");
    }
    if ast.bool(value, "generator") {
        header.push('*');
    }
    Some(fixer::replace_range(
        (ast.range(node).0, last_key.end),
        format!("{key_text}: {header}"),
    ))
}

impl R {
    fn is_constructor(&mut self, name: &str) -> bool {
        let Some(c) = name.chars().find(|c| !matches!(c, '_' | '$' | '0'..='9')) else {
            return false;
        };
        if c.is_ascii() {
            return c.is_ascii_uppercase();
        }
        if !c.is_alphabetic()
            || c.is_numeric()
            || c.is_lowercase()
            || !c.to_uppercase().eq(std::iter::once(c))
        {
            return false;
        }
        if c.is_uppercase() && !matches!(c, '\u{24B6}'..='\u{24CF}' | '\u{1F130}'..='\u{1F189}') {
            return true;
        }
        self.unsupported = true;
        false
    }

    fn check_consistency(&self, cx: &mut Cx, node: NodeId, check_redundancy: bool) {
        let ast = &cx.sc.ast;
        let props: Vec<NodeId> = ast
            .nodes_of(node, "properties")
            .filter(|p| can_have_shorthand(cx, *p))
            .collect();
        if props.is_empty() {
            return;
        }
        let shorthand = props.iter().filter(|p| is_shorthand(cx, **p)).count();
        if shorthand != props.len() {
            if shorthand > 0 {
                cx.report(Report::node(node, "unexpectedMix"));
            } else if check_redundancy && props.iter().all(|p| is_redundant(cx, *p)) {
                cx.report(Report::node(node, "expectedAllPropertiesShorthanded"));
            }
        }
    }

    fn enter_function(&mut self, cx: &Cx, node: NodeId) {
        self.lexical.push(Vec::new());
        let sm = cx.sm();
        let scope = cx.get_scope(node);
        for &v in &sm.scope(scope).variables {
            if sm.var(v).name == "arguments" {
                for &r in &sm.var(v).references {
                    self.arguments_ids.insert(sm.reference(r).identifier);
                }
            }
        }
    }

    fn report_lexical(&mut self) {
        if let Some(top) = self.lexical.last() {
            for a in top {
                if !self.arrows_with_lexical.contains(a) {
                    self.arrows_with_lexical.push(*a);
                }
            }
        }
    }

    fn containing_object_method(&self, cx: &Cx, node: NodeId) -> Option<NodeId> {
        let sm = cx.sm();
        let ast = &cx.sc.ast;
        let mut scope = Some(sm.scope(cx.get_scope(node)).variable_scope);
        while let Some(s) = scope {
            let sd = sm.scope(s);
            if matches!(
                sd.kind,
                ScopeType::ClassFieldInitializer | ScopeType::ClassStaticBlock
            ) {
                return None;
            }
            let f = sd.block;
            if ast.is(f, "FunctionExpression") || ast.is(f, "FunctionDeclaration") {
                let p = ast.parent(f)?;
                return (ast.is(p, "Property") && ast.bool(p, "method")).then_some(p);
            }
            scope = sd.upper.map(|u| sm.scope(u).variable_scope);
        }
        None
    }

    fn property_exit(&mut self, cx: &mut Cx, node: NodeId) {
        let ast = &cx.sc.ast;
        let concise = ast.bool(node, "method") || ast.bool(node, "shorthand");
        if ast.parent(node).is_some_and(|p| ast.is(p, "ObjectPattern")) {
            return;
        }
        if matches!(ast.str(node, "kind"), Some("get" | "set")) || is_proto_key(cx, node) {
            return;
        }
        let (Some(key), Some(value)) = (ast.child(node, "key"), ast.child(node, "value")) else {
            return;
        };
        let is_fn = matches!(
            ast.kind(value),
            "FunctionExpression" | "ArrowFunctionExpression"
        );
        if ast.bool(node, "computed") && !is_fn {
            return;
        }
        if concise {
            if ast.bool(node, "method")
                && !self.methods_with_super.contains(&node)
                && (self.never || (self.avoid_quotes && is_string_literal(cx, key)))
            {
                let id = if self.never {
                    "expectedMethodLongform"
                } else {
                    "expectedLiteralMethodLongform"
                };
                let mut r = Report::node(node, id);
                if let Some(f) = make_function_longform(cx, node) {
                    r = r.fix(f);
                }
                cx.report(r);
            } else if self.never && !ast.bool(node, "method") {
                let name = ast.name(key).unwrap_or("undefined");
                let f = fixer::insert_after_range(ast.range(key), format!(": {name}"));
                cx.report(Report::node(node, "expectedPropertyLongform").fix(f));
            }
        } else if self.to_methods && ast.child(value, "id").is_none() && is_fn {
            let name = get_static_property_name(ast, node);
            if self.ignore_constructors && name.as_deref().is_some_and(|n| self.is_constructor(n)) {
                return;
            }
            if let (Some(re), Some(n)) = (&self.methods_ignore, &name)
                && re.test(n)
            {
                return;
            }
            if self.avoid_quotes && is_string_literal(cx, key) {
                return;
            }
            let arrow_ok = ast.is(value, "ArrowFunctionExpression")
                && ast
                    .child(value, "body")
                    .is_some_and(|b| ast.is(b, "BlockStatement"))
                && self.avoid_explicit_return_arrows
                && !self.arrows_with_lexical.contains(&value);
            if ast.is(value, "FunctionExpression") || arrow_ok {
                let mut r = Report::node(node, "expectedMethodShorthand");
                if let Some(f) = make_function_shorthand(cx, node) {
                    r = r.fix(f);
                }
                cx.report(r);
            }
        } else if ast.is(value, "Identifier") && self.to_props {
            let vname = ast.name(value);
            let key_matches = ast.is(key, "Identifier") && ast.name(key) == vname;
            let literal_matches = ast.is(key, "Literal")
                && matches!((ast.prop(key, "value"), vname), (Some(Prop::Str(s)), Some(v)) if s == v);
            let jsdoc_type = |strict_ws: bool| {
                cx.sc.comments_inside(node).iter().any(|c| {
                    c.kind == TokKind::Block
                        && (if strict_ws {
                            c.value
                                .trim_start_matches(crate::utils::is_js_whitespace)
                                .starts_with('*')
                        } else {
                            c.value.starts_with('*')
                        })
                        && c.value.contains("@type")
                })
            };
            let report = if key_matches {
                !jsdoc_type(true)
            } else if literal_matches {
                !self.avoid_quotes && !jsdoc_type(false)
            } else {
                false
            };
            if report {
                let mut r = Report::node(node, "expectedPropertyShorthand");
                if cx.sc.comments_inside(node).is_empty() {
                    r = r.fix(fixer::replace(cx.sc, node, vname.unwrap_or("")));
                }
                cx.report(r);
            }
        }
    }
}

impl Rule for R {
    fn listeners(&self) -> &'static [&'static str] {
        &[
            "Program",
            "FunctionDeclaration",
            "FunctionExpression",
            "Program:exit",
            "FunctionDeclaration:exit",
            "FunctionExpression:exit",
            "ArrowFunctionExpression",
            "ArrowFunctionExpression:exit",
            "ThisExpression",
            "Super",
            "MetaProperty",
            "Identifier",
            "ObjectExpression",
            "Property:exit",
        ]
    }

    fn on(&mut self, li: usize, node: NodeId, cx: &mut Cx) {
        let ast = &cx.sc.ast;
        match li {
            0..=2 => self.enter_function(cx, node),
            3..=5 => {
                self.lexical.pop();
            }
            6 => {
                if let Some(top) = self.lexical.last_mut()
                    && !top.contains(&node)
                {
                    top.push(node);
                }
            }
            7 => {
                if let Some(top) = self.lexical.last_mut() {
                    top.retain(|n| *n != node);
                }
            }
            8 => self.report_lexical(),
            9 => {
                self.report_lexical();
                if let Some(m) = self.containing_object_method(cx, node)
                    && !self.methods_with_super.contains(&m)
                {
                    self.methods_with_super.push(m);
                }
            }
            10 => {
                let meta = ast.child(node, "meta").and_then(|m| ast.name(m));
                let prop = ast.child(node, "property").and_then(|m| ast.name(m));
                if meta == Some("new") && prop == Some("target") {
                    self.report_lexical();
                }
            }
            11 => {
                if self.arguments_ids.contains(&node) {
                    self.report_lexical();
                }
            }
            12 => {
                if self.consistent {
                    self.check_consistency(cx, node, false);
                } else if self.consistent_as_needed {
                    self.check_consistency(cx, node, true);
                }
            }
            _ => self.property_exit(cx, node),
        }
    }

    fn unsupported(&self) -> Option<&str> {
        self.unsupported.then_some("non-ASCII constructor name")
    }
}
