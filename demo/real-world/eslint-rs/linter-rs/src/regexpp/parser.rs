//! regexpp index.js: `RegExpParser` and `visitRegExpAST`.

use super::validator::{Flags, Handler, Options, Ref, Res, SyntaxError, Validator};

pub type Nid = usize;

#[derive(Debug, Clone)]
pub enum K {
    Pattern {
        alternatives: Vec<Nid>,
    },
    Alternative {
        elements: Vec<Nid>,
    },
    Group {
        modifiers: Option<Nid>,
        alternatives: Vec<Nid>,
    },
    CapturingGroup {
        name: Option<String>,
        alternatives: Vec<Nid>,
        references: Vec<Nid>,
    },
    Quantifier {
        min: f64,
        max: f64,
        greedy: bool,
        element: Nid,
    },
    Assertion {
        kind: &'static str,
        negate: bool,
        alternatives: Vec<Nid>,
    },
    CharacterSet {
        kind: &'static str,
        negate: bool,
        key: Option<String>,
        value: Option<String>,
        strings: bool,
    },
    Character {
        value: u32,
    },
    Backreference {
        reference: Ref,
        ambiguous: bool,
        resolved: Vec<Nid>,
    },
    CharacterClass {
        unicode_sets: bool,
        negate: bool,
        elements: Vec<Nid>,
    },
    CharacterClassRange {
        min: Nid,
        max: Nid,
    },
    ClassIntersection {
        left: Nid,
        right: Nid,
    },
    ClassSubtraction {
        left: Nid,
        right: Nid,
    },
    ExpressionCharacterClass {
        negate: bool,
        expression: Nid,
    },
    ClassStringDisjunction {
        alternatives: Vec<Nid>,
    },
    StringAlternative {
        elements: Vec<Nid>,
    },
    Modifiers {
        add: Option<Nid>,
        remove: Option<Nid>,
    },
    ModifierFlags {
        ignore_case: bool,
        multiline: bool,
        dot_all: bool,
    },
}

#[derive(Debug, Clone)]
pub struct Node {
    pub k: K,
    pub parent: Option<Nid>,
    pub start: usize,
    pub end: usize,
}

impl Node {
    pub fn type_name(&self) -> &'static str {
        match self.k {
            K::Pattern { .. } => "Pattern",
            K::Alternative { .. } => "Alternative",
            K::Group { .. } => "Group",
            K::CapturingGroup { .. } => "CapturingGroup",
            K::Quantifier { .. } => "Quantifier",
            K::Assertion { .. } => "Assertion",
            K::CharacterSet { .. } => "CharacterSet",
            K::Character { .. } => "Character",
            K::Backreference { .. } => "Backreference",
            K::CharacterClass { .. } => "CharacterClass",
            K::CharacterClassRange { .. } => "CharacterClassRange",
            K::ClassIntersection { .. } => "ClassIntersection",
            K::ClassSubtraction { .. } => "ClassSubtraction",
            K::ExpressionCharacterClass { .. } => "ExpressionCharacterClass",
            K::ClassStringDisjunction { .. } => "ClassStringDisjunction",
            K::StringAlternative { .. } => "StringAlternative",
            K::Modifiers { .. } => "Modifiers",
            K::ModifierFlags { .. } => "ModifierFlags",
        }
    }
}

#[derive(Debug, Clone, Default)]
pub struct RegExpAst {
    pub nodes: Vec<Node>,
    pub root: Nid,
    pub source: Vec<u16>,
}

impl RegExpAst {
    pub fn node(&self, id: Nid) -> &Node {
        &self.nodes[id]
    }

    pub fn raw16(&self, id: Nid) -> &[u16] {
        let n = &self.nodes[id];
        let (a, b) = (n.start.min(self.source.len()), n.end.min(self.source.len()));
        if a < b {
            &self.source[a..b]
        } else {
            &self.source[0..0]
        }
    }

    pub fn raw(&self, id: Nid) -> Option<String> {
        String::from_utf16(self.raw16(id)).ok()
    }

