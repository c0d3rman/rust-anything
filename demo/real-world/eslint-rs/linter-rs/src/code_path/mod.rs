//! Code path analysis (lib/linter/code-path-analysis) and the traversal of
//! `SourceCode#traverse()` (lib/languages/js/source-code/source-code.js).

mod analyzer;
mod state;

use crate::ast::NodeId;
use crate::source::SourceCode;
use std::rc::Rc;

pub type SegId = u32;
pub type CpId = u32;

/// `CodePathSegment`.
pub struct Segment {
    pub id: String,
    pub next: Vec<SegId>,
    pub prev: Vec<SegId>,
    pub all_next: Vec<SegId>,
    pub all_prev: Vec<SegId>,
    pub reachable: bool,
    pub used: bool,
    pub looped_prev: Vec<SegId>,
}

/// `CodePath`, with the state's arrays as they are when the code path ends.
pub struct CodePath {
    pub id: String,
    pub origin: &'static str,
    pub upper: Option<CpId>,
    pub children: Vec<CpId>,
    pub initial_segment: SegId,
    pub final_segments: Vec<SegId>,
    pub returned_segments: Vec<SegId>,
    pub thrown_segments: Vec<SegId>,
}

#[derive(Default)]
pub struct Graph {
    pub segments: Vec<Segment>,
    pub paths: Vec<CodePath>,
}

#[derive(Default)]
pub struct Controller {
    skip: bool,
    brk: bool,
}

impl Controller {
    pub fn skip(&mut self) {
        self.skip = true;
    }

    pub fn break_(&mut self) {
        self.brk = true;
    }
}

impl Graph {
    pub fn seg(&self, s: SegId) -> &Segment {
        &self.segments[s as usize]
    }

    pub fn reachable(&self, s: SegId) -> bool {
        self.segments[s as usize].reachable
    }

    pub fn path(&self, c: CpId) -> &CodePath {
        &self.paths[c as usize]
    }

    fn is_looped_prev(&self, s: SegId, p: SegId) -> bool {
        self.seg(s).looped_prev.contains(&p)
    }

    /// `CodePath#traverseSegments`.
    pub fn traverse_segments(
        &self,
        cp: CpId,
        first: Option<SegId>,
        last: Option<SegId>,
        mut cb: impl FnMut(SegId, &mut Controller),
    ) {
        let start = first.unwrap_or(self.path(cp).initial_segment);
        let n = self.segments.len();
        let mut visited = vec![false; n];
        let mut skipped = vec![false; n];
        let mut skipped_any = false;
        let mut stack: Vec<(SegId, usize)> = vec![(start, 0)];
        while let Some(&(seg, index)) = stack.last() {
            if index == 0 {
                if visited[seg as usize] {
                    stack.pop();
                    continue;
                }
                let prev = &self.seg(seg).prev;
                if seg != start
                    && !prev.is_empty()
                    && !prev
                        .iter()
                        .all(|&p| visited[p as usize] || self.is_looped_prev(seg, p))
                {
                    stack.pop();
                    continue;
                }
                visited[seg as usize] = true;
                let should_skip = skipped_any
                    && !prev.is_empty()
                    && prev
                        .iter()
                        .all(|&p| skipped[p as usize] || self.is_looped_prev(seg, p));
                if !should_skip {
                    let mut c = Controller::default();
                    cb(seg, &mut c);
                    if c.skip || Some(seg) == last {
                        skipped[seg as usize] = true;
                        skipped_any = true;
                    }
                    if c.brk {
                        break;
                    }
                } else {
                    skipped[seg as usize] = true;
                    skipped_any = true;
                }
            }
            let next = &self.seg(seg).next;
            let end = next.len() as i64 - 1;
            let top = stack.len() - 1;
            if (index as i64) < end {
                stack[top].1 += 1;
                stack.push((next[index], 0));
            } else if index as i64 == end {
                stack[top] = (next[index], 0);
            } else {
                stack.pop();
            }
        }
    }
}

pub enum EventKind {
    CodePathStart(CpId, NodeId),
    CodePathEnd(CpId, NodeId),
    SegmentStart(SegId, NodeId),
    SegmentEnd(SegId, NodeId),
    UnreachableSegmentStart(SegId, NodeId),
    UnreachableSegmentEnd(SegId, NodeId),
    SegmentLoop(SegId, SegId, NodeId),
}

pub struct Event {
    pub kind: EventKind,
    pub graph: Rc<Graph>,
}

impl Event {
    pub fn node(&self) -> NodeId {
        match self.kind {
            EventKind::CodePathStart(_, n)
            | EventKind::CodePathEnd(_, n)
            | EventKind::SegmentStart(_, n)
            | EventKind::SegmentEnd(_, n)
            | EventKind::UnreachableSegmentStart(_, n)
            | EventKind::UnreachableSegmentEnd(_, n)
            | EventKind::SegmentLoop(_, _, n) => n,
        }
    }

    pub fn segment(&self) -> Option<SegId> {
        match self.kind {
            EventKind::SegmentStart(s, _)
            | EventKind::SegmentEnd(s, _)
            | EventKind::UnreachableSegmentStart(s, _)
            | EventKind::UnreachableSegmentEnd(s, _) => Some(s),
            _ => None,
        }
    }

    pub fn code_path(&self) -> Option<CpId> {
        match self.kind {
            EventKind::CodePathStart(c, _) | EventKind::CodePathEnd(c, _) => Some(c),
            _ => None,
        }
    }
}

pub enum Step {
    Enter(NodeId),
    Exit(NodeId),
    Event(Event),
}

