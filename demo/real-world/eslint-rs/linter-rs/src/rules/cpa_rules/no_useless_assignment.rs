//! lib/rules/no-useless-assignment.js

use super::segs;
use crate::ast::{Ast, NodeId};
use crate::code_path::{Event, Graph, SegId};
use crate::eslint_utils::find_variable;
use crate::linter::Cx;
use crate::rule::Report;
use crate::rules::{Rule, RuleDef};
use crate::scope::{DefType, ScopeId, ScopeManager, ScopeType, VarId};
use serde_json::Value;
use std::collections::{HashMap, HashSet, VecDeque};

pub static DEF: RuleDef = RuleDef {
    name: "no-useless-assignment",
    messages: &[(
        "unnecessaryAssignment",
        "The value assigned to '{{name}}' is not used in subsequent statements.",
    )],
    has_default_options: false,
    create,
};

struct Assign {
    var: VarId,
    ident: NodeId,
    expr: Option<NodeId>,
    segs: Vec<SegId>,
}

#[derive(Default, Clone, Copy)]
struct SegInfo {
    first: Option<NodeId>,
    last: Option<NodeId>,
}

struct Frame {
    scope: ScopeId,
    segments: HashMap<SegId, SegInfo>,
    current: Vec<SegId>,
    assignments: Vec<(VarId, Vec<Assign>)>,
    try_blocks: Vec<NodeId>,
}

#[derive(Default)]
struct R {
    stack: Vec<Frame>,
    start_scopes: HashSet<ScopeId>,
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
    "onCodePathSegmentEnd",
    "TryStatement",
    "Identifier, JSXIdentifier",
    "VariableDeclarator[init!=null], AssignmentExpression, UpdateExpression:exit",
];

fn extract_identifiers(ast: &Ast, pattern: NodeId, out: &mut Vec<NodeId>) {
    match ast.kind(pattern) {
        "Identifier" => out.push(pattern),
        "ObjectPattern" => {
            for p in ast.list(pattern, "properties").iter().flatten() {
                let target = if ast.is(*p, "Property") {
                    ast.child(*p, "value")
                } else {
                    Some(*p)
                };
                if let Some(t) = target {
                    extract_identifiers(ast, t, out);
                }
            }
        }
        "ArrayPattern" => {
            for e in ast.list(pattern, "elements").iter().flatten() {
                extract_identifiers(ast, *e, out);
            }
        }
        "RestElement" => {
            if let Some(a) = ast.child(pattern, "argument") {
                extract_identifiers(ast, a, out);
            }
        }
        "AssignmentPattern" => {
            if let Some(l) = ast.child(pattern, "left") {
                extract_identifiers(ast, l, out);
            }
        }
        _ => {}
    }
}

fn evaluated_after(ast: &Ast, a: &Assign, identifier: NodeId) -> bool {
    let id = ast.range(identifier);
    if id.0 < ast.range(a.ident).1 {
        return false;
    }
    if let Some(e) = a.expr {
        let e = ast.range(e);
        if e.0 <= id.0 && id.1 <= e.1 {
            return false;
        }
    }
    true
}

fn used_between_assigned_and_equal(ast: &Ast, a: &Assign, identifier: NodeId) -> bool {
    let Some(e) = a.expr else {
        return false;
    };
    let id = ast.range(identifier);
    ast.range(a.ident).1 <= id.0 && id.1 <= ast.range(e).0
}

impl R {
    fn code_path_start_scope(&self, sm: &ScopeManager, scope: ScopeId) -> Option<ScopeId> {
        let mut target = Some(scope);
        while let Some(t) = target {
            if self.start_scopes.contains(&t) {
                return Some(t);
            }
            target = sm.scope(t).upper;
        }
        None
    }

