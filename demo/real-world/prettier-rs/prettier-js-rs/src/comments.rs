//! Comment attachment and printing: `src/main/comments/attach.js`, `print.js`, `utils.js`,
//! `src/language-js/comments/handle-comments.js` (the handlers that apply to JavaScript),
//! `printer-methods.js`, `print/comment.js` and
//! `parse/postprocess/merge-nestled-jsdoc-comments.js`. Attached comments live in a side table
//! keyed by node, so the AST itself is never mutated.

use std::cell::RefCell;

use crate::value::Value;

use crate::doc::*;
use crate::node::*;
use crate::text::*;
use crate::utils::{call_arguments, function_parameters, is_call_like_expression};

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Placement {
    OwnLine,
    EndOfLine,
    Remaining,
}

#[derive(Clone, Debug)]
pub struct Comment {
    pub block: bool,
    pub value: String,
    pub start: usize,
    pub end: usize,
    pub placement: Placement,
}

impl Comment {
    pub fn is_line(&self) -> bool {
        !self.block
    }

    pub fn is_type_cast(&self) -> bool {
        self.block
            && self.value.starts_with('*')
            && ["@type", "@satisfies"].iter().any(|tag| {
                self.value.match_indices(tag).any(|(i, _)| {
                    !self.value[i + tag.len()..]
                        .chars()
                        .next()
                        .is_some_and(|c| c.is_ascii_alphanumeric() || c == '_')
                })
            })
    }

    pub fn is_indentable_block(&self) -> bool {
        if !self.block {
            return false;
        }
        let text = format!("*{}*", self.value);
        let lines: Vec<&str> = text.split('\n').collect();
        lines.len() > 1
            && lines
                .iter()
                .all(|l| l.trim_start_matches(is_js_whitespace).starts_with('*'))
    }

    pub fn is_prettier_ignore(&self) -> bool {
        self.value.trim_matches(is_js_whitespace) == "prettier-ignore"
    }
}

pub fn is_js_whitespace(c: char) -> bool {
    c.is_whitespace() || c == '\u{FEFF}'
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Kind {
    Leading,
    Trailing,
    Dangling,
}

#[derive(Clone, Copy, Debug)]
pub struct Attached {
    pub index: usize,
    pub kind: Kind,
    pub marker: Option<&'static str>,
}

#[derive(Default)]
pub struct Comments {
    /// Attached for a JavaScript file, as Prettier's Babel parser shapes it (see `attach`).
    pub babel: bool,
    pub list: Vec<Comment>,
    by_node: doc_printer_rs::port::fx::FxHashMap<*const Value, Vec<Attached>>,
    printed: RefCell<doc_printer_rs::port::fx::FxHashSet<usize>>,
    ignored: RefCell<doc_printer_rs::port::fx::FxHashSet<usize>>,
    ignore_nodes: doc_printer_rs::port::fx::FxHashSet<*const Value>,
    unignore: doc_printer_rs::port::fx::FxHashSet<usize>,
}

/// Which comments of a node `hasComment` and `getComments` look at.
#[derive(Clone, Copy, Default)]
pub struct Flags {
    pub leading: bool,
    pub trailing: bool,
    pub dangling: bool,
    pub block: bool,
    pub line: bool,
    pub prettier_ignore: bool,
    pub first: bool,
    pub last: bool,
}

impl Flags {
    pub const ANY: Flags = Flags {
        leading: false,
        trailing: false,
        dangling: false,
        block: false,
        line: false,
        prettier_ignore: false,
        first: false,
        last: false,
    };
    pub const LEADING: Flags = Flags {
        leading: true,
        ..Flags::ANY
    };
    pub const TRAILING: Flags = Flags {
        trailing: true,
        ..Flags::ANY
    };
    pub const DANGLING: Flags = Flags {
        dangling: true,
        ..Flags::ANY
    };
    pub const BLOCK: Flags = Flags {
        block: true,
        ..Flags::ANY
    };
    pub const LINE: Flags = Flags {
        line: true,
        ..Flags::ANY
    };
    pub const PRETTIER_IGNORE: Flags = Flags {
        prettier_ignore: true,
        ..Flags::ANY
    };

    pub fn with(self, other: Flags) -> Flags {
        Flags {
            leading: self.leading || other.leading,
            trailing: self.trailing || other.trailing,
            dangling: self.dangling || other.dangling,
            block: self.block || other.block,
            line: self.line || other.line,
            prettier_ignore: self.prettier_ignore || other.prettier_ignore,
            first: self.first || other.first,
            last: self.last || other.last,
        }
    }
}

impl Comments {
    pub fn is_empty(&self) -> bool {
        self.list.is_empty()
    }

    pub fn of(&self, node: &Value) -> &[Attached] {
        self.by_node
            .get(&(comment_holder(node, self.babel) as *const Value))
            .map_or(&[], Vec::as_slice)
    }

    pub fn get(&self, i: usize) -> &Comment {
        &self.list[i]
    }

    /// `getComments(node, flags, fn)`.
    pub fn get_comments(
        &self,
        node: &Value,
        flags: Flags,
        f: Option<&dyn Fn(&Comment) -> bool>,
    ) -> Vec<Attached> {
        let all = self.of(node);
        let n = all.len();
        all.iter()
            .enumerate()
            .filter(|(i, a)| {
                let c = &self.list[a.index];
                !(flags.leading && a.kind != Kind::Leading
                    || flags.trailing && a.kind != Kind::Trailing
                    || flags.dangling && a.kind != Kind::Dangling
                    || flags.block && !c.block
                    || flags.line && c.block
                    || flags.first && *i != 0
                    || flags.last && *i != n - 1
                    || flags.prettier_ignore
                        && (!c.is_prettier_ignore() || self.unignore.contains(&a.index))
                    || f.is_some_and(|f| !f(c)))
            })
            .map(|(_, a)| *a)
            .collect()
    }

    /// `hasComment(node, flags, fn)`.
    pub fn has(&self, node: &Value, flags: Flags) -> bool {
        !self.of(node).is_empty() && !self.get_comments(node, flags, None).is_empty()
    }

    pub fn has_with(&self, node: &Value, flags: Flags, f: &dyn Fn(&Comment) -> bool) -> bool {
        !self.of(node).is_empty() && !self.get_comments(node, flags, Some(f)).is_empty()
    }

    pub fn has_any(&self, node: &Value) -> bool {
        !self.of(node).is_empty()
    }

    pub fn is_ignore_node(&self, node: &Value) -> bool {
        self.ignore_nodes.contains(&(node as *const Value))
    }

    pub fn mark_printed(&self, i: usize) {
        self.printed.borrow_mut().insert(i);
    }

    pub fn all_printed(&self) -> bool {
        let printed = self.printed.borrow();
        let ignored = self.ignored.borrow();
        (0..self.list.len()).all(|i| printed.contains(&i) || ignored.contains(&i))
    }

    pub fn ignore_range(&self, start: usize, end: usize) {
        let mut ignored = self.ignored.borrow_mut();
        for (i, c) in self.list.iter().enumerate() {
            if c.start >= start && c.end <= end {
                ignored.insert(i);
            }
        }
    }

    fn is_ignored(&self, i: usize) -> bool {
        self.ignored.borrow().contains(&i)
    }
}

/// Reads oxc's comments, merging nestled JSDoc comments as Prettier's postprocess does.
pub fn from_oxc(program: &oxc_ast::ast::Program<'_>, text: &str) -> Vec<Comment> {
    let mut list: Vec<Comment> = program
        .comments
        .iter()
        .map(|c| {
            let start = c.span.start as usize;
            let end = c.span.end as usize;
            let block = c.is_block();
            let value = if block {
                text[start + 2..end - 2].to_string()
            } else {
                text[start + 2..end].to_string()
            };
            Comment {
                block,
                value,
                start,
                end,
                placement: Placement::Remaining,
            }
        })
        .collect();
    if let Some(h) = &program.hashbang {
        let start = h.span.start as usize;
        let end = h.span.end as usize;
        list.insert(
            0,
            Comment {
                block: false,
                value: text.get(start + 2..end).unwrap_or("").to_string(),
                start,
                end,
                placement: Placement::Remaining,
            },
        );
    }
    if list.len() >= 2 {
        let mut i = list.len() - 1;
        while i > 0 {
            i -= 1;
            if list[i].end == list[i + 1].start
                && list[i].is_indentable_block()
                && list[i + 1].is_indentable_block()
            {
                let next = list.remove(i + 1);
                list[i].value = format!("{}*//*{}", list[i].value, next.value);
                list[i].end = next.end;
            }
        }
    }
    list
}

fn can_attach_comment(n: &Value) -> bool {
    !matches!(
        n.ty(),
        "EmptyStatement" | "TemplateElement" | "TSEmptyBodyFunctionExpression" | "ChainExpression"
    )
}

/// The node holding the comments that attach to `node`. Babel has no `ChainExpression`: its
/// outermost optional node, which oxc nests inside one, takes that place in the tree.
fn comment_holder(node: &Value, babel: bool) -> &Value {
    let mut node = node;
    while babel
        && node.is("ChainExpression")
        && let Some(e) = node.field("expression")
    {
        node = e;
    }
    node
}

/// The type Babel gives a method that oxc's ESTree output splits into a `MethodDefinition` or
/// `Property` and its `FunctionExpression` value: Babel has one `ClassMethod`,
/// `ClassPrivateMethod` or `ObjectMethod` node holding the key, parameters and body.
fn babel_method_type(n: &Value) -> Option<&'static str> {
    if !n.field("value").is_some_and(|v| v.is("FunctionExpression")) {
        return None;
    }
    if n.is("MethodDefinition") {
        let private = n.field("key").is_some_and(|k| k.is("PrivateIdentifier"));
        return Some(if private {
            "ClassPrivateMethod"
        } else {
            "ClassMethod"
        });
    }
    (n.is("Property") && crate::estree::is_method(n)).then_some("ObjectMethod")
}

