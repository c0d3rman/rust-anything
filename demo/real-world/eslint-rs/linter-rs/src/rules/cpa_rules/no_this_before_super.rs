//! lib/rules/no-this-before-super.js

use super::segs;
use crate::ast::{Ast, NodeId};
use crate::code_path::{CpId, Event, EventKind, Graph, SegId};
use crate::linter::Cx;
use crate::rule::Report;
use crate::rules::{Rule, RuleDef};
use crate::utils::is_null_literal;
use serde_json::Value;
use std::collections::HashMap;
use std::rc::Rc;

pub static DEF: RuleDef = RuleDef {
    name: "no-this-before-super",
    messages: &[(
        "noBeforeSuper",
        "'{{kind}}' is not allowed before 'super()'.",
    )],
    has_default_options: false,
    create,
};

#[derive(Default)]
struct SegInfo {
    super_called: bool,
    invalid_nodes: Vec<NodeId>,
}

struct FuncInfo {
    is_constructor: bool,
    has_extends: bool,
    code_path: CpId,
    current: Vec<SegId>,
}

#[derive(Default)]
struct R {
    graph: Option<Rc<Graph>>,
    stack: Vec<FuncInfo>,
    map: HashMap<SegId, SegInfo>,
    unsupported: Option<&'static str>,
}

fn create(options: &[Value]) -> Result<Box<dyn Rule>, String> {
    if !options.is_empty() {
        return Err("no options allowed".into());
    }
    Ok(Box::new(R::default()))
}

fn is_null_or_undefined(ast: &Ast, n: NodeId) -> bool {
    is_null_literal(ast, n)
        || (ast.is(n, "Identifier") && ast.name(n) == Some("undefined"))
        || (ast.is(n, "UnaryExpression") && ast.str(n, "operator") == Some("void"))
}

fn is_called(map: &HashMap<SegId, SegInfo>, g: &Graph, s: SegId) -> bool {
    !g.reachable(s) || map.get(&s).is_some_and(|i| i.super_called)
}

impl R {
    fn derived(&self) -> bool {
        self.stack
            .last()
            .is_some_and(|f| f.is_constructor && f.has_extends)
    }

    fn before_call_of_super(&self) -> bool {
        let (Some(f), Some(g)) = (self.stack.last(), self.graph.as_ref()) else {
            return false;
        };
        self.derived() && !f.current.iter().all(|s| is_called(&self.map, g, *s))
    }

    fn for_each_reachable_current(&mut self, mut f: impl FnMut(&mut SegInfo)) {
        let (Some(fi), Some(g)) = (self.stack.last(), self.graph.as_ref()) else {
            return;
        };
        for s in &fi.current {
            if g.reachable(*s) {
                match self.map.get_mut(s) {
                    Some(info) => f(info),
                    None => {
                        self.unsupported = Some("segment without info");
                        return;
                    }
                }
            }
        }
    }
}

