//! lib/rules/getter-return.js

use super::segs;
use crate::ast::NodeId;
use crate::code_path::{Event, Graph, SegId};
use crate::linter::Cx;
use crate::rule::Report;
use crate::rules::{Rule, RuleDef};
use crate::utils::{
    get_function_head_loc, get_function_name_with_kind, get_static_property_name,
    is_specific_member_access, skip_chain_expression,
};
use serde_json::Value;
use std::rc::Rc;

pub static DEF: RuleDef = RuleDef {
    name: "getter-return",
    messages: &[
        ("expected", "Expected to return a value in {{name}}."),
        (
            "expectedAlways",
            "Expected {{name}} to always return a value.",
        ),
    ],
    has_default_options: true,
    create,
};

struct FuncInfo {
    has_return: bool,
    should_check: bool,
    node: Option<NodeId>,
    current: Vec<SegId>,
}

struct R {
    allow_implicit: bool,
    graph: Option<Rc<Graph>>,
    stack: Vec<FuncInfo>,
}

fn create(options: &[Value]) -> Result<Box<dyn Rule>, String> {
    let mut allow_implicit = false;
    match options {
        [] => {}
        [Value::Object(o)] => {
            for (k, v) in o {
                match (k.as_str(), v) {
                    ("allowImplicit", Value::Bool(b)) => allow_implicit = *b,
                    _ => return Err("invalid option".into()),
                }
            }
        }
        _ => return Err("invalid option".into()),
    }
    let root = FuncInfo {
        has_return: false,
        should_check: false,
        node: None,
        current: Vec::new(),
    };
    Ok(Box::new(R {
        allow_implicit,
        graph: None,
        stack: vec![root],
    }))
}

fn is_argument_of_global_method_call(
    cx: &Cx,
    node: NodeId,
    object: &str,
    method: &str,
    index: usize,
) -> bool {
    let ast = &cx.sc.ast;
    let Some(call) = ast.parent(node) else {
        return false;
    };
    ast.is(call, "CallExpression")
        && ast.list(call, "arguments").get(index) == Some(&Some(node))
        && ast.child(call, "callee").is_some_and(|callee| {
            is_specific_member_access(ast, callee, Some(object), Some(method))
                && ast
                    .child(skip_chain_expression(ast, callee), "object")
                    .is_some_and(|o| cx.sm().is_global_reference(ast, o))
        })
}

fn is_property_descriptor(cx: &Cx, node: NodeId) -> bool {
    let ast = &cx.sc.ast;
    if is_argument_of_global_method_call(cx, node, "Object", "defineProperty", 2)
        || is_argument_of_global_method_call(cx, node, "Reflect", "defineProperty", 2)
    {
        return true;
    }
    let Some(parent) = ast.parent(node) else {
        return false;
    };
    if ast.is(parent, "Property")
        && ast.child(parent, "value") == Some(node)
        && let Some(gp) = ast.parent(parent)
    {
        return ast.is(gp, "ObjectExpression")
            && (is_argument_of_global_method_call(cx, gp, "Object", "create", 1)
                || is_argument_of_global_method_call(cx, gp, "Object", "defineProperties", 1));
    }
    false
}

fn is_getter(cx: &Cx, node: NodeId) -> bool {
    let ast = &cx.sc.ast;
    if !matches!(
        ast.kind(node),
        "FunctionExpression" | "ArrowFunctionExpression"
    ) {
        return false;
    }
    if !ast
        .child(node, "body")
        .is_some_and(|b| ast.is(b, "BlockStatement"))
    {
        return false;
    }
    let Some(parent) = ast.parent(node) else {
        return false;
    };
    if ast.str(parent, "kind") == Some("get") {
        return true;
    }
    ast.is(parent, "Property")
        && get_static_property_name(ast, parent).as_deref() == Some("get")
        && ast
            .parent(parent)
            .is_some_and(|gp| ast.is(gp, "ObjectExpression") && is_property_descriptor(cx, gp))
}

impl R {
    fn info(&mut self) -> Option<&mut FuncInfo> {
        self.stack.last_mut()
    }

    fn check_last_segment(&mut self, node: NodeId, cx: &mut Cx) {
        let Some(fi) = self.stack.last() else { return };
        if fi.should_check && segs::any_reachable(self.graph.as_ref(), &fi.current) {
            let id = if fi.has_return {
                "expectedAlways"
            } else {
                "expected"
            };
            let name = fi
                .node
                .map(|n| get_function_name_with_kind(&cx.sc.ast, n))
                .unwrap_or_default();
            let Some(loc) = get_function_head_loc(cx.sc, node) else {
                return;
            };
            let mut r = Report::loc(loc, id).data("name", name);
            r.node = Some(node);
            cx.report(r);
        }
    }
}

impl Rule for R {
    fn listeners(&self) -> &'static [&'static str] {
        &[
            "onCodePathStart",
            "onCodePathEnd",
            "onUnreachableCodePathSegmentStart",
            "onUnreachableCodePathSegmentEnd",
            "onCodePathSegmentStart",
            "onCodePathSegmentEnd",
            "ReturnStatement",
            "FunctionExpression:exit",
            "ArrowFunctionExpression:exit",
        ]
    }

    fn on_code_path(&mut self, l: usize, ev: &Event, cx: &mut Cx) {
        segs::keep(&mut self.graph, ev);
        match l {
            0 => {
                let node = ev.node();
                let should_check = is_getter(cx, node);
                self.stack.push(FuncInfo {
                    has_return: false,
                    should_check,
                    node: Some(node),
                    current: Vec::new(),
                });
            }
            1 => {
                self.stack.pop();
            }
            2 | 4 => {
                if let (Some(s), Some(fi)) = (ev.segment(), self.info()) {
                    segs::add(&mut fi.current, s);
                }
            }
            3 | 5 => {
                if let (Some(s), Some(fi)) = (ev.segment(), self.info()) {
                    segs::delete(&mut fi.current, s);
                }
            }
            _ => {}
        }
    }

    fn on(&mut self, l: usize, node: NodeId, cx: &mut Cx) {
        if l == 6 {
            let allow_implicit = self.allow_implicit;
            let Some(fi) = self.stack.last_mut() else {
                return;
            };
            if fi.should_check {
                fi.has_return = true;
                if !allow_implicit && cx.sc.ast.child(node, "argument").is_none() {
                    let name = fi
                        .node
                        .map(|n| get_function_name_with_kind(&cx.sc.ast, n))
                        .unwrap_or_default();
                    cx.report(Report::node(node, "expected").data("name", name));
                }
            }
        } else {
            self.check_last_segment(node, cx);
        }
    }
}
