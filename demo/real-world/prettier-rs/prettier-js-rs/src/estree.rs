//! `src/language-js/print/estree.js` with `literal.js`, `statement.js`, `block.js`,
//! `expression-statement.js` and `semicolon.js`.

use crate::value::Value;

use crate::comments::Flags;
use crate::doc::*;
use crate::node::*;
use crate::printer::{Args, Printer, R, bail};
use crate::utils::*;

pub fn print(p: &mut Printer<'_>, args: &Args) -> R {
    let node = p.node();
    let ty = node.ty();
    if ty.starts_with("JSX") {
        return crate::jsx::print_jsx(p);
    }
    if ty.starts_with("TS") {
        return crate::typescript::print_typescript(p, args);
    }
    if is_literal(node) {
        return print_literal(p);
    }
    let semi = if p.opts.semi { ";" } else { "" };
    match ty {
        "EmptyStatement" => Ok(empty()),
        "ExpressionStatement" => {
            let mut parts = vec![p.print("expression")?];
            if p.opts.semi && !single_jsx_in_markdown(p) {
                parts.push(s(";"));
            }
            Ok(concat(parts))
        }
        "ChainExpression" => p.print("expression"),
        "AssignmentExpression" => crate::assignment::print_assignment_expression(p),
        "VariableDeclarator" => crate::assignment::print_variable_declarator(p),
        "BinaryExpression" | "LogicalExpression" => crate::binaryish::print_binaryish(p),
        "AssignmentPattern" => Ok(concat(vec![p.print("left")?, s(" = "), p.print("right")?])),
        "MemberExpression" | "OptionalMemberExpression" => {
            crate::member::print_member_expression(p)
        }
        "MetaProperty" => Ok(concat(vec![p.print("meta")?, s("."), p.print("property")?])),
        "Identifier" => Ok(concat(vec![
            s(node.str_of("name").unwrap_or("").to_string()),
            crate::typescript::print_optional_token(p),
            crate::typescript::print_definite_token(p),
            crate::typescript::print_type_annotation_property(p, "typeAnnotation")?,
        ])),
        "SpreadElement" | "RestElement" => Ok(concat(vec![
            s("..."),
            p.print("argument")?,
            crate::typescript::print_type_annotation_property(p, "typeAnnotation")?,
        ])),
        "FunctionDeclaration" | "FunctionExpression" => crate::function::print_function(p, args),
        "ArrowFunctionExpression" => crate::arrow::print_arrow_function(p, args),
        "YieldExpression" => {
            let mut parts = vec![s("yield")];
            if node.flag("delegate") {
                parts.push(s("*"));
            }
            if node.field("argument").is_some() {
                parts.push(s(" "));
                parts.push(p.print("argument")?);
            }
            Ok(concat(parts))
        }
        "AwaitExpression" => print_await(p),
        "ExportDefaultDeclaration" | "ExportNamedDeclaration" | "ExportAllDeclaration" => {
            crate::module::print_export_declaration(p)
        }
        "ImportDeclaration" => crate::module::print_import_declaration(p),
        "ImportSpecifier"
        | "ExportSpecifier"
        | "ImportNamespaceSpecifier"
        | "ExportNamespaceSpecifier"
        | "ImportDefaultSpecifier"
        | "ExportDefaultSpecifier" => crate::module::print_module_specifier(p),
        "ImportAttribute" => crate::property::print_property(p),
        "Program" | "BlockStatement" | "StaticBlock" => print_block(p),
        "ThrowStatement" => Ok(concat(vec![
            s("throw"),
            crate::function::print_return_or_throw_argument(p)?,
        ])),
        "ReturnStatement" => Ok(concat(vec![
            s("return"),
            crate::function::print_return_or_throw_argument(p)?,
        ])),
        "NewExpression" | "ImportExpression" | "OptionalCallExpression" | "CallExpression" => {
            crate::call::print_call_expression(p)
        }
        "ObjectExpression" | "ObjectPattern" => crate::object::print_object(p),
        "Property" => {
            if is_method(node) {
                crate::function::print_method(p)
            } else {
                crate::property::print_property(p)
            }
        }
        "ArrayExpression" | "ArrayPattern" => crate::array::print_array(p),
        "SequenceExpression" => print_sequence(p),
        "ThisExpression" => Ok(s("this")),
        "Super" => Ok(s("super")),
        "UnaryExpression" => {
            let op = node.str_of("operator").unwrap_or("");
            let mut parts = vec![s(op.to_string())];
            if op.ends_with(|c: char| c.is_ascii_lowercase()) {
                parts.push(s(" "));
            }
            let has = node.field("argument").is_some_and(|a| p.has_any_comment(a));
            let arg = p.print("argument")?;
            parts.push(if has {
                group(concat(vec![
                    s("("),
                    indent(concat(vec![softline(), arg])),
                    softline(),
                    s(")"),
                ]))
            } else {
                arg
            });
            Ok(concat(parts))
        }
        "UpdateExpression" => {
            let op = node.str_of("operator").unwrap_or("");
            let prefix = node.flag("prefix");
            Ok(concat(vec![
                s(if prefix { op } else { "" }.to_string()),
                p.print("argument")?,
                s(if prefix { "" } else { op }.to_string()),
            ]))
        }
        "ConditionalExpression" => crate::ternary::print_ternary(p, args),
        "VariableDeclaration" => print_variable_declaration(p),
        "WithStatement" => {
            let body = adjust_clause(node.field("body"), p.print("body")?, false);
            Ok(group(concat(vec![
                s("with ("),
                p.print("object")?,
                s(")"),
                body,
            ])))
        }
        "IfStatement" => print_if(p),
        "ForStatement" => {
            let body = adjust_clause(node.field("body"), p.print("body")?, false);
            let dangling = p.print_dangling(node, false, None);
            let printed_comments = if is_empty(&dangling) {
                empty()
            } else {
                concat(vec![dangling, softline()])
            };
            if node.field("init").is_none()
                && node.field("test").is_none()
                && node.field("update").is_none()
            {
                return Ok(concat(vec![
                    printed_comments,
                    group(concat(vec![s("for (;;)"), body])),
                ]));
            }
            Ok(concat(vec![
                printed_comments,
                group(concat(vec![
                    s("for ("),
                    group(concat(vec![
                        indent(concat(vec![
                            softline(),
                            p.print("init")?,
                            s(";"),
                            line(),
                            p.print("test")?,
                            s(";"),
                            line(),
                            p.print("update")?,
                        ])),
                        softline(),
                    ])),
                    s(")"),
                    body,
                ])),
            ]))
        }
        "WhileStatement" => {
            let test = p.print("test")?;
            let body = adjust_clause(node.field("body"), p.print("body")?, false);
            Ok(group(concat(vec![
                s("while ("),
                group(concat(vec![
                    indent(concat(vec![softline(), test])),
                    softline(),
                ])),
                s(")"),
                body,
            ])))
        }
        "ForInStatement" => {
            let left = p.print("left")?;
            let right = p.print("right")?;
            let body = adjust_clause(node.field("body"), p.print("body")?, false);
            Ok(group(concat(vec![
                s("for ("),
                left,
                s(" in "),
                right,
                s(")"),
                body,
            ])))
        }
        "ForOfStatement" => {
            let left = p.print("left")?;
            let right = p.print("right")?;
            let body = adjust_clause(node.field("body"), p.print("body")?, false);
            Ok(group(concat(vec![
                s("for"),
                s(if node.flag("await") { " await" } else { "" }),
                s(" ("),
                left,
                s(" of "),
                right,
                s(")"),
                body,
            ])))
        }
        "DoWhileStatement" => {
            let clause = adjust_clause(node.field("body"), p.print("body")?, false);
            let mut parts = vec![group(concat(vec![s("do"), clause]))];
            if node.field("body").is_some_and(|b| b.is("BlockStatement")) {
                parts.push(s(" "));
            } else {
                parts.push(hardline());
            }
            let test = p.print("test")?;
            parts.push(s("while ("));
            parts.push(group(concat(vec![
                indent(concat(vec![softline(), test])),
                softline(),
            ])));
            parts.push(s(")"));
            parts.push(s(semi));
            Ok(concat(parts))
        }
        "BreakStatement" | "ContinueStatement" => {
            let mut parts = vec![s(if ty == "BreakStatement" {
                "break"
            } else {
                "continue"
            })];
            if node.field("label").is_some() {
                parts.push(s(" "));
                parts.push(p.print("label")?);
            }
            parts.push(s(semi));
            Ok(concat(parts))
        }
        "LabeledStatement" => {
            if node.field("body").is_some_and(|b| b.is("EmptyStatement")) {
                return Ok(concat(vec![p.print("label")?, s(":;")]));
            }
            Ok(concat(vec![p.print("label")?, s(": "), p.print("body")?]))
        }
        "TryStatement" => {
            let mut parts = vec![s("try "), p.print("block")?];
            if node.field("handler").is_some() {
                parts.push(s(" "));
                parts.push(p.print("handler")?);
            }
            if node.field("finalizer").is_some() {
                parts.push(s(" finally "));
                parts.push(p.print("finalizer")?);
            }
            Ok(concat(parts))
        }
        "CatchClause" => {
            if let Some(param_node) = node.field("param") {
                let text = p.text;
                let has = p.comments.has_with(param_node, Flags::ANY, &|c| {
                    !c.block
                        || (p.comments.of(param_node).iter().any(|a| {
                            std::ptr::eq(p.comments.get(a.index), c)
                                && ((a.kind == crate::comments::Kind::Leading
                                    && crate::text::has_newline(text, c.end, false))
                                    || (a.kind == crate::comments::Kind::Trailing
                                        && crate::text::has_newline(text, c.start, true)))
                        }))
                });
                let param = p.print("param")?;
                return Ok(concat(vec![
                    s("catch "),
                    if has {
                        concat(vec![
                            s("("),
                            indent(concat(vec![softline(), param])),
                            softline(),
                            s(") "),
                        ])
                    } else {
                        concat(vec![s("("), param, s(") ")])
                    },
                    p.print("body")?,
                ]));
            }
            Ok(concat(vec![s("catch "), p.print("body")?]))
        }
        "SwitchStatement" => {
            let disc = p.print("discriminant")?;
            let cases_len = node.list("cases").len();
            let text = p.text;
            let cases = p.map("cases", |p, i| {
                let printed = p.print_current(Args::default())?;
                let extra = if i + 1 != cases_len && is_next_line_empty(p.node(), text) {
                    hardline()
                } else {
                    empty()
                };
                Ok(concat(vec![printed, extra]))
            })?;
            Ok(concat(vec![
                group(concat(vec![
                    s("switch ("),
                    indent(concat(vec![softline(), disc])),
                    softline(),
                    s(")"),
                ])),
                s(" {"),
                if cases_len > 0 {
                    indent(concat(vec![hardline(), join(&hardline(), cases)]))
                } else {
                    empty()
                },
                hardline(),
                s("}"),
            ]))
        }
        "SwitchCase" => {
            let mut parts = Vec::new();
            if node.field("test").is_some() {
                parts.push(s("case "));
                parts.push(p.print("test")?);
                parts.push(s(":"));
            } else {
                parts.push(s("default:"));
            }
            if p.has_comment(node, Flags::DANGLING) {
                parts.push(s(" "));
                parts.push(p.print_dangling(node, false, None));
            }
            let consequent: Vec<&Value> = node
                .list("consequent")
                .iter()
                .filter(|n| !n.is("EmptyStatement"))
                .collect();
            if !consequent.is_empty() {
                let cons = print_statement_sequence(p, "consequent")?;
                parts.push(
                    if consequent.len() == 1 && consequent[0].is("BlockStatement") {
                        concat(vec![s(" "), cons])
                    } else {
                        indent(concat(vec![hardline(), cons]))
                    },
                );
            }
            Ok(concat(parts))
        }
        "DebuggerStatement" => Ok(concat(vec![s("debugger"), s(semi)])),
        "TemplateElement" => Ok(replace_end_of_line(
            node.get("value")
                .and_then(|v| v.str_of("raw"))
                .unwrap_or(""),
        )),
        "TemplateLiteral" => crate::template::print_template_literal(p),
        "TaggedTemplateExpression" => crate::template::print_tagged_template_literal(p),
        "ClassDeclaration" | "ClassExpression" => crate::class::print_class(p),
        "ClassBody" => crate::class::print_class_body(p),
        "JsonRoot" => Ok(concat(vec![
            p.print_dangling(node, false, None),
            p.print("node")?,
            hardline(),
        ])),
        "Decorator" => Ok(concat(vec![s("@"), p.print("expression")?])),
        "ParenthesizedExpression" => {
            let expression = node.field("expression").unwrap_or(node);
            let hug = !p.has_any_comment(expression)
                && (expression.is("ObjectExpression") || expression.is("ArrayExpression"));
            let printed = p.print("expression")?;
            Ok(if hug {
                concat(vec![s("("), printed, s(")")])
            } else {
                group(concat(vec![
                    s("("),
                    indent(concat(vec![softline(), printed])),
                    softline(),
                    s(")"),
                ]))
            })
        }
        "MethodDefinition" => crate::class::print_class_method(p),
        "PropertyDefinition" | "AccessorProperty" => crate::class::print_class_property(p),
        "PrivateIdentifier" => Ok(concat(vec![
            s("#"),
            s(node.str_of("name").unwrap_or("").to_string()),
        ])),
        _ => bail(format!("node {ty}")),
    }
}

