//! An ESTree arena built from oxc's ESTree JSON, with espree's ranges (UTF-16 code units) and
//! parent links. Traversal order follows eslint-visitor-keys 5.0.1.

use serde::de::{self, DeserializeSeed, Deserializer, MapAccess, SeqAccess, Visitor};
use serde_json::Value;

pub type NodeId = u32;

#[derive(Debug, Clone)]
pub enum Prop {
    Node(NodeId),
    List(Vec<Option<NodeId>>),
    Str(String),
    Num(f64),
    Bool(bool),
    Null,
    Other(Value),
}

#[derive(Debug, Clone)]
pub struct Node {
    pub kind: &'static str,
    pub start: u32,
    pub end: u32,
    pub parent: Option<NodeId>,
    pub props: Vec<(&'static str, Prop)>,
}

#[derive(Debug, Default)]
pub struct Ast {
    pub nodes: Vec<Node>,
}

pub fn visitor_keys(kind: &str) -> &'static [&'static str] {
    match kind {
        "ArrayExpression" | "ArrayPattern" => &["elements"],
        "ArrowFunctionExpression" => &["params", "body"],
        "AssignmentExpression" | "AssignmentPattern" | "BinaryExpression" | "LogicalExpression" => {
            &["left", "right"]
        }
        "AwaitExpression" | "RestElement" | "ReturnStatement" | "SpreadElement"
        | "ThrowStatement" | "UnaryExpression" | "UpdateExpression" | "YieldExpression"
        | "JSXSpreadAttribute" => &["argument"],
        "BlockStatement" | "ClassBody" | "Program" | "StaticBlock" => &["body"],
        "BreakStatement" | "ContinueStatement" => &["label"],
        "CallExpression" | "NewExpression" => &["callee", "arguments"],
        "CatchClause" => &["param", "body"],
        "ChainExpression" | "ExpressionStatement" | "JSXExpressionContainer" | "JSXSpreadChild" => {
            &["expression"]
        }
        "ClassDeclaration" | "ClassExpression" => &["id", "superClass", "body"],
        "ConditionalExpression" | "IfStatement" => &["test", "consequent", "alternate"],
        "DoWhileStatement" => &["body", "test"],
        "ExportAllDeclaration" => &["exported", "source", "attributes"],
        "ExportDefaultDeclaration" => &["declaration"],
        "ExportNamedDeclaration" => &["declaration", "specifiers", "source", "attributes"],
        "ExportSpecifier" => &["local", "exported"],
        "ForInStatement" | "ForOfStatement" => &["left", "right", "body"],
        "ForStatement" => &["init", "test", "update", "body"],
        "FunctionDeclaration" | "FunctionExpression" => &["id", "params", "body"],
        "ImportAttribute" => &["key", "value"],
        "ImportDeclaration" => &["specifiers", "source", "attributes"],
        "ImportDefaultSpecifier" | "ImportNamespaceSpecifier" => &["local"],
        "ImportExpression" => &["source", "options"],
        "ImportSpecifier" => &["imported", "local"],
        "JSXAttribute" => &["name", "value"],
        "JSXClosingElement" => &["name"],
        "JSXElement" => &["openingElement", "children", "closingElement"],
        "JSXFragment" => &["openingFragment", "children", "closingFragment"],
        "JSXMemberExpression" | "MemberExpression" => &["object", "property"],
        "JSXNamespacedName" => &["namespace", "name"],
        "JSXOpeningElement" => &["name", "attributes"],
        "LabeledStatement" => &["label", "body"],
        "MetaProperty" => &["meta", "property"],
        "MethodDefinition" | "Property" | "PropertyDefinition" => &["key", "value"],
        "ObjectExpression" | "ObjectPattern" => &["properties"],
        "SequenceExpression" => &["expressions"],
        "SwitchCase" => &["test", "consequent"],
        "SwitchStatement" => &["discriminant", "cases"],
        "TaggedTemplateExpression" => &["tag", "quasi"],
        "TemplateLiteral" => &["quasis", "expressions"],
        "TryStatement" => &["block", "handler", "finalizer"],
        "VariableDeclaration" => &["declarations"],
        "VariableDeclarator" => &["id", "init"],
        "WhileStatement" => &["test", "body"],
        "WithStatement" => &["object", "body"],
        _ => &[],
    }
}

#[derive(Default)]
pub struct Fnv(u64);

