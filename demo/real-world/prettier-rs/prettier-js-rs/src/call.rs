//! `src/language-js/print/call-expression.js`, `call-arguments.js` and `member-chain.js`.

use crate::value::Value;

use crate::comments::{Comments, Flags};
use crate::doc::*;
use crate::member::{print_member_lookup, print_optional_token};
use crate::node::*;
use crate::printer::{Args, Printer, R, Unsupported, bail};
use crate::utils::*;

pub const ARG_EXPANSION_BAILOUT: &str = "ArgExpansionBailout";

pub fn print_call_expression(p: &mut Printer<'_>) -> R {
    let node = p.node();
    let is_new = node.is("NewExpression");
    let is_dynamic_import = node.is("ImportExpression");
    let optional = print_optional_token(p);
    let args = call_arguments(node);
    let is_template_single = args.len() == 1 && is_template_on_its_own_line(args[0], p.text);
    if p.opts.parser == crate::options::Parser::Typescript
        && !is_template_single
        && !is_commonjs_or_amd_module_definition(p)
        && test_call_crashes(node)
    {
        return bail("Prettier crashes on an optional call in a test call");
    }
    if is_template_single
        || is_commonjs_or_amd_module_definition(p)
        || is_test_call(node, p.parent())
    {
        let mut printed = Vec::new();
        iterate_call_arguments(p, |p, _| {
            printed.push(p.print_current(Args::default())?);
            Ok(())
        })?;
        return Ok(concat(vec![
            s(if is_new { "new " } else { "" }),
            print_callee(p)?,
            optional,
            crate::typescript::print_function_type_parameters(p)?,
            s("("),
            join(&s(", "), printed),
            s(")"),
        ]));
    }
    if !is_dynamic_import && !is_new && node.field("callee").is_some_and(is_memberish) {
        let callee = node.field("callee").unwrap_or(node);
        let path: Vec<(&'static str, Option<usize>)> = if callee.is("ChainExpression") {
            vec![("callee", None), ("expression", None)]
        } else {
            vec![("callee", None)]
        };
        let callee_needs_parens = p
            .call_path(&path, crate::parens::needs_parens)
            .unwrap_or(Ok(false))?;
        if !callee_needs_parens {
            return print_member_chain(p);
        }
    }
    let contents = concat(vec![
        s(if is_new { "new " } else { "" }),
        print_callee(p)?,
        optional,
        crate::typescript::print_function_type_parameters(p)?,
        print_call_arguments(p)?,
    ]);
    if is_dynamic_import || node.field("callee").is_some_and(is_call_expression) {
        return Ok(group(contents));
    }
    Ok(contents)
}

fn print_callee(p: &mut Printer<'_>) -> R {
    let node = p.node();
    if node.is("ImportExpression") {
        return Ok(match node.str_of("phase") {
            Some(phase) => s(format!("import.{phase}")),
            None => s("import"),
        });
    }
    p.print("callee")
}

fn is_commonjs_or_amd_module_definition(p: &Printer<'_>) -> bool {
    let node = p.node();
    if !node.is("CallExpression") || node.flag("optional") {
        return false;
    }
    let Some(callee) = node.field("callee").filter(|c| c.is("Identifier")) else {
        return false;
    };
    let args = call_arguments(node);
    match callee.str_of("name") {
        Some("require") => (args.len() == 1 && is_string_literal(args[0])) || args.len() > 1,
        Some("define") if p.parent().is_some_and(|pa| pa.is("ExpressionStatement")) => {
            args.len() == 1
                || (args.len() == 2 && args[0].is("ArrayExpression"))
                || (args.len() == 3 && is_string_literal(args[0]) && args[1].is("ArrayExpression"))
        }
        _ => false,
    }
}

