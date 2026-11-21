//! Input that oxc accepts but Prettier's own parser rejects (Babel for `.js`, typescript-estree
//! for `.ts`), or that makes Prettier throw. Prettier reports an error for these, so the port
//! returns [`Unsupported`](crate::Unsupported) and the caller hands the file to Prettier. Each
//! check names the parser rule it mirrors. Runs on oxc's ESTree before `postprocess`, so
//! `ParenthesizedExpression` nodes are still in the tree.

use crate::comments::Comment;
use crate::id_chars::*;
use crate::node::NodeExt;
use crate::printer::{R, bail};
use crate::text::LINE_TERMINATORS;
use crate::value::Value;

pub struct Source<'a> {
    pub text: &'a str,
    pub comments: &'a [Comment],
    /// typescript-estree (`.ts`/`.tsx`/...) instead of Babel.
    pub ts: bool,
    pub tsx: bool,
}

pub fn check(ast: &Value, src: &Source<'_>) -> R<()> {
    if !src.ts {
        check_flow_comments(src)?;
    }
    let mut c = Checker {
        src,
        stack: Vec::new(),
        labels: Vec::new(),
    };
    c.visit(ast, "")
}

/// `@babel/parser` with the `flowComments` plugin, which Prettier enables when the file starts
/// with an `@flow` pragma: `/*:`, `/*::` and `/*flow-include` comments are code to Babel.
fn check_flow_comments(src: &Source<'_>) -> R<()> {
    if !is_flow_file(src.text) {
        return Ok(());
    }
    for c in src.comments.iter().filter(|c| c.block) {
        let body = c.value.trim_start_matches([' ', '\t']);
        if body.starts_with(':') || body.starts_with("flow-include") {
            return bail("flow comment in a flow file");
        }
    }
    Ok(())
}

/// `isFlowFile` in `parse/babel.js`: an `@flow`/`@noflow` pragma before the first token.
fn is_flow_file(text: &str) -> bool {
    let text = match text.strip_prefix("#!") {
        Some(rest) => rest.find('\n').map_or("", |i| &rest[i..]),
        None => text,
    };
    let head = &text[..skip_trivia(text, 0).0];
    static PRAGMA: std::sync::OnceLock<regex::Regex> = std::sync::OnceLock::new();
    PRAGMA
        .get_or_init(|| regex::Regex::new(r"@(?:no)?flow\b").expect("valid regex"))
        .is_match(head)
}

/// Skips whitespace and comments from `pos`; returns the next token's offset and whether a line
/// break came first (a line comment counts as one).
pub fn skip_trivia(text: &str, mut pos: usize) -> (usize, bool) {
    let bytes = text.as_bytes();
    let mut newline = false;
    while pos < bytes.len() {
        let rest = &text[pos..];
        if rest.starts_with("//") {
            pos += rest.find(LINE_TERMINATORS).unwrap_or(rest.len());
            newline = true;
        } else if let Some(body) = rest.strip_prefix("/*") {
            let len = body.find("*/").map_or(rest.len(), |i| i + 4);
            newline |= rest[..len].contains(LINE_TERMINATORS);
            pos += len;
        } else {
            let Some(ch) = rest.chars().next() else { break };
            if !ch.is_whitespace() && ch != '\u{feff}' {
                break;
            }
            newline |= LINE_TERMINATORS.contains(&ch);
            pos += ch.len_utf8();
        }
    }
    (pos, newline)
}

fn in_ranges(c: u32, ranges: &[(u32, u32)]) -> bool {
    let i = ranges.partition_point(|&(_, hi)| hi < c);
    ranges.get(i).is_some_and(|&(lo, _)| lo <= c)
}

/// Whether Babel (`ts == false`) or TypeScript accepts `name` as an identifier. Only non-ASCII
/// characters are checked: oxc's Unicode tables are newer than either parser's.
pub fn is_valid_identifier_name(name: &str, ts: bool, jsx: bool) -> bool {
    let (start, cont) = if ts {
        (TS_ID_START, TS_ID_CONTINUE)
    } else {
        (BABEL_ID_START, BABEL_ID_CONTINUE)
    };
    // JSX names: Babel's `jsxReadWord` takes identifier chars (start or part) and `-`.
    name.chars().enumerate().all(|(i, ch)| {
        let c = ch as u32;
        c < 0x80
            || in_ranges(c, if i == 0 && !jsx { start } else { cont })
            || (jsx && in_ranges(c, start))
    })
}