impl std::hash::Hasher for Fnv {
    fn finish(&self) -> u64 {
        self.0
    }

    fn write(&mut self, bytes: &[u8]) {
        let mut h = if self.0 == 0 {
            0xcbf2_9ce4_8422_2325
        } else {
            self.0
        };
        for b in bytes {
            h ^= u64::from(*b);
            h = h.wrapping_mul(0x0100_0000_01b3);
        }
        self.0 = h;
    }
}

pub type FnvBuild = std::hash::BuildHasherDefault<Fnv>;

type FnvMap<K, V> = std::collections::HashMap<K, V, FnvBuild>;

fn intern(s: &str) -> &'static str {
    use std::cell::RefCell;
    thread_local! {
        static KEYS: RefCell<FnvMap<String, &'static str>> = RefCell::new(FnvMap::default());
    }
    KEYS.with(|k| {
        let mut k = k.borrow_mut();
        if let Some(v) = k.get(s) {
            return *v;
        }
        let leaked: &'static str = Box::leak(s.to_string().into_boxed_str());
        k.insert(s.to_string(), leaked);
        leaked
    })
}

struct Build<'b> {
    ast: &'b mut Ast,
    utf16: &'b [u32],
    parent: Option<NodeId>,
}

impl<'de> DeserializeSeed<'de> for Build<'_> {
    type Value = Prop;

    fn deserialize<D: Deserializer<'de>>(self, d: D) -> Result<Prop, D::Error> {
        d.deserialize_any(self)
    }
}

struct Key;

impl<'de> DeserializeSeed<'de> for Key {
    type Value = &'static str;

    fn deserialize<D: Deserializer<'de>>(self, d: D) -> Result<&'static str, D::Error> {
        d.deserialize_str(self)
    }
}

impl Visitor<'_> for Key {
    type Value = &'static str;

    fn expecting(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        f.write_str("a key")
    }

    fn visit_str<E: de::Error>(self, v: &str) -> Result<&'static str, E> {
        Ok(intern(v))
    }
}

impl<'de> Visitor<'de> for Build<'_> {
    type Value = Prop;

    fn expecting(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        f.write_str("ESTree JSON")
    }

    fn visit_unit<E: de::Error>(self) -> Result<Prop, E> {
        Ok(Prop::Null)
    }

    fn visit_none<E: de::Error>(self) -> Result<Prop, E> {
        Ok(Prop::Null)
    }

    fn visit_bool<E: de::Error>(self, v: bool) -> Result<Prop, E> {
        Ok(Prop::Bool(v))
    }

    fn visit_i64<E: de::Error>(self, v: i64) -> Result<Prop, E> {
        Ok(Prop::Num(v as f64))
    }

    fn visit_u64<E: de::Error>(self, v: u64) -> Result<Prop, E> {
        Ok(Prop::Num(v as f64))
    }

    fn visit_f64<E: de::Error>(self, v: f64) -> Result<Prop, E> {
        Ok(Prop::Num(v))
    }

    fn visit_str<E: de::Error>(self, v: &str) -> Result<Prop, E> {
        Ok(Prop::Str(v.to_string()))
    }

    fn visit_string<E: de::Error>(self, v: String) -> Result<Prop, E> {
        Ok(Prop::Str(v))
    }

    fn visit_seq<A: SeqAccess<'de>>(self, mut seq: A) -> Result<Prop, A::Error> {
        let mut list: Vec<Option<NodeId>> = Vec::new();
        let mut scalars: Vec<Value> = Vec::new();
        while let Some(p) = seq.next_element_seed(Build {
            ast: &mut *self.ast,
            utf16: self.utf16,
            parent: self.parent,
        })? {
            match p {
                Prop::Node(n) => list.push(Some(n)),
                Prop::Null => {
                    list.push(None);
                    scalars.push(Value::Null);
                }
                other => scalars.push(prop_value(other)),
            }
        }
        if scalars.len() > list.iter().filter(|x| x.is_none()).count() {
            return Ok(Prop::Other(Value::Array(scalars)));
        }
        Ok(Prop::List(list))
    }

    fn visit_map<A: MapAccess<'de>>(self, mut map: A) -> Result<Prop, A::Error> {
        let Some(first) = map.next_key_seed(Key)? else {
            return Ok(Prop::Other(Value::Object(serde_json::Map::new())));
        };
        if first != "type" {
            let mut obj = serde_json::Map::new();
            obj.insert(first.to_string(), map.next_value::<Value>()?);
            while let Some(k) = map.next_key_seed(Key)? {
                obj.insert(k.to_string(), map.next_value::<Value>()?);
            }
            return Ok(Prop::Other(Value::Object(obj)));
        }
        let kind: std::borrow::Cow<'de, str> = map.next_value()?;
        let id = self.ast.nodes.len() as NodeId;
        self.ast.nodes.push(Node {
            kind: intern(&kind),
            start: 0,
            end: 0,
            parent: self.parent,
            props: Vec::new(),
        });
        let (mut start, mut end) = (0usize, 0usize);
        let mut props = Vec::new();
        while let Some(k) = map.next_key_seed(Key)? {
            match k {
                "start" => start = map.next_value()?,
                "end" => end = map.next_value()?,
                "range" => {
                    map.next_value::<de::IgnoredAny>()?;
                }
                _ => {
                    let p = map.next_value_seed(Build {
                        ast: &mut *self.ast,
                        utf16: self.utf16,
                        parent: Some(id),
                    })?;
                    props.push((k, p));
                }
            }
        }
        let to16 = |b: usize| {
            self.utf16
                .get(b)
                .copied()
                .ok_or_else(|| de::Error::custom("offset out of range"))
        };
        let (s, e) = (to16(start)?, to16(end)?);
        let n = &mut self.ast.nodes[id as usize];
        n.start = s;
        n.end = e;
        n.props = props;
        Ok(Prop::Node(id))
    }
}