pub fn is_method(n: &Value) -> bool {
    n.flag("method") || matches!(n.str_of("kind"), Some("get" | "set"))
}

fn print_await(p: &mut Printer<'_>) -> R {
    let node = p.node();
    let mut parts = vec![s("await")];
    if node.field("argument").is_some() {
        parts.push(s(" "));
        parts.push(p.print("argument")?);
        if let Some(parent) = p.parent()
            && ((is_call_expression(parent)
                && parent.field("callee").is_some_and(|c| c.same(node)))
                || (is_member_expression(parent)
                    && parent.field("object").is_some_and(|c| c.same(node))))
        {
            parts = vec![indent(concat(vec![softline(), concat(parts)])), softline()];
            let ancestor = p.find_ancestor(|n| n.is("AwaitExpression") || n.is("BlockStatement"));
            let keep = ancestor.is_some_and(|a| {
                a.is("AwaitExpression")
                    && a.field("argument")
                        .is_some_and(|arg| starts_with_no_lookahead_token(arg, &|l| l.same(node)))
            });
            if !keep {
                return Ok(group(concat(parts)));
            }
        }
    }
    Ok(concat(parts))
}

fn print_sequence(p: &mut Printer<'_>) -> R {
    let parent = p.parent();
    if parent.is_some_and(|pa| pa.is("ExpressionStatement") || pa.is("ForStatement")) {
        let mut parts = Vec::new();
        p.each("expressions", |p, i| {
            let d = p.print_current(Args::default())?;
            if i == 0 {
                parts.push(d);
            } else {
                parts.push(s(","));
                parts.push(indent(concat(vec![line(), d])));
            }
            Ok(())
        })?;
        return Ok(group(concat(parts)));
    }
    let printed = p.print_all("expressions")?;
    let parts = join(&concat(vec![s(","), line()]), printed);
    let key = p.key();
    if let Some(pa) = parent
        && (((pa.is("ReturnStatement") || pa.is("ThrowStatement")) && key == Some("argument"))
            || (pa.is("ArrowFunctionExpression") && key == Some("body")))
    {
        return Ok(group(if_break(
            concat(vec![
                indent(concat(vec![softline(), parts.clone()])),
                softline(),
            ]),
            parts,
        )));
    }
    Ok(group(parts))
}

