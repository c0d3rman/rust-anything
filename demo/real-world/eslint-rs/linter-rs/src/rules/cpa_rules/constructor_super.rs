//! lib/rules/constructor-super.js

use super::segs;
use crate::ast::{Ast, NodeId};
use crate::code_path::{CpId, Event, EventKind, Graph, SegId};
use crate::linter::Cx;
use crate::rule::Report;
use crate::rules::{Rule, RuleDef};
use serde_json::Value;
use std::collections::HashMap;
use std::rc::Rc;

pub static DEF: RuleDef = RuleDef {
    name: "constructor-super",
    messages: &[
        (
            "missingSome",
            "Lacked a call of 'super()' in some code paths.",
        ),
        ("missingAll", "Expected to call 'super()'."),
        ("duplicate", "Unexpected duplicate 'super()'."),
        (
            "badSuper",
            "Unexpected 'super()' because 'super' is not a constructor.",
        ),
    ],
    has_default_options: false,
    create,
};

fn is_constructor_function(ast: &Ast, node: NodeId) -> bool {
    ast.is(node, "FunctionExpression")
        && ast.parent(node).is_some_and(|p| {
            ast.is(p, "MethodDefinition") && ast.str(p, "kind") == Some("constructor")
        })
}

fn is_possible_constructor(ast: &Ast, node: Option<NodeId>) -> bool {
    let Some(node) = node else { return false };
    match ast.kind(node) {
        "ClassExpression"
        | "FunctionExpression"
        | "ThisExpression"
        | "MemberExpression"
        | "CallExpression"
        | "NewExpression"
        | "ChainExpression"
        | "YieldExpression"
        | "TaggedTemplateExpression"
        | "MetaProperty" => true,
        "Identifier" => ast.name(node) != Some("undefined"),
        "AssignmentExpression" => match ast.str(node, "operator") {
            Some("=" | "&&=") => is_possible_constructor(ast, ast.child(node, "right")),
            Some("||=" | "??=") => {
                is_possible_constructor(ast, ast.child(node, "left"))
                    || is_possible_constructor(ast, ast.child(node, "right"))
            }
            _ => false,
        },
        "LogicalExpression" => {
            if ast.str(node, "operator") == Some("&&") {
                return is_possible_constructor(ast, ast.child(node, "right"));
            }
            is_possible_constructor(ast, ast.child(node, "left"))
                || is_possible_constructor(ast, ast.child(node, "right"))
        }
        "ConditionalExpression" => {
            is_possible_constructor(ast, ast.child(node, "alternate"))
                || is_possible_constructor(ast, ast.child(node, "consequent"))
        }
        "SequenceExpression" => {
            is_possible_constructor(ast, ast.list(node, "expressions").last().copied().flatten())
        }
        _ => false,
    }
}

#[derive(Default)]
struct SegInfo {
    every: bool,
    some: bool,
    valid_nodes: Vec<NodeId>,
}

struct FuncInfo {
    is_constructor: bool,
    has_extends: bool,
    super_is_constructor: bool,
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

impl R {
    fn active(&self) -> bool {
        self.stack
            .last()
            .is_some_and(|f| f.is_constructor && f.has_extends)
    }

    fn called_in_some(&mut self, g: &Graph, s: SegId) -> bool {
        if !g.reachable(s) {
            return false;
        }
        match self.map.get(&s) {
            Some(i) => i.some,
            None => {
                self.unsupported = Some("segment without info");
                false
            }
        }
    }