fn iterate_call_arguments(
    p: &mut Printer<'_>,
    mut f: impl FnMut(&mut Printer<'_>, usize) -> R<()>,
) -> R<()> {
    let node = p.node();
    if node.is("ChainExpression") {
        return p
            .call("expression", |p| iterate_call_arguments(p, f))
            .unwrap_or(Ok(()));
    }
    if node.is("ImportExpression") || node.is("TSImportType") {
        let key = if node.is("ImportExpression") {
            "source"
        } else {
            "argument"
        };
        if let Some(r) = p.call(key, |p| f(p, 0)) {
            r?;
        }
        if node.field("options").is_some()
            && let Some(r) = p.call("options", |p| f(p, 1))
        {
            r?;
        }
        return Ok(());
    }
    p.each("arguments", |p, i| f(p, i))
}

fn print_argument(p: &mut Printer<'_>, index_from_end: bool, args: Args) -> R {
    let node = p.node();
    let count = call_arguments(node).len();
    if node.is("ImportExpression") || node.is("TSImportType") {
        let key = if index_from_end && node.field("options").is_some() {
            "options"
        } else if node.is("ImportExpression") {
            "source"
        } else {
            "argument"
        };
        return p.print_args(key, args);
    }
    let index = if index_from_end { count - 1 } else { 0 };
    p.call_index("arguments", index, |p| p.print_current(args))
        .unwrap_or(Ok(empty()))
}

fn is_bailout(e: &Unsupported) -> bool {
    e.0 == ARG_EXPANSION_BAILOUT
}

pub fn print_call_arguments(p: &mut Printer<'_>) -> R {
    let node = p.node();
    let args = call_arguments(node);
    if args.is_empty() {
        return Ok(concat(vec![
            s("("),
            p.print_dangling(node, false, None),
            s(")"),
        ]));
    }
    let last_index = args.len() - 1;
    if is_react_hook_call_with_deps_array(&args, &p.comments) {
        let mut parts = vec![s("(")];
        iterate_call_arguments(p, |p, i| {
            parts.push(p.print_current(Args::default())?);
            if i != last_index {
                parts.push(s(", "));
            }
            Ok(())
        })?;
        parts.push(s(")"));
        return Ok(concat(parts));
    }
    let text = p.text;
    let mut any_arg_empty_line = false;
    let mut printed: Vec<Doc> = Vec::new();
    iterate_call_arguments(p, |p, i| {
        let arg = p.node();
        let doc = p.print_current(Args::default())?;
        if i == last_index {
            printed.push(doc);
        } else if is_next_line_empty(arg, text) {
            any_arg_empty_line = true;
            printed.push(concat(vec![doc, s(","), hardline(), hardline()]));
        } else {
            printed.push(concat(vec![doc, s(","), line()]));
        }
        Ok(())
    })?;
    let trailing = if !node.is("ImportExpression")
        && !node.is("TSImportType")
        && p.opts.should_print_comma(false)
    {
        ","
    } else {
        ""
    };
    if node.is("TSImportType")
        && args.len() == 1
        && (is_string_literal(args[0])
            || (args[0].is("TSLiteralType")
                && args[0].field("literal").is_some_and(is_string_literal)))
        && !p.has_any_comment(args[0])
    {
        let mut parts = vec![s("(")];
        parts.extend(printed);
        parts.push(if_break(s(trailing), empty()));
        parts.push(s(")"));
        return Ok(group(concat(parts)));
    }
    let all_broken_out = |printed: &[Doc]| {
        let mut inner = vec![line()];
        inner.extend(printed.iter().cloned());
        group_with(
            concat(vec![
                s("("),
                indent(concat(inner)),
                s(trailing),
                line(),
                s(")"),
            ]),
            true,
            None,
        )
    };
    let parent_is_decorator = p.parent().is_some_and(|pa| pa.is("Decorator"));
    if any_arg_empty_line || (!parent_is_decorator && is_function_composition_args(&args)) {
        return Ok(all_broken_out(&printed));
    }
    if should_expand_first_arg(&args, &p.comments) {
        let tail = &printed[1..];
        if tail.iter().any(will_break) {
            return Ok(all_broken_out(&printed));
        }
        let first = match print_argument(
            p,
            false,
            Args {
                expand_first_arg: true,
                ..Args::default()
            },
        ) {
            Ok(d) => d,
            Err(e) if is_bailout(&e) => return Ok(all_broken_out(&printed)),
            Err(e) => return Err(e),
        };
        let with = |first: Doc| {
            let mut v = vec![s("("), first, s(", ")];
            v.extend(tail.iter().cloned());
            v.push(s(")"));
            concat(v)
        };
        if will_break(&first) {
            return Ok(concat(vec![
                break_parent(),
                conditional_group(
                    vec![
                        with(group_with(first, true, None)),
                        all_broken_out(&printed),
                    ],
                    false,
                ),
            ]));
        }
        return Ok(conditional_group(
            vec![
                with(first.clone()),
                with(group_with(first, true, None)),
                all_broken_out(&printed),
            ],
            false,
        ));
    }
    if should_expand_last_arg(&args, p.opts, &p.comments, p.text) {
        let head = &printed[..printed.len() - 1];
        if head.iter().any(will_break) {
            return Ok(all_broken_out(&printed));
        }
        let last = match print_argument(
            p,
            true,
            Args {
                expand_last_arg: true,
                ..Args::default()
            },
        ) {
            Ok(d) => d,
            Err(e) if is_bailout(&e) => return Ok(all_broken_out(&printed)),
            Err(e) => return Err(e),
        };
        let with = |last: Doc| {
            let mut v = vec![s("(")];
            v.extend(head.iter().cloned());
            v.push(last);
            v.push(s(")"));
            concat(v)
        };
        if will_break(&last) {
            return Ok(concat(vec![
                break_parent(),
                conditional_group(
                    vec![with(group_with(last, true, None)), all_broken_out(&printed)],
                    false,
                ),
            ]));
        }
        return Ok(conditional_group(
            vec![
                with(last.clone()),
                with(group_with(last, true, None)),
                all_broken_out(&printed),
            ],
            false,
        ));
    }
    let should_break = printed.iter().any(will_break) || any_arg_empty_line;
    let mut inner = vec![softline()];
    inner.extend(printed);
    let contents = concat(vec![
        s("("),
        indent(concat(inner)),
        if_break(s(trailing), empty()),
        softline(),
        s(")"),
    ]);
    if is_long_curried_call_expression(p) {
        return Ok(contents);
    }
    Ok(group_with(contents, should_break, None))
}

fn could_expand_arg(arg: &Value, arrow_chain_recursion: bool, c: &Comments) -> bool {
    (arg.is("ObjectExpression") && (!arg.list("properties").is_empty() || c.has_any(arg)))
        || (arg.is("ArrayExpression") && (!arg.list("elements").is_empty() || c.has_any(arg)))
        || (arg.is("TSTypeAssertion")
            && arg
                .field("expression")
                .is_some_and(|e| could_expand_arg(e, false, c)))
        || (is_binary_cast_expression(arg)
            && arg
                .field("expression")
                .is_some_and(|e| could_expand_arg(e, false, c)))
        || arg.is("FunctionExpression")
        || (arg.is("ArrowFunctionExpression") && {
            let body = arg.field("body");
            let return_ok = match arg
                .field("returnType")
                .and_then(|r| r.field("typeAnnotation"))
            {
                None => true,
                Some(t) => {
                    !t.is("TSTypeReference")
                        || body.is_some_and(|b| is_non_empty_block_statement(b, c))
                }
            };
            return_ok
                && body.is_some_and(|b| {
                    b.is("BlockStatement")
                        || (b.is("ArrowFunctionExpression") && could_expand_arg(b, true, c))
                        || b.is("ObjectExpression")
                        || b.is("ArrayExpression")
                        || (!arrow_chain_recursion
                            && (is_call_expression(b) || b.is("ConditionalExpression")))
                        || is_jsx_element(b)
                })
        })
        || arg.is("DoExpression")
        || arg.is("ModuleExpression")
}

fn is_non_empty_block_statement(n: &Value, c: &Comments) -> bool {
    n.is("BlockStatement")
        && (n.list("body").iter().any(|s| !s.is("EmptyStatement")) || c.has(n, Flags::DANGLING))
}

fn should_expand_last_arg(
    args: &[&Value],
    opts: &crate::Options,
    c: &Comments,
    text: &str,
) -> bool {
    let last = args[args.len() - 1];
    let penultimate = if args.len() >= 2 {
        Some(args[args.len() - 2])
    } else {
        None
    };
    !c.has(last, Flags::LEADING)
        && !c.has(last, Flags::TRAILING)
        && could_expand_arg(last, false, c)
        && penultimate.is_none_or(|pe| pe.ty() != last.ty())
        && (args.len() != 2
            || !penultimate.is_some_and(|pe| pe.is("ArrowFunctionExpression"))
            || !last.is("ArrayExpression"))
        && !(args.len() > 1 && crate::array::is_concisely_printed_array(last, opts, c, text))
}

fn should_expand_first_arg(args: &[&Value], c: &Comments) -> bool {
    if args.len() != 2 {
        return false;
    }
    let (first, second) = (args[0], args[1]);
    !c.has_any(first)
        && (first.is("FunctionExpression")
            || (first.is("ArrowFunctionExpression")
                && first.field("body").is_some_and(|b| b.is("BlockStatement"))))
        && !second.is("FunctionExpression")
        && !second.is("ArrowFunctionExpression")
        && !second.is("ConditionalExpression")
        && is_hopefully_short_call_argument(second)
        && !could_expand_arg(second, false, c)
}

fn is_hopefully_short_call_argument(n: &Value) -> bool {
    if n.is("ParenthesizedExpression") {
        return n
            .field("expression")
            .is_some_and(is_hopefully_short_call_argument);
    }
    if is_binary_cast_expression(n) || n.is("TypeCastExpression") {
        let Some(mut annotation) = n.field("typeAnnotation") else {
            return false;
        };
        if annotation.is("TypeAnnotation") {
            annotation = annotation.field("typeAnnotation").unwrap_or(annotation);
        }
        if annotation.is("TSArrayType") {
            annotation = annotation.field("elementType").unwrap_or(annotation);
            if annotation.is("TSArrayType") {
                annotation = annotation.field("elementType").unwrap_or(annotation);
            }
        }
        if annotation.is("GenericTypeAnnotation") || annotation.is("TSTypeReference") {
            let params = annotation
                .field("typeArguments")
                .or_else(|| annotation.field("typeParameters"))
                .map(|t| t.list("params"));
            if let Some(params) = params
                && params.len() == 1
            {
                annotation = &params[0];
            }
        }
        return crate::typescript::is_simple_type(annotation)
            && n.field("expression")
                .is_some_and(|e| is_simple_call_argument(e, 1));
    }
    if is_call_like_expression(n) && call_arguments(n).len() > 1 {
        return false;
    }
    if is_binaryish(n) {
        return n
            .field("left")
            .is_some_and(|l| is_simple_call_argument(l, 1))
            && n.field("right")
                .is_some_and(|r| is_simple_call_argument(r, 1));
    }
    is_regexp_literal(n) || is_simple_call_argument(n, 2)
}

fn is_react_hook_call_with_deps_array(args: &[&Value], c: &Comments) -> bool {
    let valid = |base: usize| {
        let f = args[base];
        let deps = args[base + 1];
        f.is("ArrowFunctionExpression")
            && function_parameters(f).is_empty()
            && f.field("body").is_some_and(|b| b.is("BlockStatement"))
            && deps.is("ArrayExpression")
            && !args.iter().any(|a| c.has_any(a))
    };
    match args.len() {
        2 => valid(0),
        3 => args[0].is("Identifier") && valid(1),
        _ => false,
    }
}

struct Printed<'a> {
    node: &'a Value,
    printed: Doc,
    needs_parens: bool,
    trailing_empty_line: bool,
}