/// `getSortedChildNodes`' child list. With `babel` (JavaScript files, which Prettier parses with
/// Babel), a method's `FunctionExpression` value is transparent, as Babel has no such node.
fn children(n: &Value, babel: bool) -> Vec<&Value> {
    if babel && babel_method_type(n).is_some() {
        let mut out = Vec::new();
        for c in children(n, false) {
            if c.is("FunctionExpression") && n.field("value").is_some_and(|v| v.same(c)) {
                out.extend(children(c, false));
            } else {
                out.push(c);
            }
        }
        return out;
    }
    // `getCommentChildNodes` for the ESTree parsers. `isNonEmptyArray(node.value.typeParameters)`
    // is always false for typescript-estree's `TSTypeParameterDeclaration`, so type parameters do
    // not stop this.
    if !babel
        && n.is("MethodDefinition")
        && let Some(value) = n.field("value")
        && value.is("FunctionExpression")
        && function_parameters(value).is_empty()
        && value.field("returnType").is_none()
        && value.list("typeParameters").is_empty()
        && let Some(body) = value.field("body")
    {
        let mut out: Vec<&Value> = n.list("decorators").iter().collect();
        if let Some(k) = n.field("key") {
            out.push(k);
        }
        out.push(body);
        return out;
    }
    let mut out = Vec::new();
    if let Value::Object(map) = n {
        for (k, v) in map {
            if k == "extra" {
                continue;
            }
            match v {
                Value::Object(_) if v.get("type").is_some_and(Value::is_string) => out.push(v),
                Value::Array(items) => {
                    for item in items {
                        if item.get("type").is_some_and(Value::is_string) {
                            out.push(item);
                        }
                    }
                }
                _ => {}
            }
        }
    }
    out
}

struct Attacher<'a> {
    text: &'a str,
    babel: bool,
    sorted: doc_printer_rs::port::fx::FxHashMap<*const Value, Vec<&'a Value>>,
    by_node: doc_printer_rs::port::fx::FxHashMap<*const Value, Vec<Attached>>,
    ignore_nodes: doc_printer_rs::port::fx::FxHashSet<*const Value>,
    unignore: doc_printer_rs::port::fx::FxHashSet<usize>,
}

impl<'a> Attacher<'a> {
    fn sorted_children(&mut self, n: &'a Value) -> Vec<&'a Value> {
        if let Some(v) = self.sorted.get(&(n as *const Value)) {
            return v.clone();
        }
        let mut out = Vec::new();
        for c in children(n, self.babel) {
            // Babel's outermost optional node stands where oxc's `ChainExpression` does, and
            // handlers compare it with the parent's fields; comments added to it go to its
            // expression (see `add`).
            if can_attach_comment(c) || (self.babel && c.is("ChainExpression")) {
                out.push(c);
            } else {
                out.extend(self.sorted_children(c));
            }
        }
        out.sort_by(|a, b| a.start().cmp(&b.start()).then(a.end().cmp(&b.end())));
        self.sorted.insert(n as *const Value, out.clone());
        out
    }

    fn decorate(
        &mut self,
        node: &'a Value,
        c: &Comment,
        enclosing: Option<&'a Value>,
    ) -> Result<Ctx<'a>, String> {
        let kids = self.sorted_children(node);
        let (mut preceding, mut following) = (None, None);
        let (mut left, mut right) = (0, kids.len());
        while left < right {
            let middle = (left + right) >> 1;
            let child = kids[middle];
            let (start, end) = (child.start(), child.end());
            if start <= c.start && c.end <= end {
                return self.decorate(child, c, Some(child));
            }
            if end <= c.start {
                preceding = Some(child);
                left = middle + 1;
                continue;
            }
            if c.end <= start {
                following = Some(child);
                right = middle;
                continue;
            }
            return Err("Comment location overlaps with node location".into());
        }
        if let Some(e) = enclosing
            && e.is("TemplateLiteral")
        {
            let quasis = e.list("quasis");
            let idx = expression_index(quasis, c.start);
            if preceding.is_some_and(|p| expression_index(quasis, p.start()) != idx) {
                preceding = None;
            }
            if following.is_some_and(|f| expression_index(quasis, f.start()) != idx) {
                following = None;
            }
        }
        Ok(Ctx {
            enclosing,
            preceding,
            following,
            index: 0,
        })
    }

