//! lib/linter/code-path-analysis/code-path-state.js, fork-context.js and code-path-segment.js

use super::{CpId, EventKind, Graph, Raw, SegId, Segment};
use crate::ast::NodeId;

type FcId = usize;

pub(super) struct Sink {
    pub g: Graph,
    pub out: Vec<Raw>,
    pub node: NodeId,
}

impl Graph {
    fn new_segment(&mut self, id: String, all_prev: Vec<SegId>, reachable: bool) -> SegId {
        let prev = all_prev
            .iter()
            .copied()
            .filter(|p| self.reachable(*p))
            .collect();
        self.segments.push(Segment {
            id,
            next: Vec::new(),
            prev,
            all_next: Vec::new(),
            all_prev,
            reachable,
            used: false,
            looped_prev: Vec::new(),
        });
        (self.segments.len() - 1) as SegId
    }

    fn flatten_unused(&self, segs: &[SegId]) -> Vec<SegId> {
        let mut done: Vec<SegId> = Vec::new();
        for &s in segs {
            if done.contains(&s) {
                continue;
            }
            let seg = self.seg(s);
            if !seg.used {
                for &p in &seg.all_prev {
                    if !done.contains(&p) {
                        done.push(p);
                    }
                }
            } else {
                done.push(s);
            }
        }
        done
    }

    fn new_next(&mut self, id: String, all_prev: &[SegId]) -> SegId {
        let flat = self.flatten_unused(all_prev);
        let reachable = all_prev.iter().any(|p| self.reachable(*p));
        self.new_segment(id, flat, reachable)
    }

    fn new_unreachable(&mut self, id: String, all_prev: &[SegId]) -> SegId {
        let flat = self.flatten_unused(all_prev);
        let s = self.new_segment(id, flat, false);
        self.mark_used(s);
        s
    }

    fn new_disconnected(&mut self, id: String, all_prev: &[SegId]) -> SegId {
        let reachable = all_prev.iter().any(|p| self.reachable(*p));
        self.new_segment(id, Vec::new(), reachable)
    }

    pub(super) fn mark_used(&mut self, s: SegId) {
        let seg = &mut self.segments[s as usize];
        if seg.used {
            return;
        }
        seg.used = true;
        let reachable = seg.reachable;
        let prevs = seg.all_prev.clone();
        for p in prevs {
            let ps = &mut self.segments[p as usize];
            ps.all_next.push(s);
            if reachable {
                ps.next.push(s);
            }
        }
    }
}

#[derive(Clone, Copy)]
enum Make {
    Next,
    Unreachable,
    Disconnected,
}

struct ForkCtx {
    upper: Option<FcId>,
    count: usize,
    list: Vec<Vec<SegId>>,
}

struct BreakCtx {
    breakable: bool,
    label: Option<String>,
    broken: FcId,
}

#[derive(Clone)]
pub(super) struct ChoiceCtx {
    kind: &'static str,
    forking: bool,
    t: FcId,
    f: FcId,
    n: FcId,
    processed: bool,
}

struct SwitchCtx {
    has_case: bool,
    default_segs: Option<Vec<SegId>>,
    default_body: Option<Vec<SegId>>,
    found_empty_default: bool,
    last_is_default: bool,
    fork_count: usize,
}

#[derive(PartialEq, Clone, Copy)]
enum TryPos {
    Try,
    Catch,
    Finally,
}

struct TryCtx {
    has_finalizer: bool,
    position: TryPos,
    returned: Option<FcId>,
    thrown: FcId,
    last_try_reachable: bool,
    last_catch_reachable: bool,
}

#[derive(PartialEq, Clone, Copy)]
pub(super) enum LoopType {
    While,
    DoWhile,
    For,
    ForIn,
    ForOf,
}

struct LoopCtx {
    ty: LoopType,
    label: Option<String>,
    broken: FcId,
    test: Option<bool>,
    continue_dest: Option<Vec<SegId>>,
    entry: Option<Vec<SegId>>,
    continue_fc: Option<FcId>,
    end_of_init: Option<Vec<SegId>>,
    test_segs: Option<Vec<SegId>>,
    end_of_test: Option<Vec<SegId>>,
    update_segs: Option<Vec<SegId>>,
    end_of_update: Option<Vec<SegId>>,
    prev_segs: Option<Vec<SegId>>,
    left_segs: Option<Vec<SegId>>,
    end_of_left: Option<Vec<SegId>>,
}

enum Ctx {
    State,
    Try(usize),
}

