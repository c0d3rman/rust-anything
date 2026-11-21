//! Ported core rules. Each module ports `lib/rules/<name>.js` of ESLint 10.15.0.

use crate::ast::NodeId;
use crate::linter::Cx;
use serde_json::Value;

pub trait Rule {
    /// The keys of the object `create()` returns, in order.
    fn listeners(&self) -> &'static [&'static str];
    fn on(&mut self, listener: usize, node: NodeId, cx: &mut Cx);
    /// Code path events (`onCodePathStart`, ...), when `listeners()` names them.
    fn on_code_path(&mut self, _listener: usize, _event: &crate::code_path::Event, _cx: &mut Cx) {}
    /// Set when the rule met input it does not port; the whole case then goes to ESLint.
    fn unsupported(&self) -> Option<&str> {
        None
    }
}

pub type CreateFn = fn(&[Value]) -> Result<Box<dyn Rule>, String>;

pub struct RuleDef {
    pub name: &'static str,
    pub messages: &'static [(&'static str, &'static str)],
    pub has_default_options: bool,
    pub create: CreateFn,
}

pub mod core;
pub mod cpa_rules;
pub mod group_a;
pub mod group_b;
pub mod group_re;
pub mod group_s;
pub mod scope_rules;

pub static GROUPS: &[&[&RuleDef]] = &[
    core::ALL,
    scope_rules::ALL,
    cpa_rules::ALL,
    group_a::ALL,
    group_b::ALL,
    group_re::ALL,
    group_s::ALL,
];

pub fn find(name: &str) -> Option<&'static RuleDef> {
    GROUPS
        .iter()
        .flat_map(|g| g.iter())
        .copied()
        .find(|d| d.name == name)
}