fn should_insert_empty_line_after(n: &Value, text: &str) -> bool {
    let next = crate::text::next_non_space_non_comment_index(text, n.end());
    if let Some(i) = next
        && text[i..].starts_with(')')
    {
        return crate::text::is_next_line_empty(text, i + 1);
    }
    is_next_line_empty(n, text)
}

pub fn print_member_chain(p: &mut Printer<'_>) -> R {
    if p.node().is("ChainExpression") {
        return p
            .call("expression", print_member_chain)
            .unwrap_or(Ok(empty()));
    }
    let parent = p.parent();
    let stmt_holder = if parent.is_some_and(|pa| pa.is("ChainExpression")) {
        p.grandparent()
    } else {
        parent
    };
    let is_expression_statement = stmt_holder.is_some_and(|n| n.is("ExpressionStatement"));
    let text = p.text;
    let node = p.node();
    let mut printed_nodes: Vec<Printed<'_>> = Vec::new();
    let first = concat(vec![
        print_optional_token(p),
        crate::typescript::print_function_type_parameters(p)?,
        print_call_arguments(p)?,
    ]);
    printed_nodes.push(Printed {
        node,
        printed: first,
        needs_parens: false,
        trailing_empty_line: false,
    });
    let mut rest: Vec<Printed<'_>> = Vec::new();
    if node.field("callee").is_some() {
        p.call("callee", |p| rec(p, &mut rest, text))
            .unwrap_or(Ok(()))?;
    }
    rest.reverse();
    rest.extend(printed_nodes);
    let printed_nodes = rest;

    let mut groups: Vec<Vec<usize>> = Vec::new();
    let mut current = vec![0usize];
    let mut i = 1;
    while i < printed_nodes.len() {
        let n = printed_nodes[i].node;
        if n.is("TSNonNullExpression")
            || is_call_expression(n)
            || (is_member_expression(n)
                && n.flag("computed")
                && n.field("property").is_some_and(is_numeric_literal))
        {
            current.push(i);
            i += 1;
        } else {
            break;
        }
    }
    if !is_call_expression(printed_nodes[0].node) {
        while i + 1 < printed_nodes.len() {
            if is_memberish(printed_nodes[i].node) && is_memberish(printed_nodes[i + 1].node) {
                current.push(i);
                i += 1;
            } else {
                break;
            }
        }
    }
    groups.push(current);
    let mut current: Vec<usize> = Vec::new();
    let mut has_seen_call = false;
    while i < printed_nodes.len() {
        let n = printed_nodes[i].node;
        if has_seen_call && is_memberish(n) {
            if n.flag("computed") && n.field("property").is_some_and(is_numeric_literal) {
                current.push(i);
                i += 1;
                continue;
            }
            groups.push(std::mem::take(&mut current));
            has_seen_call = false;
        }
        if is_call_expression(n) || n.is("ImportExpression") {
            has_seen_call = true;
        }
        current.push(i);
        i += 1;
        if p.has_comment(n, Flags::TRAILING) {
            groups.push(std::mem::take(&mut current));
            has_seen_call = false;
        }
    }
    if !current.is_empty() {
        groups.push(current);
    }

    let is_factory = |name: &str| {
        name.starts_with(|c: char| c.is_ascii_uppercase())
            || (!name.is_empty() && name.chars().all(|c| c == '$' || c == '_'))
    };
    let tab_width = p.opts.tab_width;
    let is_short = |name: &str| name.encode_utf16().count() <= tab_width;
    let should_not_wrap = |groups: &Vec<Vec<usize>>| {
        let has_computed = groups[1]
            .first()
            .is_some_and(|&j| printed_nodes[j].node.flag("computed"));
        if groups[0].len() == 1 {
            let first = printed_nodes[groups[0][0]].node;
            return first.is("ThisExpression")
                || (first.is("Identifier") && {
                    let name = first.str_of("name").unwrap_or("");
                    is_factory(name) || (is_expression_statement && is_short(name)) || has_computed
                });
        }
        let last = printed_nodes[*groups[0].last().unwrap_or(&0)].node;
        is_member_expression(last)
            && last.field("property").is_some_and(|pr| {
                pr.is("Identifier") && (is_factory(pr.str_of("name").unwrap_or("")) || has_computed)
            })
    };
    let should_merge = groups.len() >= 2
        && !groups[1]
            .first()
            .is_some_and(|&j| p.has_any_comment(printed_nodes[j].node))
        && should_not_wrap(&groups);

    let print_group = |g: &[usize]| -> Doc {
        let docs: Vec<Doc> = g
            .iter()
            .map(|&j| printed_nodes[j].printed.clone())
            .collect();
        if g.last().is_some_and(|&j| printed_nodes[j].needs_parens) {
            let mut v = vec![s("(")];
            v.extend(docs);
            v.push(s(")"));
            return concat(v);
        }
        concat(docs)
    };
    let print_indented_group = |gs: &[Vec<usize>]| -> Doc {
        if gs.is_empty() {
            return empty();
        }
        indent(concat(vec![
            hardline(),
            join(&hardline(), gs.iter().map(|g| print_group(g)).collect()),
        ]))
    };
    let printed_groups: Vec<Doc> = groups.iter().map(|g| print_group(g)).collect();
    let one_line = concat(printed_groups.clone());
    let cutoff = if should_merge { 3 } else { 2 };
    let flat: Vec<usize> = groups.iter().flatten().copied().collect();
    let node_has_comment = flat
        .get(1..flat.len().saturating_sub(1))
        .unwrap_or(&[])
        .iter()
        .any(|&j| p.has_comment(printed_nodes[j].node, Flags::LEADING))
        || flat[..flat.len().saturating_sub(1)]
            .iter()
            .any(|&j| p.has_comment(printed_nodes[j].node, Flags::TRAILING))
        || groups
            .get(cutoff)
            .and_then(|g| g.first())
            .is_some_and(|&j| p.has_comment(printed_nodes[j].node, Flags::LEADING));
    if groups.len() <= cutoff
        && !node_has_comment
        && !groups.iter().any(|g| {
            g.last()
                .is_some_and(|&j| printed_nodes[j].trailing_empty_line)
        })
    {
        if is_long_curried_call_expression(p) {
            return Ok(one_line);
        }
        return Ok(group(one_line));
    }
    let last_before_indent = printed_nodes[*groups[if should_merge { 1 } else { 0 }]
        .last()
        .unwrap_or(&0)]
    .node;
    let empty_line_before_indent = !is_call_expression(last_before_indent)
        && should_insert_empty_line_after(last_before_indent, text);
    let expanded = concat(vec![
        print_group(&groups[0]),
        if should_merge {
            print_group(&groups[1])
        } else {
            empty()
        },
        if empty_line_before_indent {
            hardline()
        } else {
            empty()
        },
        print_indented_group(&groups[if should_merge { 2 } else { 1 }..]),
    ]);
    let call_expressions: Vec<&Value> = printed_nodes
        .iter()
        .map(|t| t.node)
        .filter(|n| is_call_expression(n))
        .collect();
    let last_group_will_break_and_other_calls_have_function_arguments = || {
        let last_node = printed_nodes[*groups.last().and_then(|g| g.last()).unwrap_or(&0)].node;
        let last_doc = printed_groups.last();
        is_call_expression(last_node)
            && last_doc.is_some_and(will_break)
            && call_expressions[..call_expressions.len().saturating_sub(1)]
                .iter()
                .any(|n| n.list("arguments").iter().any(is_function_or_arrow))
    };
    let result = if node_has_comment
        || (call_expressions.len() > 2
            && call_expressions.iter().any(|e| {
                !e.list("arguments")
                    .iter()
                    .all(|a| is_simple_call_argument(a, 2))
            }))
        || printed_groups[..printed_groups.len() - 1]
            .iter()
            .any(will_break)
        || last_group_will_break_and_other_calls_have_function_arguments()
    {
        group(expanded)
    } else {
        concat(vec![
            if will_break(&one_line) || empty_line_before_indent {
                break_parent()
            } else {
                empty()
            },
            conditional_group(vec![one_line, expanded], false),
        ])
    };
    Ok(label("member-chain", result))
}