    pub fn children(&self, id: Nid) -> Vec<Nid> {
        match &self.nodes[id].k {
            K::Pattern { alternatives }
            | K::CapturingGroup { alternatives, .. }
            | K::ClassStringDisjunction { alternatives } => alternatives.clone(),
            K::Group {
                modifiers,
                alternatives,
            } => modifiers
                .iter()
                .copied()
                .chain(alternatives.iter().copied())
                .collect(),
            K::Assertion {
                kind, alternatives, ..
            } => {
                if *kind == "lookahead" || *kind == "lookbehind" {
                    alternatives.clone()
                } else {
                    Vec::new()
                }
            }
            K::Alternative { elements }
            | K::CharacterClass { elements, .. }
            | K::StringAlternative { elements } => elements.clone(),
            K::Quantifier { element, .. } => vec![*element],
            K::CharacterClassRange { min, max } => vec![*min, *max],
            K::ClassIntersection { left, right } | K::ClassSubtraction { left, right } => {
                vec![*left, *right]
            }
            K::ExpressionCharacterClass { expression, .. } => vec![*expression],
            K::Modifiers { add, remove } => add.iter().chain(remove.iter()).copied().collect(),
            K::CharacterSet { .. }
            | K::Character { .. }
            | K::Backreference { .. }
            | K::ModifierFlags { .. } => Vec::new(),
        }
    }

    pub fn visit(&self, f: &mut dyn FnMut(&RegExpAst, Nid, bool)) {
        self.visit_node(self.root, f);
    }

    fn visit_node(&self, id: Nid, f: &mut dyn FnMut(&RegExpAst, Nid, bool)) {
        f(self, id, true);
        for c in self.children(id) {
            self.visit_node(c, f);
        }
        f(self, id, false);
    }
}

#[derive(Default)]
struct State {
    nodes: Vec<Node>,
    cur: Option<Nid>,
    root: Option<Nid>,
    expression_buffer: Vec<(Nid, Nid)>,
    backreferences: Vec<Nid>,
    capturing_groups: Vec<Nid>,
    unknown: bool,
}

impl State {
    fn add(&mut self, k: K, parent: Option<Nid>, start: usize, end: usize) -> Nid {
        self.nodes.push(Node {
            k,
            parent,
            start,
            end,
        });
        self.nodes.len() - 1
    }

    fn kind(&self, id: Option<Nid>) -> &'static str {
        match id {
            Some(i) => self.nodes[i].type_name(),
            None => "",
        }
    }

    fn elements(&mut self, id: Nid) -> Option<&mut Vec<Nid>> {
        match &mut self.nodes[id].k {
            K::Alternative { elements }
            | K::CharacterClass { elements, .. }
            | K::StringAlternative { elements } => Some(elements),
            _ => None,
        }
    }

    fn alternatives(&mut self, id: Nid) -> Option<&mut Vec<Nid>> {
        match &mut self.nodes[id].k {
            K::Pattern { alternatives }
            | K::Group { alternatives, .. }
            | K::CapturingGroup { alternatives, .. }
            | K::Assertion { alternatives, .. }
            | K::ClassStringDisjunction { alternatives } => Some(alternatives),
            _ => None,
        }
    }

    fn push_element(&mut self, parent: Nid, child: Nid) {
        match self.elements(parent) {
            Some(e) => e.push(child),
            None => self.unknown = true,
        }
    }

    fn pop_element(&mut self, parent: Nid) -> Option<Nid> {
        self.elements(parent).and_then(Vec::pop)
    }

    fn leave(&mut self, end: usize, ok: bool) {
        let Some(node) = self.cur else {
            self.unknown = true;
            return;
        };
        if !ok {
            self.unknown = true;
            return;
        }
        self.nodes[node].end = end;
        self.cur = self.nodes[node].parent;
    }

    fn is_class_set_operand(&self, id: Nid) -> bool {
        matches!(
            self.nodes[id].k,
            K::Character { .. }
                | K::CharacterSet { .. }
                | K::CharacterClass { .. }
                | K::ExpressionCharacterClass { .. }
                | K::ClassStringDisjunction { .. }
        )
    }

    fn set_parent(&mut self, id: Nid, parent: Nid) {
        self.nodes[id].parent = Some(parent);
    }

    fn class_op(&mut self, start: usize, end: usize, intersection: bool) {
        let Some(parent) = self.cur else {
            self.unknown = true;
            return;
        };
        if !matches!(
            self.nodes[parent].k,
            K::CharacterClass {
                unicode_sets: true,
                ..
            }
        ) {
            self.unknown = true;
            return;
        }
        let right = self.pop_element(parent);
        let buffered = self
            .expression_buffer
            .iter()
            .position(|(c, _)| *c == parent);
        let left = match buffered {
            Some(i) => Some(self.expression_buffer[i].1),
            None => self.pop_element(parent),
        };
        let (Some(left), Some(right)) = (left, right) else {
            self.unknown = true;
            return;
        };
        let left_kind = self.nodes[left].type_name();
        let (same, other) = if intersection {
            ("ClassIntersection", "ClassSubtraction")
        } else {
            ("ClassSubtraction", "ClassIntersection")
        };
        if left_kind == other
            || (left_kind != same && !self.is_class_set_operand(left))
            || !self.is_class_set_operand(right)
        {
            self.unknown = true;
            return;
        }
        let k = if intersection {
            K::ClassIntersection { left, right }
        } else {
            K::ClassSubtraction { left, right }
        };
        let node = self.add(k, Some(parent), start, end);
        self.set_parent(left, node);
        self.set_parent(right, node);
        match buffered {
            Some(i) => self.expression_buffer[i].1 = node,
            None => self.expression_buffer.push((parent, node)),
        }
    }
}

