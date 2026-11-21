//! `src/language-js/print/typescript.js`, `type-annotation.js`, `type-parameters.js`,
//! `interface.js`, `enum.js`, `mapped-type.js`, `cast-expression.js` and the TypeScript tokens of
//! `misc.js`.

use crate::value::Value;

use crate::comments::Flags;
use crate::doc::*;
use crate::node::*;
use crate::printer::{Args, Printer, R, bail};
use crate::utils::*;

pub fn is_ts_keyword_type(n: &Value) -> bool {
    let ty = n.ty();
    ty.starts_with("TS") && ty.ends_with("Keyword")
}

pub fn is_simple_type(n: &Value) -> bool {
    is_ts_keyword_type(n)
        || matches!(
            n.ty(),
            "TSThisType" | "TSLiteralType" | "TSTemplateLiteralType"
        )
        || (n.is("TSTypeReference")
            && n.field("typeParameters").is_none()
            && n.field("typeArguments").is_none())
}

pub fn is_object_type(n: &Value) -> bool {
    matches!(
        n.ty(),
        "ObjectTypeAnnotation" | "TSTypeLiteral" | "TSMappedType"
    )
}

pub fn is_union_type(n: &Value) -> bool {
    matches!(n.ty(), "TSUnionType" | "UnionTypeAnnotation")
}

fn is_void_type(n: &Value) -> bool {
    matches!(
        n.ty(),
        "VoidTypeAnnotation" | "TSVoidKeyword" | "NullLiteralTypeAnnotation" | "TSNullKeyword"
    )
}

fn is_object_like_type(n: &Value) -> bool {
    matches!(
        n.ty(),
        "ObjectTypeAnnotation" | "TSTypeLiteral" | "GenericTypeAnnotation" | "TSTypeReference"
    )
}

pub fn should_hug_type(p: &Printer<'_>, n: &Value) -> bool {
    if is_simple_type(n) || is_object_type(n) {
        return true;
    }
    if is_union_type(n) {
        let types = n.list("types");
        if types.iter().any(|t| p.has_any_comment(t)) {
            return false;
        }
        let Some(object) = types.iter().find(|t| is_object_like_type(t)) else {
            return false;
        };
        return types.iter().all(|t| t.same(object) || is_void_type(t));
    }
    false
}

pub fn print_optional_token(p: &Printer<'_>) -> Doc {
    crate::member::print_optional_token(p)
}

pub fn print_definite_token(p: &Printer<'_>) -> Doc {
    let node = p.node();
    if node.flag("definite")
        || p.matches(&[
            None,
            Some(&|n, k| k == Some("id") && n.is("VariableDeclarator") && n.flag("definite")),
        ])
    {
        s("!")
    } else {
        empty()
    }
}

pub fn print_declare_token(p: &Printer<'_>) -> Doc {
    s(if p.node().flag("declare") {
        "declare "
    } else {
        ""
    })
}

pub fn print_abstract_token(p: &Printer<'_>) -> Doc {
    let node = p.node();
    s(
        if node.flag("abstract")
            || matches!(
                node.ty(),
                "TSAbstractMethodDefinition"
                    | "TSAbstractPropertyDefinition"
                    | "TSAbstractAccessorProperty"
            )
        {
            "abstract "
        } else {
            ""
        },
    )
}

pub fn print_accessibility_token(node: &Value) -> Doc {
    match node.str_of("accessibility") {
        Some(a) => s(format!("{a} ")),
        None => empty(),
    }
}

pub fn print_function_type_parameters(p: &mut Printer<'_>) -> R {
    let node = p.node();
    if node.field("typeArguments").is_some() {
        return p.print("typeArguments");
    }
    if node.field("typeParameters").is_some() {
        return p.print("typeParameters");
    }
    Ok(empty())
}

