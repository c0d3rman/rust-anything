//! lib/rules/no-unused-private-class-members.js

use crate::ast::NodeId;
use crate::linter::Cx;
use crate::report::{SugVal, merge_fixes};
use crate::rule::{Report, SuggestSpec, fixer};
use crate::rules::{Rule, RuleDef};
use crate::source::SourceCode;
use crate::tokens::{C, N};
use crate::utils::{can_continue_expression_in_class_body, needs_preceding_semicolon};
use serde_json::Value;

pub static DEF: RuleDef = RuleDef {
    name: "no-unused-private-class-members",
    messages: &[
        (
            "unusedPrivateClassMember",
            "'{{classMemberName}}' is defined but never used.",
        ),
        (
            "removeUnusedPrivateClassMember",
            "Remove unused private class member '{{classMemberName}}'.",
        ),
    ],
    has_default_options: false,
    create,
};

struct Member {
    name: String,
    declared: NodeId,
    has_reference: bool,
    is_accessor: bool,
    is_used: bool,
}

struct R {
    tracked: Vec<Vec<Member>>,
    unsupported: Option<&'static str>,
}

fn create(options: &[Value]) -> Result<Box<dyn Rule>, String> {
    super::no_options(options)?;
    Ok(Box::new(R {
        tracked: Vec::new(),
        unsupported: None,
    }))
}

fn line_start_index(sc: &SourceCode, start: u32) -> u32 {
    start - sc.loc_of(start).column
}

fn line(sc: &SourceCode, index: u32) -> u32 {
    sc.loc_of(index).line
}

fn starts_on_own_line(sc: &SourceCode, start: u32) -> Option<bool> {
    let before = sc.token_before_offset(start, C, |_| true)?;
    Some(line(sc, before.end) != line(sc, start))
}

fn shares_line_with_another_token(sc: &SourceCode, node: NodeId) -> Option<bool> {
    let (s, e) = sc.ast.range(node);
    let prev = sc.token_before(node, N)?;
    let next = sc.token_after(node, N)?;
    Some(line(sc, prev.end) == line(sc, s) || line(sc, next.start) == line(sc, e))
}

fn removal_range(sc: &SourceCode, node: NodeId) -> Option<(u32, u32)> {
    let (ns, ne) = sc.ast.range(node);
    let before = sc.comments_before(ns);
    let mut last_non_leading: isize = -1;
    for i in (0..before.len()).rev() {
        let next_start = if i + 1 < before.len() {
            before[i + 1].start
        } else {
            ns
        };
        if !starts_on_own_line(sc, before[i].start)?
            || line(sc, next_start) as i64 - line(sc, before[i].end) as i64 > 1
        {
            last_non_leading = i as isize;
            break;
        }
    }
    let leading = &before[(last_non_leading + 1) as usize..];
    let shares = shares_line_with_another_token(sc, node)?;
    let trailing: Vec<_> = if shares {
        Vec::new()
    } else {
        sc.comments_after(ne)
            .into_iter()
            .filter(|c| line(sc, c.start) == line(sc, ne))
            .collect()
    };
    let remove_leading = !leading.is_empty() && !shares;
    let last_end = trailing.last().map(|t| t.end).unwrap_or(ne);
    let prev = sc.token_before(node, N)?;
    let next = sc.token_after_offset(last_end, C, |_| true)?;
    let next_on_new_line = line(sc, next.start) > line(sc, last_end);
    let remove_own_line = !remove_leading && starts_on_own_line(sc, ns)? && next_on_new_line;
    let mut start = ns;
    let mut end = last_end;
    if remove_leading {
        start = if next_on_new_line {
            line_start_index(sc, leading[0].start)
        } else {
            leading[0].start
        };
        end = if next_on_new_line {
            line_start_index(sc, next.start)
        } else {
            next.start
        };
    } else if remove_own_line {
        start = line_start_index(sc, ns);
        end = line_start_index(sc, next.start);
    } else if line(sc, prev.end) == line(sc, ns) {
        start = prev.end;
    } else if line(sc, next.start) == line(sc, last_end) {
        end = next.start;
    }
    Some((start, end))
}

fn is_write_only_assignment(cx: &Cx, ident: NodeId) -> bool {
    let ast = &cx.sc.ast;
    let Some(parent) = ast.parent(ident) else {
        return false;
    };
    let Some(stmt) = ast.parent(parent) else {
        return false;
    };
    let is_assign = ast.is(stmt, "AssignmentExpression");
    if !is_assign
        && !matches!(
            ast.kind(stmt),
            "ForInStatement" | "ForOfStatement" | "AssignmentPattern"
        )
    {
        return false;
    }
    if ast.child(stmt, "left") != Some(parent) {
        return false;
    }
    if is_assign && ast.str(stmt, "operator") != Some("=") {
        return ast
            .parent(stmt)
            .is_some_and(|p| ast.is(p, "ExpressionStatement"));
    }
    true
}

