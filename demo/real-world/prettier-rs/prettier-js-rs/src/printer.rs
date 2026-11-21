//! The AST path and the print loop: `src/common/ast-path.js`, `src/main/ast-to-doc.js` and
//! `src/language-js/print/index.js`.

use crate::value::Value;

use crate::doc::{self, Doc};
use crate::node::{NodeExt, is_jsx_element};
use crate::options::Options;

/// The file uses something this port does not print yet; the original formats it.
#[derive(Debug, Clone)]
pub struct Unsupported(pub String);

pub type R<T = Doc> = Result<T, Unsupported>;

pub fn bail<T>(why: impl Into<String>) -> R<T> {
    Err(Unsupported(why.into()))
}

/// Arguments a parent passes to a child's print, as Prettier's `print(selector, args)`.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Args {
    pub assignment_layout: Option<&'static str>,
    pub expand_first_arg: bool,
    pub expand_last_arg: bool,
}

impl Args {
    pub fn is_empty(&self) -> bool {
        *self == Args::default()
    }
}

#[derive(Clone, Copy)]
pub struct Frame<'a> {
    pub node: &'a Value,
    pub key: Option<&'static str>,
    pub index: Option<usize>,
}

pub struct Printer<'a> {
    pub text: &'a str,
    pub opts: &'a Options,
    stack: Vec<Frame<'a>>,
    cache: doc_printer_rs::port::fx::FxHashMap<*const Value, Doc>,
    group_ids: doc_printer_rs::port::fx::FxHashMap<(&'static str, *const Value), String>,
    pub in_jest_each: bool,
    pub comments: crate::comments::Comments,
}

impl<'a> Printer<'a> {
    pub fn new(
        text: &'a str,
        opts: &'a Options,
        root: &'a Value,
        comments: crate::comments::Comments,
    ) -> Self {
        Printer {
            text,
            opts,
            stack: vec![Frame {
                node: root,
                key: None,
                index: None,
            }],
            cache: Default::default(),
            group_ids: Default::default(),
            in_jest_each: false,
            comments,
        }
    }

