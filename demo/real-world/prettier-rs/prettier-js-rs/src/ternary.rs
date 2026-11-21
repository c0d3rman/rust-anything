//! `src/language-js/print/ternary.js` and `ternary-old.js`, for `ConditionalExpression`.

use crate::value::Value;

use crate::doc::*;
use crate::node::*;
use crate::printer::{Args, Printer, R};
use crate::utils::*;

const COND: &str = "ConditionalExpression";

fn ancestor_key(ty: &str) -> Option<&'static str> {
    Some(match ty {
        "AssignmentExpression" => "right",
        "VariableDeclarator" => "init",
        "ReturnStatement" | "ThrowStatement" | "UnaryExpression" | "YieldExpression"
        | "AwaitExpression" => "argument",
        _ => return None,
    })
}

fn should_extra_indent(p: &Printer<'_>) -> bool {
    let node = p.node();
    if !node.is(COND) {
        return false;
    }
    let mut child = node;
    let mut level = 1;
    let parent = loop {
        let Some(n) = p.ancestor(level) else {
            return false;
        };
        let is = |k: &str| n.field(k).is_some_and(|c| c.same(child));
        if (n.is("ChainExpression") && is("expression"))
            || (is_call_expression(n) && is("callee"))
            || (is_member_expression(n) && is("object"))
            || (n.is("TSNonNullExpression") && is("expression"))
        {
            child = n;
            level += 1;
            continue;
        }
        if (n.is("NewExpression") && is("callee"))
            || (is_binary_cast_expression(n) && is("expression"))
        {
            child = n;
            break p.ancestor(level + 1);
        }
        break Some(n);
    };
    if child.same(node) {
        return false;
    }
    parent.is_some_and(|pa| {
        ancestor_key(pa.ty())
            .and_then(|k| pa.field(k))
            .is_some_and(|c| c.same(child))
    })
}

fn test_keys(n: &Value) -> &'static [&'static str] {
    if n.is(COND) {
        &["test"]
    } else {
        &["checkType", "extendsType"]
    }
}

fn is_in_test(parent: &Value, node: &Value) -> bool {
    parent.ty() == node.ty() && test_keys(node).iter().any(|k| same_field(parent, k, node))
}

fn first_non_conditional_parent<'a>(p: &Printer<'a>) -> (Option<&'a Value>, &'a Value) {
    let node = p.node();
    let ty = node.ty();
    let mut current: Option<&'a Value> = None;
    let mut previous: &'a Value;
    let mut level = 1;
    loop {
        previous = current.unwrap_or(node);
        current = p.ancestor(level);
        level += 1;
        let Some(c) = current else { break };
        if !(c.ty() == ty && !test_keys(node).iter().any(|k| same_field(c, k, previous))) {
            break;
        }
    }
    (current.or(p.parent()), previous)
}

fn chain_contains_jsx(node: &Value) -> bool {
    let mut list = vec![node];
    let mut i = 0;
    while i < list.len() {
        let c = list[i];
        for k in ["test", "consequent", "alternate"] {
            if let Some(n) = c.field(k) {
                if is_jsx_element(n) {
                    return true;
                }
                if n.is(COND) {
                    list.push(n);
                }
            }
        }
        i += 1;
    }
    false
}

fn wrap_in_parens(doc: Doc) -> Doc {
    concat(vec![
        if_break(s("("), empty()),
        indent(concat(vec![softline(), doc])),
        softline(),
        if_break(s(")"), empty()),
    ])
}

fn is_nil(n: &Value) -> bool {
    (n.is("Literal")
        && n.get("value").is_some_and(Value::is_null)
        && n.get("regex").is_none()
        && n.get("bigint").is_none()
        && n.str_of("raw") == Some("null"))
        || n.is("NullLiteral")
        || (n.is("Identifier") && n.str_of("name") == Some("undefined"))
}

fn same_field(n: &Value, key: &str, other: &Value) -> bool {
    n.field(key).is_some_and(|c| c.same(other))
}