fn modifier_flags(f: &Flags) -> K {
    K::ModifierFlags {
        ignore_case: f.ignore_case,
        multiline: f.multiline,
        dot_all: f.dot_all,
    }
}

impl Handler for State {
    fn on_pattern_enter(&mut self, start: usize) {
        self.nodes.clear();
        self.expression_buffer.clear();
        let id = self.add(
            K::Pattern {
                alternatives: Vec::new(),
            },
            None,
            start,
            start,
        );
        self.cur = Some(id);
        self.root = Some(id);
        self.backreferences.clear();
        self.capturing_groups.clear();
    }

    fn on_pattern_leave(&mut self, _start: usize, end: usize) -> Res<()> {
        if let Some(root) = self.root {
            self.nodes[root].end = end;
        }
        for reference in self.backreferences.clone() {
            let K::Backreference { reference: r, .. } = &self.nodes[reference].k else {
                continue;
            };
            let groups: Vec<Nid> = match r {
                Ref::Num(n) => {
                    let i = *n as usize;
                    match i.checked_sub(1).and_then(|i| self.capturing_groups.get(i)) {
                        Some(g) => vec![*g],
                        None => {
                            self.unknown = true;
                            continue;
                        }
                    }
                }
                Ref::Name(name) => self
                    .capturing_groups
                    .iter()
                    .copied()
                    .filter(|g| matches!(&self.nodes[*g].k, K::CapturingGroup { name: Some(gn), .. } if gn == name))
                    .collect(),
            };
            if let K::Backreference {
                ambiguous,
                resolved,
                ..
            } = &mut self.nodes[reference].k
            {
                *ambiguous = groups.len() != 1;
                *resolved = groups.clone();
            }
            for g in groups {
                if let K::CapturingGroup { references, .. } = &mut self.nodes[g].k {
                    references.push(reference);
                }
            }
        }
        Ok(())
    }

    fn on_alternative_enter(&mut self, start: usize, _index: usize) {
        let Some(parent) = self.cur else {
            self.unknown = true;
            return;
        };
        if !matches!(
            self.kind(Some(parent)),
            "Assertion" | "CapturingGroup" | "Group" | "Pattern"
        ) {
            self.unknown = true;
            return;
        }
        let id = self.add(
            K::Alternative {
                elements: Vec::new(),
            },
            Some(parent),
            start,
            start,
        );
        self.cur = Some(id);
        if let Some(a) = self.alternatives(parent) {
            a.push(id);
        }
    }

    fn on_alternative_leave(&mut self, _start: usize, end: usize, _index: usize) {
        let ok = self.kind(self.cur) == "Alternative";
        self.leave(end, ok);
    }

    fn on_group_enter(&mut self, start: usize) {
        let Some(parent) = self.cur.filter(|p| self.kind(Some(*p)) == "Alternative") else {
            self.unknown = true;
            return;
        };
        let id = self.add(
            K::Group {
                modifiers: None,
                alternatives: Vec::new(),
            },
            Some(parent),
            start,
            start,
        );
        self.cur = Some(id);
        self.push_element(parent, id);
    }

    fn on_group_leave(&mut self, _start: usize, end: usize) {
        let ok = self.kind(self.cur) == "Group";
        self.leave(end, ok);
    }