    /// The node's type as Babel would report it (see `babel_method_type`).
    fn ty<'v>(&self, n: &'v Value) -> &'v str {
        if self.babel
            && let Some(t) = babel_method_type(n)
        {
            return t;
        }
        n.ty()
    }

    /// The node holding the function parts (`params`, `body`) of what Babel sees as `n`.
    fn function_node<'v>(&self, n: &'v Value) -> &'v Value {
        if self.babel
            && babel_method_type(n).is_some()
            && let Some(v) = n.field("value")
        {
            return v;
        }
        n
    }

    fn add(&mut self, node: &Value, index: usize, kind: Kind, marker: Option<&'static str>) {
        // With typescript-estree the `ChainExpression` keeps its comments (a handler naming a
        // field can reach it).
        let node = comment_holder(node, self.babel);
        self.by_node
            .entry(node as *const Value)
            .or_default()
            .push(Attached {
                index,
                kind,
                marker,
            });
    }
}

fn expression_index(quasis: &[Value], start: usize) -> usize {
    let start = start.wrapping_sub(1);
    for (i, q) in quasis.iter().enumerate().skip(1) {
        if start < q.start() || start == usize::MAX {
            return i - 1;
        }
    }
    0
}

#[derive(Clone, Copy)]
struct Ctx<'a> {
    enclosing: Option<&'a Value>,
    preceding: Option<&'a Value>,
    following: Option<&'a Value>,
    index: usize,
}

fn is_all_empty_and_no_line_break(text: &str) -> bool {
    !text
        .chars()
        .any(|c| !is_js_whitespace(c) || matches!(c, '\n' | '\u{2028}' | '\u{2029}'))
}

/// `attachComments`, returning the side table.
///
/// `babel` makes attachment follow the tree shape Babel gives JavaScript files (Prettier's
/// default parser for them) rather than typescript-estree's, which oxc's output follows.
pub fn attach(
    ast: &Value,
    list: Vec<Comment>,
    text: &str,
    experimental_ternaries: bool,
    babel: bool,
) -> Result<Comments, String> {
    let mut list = list;
    if list.is_empty() {
        return Ok(Comments {
            babel,
            ..Comments::default()
        });
    }
    let mut a = Attacher {
        text,
        babel,
        sorted: Default::default(),
        by_node: Default::default(),
        ignore_nodes: Default::default(),
        unignore: Default::default(),
    };
    let mut ctxs = Vec::with_capacity(list.len());
    for (i, c) in list.iter().enumerate() {
        let mut ctx = a.decorate(ast, c, None)?;
        ctx.index = i;
        ctxs.push(ctx);
    }
    let mut ties: Vec<Ctx<'_>> = Vec::new();
    let n = list.len();
    for i in 0..n {
        let ctx = ctxs[i];
        let is_last = i == n - 1;
        let own_line = {
            let mut start = list[i].start;
            if let Some(p) = ctx.preceding {
                for j in (0..i).rev() {
                    if !ctxs[j].preceding.is_some_and(|q| q.same(p))
                        || !is_all_empty_and_no_line_break(
                            text.get(list[j].end..start).unwrap_or("x"),
                        )
                    {
                        break;
                    }
                    start = list[j].start;
                }
            }
            has_newline(text, start, true)
        };
        if own_line {
            list[i].placement = Placement::OwnLine;
            let c = list[i].clone();
            if handle_own_line(&mut a, &c, &ctx, ast, is_last, experimental_ternaries) {
            } else if let Some(f) = ctx.following {
                a.add(f, i, Kind::Leading, None);
            } else if let Some(p) = ctx.preceding {
                a.add(p, i, Kind::Trailing, None);
            } else {
                a.add(ctx.enclosing.unwrap_or(ast), i, Kind::Dangling, None);
            }
            continue;
        }
        let end_of_line = {
            let mut end = list[i].end;
            if let Some(f) = ctx.following {
                for j in i + 1..n {
                    if !ctxs[j].following.is_some_and(|q| q.same(f))
                        || !is_all_empty_and_no_line_break(
                            text.get(end..list[j].start).unwrap_or("x"),
                        )
                    {
                        break;
                    }
                    end = list[j].end;
                }
            }
            has_newline(text, end, false)
        };
        if end_of_line {
            list[i].placement = Placement::EndOfLine;
            let c = list[i].clone();
            if handle_end_of_line(&mut a, &c, &ctx, ast, is_last, experimental_ternaries) {
            } else if let Some(p) = ctx.preceding {
                a.add(p, i, Kind::Trailing, None);
            } else if let Some(f) = ctx.following {
                a.add(f, i, Kind::Leading, None);
            } else {
                a.add(ctx.enclosing.unwrap_or(ast), i, Kind::Dangling, None);
            }
            continue;
        }
        list[i].placement = Placement::Remaining;
        let c = list[i].clone();
        if handle_remaining(&mut a, &c, &ctx, ast, is_last) {
        } else if let (Some(_), Some(f)) = (ctx.preceding, ctx.following) {
            if let Some(last) = ties.last()
                && !last.following.is_some_and(|q| q.same(f))
            {
                break_ties(&mut a, &mut ties, &list);
            }
            ties.push(ctx);
        } else if let Some(p) = ctx.preceding {
            a.add(p, i, Kind::Trailing, None);
        } else if let Some(f) = ctx.following {
            a.add(f, i, Kind::Leading, None);
        } else {
            a.add(ctx.enclosing.unwrap_or(ast), i, Kind::Dangling, None);
        }
    }
    break_ties(&mut a, &mut ties, &list);
    Ok(Comments {
        babel,
        list,
        by_node: a.by_node,
        printed: RefCell::new(Default::default()),
        ignored: RefCell::new(Default::default()),
        ignore_nodes: a.ignore_nodes,
        unignore: a.unignore,
    })
}

