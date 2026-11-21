//! lib/linter/code-path-analysis/code-path-analyzer.js

use super::state::{LoopType, Sink, State};
use super::{CodePath, EventKind, Graph, Raw};
use crate::ast::{Ast, NodeId, Prop};

struct Analyzer<'a> {
    ast: &'a Ast,
    x: Sink,
    states: Vec<State>,
}

fn is_handled_logical(op: &str) -> bool {
    matches!(op, "&&" | "||" | "??")
}

fn is_logical_assign(op: &str) -> bool {
    matches!(op, "&&=" | "||=" | "??=")
}

fn is_breakable(kind: &str) -> bool {
    matches!(
        kind,
        "WhileStatement"
            | "DoWhileStatement"
            | "ForStatement"
            | "ForInStatement"
            | "ForOfStatement"
            | "SwitchStatement"
    )
}

pub(super) fn analyze(ast: &Ast, root: NodeId) -> (Vec<Raw>, Graph) {
    let mut a = Analyzer {
        ast,
        x: Sink {
            g: Graph::default(),
            out: Vec::with_capacity(ast.len() * 3),
            node: root,
        },
        states: Vec::new(),
    };
    let mut stack: Vec<(NodeId, bool)> = vec![(root, true)];
    while let Some((n, enter)) = stack.pop() {
        if enter {
            a.enter_node(n);
            stack.push((n, false));
            for c in ast.children(n).into_iter().rev() {
                stack.push((c, true));
            }
        } else {
            a.leave_node(n);
        }
    }
    (a.x.out, a.x.g)
}