pub(super) struct State {
    pub cp: CpId,
    prefix: String,
    n: u32,
    fcs: Vec<ForkCtx>,
    fork: FcId,
    choices: Vec<ChoiceCtx>,
    switches: Vec<SwitchCtx>,
    tries: Vec<TryCtx>,
    loops: Vec<LoopCtx>,
    breaks: Vec<BreakCtx>,
    chains: Vec<u32>,
    pub current: Vec<SegId>,
    pub initial: SegId,
    pub final_segs: Vec<SegId>,
    pub returned: Vec<SegId>,
    pub thrown: Vec<SegId>,
}

fn remove_from(v: &mut Vec<SegId>, x: SegId) {
    match v.iter().position(|e| *e == x) {
        Some(i) => {
            v.remove(i);
        }
        None => {
            v.pop();
        }
    }
}

fn add_returned_or_thrown(
    dest: &mut Vec<SegId>,
    others: &[SegId],
    all: &mut Vec<SegId>,
    segs: &[SegId],
) {
    for &s in segs {
        dest.push(s);
        if !others.contains(&s) {
            all.push(s);
        }
    }
}

impl State {
    pub fn new(cp: CpId, cp_id: &str, g: &mut Graph) -> State {
        let mut st = State {
            cp,
            prefix: format!("{cp_id}_"),
            n: 0,
            fcs: vec![ForkCtx {
                upper: None,
                count: 1,
                list: Vec::new(),
            }],
            fork: 0,
            choices: Vec::new(),
            switches: Vec::new(),
            tries: Vec::new(),
            loops: Vec::new(),
            breaks: Vec::new(),
            chains: Vec::new(),
            current: Vec::new(),
            initial: 0,
            final_segs: Vec::new(),
            returned: Vec::new(),
            thrown: Vec::new(),
        };
        let id = st.next_id();
        let root = g.new_segment(id, Vec::new(), true);
        st.fcs[0].list.push(vec![root]);
        st.initial = root;
        st
    }

    fn next_id(&mut self) -> String {
        self.n += 1;
        format!("{}{}", self.prefix, self.n)
    }

    pub fn head(&self) -> Vec<SegId> {
        self.fc_head(self.fork)
    }

    pub fn reachable(&self, g: &Graph) -> bool {
        self.fc_reachable(g, self.fork)
    }

    fn fc_head(&self, fc: FcId) -> Vec<SegId> {
        self.fcs[fc].list.last().cloned().unwrap_or_default()
    }

    fn fc_empty(&self, fc: FcId) -> bool {
        self.fcs[fc].list.is_empty()
    }

    fn fc_reachable(&self, g: &Graph, fc: FcId) -> bool {
        let h = self.fcs[fc].list.last();
        h.is_some_and(|h| !h.is_empty() && h.iter().any(|s| g.reachable(*s)))
    }

    fn fc_create(
        &mut self,
        g: &mut Graph,
        fc: FcId,
        range: Option<(i64, i64)>,
        make: Make,
    ) -> Vec<SegId> {
        let len = self.fcs[fc].list.len() as i64;
        let (b, e) = match range {
            Some((s, e)) => (
                if s >= 0 { s } else { len + s },
                if e >= 0 { e } else { len + e },
            ),
            None => (0, -1),
        };
        let mut out = Vec::new();
        for i in 0..self.fcs[fc].count {
            let mut all_prev = Vec::new();
            let mut j = b;
            while j <= e {
                if let Some(s) = self.fcs[fc].list.get(j as usize).and_then(|l| l.get(i)) {
                    all_prev.push(*s);
                }
                j += 1;
            }
            let id = self.next_id();
            out.push(match make {
                Make::Next => g.new_next(id, &all_prev),
                Make::Unreachable => g.new_unreachable(id, &all_prev),
                Make::Disconnected => g.new_disconnected(id, &all_prev),
            });
        }
        out
    }

    fn make_next(&mut self, g: &mut Graph, fc: FcId, s: i64, e: i64) -> Vec<SegId> {
        self.fc_create(g, fc, Some((s, e)), Make::Next)
    }

    fn merge_extra(&mut self, g: &mut Graph, fc: FcId, segs: Vec<SegId>) -> Vec<SegId> {
        let mut cur = segs;
        while cur.len() > self.fcs[fc].count {
            let length = cur.len() / 2;
            let mut merged = Vec::new();
            for i in 0..length {
                let id = self.next_id();
                merged.push(g.new_next(id, &[cur[i], cur[i + length]]));
            }
            cur = merged;
        }
        cur
    }

    fn fc_add(&mut self, g: &mut Graph, fc: FcId, segs: Vec<SegId>) {
        let m = self.merge_extra(g, fc, segs);
        self.fcs[fc].list.push(m);
    }

    fn fc_replace_head(&mut self, g: &mut Graph, fc: FcId, segs: Vec<SegId>) {
        let m = self.merge_extra(g, fc, segs);
        let list = &mut self.fcs[fc].list;
        list.pop();
        list.push(m);
    }

