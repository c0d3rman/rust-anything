//! The parts of lib/rules/utils/ast-utils.js the ported rules use.

use crate::ast::{Ast, NodeId, Prop};
use crate::source::{TokKind, Token};

pub fn is_function(ast: &Ast, n: NodeId) -> bool {
    matches!(
        ast.kind(n),
        "FunctionDeclaration" | "FunctionExpression" | "ArrowFunctionExpression"
    )
}

/// `isSpecificId(node, name)`.
pub fn is_specific_id(ast: &Ast, n: NodeId, name: &str) -> bool {
    ast.is(n, "Identifier") && ast.name(n) == Some(name)
}

/// `isCallee(node)`.
pub fn is_callee(ast: &Ast, n: NodeId) -> bool {
    ast.parent(n)
        .is_some_and(|p| ast.is(p, "CallExpression") && ast.child(p, "callee") == Some(n))
}

/// `couldBeError(node)`.
pub fn could_be_error(ast: &Ast, n: NodeId) -> bool {
    let op = ast.str(n, "operator").unwrap_or("");
    let c = |k: &str| ast.child(n, k).is_some_and(|x| could_be_error(ast, x));
    match ast.kind(n) {
        "Identifier"
        | "CallExpression"
        | "NewExpression"
        | "MemberExpression"
        | "TaggedTemplateExpression"
        | "YieldExpression"
        | "AwaitExpression"
        | "ChainExpression" => true,
        "AssignmentExpression" => match op {
            "=" | "&&=" => c("right"),
            "||=" | "??=" => c("left") || c("right"),
            _ => false,
        },
        "SequenceExpression" => ast
            .list(n, "expressions")
            .last()
            .copied()
            .flatten()
            .is_some_and(|x| could_be_error(ast, x)),
        "LogicalExpression" => {
            if op == "&&" {
                c("right")
            } else {
                c("left") || c("right")
            }
        }
        "ConditionalExpression" => c("consequent") || c("alternate"),
        _ => false,
    }
}

/// `isNullOrUndefined(node)`.
pub fn is_null_or_undefined(ast: &Ast, n: NodeId) -> bool {
    is_null_literal(ast, n)
        || (ast.is(n, "Identifier") && ast.name(n) == Some("undefined"))
        || (ast.is(n, "UnaryExpression") && ast.str(n, "operator") == Some("void"))
}

/// `getUpperFunction(node)`.
pub fn get_upper_function(ast: &Ast, n: NodeId) -> Option<NodeId> {
    std::iter::once(n)
        .chain(ast.ancestors(n))
        .find(|x| is_function(ast, *x))
}

/// `startsWithUpperCase(s)`.
pub fn starts_with_upper_case(s: &str) -> bool {
    s.chars()
        .next()
        .is_some_and(|c| c.to_lowercase().ne(std::iter::once(c)))
}

fn has_this_tag(value: &str) -> bool {
    let chars: Vec<char> = value.chars().collect();
    let mut line_start = true;
    for i in 0..chars.len() {
        if line_start {
            let mut j = i;
            while j < chars.len() && (chars[j] == '*' || is_js_whitespace(chars[j])) {
                j += 1;
            }
            if chars[j..].starts_with(&['@', 't', 'h', 'i', 's']) {
                return true;
            }
        }
        line_start = matches!(chars[i], '\n' | '\r' | '\u{2028}' | '\u{2029}');
    }
    false
}

fn find_jsdoc_comment(sc: &crate::source::SourceCode, n: NodeId) -> Option<&Token> {
    let t = sc.token_before(n, crate::tokens::C)?;
    let line = sc.node_loc(n).start.line;
    (t.kind == TokKind::Block
        && t.value.starts_with('*')
        && line as i64 - sc.loc_of(t.end).line as i64 <= 1)
        .then_some(t)
}

fn get_jsdoc_comment(sc: &crate::source::SourceCode, n: NodeId) -> Option<&Token> {
    let ast = &sc.ast;
    let parent = ast.parent(n)?;
    match ast.kind(n) {
        "ClassDeclaration" | "FunctionDeclaration" => {
            let target = if matches!(
                ast.kind(parent),
                "ExportDefaultDeclaration" | "ExportNamedDeclaration" | "ExportAllDeclaration"
            ) {
                parent
            } else {
                n
            };
            find_jsdoc_comment(sc, target)
        }
        "ClassExpression" => find_jsdoc_comment(sc, ast.parent(parent)?),
        "ArrowFunctionExpression" | "FunctionExpression" => {
            if !matches!(ast.kind(parent), "CallExpression" | "NewExpression") {
                let mut p = Some(parent);
                while let Some(x) = p {
                    if !sc.comments_before(ast.range(x).0).is_empty()
                        || ast.kind(x).contains("Function")
                        || matches!(ast.kind(x), "MethodDefinition" | "Property")
                    {
                        break;
                    }
                    p = ast.parent(x);
                }
                if let Some(x) = p
                    && !matches!(ast.kind(x), "FunctionDeclaration" | "Program")
                {
                    return find_jsdoc_comment(sc, x);
                }
            }
            find_jsdoc_comment(sc, n)
        }
        _ => None,
    }
}