/// Babel's `checkProto`: two `__proto__: value` properties (a name or string key; not
/// shorthand, computed, a method or an accessor) in one object expression. Object patterns,
/// where the check is dropped, are `ObjectPattern` in oxc's tree.
fn check_duplicate_proto(node: &Value) -> R<()> {
    let mut seen = false;
    for prop in node.list("properties") {
        if !prop.is("Property")
            || prop.flag("computed")
            || prop.flag("shorthand")
            || prop.flag("method")
            || prop.str_of("kind").is_some_and(|k| k != "init")
        {
            continue;
        }
        let Some(key) = prop.field("key") else {
            continue;
        };
        let name = if key.is("Identifier") {
            key.str_of("name")
        } else {
            key.str_of("value")
        };
        if name == Some("__proto__") {
            if seen {
                return bail("Redefinition of __proto__ property");
            }
            seen = true;
        }
    }
    Ok(())
}

/// Checks a JSON tree, which Prettier parses with Babel: identifier names and `__proto__`.
pub fn check_json_identifiers(node: &Value) -> R<()> {
    match node {
        Value::Array(items) => items.iter().try_for_each(check_json_identifiers),
        Value::Object(map) => {
            if node.is("ObjectExpression") {
                check_duplicate_proto(node)?;
            }
            if node.is("Identifier")
                && let Some(name) = node.str_of("name")
                && !is_valid_identifier_name(name, false, false)
            {
                return bail("identifier character Babel rejects");
            }
            map.values().try_for_each(check_json_identifiers)
        }
        _ => Ok(()),
    }
}

#[derive(Clone, Copy, PartialEq)]
enum LabelKind {
    Loop,
    Switch,
    Other,
}

struct Checker<'a, 's> {
    src: &'s Source<'s>,
    stack: Vec<(&'a Value, &'static str)>,
    /// Babel's `state.labels`: named labels and the unnamed entries loops and `switch` push.
    labels: Vec<(Option<&'a str>, LabelKind)>,
}

fn is_function(n: &Value) -> bool {
    matches!(
        n.ty(),
        "FunctionDeclaration"
            | "FunctionExpression"
            | "ArrowFunctionExpression"
            | "TSEmptyBodyFunctionExpression"
            | "TSDeclareFunction"
    )
}

fn is_loop(n: &Value) -> bool {
    matches!(
        n.ty(),
        "ForStatement"
            | "ForInStatement"
            | "ForOfStatement"
            | "WhileStatement"
            | "DoWhileStatement"
    )
}

impl<'a, 's> Checker<'a, 's> {
    fn text(&self) -> &'s str {
        self.src.text
    }

