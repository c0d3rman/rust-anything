//! lib/rules/no-useless-return.js

use super::segs;
use crate::ast::{Ast, NodeId};
use crate::code_path::{Event, Graph, SegId};
use crate::eslint_utils::is_in_loop;
use crate::linter::Cx;
use crate::rule::{Report, fixer};
use crate::rules::{Rule, RuleDef};
use crate::utils::is_function;
use serde_json::Value;
use std::collections::HashMap;
use std::rc::Rc;

pub static DEF: RuleDef = RuleDef {
    name: "no-useless-return",
    messages: &[("unnecessaryReturn", "Unnecessary return statement.")],
    has_default_options: false,
    create,
};

struct SegInfo {
    useless: Vec<NodeId>,
    returned: bool,
}

struct ScopeInfo {
    useless: Vec<NodeId>,
    try_blocks: Vec<NodeId>,
    current: Vec<SegId>,
}

#[derive(Default)]
struct R {
    graph: Option<Rc<Graph>>,
    map: HashMap<SegId, SegInfo>,
    stack: Vec<ScopeInfo>,
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
    "onCodePathSegmentStart",
    "onUnreachableCodePathSegmentStart",
    "onUnreachableCodePathSegmentEnd",
    "onCodePathSegmentEnd",
    "ReturnStatement",
    "TryStatement > BlockStatement.block:exit",
    "TryStatement:exit",
    "ClassDeclaration",
    "ContinueStatement",
    "DebuggerStatement",
    "DoWhileStatement",
    "EmptyStatement",
    "ExpressionStatement",
    "ForInStatement",
    "ForOfStatement",
    "ForStatement",
    "IfStatement",
    "ImportDeclaration",
    "LabeledStatement",
    "SwitchStatement",
    "ThrowStatement",
    "TryStatement",
    "VariableDeclaration",
    "WhileStatement",
    "WithStatement",
    "ExportNamedDeclaration",
    "ExportDefaultDeclaration",
    "ExportAllDeclaration",
];

fn is_in_finally(ast: &Ast, node: NodeId) -> bool {
    let mut cur = node;
    while let Some(p) = ast.parent(cur) {
        if is_function(ast, cur) {
            break;
        }
        if ast.is(p, "TryStatement") && ast.child(p, "finalizer") == Some(cur) {
            return true;
        }
        cur = p;
    }
    false
}

impl R {
    fn is_returned(&self, s: SegId) -> bool {
        self.map.get(&s).is_none_or(|i| i.returned)
    }

    fn get_useless_returns(
        &self,
        g: &Graph,
        out: &mut Vec<NodeId>,
        prev: &[SegId],
        traversed: &mut Vec<SegId>,
    ) {
        for &s in prev {
            if !g.reachable(s) {
                if !traversed.contains(&s) {
                    traversed.push(s);
                    let next: Vec<SegId> = g
                        .seg(s)
                        .all_prev
                        .iter()
                        .copied()
                        .filter(|p| self.is_returned(*p))
                        .collect();
                    self.get_useless_returns(g, out, &next, traversed);
                }
                continue;
            }
            if let Some(info) = self.map.get(&s) {
                out.extend(info.useless.iter().copied());
            }
        }
    }

    fn mark_segment(&mut self, g: &Graph, ast: &Ast, s: SegId, used: &mut Vec<SegId>) {
        if !g.reachable(s) {
            if !used.contains(&s) {
                used.push(s);
            }
            let prevs: Vec<SegId> = g
                .seg(s)
                .all_prev
                .iter()
                .copied()
                .filter(|p| self.is_returned(*p))
                .filter(|p| !used.contains(p))
                .collect();
            for p in prevs {
                self.mark_segment(g, ast, p, used);
            }
            return;
        }
        let Some(info) = self.map.get_mut(&s) else {
            return;
        };
        let Some(scope) = self.stack.last_mut() else {
            return;
        };
        let nodes = std::mem::take(&mut info.useless);
        let mut kept = Vec::new();
        for n in nodes {
            if !scope.try_blocks.is_empty() {
                let (rs, re) = ast.range(n);
                if scope.try_blocks.iter().any(|b| {
                    let (bs, be) = ast.range(*b);
                    rs >= bs && re <= be
                }) {
                    kept.push(n);
                    continue;
                }
            }
            if let Some(i) = scope.useless.iter().position(|x| *x == n) {
                scope.useless.remove(i);
            }
        }
        info.useless = kept;
    }