/// `isDefaultThisBinding(node, sourceCode, { capIsConstructor })`.
pub fn is_default_this_binding(
    sc: &crate::source::SourceCode,
    node: NodeId,
    cap_is_constructor: bool,
) -> bool {
    use crate::eslint_utils::is_specific_member_access as member;
    let ast = &sc.ast;
    if let Some(p) = ast.parent(node)
        && ast.is(p, "PropertyDefinition")
        && ast.child(p, "value") == Some(node)
    {
        return false;
    }
    if ast.is(node, "StaticBlock") {
        return false;
    }
    if matches!(ast.kind(node), "FunctionDeclaration" | "FunctionExpression")
        && ast
            .nodes_of(node, "params")
            .any(|p| is_specific_id(ast, p, "this"))
    {
        return false;
    }
    let id = ast.child(node, "id");
    let es5_ctor = id
        .and_then(|i| ast.name(i))
        .is_some_and(starts_with_upper_case);
    if (cap_is_constructor && es5_ctor)
        || get_jsdoc_comment(sc, node).is_some_and(|c| has_this_tag(&c.value))
        || sc
            .comments_before(ast.range(node).0)
            .iter()
            .any(|c| has_this_tag(&c.value))
    {
        return false;
    }
    let anonymous = id.is_none();
    let mut current = node;
    loop {
        let Some(parent) = ast.parent(current) else {
            return true;
        };
        match ast.kind(parent) {
            "LogicalExpression" | "ConditionalExpression" | "ChainExpression" => current = parent,
            "ReturnStatement" => {
                let Some(func) = get_upper_function(ast, parent) else {
                    return true;
                };
                if !is_callee(ast, func) {
                    return true;
                }
                let Some(p) = ast.parent(func) else {
                    return true;
                };
                current = p;
            }
            "ArrowFunctionExpression" => {
                if ast.child(parent, "body") != Some(current) || !is_callee(ast, parent) {
                    return true;
                }
                let Some(p) = ast.parent(parent) else {
                    return true;
                };
                current = p;
            }
            "Property" | "PropertyDefinition" | "MethodDefinition" => {
                return ast.child(parent, "value") != Some(current);
            }
            "AssignmentExpression" | "AssignmentPattern" => {
                let Some(left) = ast.child(parent, "left") else {
                    return true;
                };
                if ast.is(left, "MemberExpression") {
                    return false;
                }
                return !(cap_is_constructor
                    && anonymous
                    && ast.is(left, "Identifier")
                    && ast.name(left).is_some_and(starts_with_upper_case));
            }
            "VariableDeclarator" => {
                let id = ast.child(parent, "id");
                return !(cap_is_constructor
                    && anonymous
                    && ast.child(parent, "init") == Some(current)
                    && id.is_some_and(|i| {
                        ast.is(i, "Identifier") && ast.name(i).is_some_and(starts_with_upper_case)
                    }));
            }
            "MemberExpression" => {
                if ast.child(parent, "object") == Some(current)
                    && member(
                        ast,
                        parent,
                        None,
                        Some(&|p: &str| matches!(p, "bind" | "call" | "apply")),
                    )
                {
                    let Some(pp) = ast.parent(parent) else {
                        return true;
                    };
                    let maybe = if ast.is(pp, "ChainExpression") {
                        pp
                    } else {
                        parent
                    };
                    let Some(call) = ast.parent(maybe) else {
                        return true;
                    };
                    let first = ast.list(call, "arguments").first().copied().flatten();
                    return !(is_callee(ast, maybe)
                        && first.is_some_and(|a| !is_null_or_undefined(ast, a)));
                }
                return true;
            }
            "CallExpression" => {
                let Some(callee) = ast.child(parent, "callee") else {
                    return true;
                };
                let args = ast.list(parent, "arguments");
                let arg = |i: usize| args.get(i).copied().flatten();
                let nul = |i: usize| arg(i).is_some_and(|a| is_null_or_undefined(ast, a));
                if member(
                    ast,
                    callee,
                    Some(&|o: &str| o == "Reflect"),
                    Some(&|p: &str| p == "apply"),
                ) {
                    return args.len() != 3 || arg(0) != Some(current) || nul(1);
                }
                if member(
                    ast,
                    callee,
                    Some(&|o: &str| o.ends_with("Array")),
                    Some(&|p: &str| p == "from"),
                ) || member(
                    ast,
                    callee,
                    Some(&|o: &str| o == "Array"),
                    Some(&|p: &str| p == "fromAsync"),
                ) {
                    return args.len() != 3 || arg(1) != Some(current) || nul(2);
                }
                if member(
                    ast,
                    callee,
                    None,
                    Some(&|p: &str| {
                        matches!(
                            p,
                            "every"
                                | "filter"
                                | "find"
                                | "findIndex"
                                | "findLast"
                                | "findLastIndex"
                                | "flatMap"
                                | "forEach"
                                | "map"
                                | "some"
                        )
                    }),
                ) {
                    return args.len() != 2 || arg(0) != Some(current) || nul(1);
                }
                return true;
            }
            _ => return true,
        }
    }
}

/// `getBooleanValue(node)` of a Literal (`None` is `null`).
pub fn literal_boolean_value(ast: &Ast, n: NodeId) -> Option<bool> {
    if ast.prop(n, "regex").is_some() {
        return Some(true);
    }
    if let Some(b) = ast.str(n, "bigint") {
        let digits = match b.get(..2) {
            Some("0x" | "0X" | "0o" | "0O" | "0b" | "0B") => &b[2..],
            _ => b,
        };
        return Some(!digits.chars().all(|c| c == '0' || c == '_'));
    }
    match ast.prop(n, "value") {
        Some(Prop::Str(s)) => Some(!s.is_empty()),
        Some(Prop::Num(x)) => Some(!(x.is_nan() || *x == 0.0)),
        Some(Prop::Bool(b)) => Some(*b),
        _ => Some(false),
    }
}