fn type_annotation_first_token(p: &Printer<'_>) -> &'static str {
    if p.matches(&[
        Some(&|n, _| n.is("TSTypeAnnotation")),
        Some(&|n, k| {
            matches!(k, Some("returnType" | "typeAnnotation"))
                && (n.is("TSFunctionType") || n.is("TSConstructorType"))
        }),
    ]) {
        return "=>";
    }
    if p.matches(&[
        Some(&|n, _| n.is("TSTypeAnnotation")),
        Some(&|n, k| {
            k == Some("typeAnnotation")
                && matches!(
                    n.ty(),
                    "TSJSDocNullableType" | "TSJSDocNonNullableType" | "TSTypePredicate"
                )
        }),
    ]) {
        return "";
    }
    ":"
}

pub fn print_type_annotation_property(p: &mut Printer<'_>, key: &'static str) -> R {
    let node = p.node();
    let Some(annotation) = node.field(key) else {
        return Ok(empty());
    };
    let mut leading_space = false;
    if annotation.is("TSTypeAnnotation") || annotation.is("TypeAnnotation") {
        let first = p
            .call(key, |p| type_annotation_first_token(p))
            .unwrap_or(":");
        if first == "=>" || (first == ":" && p.has_comment(annotation, Flags::LEADING)) {
            leading_space = true;
        }
    }
    let printed = p.print(key)?;
    Ok(if leading_space {
        concat(vec![s(" "), printed])
    } else {
        printed
    })
}

fn print_type_annotation(p: &mut Printer<'_>) -> R {
    let token = type_annotation_first_token(p);
    let printed = p.print("typeAnnotation")?;
    Ok(if token.is_empty() {
        printed
    } else {
        concat(vec![s(token), s(" "), printed])
    })
}

pub fn print_return_type(p: &mut Printer<'_>) -> R {
    let returned = print_type_annotation_property(p, "returnType")?;
    let mut parts = vec![returned];
    if p.node().field("predicate").is_some() {
        parts.push(p.print("predicate")?);
    }
    Ok(concat(parts))
}

pub fn print_type_parameters(p: &mut Printer<'_>, key: &'static str) -> R {
    let node = p.node();
    let Some(params_value) = node.get(key).filter(|v| !v.is_null()) else {
        return Ok(empty());
    };
    if !params_value.is_array() {
        return p.print(key);
    }
    let params = node.list(key);
    let in_test_call = p.grandparent().is_some_and(|g| is_test_call(g, None));
    let is_arrow_function_variable = p.matches(&[
        Some(&|n, _| !(n.list(key).len() == 1 && is_object_type(&n.list(key)[0]))),
        None,
        Some(&|_, k| k == Some("typeAnnotation")),
        Some(&|n, _| n.is("Identifier")),
        Some(&|n, _| crate::assignment::is_arrow_function_variable_declarator(n)),
    ]);
    let should_inline = params.is_empty()
        || (!is_arrow_function_variable
            && (in_test_call
                || (params.len() == 1
                    && (params[0].is("NullableTypeAnnotation")
                        || should_hug_type(p, &params[0])))));
    if should_inline {
        let printed = p.print_all(key)?;
        let dangling = print_dangling_for_inline(p);
        return Ok(concat(vec![
            s("<"),
            join(&s(", "), printed),
            dangling,
            s(">"),
        ]));
    }
    let trailing_comma = if node.is("TSTypeParameterInstantiation") {
        empty()
    } else if should_force_trailing_comma(p, key) {
        s(",")
    } else if p.opts.should_print_comma(true) {
        if_break(s(","), empty())
    } else {
        empty()
    };
    let printed = p.print_all(key)?;
    let id = p.group_id_for("typeParameters", node);
    Ok(group_with(
        concat(vec![
            s("<"),
            indent(concat(vec![
                softline(),
                join(&concat(vec![s(","), line()]), printed),
            ])),
            trailing_comma,
            softline(),
            s(">"),
        ]),
        false,
        Some(id),
    ))
}

fn should_force_trailing_comma(p: &Printer<'_>, key: &str) -> bool {
    let node = p.node();
    let params = node.list(key);
    params.len() == 1
        && node.ty().starts_with("TS")
        && params[0].field("constraint").is_none()
        && p.parent()
            .is_some_and(|pa| pa.is("ArrowFunctionExpression"))
        && !p.opts.filepath.ends_with(".ts")
}

fn print_dangling_for_inline(p: &Printer<'_>) -> Doc {
    let node = p.node();
    if !p.has_comment(node, Flags::DANGLING) {
        return empty();
    }
    let only_block = !p.has_comment(node, Flags::LINE);
    let printed = p.print_dangling(node, !only_block, None);
    if only_block {
        printed
    } else {
        concat(vec![printed, hardline()])
    }
}

fn print_type_parameter(p: &mut Printer<'_>) -> R {
    let node = p.node();
    let mut parts = vec![s(if node.flag("const") { "const " } else { "" })];
    if node.flag("in") {
        parts.push(s("in "));
    }
    if node.flag("out") {
        parts.push(s("out "));
    }
    parts.push(p.print("name")?);
    if node.field("constraint").is_some() {
        let id = p.new_id("constraint");
        let constraint = p.print("constraint")?;
        parts.push(s(" extends"));
        parts.push(group_with(indent(line()), false, Some(id.clone())));
        parts.push(line_suffix_boundary());
        parts.push(indent_if_break(constraint, Some(id), false));
    }
    if node.field("default").is_some() {
        parts.push(s(" = "));
        parts.push(p.print("default")?);
    }
    Ok(group(concat(parts)))
}

fn print_union_type(p: &mut Printer<'_>) -> R {
    let node = p.node();
    let parent = p.parent().unwrap_or(node);
    let text = p.text;
    let own_line = has_leading_own_line_comment(&p.comments, text, node);
    let should_indent = !matches!(
        parent.ty(),
        "TypeParameterInstantiation"
            | "TSTypeParameterInstantiation"
            | "GenericTypeAnnotation"
            | "TSTypeReference"
            | "TSTypeAssertion"
            | "TupleTypeAnnotation"
            | "TSTupleType"
    ) && !(parent.is("TSConditionalType") && p.opts.experimental_ternaries)
        && !(matches!(
            parent.ty(),
            "TypeAlias" | "VariableDeclarator" | "TSTypeAliasDeclaration"
        ) && own_line);
    let hug = should_hug_type(p, node);
    let printed = p.map("types", |p, _| {
        let mut printed = p.print_current(Args::default())?;
        if !hug {
            printed = align(2.0, printed);
        }
        Ok(crate::comments::print_comments(
            &p.comments,
            p.node(),
            printed,
            p.text,
        ))
    })?;
    if hug {
        return Ok(join(&s(" | "), printed));
    }
    let start_line = should_indent && !own_line;
    let code = concat(vec![
        if_break(
            concat(vec![if start_line { line() } else { empty() }, s("| ")]),
            empty(),
        ),
        join(&concat(vec![line(), s("| ")]), printed),
    ]);
    if crate::parens::needs_parens(p)? {
        return Ok(group(concat(vec![indent(code), softline()])));
    }
    if parent.is("TSTupleType") && parent.list("elementTypes").len() > 1 {
        return Ok(group(concat(vec![
            indent(concat(vec![
                if_break(concat(vec![s("("), softline()]), empty()),
                code,
            ])),
            softline(),
            if_break(s(")"), empty()),
        ])));
    }
    Ok(group(if should_indent { indent(code) } else { code }))
}

fn print_intersection_type(p: &mut Printer<'_>) -> R {
    let node = p.node();
    let types = node.list("types");
    let text = p.text;
    let start = p.opts.operator_position_start;
    let mut was_indented = false;
    let mut parts = Vec::new();
    let printed = p.print_all("types")?;
    for (i, doc) in printed.into_iter().enumerate() {
        if i == 0 {
            parts.push(doc);
            continue;
        }
        let current = is_object_type(&types[i]);
        let previous = is_object_type(&types[i - 1]);
        if previous && current {
            parts.push(concat(vec![
                s(" & "),
                if was_indented { indent(doc) } else { doc },
            ]));
            continue;
        }
        if (!previous && !current) || has_leading_own_line_comment(&p.comments, text, &types[i]) {
            parts.push(if start {
                indent(concat(vec![line(), s("& "), doc]))
            } else {
                indent(concat(vec![s(" &"), line(), doc]))
            });
            continue;
        }
        if i > 1 {
            was_indented = true;
        }
        parts.push(concat(vec![
            s(" & "),
            if i > 1 { indent(doc) } else { doc },
        ]));
    }
    Ok(group(concat(parts)))
}

pub fn should_group_function_parameters(p: &Printer<'_>, node: &Value, return_type: &Doc) -> bool {
    let return_node = match node.field("returnType") {
        Some(rt) => rt.field("typeAnnotation").unwrap_or(rt),
        None => match node.field("typeAnnotation") {
            Some(t) => t,
            None => return false,
        },
    };
    let _ = p;
    if let Some(tp) = node.field("typeParameters") {
        let params = tp.list("params");
        if params.len() > 1 {
            return false;
        }
        if params.len() == 1
            && (params[0].field("constraint").is_some() || params[0].field("default").is_some())
        {
            return false;
        }
    }
    function_parameters(node).len() == 1 && (is_object_type(return_node) || will_break(return_type))
}

fn print_function_type(p: &mut Printer<'_>) -> R {
    let node = p.node();
    let mut parts = vec![print_abstract_token(p)];
    if node.is("TSConstructorType") || node.is("TSConstructSignatureDeclaration") {
        parts.push(s("new "));
    }
    let mut params = crate::function::print_function_parameters_ext(p, false, true)?;
    let key = if node.field("returnType").is_some() {
        "returnType"
    } else {
        "typeAnnotation"
    };
    let return_type = print_type_annotation_property(p, key)?;
    if should_group_function_parameters(p, node, &return_type) {
        params = group(params);
    }
    parts.push(params);
    parts.push(return_type);
    Ok(group(concat(parts)))
}

fn print_mapped_type(p: &mut Printer<'_>) -> R {
    let node = p.node();
    let mut should_break = false;
    if p.opts.object_wrap_preserve {
        let start = node.start();
        let key_start = node.field("key").map_or(start + 1, |k| k.start());
        let after = crate::module::text_without_comments(p, start + 1, key_start);
        let offset = after
            .find(|c: char| !c.is_whitespace())
            .unwrap_or(after.len());
        if crate::text::has_newline_in_range(p.text, start, start + 1 + offset) {
            should_break = true;
        }
    }
    let modifier = |v: &Value, keyword: &str| -> Doc {
        match v.as_str() {
            Some(t @ ("+" | "-")) => s(format!("{t}{keyword}")),
            _ => s(keyword.to_string()),
        }
    };
    let bracket = if p.opts.bracket_spacing {
        line()
    } else {
        softline()
    };
    let dangling = if p.has_comment(node, Flags::DANGLING) {
        group(concat(vec![
            p.print_dangling(node, false, None),
            hardline(),
        ]))
    } else {
        empty()
    };
    let readonly = match node.get("readonly") {
        Some(v) if !v.is_null() && v != &Value::Bool(false) => {
            concat(vec![modifier(v, "readonly"), s(" ")])
        }
        _ => empty(),
    };
    let key = p.print("key")?;
    let constraint = p.print("constraint")?;
    let name_type = if node.field("nameType").is_some() {
        concat(vec![s(" as "), p.print("nameType")?])
    } else {
        empty()
    };
    let optional = match node.get("optional") {
        Some(v) if !v.is_null() && v != &Value::Bool(false) => modifier(v, "?"),
        _ => empty(),
    };
    let has_annotation = node.field("typeAnnotation").is_some();
    let annotation = p.print("typeAnnotation")?;
    Ok(group_with(
        concat(vec![
            s("{"),
            indent(concat(vec![
                bracket.clone(),
                dangling,
                group(concat(vec![
                    readonly,
                    s("["),
                    key,
                    s(" in "),
                    constraint,
                    name_type,
                    s("]"),
                    optional,
                    s(if has_annotation { ": " } else { "" }),
                    annotation,
                ])),
                if p.opts.semi {
                    if_break(s(";"), empty())
                } else {
                    empty()
                },
            ])),
            bracket,
            s("}"),
        ]),
        should_break,
        None,
    ))
}

fn print_interface(p: &mut Printer<'_>) -> R {
    let node = p.node();
    let mut parts = vec![print_declare_token(p), s("interface")];
    let mut parts_group = vec![s(" "), p.print("id")?, p.print("typeParameters")?];
    let mut extends_parts = Vec::new();
    let indent_only_heritage = node
        .field("typeParameters")
        .is_some_and(|tp| !p.has_comment(tp, Flags::TRAILING.with(Flags::LINE)));
    let extends = node.list("extends");
    if !extends.is_empty() {
        let single = extends.len() == 1;
        let printed = p.print_all("extends")?;
        let joined = join(&concat(vec![s(","), line()]), printed);
        extends_parts.push(if indent_only_heritage {
            let id = node
                .field("typeParameters")
                .map(|tp| p.group_id_for("typeParameters", tp));
            if_break_id(s(" "), line(), id)
        } else {
            line()
        });
        extends_parts.push(s("extends "));
        extends_parts.push(if single { joined } else { indent(joined) });
    }
    if node
        .field("id")
        .is_some_and(|id| p.has_comment(id, Flags::TRAILING))
        || !extends.is_empty()
    {
        if indent_only_heritage {
            parts_group.push(indent(concat(extends_parts)));
            parts.push(group(concat(parts_group)));
        } else {
            parts_group.extend(extends_parts);
            parts.push(group(indent(concat(parts_group))));
        }
    } else {
        parts.extend(parts_group);
        parts.extend(extends_parts);
    }
    parts.push(s(" "));
    parts.push(p.print("body")?);
    Ok(group(concat(parts)))
}

fn print_cast(p: &mut Printer<'_>) -> R {
    let node = p.node();
    let mut parts = vec![p.print("expression")?];
    match node.ty() {
        "TSAsExpression" => {
            parts.push(s(" as "));
            parts.push(p.print("typeAnnotation")?);
        }
        "TSSatisfiesExpression" => {
            parts.push(s(" satisfies "));
            parts.push(p.print("typeAnnotation")?);
        }
        _ => {}
    }
    let key = p.key();
    let parent = p.parent();
    if (key == Some("callee") && parent.is_some_and(is_call_expression))
        || (key == Some("object") && parent.is_some_and(is_member_expression))
    {
        let mut inner = vec![softline()];
        inner.extend(parts);
        return Ok(group(concat(vec![indent(concat(inner)), softline()])));
    }
    Ok(concat(parts))
}

pub fn print_typescript(p: &mut Printer<'_>, args: &Args) -> R {
    let node = p.node();
    if is_ts_keyword_type(node) {
        let ty = node.ty();
        return Ok(s(ty[2..ty.len() - 7].to_lowercase()));
    }
    let semi = if p.opts.semi { ";" } else { "" };
    match node.ty() {
        "TSThisType" => Ok(s("this")),
        "TSTypeAssertion" => {
            let expression = node.field("expression").unwrap_or(node);
            let break_after_cast =
                !(expression.is("ArrayExpression") || expression.is("ObjectExpression"));
            let cast = shared(group(concat(vec![
                s("<"),
                indent(concat(vec![softline(), p.print("typeAnnotation")?])),
                softline(),
                s(">"),
            ])));
            let expr = p.print("expression")?;
            if break_after_cast {
                let contents = concat(vec![
                    if_break(s("("), empty()),
                    indent(concat(vec![softline(), expr.clone()])),
                    softline(),
                    if_break(s(")"), empty()),
                ]);
                return Ok(conditional_group(
                    vec![
                        concat(vec![cast.clone(), expr.clone()]),
                        concat(vec![cast.clone(), group_with(contents, true, None)]),
                        concat(vec![cast, expr]),
                    ],
                    false,
                ));
            }
            Ok(group(concat(vec![cast, expr])))
        }
        "TSDeclareFunction" => crate::function::print_function(p, args),
        "TSExportAssignment" => Ok(concat(vec![
            s("export = "),
            p.print("expression")?,
            s(semi),
        ])),
        "TSModuleBlock" => crate::estree::print_block_pub(p),
        "TSInterfaceBody" | "TSTypeLiteral" | "TSEnumBody" => crate::object::print_object(p),
        "TSTypeAliasDeclaration" => {
            let left = concat(vec![
                print_declare_token(p),
                s("type "),
                p.print("id")?,
                p.print("typeParameters")?,
            ]);
            let printed =
                crate::assignment::print_assignment(p, left, s(" ="), Some("typeAnnotation"))?;
            Ok(concat(vec![printed, s(semi)]))
        }
        "TSQualifiedName" => Ok(concat(vec![p.print("left")?, s("."), p.print("right")?])),
        "TSAbstractMethodDefinition" | "TSDeclareMethod" => crate::class::print_class_method(p),
        "TSAbstractAccessorProperty" | "TSAbstractPropertyDefinition" => {
            crate::class::print_class_property(p)
        }
        "TSInterfaceHeritage"
        | "TSClassImplements"
        | "TSExpressionWithTypeArguments"
        | "TSInstantiationExpression" => {
            let key = if node.field("typeArguments").is_some() {
                "typeArguments"
            } else {
                "typeParameters"
            };
            Ok(concat(vec![p.print("expression")?, p.print(key)?]))
        }
        "TSTemplateLiteralType" => crate::template::print_template_literal(p),
        "TSNamedTupleMember" => Ok(concat(vec![
            p.print("label")?,
            s(if node.flag("optional") { "?" } else { "" }),
            s(": "),
            p.print("elementType")?,
        ])),
        "TSRestType" => Ok(concat(vec![s("..."), p.print("typeAnnotation")?])),
        "TSOptionalType" => Ok(concat(vec![p.print("typeAnnotation")?, s("?")])),
        "TSInterfaceDeclaration" => print_interface(p),
        "TSTypeParameterDeclaration" | "TSTypeParameterInstantiation" => {
            if node.is("TSTypeParameterInstantiation")
                && p.text.get(node.start()..node.start() + 2) == Some("<<")
            {
                return bail("type arguments after `<`");
            }
            print_type_parameters(p, "params")
        }
        "TSTypeParameter" => print_type_parameter(p),
        "TSAsExpression" | "TSSatisfiesExpression" => print_cast(p),
        "TSArrayType" => Ok(concat(vec![p.print("elementType")?, s("[]")])),
        "TSPropertySignature" => Ok(concat(vec![
            s(if node.flag("readonly") {
                "readonly "
            } else {
                ""
            }),
            crate::property::print_property_key(p)?,
            print_optional_token(p),
            print_type_annotation_property(p, "typeAnnotation")?,
        ])),
        "TSParameterProperty" => Ok(concat(vec![
            print_accessibility_token(node),
            s(if node.flag("static") { "static " } else { "" }),
            s(if node.flag("override") {
                "override "
            } else {
                ""
            }),
            s(if node.flag("readonly") {
                "readonly "
            } else {
                ""
            }),
            p.print("parameter")?,
        ])),
        "TSTypeQuery" => {
            let key = if node.field("typeArguments").is_some() {
                "typeArguments"
            } else {
                "typeParameters"
            };
            Ok(concat(vec![
                s("typeof "),
                p.print("exprName")?,
                p.print(key)?,
            ]))
        }
        "TSIndexSignature" => {
            let params = node.list("parameters");
            let trailing = if params.len() > 1 {
                if_break(
                    s(if p.opts.should_print_comma(true) {
                        ","
                    } else {
                        ""
                    }),
                    empty(),
                )
            } else {
                empty()
            };
            let printed = p.print_all("parameters")?;
            let parameters = group(concat(vec![
                indent(concat(vec![
                    softline(),
                    join(&concat(vec![s(", "), softline()]), printed),
                ])),
                trailing,
                softline(),
            ]));
            let is_class_member =
                p.parent().is_some_and(|pa| pa.is("ClassBody")) && p.key() == Some("body");
            Ok(concat(vec![
                s(if is_class_member && node.flag("static") {
                    "static "
                } else {
                    ""
                }),
                s(if node.flag("readonly") {
                    "readonly "
                } else {
                    ""
                }),
                s("["),
                if node.get("parameters").is_some() {
                    parameters
                } else {
                    empty()
                },
                s("]"),
                print_type_annotation_property(p, "typeAnnotation")?,
                s(if is_class_member { semi } else { "" }),
            ]))
        }
        "TSTypePredicate" => {
            let prefix = if node.flag("asserts") { "asserts " } else { "" };
            let name = p.print("parameterName")?;
            let annotation = if node.field("typeAnnotation").is_some() {
                concat(vec![
                    s(" is "),
                    print_type_annotation_property(p, "typeAnnotation")?,
                ])
            } else {
                empty()
            };
            Ok(concat(vec![s(prefix), name, annotation]))
        }
        "TSNonNullExpression" => Ok(concat(vec![p.print("expression")?, s("!")])),
        "TSImportType" => {
            let key = if node.field("typeArguments").is_some() {
                "typeArguments"
            } else {
                "typeParameters"
            };
            Ok(concat(vec![
                s("import"),
                crate::call::print_call_arguments(p)?,
                if node.field("qualifier").is_some() {
                    concat(vec![s("."), p.print("qualifier")?])
                } else {
                    empty()
                },
                print_type_parameters(p, key)?,
            ]))
        }
        "TSLiteralType" => p.print("literal"),
        "TSIndexedAccessType" => Ok(concat(vec![
            p.print("objectType")?,
            print_optional_token(p),
            s("["),
            p.print("indexType")?,
            s("]"),
        ])),
        "TSTypeOperator" => Ok(concat(vec![
            s(node.str_of("operator").unwrap_or("").to_string()),
            s(" "),
            p.print("typeAnnotation")?,
        ])),
        "TSMappedType" => print_mapped_type(p),
        "TSMethodSignature" => {
            let kind = match node.str_of("kind") {
                Some(k) if k != "method" => format!("{k} "),
                _ => String::new(),
            };
            let computed = node.flag("computed");
            let mut parts = vec![
                print_accessibility_token(node),
                s(kind),
                s(if computed { "[" } else { "" }),
                p.print("key")?,
                s(if computed { "]" } else { "" }),
                print_optional_token(p),
            ];
            let params = crate::function::print_function_parameters_ext(p, false, true)?;
            let key = if node.field("returnType").is_some() {
                "returnType"
            } else {
                "typeAnnotation"
            };
            let has_return = node.field(key).is_some();
            let return_type = if has_return {
                print_type_annotation_property(p, key)?
            } else {
                empty()
            };
            let group_params = should_group_function_parameters(p, node, &return_type);
            parts.push(if group_params { group(params) } else { params });
            if has_return {
                parts.push(group(return_type));
            }
            Ok(group(concat(parts)))
        }
        "TSNamespaceExportDeclaration" => Ok(concat(vec![
            s("export as namespace "),
            p.print("id")?,
            s(semi),
        ])),
        "TSEnumDeclaration" => Ok(concat(vec![
            print_declare_token(p),
            s(if node.flag("const") { "const " } else { "" }),
            s("enum "),
            p.print("id")?,
            s(" "),
            p.print("body")?,
        ])),
        "TSEnumMember" => {
            let mut id = p.print("id")?;
            if node.flag("computed") {
                id = concat(vec![s("["), id, s("]")]);
            }
            if node.field("initializer").is_some() {
                return Ok(concat(vec![id, s(" = "), p.print("initializer")?]));
            }
            Ok(id)
        }
        "TSImportEqualsDeclaration" => Ok(concat(vec![
            s(if node.flag("isExport") { "export " } else { "" }),
            s("import "),
            match node.str_of("importKind") {
                Some(k) if k != "value" => s(format!("{k} ")),
                _ => empty(),
            },
            p.print("id")?,
            s(" = "),
            p.print("moduleReference")?,
            s(semi),
        ])),
        "TSExternalModuleReference" => {
            Ok(concat(vec![s("require("), p.print("expression")?, s(")")]))
        }
        "TSModuleDeclaration" => {
            let parent_is_decl = p.parent().is_some_and(|pa| pa.is("TSModuleDeclaration"));
            let body_is_decl = node
                .field("body")
                .is_some_and(|b| b.is("TSModuleDeclaration"));
            let mut parts = Vec::new();
            if parent_is_decl {
                parts.push(s("."));
            } else {
                parts.push(print_declare_token(p));
                let kind = node.str_of("kind").unwrap_or("");
                if kind != "global" {
                    parts.push(s(format!("{kind} ")));
                }
            }
            parts.push(p.print("id")?);
            if body_is_decl {
                parts.push(p.print("body")?);
            } else if node.field("body").is_some() {
                parts.push(s(" "));
                parts.push(group(p.print("body")?));
            } else {
                parts.push(s(semi));
            }
            Ok(concat(parts))
        }
        "TSConditionalType" => crate::ternary::print_ternary(p, args),
        "TSInferType" => Ok(concat(vec![s("infer "), p.print("typeParameter")?])),
        "TSIntersectionType" => print_intersection_type(p),
        "TSUnionType" => print_union_type(p),
        "TSFunctionType"
        | "TSCallSignatureDeclaration"
        | "TSConstructorType"
        | "TSConstructSignatureDeclaration" => print_function_type(p),
        "TSTupleType" => crate::array::print_array(p),
        "TSTypeReference" => {
            let key = if node.field("typeArguments").is_some() {
                "typeArguments"
            } else {
                "typeParameters"
            };
            Ok(concat(vec![
                p.print("typeName")?,
                print_type_parameters(p, key)?,
            ]))
        }
        "TSTypeAnnotation" => print_type_annotation(p),
        "TSEmptyBodyFunctionExpression" => crate::function::print_method_value_pub(p),
        "TSJSDocAllType" => Ok(s("*")),
        "TSJSDocUnknownType" => Ok(s("?")),
        other => bail(format!("node {other}")),
    }
}