impl Rule for R {
    fn listeners(&self) -> &'static [&'static str] {
        &[
            "onCodePathStart",
            "onCodePathEnd",
            "onCodePathSegmentStart",
            "onUnreachableCodePathSegmentStart",
            "onUnreachableCodePathSegmentEnd",
            "onCodePathSegmentEnd",
            "onCodePathSegmentLoop",
            "ThisExpression",
            "Super",
            "CallExpression:exit",
            "Program:exit",
        ]
    }

    fn unsupported(&self) -> Option<&str> {
        self.unsupported
    }

    fn on_code_path(&mut self, l: usize, ev: &Event, cx: &mut Cx) {
        segs::keep(&mut self.graph, ev);
        let g = Rc::clone(&ev.graph);
        let sc = cx.sc;
        let ast = &sc.ast;
        match (l, &ev.kind) {
            (0, EventKind::CodePathStart(cp, node)) => {
                let is_ctor = ast.is(*node, "FunctionExpression")
                    && ast.parent(*node).is_some_and(|p| {
                        ast.is(p, "MethodDefinition") && ast.str(p, "kind") == Some("constructor")
                    });
                let has_extends = is_ctor && {
                    let class = ast
                        .parent(*node)
                        .and_then(|p| ast.parent(p))
                        .and_then(|p| ast.parent(p));
                    class
                        .and_then(|c| ast.child(c, "superClass"))
                        .is_some_and(|s| !is_null_or_undefined(ast, s))
                };
                self.stack.push(FuncInfo {
                    is_constructor: is_ctor,
                    has_extends,
                    code_path: *cp,
                    current: Vec::new(),
                });
            }
            (1, EventKind::CodePathEnd(cp, _)) => {
                let derived = self.stack.pop().is_some_and(|f| f.has_extends);
                if !derived {
                    return;
                }
                let mut reported: Vec<NodeId> = Vec::new();
                let mut missing = false;
                let map = &self.map;
                g.traverse_segments(*cp, None, None, |seg, c| {
                    let Some(info) = map.get(&seg) else {
                        missing = true;
                        c.break_();
                        return;
                    };
                    for n in &info.invalid_nodes {
                        if !reported.contains(n) {
                            reported.push(*n);
                        }
                    }
                    if info.super_called {
                        c.skip();
                    }
                });
                if missing {
                    self.unsupported = Some("segment without info");
                    return;
                }
                for n in reported {
                    let kind = if ast.is(n, "Super") { "super" } else { "this" };
                    cx.report(Report::node(n, "noBeforeSuper").data("kind", kind));
                }
            }
            (2, EventKind::SegmentStart(s, _)) => {
                if let Some(f) = self.stack.last_mut() {
                    segs::add(&mut f.current, *s);
                }
                if !self.derived() {
                    return;
                }
                let prev = &g.seg(*s).prev;
                let super_called =
                    !prev.is_empty() && prev.iter().all(|p| is_called(&self.map, &g, *p));
                self.map.insert(
                    *s,
                    SegInfo {
                        super_called,
                        invalid_nodes: Vec::new(),
                    },
                );
            }
            (3, _) => {
                if let (Some(s), Some(f)) = (ev.segment(), self.stack.last_mut()) {
                    segs::add(&mut f.current, s);
                }
            }
            (4 | 5, _) => {
                if let (Some(s), Some(f)) = (ev.segment(), self.stack.last_mut()) {
                    segs::delete(&mut f.current, s);
                }
            }
            (6, EventKind::SegmentLoop(from, to, _)) => {
                if !self.derived() {
                    return;
                }
                let Some(cp) = self.stack.last().map(|f| f.code_path) else {
                    return;
                };
                let map = &mut self.map;
                g.traverse_segments(cp, Some(*to), Some(*from), |seg, c| {
                    if map.get(&seg).is_some_and(|i| i.super_called) {
                        c.skip();
                        return;
                    }
                    let prev = &g.seg(seg).prev;
                    let called = !prev.is_empty() && prev.iter().all(|p| is_called(map, &g, *p));
                    let info = map.entry(seg).or_default();
                    if called {
                        info.super_called = true;
                    }
                });
            }
            _ => {}
        }
    }

    fn on(&mut self, l: usize, node: NodeId, cx: &mut Cx) {
        let ast = &cx.sc.ast;
        match l {
            7 => {
                if self.before_call_of_super() {
                    self.for_each_reachable_current(|i| i.invalid_nodes.push(node));
                }
            }
            8 => {
                let is_callee = ast.parent(node).is_some_and(|p| {
                    ast.is(p, "CallExpression") && ast.child(p, "callee") == Some(node)
                });
                if !is_callee && self.before_call_of_super() {
                    self.for_each_reachable_current(|i| i.invalid_nodes.push(node));
                }
            }
            9 => {
                if ast
                    .child(node, "callee")
                    .is_some_and(|c| ast.is(c, "Super"))
                    && self.before_call_of_super()
                {
                    self.for_each_reachable_current(|i| i.super_called = true);
                }
            }
            _ => self.map.clear(),
        }
    }
}