/// `isLogicalIdentity(node, operator)`.
pub fn is_logical_identity(ast: &Ast, n: NodeId, operator: &str) -> bool {
    match ast.kind(n) {
        "Literal" => {
            let b = literal_boolean_value(ast, n);
            (operator == "||" && b == Some(true)) || (operator == "&&" && b == Some(false))
        }
        "UnaryExpression" => operator == "&&" && ast.str(n, "operator") == Some("void"),
        "LogicalExpression" => {
            ast.str(n, "operator") == Some(operator)
                && (ast
                    .child(n, "left")
                    .is_some_and(|l| is_logical_identity(ast, l, operator))
                    || ast
                        .child(n, "right")
                        .is_some_and(|r| is_logical_identity(ast, r, operator)))
        }
        "AssignmentExpression" => {
            let op = ast.str(n, "operator").unwrap_or("");
            matches!(op, "||=" | "&&=")
                && operator == &op[..op.len() - 1]
                && ast
                    .child(n, "right")
                    .is_some_and(|r| is_logical_identity(ast, r, operator))
        }
        _ => false,
    }
}

/// `isReferenceToGlobalVariable(scope, node)` of ast-utils.
pub fn is_reference_to_global_variable(
    sm: &crate::scope::ScopeManager,
    scope: crate::scope::ScopeId,
    node: NodeId,
) -> bool {
    sm.scope(scope)
        .references
        .iter()
        .find(|r| sm.reference(**r).identifier == node)
        .and_then(|r| sm.reference(*r).resolved)
        .is_some_and(|v| {
            let var = sm.var(v);
            sm.scope(var.scope).kind == crate::scope::ScopeType::Global && var.defs.is_empty()
        })
}

/// `isConstant(scope, node, inBooleanPosition)`.
pub fn is_constant(
    ast: &Ast,
    sm: &crate::scope::ScopeManager,
    scope: crate::scope::ScopeId,
    node: Option<NodeId>,
    in_bool: bool,
) -> bool {
    let Some(n) = node else { return true };
    let c = |x: Option<NodeId>, b: bool| is_constant(ast, sm, scope, x, b);
    match ast.kind(n) {
        "Literal"
        | "ArrowFunctionExpression"
        | "FunctionExpression"
        | "ClassExpression"
        | "ObjectExpression" => true,
        "TemplateLiteral" => {
            (in_bool
                && ast
                    .nodes_of(n, "quasis")
                    .any(|q| template_element_cooked(ast, q).is_some_and(|s| !s.is_empty())))
                || ast.nodes_of(n, "expressions").all(|e| c(Some(e), false))
        }
        "ArrayExpression" => in_bool || ast.list(n, "elements").iter().all(|e| c(*e, false)),
        "UnaryExpression" => {
            let op = ast.str(n, "operator").unwrap_or("");
            if op == "void" || (op == "typeof" && in_bool) {
                return true;
            }
            c(ast.child(n, "argument"), op == "!")
        }
        "BinaryExpression" => {
            c(ast.child(n, "left"), false)
                && c(ast.child(n, "right"), false)
                && ast.str(n, "operator") != Some("in")
        }
        "LogicalExpression" => {
            let op = ast.str(n, "operator").unwrap_or("");
            let (l, r) = (ast.child(n, "left"), ast.child(n, "right"));
            let lc = c(l, in_bool);
            let rc = c(r, in_bool);
            let ls = lc && l.is_some_and(|x| is_logical_identity(ast, x, op));
            let rs = in_bool && rc && r.is_some_and(|x| is_logical_identity(ast, x, op));
            (lc && rc) || ls || rs
        }
        "NewExpression" => in_bool,
        "AssignmentExpression" => {
            let op = ast.str(n, "operator").unwrap_or("");
            if op == "=" {
                return c(ast.child(n, "right"), in_bool);
            }
            if matches!(op, "||=" | "&&=") && in_bool {
                return ast
                    .child(n, "right")
                    .is_some_and(|r| is_logical_identity(ast, r, &op[..op.len() - 1]));
            }
            false
        }
        "SequenceExpression" => c(
            ast.list(n, "expressions").last().copied().flatten(),
            in_bool,
        ),
        "SpreadElement" => c(ast.child(n, "argument"), in_bool),
        "CallExpression" => {
            let Some(callee) = ast.child(n, "callee") else {
                return false;
            };
            if is_specific_id(ast, callee, "Boolean") {
                let args = ast.list(n, "arguments");
                if args.is_empty() || c(args[0], true) {
                    return is_reference_to_global_variable(sm, scope, callee);
                }
            }
            false
        }
        "Identifier" => {
            ast.name(n) == Some("undefined") && is_reference_to_global_variable(sm, scope, n)
        }
        _ => false,
    }
}

/// eslint-utils `getParentSyntaxParen(node, sourceCode)`: the start offset of the paren.
fn parent_syntax_paren(sc: &crate::source::SourceCode, node: NodeId) -> Option<u32> {
    use crate::tokens::{N, Opts};
    let ast = &sc.ast;
    let parent = ast.parent(node)?;
    let second = || {
        sc.first_token(
            parent,
            Opts {
                include_comments: false,
                skip: 1,
            },
        )
        .map(|t| t.start)
    };
    match ast.kind(parent) {
        "CallExpression" | "NewExpression" => {
            let args = ast.list(parent, "arguments");
            if args.len() == 1 && args[0] == Some(node) {
                let callee = ast.child(parent, "callee")?;
                return sc
                    .token_after_offset(ast.range(callee).1, N, is_opening_paren)
                    .map(|t| t.start);
            }
            None
        }
        "DoWhileStatement" => {
            if ast.child(parent, "test") == Some(node) {
                let body = ast.child(parent, "body")?;
                return sc
                    .token_after_offset(ast.range(body).1, N, is_opening_paren)
                    .map(|t| t.start);
            }
            None
        }
        "IfStatement" | "WhileStatement" => (ast.child(parent, "test") == Some(node))
            .then(second)
            .flatten(),
        "ImportExpression" => (ast.child(parent, "source") == Some(node))
            .then(second)
            .flatten(),
        "SwitchStatement" => (ast.child(parent, "discriminant") == Some(node))
            .then(second)
            .flatten(),
        "WithStatement" => (ast.child(parent, "object") == Some(node))
            .then(second)
            .flatten(),
        _ => None,
    }
}