    fn fc_add_all(&mut self, fc: FcId, other: FcId) {
        let l = self.fcs[other].list.clone();
        self.fcs[fc].list.extend(l);
    }

    fn fc_new_empty(&mut self, parent: FcId, fork_leaving: bool) -> FcId {
        let count = if fork_leaving { 2 } else { 1 } * self.fcs[parent].count;
        self.fcs.push(ForkCtx {
            upper: Some(parent),
            count,
            list: Vec::new(),
        });
        self.fcs.len() - 1
    }

    fn upper(&self, fc: FcId) -> FcId {
        self.fcs[fc].upper.unwrap_or(fc)
    }

    fn return_ctx(&self) -> Ctx {
        for (i, t) in self.tries.iter().enumerate().rev() {
            if t.has_finalizer && t.position != TryPos::Finally {
                return Ctx::Try(i);
            }
        }
        Ctx::State
    }

    fn throw_ctx(&self) -> Ctx {
        for (i, t) in self.tries.iter().enumerate().rev() {
            if t.position == TryPos::Try || (t.has_finalizer && t.position == TryPos::Catch) {
                return Ctx::Try(i);
            }
        }
        Ctx::State
    }

    fn add_returned(&mut self, g: &mut Graph, segs: Vec<SegId>) {
        match self.return_ctx() {
            Ctx::State => add_returned_or_thrown(
                &mut self.returned,
                &self.thrown,
                &mut self.final_segs,
                &segs,
            ),
            Ctx::Try(i) => {
                if let Some(fc) = self.tries[i].returned {
                    self.fc_add(g, fc, segs);
                }
            }
        }
    }

    fn add_thrown(&mut self, g: &mut Graph, segs: Vec<SegId>) {
        match self.throw_ctx() {
            Ctx::State => add_returned_or_thrown(
                &mut self.thrown,
                &self.returned,
                &mut self.final_segs,
                &segs,
            ),
            Ctx::Try(i) => {
                let fc = self.tries[i].thrown;
                self.fc_add(g, fc, segs);
            }
        }
    }

    fn make_looped(&mut self, x: &mut Sink, from: &[SegId], to: &[SegId]) {
        let g = &mut x.g;
        let from = g.flatten_unused(from);
        let to = g.flatten_unused(to);
        let end = from.len().min(to.len());
        for i in 0..end {
            let (f, t) = (from[i], to[i]);
            if g.reachable(t) {
                g.segments[f as usize].next.push(t);
            }
            if g.reachable(f) {
                g.segments[t as usize].prev.push(f);
            }
            g.segments[f as usize].all_next.push(t);
            g.segments[t as usize].all_prev.push(f);
            if g.seg(t).all_prev.len() >= 2 {
                g.segments[t as usize].looped_prev.push(f);
            }
            if g.reachable(f) && g.reachable(t) {
                x.out.push(Raw::Ev(EventKind::SegmentLoop(f, t, x.node)));
            }
        }
    }

    fn finalize_test_segments_of_for(&mut self, g: &mut Graph, li: usize, head: Vec<SegId>) {
        let Some(ch) = self.choices.last().cloned() else {
            return;
        };
        if !ch.processed {
            self.fc_add(g, ch.t, head.clone());
            self.fc_add(g, ch.f, head.clone());
            self.fc_add(g, ch.n, head);
        }
        if self.loops[li].test != Some(true) {
            let broken = self.loops[li].broken;
            self.fc_add_all(broken, ch.f);
        }
        let segs = self.make_next(g, ch.t, 0, -1);
        self.loops[li].end_of_test = Some(segs);
    }

    pub fn push_fork(&mut self, fork_leaving: bool) {
        self.fork = self.fc_new_empty(self.fork, fork_leaving);
    }

    pub fn pop_fork(&mut self, g: &mut Graph) {
        let last = self.fork;
        self.fork = self.upper(last);
        let segs = self.make_next(g, last, 0, -1);
        self.fc_replace_head(g, self.fork, segs);
    }

    pub fn fork_path(&mut self, g: &mut Graph) {
        let up = self.upper(self.fork);
        let segs = self.make_next(g, up, -1, -1);
        self.fc_add(g, self.fork, segs);
    }

    pub fn fork_bypass_path(&mut self, g: &mut Graph) {
        let up = self.upper(self.fork);
        let h = self.fc_head(up);
        self.fc_add(g, self.fork, h);
    }

    pub fn push_choice(&mut self, kind: &'static str, forking: bool) {
        let t = self.fc_new_empty(self.fork, false);
        let f = self.fc_new_empty(self.fork, false);
        let n = self.fc_new_empty(self.fork, false);
        self.choices.push(ChoiceCtx {
            kind,
            forking,
            t,
            f,
            n,
            processed: false,
        });
    }

