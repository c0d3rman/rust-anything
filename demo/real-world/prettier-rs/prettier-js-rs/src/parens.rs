//! `src/language-js/needs-parens.js`, for the JavaScript nodes this port prints.

use crate::value::Value;

use crate::node::*;
use crate::printer::{Printer, R, bail};
use crate::utils::*;

type Pred<'p> = Option<&'p dyn Fn(&Value, Option<&str>) -> bool>;

impl Printer<'_> {
    /// `path.match(...predicates)`: predicate `i` gets the node `i` levels up and its key.
    pub fn matches(&self, preds: &[Pred<'_>]) -> bool {
        for (level, pred) in preds.iter().enumerate() {
            let Some(frame) = self.frame(level) else {
                return false;
            };
            let key = if level == 0 {
                None
            } else {
                self.frame(level - 1).and_then(|f| f.key)
            };
            if let Some(p) = pred
                && !p(frame.node, key)
            {
                return false;
            }
        }
        true
    }
}

pub fn needs_parens(p: &mut Printer<'_>) -> R<bool> {
    if p.depth() == 1 {
        return Ok(false);
    }
    let node = p.node();
    let key = p.key();
    let Some(parent) = p.parent() else {
        return Ok(false);
    };
    if is_statement(node.ty()) {
        return Ok(false);
    }
    if node.is("Identifier") {
        let name = node.str_of("name").unwrap_or("");
        if key == Some("left")
            && ((name == "async" && !parent.flag("await")) || name == "let")
            && parent.is("ForOfStatement")
        {
            return Ok(true);
        }
        if name == "let" {
            if let Some(expr) = p
                .find_ancestor(|n| n.is("ForOfStatement"))
                .and_then(|f| f.field("left"))
                && starts_with_no_lookahead_token(expr, &|n| n.same(node))
            {
                return Ok(true);
            }
            if key == Some("object")
                && parent.is("MemberExpression")
                && parent.flag("computed")
                && !parent.flag("optional")
            {
                let statement = p.find_ancestor(|n| {
                    matches!(
                        n.ty(),
                        "ExpressionStatement" | "ForStatement" | "ForInStatement"
                    )
                });
                let expression = statement.and_then(|s| match s.ty() {
                    "ExpressionStatement" => s.field("expression"),
                    "ForStatement" => s.field("init"),
                    _ => s.field("left"),
                });
                if let Some(e) = expression
                    && starts_with_no_lookahead_token(e, &|n| n.same(node))
                {
                    return Ok(true);
                }
            }
        }
        if key == Some("expression")
            && matches!(
                name,
                "await"
                    | "interface"
                    | "module"
                    | "using"
                    | "yield"
                    | "let"
                    | "component"
                    | "hook"
                    | "type"
            )
        {
            let ancestor = p.find_ancestor(|n| !is_binary_cast_expression(n));
            if let Some(a) = ancestor
                && !a.same(parent)
                && a.is("ExpressionStatement")
            {
                return Ok(true);
            }
        }
        return Ok(false);
    }

    if matches!(
        node.ty(),
        "ObjectExpression" | "FunctionExpression" | "ClassExpression" | "DoExpression"
    ) && let Some(expr) = p
        .find_ancestor(|n| n.is("ExpressionStatement"))
        .and_then(|s| s.field("expression"))
        && starts_with_no_lookahead_token(expr, &|n| n.same(node))
    {
        return Ok(true);
    }

    if node.is("ObjectExpression")
        && let Some(body) = p
            .find_ancestor(|n| n.is("ArrowFunctionExpression"))
            .and_then(|a| a.field("body"))
        && !body.is("SequenceExpression")
        && !body.is("AssignmentExpression")
        && starts_with_no_lookahead_token(body, &|n| n.same(node))
    {
        return Ok(true);
    }

    match parent.ty() {
        "ParenthesizedExpression" => return Ok(false),
        "ClassDeclaration" | "ClassExpression" => {
            if key == Some("superClass")
                && matches!(
                    node.ty(),
                    "ArrowFunctionExpression"
                        | "AssignmentExpression"
                        | "AwaitExpression"
                        | "BinaryExpression"
                        | "ConditionalExpression"
                        | "LogicalExpression"
                        | "NewExpression"
                        | "ObjectExpression"
                        | "SequenceExpression"
                        | "TaggedTemplateExpression"
                        | "UnaryExpression"
                        | "UpdateExpression"
                        | "YieldExpression"
                        | "TSNonNullExpression"
                )
                || (key == Some("superClass")
                    && node.is("ClassExpression")
                    && !node.list("decorators").is_empty())
            {
                return Ok(true);
            }
        }
        "ExportDefaultDeclaration" => {
            return Ok(should_wrap_function_for_export_default(p)? || node.is("SequenceExpression"));
        }
        "Decorator" => {
            if key == Some("expression")
                && !can_decorator_expression_unparenthesized(
                    node,
                    p.opts.parser == crate::options::Parser::Typescript,
                )?
            {
                return Ok(true);
            }
        }
        "BinaryExpression" => {
            if key == Some("left")
                && matches!(parent.str_of("operator"), Some("in" | "instanceof"))
                && node.is("UnaryExpression")
            {
                return Ok(true);
            }
        }
        "VariableDeclarator"
            if key == Some("init")
                && p.matches(&[
                    None,
                    None,
                    Some(&|n, k| k == Some("declarations") && n.is("VariableDeclaration")),
                    Some(&|n, k| k == Some("left") && n.is("ForInStatement")),
                ]) =>
        {
            return Ok(true);
        }
        _ => {}
    }

    let ty = node.ty();
    match ty {
        "UpdateExpression" | "UnaryExpression" => {
            if ty == "UpdateExpression" && parent.is("UnaryExpression") {
                let op = node.str_of("operator");
                let pop = parent.str_of("operator");
                return Ok(node.flag("prefix")
                    && ((op == Some("++") && pop == Some("+"))
                        || (op == Some("--") && pop == Some("-"))));
            }
            Ok(match parent.ty() {
                "UnaryExpression" => {
                    let op = node.str_of("operator");
                    op == parent.str_of("operator") && matches!(op, Some("+" | "-"))
                }
                "BindExpression" => true,
                "MemberExpression" | "OptionalMemberExpression" => key == Some("object"),
                "TaggedTemplateExpression" => true,
                "NewExpression" | "CallExpression" | "OptionalCallExpression" => {
                    key == Some("callee")
                }
                "BinaryExpression" => {
                    key == Some("left") && parent.str_of("operator") == Some("**")
                }
                "TSNonNullExpression" => true,
                _ => false,
            })
        }
        "BinaryExpression"
        | "LogicalExpression"
        | "TSTypeAssertion"
        | "TSAsExpression"
        | "TSSatisfiesExpression"
        | "AsExpression"
        | "AsConstExpression"
        | "SatisfiesExpression" => {
            if ty == "BinaryExpression" {
                if parent.is("UpdateExpression") {
                    return Ok(true);
                }
                if node.str_of("operator") == Some("in") && is_path_in_for_statement_initializer(p)
                {
                    return Ok(true);
                }
            }
            Ok(match parent.ty() {
                "TSAsExpression"
                | "TSSatisfiesExpression"
                | "AsExpression"
                | "AsConstExpression"
                | "SatisfiesExpression" => !is_binary_cast_expression(node),
                "ConditionalExpression" => {
                    is_binary_cast_expression(node) || is_nullish_coalescing(node)
                }
                "CallExpression" | "NewExpression" | "OptionalCallExpression" => {
                    key == Some("callee")
                }
                "ClassExpression" | "ClassDeclaration" => key == Some("superClass"),
                "TSTypeAssertion"
                | "TaggedTemplateExpression"
                | "UnaryExpression"
                | "JSXSpreadAttribute"
                | "SpreadElement"
                | "BindExpression"
                | "AwaitExpression"
                | "TSNonNullExpression"
                | "UpdateExpression" => true,
                "MemberExpression" | "OptionalMemberExpression" => key == Some("object"),
                "AssignmentExpression" | "AssignmentPattern" => {
                    key == Some("left")
                        && (ty == "TSTypeAssertion" || is_binary_cast_expression(node))
                }
                "LogicalExpression" if ty == "LogicalExpression" => {
                    parent.str_of("operator") != node.str_of("operator")
                }
                "LogicalExpression" | "BinaryExpression" => {
                    let Some(operator) = node.str_of("operator") else {
                        return Ok(ty != "TSTypeAssertion"
                            || is_bitwise_operator(parent.str_of("operator").unwrap_or("")));
                    };
                    let precedence = precedence(operator);
                    let parent_op = parent.str_of("operator").unwrap_or("");
                    let parent_precedence = precedence_of(parent_op);
                    if parent_precedence > precedence {
                        return Ok(true);
                    }
                    if key == Some("right") && parent_precedence == precedence {
                        return Ok(true);
                    }
                    if parent_precedence == precedence && !should_flatten(parent_op, operator) {
                        return Ok(true);
                    }
                    if parent_precedence < precedence && operator == "%" {
                        return Ok(parent_op == "+" || parent_op == "-");
                    }
                    is_bitwise_operator(parent_op)
                }
                _ => false,
            })
        }
        "SequenceExpression" => Ok(!parent.is("ForStatement")),
        "YieldExpression" | "AwaitExpression" => {
            if ty == "YieldExpression"
                && matches!(parent.ty(), "AwaitExpression" | "TSTypeAssertion")
            {
                return Ok(true);
            }
            Ok(match parent.ty() {
                "TaggedTemplateExpression"
                | "UnaryExpression"
                | "LogicalExpression"
                | "SpreadElement"
                | "TSAsExpression"
                | "TSSatisfiesExpression"
                | "TSNonNullExpression"
                | "AsExpression"
                | "AsConstExpression"
                | "SatisfiesExpression"
                | "BindExpression" => true,
                "MemberExpression" | "OptionalMemberExpression" => key == Some("object"),
                "NewExpression" | "CallExpression" | "OptionalCallExpression" => {
                    key == Some("callee")
                }
                "ConditionalExpression" => key == Some("test"),
                "BinaryExpression" => {
                    !(node.field("argument").is_none() && parent.str_of("operator") == Some("|>"))
                }
                _ => false,
            })
        }
        "Literal" => {
            if node.get("value").is_some_and(Value::is_string)
                && parent.is("ExpressionStatement")
                && !parent.get("directive").is_some_and(Value::is_string)
            {
                let gp = p.grandparent();
                return Ok(gp.is_some_and(|g| g.is("Program") || g.is("BlockStatement")));
            }
            Ok(key == Some("object") && is_member_expression(parent) && is_numeric_literal(node))
        }
        "AssignmentExpression" => {
            if matches!(key, Some("init" | "update")) && parent.is("ForStatement") {
                return Ok(false);
            }
            if key == Some("expression")
                && !node.field("left").is_some_and(|l| l.is("ObjectPattern"))
                && parent.is("ExpressionStatement")
            {
                return Ok(false);
            }
            if key == Some("key") && parent.is("TSPropertySignature") {
                return Ok(false);
            }
            if parent.is("AssignmentExpression") {
                return Ok(false);
            }
            if key == Some("expressions")
                && parent.is("SequenceExpression")
                && p.matches(&[
                    None,
                    None,
                    Some(&|n, k| matches!(k, Some("init" | "update")) && n.is("ForStatement")),
                ])
            {
                return Ok(false);
            }
            if key == Some("value")
                && parent.is("Property")
                && p.matches(&[
                    None,
                    None,
                    Some(&|n, k| k == Some("properties") && n.is("ObjectPattern")),
                ])
            {
                return Ok(false);
            }
            Ok(true)
        }
        "ConditionalExpression" => Ok(match parent.ty() {
            "TaggedTemplateExpression"
            | "UnaryExpression"
            | "SpreadElement"
            | "BinaryExpression"
            | "LogicalExpression"
            | "NGPipeExpression"
            | "ExportDefaultDeclaration"
            | "AwaitExpression"
            | "JSXSpreadAttribute"
            | "TSTypeAssertion"
            | "TypeCastExpression"
            | "TSAsExpression"
            | "TSSatisfiesExpression"
            | "AsExpression"
            | "AsConstExpression"
            | "SatisfiesExpression"
            | "TSNonNullExpression" => true,
            "NewExpression" | "CallExpression" | "OptionalCallExpression" => key == Some("callee"),
            "ConditionalExpression" => !p.opts.experimental_ternaries && key == Some("test"),
            "MemberExpression" | "OptionalMemberExpression" => key == Some("object"),
            _ => false,
        }),
        "FunctionExpression" => Ok(match parent.ty() {
            "NewExpression" | "CallExpression" | "OptionalCallExpression" => key == Some("callee"),
            "TaggedTemplateExpression" => true,
            _ => false,
        }),
        "ArrowFunctionExpression" => Ok(match parent.ty() {
            "BinaryExpression" => parent.str_of("operator") != Some("|>") || node.parenthesized(),
            "NewExpression" | "CallExpression" | "OptionalCallExpression" => key == Some("callee"),
            "MemberExpression" | "OptionalMemberExpression" => key == Some("object"),
            "TSAsExpression"
            | "TSSatisfiesExpression"
            | "AsExpression"
            | "AsConstExpression"
            | "SatisfiesExpression"
            | "TSNonNullExpression"
            | "BindExpression"
            | "TaggedTemplateExpression"
            | "UnaryExpression"
            | "LogicalExpression"
            | "AwaitExpression"
            | "TSTypeAssertion" => true,
            "ConditionalExpression" => key == Some("test"),
            _ => false,
        }),
        "ClassExpression" => Ok(parent.is("NewExpression") && key == Some("callee")),
        "TSFunctionType" | "TSConditionalType" | "TSConstructorType" | "TSUnionType"
        | "TSIntersectionType" | "TSInferType" | "TSTypeOperator" => {
            Ok(ts_type_needs_parens(p, node, parent, key))
        }
        "TSTypeQuery" => Ok(
            (key == Some("objectType") && parent.is("TSIndexedAccessType"))
                || (key == Some("elementType") && parent.is("TSArrayType")),
        ),
        "TSInstantiationExpression" => Ok(key == Some("object") && is_member_expression(parent)),
        "OptionalMemberExpression"
        | "OptionalCallExpression"
        | "CallExpression"
        | "MemberExpression"
        | "TaggedTemplateExpression"
        | "TSNonNullExpression" => {
            if matches!(
                ty,
                "OptionalMemberExpression"
                    | "OptionalCallExpression"
                    | "CallExpression"
                    | "MemberExpression"
            ) && should_add_parentheses_to_chain_element(p)
            {
                return Ok(true);
            }
            if key == Some("callee") && matches!(parent.ty(), "BindExpression" | "NewExpression") {
                let mut object = Some(node);
                while let Some(o) = object {
                    match o.ty() {
                        "CallExpression" | "OptionalCallExpression" => return Ok(true),
                        "MemberExpression" | "OptionalMemberExpression" | "BindExpression" => {
                            object = o.field("object")
                        }
                        "TaggedTemplateExpression" => object = o.field("tag"),
                        "TSNonNullExpression" => object = o.field("expression"),
                        // Babel has no `ChainExpression`: the chain's links are its
                        // `Optional*` nodes, which this loop walks through.
                        "ChainExpression" if p.opts.parser == crate::options::Parser::Babel => {
                            object = o.field("expression")
                        }
                        _ => return Ok(false),
                    }
                }
            }
            Ok(false)
        }
        "JSXFragment" | "JSXElement" => Ok(key == Some("callee")
            || (key == Some("left")
                && parent.is("BinaryExpression")
                && parent.str_of("operator") == Some("<"))
            || (!parent.is("ArrayExpression")
                && !matches!(
                    parent.ty(),
                    "ArrowFunctionExpression"
                        | "AssignmentExpression"
                        | "AssignmentPattern"
                        | "BinaryExpression"
                        | "NewExpression"
                        | "ConditionalExpression"
                        | "ExpressionStatement"
                        | "JsExpressionRoot"
                        | "JSXAttribute"
                        | "JSXElement"
                        | "JSXExpressionContainer"
                        | "JSXFragment"
                        | "LogicalExpression"
                        | "ReturnStatement"
                        | "ThrowStatement"
                        | "TypeCastExpression"
                        | "VariableDeclarator"
                        | "YieldExpression"
                )
                && !is_call_expression(parent)
                && !is_object_property(parent))),
        _ => Ok(false),
    }
}