fn rec<'a>(p: &mut Printer<'a>, out: &mut Vec<Printed<'a>>, text: &str) -> R<()> {
    let node = p.node();
    if node.is("ChainExpression") {
        return p
            .call("expression", |p| rec(p, out, text))
            .unwrap_or(Ok(()));
    }
    if is_call_expression(node)
        && node
            .field("callee")
            .is_some_and(|c| is_memberish(c) || is_call_expression(c))
    {
        let trailing = should_insert_empty_line_after(node, text);
        let inner = concat(vec![
            print_optional_token(p),
            crate::typescript::print_function_type_parameters(p)?,
            print_call_arguments(p)?,
        ]);
        let printed = concat(vec![
            crate::comments::print_comments(&p.comments, node, inner, text),
            if trailing { hardline() } else { empty() },
        ]);
        out.push(Printed {
            node,
            printed,
            needs_parens: false,
            trailing_empty_line: trailing,
        });
        return p.call("callee", |p| rec(p, out, text)).unwrap_or(Ok(()));
    }
    if is_memberish(node) {
        let needs_parens = crate::parens::needs_parens(p)?;
        let printed = if is_member_expression(node) {
            print_member_lookup(p)?
        } else {
            return bail("bind");
        };
        let printed = crate::comments::print_comments(&p.comments, node, printed, text);
        out.push(Printed {
            node,
            printed,
            needs_parens,
            trailing_empty_line: false,
        });
        return p.call("object", |p| rec(p, out, text)).unwrap_or(Ok(()));
    }
    if node.is("TSNonNullExpression") {
        out.push(Printed {
            node,
            printed: crate::comments::print_comments(&p.comments, node, s("!"), text),
            needs_parens: false,
            trailing_empty_line: false,
        });
        return p
            .call("expression", |p| rec(p, out, text))
            .unwrap_or(Ok(()));
    }
    let printed = p.print_current(Args::default())?;
    out.push(Printed {
        node,
        printed,
        needs_parens: false,
        trailing_empty_line: false,
    });
    Ok(())
}