    fn parent(&self) -> Option<&'a Value> {
        self.stack.last().map(|(n, _)| *n)
    }

    /// The class a member belongs to (its parent is the `ClassBody`).
    fn member_class(&self) -> Option<&'a Value> {
        self.stack.iter().rev().nth(1).map(|(n, _)| *n)
    }

    fn visit(&mut self, node: &'a Value, key: &'static str) -> R<()> {
        match node {
            Value::Array(items) => {
                for item in items {
                    self.visit(item, key)?;
                }
                Ok(())
            }
            Value::Object(map) => {
                if node.get("type").is_none() {
                    return Ok(());
                }
                self.check_node(node, key)?;
                let saved_labels = if is_function(node) || node.is("StaticBlock") {
                    Some(std::mem::take(&mut self.labels))
                } else {
                    None
                };
                let pushed = self.push_label(node);
                self.stack.push((node, key));
                for (k, v) in map.iter() {
                    if matches!(v, Value::Object(_) | Value::Array(_)) {
                        self.visit(v, k)?;
                    }
                }
                self.stack.pop();
                if pushed {
                    self.labels.pop();
                }
                if let Some(saved) = saved_labels {
                    self.labels = saved;
                }
                Ok(())
            }
            _ => Ok(()),
        }
    }

    fn push_label(&mut self, node: &'a Value) -> bool {
        let entry = if node.is("LabeledStatement") {
            let mut body = node.field("body");
            while let Some(b) = body.filter(|b| b.is("LabeledStatement")) {
                body = b.field("body");
            }
            let kind = match body {
                Some(b) if is_loop(b) => LabelKind::Loop,
                Some(b) if b.is("SwitchStatement") => LabelKind::Switch,
                _ => LabelKind::Other,
            };
            let name = node.field("label").and_then(|l| l.str_of("name"));
            (name, kind)
        } else if is_loop(node) {
            (None, LabelKind::Loop)
        } else if node.is("SwitchStatement") {
            (None, LabelKind::Switch)
        } else {
            return false;
        };
        self.labels.push(entry);
        true
    }

    fn check_node(&mut self, node: &'a Value, key: &'static str) -> R<()> {
        let ts = self.src.ts;
        match node.ty() {
            "Identifier" | "PrivateIdentifier" | "JSXIdentifier" => {
                if let Some(name) = node.str_of("name")
                    && !is_valid_identifier_name(name, ts, node.is("JSXIdentifier"))
                {
                    return bail("identifier character the parser rejects");
                }
            }
            "ClassDeclaration" => {
                // Babel `parseClassId`, typescript-estree: "A class declaration without the
                // 'default' modifier must have a name."
                if node.field("id").is_none()
                    && !self
                        .parent()
                        .is_some_and(|p| p.is("ExportDefaultDeclaration"))
                {
                    return bail("class declaration without a name");
                }
            }
            "TemplateLiteral" | "TSTemplateLiteralType" => self.check_template(node)?,
            _ => {}
        }
        if ts {
            self.check_ts(node, key)
        } else {
            self.check_babel(node)
        }
    }

    /// oxc drops a second expression inside `${}`, as in `` `${'' ''}` ``.
    fn check_template(&self, node: &Value) -> R<()> {
        let parts = if node.is("TSTemplateLiteralType") {
            node.list("types")
        } else {
            node.list("expressions")
        };
        for part in parts {
            let (next, _) = skip_trivia(self.text(), part.end());
            if self.text().as_bytes().get(next) != Some(&b'}') {
                return bail("junk in template substitution");
            }
        }
        Ok(())
    }

    fn check_babel(&mut self, node: &'a Value) -> R<()> {
        match node.ty() {
            "ObjectExpression" => check_duplicate_proto(node)?,
            "JSXElement" | "JSXFragment" => {
                // Babel continues a JSX element into a call, member or tagged template on the
                // next line (`<a />\n(1)` is `(<a />)(1)`); oxc inserts a semicolon there. Checked
                // here rather than when printing, as an ignored node is never printed.
                if !self.parent().is_some_and(|pa| {
                    matches!(pa.ty(), "JSXElement" | "JSXFragment" | "JSXAttribute")
                }) && matches!(
                    crate::text::next_non_space_non_comment_char(self.text(), node.end()),
                    Some('(' | '[' | '`')
                ) {
                    return bail("JSX element continued on the next line");
                }
            }
            "LabeledStatement" => {
                let name = node.field("label").and_then(|l| l.str_of("name"));
                if self.labels.iter().any(|(n, _)| n.is_some() && *n == name) {
                    return bail("Label is already declared");
                }
            }
            "BreakStatement" | "ContinueStatement" => {
                // Babel `verifyBreakContinue`.
                let is_break = node.is("BreakStatement");
                let label = node.field("label").and_then(|l| l.str_of("name"));
                let ok = self.labels.iter().any(|(name, kind)| {
                    (label.is_none() || *name == label)
                        && ((*kind != LabelKind::Other && (is_break || *kind == LabelKind::Loop))
                            || (label.is_some() && is_break))
                });
                if !ok {
                    return bail("Unsyntactic break/continue");
                }
            }
            "Decorator" => {
                if let Some(e) = node.field("expression")
                    && !is_babel_decorator_expression(e)
                {
                    return bail("decorator expression Babel rejects");
                }
            }
            "MetaProperty" => {
                // Babel puts a comment between `import` and `.meta` where Prettier never prints
                // it.
                if node.field("meta").and_then(|m| m.str_of("name")) == Some("import") {
                    let dot = self.text()[node.start()..node.end()]
                        .find('.')
                        .map_or(node.end(), |i| node.start() + i);
                    if self
                        .src
                        .comments
                        .iter()
                        .any(|c| c.start >= node.start() && c.end <= dot)
                    {
                        return bail("comment inside import.meta");
                    }
                }
            }
            _ => {}
        }
        Ok(())
    }

    fn check_ts(&mut self, node: &'a Value, key: &'static str) -> R<()> {
        let text = self.text();
        match node.ty() {
            "Literal" => {
                let raw = node.str_of("raw").unwrap_or("");
                let b = raw.as_bytes();
                // Scanner: "Octal literals are not allowed." / "Decimals with leading zeros are
                // not allowed."
                if b.len() > 1 && b[0] == b'0' && b[1].is_ascii_digit() {
                    return bail("legacy octal literal");
                }
                if (raw.starts_with('"') || raw.starts_with('\'')) && has_octal_escape(raw) {
                    return bail("octal escape in string");
                }
            }
            "AwaitExpression" => self.check_ts_await(node)?,
            "MemberExpression" | "CallExpression"
                if node
                    .field("property")
                    .is_some_and(|p| p.is("PrivateIdentifier"))
                    && optional_chain_below(node) =>
            {
                // `parsePropertyAccessExpressionRest`.
                return bail("private name in an optional chain");
            }
            "PropertyDefinition" | "VariableDeclarator" | "TSAbstractPropertyDefinition"
                if node.flag("definite") =>
            {
                // A definite `!` must be on the name's line.
                let name = node.field("key").or_else(|| node.field("id"));
                let name_end = name
                    .map(|n| n.field("typeAnnotation").map_or(n.end(), |t| t.start()))
                    .unwrap_or(node.start());
                let (bang, newline) = skip_trivia(text, name_end.min(text.len()));
                if newline && text.as_bytes().get(bang) == Some(&b'!') {
                    return bail("definite assignment on the next line");
                }
            }
            "TSParenthesizedType" => {
                // `parseFunctionOrConstructorTypeToError`: `|(a) => b` is a function type that
                // must be parenthesized; oxc reads `|(a)` and an arrow.
                let before = &text[..self.skip_trivia_back(node.start())];
                let (after, _) = skip_trivia(text, node.end());
                if before.ends_with(['|', '&']) && text[after..].starts_with("=>") {
                    return bail("function type in a union or intersection");
                }
            }
            "TSConditionalType" if self.in_disallow_conditional_types(key) => {
                return bail("conditional type where TypeScript disallows one");
            }
            "TSTypeParameterInstantiation" if text[node.start()..].starts_with("<<") => {
                // Heritage clauses and JSX tags take type arguments without rescanning `<<`.
                if self.parent().is_some_and(|p| {
                    matches!(
                        p.ty(),
                        "ClassDeclaration"
                            | "ClassExpression"
                            | "TSClassImplements"
                            | "TSInterfaceHeritage"
                            | "JSXOpeningElement"
                    )
                }) {
                    return bail("`<<` before type arguments");
                }
            }
            "TSTypeAliasDeclaration" | "TSInterfaceDeclaration" | "TSModuleDeclaration" => {
                // `export type\nX = ...`: TypeScript wants the name on the keyword's line.
                if let Some(id) = node.field("id") {
                    let head = &text[node.start().min(id.start())..id.start()];
                    let kw = ["type", "interface", "namespace", "module"]
                        .iter()
                        .filter_map(|kw| head.rfind(kw).map(|i| i + kw.len()))
                        .max();
                    if let Some(kw_end) = kw
                        && head[kw_end..].contains(LINE_TERMINATORS)
                    {
                        return bail("line break after declaration keyword");
                    }
                }
            }
            "MemberExpression" if !node.flag("computed") => {
                // `parseRightSideOfDot`: a name on the next line followed by another name on
                // its line is read as a missing name.
                if let Some(prop) = node.field("property") {
                    let dot_end = text[..prop.start()].rfind('.').map_or(0, |i| i + 1);
                    let (_, newline) = skip_trivia(text, dot_end);
                    let (next, newline_after) = skip_trivia(text, prop.end());
                    if newline
                        && !newline_after
                        && text[next..].chars().next().is_some_and(is_ident_start)
                    {
                        return bail("member name on the next line");
                    }
                }
            }
            "ClassDeclaration" | "ClassExpression" => self.check_ts_class(node)?,
            "MethodDefinition" | "TSAbstractMethodDefinition" => self.check_ts_method(node)?,
            "PropertyDefinition" | "TSAbstractPropertyDefinition" => {
                // `nodeCanBeDecorated`: an abstract property only in a class declaration.
                if !node.list("decorators").is_empty()
                    && (node.is("TSAbstractPropertyDefinition") || node.flag("abstract"))
                    && !self
                        .member_class()
                        .is_some_and(|c| c.is("ClassDeclaration"))
                {
                    return bail("decorator on abstract property of a class expression");
                }
            }
            "AssignmentExpression" => {
                // `<T>x = 1`: a type assertion is not a left-hand side.
                if node
                    .field("left")
                    .is_some_and(|l| l.is("TSTypeAssertion") && l.start() == node.start())
                {
                    return bail("assignment to a type assertion");
                }
            }
            "ArrowFunctionExpression" => {
                // `parseArrowFunctionExpressionBody`: `@` starts a statement, not an expression.
                if let Some(body) = node.field("body")
                    && !body.is("BlockStatement")
                    && text.as_bytes().get(body.start()) == Some(&b'@')
                {
                    return bail("arrow body starts with a decorator");
                }
            }
            // `extends (...)` is a left-hand side expression, where no arrow is tried.
            "ParenthesizedExpression" if key != "superClass" => {
                self.check_ts_paren_then_brace(node)?
            }
            "BinaryExpression" => self.check_ts_type_arguments(node)?,
            // TypeScript wants `</` as one token followed by the name; after `</` a `/` can
            // only be oxc reading a comment (`</*`, `<//`) that TypeScript does not.
            "JSXClosingElement" | "JSXClosingFragment"
                if !text[node.start()..].starts_with("</")
                    || text[node.start() + 2..].starts_with(['/', '*']) =>
            {
                return bail("space or comment in JSX closing tag");
            }
            _ => {}
        }
        if self.src.tsx {
            self.check_tsx_less_than_slash(node)?;
        }
        Ok(())
    }

    /// The end of the token before `pos`: skips whitespace and comments backwards.
    fn skip_trivia_back(&self, mut pos: usize) -> usize {
        let text = self.text();
        loop {
            let trimmed = text[..pos].trim_end().len();
            match self.src.comments.iter().find(|c| c.end == trimmed) {
                Some(c) => pos = c.start,
                None => return trimmed,
            }
        }
    }

    /// `disallowConditionalTypesAnd`: a conditional type's `extends` type, and the parameter
    /// types and type parameters of a function type there (`parseType` without a reset), cannot
    /// themselves be conditional types. Every other type position resets the context.
    fn in_disallow_conditional_types(&self, mut key: &'static str) -> bool {
        let mut seen_function = false;
        for (parent, parent_key) in self.stack.iter().rev() {
            let up = match (parent.ty(), key) {
                ("TSTypeAnnotation", "typeAnnotation")
                | ("TSTypeParameter", "constraint" | "default")
                | ("TSTypeParameterDeclaration", "params") => true,
                (
                    "Identifier" | "RestElement" | "ObjectPattern" | "ArrayPattern"
                    | "AssignmentPattern",
                    "typeAnnotation",
                ) => true,
                ("TSFunctionType" | "TSConstructorType", "params" | "typeParameters") => {
                    seen_function = true;
                    true
                }
                ("TSConditionalType", "extendsType") => return seen_function,
                _ => false,
            };
            if !up {
                return false;
            }
            key = parent_key;
        }
        false
    }

    /// In `.tsx` the scanner reads `</` as one token wherever a `<` starts (not before `/*`;
    /// in JSX children, before `/*` too), so `<//c`, `a</re/` and a child `</*c*/a>` fail.
    fn check_tsx_less_than_slash(&self, node: &Value) -> R<()> {
        let text = self.text();
        let lt = match node.ty() {
            "JSXOpeningElement" | "JSXOpeningFragment" => {
                let child = self.stack.last().is_some_and(|(_, k)| *k == "children");
                if child && text[node.start()..].starts_with("</") {
                    return bail("JSX child read as a closing tag");
                }
                node.start()
            }
            "TSTypeParameterDeclaration" | "TSTypeParameterInstantiation" => node.start(),
            "BinaryExpression" if node.str_of("operator") == Some("<") => {
                match node.field("left") {
                    Some(left) => skip_trivia(text, left.end()).0,
                    None => return Ok(()),
                }
            }
            _ => return Ok(()),
        };
        let rest = &text[lt..];
        if rest.starts_with("</") && !rest.starts_with("</*") {
            return bail("`</` token in .tsx");
        }
        Ok(())
    }

    /// typescript-estree `#checkModifiers` (`nodeCanBeDecorated`, parameter properties).
    fn check_ts_method(&self, node: &Value) -> R<()> {
        let Some(value) = node.field("value") else {
            return Ok(());
        };
        let has_body = value.field("body").is_some();
        let class = self.member_class();
        let kind = node.str_of("kind");
        for param in value.list("params") {
            let inner = param.field("parameter");
            let decorated = !param.list("decorators").is_empty()
                || inner.is_some_and(|i| !i.list("decorators").is_empty());
            if decorated
                && (!has_body
                    || kind == Some("get")
                    || !class.is_some_and(|c| c.is("ClassDeclaration")))
            {
                return bail("parameter decorator TypeScript rejects");
            }
            if param.is("TSParameterProperty") && !(kind == Some("constructor") && has_body) {
                return bail("parameter property outside a constructor implementation");
            }
        }
        Ok(())
    }

    fn check_ts_class(&self, node: &Value) -> R<()> {
        // `isValidHeritageClauseObjectLiteral`: `extends {}` must be followed by `,`, `{`,
        // `extends` or `implements`.
        let Some(sup) = node.field("superClass") else {
            return Ok(());
        };
        let text = self.text();
        if text.as_bytes().get(sup.start()) != Some(&b'{') {
            return Ok(());
        }
        let (close, _) = skip_trivia(text, sup.start() + 1);
        if text.as_bytes().get(close) != Some(&b'}') {
            return Ok(());
        }
        let (next, _) = skip_trivia(text, close + 1);
        let rest = &text[next..];
        let word_is = |w: &str| {
            rest.starts_with(w) && !rest[w.len()..].chars().next().is_some_and(is_ident_part)
        };
        if rest.starts_with([',', '{']) || word_is("extends") || word_is("implements") {
            return Ok(());
        }
        bail("'extends' list cannot be empty")
    }

    /// TypeScript reads `await` as an identifier where it is not in an await context, unless
    /// an identifier, keyword or literal follows on the same line (`isAwaitExpression`). oxc
    /// keeps the await context in enum initializers after `const`/`export`. At the top level
    /// TypeScript reparses statements with `await`, and when the first parse ended a statement
    /// elsewhere (`(await\nx)`) the reparse repeats the statements that follow.
    fn check_ts_await(&self, node: &Value) -> R<()> {
        let mut in_enum = false;
        for (n, _) in self.stack.iter().rev() {
            if is_function(n) {
                return Ok(());
            }
            if n.is("TSEnumDeclaration") {
                in_enum = true;
                break;
            }
        }
        let text = self.text();
        let (next, newline) = skip_trivia(text, node.start() + "await".len());
        let operand_follows = text[next..]
            .chars()
            .next()
            .is_some_and(|c| is_ident_start(c) || c.is_ascii_digit() || "'\"`".contains(c));
        if in_enum && (newline || !operand_follows) {
            return bail("await in an enum initializer");
        }
        if newline && operand_follows {
            return bail("top-level await before a line break");
        }
        // `await {`: the first parse reads `await` as a name and ends the statement at `{`.
        if text.as_bytes().get(next) == Some(&b'{') {
            return bail("top-level await before an object literal");
        }
        Ok(())
    }

    /// `isParenthesizedArrowFunctionExpression`: `(a)`, `(a, b)`, `(a = 1)`, `({...})` or
    /// `([...])` followed by `{` is read as an arrow function missing its `=>`.
    fn check_ts_paren_then_brace(&self, node: &Value) -> R<()> {
        let text = self.text();
        let (after, _) = skip_trivia(text, node.end());
        if text.as_bytes().get(after) != Some(&b'{') {
            return Ok(());
        }
        let (first, _) = skip_trivia(text, node.start() + 1);
        let rest = &text[first..];
        let Some(c) = rest.chars().next() else {
            return Ok(());
        };
        if c == '{' || c == '[' {
            return bail("parenthesized expression read as arrow parameters");
        }
        if !is_ident_start(c) {
            return Ok(());
        }
        let word_end = first
            + rest
                .char_indices()
                .find(|&(_, ch)| !is_ident_part(ch))
                .map_or(rest.len(), |(i, _)| i);
        let (next, _) = skip_trivia(text, word_end);
        let tail = &text[next..];
        if tail.starts_with(',')
            || tail.starts_with(')')
            || (tail.starts_with('=') && !tail.starts_with("==") && !tail.starts_with("=>"))
        {
            return bail("parenthesized expression read as arrow parameters");
        }
        Ok(())
    }

    /// `parseTypeArgumentsInExpression`: after `<` (or `<<`, rescanned), TypeScript tries type
    /// arguments and keeps them, parse errors included, when the closing `>` is followed by a
    /// token that `canFollowTypeArgumentsInExpression` allows. Refused when only names, dots,
    /// commas, `(` and `[` stand before a `>` followed by a line break, or when the right
    /// operand starts with `(`, `<` or `[` (a type TypeScript parses with error recovery) and
    /// the first `>` after the operator is followed by such a token.
    fn check_ts_type_arguments(&self, node: &Value) -> R<()> {
        let Some(operator @ ("<" | "<<")) = node.str_of("operator") else {
            return Ok(());
        };
        let (Some(left), Some(right)) = (node.field("left"), node.field("right")) else {
            return Ok(());
        };
        let text = self.text();
        let (op, _) = skip_trivia(text, left.end());
        let after_op = op + operator.len();
        if names_then_closing_angle_at_line_end(text, after_op, right.end()) {
            return bail("expression read as type arguments");
        }
        let (first, _) = skip_trivia(text, after_op);
        if first >= right.end() || !text[first..].starts_with(['(', '<', '[']) {
            return Ok(());
        }
        // `reScanLessThanToken` turns `<<` into `<` and scans on from its second character:
        // `<<<` yields `<<`, which cannot start a type, and the attempt fails.
        if operator == "<<" && text[op + 2..].starts_with('<') {
            return Ok(());
        }
        let Some(gt) = first_closing_angle(text, after_op) else {
            return Ok(());
        };
        if can_follow_type_arguments(text, gt + 1) {
            return bail("expression read as type arguments");
        }
        Ok(())
    }
}

