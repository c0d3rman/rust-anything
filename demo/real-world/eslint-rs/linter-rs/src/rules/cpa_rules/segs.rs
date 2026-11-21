//! lib/rules/utils/code-path-utils.js and the `Set<CodePathSegment>` bookkeeping the rules share.

use crate::code_path::{Event, Graph, SegId};
use std::rc::Rc;

pub fn add(v: &mut Vec<SegId>, s: SegId) {
    if !v.contains(&s) {
        v.push(s);
    }
}

pub fn delete(v: &mut Vec<SegId>, s: SegId) {
    v.retain(|x| *x != s);
}

/// `isAnySegmentReachable(segments)`.
pub fn any_reachable(g: Option<&Rc<Graph>>, v: &[SegId]) -> bool {
    g.is_some_and(|g| v.iter().any(|s| g.reachable(*s)))
}

/// Keeps the code path graph of the latest event.
pub fn keep(slot: &mut Option<Rc<Graph>>, ev: &Event) {
    if slot.is_none() {
        *slot = Some(Rc::clone(&ev.graph));
    }
}