fn print_variable_declaration(p: &mut Printer<'_>) -> R {
    let node = p.node();
    let printed = p.print_all("declarations")?;
    let parent = p.parent();
    let is_parent_for_loop = parent.is_some_and(|pa| {
        matches!(
            pa.ty(),
            "ForStatement" | "ForInStatement" | "ForOfStatement"
        )
    });
    let has_value = node
        .list("declarations")
        .iter()
        .any(|d| d.field("init").is_some());
    let mut iter = printed.into_iter();
    let first = iter.next();
    let rest: Vec<Doc> = iter.collect();
    let first_has_comment = node
        .list("declarations")
        .first()
        .is_some_and(|d| p.has_any_comment(d));
    let first_variable = match first {
        Some(f) if rest.is_empty() && !first_has_comment => Some(f),
        Some(f) => Some(indent(f)),
        None => None,
    };
    let mut parts = vec![
        crate::typescript::print_declare_token(p),
        s(node.str_of("kind").unwrap_or("var").to_string()),
        match first_variable {
            Some(f) => concat(vec![s(" "), f]),
            None => empty(),
        },
        indent(concat(
            rest.into_iter()
                .map(|d| {
                    concat(vec![
                        s(","),
                        if has_value && !is_parent_for_loop {
                            hardline()
                        } else {
                            line()
                        },
                        d,
                    ])
                })
                .collect(),
        )),
    ];
    let is_body = parent.is_some_and(|pa| pa.field("body").is_some_and(|b| b.same(node)));
    if !(is_parent_for_loop && !is_body) {
        parts.push(s(if p.opts.semi { ";" } else { "" }));
    }
    Ok(group(concat(parts)))
}