/// Whether only names, dots, commas, `(` and `[` stand from `pos` to a lone `>` (before `end`)
/// that a line break follows.
fn names_then_closing_angle_at_line_end(text: &str, mut pos: usize, end: usize) -> bool {
    loop {
        let (next, _) = skip_trivia(text, pos);
        if next >= end {
            return false;
        }
        let Some(c) = text[next..].chars().next() else {
            return false;
        };
        if c == '>' {
            return !text[next + 1..].starts_with(['>', '=']) && skip_trivia(text, next + 1).1;
        }
        if !(is_ident_part(c) || matches!(c, '.' | ',' | '(' | '[')) {
            return false;
        }
        pos = next + c.len_utf8();
    }
}

/// Whether an optional link (`?.`) comes before `node` in its own chain.
fn optional_chain_below(node: &Value) -> bool {
    let mut n = node;
    loop {
        if n.flag("optional") {
            return true;
        }
        let inner = match n.ty() {
            "MemberExpression" => n.field("object"),
            "CallExpression" => n.field("callee"),
            // `a?.b!.#c`: a non-null assertion stays inside the chain, and oxc wraps its operand
            // in a `ChainExpression` of its own. Parentheses (kept until `postprocess`) end it.
            "TSNonNullExpression" | "ChainExpression" => n.field("expression"),
            _ => None,
        };
        let Some(inner) = inner else {
            return false;
        };
        n = inner;
    }
}

