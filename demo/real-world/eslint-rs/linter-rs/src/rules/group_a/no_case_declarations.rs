//! lib/rules/no-case-declarations.js

use crate::ast::NodeId;
use crate::linter::Cx;
use crate::report::SugVal;
use crate::rule::{Report, SuggestSpec, fixer};
use crate::rules::{Rule, RuleDef};
use serde_json::Value;

pub static DEF: RuleDef = RuleDef {
    name: "no-case-declarations",
    messages: &[
        ("addBrackets", "Add {} brackets around the case block."),
        (
            "unexpected",
            "Unexpected lexical declaration in case block.",
        ),
    ],
    has_default_options: false,
    create,
};

struct R;

fn create(options: &[Value]) -> Result<Box<dyn Rule>, String> {
    if !options.is_empty() {
        return Err("invalid option".into());
    }
    Ok(Box::new(R))
}

impl Rule for R {
    fn listeners(&self) -> &'static [&'static str] {
        &["SwitchCase"]
    }

    fn on(&mut self, _: usize, node: NodeId, cx: &mut Cx) {
        let ast = &cx.sc.ast;
        let consequent: Vec<NodeId> = ast.nodes_of(node, "consequent").collect();
        let (Some(first), Some(last)) = (consequent.first(), consequent.last()) else {
            return;
        };
        let (first, last) = (ast.range(*first), ast.range(*last));
        for statement in &consequent {
            let lexical = match ast.kind(*statement) {
                "FunctionDeclaration" | "ClassDeclaration" => true,
                "VariableDeclaration" => ast.str(*statement, "kind") != Some("var"),
                _ => false,
            };
            if !lexical {
                continue;
            }
            let fix = crate::report::merge_fixes(
                vec![
                    fixer::insert_before_range(first, "{ "),
                    fixer::insert_after_range(last, " }"),
                ],
                cx.sc,
            );
            let mut r = Report::node(*statement, "unexpected");
            r.suggest.push(SuggestSpec {
                keys: vec![
                    ("messageId", SugVal::Str("addBrackets".into())),
                    ("fix", SugVal::Fix),
                ],
                fix,
            });
            cx.report(r);
        }
    }
}