fn print_if(p: &mut Printer<'_>) -> R {
    let node = p.node();
    let consequent = adjust_clause(node.field("consequent"), p.print("consequent")?, false);
    let test = p.print("test")?;
    let opening = group(concat(vec![
        s("if ("),
        group(concat(vec![
            indent(concat(vec![softline(), test])),
            softline(),
        ])),
        s(")"),
        consequent,
    ]));
    let mut parts = vec![opening];
    if let Some(alternate) = node.field("alternate") {
        let cons_node = node.field("consequent");
        let comment_on_own_line = cons_node
            .is_some_and(|c| p.has_comment(c, Flags::TRAILING.with(Flags::LINE)))
            || needs_hardline_after_dangling_comment(p, node);
        let else_on_same_line =
            cons_node.is_some_and(|c| c.is("BlockStatement")) && !comment_on_own_line;
        parts.push(if else_on_same_line {
            s(" ")
        } else {
            hardline()
        });
        if p.has_comment(node, Flags::DANGLING) {
            parts.push(p.print_dangling(node, false, None));
            parts.push(if comment_on_own_line {
                hardline()
            } else {
                s(" ")
            });
        }
        let alt = p.print("alternate")?;
        parts.push(s("else"));
        parts.push(group(adjust_clause(
            Some(alternate),
            alt,
            alternate.is("IfStatement"),
        )));
    }
    Ok(concat(parts))
}