fn break_ties(a: &mut Attacher<'_>, ties: &mut Vec<Ctx<'_>>, list: &[Comment]) {
    let Some(first) = ties.first().copied() else {
        return;
    };
    let (Some(preceding), Some(following)) = (first.preceding, first.following) else {
        ties.clear();
        return;
    };
    let mut gap_end = following.start();
    let mut first_leading = ties.len();
    while first_leading > 0 {
        let c = &list[ties[first_leading - 1].index];
        let gap = a.text.get(c.end..gap_end).unwrap_or("x");
        if gap.chars().all(|ch| is_js_whitespace(ch) || ch == '(') {
            gap_end = c.start;
        } else {
            break;
        }
        first_leading -= 1;
    }
    for (i, t) in ties.iter().enumerate() {
        if i < first_leading {
            a.add(preceding, t.index, Kind::Trailing, None);
        } else {
            a.add(following, t.index, Kind::Leading, None);
        }
    }
    for node in [preceding, following] {
        let node = comment_holder(node, a.babel);
        if let Some(v) = a.by_node.get_mut(&(node as *const Value))
            && v.len() > 1
        {
            v.sort_by_key(|x| list[x.index].start);
        }
    }
    ties.clear();
}

fn next_char(text: &str, i: usize) -> Option<char> {
    next_non_space_non_comment_char(text, i)
}

fn is_single_line(c: &Comment, text: &str) -> bool {
    c.is_line() || !has_newline_in_range(text, c.start, c.end)
}

fn add_block_statement_first(a: &mut Attacher<'_>, node: &Value, i: usize) {
    let body = if node.get("body").is_some_and(Value::is_array) {
        node.list("body")
    } else {
        node.list("properties")
    };
    // Babel keeps directives out of `body`.
    let is_directive = |n: &Value| {
        a.babel && n.is("ExpressionStatement") && n.get("directive").is_some_and(Value::is_string)
    };
    match body
        .iter()
        .find(|n| !n.is("EmptyStatement") && !is_directive(n))
    {
        Some(first) => a.add(first, i, Kind::Leading, None),
        None => a.add(node, i, Kind::Dangling, None),
    }
}

fn add_block_or_not(a: &mut Attacher<'_>, node: &Value, i: usize) {
    if node.is("BlockStatement") {
        add_block_statement_first(a, node, i);
    } else {
        a.add(node, i, Kind::Leading, None);
    }
}

fn is_real_function_like(ty: &str) -> bool {
    matches!(
        ty,
        "ArrowFunctionExpression"
            | "FunctionExpression"
            | "FunctionDeclaration"
            | "ObjectMethod"
            | "ClassMethod"
            | "TSDeclareFunction"
            | "TSCallSignatureDeclaration"
            | "TSConstructSignatureDeclaration"
            | "TSMethodSignature"
            | "TSConstructorType"
            | "TSFunctionType"
            | "TSDeclareMethod"
    )
}

type Handler = fn(&mut Attacher<'_>, &Comment, &Ctx<'_>, &Value, bool, bool) -> bool;

fn handle_own_line(
    a: &mut Attacher<'_>,
    c: &Comment,
    ctx: &Ctx<'_>,
    ast: &Value,
    last: bool,
    exp: bool,
) -> bool {
    let handlers: [Handler; 20] = [
        ignore_comments,
        conditional_expression,
        last_function_arg,
        member_expression,
        if_statement,
        while_statement,
        try_statement,
        class,
        for_statement,
        union_type,
        only_comments,
        module_specifiers,
        assignment_pattern,
        method_name,
        labeled_statement,
        break_and_continue,
        nested_conditional,
        destructuring_pattern,
        mapped_type,
        never,
    ];
    handlers.iter().any(|h| h(a, c, ctx, ast, last, exp))
}

fn handle_end_of_line(
    a: &mut Attacher<'_>,
    c: &Comment,
    ctx: &Ctx<'_>,
    ast: &Value,
    last: bool,
    exp: bool,
) -> bool {
    let handlers: [Handler; 18] = [
        closure_type_cast,
        last_function_arg,
        conditional_expression,
        module_specifiers,
        if_statement,
        while_statement,
        try_statement,
        class,
        labeled_statement,
        call_expression,
        property,
        only_comments,
        variable_declarator,
        break_and_continue,
        switch_default_case,
        last_union_element,
        last_binary_operand,
        mapped_type,
    ];
    handlers.iter().any(|h| h(a, c, ctx, ast, last, exp))
}

fn handle_remaining(
    a: &mut Attacher<'_>,
    c: &Comment,
    ctx: &Ctx<'_>,
    ast: &Value,
    last: bool,
) -> bool {
    let handlers: [Handler; 11] = [
        ignore_comments,
        if_statement,
        while_statement,
        object_property_assignment,
        comment_in_empty_parens,
        method_name,
        only_comments,
        comment_after_arrow_params,
        function_name,
        break_and_continue,
        ts_function_trailing,
    ];
    handlers.iter().any(|h| h(a, c, ctx, ast, last, false))
}

fn never(_: &mut Attacher<'_>, _: &Comment, _: &Ctx<'_>, _: &Value, _: bool, _: bool) -> bool {
    false
}

fn is_union(n: &Value) -> bool {
    n.is("TSUnionType") || n.is("UnionTypeAnnotation")
}

fn ignore_comments(
    a: &mut Attacher<'_>,
    c: &Comment,
    ctx: &Ctx<'_>,
    _: &Value,
    _: bool,
    _: bool,
) -> bool {
    if c.is_prettier_ignore()
        && let Some(e) = ctx.enclosing
        && e.is("TSMappedType")
        && ctx
            .following
            .is_some_and(|f| e.field("key").is_some_and(|k| k.same(f)))
    {
        a.ignore_nodes.insert(e as *const Value);
        a.unignore.insert(ctx.index);
        return true;
    }
    false
}

fn union_type(
    a: &mut Attacher<'_>,
    c: &Comment,
    ctx: &Ctx<'_>,
    _: &Value,
    _: bool,
    _: bool,
) -> bool {
    if let Some(e) = ctx.enclosing
        && is_union(e)
    {
        if c.is_prettier_ignore() {
            if let Some(f) = ctx.following {
                a.ignore_nodes.insert(f as *const Value);
            }
            a.unignore.insert(ctx.index);
        }
        if let Some(p) = ctx.preceding {
            a.add(p, ctx.index, Kind::Trailing, None);
            return true;
        }
        return false;
    }
    if let Some(f) = ctx.following
        && is_union(f)
        && c.is_prettier_ignore()
    {
        if let Some(first) = f.list("types").first() {
            a.ignore_nodes.insert(first as *const Value);
        }
        a.unignore.insert(ctx.index);
    }
    false
}

fn mapped_type(
    a: &mut Attacher<'_>,
    _: &Comment,
    ctx: &Ctx<'_>,
    _: &Value,
    _: bool,
    _: bool,
) -> bool {
    let Some(e) = ctx.enclosing.filter(|e| e.is("TSMappedType")) else {
        return false;
    };
    if ctx.preceding.is_none() {
        a.add(e, ctx.index, Kind::Dangling, None);
        return true;
    }
    false
}