/// eslint-utils `isParenthesized(times, node, sourceCode)`.
pub fn is_parenthesized_times(sc: &crate::source::SourceCode, times: u32, node: NodeId) -> bool {
    use crate::tokens::N;
    let ast = &sc.ast;
    let Some(parent) = ast.parent(node) else {
        return false;
    };
    if ast.is(parent, "CatchClause") && ast.child(parent, "param") == Some(node) {
        return false;
    }
    let syntax = parent_syntax_paren(sc, node);
    let (mut start, mut end) = ast.range(node);
    let mut times = times;
    while let (Some(l), Some(r)) = (
        sc.token_before_offset(start, N, |_| true),
        sc.token_after_offset(end, N, |_| true),
    ) {
        if !(is_opening_paren(l) && is_closing_paren(r)) || Some(l.start) == syntax {
            break;
        }
        times -= 1;
        if times == 0 {
            break;
        }
        start = l.start;
        end = r.end;
    }
    times == 0
}

pub fn is_closing_bracket(t: &Token) -> bool {
    t.kind == TokKind::Punctuator && t.value == "]"
}

/// `isLogicalAssignmentOperator(operator)`.
pub fn is_logical_assignment_operator(op: &str) -> bool {
    matches!(op, "&&=" | "||=" | "??=")
}

/// `isMixedLogicalAndCoalesceExpressions(left, right)`.
pub fn is_mixed_logical_and_coalesce_expressions(ast: &Ast, l: NodeId, r: NodeId) -> bool {
    let op = |n: NodeId| {
        if ast.is(n, "LogicalExpression") {
            ast.str(n, "operator")
        } else {
            None
        }
    };
    let (a, b) = (op(l), op(r));
    (matches!(a, Some("&&") | Some("||")) && b == Some("??"))
        || (a == Some("??") && matches!(b, Some("&&") | Some("||")))
}

pub fn is_null_literal(ast: &Ast, n: NodeId) -> bool {
    ast.is(n, "Literal")
        && ast.is_null(n, "value")
        && ast.prop(n, "regex").is_none()
        && ast.prop(n, "bigint").is_none()
}

pub fn is_static_template_literal(ast: &Ast, n: NodeId) -> bool {
    ast.is(n, "TemplateLiteral") && ast.list(n, "expressions").is_empty()
}

/// `typeof node.value` of a Literal.
pub fn literal_typeof(ast: &Ast, n: NodeId) -> &'static str {
    if ast.prop(n, "regex").is_some() {
        return "object";
    }
    if ast.prop(n, "bigint").is_some() {
        return "bigint";
    }
    match ast.prop(n, "value") {
        Some(Prop::Str(_)) => "string",
        Some(Prop::Num(_)) => "number",
        Some(Prop::Bool(_)) => "boolean",
        _ => "object",
    }
}

pub fn is_opening_brace(t: &Token) -> bool {
    t.kind == TokKind::Punctuator && t.value == "{"
}

pub fn is_closing_brace(t: &Token) -> bool {
    t.kind == TokKind::Punctuator && t.value == "}"
}

pub fn is_opening_paren(t: &Token) -> bool {
    t.kind == TokKind::Punctuator && t.value == "("
}

pub fn is_closing_paren(t: &Token) -> bool {
    t.kind == TokKind::Punctuator && t.value == ")"
}

pub fn is_semicolon(t: &Token) -> bool {
    t.kind == TokKind::Punctuator && t.value == ";"
}

pub fn is_comma(t: &Token) -> bool {
    t.kind == TokKind::Punctuator && t.value == ","
}

pub fn is_comment(t: &Token) -> bool {
    t.kind.is_comment()
}

pub fn is_string_literal(ast: &Ast, n: NodeId) -> bool {
    (ast.is(n, "Literal") && literal_typeof(ast, n) == "string") || ast.is(n, "TemplateLiteral")
}

pub fn is_numeric_literal(ast: &Ast, n: NodeId) -> bool {
    ast.is(n, "Literal") && (literal_typeof(ast, n) == "number" || ast.prop(n, "bigint").is_some())
}

pub fn is_coalesce_expression(ast: &Ast, n: NodeId) -> bool {
    ast.is(n, "LogicalExpression") && ast.str(n, "operator") == Some("??")
}

pub fn skip_chain_expression(ast: &Ast, n: NodeId) -> NodeId {
    if ast.is(n, "ChainExpression") {
        ast.child(n, "expression").unwrap_or(n)
    } else {
        n
    }
}

/// The `cooked` value of a TemplateElement (`None` when it is `null`).
pub fn template_element_cooked(ast: &Ast, n: NodeId) -> Option<&str> {
    ast.other(n, "value")
        .and_then(|v| v.get("cooked"))
        .and_then(|v| v.as_str())
}

pub fn template_element_raw(ast: &Ast, n: NodeId) -> Option<&str> {
    ast.other(n, "value")
        .and_then(|v| v.get("raw"))
        .and_then(|v| v.as_str())
}

/// `getStaticStringValue(node)`.
pub fn get_static_string_value(ast: &Ast, n: NodeId) -> Option<String> {
    match ast.kind(n) {
        "Literal" => match ast.prop(n, "value") {
            Some(Prop::Str(s)) => Some(s.clone()),
            Some(Prop::Num(x)) => Some(crate::json::js_number(*x)),
            Some(Prop::Bool(b)) => Some(b.to_string()),
            _ => {
                if is_null_literal(ast, n) {
                    return Some("null".into());
                }
                if let Some(r) = ast.other(n, "regex") {
                    let p = r.get("pattern").and_then(|v| v.as_str()).unwrap_or("");
                    let f = r.get("flags").and_then(|v| v.as_str()).unwrap_or("");
                    return Some(format!("/{p}/{f}"));
                }
                ast.str(n, "bigint").map(str::to_string)
            }
        },
        "TemplateLiteral" => {
            let q = ast.list(n, "quasis");
            if ast.list(n, "expressions").is_empty() && q.len() == 1 {
                return q[0]
                    .and_then(|e| template_element_cooked(ast, e))
                    .map(str::to_string);
            }
            None
        }
        _ => None,
    }
}