/// The first lone `>` token from `pos` (not `>>`, `>=` or the end of `=>`), skipping strings,
/// templates and comments; `None` at a `;` or at a bracket that closes before `pos`.
fn first_closing_angle(text: &str, mut pos: usize) -> Option<usize> {
    let b = text.as_bytes();
    let mut depth = 0i32;
    let mut prev = 0u8;
    loop {
        pos = skip_trivia(text, pos).0;
        let c = *b.get(pos)?;
        match c {
            b'\'' | b'"' | b'`' => {
                let mut i = pos + 1;
                while i < b.len() && b[i] != c {
                    i += if b[i] == b'\\' { 2 } else { 1 };
                }
                pos = i + 1;
            }
            b'(' | b'[' | b'{' => {
                depth += 1;
                pos += 1;
            }
            b')' | b']' | b'}' => {
                depth -= 1;
                if depth < 0 {
                    return None;
                }
                pos += 1;
            }
            b';' => return None,
            b'>' => {
                if prev == b'=' || prev == b'>' || matches!(b.get(pos + 1), Some(b'>' | b'=')) {
                    while b.get(pos) == Some(&b'>') {
                        pos += 1;
                    }
                    pos += usize::from(b.get(pos) == Some(&b'='));
                } else {
                    return Some(pos);
                }
            }
            _ => pos += text[pos..].chars().next().map_or(1, char::len_utf8),
        }
        prev = c;
    }
}

