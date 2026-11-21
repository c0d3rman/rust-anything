//! lib/rules/no-param-reassign.js

use super::common::{simple_pattern, Pat};
use crate::ast::{Ast, NodeId};
use crate::linter::Cx;
use crate::rule::Report;
use crate::rules::{Rule, RuleDef};
use crate::scope::DefType;
use serde_json::{Map, Value};

pub static DEF: RuleDef = RuleDef {
    name: "no-param-reassign",
    messages: &[
        ("assignmentToFunctionParam", "Assignment to function parameter '{{name}}'."),
        ("assignmentToFunctionParamProp", "Assignment to property of function parameter '{{name}}'."),
    ],
    has_default_options: false,
    create,
};

struct R {
    props: bool,
    ignored: Vec<String>,
    ignored_regex: Vec<Pat>,
}

fn string_set(v: &Value) -> Option<Vec<String>> {
    let Value::Array(a) = v else { return None };
    let mut out: Vec<String> = Vec::new();
    for x in a {
        let Value::String(s) = x else { return None };
        if out.contains(s) {
            return None;
        }
        out.push(s.clone());
    }
    Some(out)
}

fn matches_first(o: &Map<String, Value>) -> bool {
    o.iter().all(|(k, v)| k == "props" && *v == Value::Bool(false))
}

fn matches_second(o: &Map<String, Value>) -> bool {
    o.iter().all(|(k, v)| match k.as_str() {
        "props" => *v == Value::Bool(true),
        "ignorePropertyModificationsFor" | "ignorePropertyModificationsForRegex" => string_set(v).is_some(),
        _ => false,
    })
}

fn create(options: &[Value]) -> Result<Box<dyn Rule>, String> {
    let mut r = R { props: false, ignored: Vec::new(), ignored_regex: Vec::new() };
    match options {
        [] => {}
        [Value::Object(o)] => {
            if matches_first(o) == matches_second(o) {
                return Err("invalid option".into());
            }
            r.props = o.get("props") == Some(&Value::Bool(true));
            if let Some(v) = o.get("ignorePropertyModificationsFor") {
                r.ignored = string_set(v).unwrap_or_default();
            }
            if let Some(v) = o.get("ignorePropertyModificationsForRegex") {
                for src in string_set(v).unwrap_or_default() {
                    r.ignored_regex.push(simple_pattern(&src).ok_or("ignorePropertyModificationsForRegex pattern not ported")?);
                }
            }
        }
        _ => return Err("invalid option".into()),
    }
    Ok(Box::new(r))
}

fn is_stop_node(kind: &str) -> bool {
    kind.ends_with("Statement") || kind.ends_with("Declaration") || kind.ends_with("Function") || kind.ends_with("FunctionExpression") || kind.ends_with("Program")
}

fn is_modifying_prop(ast: &Ast, id: NodeId) -> bool {
    let mut node = id;
    let mut parent = ast.parent(node);
    while let Some(p) = parent {
        let k = ast.kind(p);
        if is_stop_node(k) && k != "ForInStatement" && k != "ForOfStatement" {
            break;
        }
        match k {
            "AssignmentExpression" => return ast.child(p, "left") == Some(node),
            "UpdateExpression" => return true,
            "UnaryExpression" => {
                if ast.str(p, "operator") == Some("delete") {
                    return true;
                }
            }
            "ForInStatement" | "ForOfStatement" => return ast.child(p, "left") == Some(node),
            "CallExpression" => {
                if ast.child(p, "callee") != Some(node) {
                    return false;
                }
            }
            "MemberExpression" => {
                if ast.child(p, "property") == Some(node) {
                    return false;
                }
            }
            "Property" => {
                if ast.child(p, "key") == Some(node) {
                    return false;
                }
            }
            "ConditionalExpression"
                if ast.child(p, "test") == Some(node) => {
                    return false;
                }
            _ => {}
        }
        node = p;
        parent = ast.parent(node);
    }
    false
}

impl Rule for R {
    fn listeners(&self) -> &'static [&'static str] {
        &["FunctionDeclaration:exit", "FunctionExpression:exit", "ArrowFunctionExpression:exit"]
    }

    fn on(&mut self, _: usize, node: NodeId, cx: &mut Cx) {
        let sm = cx.sm();
        let ast = &cx.sc.ast;
        for &v in cx.declared_variables(node) {
            let var = sm.var(v);
            if var.defs.first().map(|d| d.kind) != Some(DefType::Parameter) {
                continue;
            }
            for (i, &r) in var.references.iter().enumerate() {
                let rf = sm.reference(r);
                let id = rf.identifier;
                if rf.init == Some(true) || (i > 0 && sm.reference(var.references[i - 1]).identifier == id) {
                    continue;
                }
                let name = ast.name(id).unwrap_or("").to_string();
                if rf.is_write() {
                    cx.report(Report::node(id, "assignmentToFunctionParam").data("name", name));
                } else if self.props && is_modifying_prop(ast, id) && !self.ignored.contains(&name)
                    && !self.ignored_regex.iter().any(|p| p.test(&name))
                {
                    cx.report(Report::node(id, "assignmentToFunctionParamProp").data("name", name));
                }
            }
        }
    }
}
