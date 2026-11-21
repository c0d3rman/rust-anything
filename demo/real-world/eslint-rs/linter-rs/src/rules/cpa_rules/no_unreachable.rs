//! lib/rules/no-unreachable.js

use super::segs;
use crate::ast::NodeId;
use crate::code_path::{Event, Graph, SegId};
use crate::linter::Cx;
use crate::rule::Report;
use crate::rules::{Rule, RuleDef};
use crate::source::Loc;
use crate::tokens::N;
use serde_json::Value;
use std::rc::Rc;

pub static DEF: RuleDef = RuleDef {
    name: "no-unreachable",
    messages: &[("unreachableCode", "Unreachable code.")],
    has_default_options: false,
    create,
};

#[derive(Default)]
struct R {
    graph: Option<Rc<Graph>>,
    constructor_info: Vec<bool>,
    range: Option<(NodeId, NodeId)>,
    code_path_segments: Vec<Vec<SegId>>,
    current: Vec<SegId>,
}

fn create(options: &[Value]) -> Result<Box<dyn Rule>, String> {
    if !options.is_empty() {
        return Err("no options allowed".into());
    }
    Ok(Box::new(R::default()))
}

const LISTENERS: &[&str] = &[
    "onCodePathStart",
    "onCodePathEnd",
    "onUnreachableCodePathSegmentStart",
    "onUnreachableCodePathSegmentEnd",
    "onCodePathSegmentEnd",
    "onCodePathSegmentStart",
    "BlockStatement",
    "BreakStatement",
    "ClassDeclaration",
    "ContinueStatement",
    "DebuggerStatement",
    "DoWhileStatement",
    "ExpressionStatement",
    "ForInStatement",
    "ForOfStatement",
    "ForStatement",
    "IfStatement",
    "LabeledStatement",
    "ReturnStatement",
    "SwitchStatement",
    "ThrowStatement",
    "TryStatement",
    "VariableDeclaration",
    "WhileStatement",
    "WithStatement",
    "ExportNamedDeclaration",
    "ExportDefaultDeclaration",
    "ExportAllDeclaration",
    "Program:exit",
    "MethodDefinition[kind='constructor']",
    "MethodDefinition[kind='constructor']:exit",
    "CallExpression > Super.callee",
];

impl R {
    fn contains(&self, cx: &Cx, r: (u32, u32)) -> bool {
        let Some((s, e)) = self.range else {
            return false;
        };
        r.0 >= cx.sc.ast.range(s).0 && r.1 <= cx.sc.ast.range(e).1
    }

    fn report_if_unreachable(&mut self, node: Option<NodeId>, cx: &mut Cx) {
        let sc = cx.sc;
        let ast = &sc.ast;
        let mut next = None;
        if let Some(n) = node
            && (ast.is(n, "PropertyDefinition")
                || !segs::any_reachable(self.graph.as_ref(), &self.current))
        {
            let Some((start, _)) = self.range else {
                self.range = Some((n, n));
                return;
            };
            if self.contains(cx, ast.range(n)) {
                return;
            }
            let before = cx.sc.token_before(n, N).map(|t| (t.start, t.end));
            if before.is_some_and(|b| self.contains(cx, b)) {
                self.range = Some((start, n));
                return;
            }
            next = Some(n);
        }
        if let Some((s, e)) = self.range {
            let loc = Loc {
                start: cx.sc.loc_of(ast.range(s).0),
                end: cx.sc.loc_of(ast.range(e).1),
            };
            let mut r = Report::loc(loc, "unreachableCode");
            r.node = Some(s);
            cx.report(r);
        }
        self.range = next.map(|n| (n, n));
    }
}

impl Rule for R {
    fn listeners(&self) -> &'static [&'static str] {
        LISTENERS
    }

    fn on_code_path(&mut self, l: usize, ev: &Event, _cx: &mut Cx) {
        segs::keep(&mut self.graph, ev);
        match l {
            0 => self
                .code_path_segments
                .push(std::mem::take(&mut self.current)),
            1 => self.current = self.code_path_segments.pop().unwrap_or_default(),
            2 | 5 => {
                if let Some(s) = ev.segment() {
                    segs::add(&mut self.current, s);
                }
            }
            3 | 4 => {
                if let Some(s) = ev.segment() {
                    segs::delete(&mut self.current, s);
                }
            }
            _ => {}
        }
    }

    fn on(&mut self, l: usize, node: NodeId, cx: &mut Cx) {
        let sc = cx.sc;
        let ast = &sc.ast;
        match LISTENERS[l] {
            "VariableDeclaration" => {
                if ast.str(node, "kind") != Some("var")
                    || ast
                        .nodes_of(node, "declarations")
                        .any(|d| ast.child(d, "init").is_some())
                {
                    self.report_if_unreachable(Some(node), cx);
                }
            }
            "Program:exit" => self.report_if_unreachable(None, cx),
            "MethodDefinition[kind='constructor']" => {
                if ast.str(node, "kind") == Some("constructor") {
                    self.constructor_info.push(false);
                }
            }
            "MethodDefinition[kind='constructor']:exit" => {
                if ast.str(node, "kind") != Some("constructor") {
                    return;
                }
                let has_super_call = self.constructor_info.pop().unwrap_or(false);
                if ast
                    .child(node, "value")
                    .and_then(|v| ast.child(v, "body"))
                    .is_none()
                {
                    return;
                }
                let Some(class) = ast.parent(node).and_then(|b| ast.parent(b)) else {
                    return;
                };
                if ast.child(class, "superClass").is_some() && !has_super_call {
                    let body = ast.child(class, "body");
                    let elements: Vec<NodeId> = body
                        .map(|b| ast.nodes_of(b, "body").collect())
                        .unwrap_or_default();
                    for el in elements {
                        if ast.is(el, "PropertyDefinition") && !ast.bool(el, "static") {
                            self.report_if_unreachable(Some(el), cx);
                        }
                    }
                }
            }
            "CallExpression > Super.callee" => {
                let is_callee = ast.parent(node).is_some_and(|p| {
                    ast.is(p, "CallExpression") && ast.child(p, "callee") == Some(node)
                });
                if is_callee && let Some(last) = self.constructor_info.last_mut() {
                    *last = true;
                }
            }
            _ => self.report_if_unreachable(Some(node), cx),
        }
    }
}