/// `isDecoratorMemberExpression`. typescript-estree keeps `ChainExpression` inside a member
/// chain (`(a?.b).c`), which `isMemberExpression` sees through but `node.property` does not:
/// Prettier throws there.
fn is_decorator_member_expression(n: &Value, ts: bool) -> R<bool> {
    if n.is("Identifier") {
        return Ok(true);
    }
    if is_member_expression(n) {
        if ts && n.is("ChainExpression") {
            return bail("Prettier crashes on a parenthesized optional chain in a decorator");
        }
        return Ok(!n.flag("computed")
            && !n.flag("optional")
            && n.field("property").is_some_and(|p| p.is("Identifier"))
            && match n.field("object") {
                Some(o) => is_decorator_member_expression(o, ts)?,
                None => false,
            });
    }
    Ok(false)
}

fn can_decorator_expression_unparenthesized(n: &Value, ts: bool) -> R<bool> {
    let n = if n.is("ChainExpression") {
        n.field("expression").unwrap_or(n)
    } else {
        n
    };
    if is_decorator_member_expression(n, ts)? {
        return Ok(true);
    }
    Ok(is_call_expression(n)
        && !n.flag("optional")
        && match n.field("callee") {
            Some(c) => is_decorator_member_expression(c, ts)?,
            None => false,
        })
}