pub fn adjust_clause(node: Option<&Value>, clause: Doc, force_space: bool) -> Doc {
    let Some(node) = node else {
        return indent(concat(vec![line(), clause]));
    };
    if node.is("EmptyStatement") {
        return s(";");
    }
    if node.is("BlockStatement") || force_space {
        return concat(vec![s(" "), clause]);
    }
    indent(concat(vec![line(), clause]))
}

pub fn print_statement_sequence(p: &mut Printer<'_>, key: &'static str) -> R {
    let node = p.node();
    let last = node.list(key).iter().rposition(|s| !s.is("EmptyStatement"));
    let text = p.text;
    let mut parts = Vec::new();
    p.each(key, |p, i| {
        let child = p.node();
        if child.is("EmptyStatement") {
            return Ok(());
        }
        parts.push(p.print_current(Args::default())?);
        if Some(i) != last {
            parts.push(hardline());
            if is_next_line_empty(child, text) {
                parts.push(hardline());
            }
        }
        Ok(())
    })?;
    Ok(concat(parts))
}

pub fn needs_hardline_after_dangling_comment(p: &Printer<'_>, node: &Value) -> bool {
    if !p.has_any_comment(node) {
        return false;
    }
    p.comments
        .get_comments(node, Flags::DANGLING, None)
        .last()
        .is_some_and(|a| !p.comments.get(a.index).block)
}

