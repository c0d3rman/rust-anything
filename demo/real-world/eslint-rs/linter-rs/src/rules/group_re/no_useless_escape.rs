//! lib/rules/no-useless-escape.js

use super::util::regex_of;
use crate::ast::NodeId;
use crate::linter::Cx;
use crate::regexpp::{K, parse_pattern};
use crate::report::SugVal;
use crate::rule::{Report, SuggestSpec, fixer};
use crate::rules::{Rule, RuleDef};
use crate::source::{Loc, Pos};
use serde_json::Value;

pub static DEF: RuleDef = RuleDef {
    name: "no-useless-escape",
    messages: &[
        (
            "unnecessaryEscape",
            "Unnecessary escape character: \\{{character}}.",
        ),
        (
            "removeEscape",
            "Remove the `\\`. This maintains the current functionality.",
        ),
        (
            "removeEscapeDoNotKeepSemantics",
            "Remove the `\\` if it was inserted by mistake.",
        ),
        (
            "escapeBackslash",
            "Replace the `\\` with `\\\\` to include the actual backslash character.",
        ),
    ],
    has_default_options: true,
    create,
};

const VALID_STRING_ESCAPES: &str = "\\nrvtbfux\r\n\u{2028}\u{2029}";
const REGEX_GENERAL_ESCAPES: &str = "\\bcdDfnpPrsStvwWxu0123456789]";
const REGEX_NON_CHARCLASS_ESCAPES: &str = "\\bcdDfnpPrsStvwWxu0123456789]^/.$*+?[{}|()Bk";
const REGEX_CLASSSET_CHARACTER_ESCAPES: &str = "\\bcdDfnpPrsStvwWxu0123456789]q/[{}|()-";
const REGEX_CLASS_SET_RESERVED_DOUBLE_PUNCTUATOR: &str = "!#$%&*+,.:;<=>?@^`~";

struct R {
    allow_regex_characters: Vec<String>,
}

fn create(options: &[Value]) -> Result<Box<dyn Rule>, String> {
    let allow = super::string_array_option(options, "allowRegexCharacters")?.unwrap_or_default();
    Ok(Box::new(R {
        allow_regex_characters: allow,
    }))
}

fn is_directive(cx: &Cx, node: Option<NodeId>) -> bool {
    let ast = &cx.sc.ast;
    node.is_some_and(|n| ast.is(n, "ExpressionStatement") && ast.str(n, "directive").is_some())
}

fn report(
    cx: &mut Cx,
    node: NodeId,
    start_offset: usize,
    character: &str,
    disable_escape_backslash_suggest: bool,
) {
    let range_start = cx.sc.ast.range(node).0 + start_offset as u32;
    let range = (range_start, range_start + 1);
    let start = cx.sc.loc_of(range_start);
    let loc = Loc {
        start,
        end: Pos {
            line: start.line,
            column: start.column + 1,
        },
    };
    let message_id = if is_directive(cx, cx.sc.ast.parent(node)) {
        "removeEscapeDoNotKeepSemantics"
    } else {
        "removeEscape"
    };
    let mut r = Report::loc(loc, "unnecessaryEscape").data("character", character);
    r.node = Some(node);
    r.suggest.push(SuggestSpec {
        keys: vec![
            ("messageId", SugVal::Str(message_id.into())),
            ("fix", SugVal::Fix),
        ],
        fix: Some(fixer::remove_range(range)),
    });
    if !disable_escape_backslash_suggest {
        r.suggest.push(SuggestSpec {
            keys: vec![
                ("messageId", SugVal::Str("escapeBackslash".into())),
                ("fix", SugVal::Fix),
            ],
            fix: Some(fixer::insert_before_range(range, "\\")),
        });
    }
    cx.report(r);
}

impl R {
    fn validate_string(&self, cx: &mut Cx, node: NodeId, value: &[char], index: usize) {
        let is_template_element = cx.sc.ast.is(node, "TemplateElement");
        let escaped = value[index + 1];
        let mut is_unnecessary_escape = !VALID_STRING_ESCAPES.contains(escaped);
        let is_quote_escape;
        if is_template_element {
            is_quote_escape = escaped == '`';
            if escaped == '$' {
                is_unnecessary_escape = value.get(index + 2) != Some(&'{');
            } else if escaped == '{' {
                is_unnecessary_escape = index == 0 || value[index - 1] != '$';
            }
        } else {
            let raw = cx.sc.node_text(node);
            is_quote_escape = raw.starts_with(escaped);
        }
        if is_unnecessary_escape && !is_quote_escape {
            let offset: usize = value[..index].iter().map(|c| c.len_utf16()).sum();
            report(cx, node, offset, &escaped.to_string(), false);
        }
    }

