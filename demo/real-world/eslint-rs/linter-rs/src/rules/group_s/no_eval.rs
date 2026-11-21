//! lib/rules/no-eval.js

use crate::ast::NodeId;
use crate::linter::Cx;
use crate::rule::Report;
use crate::rules::{Rule, RuleDef};
use crate::source::SourceType;
use crate::utils::{is_callee, is_default_this_binding, is_specific_id, is_specific_member_access};
use serde_json::Value;

pub static DEF: RuleDef = RuleDef {
    name: "no-eval",
    messages: &[("unexpected", "`eval` can be harmful.")],
    has_default_options: true,
    create,
};

struct Info {
    node: NodeId,
    strict: bool,
    top_level_of_script: bool,
    default_this: bool,
    initialized: bool,
}

struct R {
    allow_indirect: bool,
    stack: Vec<Info>,
}

fn create(options: &[Value]) -> Result<Box<dyn Rule>, String> {
    let o = super::common::bool_object(options, &["allowIndirect"])?;
    Ok(Box::new(R {
        allow_indirect: o[0].unwrap_or(false),
        stack: Vec::new(),
    }))
}

const FULL: &[&str] = &[
    "CallExpression:exit",
    "Program",
    "Program:exit",
    "FunctionDeclaration",
    "FunctionDeclaration:exit",
    "FunctionExpression",
    "FunctionExpression:exit",
    "PropertyDefinition > *.value",
    "PropertyDefinition > *.value:exit",
    "StaticBlock",
    "StaticBlock:exit",
    "ThisExpression",
];

fn is_member(cx: &Cx, node: NodeId, name: &str) -> bool {
    is_specific_member_access(&cx.sc.ast, node, None, Some(name))
}

fn report(cx: &mut Cx, node: NodeId) {
    let ast = &cx.sc.ast;
    let Some(parent) = ast.parent(node) else {
        return;
    };
    let location = if ast.is(node, "MemberExpression") {
        ast.child(node, "property").unwrap_or(node)
    } else {
        node
    };
    let report_node =
        if ast.is(parent, "CallExpression") && ast.child(parent, "callee") == Some(node) {
            parent
        } else {
            node
        };
    let loc = cx.sc.node_loc(location);
    let mut r = Report::loc(loc, "unexpected");
    r.node = Some(report_node);
    cx.report(r);
}

impl R {
    fn enter(&mut self, cx: &Cx, node: NodeId) {
        let strict = cx.sm().is_strict(cx.get_scope(node));
        self.stack.push(Info {
            node,
            strict,
            top_level_of_script: false,
            default_this: false,
            initialized: strict,
        });
    }

    fn program(&mut self, cx: &Cx, node: NodeId) {
        let sm = cx.sm();
        let scope = cx.get_scope(node);
        let module = cx.sc.source_type == SourceType::Module;
        let child_strict = sm
            .scope(scope)
            .child_scopes
            .first()
            .is_some_and(|c| sm.is_strict(*c));
        let gr = cx.sc.global_return;
        let strict = sm.is_strict(scope) || module || (gr && child_strict);
        self.stack.push(Info {
            node,
            strict,
            top_level_of_script: !module && !gr,
            default_this: true,
            initialized: true,
        });
    }

    fn this_expression(&mut self, cx: &mut Cx, node: NodeId) {
        let Some(parent) = cx.sc.ast.parent(node) else {
            return;
        };
        if !is_member(cx, parent, "eval") {
            return;
        }
        let Some(info) = self.stack.last_mut() else {
            return;
        };
        if !info.initialized {
            info.initialized = true;
            info.default_this = is_default_this_binding(cx.sc, info.node, true);
        }
        if info.top_level_of_script || (!info.strict && info.default_this) {
            report(cx, parent);
        }
    }

    fn report_accessing_eval(&self, cx: &mut Cx, global: crate::scope::ScopeId) {
        let sm = cx.sm();
        let Some(v) = sm.variable_by_name(global, "eval") else {
            return;
        };
        for &r in &sm.var(v).references {
            let id = sm.reference(r).identifier;
            if cx.sc.ast.name(id) == Some("eval") && !is_callee(&cx.sc.ast, id) {
                report(cx, id);
            }
        }
    }

    fn report_via_global_object(&self, cx: &mut Cx, global: crate::scope::ScopeId) {
        let sm = cx.sm();
        for name in ["global", "window", "globalThis"] {
            let Some(v) = sm.variable_by_name(global, name) else {
                continue;
            };
            for &r in &sm.var(v).references {
                let id = sm.reference(r).identifier;
                let Some(mut node) = cx.sc.ast.parent(id) else {
                    continue;
                };
                while is_member(cx, node, name) {
                    match cx.sc.ast.parent(node) {
                        Some(p) => node = p,
                        None => break,
                    }
                }
                if is_member(cx, node, "eval") {
                    report(cx, node);
                }
            }
        }
    }
}

fn is_property_value(cx: &Cx, node: NodeId) -> bool {
    let ast = &cx.sc.ast;
    ast.parent(node)
        .is_some_and(|p| ast.is(p, "PropertyDefinition") && ast.child(p, "value") == Some(node))
}

impl Rule for R {
    fn listeners(&self) -> &'static [&'static str] {
        if self.allow_indirect {
            &["CallExpression:exit"]
        } else {
            FULL
        }
    }

    fn on(&mut self, li: usize, node: NodeId, cx: &mut Cx) {
        let sel = self.listeners()[li];
        match sel {
            "CallExpression:exit" => {
                let ast = &cx.sc.ast;
                let Some(callee) = ast.child(node, "callee") else {
                    return;
                };
                if self.allow_indirect && ast.bool(node, "optional") {
                    return;
                }
                if is_specific_id(ast, callee, "eval") {
                    report(cx, callee);
                }
            }
            "Program" => self.program(cx, node),
            "Program:exit" => {
                let global = cx.get_scope(node);
                self.stack.pop();
                self.report_accessing_eval(cx, global);
                self.report_via_global_object(cx, global);
            }
            "FunctionDeclaration" | "FunctionExpression" | "StaticBlock" => self.enter(cx, node),
            "PropertyDefinition > *.value" => {
                if is_property_value(cx, node) {
                    self.enter(cx, node);
                }
            }
            "PropertyDefinition > *.value:exit" => {
                if is_property_value(cx, node) {
                    self.stack.pop();
                }
            }
            "ThisExpression" => self.this_expression(cx, node),
            _ => {
                self.stack.pop();
            }
        }
    }
}