    fn called_in_every(&mut self, g: &Graph, s: SegId) -> bool {
        if !g.reachable(s) {
            return false;
        }
        match self.map.get(&s) {
            Some(i) => i.every,
            None => {
                self.unsupported = Some("segment without info");
                false
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
            "CallExpression:exit",
            "ReturnStatement",
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
                let info = if is_constructor_function(ast, *node) {
                    let class = ast
                        .parent(*node)
                        .and_then(|p| ast.parent(p))
                        .and_then(|p| ast.parent(p));
                    let sup = class.and_then(|c| ast.child(c, "superClass"));
                    FuncInfo {
                        is_constructor: true,
                        has_extends: sup.is_some(),
                        super_is_constructor: is_possible_constructor(ast, sup),
                        code_path: *cp,
                        current: Vec::new(),
                    }
                } else {
                    FuncInfo {
                        is_constructor: false,
                        has_extends: false,
                        super_is_constructor: false,
                        code_path: *cp,
                        current: Vec::new(),
                    }
                };
                self.stack.push(info);
            }
            (1, EventKind::CodePathEnd(cp, node)) => {
                let has_extends = self.stack.pop().is_some_and(|f| f.has_extends);
                if !has_extends {
                    return;
                }
                let returned = g.path(*cp).returned_segments.clone();
                let mut every = true;
                for s in &returned {
                    if !self.called_in_every(&g, *s) {
                        every = false;
                        break;
                    }
                }
                let mut some = false;
                for s in &returned {
                    if self.called_in_some(&g, *s) {
                        some = true;
                        break;
                    }
                }
                if !every && let Some(p) = ast.parent(*node) {
                    cx.report(Report::node(
                        p,
                        if some { "missingSome" } else { "missingAll" },
                    ));
                }
            }
            (2, EventKind::SegmentStart(s, node)) => {
                if let Some(f) = self.stack.last_mut() {
                    segs::add(&mut f.current, *s);
                }
                if !self.active() {
                    return;
                }
                self.map.insert(*s, SegInfo::default());
                let seen: Vec<SegId> = g
                    .seg(*s)
                    .prev
                    .iter()
                    .copied()
                    .filter(|p| self.map.contains_key(p))
                    .collect();
                let mut info = SegInfo::default();
                if !seen.is_empty() {
                    info.some = seen
                        .iter()
                        .any(|p| g.reachable(*p) && self.map.get(p).is_some_and(|i| i.some));
                    info.every = seen
                        .iter()
                        .all(|p| g.reachable(*p) && self.map.get(p).is_some_and(|i| i.every));
                }
                let for_update = ast.parent(*node).is_some_and(|p| {
                    ast.is(p, "ForStatement") && ast.child(p, "update") == Some(*node)
                });
                if for_update {
                    info.every = true;
                }
                self.map.insert(*s, info);
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
                if !self.active() {
                    return;
                }
                let Some(cp) = self.stack.last().map(|f| f.code_path) else {
                    return;
                };
                let mut reports = Vec::new();
                let map = &mut self.map;
                g.traverse_segments(cp, Some(*to), Some(*from), |seg, c| {
                    if !map.contains_key(&seg) {
                        c.skip();
                        return;
                    }
                    let seen: Vec<SegId> = g
                        .seg(seg)
                        .prev
                        .iter()
                        .copied()
                        .filter(|p| map.contains_key(p))
                        .collect();
                    let some = seen
                        .iter()
                        .any(|p| g.reachable(*p) && map.get(p).is_some_and(|i| i.some));
                    let every = seen
                        .iter()
                        .all(|p| g.reachable(*p) && map.get(p).is_some_and(|i| i.every));
                    let Some(info) = map.get_mut(&seg) else {
                        return;
                    };
                    info.some = info.some || some;
                    info.every = info.every || every;
                    if some {
                        reports.extend(std::mem::take(&mut info.valid_nodes));
                    }
                });
                for n in reports {
                    cx.report(Report::node(n, "duplicate"));
                }
            }
            _ => {}
        }
    }

    fn on(&mut self, l: usize, node: NodeId, cx: &mut Cx) {
        if !self.active() {
            return;
        }
        let ast = &cx.sc.ast;
        let Some(g) = self.graph.clone() else { return };
        let Some(f) = self.stack.last() else { return };
        let current = f.current.clone();
        if l == 7 {
            if !ast
                .child(node, "callee")
                .is_some_and(|c| ast.is(c, "Super"))
            {
                return;
            }
            let super_is_constructor = f.super_is_constructor;
            let mut duplicate = false;
            let mut last = None;
            for s in current {
                if g.reachable(s) {
                    let Some(info) = self.map.get_mut(&s) else {
                        self.unsupported = Some("segment without info");
                        return;
                    };
                    duplicate = duplicate || info.some;
                    info.some = true;
                    info.every = true;
                    last = Some(s);
                }
            }
            if let Some(s) = last {
                if duplicate {
                    cx.report(Report::node(node, "duplicate"));
                } else if !super_is_constructor {
                    cx.report(Report::node(node, "badSuper"));
                } else if let Some(info) = self.map.get_mut(&s) {
                    info.valid_nodes.push(node);
                }
            }
        } else {
            if ast.child(node, "argument").is_none() {
                return;
            }
            for s in current {
                if g.reachable(s) {
                    let Some(info) = self.map.get_mut(&s) else {
                        self.unsupported = Some("segment without info");
                        return;
                    };
                    info.some = true;
                    info.every = true;
                }
            }
        }
    }
}