pub fn event_name(ev: &Event) -> &'static str {
    match ev.kind {
        EventKind::CodePathStart(..) => "onCodePathStart",
        EventKind::CodePathEnd(..) => "onCodePathEnd",
        EventKind::SegmentStart(..) => "onCodePathSegmentStart",
        EventKind::SegmentEnd(..) => "onCodePathSegmentEnd",
        EventKind::UnreachableSegmentStart(..) => "onUnreachableCodePathSegmentStart",
        EventKind::UnreachableSegmentEnd(..) => "onUnreachableCodePathSegmentEnd",
        EventKind::SegmentLoop(..) => "onCodePathSegmentLoop",
    }
}

pub(crate) enum Raw {
    Enter(NodeId),
    Exit(NodeId),
    Ev(EventKind),
}

/// The traversal: enter and leave steps, with code path events between them when `with_events`.
pub fn steps(sc: &SourceCode, with_events: bool) -> Vec<Step> {
    if !with_events {
        let mut out = Vec::with_capacity(sc.ast.len() * 2);
        let mut stack: Vec<(NodeId, bool)> = vec![(sc.root, true)];
        while let Some((n, enter)) = stack.pop() {
            if enter {
                out.push(Step::Enter(n));
                stack.push((n, false));
                for c in sc.ast.children(n).into_iter().rev() {
                    stack.push((c, true));
                }
            } else {
                out.push(Step::Exit(n));
            }
        }
        return out;
    }
    let (raw, graph) = analyzer::analyze(&sc.ast, sc.root);
    let graph = Rc::new(graph);
    raw.into_iter()
        .map(|r| match r {
            Raw::Enter(n) => Step::Enter(n),
            Raw::Exit(n) => Step::Exit(n),
            Raw::Ev(kind) => Step::Event(Event {
                kind,
                graph: Rc::clone(&graph),
            }),
        })
        .collect()
}

fn seg_num(id: &str) -> u64 {
    id.rsplit('_')
        .next()
        .and_then(|n| n.parse().ok())
        .unwrap_or(0)
}

/// A text dump of the steps, for comparing with ESLint's analyzer.
pub fn dump(sc: &SourceCode) -> String {
    use std::fmt::Write;
    let st = steps(sc, true);
    let mut out = String::new();
    let mut seen: Vec<Vec<SegId>> = Vec::new();
    let nd = |n: NodeId| {
        let (s, e) = sc.ast.range(n);
        format!("{} {s}-{e}", sc.ast.kind(n))
    };
    for step in &st {
        match step {
            Step::Enter(n) => {
                let _ = writeln!(out, "enter {}", nd(*n));
            }
            Step::Exit(n) => {
                let _ = writeln!(out, "exit {}", nd(*n));
            }
            Step::Event(ev) => {
                let g = &ev.graph;
                if seen.len() < g.paths.len() {
                    seen.resize(g.paths.len(), Vec::new());
                }
                let name = event_name(ev);
                match ev.kind {
                    EventKind::CodePathStart(c, n) => {
                        let p = g.path(c);
                        let _ = writeln!(out, "{name} {} {} {}", p.id, p.origin, nd(n));
                    }
                    EventKind::CodePathEnd(c, n) => {
                        let p = g.path(c);
                        let ids = |v: &[SegId]| {
                            v.iter()
                                .map(|s| g.seg(*s).id.clone())
                                .collect::<Vec<_>>()
                                .join(",")
                        };
                        let _ = writeln!(
                            out,
                            "{name} {} {} initial={} final=[{}] returned=[{}] thrown=[{}]",
                            p.id,
                            nd(n),
                            g.seg(p.initial_segment).id,
                            ids(&p.final_segments),
                            ids(&p.returned_segments),
                            ids(&p.thrown_segments)
                        );
                        let mut set: Vec<SegId> = Vec::new();
                        let all = std::iter::once(&p.initial_segment)
                            .chain(&p.final_segments)
                            .chain(&p.returned_segments)
                            .chain(&p.thrown_segments)
                            .chain(&seen[c as usize]);
                        for s in all {
                            if !set.contains(s) {
                                set.push(*s);
                            }
                        }
                        let mut i = 0;
                        while i < set.len() {
                            let s = g.seg(set[i]);
                            for x in s
                                .all_next
                                .iter()
                                .chain(&s.all_prev)
                                .chain(&s.next)
                                .chain(&s.prev)
                            {
                                if !set.contains(x) {
                                    set.push(*x);
                                }
                            }
                            i += 1;
                        }
                        set.sort_by_key(|s| seg_num(&g.seg(*s).id));
                        for s in set {
                            let x = g.seg(s);
                            let _ = writeln!(
                                out,
                                "  seg {} reachable={} prev=[{}] next=[{}] allPrev=[{}] allNext=[{}] looped=[{}]",
                                x.id,
                                x.reachable,
                                ids(&x.prev),
                                ids(&x.next),
                                ids(&x.all_prev),
                                ids(&x.all_next),
                                ids(&x.looped_prev)
                            );
                        }
                    }
                    EventKind::SegmentLoop(a, b, n) => {
                        let _ = writeln!(out, "{name} {} {} {}", g.seg(a).id, g.seg(b).id, nd(n));
                    }
                    _ => {
                        let s = ev.segment().unwrap_or(0);
                        let cp = g
                            .seg(s)
                            .id
                            .split('_')
                            .next()
                            .unwrap_or("")
                            .trim_start_matches('s')
                            .parse::<usize>()
                            .unwrap_or(1);
                        if let Some(v) = seen.get_mut(cp.saturating_sub(1)) {
                            v.push(s);
                        }
                        let _ = writeln!(out, "{name} {} {}", g.seg(s).id, nd(ev.node()));
                    }
                }
            }
        }
    }
    out
}

#[cfg(test)]
mod tests;