/// `getStaticPropertyName(node)`.
pub fn get_static_property_name(ast: &Ast, n: NodeId) -> Option<String> {
    let prop = match ast.kind(n) {
        "ChainExpression" => return get_static_property_name(ast, ast.child(n, "expression")?),
        "Property" | "PropertyDefinition" | "MethodDefinition" => ast.child(n, "key")?,
        "MemberExpression" => ast.child(n, "property")?,
        _ => return None,
    };
    if ast.is(prop, "Identifier") && !ast.bool(n, "computed") {
        return ast.name(prop).map(str::to_string);
    }
    get_static_string_value(ast, prop)
}

fn equal_literal_value(ast: &Ast, l: NodeId, r: NodeId) -> bool {
    let (lr, rr) = (ast.other(l, "regex"), ast.other(r, "regex"));
    if lr.is_some() || rr.is_some() {
        return match (lr, rr) {
            (Some(a), Some(b)) => {
                a.get("pattern") == b.get("pattern") && a.get("flags") == b.get("flags")
            }
            _ => false,
        };
    }
    let (lb, rb) = (ast.str(l, "bigint"), ast.str(r, "bigint"));
    if lb.is_some() || rb.is_some() {
        return lb == rb;
    }
    match (ast.prop(l, "value"), ast.prop(r, "value")) {
        (Some(Prop::Str(a)), Some(Prop::Str(b))) => a == b,
        (Some(Prop::Num(a)), Some(Prop::Num(b))) => a == b,
        (Some(Prop::Bool(a)), Some(Prop::Bool(b))) => a == b,
        (Some(Prop::Null) | None, Some(Prop::Null) | None) => true,
        _ => false,
    }
}

/// `isSameReference(left, right, disableStaticComputedKey)`.
pub fn is_same_reference(
    ast: &Ast,
    l: NodeId,
    r: NodeId,
    disable_static_computed_key: bool,
) -> bool {
    let (lk, rk) = (ast.kind(l), ast.kind(r));
    if lk != rk {
        if lk == "ChainExpression" {
            return ast
                .child(l, "expression")
                .is_some_and(|e| is_same_reference(ast, e, r, disable_static_computed_key));
        }
        if rk == "ChainExpression" {
            return ast
                .child(r, "expression")
                .is_some_and(|e| is_same_reference(ast, l, e, disable_static_computed_key));
        }
        return false;
    }
    match lk {
        "Super" | "ThisExpression" => true,
        "Identifier" | "PrivateIdentifier" => ast.name(l) == ast.name(r),
        "Literal" => equal_literal_value(ast, l, r),
        "ChainExpression" => match (ast.child(l, "expression"), ast.child(r, "expression")) {
            (Some(a), Some(b)) => is_same_reference(ast, a, b, disable_static_computed_key),
            _ => false,
        },
        "MemberExpression" => {
            let (Some(lo), Some(ro)) = (ast.child(l, "object"), ast.child(r, "object")) else {
                return false;
            };
            if !disable_static_computed_key && let Some(name_a) = get_static_property_name(ast, l) {
                return is_same_reference(ast, lo, ro, disable_static_computed_key)
                    && get_static_property_name(ast, r).as_deref() == Some(name_a.as_str());
            }
            let (Some(lp), Some(rp)) = (ast.child(l, "property"), ast.child(r, "property")) else {
                return false;
            };
            ast.bool(l, "computed") == ast.bool(r, "computed")
                && is_same_reference(ast, lo, ro, disable_static_computed_key)
                && is_same_reference(ast, lp, rp, disable_static_computed_key)
        }
        _ => false,
    }
}

/// `isSpecificMemberAccess(node, objectName, propertyName)` with plain string names.
pub fn is_specific_member_access(
    ast: &Ast,
    n: NodeId,
    object: Option<&str>,
    property: Option<&str>,
) -> bool {
    let c = skip_chain_expression(ast, n);
    if !ast.is(c, "MemberExpression") {
        return false;
    }
    if let Some(o) = object {
        let Some(obj) = ast.child(c, "object") else {
            return false;
        };
        if !(ast.is(obj, "Identifier") && ast.name(obj) == Some(o)) {
            return false;
        }
    }
    if let Some(p) = property
        && get_static_property_name(ast, c).as_deref() != Some(p)
    {
        return false;
    }
    true
}

/// `isParenthesised(sourceCode, node)`.
pub fn is_parenthesised(sc: &crate::source::SourceCode, n: NodeId) -> bool {
    let (s, e) = sc.ast.range(n);
    let (Some(prev), Some(next)) = (
        sc.token_before(n, crate::tokens::N),
        sc.token_after(n, crate::tokens::N),
    ) else {
        return false;
    };
    prev.value == "(" && prev.end <= s && next.value == ")" && next.start >= e
}

/// `getParenthesisedText(sourceCode, node)`.
pub fn get_parenthesised_text(sc: &crate::source::SourceCode, n: NodeId) -> String {
    use crate::tokens::N;
    let (Some(mut left), Some(mut right)) = (sc.first_token(n, N), sc.last_token(n, N)) else {
        return sc.node_text(n).to_string();
    };
    while let (Some(b), Some(a)) = (sc.token_before_tok(left, N), sc.token_after_tok(right, N)) {
        if !(is_opening_paren(b) && is_closing_paren(a)) {
            break;
        }
        left = b;
        right = a;
    }
    sc.slice16(left.start, right.end).to_string()
}