    fn on_modifiers_enter(&mut self, start: usize) {
        let Some(parent) = self.cur.filter(|p| self.kind(Some(*p)) == "Group") else {
            self.unknown = true;
            return;
        };
        let id = self.add(
            K::Modifiers {
                add: None,
                remove: None,
            },
            Some(parent),
            start,
            start,
        );
        self.cur = Some(id);
        if let K::Group { modifiers, .. } = &mut self.nodes[parent].k {
            *modifiers = Some(id);
        }
    }

    fn on_modifiers_leave(&mut self, _start: usize, end: usize) {
        let ok = self.kind(self.cur) == "Modifiers";
        self.leave(end, ok);
    }

    fn on_add_modifiers(&mut self, start: usize, end: usize, flags: &Flags) {
        let Some(parent) = self.cur.filter(|p| self.kind(Some(*p)) == "Modifiers") else {
            self.unknown = true;
            return;
        };
        let id = self.add(modifier_flags(flags), Some(parent), start, end);
        if let K::Modifiers { add, .. } = &mut self.nodes[parent].k {
            *add = Some(id);
        }
    }

    fn on_remove_modifiers(&mut self, start: usize, end: usize, flags: &Flags) {
        let Some(parent) = self.cur.filter(|p| self.kind(Some(*p)) == "Modifiers") else {
            self.unknown = true;
            return;
        };
        let id = self.add(modifier_flags(flags), Some(parent), start, end);
        if let K::Modifiers { remove, .. } = &mut self.nodes[parent].k {
            *remove = Some(id);
        }
    }

    fn on_capturing_group_enter(&mut self, start: usize, name: Option<&str>) {
        let Some(parent) = self.cur.filter(|p| self.kind(Some(*p)) == "Alternative") else {
            self.unknown = true;
            return;
        };
        let k = K::CapturingGroup {
            name: name.map(str::to_string),
            alternatives: Vec::new(),
            references: Vec::new(),
        };
        let id = self.add(k, Some(parent), start, start);
        self.cur = Some(id);
        self.push_element(parent, id);
        self.capturing_groups.push(id);
    }

    fn on_capturing_group_leave(&mut self, _start: usize, end: usize, _name: Option<&str>) {
        let ok = self.kind(self.cur) == "CapturingGroup";
        self.leave(end, ok);
    }

    fn on_quantifier(&mut self, _start: usize, end: usize, min: f64, max: f64, greedy: bool) {
        let Some(parent) = self.cur.filter(|p| self.kind(Some(*p)) == "Alternative") else {
            self.unknown = true;
            return;
        };
        let Some(element) = self.pop_element(parent) else {
            self.unknown = true;
            return;
        };
        let bad = match &self.nodes[element].k {
            K::Quantifier { .. } => true,
            K::Assertion { kind, .. } => *kind != "lookahead",
            _ => false,
        };
        if bad {
            self.unknown = true;
            return;
        }
        let start = self.nodes[element].start;
        let id = self.add(
            K::Quantifier {
                min,
                max,
                greedy,
                element,
            },
            Some(parent),
            start,
            end,
        );
        self.push_element(parent, id);
        self.set_parent(element, id);
    }

    fn on_lookaround_assertion_enter(&mut self, start: usize, kind: &'static str, negate: bool) {
        let Some(parent) = self.cur.filter(|p| self.kind(Some(*p)) == "Alternative") else {
            self.unknown = true;
            return;
        };
        let id = self.add(
            K::Assertion {
                kind,
                negate,
                alternatives: Vec::new(),
            },
            Some(parent),
            start,
            start,
        );
        self.cur = Some(id);
        self.push_element(parent, id);
    }

    fn on_lookaround_assertion_leave(
        &mut self,
        _start: usize,
        end: usize,
        _kind: &'static str,
        _negate: bool,
    ) {
        let ok = self.kind(self.cur) == "Assertion";
        self.leave(end, ok);
    }

    fn on_edge_assertion(&mut self, start: usize, end: usize, kind: &'static str) {
        let Some(parent) = self.cur.filter(|p| self.kind(Some(*p)) == "Alternative") else {
            self.unknown = true;
            return;
        };
        let id = self.add(
            K::Assertion {
                kind,
                negate: false,
                alternatives: Vec::new(),
            },
            Some(parent),
            start,
            end,
        );
        self.push_element(parent, id);
    }