fn is_path_in_for_statement_initializer(p: &Printer<'_>) -> bool {
    let mut level = 0;
    while let Some(frame) = p.frame(level) {
        let Some(parent) = p.frame(level + 1) else {
            return false;
        };
        if parent.node.is("ForStatement")
            && parent
                .node
                .field("init")
                .is_some_and(|i| i.same(frame.node))
        {
            return true;
        }
        level += 1;
    }
    false
}

fn should_wrap_function_for_export_default(p: &mut Printer<'_>) -> R<bool> {
    let node = p.node();
    let parent = p.parent();
    let parent_is_export = parent.is_some_and(|n| n.is("ExportDefaultDeclaration"));
    if node.is("FunctionExpression") || node.is("ClassExpression") {
        return Ok(parent_is_export || !needs_parens(p)?);
    }
    if !has_naked_left_side(node) || (!parent_is_export && needs_parens(p)?) {
        return Ok(false);
    }
    let path = left_side_path(node);
    match p.call_path(&path, should_wrap_function_for_export_default) {
        Some(r) => r,
        None => Ok(false),
    }
}

fn should_add_parentheses_to_chain_element(p: &Printer<'_>) -> bool {
    let is = |t: &'static str| move |n: &Value, _k: Option<&str>| n.is(t);
    let optional = |n: &Value, _k: Option<&str>| {
        matches!(
            n.ty(),
            "OptionalCallExpression" | "OptionalMemberExpression"
        )
    };
    let key_ty = |key: &'static str, t: &'static str| {
        move |n: &Value, k: Option<&str>| k == Some(key) && n.is(t)
    };
    let tag_template = key_ty("tag", "TaggedTemplateExpression");
    let chain = key_ty("expression", "ChainExpression");
    let non_null = key_ty("expression", "TSNonNullExpression");
    if p.matches(&[None, Some(&chain), Some(&tag_template)])
        || p.matches(&[Some(&optional), Some(&tag_template)])
        || p.matches(&[Some(&optional), Some(&non_null), Some(&tag_template)])
        || p.matches(&[None, Some(&chain), Some(&non_null), Some(&tag_template)])
        || p.matches(&[None, Some(&non_null), Some(&chain), Some(&tag_template)])
    {
        return true;
    }
    let object_or_callee = |n: &Value, k: Option<&str>| {
        (k == Some("object") && n.is("MemberExpression"))
            || (k == Some("callee") && (n.is("CallExpression") || n.is("NewExpression")))
    };
    let object_or_call_callee = |n: &Value, k: Option<&str>| {
        (k == Some("object") && n.is("MemberExpression"))
            || (k == Some("callee") && n.is("CallExpression"))
    };
    if p.matches(&[Some(&optional), Some(&object_or_callee)])
        || p.matches(&[
            Some(&optional),
            Some(&non_null),
            Some(&object_or_call_callee),
        ])
    {
        return true;
    }
    let call_or_member =
        |n: &Value, _k: Option<&str>| n.is("CallExpression") || n.is("MemberExpression");
    let not_optional_parent = |n: &Value, k: Option<&str>| {
        (k == Some("callee")
            && ((n.is("CallExpression") && !n.flag("optional")) || n.is("NewExpression")))
            || (k == Some("object") && n.is("MemberExpression") && !n.flag("optional"))
    };
    if p.matches(&[Some(&call_or_member), Some(&chain)])
        && (p.matches(&[None, None, Some(&not_optional_parent)])
            || p.matches(&[None, None, Some(&non_null), Some(&object_or_call_callee)]))
    {
        return true;
    }
    let _ = is("");
    p.matches(&[
        Some(&call_or_member),
        Some(&non_null),
        Some(&chain),
        Some(&object_or_call_callee),
    ])
}