fn prop_value(p: Prop) -> Value {
    match p {
        Prop::Null => Value::Null,
        Prop::Bool(b) => Value::Bool(b),
        Prop::Num(n) => serde_json::Number::from_f64(n).map_or(Value::Null, Value::Number),
        Prop::Str(s) => Value::String(s),
        Prop::Other(v) => v,
        Prop::Node(_) | Prop::List(_) => Value::Null,
    }
}

impl Ast {
    /// Reads oxc's ESTree JSON straight into the arena.
    pub fn from_json(json: &str, utf16: &[u32]) -> Result<(Ast, NodeId), serde_json::Error> {
        let mut ast = Ast {
            nodes: Vec::with_capacity(json.len() / 64),
        };
        let mut de = serde_json::Deserializer::from_str(json);
        de.disable_recursion_limit();
        let root = Build {
            ast: &mut ast,
            utf16,
            parent: None,
        }
        .deserialize(&mut de)?;
        match root {
            Prop::Node(id) => Ok((ast, id)),
            _ => Err(de::Error::custom("root is not a node")),
        }
    }

    pub fn from_estree(root: &Value, utf16: &[u32]) -> Result<(Ast, NodeId), String> {
        let mut ast = Ast {
            nodes: Vec::with_capacity(1024),
        };
        let id = ast.build(root, None, utf16)?;
        Ok((ast, id))
    }

    fn build(
        &mut self,
        v: &Value,
        parent: Option<NodeId>,
        utf16: &[u32],
    ) -> Result<NodeId, String> {
        let obj = v.as_object().ok_or("node is not an object")?;
        let kind = obj
            .get("type")
            .and_then(Value::as_str)
            .ok_or("node without type")?;
        let start = obj
            .get("start")
            .and_then(Value::as_u64)
            .ok_or("node without start")? as usize;
        let end = obj
            .get("end")
            .and_then(Value::as_u64)
            .ok_or("node without end")? as usize;
        let to16 = |b: usize| utf16.get(b).copied().ok_or("offset out of range");
        let id = self.nodes.len() as NodeId;
        self.nodes.push(Node {
            kind: intern(kind),
            start: to16(start)?,
            end: to16(end)?,
            parent,
            props: Vec::new(),
        });
        let mut props = Vec::with_capacity(obj.len());
        for (k, val) in obj {
            if matches!(k.as_str(), "type" | "start" | "end" | "range") {
                continue;
            }
            let p = match val {
                Value::Null => Prop::Null,
                Value::Bool(b) => Prop::Bool(*b),
                Value::Number(n) => Prop::Num(n.as_f64().unwrap_or(f64::NAN)),
                Value::String(s) => Prop::Str(s.clone()),
                Value::Array(items) => {
                    if items.iter().all(|i| i.is_null() || is_node(i)) {
                        let mut list = Vec::with_capacity(items.len());
                        for i in items {
                            if i.is_null() {
                                list.push(None);
                            } else {
                                list.push(Some(self.build(i, Some(id), utf16)?));
                            }
                        }
                        Prop::List(list)
                    } else {
                        Prop::Other(val.clone())
                    }
                }
                Value::Object(_) => {
                    if is_node(val) {
                        Prop::Node(self.build(val, Some(id), utf16)?)
                    } else {
                        Prop::Other(val.clone())
                    }
                }
            };
            props.push((intern(k), p));
        }
        self.nodes[id as usize].props = props;
        Ok(id)
    }