    pub fn pop_choice(&mut self, g: &mut Graph) -> Option<ChoiceCtx> {
        let c = self.choices.pop()?;
        let fc = self.fork;
        let head = self.fc_head(fc);
        match c.kind {
            "&&" | "||" | "??" => {
                if !c.processed {
                    self.fc_add(g, c.t, head.clone());
                    self.fc_add(g, c.f, head.clone());
                    self.fc_add(g, c.n, head);
                }
                if c.forking {
                    let p = self.choices.last().cloned()?;
                    self.fc_add_all(p.t, c.t);
                    self.fc_add_all(p.f, c.f);
                    self.fc_add_all(p.n, c.n);
                    if let Some(pm) = self.choices.last_mut() {
                        pm.processed = true;
                    }
                    return Some(c);
                }
            }
            "test" => {
                if !c.processed {
                    self.fcs[c.t].list.clear();
                    self.fc_add(g, c.t, head);
                } else {
                    self.fcs[c.f].list.clear();
                    self.fc_add(g, c.f, head);
                }
            }
            _ => return Some(c),
        }
        self.fc_add_all(c.t, c.f);
        let segs = self.make_next(g, c.t, 0, -1);
        self.fc_replace_head(g, fc, segs);
        Some(c)
    }

    pub fn make_logical_right(&mut self, g: &mut Graph) {
        let Some(c) = self.choices.last().cloned() else {
            return;
        };
        let fc = self.fork;
        if c.processed {
            let prev = match c.kind {
                "&&" => c.t,
                "||" => c.f,
                _ => c.n,
            };
            let segs = self.make_next(g, prev, 0, -1);
            self.fc_replace_head(g, fc, segs);
            self.fcs[prev].list.clear();
            if let Some(cm) = self.choices.last_mut() {
                cm.processed = false;
            }
        } else {
            let head = self.fc_head(fc);
            match c.kind {
                "&&" => {
                    self.fc_add(g, c.f, head.clone());
                    self.fc_add(g, c.n, head);
                }
                "||" => self.fc_add(g, c.t, head),
                _ => {
                    self.fc_add(g, c.t, head.clone());
                    self.fc_add(g, c.f, head);
                }
            }
            let segs = self.make_next(g, fc, -1, -1);
            self.fc_replace_head(g, fc, segs);
        }
    }

    pub fn make_if_consequent(&mut self, g: &mut Graph) {
        let Some(c) = self.choices.last().cloned() else {
            return;
        };
        let fc = self.fork;
        if !c.processed {
            let head = self.fc_head(fc);
            self.fc_add(g, c.t, head.clone());
            self.fc_add(g, c.f, head.clone());
            self.fc_add(g, c.n, head);
        }
        if let Some(cm) = self.choices.last_mut() {
            cm.processed = false;
        }
        let segs = self.make_next(g, c.t, 0, -1);
        self.fc_replace_head(g, fc, segs);
    }

    pub fn make_if_alternate(&mut self, g: &mut Graph) {
        let Some(c) = self.choices.last().cloned() else {
            return;
        };
        let fc = self.fork;
        self.fcs[c.t].list.clear();
        let head = self.fc_head(fc);
        self.fc_add(g, c.t, head);
        if let Some(cm) = self.choices.last_mut() {
            cm.processed = true;
        }
        let segs = self.make_next(g, c.f, 0, -1);
        self.fc_replace_head(g, fc, segs);
    }

    pub fn push_chain(&mut self) {
        self.chains.push(0);
    }

    pub fn pop_chain(&mut self, g: &mut Graph) {
        let Some(c) = self.chains.pop() else { return };
        for _ in 0..c {
            self.pop_choice(g);
        }
    }

    pub fn make_optional_node(&mut self) {
        if let Some(c) = self.chains.last_mut() {
            *c += 1;
            self.push_choice("??", false);
        }
    }

    pub fn make_optional_right(&mut self, g: &mut Graph) {
        if !self.chains.is_empty() {
            self.make_logical_right(g);
        }
    }

    pub fn push_switch(&mut self, has_case: bool, label: Option<String>) {
        self.switches.push(SwitchCtx {
            has_case,
            default_segs: None,
            default_body: None,
            found_empty_default: false,
            last_is_default: false,
            fork_count: 0,
        });
        self.push_break(true, label);
    }

