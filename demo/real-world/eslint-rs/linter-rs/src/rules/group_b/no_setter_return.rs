//! lib/rules/no-setter-return.js

use crate::ast::NodeId;
use crate::linter::Cx;
use crate::rule::Report;
use crate::rules::{Rule, RuleDef};
use crate::utils::{get_static_property_name, is_specific_member_access, skip_chain_expression};
use serde_json::Value;

pub static DEF: RuleDef = RuleDef {
    name: "no-setter-return",
    messages: &[("returnsValue", "Setter cannot return a value.")],
    has_default_options: false,
    create,
};

struct R {
    stack: Vec<bool>,
    unsupported: Option<&'static str>,
}

fn create(options: &[Value]) -> Result<Box<dyn Rule>, String> {
    super::no_options(options)?;
    Ok(Box::new(R {
        stack: Vec::new(),
        unsupported: None,
    }))
}

fn is_argument_of_global_method_call(
    cx: &Cx,
    node: NodeId,
    object: &str,
    method: &str,
    index: usize,
) -> Option<bool> {
    let ast = &cx.sc.ast;
    let Some(call) = ast.parent(node) else {
        return Some(false);
    };
    if !ast.is(call, "CallExpression")
        || ast.list(call, "arguments").get(index).copied().flatten() != Some(node)
    {
        return Some(false);
    }
    let Some(callee) = ast.child(call, "callee") else {
        return Some(false);
    };
    if !is_specific_member_access(ast, callee, Some(object), Some(method)) {
        return Some(false);
    }
    let obj = ast.child(skip_chain_expression(ast, callee), "object")?;
    super::builtin_global_reference(cx, obj)
}

fn is_property_descriptor(cx: &Cx, node: NodeId) -> Option<bool> {
    let ast = &cx.sc.ast;
    if is_argument_of_global_method_call(cx, node, "Object", "defineProperty", 2)?
        || is_argument_of_global_method_call(cx, node, "Reflect", "defineProperty", 2)?
    {
        return Some(true);
    }
    let Some(parent) = ast.parent(node) else {
        return Some(false);
    };
    if ast.is(parent, "Property") && ast.child(parent, "value") == Some(node) {
        let Some(grand) = ast.parent(parent) else {
            return Some(false);
        };
        if ast.is(grand, "ObjectExpression")
            && (is_argument_of_global_method_call(cx, grand, "Object", "create", 1)?
                || is_argument_of_global_method_call(cx, grand, "Object", "defineProperties", 1)?)
        {
            return Some(true);
        }
    }
    Some(false)
}

fn is_setter(cx: &Cx, node: NodeId) -> Option<bool> {
    let ast = &cx.sc.ast;
    let Some(parent) = ast.parent(node) else {
        return Some(false);
    };
    let is_value = ast.child(parent, "value") == Some(node);
    if matches!(ast.kind(parent), "Property" | "MethodDefinition")
        && ast.str(parent, "kind") == Some("set")
        && is_value
    {
        return Some(true);
    }
    if ast.is(parent, "Property")
        && is_value
        && get_static_property_name(ast, parent).as_deref() == Some("set")
        && let Some(grand) = ast.parent(parent)
        && ast.is(grand, "ObjectExpression")
    {
        return is_property_descriptor(cx, grand);
    }
    Some(false)
}

impl Rule for R {
    fn listeners(&self) -> &'static [&'static str] {
        &[
            "FunctionDeclaration",
            "FunctionExpression",
            "ArrowFunctionExpression",
            "FunctionDeclaration:exit",
            "FunctionExpression:exit",
            "ArrowFunctionExpression:exit",
            "ReturnStatement",
        ]
    }

    fn on(&mut self, l: usize, node: NodeId, cx: &mut Cx) {
        let sc = cx.sc;
        let ast = &sc.ast;
        match l {
            0..=2 => {
                let Some(setter) = is_setter(cx, node) else {
                    self.unsupported = Some("isGlobalReference needs scope analysis");
                    return;
                };
                self.stack.push(setter);
                if l == 2
                    && setter
                    && ast.bool(node, "expression")
                    && let Some(body) = ast.child(node, "body")
                {
                    cx.report(Report::node(body, "returnsValue"));
                }
            }
            3..=5 => {
                self.stack.pop();
            }
            _ => {
                if self.stack.last() == Some(&true) && ast.child(node, "argument").is_some() {
                    cx.report(Report::node(node, "returnsValue"));
                }
            }
        }
    }

    fn unsupported(&self) -> Option<&str> {
        self.unsupported
    }
}