    pub fn node(&self) -> &'a Value {
        self.stack[self.stack.len() - 1].node
    }

    pub fn ancestor(&self, level: usize) -> Option<&'a Value> {
        let len = self.stack.len();
        (len > level).then(|| self.stack[len - 1 - level].node)
    }

    pub fn parent(&self) -> Option<&'a Value> {
        self.ancestor(1)
    }

    pub fn grandparent(&self) -> Option<&'a Value> {
        self.ancestor(2)
    }

    pub fn key(&self) -> Option<&'static str> {
        self.stack[self.stack.len() - 1].key
    }

    pub fn index(&self) -> Option<usize> {
        self.stack[self.stack.len() - 1].index
    }

    pub fn frame(&self, level: usize) -> Option<Frame<'a>> {
        let len = self.stack.len();
        (len > level).then(|| self.stack[len - 1 - level])
    }

    pub fn depth(&self) -> usize {
        self.stack.len()
    }

    pub fn find_ancestor(&self, f: impl Fn(&Value) -> bool) -> Option<&'a Value> {
        self.stack
            .iter()
            .rev()
            .skip(1)
            .map(|f| f.node)
            .find(|n| f(n))
    }

    pub fn group_id_for(&mut self, name: &'static str, node: &Value) -> String {
        let key = (name, node as *const Value);
        if let Some(id) = self.group_ids.get(&key) {
            return id.clone();
        }
        let id = self.new_id(name);
        self.group_ids.insert(key, id.clone());
        id
    }

    /// Prettier's group ids are `Symbol`s, unique across the whole run. Embedded code (one
    /// `Printer` per Markdown code block) shares one doc, so the counter is process-wide, not
    /// per printer.
    pub fn new_id(&mut self, name: &str) -> String {
        static IDS: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
        let n = IDS.fetch_add(1, std::sync::atomic::Ordering::Relaxed) + 1;
        format!("{name}#{n}")
    }

    fn push(&mut self, node: &'a Value, key: &'static str, index: Option<usize>) {
        self.stack.push(Frame {
            node,
            key: Some(key),
            index,
        });
    }

    pub fn call<T>(&mut self, key: &'static str, f: impl FnOnce(&mut Self) -> T) -> Option<T> {
        let child = self.node().field(key)?;
        self.push(child, key, None);
        let out = f(self);
        self.stack.pop();
        Some(out)
    }

    pub fn call_index<T>(
        &mut self,
        key: &'static str,
        index: usize,
        f: impl FnOnce(&mut Self) -> T,
    ) -> Option<T> {
        let child = self.node().list(key).get(index)?;
        self.push(child, key, Some(index));
        let out = f(self);
        self.stack.pop();
        Some(out)
    }

    pub fn call_path<T>(
        &mut self,
        path: &[(&'static str, Option<usize>)],
        f: impl FnOnce(&mut Self) -> T,
    ) -> Option<T> {
        let depth = self.stack.len();
        for &(key, index) in path {
            let child = match index {
                Some(i) => self.node().list(key).get(i),
                None => self.node().field(key),
            };
            let Some(child) = child else {
                self.stack.truncate(depth);
                return None;
            };
            self.push(child, key, index);
        }
        let out = f(self);
        self.stack.truncate(depth);
        Some(out)
    }

    pub fn each(
        &mut self,
        key: &'static str,
        mut f: impl FnMut(&mut Self, usize) -> R<()>,
    ) -> R<()> {
        let items = self.node().list(key);
        for (i, child) in items.iter().enumerate() {
            self.push(child, key, Some(i));
            let r = f(self, i);
            self.stack.pop();
            r?;
        }
        Ok(())
    }

    pub fn map<T>(
        &mut self,
        key: &'static str,
        mut f: impl FnMut(&mut Self, usize) -> R<T>,
    ) -> R<Vec<T>> {
        let items = self.node().list(key);
        let mut out = Vec::with_capacity(items.len());
        for (i, child) in items.iter().enumerate() {
            self.push(child, key, Some(i));
            let r = f(self, i);
            self.stack.pop();
            out.push(r?);
        }
        Ok(out)
    }

    pub fn print(&mut self, key: &'static str) -> R {
        self.print_args(key, Args::default())
    }

    pub fn print_args(&mut self, key: &'static str, args: Args) -> R {
        match self.call(key, |p| p.print_current(args)) {
            Some(r) => r,
            None => Ok(doc::empty()),
        }
    }

    pub fn print_index(&mut self, key: &'static str, index: usize) -> R {
        match self.call_index(key, index, |p| p.print_current(Args::default())) {
            Some(r) => r,
            None => Ok(doc::empty()),
        }
    }

    pub fn print_all(&mut self, key: &'static str) -> R<Vec<Doc>> {
        self.map(key, |p, _| p.print_current(Args::default()))
    }

    /// `mainPrint` for the node on top of the path: cached when printed without arguments.
    pub fn print_current(&mut self, args: Args) -> R {
        let node = self.node();
        let cacheable = args.is_empty();
        let key = node as *const Value;
        if cacheable && let Some(d) = self.cache.get(&key) {
            return Ok(d.clone());
        }
        let printed = self.generic_print(args)?;
        // Babel has no `ChainExpression`; the optional node it holds stands in its place, so that
        // node's comments are printed around the `ChainExpression` (and its parentheses).
        let in_chain =
            self.comments.babel && self.parent().is_some_and(|p| p.is("ChainExpression"));
        let printed = if self.will_print_own_comments() || in_chain {
            printed
        } else {
            crate::comments::print_comments(&self.comments, node, printed, self.text)
        };
        if cacheable {
            let printed = doc::shared(printed);
            self.cache.insert(key, printed.clone());
            return Ok(printed);
        }
        Ok(printed)
    }

    pub fn has_node_ignore_comment(&self, node: &Value) -> bool {
        // Under Babel the optional node inside holds the comment and is the one printed as
        // written (with the parentheses `needs_parens` gives it).
        if self.comments.babel && node.is("ChainExpression") {
            return false;
        }
        self.comments.is_ignore_node(node)
            || self
                .comments
                .has(node, crate::comments::Flags::PRETTIER_IGNORE)
    }

    pub fn is_ignored(&self) -> bool {
        let node = self.node();
        if self.has_node_ignore_comment(node) {
            return true;
        }
        let (Some(parent), Some(index)) = (self.parent(), self.index()) else {
            return false;
        };
        if !is_jsx_element(node) || !is_jsx_element(parent) {
            return false;
        }
        let siblings = parent.list("children");
        let mut prev = None;
        for i in (1..=index).rev() {
            let c = &siblings[i - 1];
            if c.is("JSXText") && !crate::jsx::is_meaningful_jsx_text(c) {
                continue;
            }
            prev = Some(c);
            break;
        }
        prev.is_some_and(|pv| {
            pv.is("JSXExpressionContainer")
                && pv
                    .field("expression")
                    .is_some_and(|e| e.is("JSXEmptyExpression") && self.has_node_ignore_comment(e))
        })
    }

    pub fn will_print_own_comments(&self) -> bool {
        let node = self.node();
        let parent = self.parent();
        let union_parent =
            parent.is_some_and(|p| matches!(p.ty(), "TSUnionType" | "UnionTypeAnnotation"));
        (is_jsx_element(node)
            || parent.is_some_and(|p| {
                matches!(
                    p.ty(),
                    "JSXSpreadAttribute" | "JSXSpreadChild" | "TSUnionType" | "UnionTypeAnnotation"
                ) || ((p.is("ClassDeclaration") || p.is("ClassExpression"))
                    && p.field("superClass").is_some_and(|s| s.same(node)))
            }))
            && (!self.has_own_comments_ignore_comment(node) || union_parent)
    }

    /// `hasNodeIgnoreComment` as `willPrintOwnComments` asks it. Under Babel a `ChainExpression`
    /// stands for the optional node inside, whose prettier-ignore comment counts here, although
    /// the inner node is the one printed as written.
    fn has_own_comments_ignore_comment(&self, node: &Value) -> bool {
        self.has_node_ignore_comment(node)
            || (self.comments.babel
                && node.is("ChainExpression")
                && self
                    .comments
                    .has(node, crate::comments::Flags::PRETTIER_IGNORE))
    }

    pub fn has_comment(&self, node: &Value, flags: crate::comments::Flags) -> bool {
        self.comments.has(node, flags)
    }

    pub fn has_any_comment(&self, node: &Value) -> bool {
        self.comments.has_any(node)
    }

    pub fn print_dangling(
        &self,
        node: &Value,
        should_indent: bool,
        marker: Option<&'static str>,
    ) -> doc::Doc {
        crate::comments::print_dangling_comments(
            &self.comments,
            node,
            self.text,
            should_indent,
            marker,
            None,
        )
    }

    fn generic_print(&mut self, args: Args) -> R {
        let node = self.node();
        let printed = if self.is_ignored() {
            // Prettier prints embedded languages before the estree printer sees the
            // `prettier-ignore` comment, so an embedded template is formatted anyway.
            if node.is("TemplateLiteral") && crate::template::is_embedded(self) {
                return bail("embedded language");
            }
            let (start, end) = (node.start(), node.end());
            self.comments.ignore_range(start, end);
            doc::s(self.text.get(start..end).unwrap_or("").to_string())
        } else {
            crate::estree::print(self, &args)?
        };
        if is_printed_directly(node.ty()) {
            return Ok(printed);
        }
        let has_decorators = !node.list("decorators").is_empty();
        let decorators = crate::class::print_decorators(self)?;
        let is_class_expression = node.is("ClassExpression");
        if has_decorators && !is_class_expression {
            return Ok(doc::inherit_label(printed, |d| {
                doc::group(doc::concat(vec![decorators, d]))
            }));
        }
        let needs_parens = crate::parens::needs_parens(self)?;
        let needs_semi = crate::estree::should_print_leading_semicolon(self)?;
        if doc::is_empty(&decorators) && !needs_parens && !needs_semi {
            return Ok(printed);
        }
        Ok(doc::inherit_label(printed, |d| {
            doc::concat(vec![
                doc::s(if needs_semi { ";" } else { "" }),
                doc::s(if needs_parens { "(" } else { "" }),
                if needs_parens && is_class_expression && has_decorators {
                    doc::concat(vec![
                        doc::indent(doc::concat(vec![doc::line(), decorators, d])),
                        doc::line(),
                    ])
                } else {
                    doc::concat(vec![decorators, d])
                },
                doc::s(if needs_parens { ")" } else { "" }),
            ])
        }))
    }
}

fn is_printed_directly(ty: &str) -> bool {
    matches!(
        ty,
        "ClassMethod"
            | "ClassPrivateMethod"
            | "ClassProperty"
            | "ClassAccessorProperty"
            | "AccessorProperty"
            | "TSAbstractAccessorProperty"
            | "PropertyDefinition"
            | "TSAbstractPropertyDefinition"
            | "ClassPrivateProperty"
            | "MethodDefinition"
            | "TSAbstractMethodDefinition"
            | "TSDeclareMethod"
    )
}