/// `getPrecedence(node)` for ESTree nodes (every ESTree type is in eslint-visitor-keys).
pub fn get_precedence(ast: &Ast, n: NodeId) -> i32 {
    let op = ast.str(n, "operator").unwrap_or("");
    let binary = |op: &str| match op {
        "|" => 6,
        "^" => 7,
        "&" => 8,
        "==" | "!=" | "===" | "!==" => 9,
        "<" | "<=" | ">" | ">=" | "in" | "instanceof" => 10,
        "<<" | ">>" | ">>>" => 11,
        "+" | "-" => 12,
        "*" | "/" | "%" => 13,
        "**" => 15,
        _ => 16,
    };
    match ast.kind(n) {
        "SequenceExpression" => 0,
        "AssignmentExpression" | "ArrowFunctionExpression" | "YieldExpression" => 1,
        "ConditionalExpression" => 3,
        "LogicalExpression" => match op {
            "||" | "??" => 4,
            "&&" => 5,
            _ => binary(op),
        },
        "BinaryExpression" => binary(op),
        "UnaryExpression" | "AwaitExpression" => 16,
        "UpdateExpression" => 17,
        "CallExpression" | "ChainExpression" | "ImportExpression" => 18,
        "NewExpression" => 19,
        _ => 20,
    }
}

/// `canTokensBeAdjacent(leftToken, rightToken)` for two tokens (or comments).
pub fn can_tokens_be_adjacent(left: &Token, right: &Token) -> bool {
    use TokKind::*;
    if left.kind == Shebang {
        return false;
    }
    if left.kind == Punctuator || right.kind == Punctuator {
        if left.kind == Punctuator && right.kind == Punctuator {
            let plus = |v: &str| v == "+" || v == "++";
            let minus = |v: &str| v == "-" || v == "--";
            return !((plus(&left.value) && plus(&right.value))
                || (minus(&left.value) && minus(&right.value)));
        }
        if left.kind == Punctuator && left.value == "/" {
            return !matches!(right.kind, Block | Line | RegularExpression);
        }
        return true;
    }
    if matches!(left.kind, String | Template) || matches!(right.kind, String | Template) {
        return true;
    }
    if left.kind != Numeric && right.kind == Numeric && right.value.starts_with('.') {
        return true;
    }
    if left.kind == Block || right.kind == Block || right.kind == Line {
        return true;
    }
    right.kind == PrivateIdentifier
}

/// `hasOctalOrNonOctalDecimalEscapeSequence(rawString)`: `/^(?:[^\\]|\\.)*\\(?:[1-9]|0\d)/su`.
pub fn has_octal_or_non_octal_decimal_escape_sequence(raw: &str) -> bool {
    let c: Vec<char> = raw.chars().collect();
    let mut i = 0;
    while i < c.len() {
        if c[i] == '\\' {
            match c.get(i + 1) {
                Some('1'..='9') => return true,
                Some('0') if c.get(i + 2).is_some_and(|d| d.is_ascii_digit()) => return true,
                Some(_) => i += 2,
                None => return false,
            }
        } else {
            i += 1;
        }
    }
    false
}

/// `isStartOfExpressionStatement(node)`.
pub fn is_start_of_expression_statement(ast: &Ast, n: NodeId) -> bool {
    let start = ast.range(n).0;
    for a in ast.ancestors(n) {
        if ast.range(a).0 != start {
            break;
        }
        if ast.is(a, "ExpressionStatement") {
            return true;
        }
    }
    false
}

/// `canContinueExpressionInClassBody(token)`.
pub fn can_continue_expression_in_class_body(t: &Token) -> bool {
    (t.kind == TokKind::Punctuator && (t.value == "[" || t.value == "*"))
        || (matches!(t.kind, TokKind::Identifier | TokKind::Keyword)
            && (t.value == "in" || t.value == "instanceof"))
}

/// `sourceCode.getNodeByRangeIndex(index)`.
pub fn node_by_range_index(sc: &crate::source::SourceCode, index: u32) -> Option<NodeId> {
    let ast = &sc.ast;
    let contains = |n: NodeId| {
        let (s, e) = ast.range(n);
        s <= index && index < e
    };
    if !contains(sc.root) {
        return None;
    }
    let mut n = sc.root;
    while let Some(c) = ast.children(n).into_iter().find(|c| contains(*c)) {
        n = c;
    }
    Some(n)
}

/// `needsPrecedingSemicolon(sourceCode, node)` for JavaScript (no TypeScript nodes).
pub fn needs_preceding_semicolon(sc: &crate::source::SourceCode, n: NodeId) -> bool {
    let ast = &sc.ast;
    let Some(prev) = sc.token_before(n, crate::tokens::N) else {
        return false;
    };
    if prev.kind == TokKind::Punctuator
        && matches!(prev.value.as_str(), ":" | ";" | "{" | "=>" | "++" | "--")
    {
        return false;
    }
    let Some(prev_node) = node_by_range_index(sc, prev.start) else {
        return false;
    };
    let parent = ast.parent(prev_node);
    let parent_kind = parent.map(|p| ast.kind(p)).unwrap_or("");
    if (parent_kind == "PropertyDefinition"
        && parent.and_then(|p| ast.child(p, "key")) == Some(prev_node))
        || (ast.is(prev_node, "PropertyDefinition")
            && prev.kind == TokKind::Punctuator
            && prev.value == "]")
    {
        return false;
    }
    if is_closing_paren(prev) {
        return !matches!(
            ast.kind(prev_node),
            "DoWhileStatement"
                | "ForInStatement"
                | "ForOfStatement"
                | "ForStatement"
                | "IfStatement"
                | "WhileStatement"
                | "WithStatement"
        );
    }
    if is_closing_brace(prev) {
        let grand_kind = parent
            .and_then(|p| ast.parent(p))
            .map(|g| ast.kind(g))
            .unwrap_or("");
        return (ast.is(prev_node, "BlockStatement")
            && parent_kind == "FunctionExpression"
            && grand_kind != "MethodDefinition")
            || (ast.is(prev_node, "ClassBody") && parent_kind == "ClassExpression")
            || ast.is(prev_node, "ObjectExpression");
    }
    if matches!(prev.kind, TokKind::Identifier | TokKind::Keyword) {
        if parent_kind == "VariableDeclarator"
            && parent.is_some_and(|p| ast.child(p, "init").is_none())
        {
            return false;
        }
        if matches!(parent_kind, "BreakStatement" | "ContinueStatement") {
            return false;
        }
        let node_type = match prev.value.as_str() {
            "break" => "BreakStatement",
            "continue" => "ContinueStatement",
            "debugger" => "DebuggerStatement",
            "do" => "DoWhileStatement",
            "else" => "IfStatement",
            "return" => "ReturnStatement",
            "yield" => "YieldExpression",
            _ => return true,
        };
        return ast.kind(prev_node) != node_type;
    }
    if prev.kind == TokKind::String {
        return !matches!(
            parent_kind,
            "ExportAllDeclaration" | "ExportNamedDeclaration" | "ImportDeclaration"
        );
    }
    true
}