    pub fn pop_switch(&mut self, x: &mut Sink) {
        let Some(ctx) = self.switches.pop() else {
            return;
        };
        let fc = self.fork;
        let Some(broken) = self.pop_break(&mut x.g) else {
            return;
        };
        let g = &mut x.g;
        if ctx.fork_count == 0 {
            if !self.fc_empty(broken) {
                let s = self.make_next(g, fc, -1, -1);
                self.fc_add(g, broken, s);
                let s2 = self.make_next(g, broken, 0, -1);
                self.fc_replace_head(g, fc, s2);
            }
            return;
        }
        let last = self.fc_head(fc);
        self.fork_bypass_path(g);
        let last_case = self.fc_head(fc);
        self.fc_add(g, broken, last);
        if !ctx.last_is_default {
            if let Some(db) = &ctx.default_body {
                let ds = ctx.default_segs.clone().unwrap_or_default();
                for (i, &p) in ds.iter().enumerate() {
                    let n = db.get(i).copied().unwrap_or(0);
                    remove_from(&mut g.segments[p as usize].next, n);
                    remove_from(&mut g.segments[p as usize].all_next, n);
                    remove_from(&mut g.segments[n as usize].prev, p);
                    remove_from(&mut g.segments[n as usize].all_prev, p);
                }
                self.make_looped(x, &last_case, db);
            } else {
                self.fc_add(g, broken, last_case);
            }
        }
        for _ in 0..ctx.fork_count {
            self.fork = self.upper(self.fork);
        }
        let g = &mut x.g;
        let s = self.make_next(g, broken, 0, -1);
        self.fc_replace_head(g, self.fork, s);
    }

    pub fn make_switch_case_body(&mut self, g: &mut Graph, empty: bool, is_default: bool) {
        if !self.switches.last().is_some_and(|c| c.has_case) {
            return;
        }
        let parent = self.fork;
        self.push_fork(false);
        let fc = self.fork;
        let s = self.make_next(g, parent, 0, -1);
        self.fc_add(g, fc, s);
        let ph = self.fc_head(parent);
        let h = self.fc_head(fc);
        let Some(ctx) = self.switches.last_mut() else {
            return;
        };
        if is_default {
            ctx.default_segs = Some(ph);
            if empty {
                ctx.found_empty_default = true;
            } else {
                ctx.default_body = Some(h);
            }
        } else if !empty && ctx.found_empty_default {
            ctx.found_empty_default = false;
            ctx.default_body = Some(h);
        }
        ctx.last_is_default = is_default;
        ctx.fork_count += 1;
    }

    pub fn push_try(&mut self, has_finalizer: bool) {
        let returned = if has_finalizer {
            Some(self.fc_new_empty(self.fork, false))
        } else {
            None
        };
        let thrown = self.fc_new_empty(self.fork, false);
        self.tries.push(TryCtx {
            has_finalizer,
            position: TryPos::Try,
            returned,
            thrown,
            last_try_reachable: false,
            last_catch_reachable: false,
        });
    }

    pub fn pop_try(&mut self, g: &mut Graph) {
        let Some(ctx) = self.tries.pop() else { return };
        if ctx.position == TryPos::Catch {
            self.pop_fork(g);
            return;
        }
        let ret_empty = ctx.returned.is_none_or(|r| self.fc_empty(r));
        let thr_empty = self.fc_empty(ctx.thrown);
        if ret_empty && thr_empty {
            return;
        }
        let head = self.fc_head(self.fork);
        self.fork = self.upper(self.fork);
        let half = head.len() / 2;
        let normal = head[..half].to_vec();
        let leaving = head[half..].to_vec();
        if !ret_empty {
            self.add_returned(g, leaving.clone());
        }
        if !thr_empty {
            self.add_thrown(g, leaving);
        }
        self.fc_replace_head(g, self.fork, normal);
        if !ctx.last_try_reachable && !ctx.last_catch_reachable {
            self.fc_create(g, self.fork, None, Make::Unreachable);
        }
    }

    pub fn make_catch_block(&mut self, g: &mut Graph) {
        let fc = self.fork;
        let new_thrown = self.fc_new_empty(fc, false);
        let reachable = self.fc_reachable(g, fc);
        let Some(ctx) = self.tries.last_mut() else {
            return;
        };
        let orig = ctx.thrown;
        ctx.position = TryPos::Catch;
        ctx.thrown = new_thrown;
        ctx.last_try_reachable = reachable;
        let h = self.fc_head(fc);
        self.fc_add(g, orig, h);
        let thrown_segs = self.make_next(g, orig, 0, -1);
        self.push_fork(false);
        self.fork_bypass_path(g);
        self.fc_add(g, self.fork, thrown_segs);
    }

