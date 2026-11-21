//! lib/rules/no-constant-binary-expression.js

use crate::ast::{Ast, NodeId, Prop};
use crate::linter::Cx;
use crate::rule::Report;
use crate::rules::{Rule, RuleDef};
use crate::scope::{ScopeId, ScopeManager};
use crate::utils::{
    is_constant, is_logical_assignment_operator, is_null_literal, is_reference_to_global_variable,
};
use serde_json::Value;

pub static DEF: RuleDef = RuleDef {
    name: "no-constant-binary-expression",
    messages: &[
        (
            "constantBinaryOperand",
            "Unexpected constant binary expression. Compares constantly with the {{otherSide}}-hand side of the `{{operator}}`.",
        ),
        (
            "constantShortCircuit",
            "Unexpected constant {{property}} on the left-hand side of a `{{operator}}` expression.",
        ),
        (
            "alwaysNew",
            "Unexpected comparison to newly constructed object. These two values can never be equal.",
        ),
        (
            "bothAlwaysNew",
            "Unexpected comparison of two newly constructed objects. These two values can never be equal.",
        ),
        (
            "constantRelationalComparison",
            "Unexpected constant relational comparison. Both sides of the `{{operator}}` are literal values.",
        ),
    ],
    has_default_options: true,
    create,
};

const ECMASCRIPT_GLOBALS: &[&str] = &[
    "Array",
    "Boolean",
    "constructor",
    "Date",
    "decodeURI",
    "decodeURIComponent",
    "encodeURI",
    "encodeURIComponent",
    "Error",
    "escape",
    "eval",
    "EvalError",
    "Function",
    "hasOwnProperty",
    "Infinity",
    "isFinite",
    "isNaN",
    "isPrototypeOf",
    "Math",
    "NaN",
    "Number",
    "Object",
    "parseFloat",
    "parseInt",
    "propertyIsEnumerable",
    "RangeError",
    "ReferenceError",
    "RegExp",
    "String",
    "SyntaxError",
    "toLocaleString",
    "toString",
    "TypeError",
    "undefined",
    "unescape",
    "URIError",
    "valueOf",
    "JSON",
    "ArrayBuffer",
    "DataView",
    "Float32Array",
    "Float64Array",
    "Int16Array",
    "Int32Array",
    "Int8Array",
    "Intl",
    "Map",
    "Promise",
    "Proxy",
    "Reflect",
    "Set",
    "Symbol",
    "Uint16Array",
    "Uint32Array",
    "Uint8Array",
    "Uint8ClampedArray",
    "WeakMap",
    "WeakSet",
    "Atomics",
    "SharedArrayBuffer",
    "BigInt",
    "BigInt64Array",
    "BigUint64Array",
    "globalThis",
    "AggregateError",
    "FinalizationRegistry",
    "WeakRef",
    "Float16Array",
    "Iterator",
    "AsyncDisposableStack",
    "DisposableStack",
    "SuppressedError",
    "Temporal",
];

struct R {
    check_relational: bool,
}

fn create(options: &[Value]) -> Result<Box<dyn Rule>, String> {
    let o = super::common::bool_object(options, &["checkRelationalComparisons"])?;
    Ok(Box::new(R {
        check_relational: o[0].unwrap_or(false),
    }))
}

struct Ctx<'a> {
    ast: &'a Ast,
    sm: &'a ScopeManager,
    scope: ScopeId,
}