fn destructuring_pattern(
    a: &mut Attacher<'_>,
    _: &Comment,
    ctx: &Ctx<'_>,
    _: &Value,
    _: bool,
    _: bool,
) -> bool {
    if let Some(e) = ctx.enclosing
        && (e.is("ObjectPattern") || e.is("ArrayPattern"))
        && ctx.following.is_some_and(|f| f.is("TSTypeAnnotation"))
    {
        match ctx.preceding {
            Some(p) => a.add(p, ctx.index, Kind::Trailing, None),
            None => a.add(e, ctx.index, Kind::Dangling, None),
        }
        return true;
    }
    false
}

fn last_union_element(
    a: &mut Attacher<'_>,
    _: &Comment,
    ctx: &Ctx<'_>,
    _: &Value,
    _: bool,
    _: bool,
) -> bool {
    if let (Some(p), Some(e)) = (ctx.preceding, ctx.enclosing)
        && is_union(p)
        && (((e.is("TSArrayType") || e.is("ArrayTypeAnnotation")) && ctx.following.is_none())
            || e.is("TSIntersectionType")
            || e.is("IntersectionTypeAnnotation"))
        && let Some(last) = p.list("types").last()
    {
        a.add(last, ctx.index, Kind::Trailing, None);
        return true;
    }
    false
}

fn ts_function_trailing(
    a: &mut Attacher<'_>,
    c: &Comment,
    ctx: &Ctx<'_>,
    _: &Value,
    _: bool,
    _: bool,
) -> bool {
    if ctx.following.is_none()
        && let Some(e) = ctx.enclosing
        && matches!(
            e.ty(),
            "TSMethodSignature" | "TSDeclareFunction" | "TSAbstractMethodDefinition"
        )
        && next_char(a.text, c.end) == Some(';')
    {
        a.add(e, ctx.index, Kind::Trailing, None);
        return true;
    }
    false
}

fn closure_type_cast(
    a: &mut Attacher<'_>,
    c: &Comment,
    ctx: &Ctx<'_>,
    _: &Value,
    _: bool,
    _: bool,
) -> bool {
    if let Some(f) = ctx.following
        && c.is_type_cast()
    {
        a.add(f, ctx.index, Kind::Leading, None);
        return true;
    }
    false
}

fn if_statement(
    a: &mut Attacher<'_>,
    c: &Comment,
    ctx: &Ctx<'_>,
    _: &Value,
    _: bool,
    _: bool,
) -> bool {
    let (Some(e), Some(f)) = (ctx.enclosing, ctx.following) else {
        return false;
    };
    if !e.is("IfStatement") {
        return false;
    }
    let text = a.text;
    if next_char(text, c.end) == Some(')') {
        if let Some(p) = ctx.preceding {
            a.add(p, ctx.index, Kind::Trailing, None);
            return true;
        }
        return false;
    }
    let consequent = e.field("consequent");
    let alternate = e.field("alternate");
    if let (Some(p), Some(cons), Some(alt)) = (ctx.preceding, consequent, alternate)
        && p.same(cons)
        && f.same(alt)
    {
        let else_idx = next_non_space_non_comment_index(text, cons.end());
        if else_idx.is_some_and(|ix| c.start < ix) || alt.is("BlockStatement") {
            if p.is("BlockStatement") {
                a.add(p, ctx.index, Kind::Trailing, None);
                return true;
            }
            if is_single_line(c, text) && !has_newline_in_range(text, p.start(), c.start) {
                a.add(p, ctx.index, Kind::Trailing, None);
                return true;
            }
            a.add(e, ctx.index, Kind::Dangling, None);
            return true;
        }
    }
    if f.is("BlockStatement") {
        add_block_statement_first(a, f, ctx.index);
        return true;
    }
    if f.is("IfStatement") {
        if let Some(cons) = f.field("consequent") {
            add_block_or_not(a, cons, ctx.index);
            return true;
        }
        return false;
    }
    if consequent.is_some_and(|cons| cons.same(f)) {
        a.add(f, ctx.index, Kind::Leading, None);
        return true;
    }
    false
}

fn while_statement(
    a: &mut Attacher<'_>,
    c: &Comment,
    ctx: &Ctx<'_>,
    _: &Value,
    _: bool,
    _: bool,
) -> bool {
    let (Some(e), Some(f)) = (ctx.enclosing, ctx.following) else {
        return false;
    };
    if !e.is("WhileStatement") {
        return false;
    }
    if next_char(a.text, c.end) == Some(')') {
        if let Some(p) = ctx.preceding {
            a.add(p, ctx.index, Kind::Trailing, None);
            return true;
        }
        return false;
    }
    if f.is("BlockStatement") {
        add_block_statement_first(a, f, ctx.index);
        return true;
    }
    if e.field("body").is_some_and(|b| b.same(f)) {
        a.add(f, ctx.index, Kind::Leading, None);
        return true;
    }
    false
}

fn try_statement(
    a: &mut Attacher<'_>,
    _: &Comment,
    ctx: &Ctx<'_>,
    _: &Value,
    _: bool,
    _: bool,
) -> bool {
    let (Some(e), Some(f)) = (ctx.enclosing, ctx.following) else {
        return false;
    };
    if !e.is("TryStatement") && !e.is("CatchClause") {
        return false;
    }
    if e.is("CatchClause")
        && let Some(p) = ctx.preceding
    {
        a.add(p, ctx.index, Kind::Trailing, None);
        return true;
    }
    if f.is("BlockStatement") {
        add_block_statement_first(a, f, ctx.index);
        return true;
    }
    if f.is("TryStatement") {
        if let Some(fin) = f.field("finalizer") {
            add_block_or_not(a, fin, ctx.index);
            return true;
        }
        return false;
    }
    if f.is("CatchClause") {
        if let Some(body) = f.field("body") {
            add_block_or_not(a, body, ctx.index);
            return true;
        }
        return false;
    }
    false
}

fn member_expression(
    a: &mut Attacher<'_>,
    _: &Comment,
    ctx: &Ctx<'_>,
    _: &Value,
    _: bool,
    _: bool,
) -> bool {
    if let Some(e) = ctx.enclosing
        && is_member_expression(e)
        && ctx.following.is_some_and(|f| f.is("Identifier"))
    {
        a.add(e, ctx.index, Kind::Leading, None);
        return true;
    }
    false
}

fn nested_conditional(
    a: &mut Attacher<'_>,
    _: &Comment,
    ctx: &Ctx<'_>,
    _: &Value,
    _: bool,
    exp: bool,
) -> bool {
    if !exp {
        return false;
    }
    let is_cond = |n: &Value| {
        n.is("ConditionalExpression")
            || n.is("TSConditionalType")
            || n.is("ConditionalTypeAnnotation")
    };
    let Some(e) = ctx.enclosing.filter(|e| is_cond(e)) else {
        return false;
    };
    if ctx.following.is_some_and(is_cond) {
        a.add(e, ctx.index, Kind::Dangling, None);
        return true;
    }
    false
}