pub fn is_statement(ty: &str) -> bool {
    matches!(
        ty,
        "BlockStatement"
            | "BreakStatement"
            | "ComponentDeclaration"
            | "ClassBody"
            | "ClassDeclaration"
            | "ClassMethod"
            | "ClassProperty"
            | "PropertyDefinition"
            | "ClassPrivateProperty"
            | "ContinueStatement"
            | "DebuggerStatement"
            | "DeclareComponent"
            | "DeclareClass"
            | "DeclareExportAllDeclaration"
            | "DeclareExportDeclaration"
            | "DeclareFunction"
            | "DeclareHook"
            | "DeclareInterface"
            | "DeclareModule"
            | "DeclareModuleExports"
            | "DeclareNamespace"
            | "DeclareVariable"
            | "DeclareEnum"
            | "DoWhileStatement"
            | "EnumDeclaration"
            | "ExportAllDeclaration"
            | "ExportDefaultDeclaration"
            | "ExportNamedDeclaration"
            | "ExpressionStatement"
            | "ForInStatement"
            | "ForOfStatement"
            | "ForStatement"
            | "FunctionDeclaration"
            | "HookDeclaration"
            | "IfStatement"
            | "ImportDeclaration"
            | "InterfaceDeclaration"
            | "LabeledStatement"
            | "MethodDefinition"
            | "ReturnStatement"
            | "SwitchStatement"
            | "ThrowStatement"
            | "TryStatement"
            | "TSDeclareFunction"
            | "TSEnumDeclaration"
            | "TSImportEqualsDeclaration"
            | "TSInterfaceDeclaration"
            | "TSModuleDeclaration"
            | "TSNamespaceExportDeclaration"
            | "TypeAlias"
            | "VariableDeclaration"
            | "WhileStatement"
            | "WithStatement"
    )
}