impl Ctx<'_> {
    fn global(&self, n: NodeId) -> bool {
        is_reference_to_global_variable(self.sm, self.scope, n)
    }

    fn constant(&self, n: Option<NodeId>, b: bool) -> bool {
        is_constant(self.ast, self.sm, self.scope, n, b)
    }

    fn op(&self, n: NodeId) -> &str {
        self.ast.str(n, "operator").unwrap_or("")
    }

    fn last(&self, n: NodeId) -> Option<NodeId> {
        self.ast.list(n, "expressions").last().copied().flatten()
    }

    fn callee_name(&self, n: NodeId) -> Option<&str> {
        let c = self.ast.child(n, "callee")?;
        if self.ast.is(c, "Identifier") {
            self.ast.name(c)
        } else {
            None
        }
    }

    fn callee_global(&self, n: NodeId) -> bool {
        self.ast.child(n, "callee").is_some_and(|c| self.global(c))
    }

    fn first_arg_constant(&self, n: NodeId) -> bool {
        let args = self.ast.list(n, "arguments");
        args.is_empty() || self.constant(args[0], true)
    }

    fn is_undefined_global(&self, n: NodeId) -> bool {
        self.ast.is(n, "Identifier") && self.ast.name(n) == Some("undefined") && self.global(n)
    }

    fn null_or_undefined(&self, n: NodeId) -> bool {
        is_null_literal(self.ast, n)
            || self.is_undefined_global(n)
            || (self.ast.is(n, "UnaryExpression") && self.op(n) == "void")
    }

    fn nullishness(&self, n: Option<NodeId>, non_nullish: bool) -> bool {
        let Some(n) = n else { return false };
        if non_nullish && self.null_or_undefined(n) {
            return false;
        }
        let ast = self.ast;
        match ast.kind(n) {
            "ObjectExpression"
            | "ArrayExpression"
            | "ArrowFunctionExpression"
            | "FunctionExpression"
            | "ClassExpression"
            | "NewExpression"
            | "Literal"
            | "TemplateLiteral"
            | "UpdateExpression"
            | "BinaryExpression" => true,
            "CallExpression" => {
                matches!(
                    self.callee_name(n),
                    Some("Boolean" | "String" | "Number" | "Symbol" | "BigInt")
                ) && self.callee_global(n)
            }
            "LogicalExpression" => {
                self.op(n) == "??" && self.nullishness(ast.child(n, "right"), true)
            }
            "AssignmentExpression" => {
                if self.op(n) == "=" {
                    return self.nullishness(ast.child(n, "right"), non_nullish);
                }
                !is_logical_assignment_operator(self.op(n))
            }
            "UnaryExpression" => true,
            "SequenceExpression" => self.nullishness(self.last(n), non_nullish),
            "Identifier" => self.is_undefined_global(n),
            _ => false,
        }
    }

    fn static_boolean(&self, n: NodeId) -> bool {
        let ast = self.ast;
        match ast.kind(n) {
            "Literal" => {
                ast.prop(n, "regex").is_none()
                    && matches!(ast.prop(n, "value"), Some(Prop::Bool(_)))
            }
            "CallExpression" => {
                self.callee_name(n) == Some("Boolean")
                    && self.callee_global(n)
                    && self.first_arg_constant(n)
            }
            "UnaryExpression" => self.op(n) == "!" && self.constant(ast.child(n, "argument"), true),
            _ => false,
        }
    }

    fn loose_boolean(&self, n: Option<NodeId>) -> bool {
        let Some(n) = n else { return false };
        let ast = self.ast;
        match ast.kind(n) {
            "ObjectExpression"
            | "ClassExpression"
            | "ArrowFunctionExpression"
            | "FunctionExpression"
            | "Literal" => true,
            "ArrayExpression" => {
                let els = ast.list(n, "elements");
                let non_spread = els
                    .iter()
                    .filter(|e| e.is_some_and(|e| !ast.is(e, "SpreadElement")))
                    .count();
                els.is_empty() || non_spread > 1
            }
            "UnaryExpression" => match self.op(n) {
                "void" | "typeof" => true,
                "!" => self.constant(ast.child(n, "argument"), true),
                _ => false,
            },
            "CallExpression" => {
                self.callee_name(n) == Some("Boolean")
                    && self.callee_global(n)
                    && self.first_arg_constant(n)
            }
            "Identifier" => self.is_undefined_global(n),
            "TemplateLiteral" => ast.list(n, "expressions").is_empty(),
            "AssignmentExpression" => {
                self.op(n) == "=" && self.loose_boolean(ast.child(n, "right"))
            }
            "SequenceExpression" => self.loose_boolean(self.last(n)),
            _ => false,
        }
    }

    fn strict_boolean(&self, n: Option<NodeId>) -> bool {
        let Some(n) = n else { return false };
        let ast = self.ast;
        match ast.kind(n) {
            "ObjectExpression"
            | "ArrayExpression"
            | "ArrowFunctionExpression"
            | "FunctionExpression"
            | "ClassExpression"
            | "NewExpression"
            | "TemplateLiteral"
            | "Literal"
            | "UpdateExpression" => true,
            "BinaryExpression" => matches!(
                self.op(n),
                "+" | "-" | "*" | "/" | "%" | "|" | "^" | "&" | "**" | "<<" | ">>" | ">>>"
            ),
            "UnaryExpression" => match self.op(n) {
                "delete" => false,
                "!" => self.constant(ast.child(n, "argument"), true),
                _ => true,
            },
            "SequenceExpression" => self.strict_boolean(self.last(n)),
            "Identifier" => self.is_undefined_global(n),
            "AssignmentExpression" => {
                if self.op(n) == "=" {
                    return self.strict_boolean(ast.child(n, "right"));
                }
                !is_logical_assignment_operator(self.op(n))
            }
            "CallExpression" => match self.callee_name(n) {
                Some("String" | "Number" | "BigInt" | "Symbol") if self.callee_global(n) => true,
                Some("Boolean") if self.callee_global(n) => self.first_arg_constant(n),
                _ => false,
            },
            _ => false,
        }
    }

    fn always_new(&self, n: Option<NodeId>) -> bool {
        let Some(n) = n else { return false };
        let ast = self.ast;
        match ast.kind(n) {
            "ObjectExpression"
            | "ArrayExpression"
            | "ArrowFunctionExpression"
            | "FunctionExpression"
            | "ClassExpression" => true,
            "NewExpression" => {
                let Some(c) = ast.child(n, "callee") else {
                    return false;
                };
                ast.is(c, "Identifier")
                    && ast.name(c).is_some_and(|x| ECMASCRIPT_GLOBALS.contains(&x))
                    && self.global(c)
            }
            "Literal" => ast.prop(n, "regex").is_some(),
            "SequenceExpression" => self.always_new(self.last(n)),
            "AssignmentExpression" => self.op(n) == "=" && self.always_new(ast.child(n, "right")),
            "ConditionalExpression" => {
                self.always_new(ast.child(n, "consequent"))
                    && self.always_new(ast.child(n, "alternate"))
            }
            _ => false,
        }
    }

    fn constant_operand(&self, a: NodeId, b: NodeId, op: &str) -> Option<NodeId> {
        if op == "==" || op == "!=" {
            if (self.null_or_undefined(a) && self.nullishness(Some(b), false))
                || (self.static_boolean(a) && self.loose_boolean(Some(b)))
            {
                return Some(b);
            }
        } else if (op == "===" || op == "!==")
            && ((self.null_or_undefined(a) && self.nullishness(Some(b), false))
                || (self.static_boolean(a) && self.strict_boolean(Some(b))))
        {
            return Some(b);
        }
        None
    }

    fn static_literal(&self, n: NodeId) -> bool {
        let ast = self.ast;
        match ast.kind(n) {
            "Literal" => true,
            "UnaryExpression" => {
                matches!(self.op(n), "-" | "+" | "~")
                    && ast
                        .child(n, "argument")
                        .is_some_and(|a| ast.is(a, "Literal"))
            }
            "Identifier" => self.is_undefined_global(n),
            "TemplateLiteral" => ast.list(n, "expressions").is_empty(),
            _ => false,
        }
    }
}