fn conditional_expression(
    a: &mut Attacher<'_>,
    c: &Comment,
    ctx: &Ctx<'_>,
    _: &Value,
    _: bool,
    exp: bool,
) -> bool {
    let text = a.text;
    let same_line = ctx
        .preceding
        .is_some_and(|p| !has_newline_in_range(text, p.end(), c.start));
    let (Some(e), Some(f)) = (ctx.enclosing, ctx.following) else {
        return false;
    };
    if (ctx.preceding.is_none() || !same_line)
        && (e.is("ConditionalExpression")
            || e.is("TSConditionalType")
            || e.is("ConditionalTypeAnnotation"))
    {
        if exp
            && e.field("alternate").is_some_and(|alt| alt.same(f))
            && !(c.block && !has_newline_in_range(text, c.start, c.end))
        {
            a.add(e, ctx.index, Kind::Dangling, None);
            return true;
        }
        a.add(f, ctx.index, Kind::Leading, None);
        return true;
    }
    false
}

fn object_property_assignment(
    a: &mut Attacher<'_>,
    _: &Comment,
    ctx: &Ctx<'_>,
    _: &Value,
    _: bool,
    _: bool,
) -> bool {
    if let Some(e) = ctx.enclosing
        && is_object_property(e)
        && e.flag("shorthand")
        && let (Some(k), Some(p)) = (e.field("key"), ctx.preceding)
        && k.same(p)
        && let Some(v) = e.field("value")
        && v.is("AssignmentPattern")
        && let Some(left) = v.field("left")
    {
        a.add(left, ctx.index, Kind::Trailing, None);
        return true;
    }
    false
}

fn class(a: &mut Attacher<'_>, _: &Comment, ctx: &Ctx<'_>, _: &Value, _: bool, _: bool) -> bool {
    let Some(e) = ctx.enclosing else {
        return false;
    };
    if !matches!(
        e.ty(),
        "ClassDeclaration"
            | "ClassExpression"
            | "DeclareClass"
            | "DeclareInterface"
            | "InterfaceDeclaration"
            | "TSInterfaceDeclaration"
    ) {
        return false;
    }
    let decorators = e.list("decorators");
    if !decorators.is_empty() && !ctx.following.is_some_and(|f| f.is("Decorator")) {
        if let Some(last) = decorators.last() {
            a.add(last, ctx.index, Kind::Trailing, None);
        }
        return true;
    }
    if let (Some(body), Some(f)) = (e.field("body"), ctx.following)
        && f.same(body)
    {
        add_block_statement_first(a, body, ctx.index);
        return true;
    }
    if let Some(f) = ctx.following {
        let is_head = |p: &Value| {
            e.field("id").is_some_and(|i| i.same(p))
                || e.field("typeParameters").is_some_and(|t| t.same(p))
        };
        if let (Some(sc), Some(p)) = (e.field("superClass"), ctx.preceding)
            && f.same(sc)
            && is_head(p)
        {
            a.add(p, ctx.index, Kind::Trailing, None);
            return true;
        }
        for prop in ["implements", "extends", "mixins"] {
            if e.list(prop).first().is_some_and(|x| x.same(f)) {
                match ctx.preceding {
                    Some(p) if is_head(p) || e.field("superClass").is_some_and(|s| s.same(p)) => {
                        a.add(p, ctx.index, Kind::Trailing, None)
                    }
                    _ => a.add(e, ctx.index, Kind::Dangling, Some(prop)),
                }
                return true;
            }
        }
    }
    false
}

const PROPERTY_LIKE: [&str; 11] = [
    "ClassMethod",
    "ClassProperty",
    "PropertyDefinition",
    "TSAbstractPropertyDefinition",
    "TSAbstractMethodDefinition",
    "TSDeclareMethod",
    "MethodDefinition",
    "ClassAccessorProperty",
    "AccessorProperty",
    "TSAbstractAccessorProperty",
    "TSParameterProperty",
];

fn method_name(
    a: &mut Attacher<'_>,
    c: &Comment,
    ctx: &Ctx<'_>,
    _: &Value,
    _: bool,
    _: bool,
) -> bool {
    let text = a.text;
    if let (Some(e), Some(p)) = (ctx.enclosing, ctx.preceding)
        && next_char(text, c.end) == Some('(')
        && matches!(
            a.ty(e),
            "Property" | "TSDeclareMethod" | "TSAbstractMethodDefinition"
        )
        && p.is("Identifier")
        && e.field("key").is_some_and(|k| k.same(p))
        && next_char(text, p.end()) != Some(':')
    {
        a.add(p, ctx.index, Kind::Trailing, None);
        return true;
    }
    if let Some(p) = ctx.preceding
        && p.is("Decorator")
        && ctx
            .enclosing
            .is_some_and(|e| PROPERTY_LIKE.contains(&a.ty(e)))
        && (c.is_line() || c.placement == Placement::OwnLine)
    {
        a.add(p, ctx.index, Kind::Trailing, None);
        return true;
    }
    false
}

fn function_name(
    a: &mut Attacher<'_>,
    c: &Comment,
    ctx: &Ctx<'_>,
    _: &Value,
    _: bool,
    _: bool,
) -> bool {
    if next_char(a.text, c.end) != Some('(') {
        return false;
    }
    if let Some(p) = ctx.preceding
        && ctx.enclosing.is_some_and(|e| {
            matches!(
                a.ty(e),
                "FunctionDeclaration"
                    | "FunctionExpression"
                    | "ClassMethod"
                    | "MethodDefinition"
                    | "ObjectMethod"
            )
        })
    {
        a.add(p, ctx.index, Kind::Trailing, None);
        return true;
    }
    false
}

fn comment_after_arrow_params(
    a: &mut Attacher<'_>,
    c: &Comment,
    ctx: &Ctx<'_>,
    _: &Value,
    _: bool,
    _: bool,
) -> bool {
    let Some(e) = ctx.enclosing.filter(|e| e.is("ArrowFunctionExpression")) else {
        return false;
    };
    if let Some(i) = next_non_space_non_comment_index(a.text, c.end)
        && a.text.get(i..i + 2) == Some("=>")
    {
        a.add(e, ctx.index, Kind::Dangling, None);
        return true;
    }
    false
}