impl Rule for R {
    fn listeners(&self) -> &'static [&'static str] {
        &["ClassBody", "PrivateIdentifier", "ClassBody:exit"]
    }

    fn on(&mut self, l: usize, node: NodeId, cx: &mut Cx) {
        let sc = cx.sc;
        let ast = &sc.ast;
        match l {
            0 => {
                let mut members: Vec<Member> = Vec::new();
                for m in ast.nodes_of(node, "body") {
                    if !matches!(ast.kind(m), "PropertyDefinition" | "MethodDefinition") {
                        continue;
                    }
                    let Some(key) = ast.child(m, "key") else {
                        continue;
                    };
                    if !ast.is(key, "PrivateIdentifier") {
                        continue;
                    }
                    let name = ast.name(key).unwrap_or("").to_string();
                    let member = Member {
                        name: name.clone(),
                        declared: m,
                        has_reference: false,
                        is_accessor: ast.is(m, "MethodDefinition")
                            && matches!(ast.str(m, "kind"), Some("set" | "get")),
                        is_used: false,
                    };
                    match members.iter_mut().find(|x| x.name == name) {
                        Some(slot) => *slot = member,
                        None => members.push(member),
                    }
                }
                self.tracked.insert(0, members);
            }
            1 => {
                let name = ast.name(node).unwrap_or("");
                let Some(member) = self
                    .tracked
                    .iter_mut()
                    .find_map(|c| c.iter_mut().find(|m| m.name == name))
                else {
                    return;
                };
                if member.is_used {
                    return;
                }
                let Some(parent) = ast.parent(node) else {
                    return;
                };
                if matches!(ast.kind(parent), "PropertyDefinition" | "MethodDefinition") {
                    return;
                }
                member.has_reference = true;
                if member.is_accessor {
                    member.is_used = true;
                    return;
                }
                if is_write_only_assignment(cx, node) {
                    return;
                }
                let wrapping = ast.parent(parent);
                let wrapping_kind = wrapping.map(|w| ast.kind(w)).unwrap_or("");
                let outer_kind = wrapping
                    .and_then(|w| ast.parent(w))
                    .map(|w| ast.kind(w))
                    .unwrap_or("");
                if wrapping_kind == "UpdateExpression" && outer_kind == "ExpressionStatement" {
                    return;
                }
                if wrapping_kind == "Property"
                    && outer_kind == "ObjectPattern"
                    && wrapping.and_then(|w| ast.child(w, "value")) == Some(parent)
                {
                    return;
                }
                if matches!(wrapping_kind, "RestElement" | "ArrayPattern") {
                    return;
                }
                member.is_used = true;
            }
            _ => {
                if self.tracked.is_empty() {
                    return;
                }
                let members = self.tracked.remove(0);
                for m in members {
                    if m.is_used {
                        continue;
                    }
                    let Some(key) = ast.child(m.declared, "key") else {
                        continue;
                    };
                    let display = format!("#{}", m.name);
                    let mut r = Report::loc(sc.node_loc(key), "unusedPrivateClassMember")
                        .data("classMemberName", display.as_str());
                    r.node = Some(m.declared);
                    let fix = if m.has_reference {
                        None
                    } else {
                        let Some(range) = removal_range(sc, m.declared) else {
                            self.unsupported = Some("member removal range");
                            return;
                        };
                        let mut fixes = vec![fixer::replace_range(range, "")];
                        let next = sc.token_after(m.declared, N);
                        if next.is_some_and(can_continue_expression_in_class_body)
                            && needs_preceding_semicolon(sc, m.declared)
                            && let Some(prev) = sc.token_before(m.declared, N)
                        {
                            fixes.push(fixer::insert_after_range((prev.start, prev.end), ";"));
                        }
                        merge_fixes(fixes, sc)
                    };
                    r.suggest.push(SuggestSpec {
                        keys: vec![
                            (
                                "messageId",
                                SugVal::Str("removeUnusedPrivateClassMember".into()),
                            ),
                            (
                                "data",
                                SugVal::Data(vec![(
                                    "classMemberName".into(),
                                    display.as_str().into(),
                                )]),
                            ),
                            ("fix", SugVal::Fix),
                        ],
                        fix,
                    });
                    cx.report(r);
                }
            }
        }
    }

    fn unsupported(&self) -> Option<&str> {
        self.unsupported
    }
}