/// `canFollowTypeArgumentsInExpression`.
fn can_follow_type_arguments(text: &str, pos: usize) -> bool {
    let (next, newline) = skip_trivia(text, pos);
    let Some(c) = text[next..].chars().next() else {
        return true;
    };
    match c {
        '(' | '`' => true,
        '<' | '>' | '+' | '-' => false,
        _ if newline => true,
        // Binary operators and tokens that cannot start an expression.
        ')' | ']' | '}' | ';' | ',' | ':' | '?' | '.' | '=' | '|' | '&' | '*' | '%' | '^' | '/' => {
            true
        }
        _ => false,
    }
}

fn is_ident_start(c: char) -> bool {
    c.is_ascii_alphabetic() || c == '_' || c == '$' || c == '\\' || !c.is_ascii()
}

fn is_ident_part(c: char) -> bool {
    is_ident_start(c) || c.is_ascii_digit()
}

/// TypeScript's scanner rejects `\0` followed by a digit, `\1`-`\7` and `\8`/`\9` in strings.
fn has_octal_escape(raw: &str) -> bool {
    let b = raw.as_bytes();
    let mut i = 0;
    while i + 1 < b.len() {
        if b[i] == b'\\' {
            let next = b[i + 1];
            if (b'1'..=b'9').contains(&next)
                || (next == b'0' && b.get(i + 2).is_some_and(u8::is_ascii_digit))
            {
                return true;
            }
            i += 2;
        } else {
            i += 1;
        }
    }
    false
}