    fn on_word_boundary_assertion(
        &mut self,
        start: usize,
        end: usize,
        kind: &'static str,
        negate: bool,
    ) {
        let Some(parent) = self.cur.filter(|p| self.kind(Some(*p)) == "Alternative") else {
            self.unknown = true;
            return;
        };
        let id = self.add(
            K::Assertion {
                kind,
                negate,
                alternatives: Vec::new(),
            },
            Some(parent),
            start,
            end,
        );
        self.push_element(parent, id);
    }

    fn on_any_character_set(&mut self, start: usize, end: usize, kind: &'static str) {
        let Some(parent) = self.cur.filter(|p| self.kind(Some(*p)) == "Alternative") else {
            self.unknown = true;
            return;
        };
        let k = K::CharacterSet {
            kind,
            negate: false,
            key: None,
            value: None,
            strings: false,
        };
        let id = self.add(k, Some(parent), start, end);
        self.push_element(parent, id);
    }

    fn on_escape_character_set(
        &mut self,
        start: usize,
        end: usize,
        kind: &'static str,
        negate: bool,
    ) {
        let Some(parent) = self
            .cur
            .filter(|p| matches!(self.kind(Some(*p)), "Alternative" | "CharacterClass"))
        else {
            self.unknown = true;
            return;
        };
        let k = K::CharacterSet {
            kind,
            negate,
            key: None,
            value: None,
            strings: false,
        };
        let id = self.add(k, Some(parent), start, end);
        self.push_element(parent, id);
    }

    fn on_unicode_property_character_set(
        &mut self,
        start: usize,
        end: usize,
        kind: &'static str,
        key: &str,
        value: Option<&str>,
        negate: bool,
        strings: bool,
    ) {
        let Some(parent) = self
            .cur
            .filter(|p| matches!(self.kind(Some(*p)), "Alternative" | "CharacterClass"))
        else {
            self.unknown = true;
            return;
        };
        if strings {
            let plain_class = matches!(
                self.nodes[parent].k,
                K::CharacterClass {
                    unicode_sets: false,
                    ..
                }
            );
            if plain_class || negate || value.is_some() {
                self.unknown = true;
                return;
            }
        }
        let k = K::CharacterSet {
            kind,
            negate,
            key: Some(key.to_string()),
            value: value.map(str::to_string),
            strings,
        };
        let id = self.add(k, Some(parent), start, end);
        self.push_element(parent, id);
    }

    fn on_character(&mut self, start: usize, end: usize, value: f64) {
        let Some(parent) = self.cur.filter(|p| {
            matches!(
                self.kind(Some(*p)),
                "Alternative" | "CharacterClass" | "StringAlternative"
            )
        }) else {
            self.unknown = true;
            return;
        };
        let id = self.add(
            K::Character {
                value: value as u32,
            },
            Some(parent),
            start,
            end,
        );
        self.push_element(parent, id);
    }

    fn on_backreference(&mut self, start: usize, end: usize, r: Ref) {
        let Some(parent) = self.cur.filter(|p| self.kind(Some(*p)) == "Alternative") else {
            self.unknown = true;
            return;
        };
        let id = self.add(
            K::Backreference {
                reference: r,
                ambiguous: false,
                resolved: Vec::new(),
            },
            Some(parent),
            start,
            end,
        );
        self.push_element(parent, id);
        self.backreferences.push(id);
    }

    fn on_character_class_enter(&mut self, start: usize, negate: bool, unicode_sets: bool) {
        let Some(parent) = self.cur else {
            self.unknown = true;
            return;
        };
        let ok = match &self.nodes[parent].k {
            K::Alternative { .. } => true,
            K::CharacterClass {
                unicode_sets: true, ..
            } => unicode_sets,
            _ => false,
        };
        if !ok {
            self.unknown = true;
            return;
        }
        let id = self.add(
            K::CharacterClass {
                unicode_sets,
                negate,
                elements: Vec::new(),
            },
            Some(parent),
            start,
            start,
        );
        self.cur = Some(id);
        self.push_element(parent, id);
    }