fn comment_in_empty_parens(
    a: &mut Attacher<'_>,
    c: &Comment,
    ctx: &Ctx<'_>,
    _: &Value,
    _: bool,
    _: bool,
) -> bool {
    if next_char(a.text, c.end) != Some(')') {
        return false;
    }
    let Some(e) = ctx.enclosing else {
        return false;
    };
    if (is_real_function_like(a.ty(e)) && function_parameters(a.function_node(e)).is_empty())
        || (is_call_like_expression(e) && call_arguments(e).is_empty())
    {
        // Prettier prints a Babel method's dangling comments where the port prints its value's.
        let target = a.function_node(e);
        a.add(target, ctx.index, Kind::Dangling, None);
        return true;
    }
    if matches!(a.ty(e), "MethodDefinition" | "TSAbstractMethodDefinition")
        && let Some(v) = e.field("value")
        && function_parameters(v).is_empty()
    {
        a.add(v, ctx.index, Kind::Dangling, None);
        return true;
    }
    false
}

fn last_function_arg(
    a: &mut Attacher<'_>,
    c: &Comment,
    ctx: &Ctx<'_>,
    _: &Value,
    _: bool,
    _: bool,
) -> bool {
    let text = a.text;
    if let (Some(p), Some(e)) = (ctx.preceding, ctx.enclosing)
        && matches!(
            p.ty(),
            "Identifier"
                | "AssignmentPattern"
                | "ObjectPattern"
                | "ArrayPattern"
                | "RestElement"
                | "TSParameterProperty"
        )
        && is_real_function_like(a.ty(e))
        && next_char(text, c.end) == Some(')')
    {
        a.add(p, ctx.index, Kind::Trailing, None);
        return true;
    }
    if c.is_line()
        && let (Some(e), Some(f)) = (ctx.enclosing, ctx.following)
        && matches!(
            a.ty(e),
            "FunctionDeclaration" | "FunctionExpression" | "ObjectMethod"
        )
        && f.is("BlockStatement")
        && a.function_node(e).field("body").is_some_and(|b| b.same(f))
        && next_non_space_non_comment_index(text, c.end) == Some(f.start())
    {
        add_block_statement_first(a, f, ctx.index);
        return true;
    }
    false
}

fn labeled_statement(
    a: &mut Attacher<'_>,
    _: &Comment,
    ctx: &Ctx<'_>,
    _: &Value,
    _: bool,
    _: bool,
) -> bool {
    if let Some(e) = ctx.enclosing.filter(|e| e.is("LabeledStatement")) {
        a.add(e, ctx.index, Kind::Leading, None);
        return true;
    }
    false
}

fn break_and_continue(
    a: &mut Attacher<'_>,
    _: &Comment,
    ctx: &Ctx<'_>,
    _: &Value,
    _: bool,
    _: bool,
) -> bool {
    if let Some(e) = ctx.enclosing
        && (e.is("ContinueStatement") || e.is("BreakStatement"))
        && e.field("label").is_none()
    {
        a.add(e, ctx.index, Kind::Trailing, None);
        return true;
    }
    false
}

fn call_expression(
    a: &mut Attacher<'_>,
    _: &Comment,
    ctx: &Ctx<'_>,
    _: &Value,
    _: bool,
    _: bool,
) -> bool {
    if let (Some(e), Some(p)) = (ctx.enclosing, ctx.preceding)
        && is_call_expression(e)
        && e.field("callee").is_some_and(|c| c.same(p))
        && let Some(first) = e.list("arguments").first()
    {
        a.add(first, ctx.index, Kind::Leading, None);
        return true;
    }
    false
}

fn property(a: &mut Attacher<'_>, _: &Comment, ctx: &Ctx<'_>, _: &Value, _: bool, _: bool) -> bool {
    if let Some(e) = ctx.enclosing.filter(|e| is_object_property(e)) {
        a.add(e, ctx.index, Kind::Leading, None);
        return true;
    }
    false
}

fn only_comments(
    a: &mut Attacher<'_>,
    _: &Comment,
    ctx: &Ctx<'_>,
    ast: &Value,
    last: bool,
    _: bool,
) -> bool {
    if ast
        .get("body")
        .is_some_and(|b| b.as_array().is_some_and(Vec::is_empty))
    {
        a.add(
            ast,
            ctx.index,
            if last { Kind::Dangling } else { Kind::Leading },
            None,
        );
        return true;
    }
    if let Some(e) = ctx.enclosing
        && e.is("Program")
        && e.list("body").is_empty()
        && e.list("directives").is_empty()
    {
        a.add(
            e,
            ctx.index,
            if last { Kind::Dangling } else { Kind::Leading },
            None,
        );
        return true;
    }
    false
}

fn for_statement(
    a: &mut Attacher<'_>,
    _: &Comment,
    ctx: &Ctx<'_>,
    _: &Value,
    _: bool,
    _: bool,
) -> bool {
    if let Some(e) = ctx
        .enclosing
        .filter(|e| e.is("ForInStatement") || e.is("ForOfStatement"))
    {
        a.add(e, ctx.index, Kind::Leading, None);
        return true;
    }
    false
}

fn module_specifiers(
    a: &mut Attacher<'_>,
    c: &Comment,
    ctx: &Ctx<'_>,
    _: &Value,
    _: bool,
    _: bool,
) -> bool {
    if let Some(e) = ctx
        .enclosing
        .filter(|e| e.is("ImportSpecifier") || e.is("ExportSpecifier"))
    {
        a.add(e, ctx.index, Kind::Leading, None);
        return true;
    }
    let (Some(p), Some(e)) = (ctx.preceding, ctx.enclosing) else {
        return false;
    };
    let is_import = p.is("ImportSpecifier") && e.is("ImportDeclaration");
    let is_export = p.is("ExportSpecifier") && e.is("ExportNamedDeclaration");
    if (is_import || is_export) && has_newline(a.text, c.end, false) {
        a.add(p, ctx.index, Kind::Trailing, None);
        return true;
    }
    false
}

fn assignment_pattern(
    a: &mut Attacher<'_>,
    _: &Comment,
    ctx: &Ctx<'_>,
    _: &Value,
    _: bool,
    _: bool,
) -> bool {
    if let Some(e) = ctx.enclosing.filter(|e| e.is("AssignmentPattern")) {
        a.add(e, ctx.index, Kind::Leading, None);
        return true;
    }
    false
}

fn variable_declarator(
    a: &mut Attacher<'_>,
    c: &Comment,
    ctx: &Ctx<'_>,
    _: &Value,
    _: bool,
    _: bool,
) -> bool {
    if let (Some(e), Some(f)) = (ctx.enclosing, ctx.following)
        && matches!(
            e.ty(),
            "VariableDeclarator" | "AssignmentExpression" | "TypeAlias" | "TSTypeAliasDeclaration"
        )
        && (matches!(
            f.ty(),
            "ObjectExpression"
                | "ArrayExpression"
                | "TemplateLiteral"
                | "TaggedTemplateExpression"
                | "ObjectTypeAnnotation"
                | "TSTypeLiteral"
        ) || c.block)
    {
        a.add(f, ctx.index, Kind::Leading, None);
        return true;
    }
    false
}