    pub fn make_finally_block(&mut self, g: &mut Graph) {
        let Some(ti) = self.tries.len().checked_sub(1) else {
            return;
        };
        let mut fc = self.fork;
        let ret = self.tries[ti].returned;
        let thr = self.tries[ti].thrown;
        let head_leaving = self.fc_head(fc);
        if self.tries[ti].position == TryPos::Catch {
            self.pop_fork(g);
            fc = self.fork;
            self.tries[ti].last_catch_reachable = self.fc_reachable(g, fc);
        } else {
            self.tries[ti].last_try_reachable = self.fc_reachable(g, fc);
        }
        self.tries[ti].position = TryPos::Finally;
        let ret_empty = ret.is_none_or(|r| self.fc_empty(r));
        if ret_empty && self.fc_empty(thr) {
            return;
        }
        let mut segs = self.make_next(g, fc, -1, -1);
        for i in 0..self.fcs[fc].count {
            let mut prevs: Vec<SegId> = head_leaving.get(i).copied().into_iter().collect();
            if let Some(r) = ret {
                for l in &self.fcs[r].list {
                    if let Some(s) = l.get(i) {
                        prevs.push(*s);
                    }
                }
            }
            for l in &self.fcs[thr].list {
                if let Some(s) = l.get(i) {
                    prevs.push(*s);
                }
            }
            let id = self.next_id();
            segs.push(g.new_next(id, &prevs));
        }
        self.push_fork(true);
        self.fc_add(g, self.fork, segs);
    }

    pub fn make_yield(&mut self, g: &mut Graph) {
        let fc = self.fork;
        let leaving = self.fc_head(fc);
        if self.fc_reachable(g, fc) {
            self.add_returned(g, leaving.clone());
            self.add_thrown(g, leaving);
            let s = self.make_next(g, fc, -1, -1);
            self.fc_replace_head(g, fc, s);
        }
    }

    pub fn make_first_throwable(&mut self, g: &mut Graph) {
        let fc = self.fork;
        if !self.fc_reachable(g, fc) {
            return;
        }
        let Ctx::Try(i) = self.throw_ctx() else {
            return;
        };
        let t = &self.tries[i];
        if !self.fc_empty(t.thrown) {
            return;
        }
        if t.position != TryPos::Try && (t.position != TryPos::Catch || !t.has_finalizer) {
            return;
        }
        let thrown = t.thrown;
        let h = self.fc_head(fc);
        self.fc_add(g, thrown, h);
        let s = self.make_next(g, fc, -1, -1);
        self.fc_replace_head(g, fc, s);
    }

    pub fn push_loop(&mut self, ty: LoopType, label: Option<String>) {
        let fc = self.fork;
        let broken = self.push_break(true, label.clone());
        let mut continue_fc = None;
        match ty {
            LoopType::While | LoopType::For => self.push_choice("loop", false),
            LoopType::DoWhile => {
                self.push_choice("loop", false);
                continue_fc = Some(self.fc_new_empty(fc, false));
            }
            _ => {}
        }
        self.loops.push(LoopCtx {
            ty,
            label,
            broken,
            test: None,
            continue_dest: None,
            entry: None,
            continue_fc,
            end_of_init: None,
            test_segs: None,
            end_of_test: None,
            update_segs: None,
            end_of_update: None,
            prev_segs: None,
            left_segs: None,
            end_of_left: None,
        });
    }

    pub fn pop_loop(&mut self, x: &mut Sink) {
        let Some(ctx) = self.loops.pop() else { return };
        let fc = self.fork;
        let Some(broken) = self.pop_break(&mut x.g) else {
            return;
        };
        match ctx.ty {
            LoopType::While | LoopType::For => {
                self.pop_choice(&mut x.g);
                let h = self.fc_head(fc);
                self.make_looped(x, &h, &ctx.continue_dest.unwrap_or_default());
            }
            LoopType::DoWhile => {
                let Some(ch) = self.pop_choice(&mut x.g) else {
                    return;
                };
                if !ch.processed {
                    let h = self.fc_head(fc);
                    self.fc_add(&mut x.g, ch.t, h.clone());
                    self.fc_add(&mut x.g, ch.f, h);
                }
                if ctx.test != Some(true) {
                    self.fc_add_all(broken, ch.f);
                }
                let list = self.fcs[ch.t].list.clone();
                let entry = ctx.entry.unwrap_or_default();
                for l in list {
                    self.make_looped(x, &l, &entry);
                }
            }
            LoopType::ForIn | LoopType::ForOf => {
                let h = self.fc_head(fc);
                self.fc_add(&mut x.g, broken, h.clone());
                self.make_looped(x, &h, &ctx.left_segs.unwrap_or_default());
            }
        }
        let g = &mut x.g;
        let s = if self.fc_empty(broken) {
            self.fc_create(g, fc, Some((-1, -1)), Make::Unreachable)
        } else {
            self.make_next(g, broken, 0, -1)
        };
        self.fc_replace_head(g, fc, s);
    }