    fn on_character_class_leave(&mut self, _start: usize, end: usize, _negate: bool) {
        let Some(node) = self.cur.filter(|n| self.kind(Some(*n)) == "CharacterClass") else {
            self.unknown = true;
            return;
        };
        let Some(parent) = self.nodes[node]
            .parent
            .filter(|p| matches!(self.kind(Some(*p)), "Alternative" | "CharacterClass"))
        else {
            self.unknown = true;
            return;
        };
        self.nodes[node].end = end;
        self.cur = Some(parent);
        let Some(i) = self.expression_buffer.iter().position(|(c, _)| *c == node) else {
            return;
        };
        let K::CharacterClass {
            negate, elements, ..
        } = &self.nodes[node].k
        else {
            return;
        };
        if !elements.is_empty() {
            self.unknown = true;
            return;
        }
        let negate = *negate;
        let (_, expression) = self.expression_buffer.remove(i);
        if self.elements(parent).and_then(|e| e.last().copied()) != Some(node) {
            self.unknown = true;
            return;
        }
        self.nodes[node].k = K::ExpressionCharacterClass { negate, expression };
        self.set_parent(expression, node);
    }

    fn on_character_class_range(&mut self, start: usize, end: usize, _min: f64, _max: f64) {
        let Some(parent) = self.cur else {
            self.unknown = true;
            return;
        };
        let K::CharacterClass { unicode_sets, .. } = self.nodes[parent].k else {
            self.unknown = true;
            return;
        };
        let max = self
            .pop_element(parent)
            .filter(|m| self.kind(Some(*m)) == "Character");
        let Some(max) = max else {
            self.unknown = true;
            return;
        };
        if !unicode_sets {
            let hyphen = self.pop_element(parent);
            let ok = matches!(
                hyphen.map(|h| &self.nodes[h].k),
                Some(K::Character { value: 0x2d })
            );
            if !ok {
                self.unknown = true;
                return;
            }
        }
        let min = self
            .pop_element(parent)
            .filter(|m| self.kind(Some(*m)) == "Character");
        let Some(min) = min else {
            self.unknown = true;
            return;
        };
        let id = self.add(
            K::CharacterClassRange { min, max },
            Some(parent),
            start,
            end,
        );
        self.set_parent(min, id);
        self.set_parent(max, id);
        self.push_element(parent, id);
    }

    fn on_class_intersection(&mut self, start: usize, end: usize) {
        self.class_op(start, end, true);
    }

    fn on_class_subtraction(&mut self, start: usize, end: usize) {
        self.class_op(start, end, false);
    }

    fn on_class_string_disjunction_enter(&mut self, start: usize) {
        let Some(parent) = self.cur.filter(|p| {
            matches!(
                self.nodes[*p].k,
                K::CharacterClass {
                    unicode_sets: true,
                    ..
                }
            )
        }) else {
            self.unknown = true;
            return;
        };
        let id = self.add(
            K::ClassStringDisjunction {
                alternatives: Vec::new(),
            },
            Some(parent),
            start,
            start,
        );
        self.cur = Some(id);
        self.push_element(parent, id);
    }

    fn on_class_string_disjunction_leave(&mut self, _start: usize, end: usize) {
        let ok = self.kind(self.cur) == "ClassStringDisjunction";
        self.leave(end, ok);
    }

    fn on_string_alternative_enter(&mut self, start: usize, _index: usize) {
        let Some(parent) = self
            .cur
            .filter(|p| self.kind(Some(*p)) == "ClassStringDisjunction")
        else {
            self.unknown = true;
            return;
        };
        let id = self.add(
            K::StringAlternative {
                elements: Vec::new(),
            },
            Some(parent),
            start,
            start,
        );
        self.cur = Some(id);
        if let Some(a) = self.alternatives(parent) {
            a.push(id);
        }
    }

    fn on_string_alternative_leave(&mut self, _start: usize, end: usize, _index: usize) {
        let ok = self.kind(self.cur) == "StringAlternative";
        self.leave(end, ok);
    }
}

pub fn parse_pattern(source: &[u16], unicode: bool, unicode_sets: bool) -> Res<RegExpAst> {
    let mut v = Validator::new(Options::default(), State::default());
    v.validate_pattern(source, 0, source.len(), Some((unicode, unicode_sets)))?;
    let st = v.h;
    let unknown = || SyntaxError {
        message: "UnknownError".into(),
        index: 0,
        lossy: false,
    };
    if st.unknown {
        return Err(unknown());
    }
    let Some(root) = st.root else {
        return Err(unknown());
    };
    Ok(RegExpAst {
        nodes: st.nodes,
        root,
        source: source.to_vec(),
    })
}