    fn validate_regexp(&self, cx: &mut Cx, node: NodeId) {
        let Some((pattern, flags)) = regex_of(cx, node) else {
            return;
        };
        let unicode = flags.contains('u');
        let unicode_sets = flags.contains('v');
        let Ok(ast) = parse_pattern(&pattern, unicode, unicode_sets) else {
            return;
        };
        let mut stack: Vec<usize> = Vec::new();
        let mut reports: Vec<(usize, String, bool)> = Vec::new();
        ast.visit(&mut |a, id, enter| {
            let n = a.node(id);
            match &n.k {
                K::CharacterClass { .. } | K::ExpressionCharacterClass { .. } => {
                    if enter {
                        stack.insert(0, id);
                    } else if !stack.is_empty() {
                        stack.remove(0);
                    }
                }
                K::Character { value } if enter => {
                    let raw = a.raw16(id);
                    if raw.first() != Some(&0x5c) {
                        return;
                    }
                    let Ok(escaped) = String::from_utf16(&raw[1..]) else {
                        return;
                    };
                    let same = char::from_u32(*value).is_some_and(|c| escaped == c.to_string());
                    if !same || self.allow_regex_characters.contains(&escaped) {
                        return;
                    }
                    let Some(ch) = escaped.chars().next() else {
                        return;
                    };
                    let allowed = if stack.is_empty() {
                        REGEX_NON_CHARCLASS_ESCAPES
                    } else if unicode_sets {
                        REGEX_CLASSSET_CHARACTER_ESCAPES
                    } else {
                        REGEX_GENERAL_ESCAPES
                    };
                    if allowed.contains(ch) {
                        return;
                    }
                    let reported_index = n.start + 1;
                    let mut disable = false;
                    if let Some(&class) = stack.first() {
                        let cn = a.node(class);
                        if ch == '^' && cn.start + 1 == n.start {
                            return;
                        }
                        if !unicode_sets {
                            if ch == '-' && cn.start + 1 != n.start && n.end != cn.end - 1 {
                                return;
                            }
                        } else {
                            if REGEX_CLASS_SET_RESERVED_DOUBLE_PUNCTUATOR.contains(ch) {
                                if pattern.get(n.end) == Some(&(ch as u16)) {
                                    return;
                                }
                                if n.start > 0 && pattern.get(n.start - 1) == Some(&(ch as u16)) {
                                    if ch != '^' {
                                        return;
                                    }
                                    let negate = matches!(
                                        cn.k,
                                        K::CharacterClass { negate: true, .. }
                                            | K::ExpressionCharacterClass { negate: true, .. }
                                    );
                                    if !negate {
                                        return;
                                    }
                                    if cn.start + 1 < n.start - 1 {
                                        return;
                                    }
                                }
                            }
                            if let Some(p) = n.parent
                                && matches!(
                                    a.node(p).k,
                                    K::ClassIntersection { .. } | K::ClassSubtraction { .. }
                                ) {
                                    disable = true;
                                }
                        }
                    }
                    reports.push((reported_index, escaped, disable));
                }
                _ => {}
            }
        });
        for (index, ch, disable) in reports {
            report(cx, node, index, &ch, disable);
        }
    }
}

impl Rule for R {
    fn listeners(&self) -> &'static [&'static str] {
        &["Literal", "TemplateElement"]
    }

    fn on(&mut self, _: usize, node: NodeId, cx: &mut Cx) {
        let ast = &cx.sc.ast;
        let is_template_element = ast.is(node, "TemplateElement");
        if is_template_element {
            let parent = ast.parent(node);
            let grand = parent.and_then(|p| ast.parent(p));
            if let (Some(p), Some(g)) = (parent, grand)
                && ast.is(g, "TaggedTemplateExpression") && ast.child(g, "quasi") == Some(p) {
                    return;
                }
        }
        let is_string = ast.is(node, "Literal")
            && matches!(ast.prop(node, "value"), Some(crate::ast::Prop::Str(_)));
        if is_string || is_template_element {
            let Some(parent) = ast.parent(node) else {
                return;
            };
            if matches!(
                ast.kind(parent),
                "JSXAttribute" | "JSXElement" | "JSXFragment"
            ) {
                return;
            }
            let value: Vec<char> = cx.sc.node_text(node).chars().collect();
            let mut i = 0;
            while i + 1 < value.len() {
                if value[i] == '\\' && !value[i + 1].is_ascii_digit() {
                    self.validate_string(cx, node, &value, i);
                    i += 2;
                } else {
                    i += 1;
                }
            }
        } else if ast.prop(node, "regex").is_some() {
            self.validate_regexp(cx, node);
        }
    }
}