    pub fn make_while_test(&mut self, g: &mut Graph, test: Option<bool>) {
        let fc = self.fork;
        let segs = self.make_next(g, fc, 0, -1);
        if let Some(l) = self.loops.last_mut() {
            l.test = test;
            l.continue_dest = Some(segs.clone());
        }
        self.fc_replace_head(g, fc, segs);
    }

    pub fn make_while_body(&mut self, g: &mut Graph) {
        let Some(ch) = self.choices.last().cloned() else {
            return;
        };
        let fc = self.fork;
        if !ch.processed {
            let h = self.fc_head(fc);
            self.fc_add(g, ch.t, h.clone());
            self.fc_add(g, ch.f, h);
        }
        if let Some(l) = self.loops.last()
            && l.test != Some(true)
        {
            let b = l.broken;
            self.fc_add_all(b, ch.f);
        }
        let s = self.make_next(g, ch.t, 0, -1);
        self.fc_replace_head(g, fc, s);
    }

    pub fn make_do_while_body(&mut self, g: &mut Graph) {
        let fc = self.fork;
        let body = self.make_next(g, fc, -1, -1);
        if let Some(l) = self.loops.last_mut() {
            l.entry = Some(body.clone());
        }
        self.fc_replace_head(g, fc, body);
    }

    pub fn make_do_while_test(&mut self, g: &mut Graph, test: Option<bool>) {
        let fc = self.fork;
        let Some(l) = self.loops.last_mut() else {
            return;
        };
        l.test = test;
        let Some(cfc) = l.continue_fc else { return };
        if !self.fc_empty(cfc) {
            let h = self.fc_head(fc);
            self.fc_add(g, cfc, h);
            let s = self.make_next(g, cfc, 0, -1);
            self.fc_replace_head(g, fc, s);
        }
    }

    pub fn make_for_test(&mut self, g: &mut Graph, test: Option<bool>) {
        let fc = self.fork;
        let end_of_init = self.fc_head(fc);
        let segs = self.make_next(g, fc, -1, -1);
        if let Some(l) = self.loops.last_mut() {
            l.test = test;
            l.end_of_init = Some(end_of_init);
            l.continue_dest = Some(segs.clone());
            l.test_segs = Some(segs.clone());
        }
        self.fc_replace_head(g, fc, segs);
    }

    pub fn make_for_update(&mut self, g: &mut Graph) {
        let fc = self.fork;
        let Some(li) = self.loops.len().checked_sub(1) else {
            return;
        };
        let h = self.fc_head(fc);
        if self.loops[li].test_segs.is_some() {
            self.finalize_test_segments_of_for(g, li, h);
        } else {
            self.loops[li].end_of_init = Some(h);
        }
        let upd = self.fc_create(g, fc, Some((-1, -1)), Make::Disconnected);
        self.loops[li].continue_dest = Some(upd.clone());
        self.loops[li].update_segs = Some(upd.clone());
        self.fc_replace_head(g, fc, upd);
    }

    pub fn make_for_body(&mut self, x: &mut Sink) {
        let fc = self.fork;
        let Some(li) = self.loops.len().checked_sub(1) else {
            return;
        };
        if self.loops[li].update_segs.is_some() {
            let h = self.fc_head(fc);
            self.loops[li].end_of_update = Some(h.clone());
            if let Some(ts) = self.loops[li].test_segs.clone() {
                self.make_looped(x, &h, &ts);
            }
        } else if self.loops[li].test_segs.is_some() {
            let h = self.fc_head(fc);
            self.finalize_test_segments_of_for(&mut x.g, li, h);
        } else {
            self.loops[li].end_of_init = Some(self.fc_head(fc));
        }
        let g = &mut x.g;
        let body = match self.loops[li].end_of_test.clone() {
            Some(b) => b,
            None => {
                let p = self.fc_new_empty(fc, false);
                let ei = self.loops[li].end_of_init.clone().unwrap_or_default();
                self.fc_add(g, p, ei);
                if let Some(eu) = self.loops[li].end_of_update.clone() {
                    self.fc_add(g, p, eu);
                }
                self.make_next(g, p, 0, -1)
            }
        };
        if self.loops[li].continue_dest.is_none() {
            self.loops[li].continue_dest = Some(body.clone());
        }
        self.fc_replace_head(g, fc, body);
    }

    pub fn make_for_in_of_left(&mut self, g: &mut Graph) {
        let fc = self.fork;
        let left = self.fc_create(g, fc, Some((-1, -1)), Make::Disconnected);
        let h = self.fc_head(fc);
        if let Some(l) = self.loops.last_mut() {
            l.prev_segs = Some(h);
            l.left_segs = Some(left.clone());
            l.continue_dest = Some(left.clone());
        }
        self.fc_replace_head(g, fc, left);
    }