pub fn is_opening_bracket(t: &Token) -> bool {
    t.kind == TokKind::Punctuator && t.value == "["
}

/// `\s` of a JS regular expression: WhiteSpace and LineTerminator.
pub fn is_js_whitespace(c: char) -> bool {
    matches!(
        c,
        '\t' | '\n' | '\u{b}' | '\u{c}' | '\r' | ' ' | '\u{a0}' | '\u{1680}' | '\u{2000}'
            ..='\u{200a}'
                | '\u{2028}'
                | '\u{2029}'
                | '\u{202f}'
                | '\u{205f}'
                | '\u{3000}'
                | '\u{feff}'
    )
}

/// `String.prototype.trim`.
pub fn js_trim(s: &str) -> &str {
    s.trim_matches(is_js_whitespace)
}

/// `equalTokens(left, right, sourceCode)`.
pub fn equal_tokens(sc: &crate::source::SourceCode, a: NodeId, b: NodeId) -> bool {
    let l = sc.tokens_of(a, crate::tokens::N);
    let r = sc.tokens_of(b, crate::tokens::N);
    l.len() == r.len()
        && l.iter()
            .zip(r)
            .all(|(x, y)| x.kind == y.kind && x.value == y.value)
}

/// `areBracesNecessary(node, sourceCode)` for a `BlockStatement` with one statement.
pub fn are_braces_necessary(sc: &crate::source::SourceCode, block: NodeId) -> bool {
    fn has_unsafe_if(ast: &Ast, n: NodeId) -> bool {
        match ast.kind(n) {
            "IfStatement" => match ast.child(n, "alternate") {
                None => true,
                Some(a) => has_unsafe_if(ast, a),
            },
            "ForStatement" | "ForInStatement" | "ForOfStatement" | "LabeledStatement"
            | "WithStatement" | "WhileStatement" => {
                ast.child(n, "body").is_some_and(|b| has_unsafe_if(ast, b))
            }
            _ => false,
        }
    }
    let ast = &sc.ast;
    let Some(Some(statement)) = ast.list(block, "body").first().copied() else {
        return false;
    };
    let lexical = match ast.kind(statement) {
        "VariableDeclaration" => {
            matches!(
                ast.str(statement, "kind"),
                Some("let" | "const" | "using" | "await using")
            )
        }
        "FunctionDeclaration" | "ClassDeclaration" => true,
        _ => false,
    };
    lexical
        || (has_unsafe_if(ast, statement)
            && sc
                .token_after(block, crate::tokens::N)
                .is_some_and(|t| t.value == "else" && t.kind == TokKind::Keyword))
}

/// `getFunctionNameWithKind(node)`.
pub fn get_function_name_with_kind(ast: &Ast, node: NodeId) -> String {
    let Some(parent) = ast.parent(node) else {
        return String::new();
    };
    let pk = ast.kind(parent);
    let mut tokens: Vec<String> = Vec::new();
    let key = ast.child(parent, "key");
    let private_key =
        !ast.bool(parent, "computed") && key.is_some_and(|k| ast.is(k, "PrivateIdentifier"));
    if pk == "MethodDefinition" || pk == "PropertyDefinition" {
        if ast.bool(parent, "static") {
            tokens.push("static".into());
        }
        if private_key {
            tokens.push("private".into());
        }
    }
    if ast.bool(node, "async") {
        tokens.push("async".into());
    }
    if ast.bool(node, "generator") {
        tokens.push("generator".into());
    }
    if pk == "Property" || pk == "MethodDefinition" {
        match ast.str(parent, "kind") {
            Some("constructor") => return "constructor".into(),
            Some("get") => tokens.push("getter".into()),
            Some("set") => tokens.push("setter".into()),
            _ => tokens.push("method".into()),
        }
    } else if pk == "PropertyDefinition" {
        tokens.push("method".into());
    } else {
        if ast.is(node, "ArrowFunctionExpression") {
            tokens.push("arrow".into());
        }
        tokens.push("function".into());
    }
    let id_name = || {
        ast.child(node, "id")
            .and_then(|i| ast.name(i))
            .map(|n| format!("'{n}'"))
    };
    if matches!(pk, "Property" | "MethodDefinition" | "PropertyDefinition") {
        if private_key {
            tokens.push(format!("#{}", key.and_then(|k| ast.name(k)).unwrap_or("")));
        } else if let Some(name) = get_static_property_name(ast, parent) {
            tokens.push(format!("'{name}'"));
        } else if let Some(n) = id_name() {
            tokens.push(n);
        }
    } else if let Some(n) = id_name() {
        tokens.push(n);
    }
    tokens.join(" ")
}