/// Babel's `parseDecorator` with the `decorators` plugin: `@(expr)`, `@(expr)(args)`, or a
/// dotted name with an optional single call.
fn is_babel_decorator_expression(e: &Value) -> bool {
    if e.is("ParenthesizedExpression") {
        return true;
    }
    let e = if e.is("CallExpression") && !e.flag("optional") {
        match e.field("callee") {
            Some(c) if c.is("ParenthesizedExpression") => return true,
            Some(c) => c,
            None => return false,
        }
    } else {
        e
    };
    is_babel_decorator_member(e)
}

fn is_babel_decorator_member(e: &Value) -> bool {
    match e.ty() {
        "Identifier" => true,
        "MemberExpression" => {
            !e.flag("computed")
                && !e.flag("optional")
                && e.field("property")
                    .is_some_and(|p| p.is("Identifier") || p.is("PrivateIdentifier"))
                && e.field("object").is_some_and(is_babel_decorator_member)
        }
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn refuses(text: &str, path: &str) -> bool {
        let opts =
            crate::Options::from_cli(&["--stdin-filepath".into(), path.into()]).expect("options");
        crate::format(text, &opts).is_err()
    }

    #[test]
    fn identifier_tables() {
        assert!(is_valid_identifier_name("café", false, false));
        assert!(!is_valid_identifier_name("a\u{58b}", false, false));
        assert!(!is_valid_identifier_name("a\u{58b}", true, false));
    }

    #[test]
    fn skips_trivia() {
        assert_eq!(skip_trivia("  /* a */ x", 0), (10, false));
        assert_eq!(skip_trivia(" // a\n x", 0), (7, true));
    }

    #[test]
    fn refuses_what_prettier_rejects() {
        for (text, path) in [
            ("{break}", "a.js"),
            ("a:a:;", "a.js"),
            ("x={__proto__:1,'__proto__':2}", "a.js"),
            ("{\"__proto__\":1,\"__proto__\":2}", "a.json"),
            ("@this.t\nclass A{}", "a.js"),
            ("class{}", "a.js"),
            ("[09]", "a.ts"),
            ("export type\nR=L", "a.ts"),
            ("(m=1())\n{}", "a.ts"),
            ("<div>< /div>", "a.tsx"),
            ("x=a<//c\nb", "a.tsx"),
            ("class q{s(){o?.#s}}", "a.ts"),
            ("type d<T>=T extends(a:r extends K?v:K)=>i?U:T", "a.ts"),
            ("f<<(r>(i))", "a.ts"),
            ("g<(a?b:c)>\n{}", "a.ts"),
            ("class t{o\n!:r}", "a.ts"),
            ("((t:t):&(b)=>1)", "a.ts"),
            ("<div></**//div>", "a.tsx"),
            ("<ul><//\n/ul>", "a.tsx"),
            ("():|/**/(b)=>l", "a.ts"),
            ("():|//c\n(b)=>l", "a.ts"),
            ("class e{r(){a?.b!.#s}}", "a.ts"),
        ] {
            assert!(refuses(text, path), "{text}");
        }
        for (text, path) in [
            ("a:for(;;){break a}", "a.js"),
            ("{break}", "a.ts"),
            ("export default class{}", "a.js"),
            ("(<x>a) = 1", "a.ts"),
            ("class a extends ({}!) {}", "a.ts"),
            ("f<<<T>(x)", "a.ts"),
            ("x={__proto__:1,__proto__:2}", "a.ts"),
            ("x={__proto__,__proto__:1,__proto__(){}}", "a.js"),
            ("({__proto__:a,__proto__:b}=c)", "a.js"),
            ("x=[1e400,-1e400,'1e+400']", "a.js"),
            ("type T=1e999", "a.ts"),
            ("1 << (n - 1)", "a.ts"),
            ("x < (y > 0 ? 1 : 2)", "a.ts"),
            ("type d<T>=T extends(a:(r extends K?v:K))=>i?U:T", "a.ts"),
        ] {
            assert!(!refuses(text, path), "{text}");
        }
    }
}