fn ts_type_needs_parens(p: &Printer<'_>, node: &Value, parent: &Value, key: Option<&str>) -> bool {
    let ty = node.ty();
    let is_cond = |n: &Value| n.is("TSConditionalType") || n.is("ConditionalTypeAnnotation");
    if ty == "TSFunctionType"
        && p.matches(&[
            Some(&|n, _| n.is("TSFunctionType")),
            Some(&|n, k| k == Some("typeAnnotation") && n.is("TSTypeAnnotation")),
            Some(&|n, k| k == Some("returnType") && n.is("ArrowFunctionExpression")),
        ])
    {
        return true;
    }
    if matches!(
        ty,
        "TSFunctionType" | "TSConditionalType" | "TSConstructorType"
    ) {
        if key == Some("extendsType") && is_cond(node) && parent.ty() == ty {
            return true;
        }
        if key == Some("checkType") && is_cond(parent) {
            return true;
        }
        if key == Some("extendsType") && parent.is("TSConditionalType") {
            let annotation = node
                .field("returnType")
                .or_else(|| node.field("typeAnnotation"))
                .and_then(|a| a.field("typeAnnotation"));
            let mut annotation = annotation;
            if let Some(a) = annotation
                && a.is("TSTypePredicate")
                && let Some(inner) = a.field("typeAnnotation")
            {
                annotation = inner.field("typeAnnotation");
            }
            if annotation.is_some_and(|a| {
                a.is("TSInferType")
                    && a.field("typeParameter")
                        .is_some_and(|tp| tp.field("constraint").is_some())
            }) {
                return true;
            }
        }
    }
    if matches!(
        ty,
        "TSFunctionType"
            | "TSConditionalType"
            | "TSConstructorType"
            | "TSUnionType"
            | "TSIntersectionType"
    ) && matches!(
        parent.ty(),
        "TSUnionType" | "UnionTypeAnnotation" | "TSIntersectionType" | "IntersectionTypeAnnotation"
    ) && parent.list("types").len() > 1
        && (node.get("types").is_none() || node.list("types").len() > 1)
    {
        return true;
    }
    if ty == "TSInferType" {
        if parent.is("TSRestType") {
            return false;
        }
        if key == Some("types")
            && (parent.is("TSUnionType") || parent.is("TSIntersectionType"))
            && node
                .field("typeParameter")
                .is_some_and(|tp| tp.is("TSTypeParameter") && tp.field("constraint").is_some())
        {
            return true;
        }
    }
    parent.is("TSArrayType")
        || parent.is("TSOptionalType")
        || parent.is("TSRestType")
        || (key == Some("objectType") && parent.is("TSIndexedAccessType"))
        || parent.is("TSTypeOperator")
        || (parent.is("TSTypeAnnotation")
            && p.grandparent()
                .is_some_and(|g| g.ty().starts_with("TSJSDoc")))
}