pub fn print_ternary(p: &mut Printer<'_>, args: &Args) -> R {
    if p.opts.experimental_ternaries {
        return print_ternary_new(p, args);
    }
    let node = p.node();
    let ty = node.ty();
    let is_ce = node.is(COND);
    let (cons_key, alt_key) = if is_ce {
        ("consequent", "alternate")
    } else {
        ("trueType", "falseType")
    };
    let parent = p.parent().unwrap_or(node);
    let consequent_node = node.field(cons_key).unwrap_or(node);
    let alternate_node = node.field(alt_key).unwrap_or(node);
    let is_parent_test = is_in_test(parent, node);
    let mut force_no_indent = parent.ty() == ty && !is_parent_test;
    let (first_non_cond, last_cond) = first_non_conditional_parent(p);
    let mut jsx_mode = false;
    let mut parts = Vec::new();
    if is_ce
        && (node.field("test").is_some_and(is_jsx_element)
            || is_jsx_element(consequent_node)
            || is_jsx_element(alternate_node)
            || chain_contains_jsx(last_cond))
    {
        jsx_mode = true;
        force_no_indent = true;
        let cons = p.print("consequent")?;
        let alt = p.print("alternate")?;
        parts.push(s(" ? "));
        parts.push(if is_nil(consequent_node) {
            cons
        } else {
            wrap_in_parens(cons)
        });
        parts.push(s(" : "));
        parts.push(if alternate_node.is(COND) || is_nil(alternate_node) {
            alt
        } else {
            wrap_in_parens(alt)
        });
    } else {
        let use_tabs = p.opts.use_tabs;
        let branch = |d: Doc| if use_tabs { indent(d) } else { align(2.0, d) };
        let cons_is_cond = consequent_node.ty() == ty;
        let cons = branch(p.print(cons_key)?);
        let alt = branch(p.print(alt_key)?);
        let part = concat(vec![
            line(),
            s("? "),
            if cons_is_cond {
                if_break(empty(), s("("))
            } else {
                empty()
            },
            cons,
            if cons_is_cond {
                if_break(empty(), s(")"))
            } else {
                empty()
            },
            line(),
            s(": "),
            alt,
        ]);
        parts.push(
            if parent.ty() != ty || same_field(parent, alt_key, node) || is_parent_test {
                part
            } else if use_tabs {
                dedent(indent(part))
            } else {
                align(p.opts.tab_width.saturating_sub(2) as f64, part)
            },
        );
    }
    let text = p.text;
    let mut keys = vec![cons_key, alt_key];
    keys.extend(test_keys(node));
    let should_break = keys.iter().any(|k| {
        node.field(k).is_some_and(|n| {
            p.comments.has_with(n, crate::comments::Flags::ANY, &|c| {
                c.block && crate::text::has_newline_in_range(text, c.start, c.end)
            })
        })
    });
    let break_closing_paren = !jsx_mode && is_member_expression(parent) && !parent.flag("computed");
    let extra = should_extra_indent(p);
    let printed_test = if is_ce {
        p.print("test")?
    } else {
        concat(vec![
            p.print("checkType")?,
            s(" extends "),
            p.print("extendsType")?,
        ])
    };
    let test = if parent.ty() == ty && same_field(parent, alt_key, node) {
        align(2.0, printed_test)
    } else {
        printed_test
    };
    let contents = concat(vec![
        test,
        if force_no_indent {
            concat(parts)
        } else {
            indent(concat(parts))
        },
        if is_ce && break_closing_paren && !extra {
            softline()
        } else {
            empty()
        },
    ]);
    let result = if first_non_cond.is_some_and(|f| f.same(parent)) {
        group_with(contents, should_break, None)
    } else if should_break {
        concat(vec![contents, break_parent()])
    } else {
        contents
    };
    Ok(if is_parent_test || extra {
        group(concat(vec![
            indent(concat(vec![softline(), result])),
            softline(),
        ]))
    } else {
        result
    })
}

