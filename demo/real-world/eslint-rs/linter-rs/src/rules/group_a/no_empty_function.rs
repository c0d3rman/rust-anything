//! lib/rules/no-empty-function.js

use crate::ast::NodeId;
use crate::linter::Cx;
use crate::report::SugVal;
use crate::rule::{Report, SuggestSpec, fixer};
use crate::rules::{Rule, RuleDef};
use crate::utils::get_function_name_with_kind;
use serde_json::Value;

pub static DEF: RuleDef = RuleDef {
    name: "no-empty-function",
    messages: &[
        ("unexpected", "Unexpected empty {{name}}."),
        ("suggestComment", "Add comment inside empty {{name}}."),
    ],
    has_default_options: true,
    create,
};

const ALLOW_OPTIONS: &[&str] = &[
    "functions",
    "arrowFunctions",
    "generatorFunctions",
    "methods",
    "generatorMethods",
    "getters",
    "setters",
    "constructors",
    "asyncFunctions",
    "asyncMethods",
    "privateConstructors",
    "protectedConstructors",
    "decoratedFunctions",
    "overrideMethods",
];

struct R {
    allow: Vec<String>,
}

fn create(options: &[Value]) -> Result<Box<dyn Rule>, String> {
    let mut allow: Vec<String> = Vec::new();
    match options {
        [] => {}
        [Value::Object(o)] => {
            for (k, v) in o {
                match (k.as_str(), v) {
                    ("allow", Value::Array(items)) => {
                        allow.clear();
                        for i in items {
                            let Some(s) = i.as_str().filter(|s| ALLOW_OPTIONS.contains(s)) else {
                                return Err("invalid option".into());
                            };
                            if allow.iter().any(|a| a == s) {
                                return Err("invalid option".into());
                            }
                            allow.push(s.to_string());
                        }
                    }
                    _ => return Err("invalid option".into()),
                }
            }
        }
        _ => return Err("invalid option".into()),
    }
    Ok(Box::new(R { allow }))
}

fn get_kind(cx: &Cx, node: NodeId) -> String {
    let ast = &cx.sc.ast;
    if ast.is(node, "ArrowFunctionExpression") {
        return "arrowFunctions".into();
    }
    let parent = ast.parent(node);
    let pk = parent.map(|p| ast.kind(p)).unwrap_or("");
    let pkind = parent.and_then(|p| ast.str(p, "kind"));
    let kind = if pk == "Property" {
        match pkind {
            Some("get") => return "getters".into(),
            Some("set") => return "setters".into(),
            _ => {
                if parent.is_some_and(|p| ast.bool(p, "method")) {
                    "methods"
                } else {
                    "functions"
                }
            }
        }
    } else if pk == "MethodDefinition" {
        match pkind {
            Some("get") => return "getters".into(),
            Some("set") => return "setters".into(),
            Some("constructor") => return "constructors".into(),
            _ => "methods",
        }
    } else {
        "functions"
    };
    let prefix = if ast.bool(node, "generator") {
        "generator"
    } else if ast.bool(node, "async") {
        "async"
    } else {
        return kind.into();
    };
    format!("{prefix}{}{}", kind[..1].to_ascii_uppercase(), &kind[1..])
}

impl R {
    fn allowed(&self, kind: &str) -> bool {
        self.allow.iter().any(|a| a == kind)
    }

    fn is_allowed_empty_function(&self, cx: &Cx, node: NodeId) -> bool {
        let ast = &cx.sc.ast;
        let kind = get_kind(cx, node);
        if self.allowed(&kind) {
            return true;
        }
        let parent = ast.parent(node);
        if kind == "constructors" {
            let access = parent.and_then(|p| ast.str(p, "accessibility"));
            if (access == Some("private") && self.allowed("privateConstructors"))
                || (access == Some("protected") && self.allowed("protectedConstructors"))
                || ast
                    .nodes_of(node, "params")
                    .any(|p| ast.is(p, "TSParameterProperty"))
            {
                return true;
            }
        }
        let lower = kind.to_ascii_lowercase();
        if lower.contains("getters") || lower.contains("setters") || lower.ends_with("methods") {
            let decorated = parent.is_some_and(|p| !ast.list(p, "decorators").is_empty());
            let overridden = parent.is_some_and(|p| ast.bool(p, "override"));
            if (decorated && self.allowed("decoratedFunctions"))
                || (overridden && self.allowed("overrideMethods"))
            {
                return true;
            }
        }
        false
    }
}

impl Rule for R {
    fn listeners(&self) -> &'static [&'static str] {
        &[
            "ArrowFunctionExpression",
            "FunctionDeclaration",
            "FunctionExpression",
        ]
    }

    fn on(&mut self, _: usize, node: NodeId, cx: &mut Cx) {
        let sc = cx.sc;
        let ast = &sc.ast;
        let Some(body) = ast.child(node, "body") else {
            return;
        };
        let name = get_function_name_with_kind(ast, node);
        let inner_comments = !sc.comments_inside(body).is_empty();
        if self.is_allowed_empty_function(cx, node)
            || !ast.is(body, "BlockStatement")
            || !ast.list(body, "body").is_empty()
            || inner_comments
        {
            return;
        }
        let (s, e) = ast.range(body);
        let mut r = Report::loc(sc.node_loc(body), "unexpected").data("name", name.as_str());
        r.node = Some(node);
        r.suggest.push(SuggestSpec {
            keys: vec![
                ("messageId", SugVal::Str("suggestComment".into())),
                (
                    "data",
                    SugVal::Data(vec![("name".into(), name.as_str().into())]),
                ),
                ("fix", SugVal::Fix),
            ],
            fix: Some(fixer::replace_range((s + 1, e - 1), " /* empty */ ")),
        });
        cx.report(r);
    }
}