/// `isDecimalInteger(node)`.
pub fn is_decimal_integer(ast: &Ast, n: NodeId) -> bool {
    if !ast.is(n, "Literal") || literal_typeof(ast, n) != "number" {
        return false;
    }
    let raw = ast.str(n, "raw").unwrap_or("").as_bytes();
    match raw {
        [b'0'] => true,
        [b'0', rest @ ..] => {
            let i = rest
                .iter()
                .take_while(|c| (b'0'..=b'7').contains(c))
                .count();
            matches!(rest.get(i), Some(b'8' | b'9')) && rest[i + 1..].iter().all(u8::is_ascii_digit)
        }
        [b'1'..=b'9', rest @ ..] => {
            let mut prev_us = false;
            for c in rest {
                match c {
                    b'_' if !prev_us => prev_us = true,
                    b'0'..=b'9' => prev_us = false,
                    _ => return false,
                }
            }
            !prev_us
        }
        _ => false,
    }
}

/// lib/rules/utils/keywords.js
pub const KEYWORDS: &[&str] = &[
    "abstract",
    "boolean",
    "break",
    "byte",
    "case",
    "catch",
    "char",
    "class",
    "const",
    "continue",
    "debugger",
    "default",
    "delete",
    "do",
    "double",
    "else",
    "enum",
    "export",
    "extends",
    "false",
    "final",
    "finally",
    "float",
    "for",
    "function",
    "goto",
    "if",
    "implements",
    "import",
    "in",
    "instanceof",
    "int",
    "interface",
    "long",
    "native",
    "new",
    "null",
    "package",
    "private",
    "protected",
    "public",
    "return",
    "short",
    "static",
    "super",
    "switch",
    "synchronized",
    "this",
    "throw",
    "throws",
    "transient",
    "true",
    "try",
    "typeof",
    "var",
    "void",
    "volatile",
    "while",
    "with",
];

/// `getOpeningParenOfParams(node, sourceCode)`.
pub fn get_opening_paren_of_params(sc: &crate::source::SourceCode, n: NodeId) -> Option<&Token> {
    let ast = &sc.ast;
    use crate::tokens::N;
    let params = ast.list(n, "params");
    if ast.is(n, "ArrowFunctionExpression") && params.len() == 1 {
        let arg = sc.first_token(params[0]?, N)?;
        let maybe = sc.token_before_tok(arg, N)?;
        return Some(if is_opening_paren(maybe) { maybe } else { arg });
    }
    match ast.child(n, "id") {
        Some(id) => sc.token_after_f(id, is_opening_paren),
        None => sc.first_token_f(n, is_opening_paren),
    }
}

/// `getFunctionHeadLoc(node, sourceCode)` for JavaScript functions.
pub fn get_function_head_loc(
    sc: &crate::source::SourceCode,
    n: NodeId,
) -> Option<crate::source::Loc> {
    let ast = &sc.ast;
    let parent = ast.parent(n)?;
    let (start, end) = if matches!(
        ast.kind(parent),
        "Property" | "MethodDefinition" | "PropertyDefinition"
    ) {
        (
            sc.loc_of(ast.range(parent).0),
            sc.loc_of(get_opening_paren_of_params(sc, n)?.start),
        )
    } else if ast.is(n, "ArrowFunctionExpression") {
        let body = ast.child(n, "body")?;
        let arrow =
            sc.token_before_f(body, |t| t.kind == TokKind::Punctuator && t.value == "=>")?;
        (sc.loc_of(arrow.start), sc.loc_of(arrow.end))
    } else {
        (
            sc.loc_of(ast.range(n).0),
            sc.loc_of(get_opening_paren_of_params(sc, n)?.start),
        )
    };
    Some(crate::source::Loc { start, end })
}

#[cfg(test)]
mod group_b_tests {
    use super::*;

    #[test]
    fn octal_escape_pattern() {
        assert!(has_octal_or_non_octal_decimal_escape_sequence(r"'\1'"));
        assert!(has_octal_or_non_octal_decimal_escape_sequence(r"'a\08'"));
        assert!(has_octal_or_non_octal_decimal_escape_sequence(r"'\9'"));
        assert!(!has_octal_or_non_octal_decimal_escape_sequence(r"'\0'"));
        assert!(!has_octal_or_non_octal_decimal_escape_sequence(r"'\\1'"));
        assert!(has_octal_or_non_octal_decimal_escape_sequence(r"'\\\1'"));
        assert!(!has_octal_or_non_octal_decimal_escape_sequence(r"'\0a'"));
    }

    fn tok(kind: TokKind, v: &str) -> Token {
        Token {
            kind,
            start: 0,
            end: 0,
            value: v.into(),
        }
    }

    #[test]
    fn adjacent_tokens() {
        use TokKind::*;
        assert!(!can_tokens_be_adjacent(
            &tok(Punctuator, "+"),
            &tok(Punctuator, "++")
        ));
        assert!(can_tokens_be_adjacent(
            &tok(Punctuator, "+"),
            &tok(Punctuator, "-")
        ));
        assert!(!can_tokens_be_adjacent(
            &tok(Keyword, "typeof"),
            &tok(Boolean, "true")
        ));
        assert!(can_tokens_be_adjacent(
            &tok(Identifier, "a"),
            &tok(Numeric, ".5")
        ));
        assert!(!can_tokens_be_adjacent(
            &tok(Numeric, "1"),
            &tok(Numeric, ".5")
        ));
        assert!(!can_tokens_be_adjacent(
            &tok(Punctuator, "/"),
            &tok(RegularExpression, "/a/")
        ));
        assert!(can_tokens_be_adjacent(
            &tok(String, "'a'"),
            &tok(Identifier, "in")
        ));
    }
}