pub fn print_block_pub(p: &mut Printer<'_>) -> R {
    print_block(p)
}

fn print_block(p: &mut Printer<'_>) -> R {
    let node = p.node();
    let has_body = node.list("body").iter().any(|n| !n.is("EmptyStatement"));
    let has_dangling = p.has_comment(node, Flags::DANGLING);
    let body = if has_body || has_dangling {
        let mut parts = Vec::new();
        if has_body {
            parts.push(print_statement_sequence(p, "body")?);
        }
        // Babel keeps directives out of `body`, and `printBlockBody` puts a line between them and
        // dangling comments when nothing else follows them.
        let last = node.list("body").iter().rfind(|n| !n.is("EmptyStatement"));
        if has_dangling
            && p.comments.babel
            && let Some(last) = last.filter(|n| n.get("directive").is_some_and(Value::is_string))
        {
            parts.push(hardline());
            if is_next_line_empty(last, p.text) {
                parts.push(hardline());
            }
        }
        if has_dangling {
            parts.push(p.print_dangling(node, false, None));
        }
        Some(concat(parts))
    } else {
        None
    };
    let parent = p.parent();
    if node.is("Program") {
        return Ok(match body {
            Some(b) => concat(vec![b, hardline()]),
            None => empty(),
        });
    }
    let mut parts = Vec::new();
    if node.is("StaticBlock") {
        parts.push(s("static "));
    }
    parts.push(s("{"));
    match body {
        Some(b) => {
            parts.push(indent(concat(vec![hardline(), b])));
            parts.push(hardline());
        }
        None => {
            let pt = parent.map(|n| n.ty()).unwrap_or("");
            let gp = p.grandparent();
            let skip = matches!(
                pt,
                "ArrowFunctionExpression"
                    | "FunctionExpression"
                    | "FunctionDeclaration"
                    | "ComponentDeclaration"
                    | "HookDeclaration"
                    | "ObjectMethod"
                    | "ClassMethod"
                    | "ClassPrivateMethod"
                    | "ForStatement"
                    | "WhileStatement"
                    | "DoWhileStatement"
                    | "DoExpression"
                    | "ModuleExpression"
                    | "TSModuleDeclaration"
            ) || (pt == "CatchClause"
                && gp.is_some_and(|g| g.field("finalizer").is_none()))
                || node.is("StaticBlock");
            if !skip {
                parts.push(hardline());
            }
        }
    }
    parts.push(s("}"));
    Ok(concat(parts))
}

