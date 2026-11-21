//! lib/rules/no-self-assign.js

use crate::ast::NodeId;
use crate::linter::Cx;
use crate::rule::Report;
use crate::rules::{Rule, RuleDef};
use crate::utils::{get_static_property_name, is_same_reference, skip_chain_expression};
use serde_json::Value;

pub static DEF: RuleDef = RuleDef {
    name: "no-self-assign",
    messages: &[("selfAssignment", "'{{name}}' is assigned to itself.")],
    has_default_options: true,
    create,
};

struct R {
    props: bool,
}

fn create(options: &[Value]) -> Result<Box<dyn Rule>, String> {
    if options.len() > 1 {
        return Err("invalid option".into());
    }
    let o = super::bool_props(options, 0, &["props"])?;
    Ok(Box::new(R {
        props: o[0].unwrap_or(true),
    }))
}

fn each_self_assignment(
    ast: &crate::ast::Ast,
    left: Option<NodeId>,
    right: Option<NodeId>,
    props: bool,
    out: &mut Vec<NodeId>,
) {
    let (Some(left), Some(right)) = (left, right) else {
        return;
    };
    let (lk, rk) = (ast.kind(left), ast.kind(right));
    if lk == "Identifier" && rk == "Identifier" && ast.name(left) == ast.name(right) {
        out.push(right);
    } else if lk == "ArrayPattern" && rk == "ArrayExpression" {
        let le = ast.list(left, "elements");
        let re = ast.list(right, "elements");
        let end = le.len().min(re.len());
        for i in 0..end {
            let l = le[i];
            let r = re[i];
            if l.is_some_and(|l| ast.is(l, "RestElement")) && i + 1 < re.len() {
                break;
            }
            each_self_assignment(ast, l, r, props, out);
            if r.is_some_and(|r| ast.is(r, "SpreadElement")) {
                break;
            }
        }
    } else if lk == "RestElement" && rk == "SpreadElement" {
        each_self_assignment(
            ast,
            ast.child(left, "argument"),
            ast.child(right, "argument"),
            props,
            out,
        );
    } else if lk == "ObjectPattern"
        && rk == "ObjectExpression"
        && !ast.list(right, "properties").is_empty()
    {
        let rp = ast.list(right, "properties");
        let mut start_j = 0;
        for i in (0..rp.len()).rev() {
            if rp[i].is_some_and(|p| ast.is(p, "SpreadElement")) {
                start_j = i + 1;
                break;
            }
        }
        for &l in ast.list(left, "properties") {
            for &r in &rp[start_j..] {
                each_self_assignment(ast, l, r, props, out);
            }
        }
    } else if lk == "Property"
        && rk == "Property"
        && ast.str(right, "kind") == Some("init")
        && !ast.bool(right, "method")
    {
        if let Some(name) = get_static_property_name(ast, left)
            && get_static_property_name(ast, right).as_deref() == Some(name.as_str())
        {
            each_self_assignment(
                ast,
                ast.child(left, "value"),
                ast.child(right, "value"),
                props,
                out,
            );
        }
    } else if props
        && ast.is(skip_chain_expression(ast, left), "MemberExpression")
        && ast.is(skip_chain_expression(ast, right), "MemberExpression")
        && is_same_reference(ast, left, right, false)
    {
        out.push(right);
    }
}

impl Rule for R {
    fn listeners(&self) -> &'static [&'static str] {
        &["AssignmentExpression"]
    }

    fn on(&mut self, _: usize, node: NodeId, cx: &mut Cx) {
        let sc = cx.sc;
        let ast = &sc.ast;
        if !matches!(ast.str(node, "operator"), Some("=" | "&&=" | "||=" | "??=")) {
            return;
        }
        let mut found = Vec::new();
        each_self_assignment(
            ast,
            ast.child(node, "left"),
            ast.child(node, "right"),
            self.props,
            &mut found,
        );
        for n in found {
            let name: String = sc
                .node_text(n)
                .chars()
                .filter(|c| !super::is_js_whitespace(*c))
                .collect();
            cx.report(Report::node(n, "selfAssignment").data("name", name));
        }
    }
}