impl Rule for R {
    fn listeners(&self) -> &'static [&'static str] {
        &["LogicalExpression", "BinaryExpression"]
    }

    fn on(&mut self, li: usize, node: NodeId, cx: &mut Cx) {
        let c = Ctx {
            ast: &cx.sc.ast,
            sm: cx.sm(),
            scope: cx.get_scope(node),
        };
        let op = c.op(node).to_string();
        let (Some(left), Some(right)) = (c.ast.child(node, "left"), c.ast.child(node, "right"))
        else {
            return;
        };
        if li == 0 {
            if (op == "&&" || op == "||") && c.constant(Some(left), true) {
                cx.report(
                    Report::node(left, "constantShortCircuit")
                        .data("property", "truthiness")
                        .data("operator", op),
                );
            } else if op == "??" && c.nullishness(Some(left), false) {
                cx.report(
                    Report::node(left, "constantShortCircuit")
                        .data("property", "nullishness")
                        .data("operator", op),
                );
            }
            return;
        }
        let right_const = c.constant_operand(left, right, &op);
        let left_const = c.constant_operand(right, left, &op);
        if let Some(r) = right_const {
            cx.report(
                Report::node(r, "constantBinaryOperand")
                    .data("operator", op)
                    .data("otherSide", "left"),
            );
        } else if let Some(l) = left_const {
            cx.report(
                Report::node(l, "constantBinaryOperand")
                    .data("operator", op)
                    .data("otherSide", "right"),
            );
        } else if op == "===" || op == "!==" {
            if c.always_new(Some(left)) {
                cx.report(Report::node(left, "alwaysNew"));
            } else if c.always_new(Some(right)) {
                cx.report(Report::node(right, "alwaysNew"));
            }
        } else if op == "==" || op == "!=" {
            if c.always_new(Some(left)) && c.always_new(Some(right)) {
                cx.report(Report::node(left, "bothAlwaysNew"));
            }
        } else if self.check_relational
            && matches!(op.as_str(), "<" | "<=" | ">" | ">=")
            && c.static_literal(left)
            && c.static_literal(right)
        {
            cx.report(Report::node(node, "constantRelationalComparison").data("operator", op));
        }
    }
}