fn switch_default_case(
    a: &mut Attacher<'_>,
    c: &Comment,
    ctx: &Ctx<'_>,
    _: &Value,
    _: bool,
    _: bool,
) -> bool {
    let (Some(e), Some(f)) = (ctx.enclosing, ctx.following) else {
        return false;
    };
    if !e.is("SwitchCase")
        || e.field("test").is_some()
        || !e.list("consequent").first().is_some_and(|x| x.same(f))
    {
        return false;
    }
    if f.is("BlockStatement") && c.is_line() {
        add_block_statement_first(a, f, ctx.index);
    } else {
        a.add(e, ctx.index, Kind::Dangling, None);
    }
    true
}

fn last_binary_operand(
    a: &mut Attacher<'_>,
    c: &Comment,
    ctx: &Ctx<'_>,
    _: &Value,
    _: bool,
    _: bool,
) -> bool {
    let text = a.text;
    if ctx.following.is_none()
        && let (Some(e), Some(p)) = (ctx.enclosing, ctx.preceding)
        && e.is("UnaryExpression")
        && (p.is("LogicalExpression") || p.is("BinaryExpression"))
        && let (Some(arg), Some(right)) = (e.field("argument"), p.field("right"))
        && has_newline_in_range(text, arg.start(), right.start())
        && is_single_line(c, text)
        && !has_newline_in_range(text, right.start(), c.start)
    {
        a.add(right, ctx.index, Kind::Trailing, None);
        return true;
    }
    false
}

pub fn print_comment(c: &Comment, text: &str) -> Doc {
    if c.is_line() {
        return s(text[c.start..c.end]
            .trim_end_matches(is_js_whitespace)
            .to_string());
    }
    if c.is_indentable_block() {
        let lines: Vec<&str> = c.value.split('\n').collect();
        let n = lines.len();
        let docs = lines
            .iter()
            .enumerate()
            .map(|(i, l)| {
                if i == 0 {
                    s(l.trim_end_matches(is_js_whitespace).to_string())
                } else if i < n - 1 {
                    s(format!(" {}", l.trim_matches(is_js_whitespace)))
                } else {
                    s(format!(" {}", l.trim_start_matches(is_js_whitespace)))
                }
            })
            .collect();
        return concat(vec![s("/*"), join(&hardline(), docs), s("*/")]);
    }
    concat(vec![s("/*"), replace_end_of_line(&c.value), s("*/")])
}

pub fn is_previous_line_empty(text: &str, start: usize) -> bool {
    let idx = text
        .get(..start)
        .and_then(|t| t.char_indices().next_back())
        .map_or(usize::MAX, |(p, _)| p);
    let idx = skip_spaces(text, Some(idx), true);
    let idx = skip_newline(text, idx, true);
    let idx = skip_spaces(text, idx, true);
    let idx2 = skip_newline(text, idx, true);
    idx != idx2
}

fn print_leading_comment(comments: &Comments, i: usize, text: &str) -> Doc {
    comments.mark_printed(i);
    let c = comments.get(i);
    let mut parts = vec![print_comment(c, text)];
    if c.block {
        parts.push(if has_newline(text, c.end, false) {
            if has_newline(text, c.start, true) {
                hardline()
            } else {
                line()
            }
        } else {
            s(" ")
        });
    } else {
        parts.push(hardline());
    }
    let index = skip_newline(text, skip_spaces(text, Some(c.end), false), false);
    if let Some(index) = index
        && has_newline(text, index, false)
    {
        parts.push(hardline());
    }
    concat(parts)
}

struct PrintedTrailing {
    doc: Doc,
    is_block: bool,
    has_line_suffix: bool,
}

fn print_trailing_comment(
    comments: &Comments,
    i: usize,
    text: &str,
    previous: Option<&PrintedTrailing>,
) -> PrintedTrailing {
    comments.mark_printed(i);
    let c = comments.get(i);
    let printed = print_comment(c, text);
    let is_block = c.block;
    if previous.is_some_and(|p| p.has_line_suffix && !p.is_block)
        || has_newline(text, c.start, true)
    {
        let empty_before = is_previous_line_empty(text, c.start);
        return PrintedTrailing {
            doc: line_suffix(concat(vec![
                hardline(),
                if empty_before { hardline() } else { empty() },
                printed,
            ])),
            is_block,
            has_line_suffix: true,
        };
    }
    if !is_block || previous.is_some_and(|p| p.has_line_suffix) {
        return PrintedTrailing {
            doc: concat(vec![
                line_suffix(concat(vec![s(" "), printed])),
                break_parent(),
            ]),
            is_block,
            has_line_suffix: true,
        };
    }
    PrintedTrailing {
        doc: concat(vec![s(" "), printed]),
        is_block,
        has_line_suffix: false,
    }
}

/// `printCommentsSeparately`: `None` when the node has no comments.
pub fn print_comments_separately(
    comments: &Comments,
    node: &Value,
    text: &str,
) -> Option<(Vec<Doc>, Vec<Doc>)> {
    let attached: Vec<Attached> = comments
        .of(node)
        .iter()
        .filter(|a| !comments.is_ignored(a.index))
        .copied()
        .collect();
    if comments.of(node).is_empty() {
        return None;
    }
    let mut leading = Vec::new();
    let mut trailing = Vec::new();
    let mut previous: Option<PrintedTrailing> = None;
    for a in &attached {
        match a.kind {
            Kind::Leading => leading.push(print_leading_comment(comments, a.index, text)),
            Kind::Trailing => {
                let printed = print_trailing_comment(comments, a.index, text, previous.as_ref());
                trailing.push(printed.doc.clone());
                previous = Some(printed);
            }
            Kind::Dangling => {}
        }
    }
    Some((leading, trailing))
}

/// `printComments`.
pub fn print_comments(comments: &Comments, node: &Value, doc: Doc, text: &str) -> Doc {
    match print_comments_separately(comments, node, text) {
        None => doc,
        Some((leading, trailing)) => {
            inherit_label(doc, |d| concat(vec![concat(leading), d, concat(trailing)]))
        }
    }
}

/// `printDanglingComments(path, options, { indent, marker, filter })`.
pub fn print_dangling_comments(
    comments: &Comments,
    node: &Value,
    text: &str,
    should_indent: bool,
    marker: Option<&'static str>,
    filter: Option<&dyn Fn(&Comment) -> bool>,
) -> Doc {
    let mut parts = Vec::new();
    for a in comments.of(node) {
        let c = comments.get(a.index);
        if a.kind != Kind::Dangling || a.marker != marker || filter.is_some_and(|f| !f(c)) {
            continue;
        }
        comments.mark_printed(a.index);
        parts.push(print_comment(c, text));
    }
    if parts.is_empty() {
        return empty();
    }
    let doc = join(&hardline(), parts);
    if should_indent {
        indent(concat(vec![hardline(), doc]))
    } else {
        doc
    }
}