    fn verify(&self, target: &mut Frame, g: &Graph, cx: &mut Cx) {
        let sm = cx.sm();
        let ast = &cx.sc.ast;
        let mut all = std::mem::take(&mut target.assignments);
        let mut reports = Vec::new();
        for (_, assignments) in all.iter_mut() {
            assignments.sort_by_key(|a| ast.range(a.ident).0);
            for ai in 0..assignments.len() {
                if self.verify_assignment(target, g, sm, ast, ai, assignments) {
                    reports.push(assignments[ai].ident);
                }
            }
        }
        for ident in reports {
            let name = ast.name(ident).unwrap_or("").to_string();
            cx.report(Report::node(ident, "unnecessaryAssignment").data("name", name));
        }
    }

    fn verify_assignment(
        &self,
        target: &Frame,
        g: &Graph,
        sm: &ScopeManager,
        ast: &Ast,
        ti: usize,
        all: &[Assign],
    ) -> bool {
        let ta = &all[ti];
        let used_in_segment = |s: SegId, identifier: NodeId| -> bool {
            let Some(info) = target.segments.get(&s) else {
                return false;
            };
            let (Some(first), Some(last)) = (info.first, info.last) else {
                return false;
            };
            let id = ast.range(identifier);
            ast.range(first).0 <= id.0 && id.1 <= ast.range(last).1
        };
        let tid = ast.range(ta.ident);
        if target.try_blocks.iter().any(|b| {
            let b = ast.range(*b);
            b.0 <= tid.0 && tid.1 <= b.1
        }) {
            return false;
        }

        let mut results: Vec<(SegId, Option<usize>)> = Vec::new();
        let mut visited: HashSet<SegId> = HashSet::new();
        let mut queue: VecDeque<SegId> = ta
            .segs
            .iter()
            .flat_map(|s| g.seg(*s).next.iter().copied())
            .collect();
        let mut next_result = |i: usize, results: &mut Vec<(SegId, Option<usize>)>| {
            if i < results.len() {
                return Some(results[i]);
            }
            while let Some(next) = queue.pop_front() {
                if !visited.insert(next) {
                    continue;
                }
                let found = all.iter().position(|o| {
                    o.segs.contains(&next) && !used_between_assigned_and_equal(ast, o, ta.ident)
                });
                if found.is_none() {
                    queue.extend(g.seg(next).next.iter().copied());
                }
                results.push((next, found));
                return Some((next, found));
            }
            None
        };

        let variable = sm.var(ta.var);
        let reads: Vec<_> = variable
            .references
            .iter()
            .map(|r| sm.reference(*r))
            .filter(|r| r.is_read())
            .collect();
        if reads.is_empty() {
            return false;
        }

        let other = all.iter().enumerate().find_map(|(i, a)| {
            if i == ti || (!a.segs.is_empty() && a.segs.iter().all(|s| !ta.segs.contains(s))) {
                return None;
            }
            if evaluated_after(ast, ta, a.ident) {
                return Some(a);
            }
            if let Some(e) = a.expr {
                let e = ast.range(e);
                if e.0 <= tid.0 && tid.1 <= e.1 {
                    return Some(a);
                }
            }
            None
        });

        for reference in reads {
            if self.code_path_start_scope(sm, reference.from) != Some(target.scope) {
                return false;
            }
            let rid = reference.identifier;
            if evaluated_after(ast, ta, rid)
                && (used_between_assigned_and_equal(ast, ta, rid)
                    || ta.segs.iter().any(|s| used_in_segment(*s, rid)))
            {
                if other.is_some_and(|o| evaluated_after(ast, o, rid)) {
                    continue;
                }
                return false;
            }
            if other.is_some() {
                continue;
            }
            let mut i = 0;
            while let Some((seg, assignment)) = next_result(i, &mut results) {
                i += 1;
                if used_in_segment(seg, rid) {
                    if assignment.is_some_and(|a| evaluated_after(ast, &all[a], rid)) {
                        continue;
                    }
                    return false;
                }
            }
        }
        true
    }

