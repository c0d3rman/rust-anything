//! lib/rules/no-constant-condition.js

use crate::ast::{NodeId, Prop};
use crate::linter::Cx;
use crate::rule::Report;
use crate::rules::{Rule, RuleDef};
use crate::utils::is_constant;
use serde_json::Value;

pub static DEF: RuleDef = RuleDef {
    name: "no-constant-condition",
    messages: &[("unexpected", "Unexpected constant condition.")],
    has_default_options: true,
    create,
};

#[derive(PartialEq)]
enum Loops {
    All,
    AllExceptWhileTrue,
    None,
}

struct R {
    check_loops: Loops,
    stack: Vec<Vec<NodeId>>,
    current: Vec<NodeId>,
}

fn create(options: &[Value]) -> Result<Box<dyn Rule>, String> {
    let bad = || Err("invalid option".to_string());
    let mut check_loops = Loops::AllExceptWhileTrue;
    match options {
        [] => {}
        [Value::Object(o)] => {
            for (k, v) in o {
                if k != "checkLoops" {
                    return bad();
                }
                check_loops = match v {
                    Value::String(s) if s == "all" => Loops::All,
                    Value::String(s) if s == "allExceptWhileTrue" => Loops::AllExceptWhileTrue,
                    Value::String(s) if s == "none" => Loops::None,
                    Value::Bool(true) => Loops::All,
                    Value::Bool(false) => Loops::None,
                    _ => return bad(),
                };
            }
        }
        _ => return bad(),
    }
    Ok(Box::new(R {
        check_loops,
        stack: Vec::new(),
        current: Vec::new(),
    }))
}

fn constant_test(cx: &Cx, node: NodeId) -> Option<NodeId> {
    let test = cx.sc.ast.child(node, "test")?;
    is_constant(&cx.sc.ast, cx.sm(), cx.get_scope(node), Some(test), true).then_some(test)
}

impl R {
    fn check_loop(&mut self, cx: &Cx, node: NodeId) {
        if self.check_loops != Loops::None
            && constant_test(cx, node).is_some()
            && !self.current.contains(&node)
        {
            self.current.push(node);
        }
    }

    fn exit_loop(&mut self, cx: &mut Cx, node: NodeId) {
        if let Some(i) = self.current.iter().position(|n| *n == node) {
            self.current.remove(i);
            if let Some(test) = cx.sc.ast.child(node, "test") {
                cx.report(Report::node(test, "unexpected"));
            }
        }
    }
}

impl Rule for R {
    fn listeners(&self) -> &'static [&'static str] {
        &[
            "ConditionalExpression",
            "IfStatement",
            "WhileStatement",
            "WhileStatement:exit",
            "DoWhileStatement",
            "DoWhileStatement:exit",
            "ForStatement",
            "ForStatement > .test",
            "ForStatement:exit",
            "FunctionDeclaration",
            "FunctionDeclaration:exit",
            "FunctionExpression",
            "FunctionExpression:exit",
            "YieldExpression",
        ]
    }

    fn on(&mut self, li: usize, node: NodeId, cx: &mut Cx) {
        let ast = &cx.sc.ast;
        match li {
            0 | 1 => {
                if let Some(test) = constant_test(cx, node) {
                    cx.report(Report::node(test, "unexpected"));
                }
            }
            2 => {
                let while_true = ast.child(node, "test").is_some_and(|t| {
                    ast.is(t, "Literal") && matches!(ast.prop(t, "value"), Some(Prop::Bool(true)))
                });
                if while_true && self.check_loops == Loops::AllExceptWhileTrue {
                    return;
                }
                self.check_loop(cx, node);
            }
            4 | 6 => self.check_loop(cx, node),
            7 => {
                if let Some(p) = ast.parent(node)
                    && ast.is(p, "ForStatement")
                    && ast.child(p, "test") == Some(node)
                {
                    self.check_loop(cx, p);
                }
            }
            3 | 5 | 8 => self.exit_loop(cx, node),
            9 | 11 => self.stack.push(std::mem::take(&mut self.current)),
            10 | 12 => self.current = self.stack.pop().unwrap_or_default(),
            _ => self.current.clear(),
        }
    }
}