    pub fn make_for_in_of_right(&mut self, g: &mut Graph) {
        let fc = self.fork;
        let Some(li) = self.loops.len().checked_sub(1) else {
            return;
        };
        let temp = self.fc_new_empty(fc, false);
        let prev = self.loops[li].prev_segs.clone().unwrap_or_default();
        self.fc_add(g, temp, prev);
        let right = self.make_next(g, temp, -1, -1);
        self.loops[li].end_of_left = Some(self.fc_head(fc));
        self.fc_replace_head(g, fc, right);
    }

    pub fn make_for_in_of_body(&mut self, x: &mut Sink) {
        let fc = self.fork;
        let Some(li) = self.loops.len().checked_sub(1) else {
            return;
        };
        let temp = self.fc_new_empty(fc, false);
        let eol = self.loops[li].end_of_left.clone().unwrap_or_default();
        self.fc_add(&mut x.g, temp, eol);
        let body = self.make_next(&mut x.g, temp, -1, -1);
        let h = self.fc_head(fc);
        let left = self.loops[li].left_segs.clone().unwrap_or_default();
        self.make_looped(x, &h, &left);
        let broken = self.loops[li].broken;
        self.fc_add(&mut x.g, broken, h);
        self.fc_replace_head(&mut x.g, fc, body);
    }

    pub fn push_break(&mut self, breakable: bool, label: Option<String>) -> FcId {
        let broken = self.fc_new_empty(self.fork, false);
        self.breaks.push(BreakCtx {
            breakable,
            label,
            broken,
        });
        broken
    }

    pub fn pop_break(&mut self, g: &mut Graph) -> Option<FcId> {
        let ctx = self.breaks.pop()?;
        let fc = self.fork;
        if !ctx.breakable && !self.fc_empty(ctx.broken) {
            let h = self.fc_head(fc);
            self.fc_add(g, ctx.broken, h);
            let s = self.make_next(g, ctx.broken, 0, -1);
            self.fc_replace_head(g, fc, s);
        }
        Some(ctx.broken)
    }

    pub fn make_break(&mut self, g: &mut Graph, label: Option<&str>) {
        let fc = self.fork;
        if !self.fc_reachable(g, fc) {
            return;
        }
        let ctx = self.breaks.iter().rev().find(|c| match label {
            Some(l) => c.label.as_deref() == Some(l),
            None => c.breakable,
        });
        if let Some(b) = ctx.map(|c| c.broken) {
            let h = self.fc_head(fc);
            self.fc_add(g, b, h);
        }
        let s = self.fc_create(g, fc, Some((-1, -1)), Make::Unreachable);
        self.fc_replace_head(g, fc, s);
    }

    pub fn make_continue(&mut self, x: &mut Sink, label: Option<&str>) {
        let fc = self.fork;
        if !self.fc_reachable(&x.g, fc) {
            return;
        }
        let li = match label {
            None => self.loops.len().checked_sub(1),
            Some(l) => self
                .loops
                .iter()
                .rposition(|c| c.label.as_deref() == Some(l)),
        };
        if let Some(li) = li {
            let h = self.fc_head(fc);
            if let Some(cd) = self.loops[li].continue_dest.clone() {
                self.make_looped(x, &h, &cd);
                if matches!(self.loops[li].ty, LoopType::ForIn | LoopType::ForOf) {
                    let b = self.loops[li].broken;
                    self.fc_add(&mut x.g, b, h);
                }
            } else if let Some(cfc) = self.loops[li].continue_fc {
                self.fc_add(&mut x.g, cfc, h);
            }
        }
        let s = self.fc_create(&mut x.g, fc, Some((-1, -1)), Make::Unreachable);
        self.fc_replace_head(&mut x.g, fc, s);
    }

    pub fn make_return(&mut self, g: &mut Graph) {
        let fc = self.fork;
        if self.fc_reachable(g, fc) {
            let h = self.fc_head(fc);
            self.add_returned(g, h);
            let s = self.fc_create(g, fc, Some((-1, -1)), Make::Unreachable);
            self.fc_replace_head(g, fc, s);
        }
    }

    pub fn make_throw(&mut self, g: &mut Graph) {
        let fc = self.fork;
        if self.fc_reachable(g, fc) {
            let h = self.fc_head(fc);
            self.add_thrown(g, h);
            let s = self.fc_create(g, fc, Some((-1, -1)), Make::Unreachable);
            self.fc_replace_head(g, fc, s);
        }
    }

    pub fn make_final(&mut self, g: &Graph) {
        let segs = self.current.clone();
        if !segs.is_empty() && g.reachable(segs[0]) {
            add_returned_or_thrown(
                &mut self.returned,
                &self.thrown,
                &mut self.final_segs,
                &segs,
            );
        }
    }
}