    #[inline]
    pub fn node(&self, id: NodeId) -> &Node {
        &self.nodes[id as usize]
    }

    #[inline]
    pub fn kind(&self, id: NodeId) -> &'static str {
        self.nodes[id as usize].kind
    }

    #[inline]
    pub fn is(&self, id: NodeId, kind: &str) -> bool {
        self.nodes[id as usize].kind == kind
    }

    #[inline]
    pub fn parent(&self, id: NodeId) -> Option<NodeId> {
        self.nodes[id as usize].parent
    }

    #[inline]
    pub fn range(&self, id: NodeId) -> (u32, u32) {
        let n = &self.nodes[id as usize];
        (n.start, n.end)
    }

    pub fn prop(&self, id: NodeId, key: &str) -> Option<&Prop> {
        self.nodes[id as usize]
            .props
            .iter()
            .find(|(k, _)| *k == key)
            .map(|(_, p)| p)
    }

    pub fn child(&self, id: NodeId, key: &str) -> Option<NodeId> {
        match self.prop(id, key) {
            Some(Prop::Node(n)) => Some(*n),
            _ => None,
        }
    }

    pub fn list(&self, id: NodeId, key: &str) -> &[Option<NodeId>] {
        match self.prop(id, key) {
            Some(Prop::List(l)) => l,
            _ => &[],
        }
    }

    pub fn nodes_of(&self, id: NodeId, key: &str) -> impl Iterator<Item = NodeId> + '_ {
        self.list(id, key).iter().filter_map(|x| *x)
    }

    pub fn str(&self, id: NodeId, key: &str) -> Option<&str> {
        match self.prop(id, key) {
            Some(Prop::Str(s)) => Some(s),
            _ => None,
        }
    }

    pub fn bool(&self, id: NodeId, key: &str) -> bool {
        matches!(self.prop(id, key), Some(Prop::Bool(true)))
    }

    pub fn num(&self, id: NodeId, key: &str) -> Option<f64> {
        match self.prop(id, key) {
            Some(Prop::Num(n)) => Some(*n),
            _ => None,
        }
    }

    pub fn is_null(&self, id: NodeId, key: &str) -> bool {
        matches!(self.prop(id, key), None | Some(Prop::Null))
    }

    pub fn other(&self, id: NodeId, key: &str) -> Option<&Value> {
        match self.prop(id, key) {
            Some(Prop::Other(v)) => Some(v),
            _ => None,
        }
    }

    pub fn set(&mut self, id: NodeId, key: &str, p: Prop) {
        let props = &mut self.nodes[id as usize].props;
        if let Some(slot) = props.iter_mut().find(|(k, _)| *k == key) {
            slot.1 = p;
        } else {
            props.push((intern(key), p));
        }
    }

    pub fn name(&self, id: NodeId) -> Option<&str> {
        self.str(id, "name")
    }

    /// Children in visitor-key order.
    pub fn children(&self, id: NodeId) -> Vec<NodeId> {
        let mut out = Vec::new();
        for key in visitor_keys(self.kind(id)) {
            match self.prop(id, key) {
                Some(Prop::Node(n)) => out.push(*n),
                Some(Prop::List(l)) => out.extend(l.iter().flatten().copied()),
                _ => {}
            }
        }
        out
    }

    pub fn ancestors(&self, id: NodeId) -> impl Iterator<Item = NodeId> + '_ {
        std::iter::successors(self.parent(id), move |n| self.parent(*n))
    }

    pub fn len(&self) -> usize {
        self.nodes.len()
    }

    pub fn is_empty(&self) -> bool {
        self.nodes.is_empty()
    }
}

fn is_node(v: &Value) -> bool {
    v.as_object()
        .is_some_and(|o| o.get("type").is_some_and(Value::is_string) && o.contains_key("start"))
}