    fn mark_current(&mut self, ast: &Ast) {
        let Some(g) = self.graph.clone() else { return };
        let current = self
            .stack
            .last()
            .map(|s| s.current.clone())
            .unwrap_or_default();
        for s in current {
            self.mark_segment(&g, ast, s, &mut Vec::new());
        }
    }
}

impl Rule for R {
    fn listeners(&self) -> &'static [&'static str] {
        LISTENERS
    }

    fn on_code_path(&mut self, l: usize, ev: &Event, cx: &mut Cx) {
        segs::keep(&mut self.graph, ev);
        let sc = cx.sc;
        let ast = &sc.ast;
        match l {
            0 => self.stack.push(ScopeInfo {
                useless: Vec::new(),
                try_blocks: Vec::new(),
                current: Vec::new(),
            }),
            1 => {
                let Some(scope) = self.stack.pop() else {
                    return;
                };
                for node in scope.useless {
                    let removable = ast.parent(node).is_some_and(|p| {
                        matches!(
                            ast.kind(p),
                            "Program" | "BlockStatement" | "StaticBlock" | "SwitchCase"
                        )
                    });
                    let mut r = Report::loc(sc.node_loc(node), "unnecessaryReturn");
                    r.node = Some(node);
                    if removable && sc.comments_inside(node).is_empty() {
                        let func = std::iter::once(node)
                            .chain(ast.ancestors(node))
                            .find(|n| is_function(ast, *n));
                        let retained = ast.range(func.unwrap_or(sc.root));
                        let range = ast.range(node);
                        let actual = (retained.0.min(range.0), retained.1.max(range.1));
                        let text = format!(
                            "{}{}",
                            sc.slice16(actual.0, range.0),
                            sc.slice16(range.1, actual.1)
                        );
                        r = r.fix(fixer::replace_range(actual, text));
                    }
                    cx.report(r);
                }
            }
            2 => {
                let Some(s) = ev.segment() else { return };
                if let Some(scope) = self.stack.last_mut() {
                    segs::add(&mut scope.current, s);
                }
                let mut useless = Vec::new();
                let all_prev = ev.graph.seg(s).all_prev.clone();
                self.get_useless_returns(&ev.graph, &mut useless, &all_prev, &mut Vec::new());
                self.map.insert(
                    s,
                    SegInfo {
                        useless,
                        returned: false,
                    },
                );
            }
            3 => {
                if let (Some(s), Some(scope)) = (ev.segment(), self.stack.last_mut()) {
                    segs::add(&mut scope.current, s);
                }
            }
            4 | 5 => {
                if let (Some(s), Some(scope)) = (ev.segment(), self.stack.last_mut()) {
                    segs::delete(&mut scope.current, s);
                }
            }
            _ => {}
        }
    }

    fn on(&mut self, l: usize, node: NodeId, cx: &mut Cx) {
        let sc = cx.sc;
        let ast = &sc.ast;
        match LISTENERS[l] {
            "ReturnStatement" => {
                let has_arg = ast.child(node, "argument").is_some();
                if has_arg {
                    self.mark_current(ast);
                }
                let current = self
                    .stack
                    .last()
                    .map(|s| s.current.clone())
                    .unwrap_or_default();
                if has_arg
                    || is_in_loop(ast, node)
                    || is_in_finally(ast, node)
                    || !segs::any_reachable(self.graph.as_ref(), &current)
                {
                    return;
                }
                for s in current {
                    if let Some(info) = self.map.get_mut(&s) {
                        info.useless.push(node);
                        info.returned = true;
                    }
                }
                if let Some(scope) = self.stack.last_mut() {
                    scope.useless.push(node);
                }
            }
            "TryStatement > BlockStatement.block:exit" => {
                let is_block = ast.parent(node).is_some_and(|p| {
                    ast.is(p, "TryStatement") && ast.child(p, "block") == Some(node)
                });
                if is_block && let Some(scope) = self.stack.last_mut() {
                    scope.try_blocks.push(node);
                }
            }
            "TryStatement:exit" => {
                if let Some(scope) = self.stack.last_mut() {
                    scope.try_blocks.pop();
                }
            }
            _ => self.mark_current(ast),
        }
    }
}