fn print_literal(p: &mut Printer<'_>) -> R {
    let node = p.node();
    if let Some(regex) = node.field("regex") {
        let mut flags: Vec<char> = regex.str_of("flags").unwrap_or("").chars().collect();
        flags.sort_unstable();
        let flags: String = flags.into_iter().collect();
        return Ok(s(format!(
            "/{}/{}",
            regex.str_of("pattern").unwrap_or(""),
            flags
        )));
    }
    let raw = node.str_of("raw").unwrap_or("");
    if node.field("bigint").is_some() {
        return Ok(s(raw.to_ascii_lowercase()));
    }
    match node.get("value") {
        Some(Value::Number(_)) => Ok(s(print_number(raw))),
        Some(Value::String(_)) => {
            if is_directive(p) {
                let content = &raw[1..raw.len() - 1];
                if content.contains('"') || content.contains('\'') {
                    return Ok(s(raw.to_string()));
                }
                let q = if p.opts.single_quote { "'" } else { "\"" };
                return Ok(s(format!("{q}{content}{q}")));
            }
            Ok(replace_end_of_line(&print_string(raw, p.opts)))
        }
        Some(Value::Bool(b)) => Ok(s(b.to_string())),
        Some(Value::Null) => {
            if raw == "null" {
                Ok(s("null"))
            } else {
                bail("literal without value")
            }
        }
        _ => bail("literal"),
    }
}

fn is_directive(p: &Printer<'_>) -> bool {
    p.key() == Some("expression")
        && p.parent().is_some_and(|pa| {
            pa.is("ExpressionStatement") && pa.get("directive").is_some_and(Value::is_string)
        })
}

/// `shouldPrintLeadingSemicolon` (`semicolon.js`).
fn single_jsx_in_markdown(p: &Printer<'_>) -> bool {
    p.opts.parent_markdown
        && p.node().is("ExpressionStatement")
        && p.node()
            .get("expression")
            .is_some_and(|e| e.is("JSXElement") || e.is("JSXFragment"))
        && p.parent()
            .is_some_and(|pa| pa.is("Program") && pa.list("body").len() == 1)
}

pub fn should_print_leading_semicolon(p: &mut Printer<'_>) -> R<bool> {
    if p.opts.semi || single_jsx_in_markdown(p) {
        return Ok(false);
    }
    let node = p.node();
    let key = p.key();
    let Some(parent) = p.parent() else {
        return Ok(false);
    };
    if node.is("ExpressionStatement")
        && ((key == Some("body")
            && matches!(
                parent.ty(),
                "Program" | "BlockStatement" | "StaticBlock" | "TSModuleBlock"
            ))
            || (key == Some("consequent") && parent.is("SwitchCase")))
    {
        return p
            .call("expression", expression_needs_asi_protection)
            .unwrap_or(Ok(false));
    }
    Ok(false)
}

fn expression_needs_asi_protection(p: &mut Printer<'_>) -> R<bool> {
    let node = p.node();
    match node.ty() {
        "ParenthesizedExpression"
        | "TypeCastExpression"
        | "ArrayExpression"
        | "ArrayPattern"
        | "TemplateLiteral"
        | "TemplateElement"
        | "RegExpLiteral" => return Ok(true),
        "ArrowFunctionExpression" => {
            if !crate::function::should_print_params_without_parens(p) {
                return Ok(true);
            }
        }
        "UnaryExpression" => {
            if node.flag("prefix") && matches!(node.str_of("operator"), Some("+" | "-")) {
                return Ok(true);
            }
        }
        "BindExpression" => {
            if node.field("object").is_none() {
                return Ok(true);
            }
        }
        "Literal" => {
            if node.field("regex").is_some() {
                return Ok(true);
            }
        }
        _ => {
            if is_jsx_element(node) {
                return Ok(true);
            }
        }
    }
    if crate::parens::needs_parens(p)? {
        return Ok(true);
    }
    if !has_naked_left_side(node) {
        return Ok(false);
    }
    let path = left_side_path(node);
    p.call_path(&path, expression_needs_asi_protection)
        .unwrap_or(Ok(false))
}