fn print_ternary_new(p: &mut Printer<'_>, args: &Args) -> R {
    let node = p.node();
    let ty = node.ty();
    let is_ce = node.is(COND);
    let is_ts = !is_ce;
    let (cons_key, alt_key) = if is_ce {
        ("consequent", "alternate")
    } else {
        ("trueType", "falseType")
    };
    let parent = p.parent().unwrap_or(node);
    let consequent_node = node.field(cons_key).unwrap_or(node);
    let alternate_node = node.field(alt_key).unwrap_or(node);
    let test_node = node.field("test").unwrap_or(node);
    let test_nodes: Vec<&Value> = test_keys(node)
        .iter()
        .filter_map(|k| node.field(k))
        .collect();
    let is_parent_ternary = parent.ty() == ty;
    let in_test = is_in_test(parent, node);
    let is_in_alternate = is_parent_ternary && same_field(parent, alt_key, node);
    let is_consequent_ternary = consequent_node.ty() == ty;
    let is_alternate_ternary = alternate_node.ty() == ty;
    let is_in_chain = is_alternate_ternary || is_in_alternate;
    let is_big_tabs = p.opts.tab_width > 2 || p.opts.use_tabs;
    let (first_non_cond, _) = first_non_conditional_parent(p);
    let first_non_cond = first_non_cond.unwrap_or(parent);
    let is_on_same_line_as_assignment = args
        .assignment_layout
        .is_some_and(|l| l != "break-after-operator")
        && matches!(
            parent.ty(),
            "AssignmentExpression"
                | "VariableDeclarator"
                | "ClassProperty"
                | "PropertyDefinition"
                | "ClassPrivateProperty"
                | "ObjectProperty"
                | "Property"
        );
    let is_on_same_line_as_return = (parent.is("ReturnStatement") || parent.is("ThrowStatement"))
        && !(is_consequent_ternary || is_alternate_ternary);
    let is_in_jsx = is_ce
        && first_non_cond.is("JSXExpressionContainer")
        && !p.grandparent().is_some_and(|g| g.is("JSXAttribute"));
    let extra = should_extra_indent(p);
    let break_closing_paren = is_member_expression(parent) && !parent.flag("computed");
    let break_ts_closing_paren = is_ts && crate::parens::needs_parens(p)?;
    let fill_tab = if !is_big_tabs {
        String::new()
    } else if p.opts.use_tabs {
        "\t".to_string()
    } else {
        " ".repeat(p.opts.tab_width - 1)
    };
    let text = p.text;
    let mut break_nodes = test_nodes.clone();
    break_nodes.push(consequent_node);
    break_nodes.push(alternate_node);
    let should_break = break_nodes.iter().any(|n| {
        p.comments.has_with(n, crate::comments::Flags::ANY, &|c| {
            c.block && crate::text::has_newline_in_range(text, c.start, c.end)
        })
    }) || is_consequent_ternary
        || is_alternate_ternary;
    let try_parenthesize_alternate = !is_in_chain
        && !is_parent_ternary
        && !is_ts
        && if is_in_jsx {
            is_nil(consequent_node) && !consequent_node.is("Identifier")
        } else {
            is_lone_short_argument(consequent_node, p.opts.print_width, p.opts, &p.comments)
                && is_simple_expression_by_node_count(test_node, 3)
        };
    let should_group_test_and_consequent = is_in_chain
        || is_in_alternate
        || (is_ts && !is_parent_ternary)
        || (is_parent_ternary && is_ce && is_simple_expression_by_node_count(test_node, 1))
        || try_parenthesize_alternate;
    // Prettier prints these through `path.call(..., "consequent")` and `"alternate"`, keys a
    // conditional type does not have (it has `trueType` and `falseType`), so for one it prints
    // nothing there and leaves the comments to the branch.
    let dangling_of = |p: &mut Printer<'_>, n: &Value| {
        if is_ce {
            p.print_dangling(n, false, None)
        } else {
            empty()
        }
    };
    let mut consequent_comments = Vec::new();
    if !is_consequent_ternary && p.has_comment(consequent_node, crate::comments::Flags::DANGLING) {
        consequent_comments.push(dangling_of(p, consequent_node));
        consequent_comments.push(hardline());
    }
    let mut alternate_comments = Vec::new();
    if is_ce && p.has_comment(test_node, crate::comments::Flags::DANGLING) {
        alternate_comments.push(p.print_dangling(test_node, false, None));
    }
    if !is_alternate_ternary && p.has_comment(alternate_node, crate::comments::Flags::DANGLING) {
        alternate_comments.push(dangling_of(p, alternate_node));
    }
    if p.has_comment(node, crate::comments::Flags::DANGLING) {
        alternate_comments.push(p.print_dangling(node, false, None));
    }
    let test_id = p.new_id("test");
    let consequent_id = p.new_id("consequent");
    let test_and_consequent_id = p.new_id("test-and-consequent");
    let printed_test = if is_ce {
        concat(vec![
            wrap_in_parens(p.print("test")?),
            if test_node.is(COND) {
                break_parent()
            } else {
                empty()
            },
        ])
    } else {
        let extends = node.field("extendsType").unwrap_or(node);
        let check = p.print("checkType")?;
        let printed_extends = p.print("extendsType")?;
        concat(vec![
            check,
            s(" extends "),
            if extends.is("TSConditionalType") || extends.is("TSMappedType") {
                printed_extends
            } else {
                group(wrap_in_parens(printed_extends))
            },
        ])
    };
    let printed_test_q = group_with(
        concat(vec![printed_test, s(" ?")]),
        false,
        Some(test_id.clone()),
    );
    let printed_consequent = p.print(cons_key)?;
    let consequent = indent(concat(vec![
        if is_consequent_ternary
            || (is_in_jsx && (is_jsx_element(consequent_node) || is_parent_ternary || is_in_chain))
        {
            hardline()
        } else {
            line()
        },
        concat(consequent_comments),
        printed_consequent,
    ]));
    let printed_test_and_consequent = if should_group_test_and_consequent {
        group_with(
            concat(vec![
                printed_test_q,
                if is_in_chain {
                    consequent
                } else {
                    if_break_id(
                        consequent.clone(),
                        group_with(consequent, false, Some(consequent_id)),
                        Some(test_id),
                    )
                },
            ]),
            false,
            Some(test_and_consequent_id.clone()),
        )
    } else {
        concat(vec![printed_test_q, consequent])
    };
    let printed_alternate = p.print(alt_key)?;
    let alt = if try_parenthesize_alternate {
        if_break_id(
            printed_alternate.clone(),
            dedent(wrap_in_parens(printed_alternate)),
            Some(test_and_consequent_id.clone()),
        )
    } else {
        printed_alternate
    };
    let parts = concat(vec![
        printed_test_and_consequent,
        if !alternate_comments.is_empty() {
            concat(vec![
                indent(concat(vec![hardline(), concat(alternate_comments)])),
                hardline(),
            ])
        } else if is_alternate_ternary {
            hardline()
        } else if try_parenthesize_alternate {
            if_break_id(line(), s(" "), Some(test_and_consequent_id.clone()))
        } else {
            line()
        },
        s(":"),
        if is_alternate_ternary || !is_big_tabs {
            s(" ")
        } else if should_group_test_and_consequent {
            if_break_id(
                s(fill_tab.clone()),
                if_break(
                    s(if is_in_chain || try_parenthesize_alternate {
                        " ".to_string()
                    } else {
                        fill_tab.clone()
                    }),
                    s(" "),
                ),
                Some(test_and_consequent_id),
            )
        } else {
            if_break(s(fill_tab), s(" "))
        },
        if is_alternate_ternary {
            alt
        } else {
            group(concat(vec![
                indent(alt),
                if is_in_jsx && !try_parenthesize_alternate {
                    softline()
                } else {
                    empty()
                },
            ]))
        },
        if break_closing_paren && !extra {
            softline()
        } else {
            empty()
        },
        if should_break {
            break_parent()
        } else {
            empty()
        },
    ]);
    Ok(if is_on_same_line_as_assignment && !should_break {
        group(indent(concat(vec![softline(), group(parts)])))
    } else if is_on_same_line_as_assignment || is_on_same_line_as_return {
        group(indent(parts))
    } else if extra || (is_ts && in_test) {
        group(concat(vec![
            indent(concat(vec![softline(), parts])),
            if break_ts_closing_paren {
                softline()
            } else {
                empty()
            },
        ]))
    } else if parent.same(first_non_cond) {
        group(parts)
    } else {
        parts
    })
}