impl Analyzer<'_> {
    fn is_property_definition_value(&self, n: NodeId) -> bool {
        self.ast.parent(n).is_some_and(|p| {
            self.ast.is(p, "PropertyDefinition") && self.ast.child(p, "value") == Some(n)
        })
    }

    fn label_of(&self, n: NodeId) -> Option<String> {
        let p = self.ast.parent(n)?;
        if self.ast.is(p, "LabeledStatement") {
            let l = self.ast.child(p, "label")?;
            return self.ast.name(l).map(str::to_string);
        }
        None
    }

    fn is_forking_by_true_or_false(&self, n: NodeId) -> bool {
        let ast = self.ast;
        let Some(p) = ast.parent(n) else { return false };
        match ast.kind(p) {
            "ConditionalExpression"
            | "IfStatement"
            | "WhileStatement"
            | "DoWhileStatement"
            | "ForStatement" => ast.child(p, "test") == Some(n),
            "LogicalExpression" => ast.str(p, "operator").is_some_and(is_handled_logical),
            "AssignmentExpression" => ast.str(p, "operator").is_some_and(is_logical_assign),
            _ => false,
        }
    }

    fn simple_constant(&self, n: NodeId) -> Option<bool> {
        let ast = self.ast;
        if !ast.is(n, "Literal") {
            return None;
        }
        if ast
            .prop(n, "regex")
            .is_some_and(|p| !matches!(p, Prop::Null))
        {
            return Some(true);
        }
        if let Some(b) = ast.str(n, "bigint") {
            let digits = b
                .trim_start_matches("0x")
                .trim_start_matches("0X")
                .trim_start_matches("0o")
                .trim_start_matches("0O");
            let digits = digits.trim_start_matches("0b").trim_start_matches("0B");
            return Some(!digits.chars().all(|c| c == '0' || c == '_'));
        }
        Some(match ast.prop(n, "value") {
            Some(Prop::Bool(b)) => *b,
            Some(Prop::Num(x)) => *x != 0.0 && !x.is_nan(),
            Some(Prop::Str(s)) => !s.is_empty(),
            _ => false,
        })
    }

    fn is_identifier_reference(&self, n: NodeId) -> bool {
        let ast = self.ast;
        let Some(p) = ast.parent(n) else { return true };
        match ast.kind(p) {
            "LabeledStatement"
            | "BreakStatement"
            | "ContinueStatement"
            | "ArrayPattern"
            | "RestElement"
            | "ImportSpecifier"
            | "ImportDefaultSpecifier"
            | "ImportNamespaceSpecifier"
            | "CatchClause" => false,
            "FunctionDeclaration"
            | "FunctionExpression"
            | "ArrowFunctionExpression"
            | "ClassDeclaration"
            | "ClassExpression"
            | "VariableDeclarator" => ast.child(p, "id") != Some(n),
            "Property" | "PropertyDefinition" | "MethodDefinition" => {
                ast.child(p, "key") != Some(n)
                    || ast.bool(p, "computed")
                    || ast.bool(p, "shorthand")
            }
            _ => true,
        }
    }

    fn forward_current_to_head(&mut self, node: NodeId) {
        let Some(st) = self.states.last_mut() else {
            return;
        };
        let g = &mut self.x.g;
        let current = std::mem::take(&mut st.current);
        let head = st.head();
        let end = current.len().max(head.len());
        for i in 0..end {
            let (c, h) = (current.get(i).copied(), head.get(i).copied());
            if let Some(c) = c
                && Some(c) != h
            {
                let k = if g.reachable(c) {
                    EventKind::SegmentEnd(c, node)
                } else {
                    EventKind::UnreachableSegmentEnd(c, node)
                };
                self.x.out.push(Raw::Ev(k));
            }
        }
        st.current = head.clone();
        for i in 0..end {
            let (c, h) = (current.get(i).copied(), head.get(i).copied());
            if let Some(h) = h
                && c != Some(h)
            {
                g.mark_used(h);
                let k = if g.reachable(h) {
                    EventKind::SegmentStart(h, node)
                } else {
                    EventKind::UnreachableSegmentStart(h, node)
                };
                self.x.out.push(Raw::Ev(k));
            }
        }
    }

    fn leave_from_current_segment(&mut self, node: NodeId) {
        let Some(st) = self.states.last_mut() else {
            return;
        };
        for c in std::mem::take(&mut st.current) {
            let k = if self.x.g.reachable(c) {
                EventKind::SegmentEnd(c, node)
            } else {
                EventKind::UnreachableSegmentEnd(c, node)
            };
            self.x.out.push(Raw::Ev(k));
        }
    }

    fn preprocess(&mut self, node: NodeId) {
        let ast = self.ast;
        let Some(parent) = ast.parent(node) else {
            return;
        };
        let is = |key: &str| ast.child(parent, key) == Some(node);
        let test_const = self.simple_constant(node);
        let Some(st) = self.states.last_mut() else {
            return;
        };
        let g = &mut self.x.g;
        match ast.kind(parent) {
            "CallExpression" => {
                if ast.bool(parent, "optional")
                    && ast.list(parent, "arguments").first() == Some(&Some(node))
                {
                    st.make_optional_right(g);
                }
            }
            "MemberExpression" => {
                if ast.bool(parent, "optional") && is("property") {
                    st.make_optional_right(g);
                }
            }
            "LogicalExpression" => {
                if is("right") && ast.str(parent, "operator").is_some_and(is_handled_logical) {
                    st.make_logical_right(g);
                }
            }
            "AssignmentExpression" => {
                if is("right") && ast.str(parent, "operator").is_some_and(is_logical_assign) {
                    st.make_logical_right(g);
                }
            }
            "ConditionalExpression" | "IfStatement" => {
                if is("consequent") {
                    st.make_if_consequent(g);
                } else if is("alternate") {
                    st.make_if_alternate(g);
                }
            }
            "SwitchCase" => {
                if ast.list(parent, "consequent").first() == Some(&Some(node)) {
                    let is_default = ast.child(parent, "test").is_none();
                    st.make_switch_case_body(g, false, is_default);
                }
            }
            "TryStatement" => {
                if is("handler") {
                    st.make_catch_block(g);
                } else if is("finalizer") {
                    st.make_finally_block(g);
                }
            }
            "WhileStatement" => {
                if is("test") {
                    st.make_while_test(g, test_const);
                } else {
                    st.make_while_body(g);
                }
            }
            "DoWhileStatement" => {
                if is("body") {
                    st.make_do_while_body(g);
                } else {
                    st.make_do_while_test(g, test_const);
                }
            }
            "ForStatement" => {
                if is("test") {
                    st.make_for_test(g, test_const);
                } else if is("update") {
                    st.make_for_update(g);
                } else if is("body") {
                    st.make_for_body(&mut self.x);
                }
            }
            "ForInStatement" | "ForOfStatement" => {
                if is("left") {
                    st.make_for_in_of_left(g);
                } else if is("right") {
                    st.make_for_in_of_right(g);
                } else {
                    st.make_for_in_of_body(&mut self.x);
                }
            }
            "AssignmentPattern" if is("right") => {
                st.push_fork(false);
                st.fork_bypass_path(g);
                st.fork_path(g);
            }
            _ => {}
        }
    }

    fn start_code_path(&mut self, origin: &'static str, node: NodeId) {
        if !self.states.is_empty() {
            self.forward_current_to_head(node);
        }
        let upper = self.states.last().map(|s| s.cp);
        let cp = self.x.g.paths.len() as u32;
        let id = format!("s{}", cp + 1);
        let st = State::new(cp, &id, &mut self.x.g);
        self.x.g.paths.push(CodePath {
            id,
            origin,
            upper,
            children: Vec::new(),
            initial_segment: st.initial,
            final_segments: Vec::new(),
            returned_segments: Vec::new(),
            thrown_segments: Vec::new(),
        });
        if let Some(u) = upper {
            self.x.g.paths[u as usize].children.push(cp);
        }
        self.states.push(st);
        self.x.out.push(Raw::Ev(EventKind::CodePathStart(cp, node)));
    }

    fn process_enter(&mut self, node: NodeId) {
        let ast = self.ast;
        if self.is_property_definition_value(node) {
            self.start_code_path("class-field-initializer", node);
        }
        let kind = ast.kind(node);
        match kind {
            "Program" => self.start_code_path("program", node),
            "FunctionDeclaration" | "FunctionExpression" | "ArrowFunctionExpression" => {
                self.start_code_path("function", node)
            }
            "StaticBlock" => self.start_code_path("class-static-block", node),
            _ => {
                let forking = self.is_forking_by_true_or_false(node);
                let label = self.label_of(node);
                let Some(st) = self.states.last_mut() else {
                    return;
                };
                let g = &mut self.x.g;
                match kind {
                    "ChainExpression" => st.push_chain(),
                    "CallExpression" | "MemberExpression" => {
                        if ast.bool(node, "optional") {
                            st.make_optional_node();
                        }
                    }
                    "LogicalExpression" => {
                        if let Some(op) =
                            ast.str(node, "operator").filter(|o| is_handled_logical(o))
                        {
                            st.push_choice(static_op(op), forking);
                        }
                    }
                    "AssignmentExpression" => {
                        if let Some(op) = ast.str(node, "operator").filter(|o| is_logical_assign(o))
                        {
                            st.push_choice(static_op(&op[..op.len() - 1]), forking);
                        }
                    }
                    "ConditionalExpression" | "IfStatement" => st.push_choice("test", false),
                    "SwitchStatement" => {
                        let has_case = ast
                            .nodes_of(node, "cases")
                            .any(|c| ast.child(c, "test").is_some());
                        st.push_switch(has_case, label);
                    }
                    "TryStatement" => st.push_try(ast.child(node, "finalizer").is_some()),
                    "SwitchCase" => {
                        if let Some(p) = ast.parent(node)
                            && ast.child(p, "discriminant") != Some(node)
                            && ast.list(p, "cases").first() != Some(&Some(node))
                        {
                            st.fork_path(g);
                        }
                    }
                    "WhileStatement" => st.push_loop(LoopType::While, label),
                    "DoWhileStatement" => st.push_loop(LoopType::DoWhile, label),
                    "ForStatement" => st.push_loop(LoopType::For, label),
                    "ForInStatement" => st.push_loop(LoopType::ForIn, label),
                    "ForOfStatement" => st.push_loop(LoopType::ForOf, label),
                    "LabeledStatement" => {
                        if let Some(b) = ast.child(node, "body")
                            && !is_breakable(ast.kind(b))
                        {
                            let l = ast
                                .child(node, "label")
                                .and_then(|l| ast.name(l))
                                .map(str::to_string);
                            st.push_break(false, l);
                        }
                    }
                    _ => {}
                }
            }
        }
        self.forward_current_to_head(node);
    }

    fn process_exit(&mut self, node: NodeId) {
        let ast = self.ast;
        let kind = ast.kind(node);
        let mut dont_forward = false;
        let label = |n: NodeId| ast.child(n, "label").and_then(|l| ast.name(l));
        match kind {
            "BreakStatement" | "ContinueStatement" | "ReturnStatement" | "ThrowStatement" => {
                self.forward_current_to_head(node);
                let Some(st) = self.states.last_mut() else {
                    return;
                };
                match kind {
                    "BreakStatement" => st.make_break(&mut self.x.g, label(node)),
                    "ContinueStatement" => st.make_continue(&mut self.x, label(node)),
                    "ReturnStatement" => st.make_return(&mut self.x.g),
                    _ => st.make_throw(&mut self.x.g),
                }
                dont_forward = true;
            }
            _ => {
                let ident_ref = kind == "Identifier" && self.is_identifier_reference(node);
                let Some(st) = self.states.last_mut() else {
                    return;
                };
                let g = &mut self.x.g;
                match kind {
                    "ChainExpression" => st.pop_chain(g),
                    "IfStatement" | "ConditionalExpression" => {
                        st.pop_choice(g);
                    }
                    "LogicalExpression" => {
                        if ast.str(node, "operator").is_some_and(is_handled_logical) {
                            st.pop_choice(g);
                        }
                    }
                    "AssignmentExpression" => {
                        if ast.str(node, "operator").is_some_and(is_logical_assign) {
                            st.pop_choice(g);
                        }
                    }
                    "SwitchStatement" => st.pop_switch(&mut self.x),
                    "SwitchCase" => {
                        if ast.list(node, "consequent").is_empty() {
                            st.make_switch_case_body(g, true, ast.child(node, "test").is_none());
                        }
                        if st.reachable(&self.x.g) {
                            dont_forward = true;
                        }
                    }
                    "TryStatement" => st.pop_try(g),
                    "Identifier" => {
                        if ident_ref {
                            st.make_first_throwable(g);
                            dont_forward = true;
                        }
                    }
                    "CallExpression" | "ImportExpression" | "MemberExpression"
                    | "NewExpression" => st.make_first_throwable(g),
                    "YieldExpression" => st.make_yield(g),
                    "WhileStatement" | "DoWhileStatement" | "ForStatement" | "ForInStatement"
                    | "ForOfStatement" => st.pop_loop(&mut self.x),
                    "AssignmentPattern" => st.pop_fork(g),
                    "LabeledStatement"
                        if ast
                            .child(node, "body")
                            .is_some_and(|b| !is_breakable(ast.kind(b))) =>
                    {
                        st.pop_break(g);
                    }
                    _ => {}
                }
            }
        }
        if !dont_forward {
            self.forward_current_to_head(node);
        }
    }

    fn end_code_path(&mut self, node: NodeId) {
        if let Some(st) = self.states.last_mut() {
            st.make_final(&self.x.g);
        }
        self.leave_from_current_segment(node);
        let Some(st) = self.states.pop() else { return };
        let p = &mut self.x.g.paths[st.cp as usize];
        p.final_segments = st.final_segs;
        p.returned_segments = st.returned;
        p.thrown_segments = st.thrown;
        self.x
            .out
            .push(Raw::Ev(EventKind::CodePathEnd(st.cp, node)));
    }

    fn postprocess(&mut self, node: NodeId) {
        let ast = self.ast;
        match ast.kind(node) {
            "Program"
            | "FunctionDeclaration"
            | "FunctionExpression"
            | "ArrowFunctionExpression"
            | "StaticBlock" => self.end_code_path(node),
            "CallExpression" => {
                if ast.bool(node, "optional")
                    && ast.list(node, "arguments").is_empty()
                    && let Some(st) = self.states.last_mut()
                {
                    st.make_optional_right(&mut self.x.g);
                }
            }
            _ => {}
        }
        if self.is_property_definition_value(node) {
            self.end_code_path(node);
        }
    }

    fn enter_node(&mut self, node: NodeId) {
        self.x.node = node;
        if self.ast.parent(node).is_some() {
            self.preprocess(node);
        }
        self.process_enter(node);
        self.x.out.push(Raw::Enter(node));
    }

    fn leave_node(&mut self, node: NodeId) {
        self.x.node = node;
        self.process_exit(node);
        self.x.out.push(Raw::Exit(node));
        self.postprocess(node);
    }
}

fn static_op(op: &str) -> &'static str {
    match op {
        "&&" => "&&",
        "||" => "||",
        _ => "??",
    }
}