    fn on_assignment(&mut self, node: NodeId, cx: &mut Cx) {
        let ast = &cx.sc.ast;
        let sm = cx.sm();
        let Some(frame) = self.stack.last() else {
            return;
        };
        if frame.current.is_empty() {
            return;
        }
        let (pattern, expression) = match ast.kind(node) {
            "VariableDeclarator" => (ast.child(node, "id"), ast.child(node, "init")),
            "AssignmentExpression" => (ast.child(node, "left"), ast.child(node, "right")),
            _ => (ast.child(node, "argument"), None),
        };
        let mut idents = Vec::new();
        if let Some(p) = pattern {
            extract_identifiers(ast, p, &mut idents);
        }
        let frame_scope = frame.scope;
        let current = frame.current.clone();
        for identifier in idents {
            let scope = cx.get_scope(identifier);
            let Some(vid) = find_variable(sm, ast, scope, identifier) else {
                continue;
            };
            let variable = sm.var(vid);
            let vscope = sm.scope(variable.scope).kind;
            if vscope == ScopeType::Global && variable.defs.is_empty() {
                continue;
            }
            if self.code_path_start_scope(sm, variable.scope) != Some(frame_scope) {
                continue;
            }
            if variable.eslint_used.get() {
                continue;
            }
            if vscope == ScopeType::Module {
                let exported_decl = |n: Option<NodeId>| {
                    n.and_then(|n| ast.parent(n)).is_some_and(|p| {
                        matches!(
                            ast.kind(p),
                            "ExportNamedDeclaration" | "ExportDefaultDeclaration"
                        )
                    })
                };
                if variable.defs.iter().any(|def| match def.kind {
                    DefType::Variable => def
                        .parent
                        .and_then(|p| ast.parent(p))
                        .is_some_and(|pp| ast.is(pp, "ExportNamedDeclaration")),
                    DefType::FunctionName | DefType::ClassName => exported_decl(Some(def.node)),
                    _ => false,
                }) {
                    continue;
                }
                if variable.references.iter().any(|r| {
                    ast.parent(sm.reference(*r).identifier)
                        .is_some_and(|p| ast.is(p, "ExportSpecifier"))
                }) {
                    continue;
                }
            }
            let Some(frame) = self.stack.last_mut() else {
                return;
            };
            let a = Assign {
                var: vid,
                ident: identifier,
                expr: expression,
                segs: current.clone(),
            };
            match frame.assignments.iter_mut().find(|(v, _)| *v == vid) {
                Some((_, list)) => list.push(a),
                None => frame.assignments.push((vid, vec![a])),
            }
        }
    }
}

impl Rule for R {
    fn listeners(&self) -> &'static [&'static str] {
        LISTENERS
    }

    fn on_code_path(&mut self, l: usize, ev: &Event, cx: &mut Cx) {
        match l {
            0 => {
                let scope = cx.get_scope(ev.node());
                self.stack.push(Frame {
                    scope,
                    segments: HashMap::new(),
                    current: Vec::new(),
                    assignments: Vec::new(),
                    try_blocks: Vec::new(),
                });
                self.start_scopes.insert(scope);
            }
            1 => {
                if let Some(mut frame) = self.stack.pop() {
                    self.verify(&mut frame, &ev.graph, cx);
                }
            }
            2 => {
                if let (Some(s), Some(frame)) = (ev.segment(), self.stack.last_mut()) {
                    frame.segments.insert(s, SegInfo::default());
                    segs::add(&mut frame.current, s);
                }
            }
            3 => {
                if let (Some(s), Some(frame)) = (ev.segment(), self.stack.last_mut()) {
                    segs::delete(&mut frame.current, s);
                }
            }
            _ => {}
        }
    }

    fn on(&mut self, l: usize, node: NodeId, cx: &mut Cx) {
        let ast = &cx.sc.ast;
        match l {
            4 => {
                if let (Some(b), Some(frame)) = (ast.child(node, "block"), self.stack.last_mut()) {
                    frame.try_blocks.push(b);
                }
            }
            5 => {
                if let Some(frame) = self.stack.last_mut() {
                    for s in frame.current.clone() {
                        let info = frame.segments.entry(s).or_default();
                        if info.first.is_none() {
                            info.first = Some(node);
                        }
                        info.last = Some(node);
                    }
                }
            }
            6 => {
                if ast.is(node, "VariableDeclarator") && ast.child(node, "init").is_none() {
                    return;
                }
                self.on_assignment(node, cx);
            }
            _ => {}
        }
    }
}
