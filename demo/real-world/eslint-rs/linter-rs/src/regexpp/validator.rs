//! regexpp index.js: `RegExpValidator`.

use super::data;

pub const LATEST_ECMA_VERSION: u32 = 2025;

const BACKSPACE: i64 = 0x08;
const CHARACTER_TABULATION: i64 = 0x09;
const LINE_FEED: i64 = 0x0a;
const LINE_TABULATION: i64 = 0x0b;
const FORM_FEED: i64 = 0x0c;
const CARRIAGE_RETURN: i64 = 0x0d;
const EXCLAMATION_MARK: i64 = 0x21;
const NUMBER_SIGN: i64 = 0x23;
const DOLLAR_SIGN: i64 = 0x24;
const PERCENT_SIGN: i64 = 0x25;
const AMPERSAND: i64 = 0x26;
const LEFT_PARENTHESIS: i64 = 0x28;
const RIGHT_PARENTHESIS: i64 = 0x29;
const ASTERISK: i64 = 0x2a;
const PLUS_SIGN: i64 = 0x2b;
const COMMA: i64 = 0x2c;
const HYPHEN_MINUS: i64 = 0x2d;
const FULL_STOP: i64 = 0x2e;
const SOLIDUS: i64 = 0x2f;
const DIGIT_ZERO: i64 = 0x30;
const DIGIT_ONE: i64 = 0x31;
const DIGIT_SEVEN: i64 = 0x37;
const DIGIT_NINE: i64 = 0x39;
const COLON: i64 = 0x3a;
const SEMICOLON: i64 = 0x3b;
const LESS_THAN_SIGN: i64 = 0x3c;
const EQUALS_SIGN: i64 = 0x3d;
const GREATER_THAN_SIGN: i64 = 0x3e;
const QUESTION_MARK: i64 = 0x3f;
const COMMERCIAL_AT: i64 = 0x40;
const LATIN_CAPITAL_LETTER_A: i64 = 0x41;
const LATIN_CAPITAL_LETTER_B: i64 = 0x42;
const LATIN_CAPITAL_LETTER_D: i64 = 0x44;
const LATIN_CAPITAL_LETTER_F: i64 = 0x46;
const LATIN_CAPITAL_LETTER_P: i64 = 0x50;
const LATIN_CAPITAL_LETTER_S: i64 = 0x53;
const LATIN_CAPITAL_LETTER_W: i64 = 0x57;
const LATIN_CAPITAL_LETTER_Z: i64 = 0x5a;
const LOW_LINE: i64 = 0x5f;
const LATIN_SMALL_LETTER_A: i64 = 0x61;
const LATIN_SMALL_LETTER_B: i64 = 0x62;
const LATIN_SMALL_LETTER_C: i64 = 0x63;
const LATIN_SMALL_LETTER_D: i64 = 0x64;
const LATIN_SMALL_LETTER_F: i64 = 0x66;
const LATIN_SMALL_LETTER_G: i64 = 0x67;
const LATIN_SMALL_LETTER_I: i64 = 0x69;
const LATIN_SMALL_LETTER_K: i64 = 0x6b;
const LATIN_SMALL_LETTER_M: i64 = 0x6d;
const LATIN_SMALL_LETTER_N: i64 = 0x6e;
const LATIN_SMALL_LETTER_P: i64 = 0x70;
const LATIN_SMALL_LETTER_Q: i64 = 0x71;
const LATIN_SMALL_LETTER_R: i64 = 0x72;
const LATIN_SMALL_LETTER_S: i64 = 0x73;
const LATIN_SMALL_LETTER_T: i64 = 0x74;
const LATIN_SMALL_LETTER_U: i64 = 0x75;
const LATIN_SMALL_LETTER_V: i64 = 0x76;
const LATIN_SMALL_LETTER_W: i64 = 0x77;
const LATIN_SMALL_LETTER_X: i64 = 0x78;
const LATIN_SMALL_LETTER_Y: i64 = 0x79;
const LATIN_SMALL_LETTER_Z: i64 = 0x7a;
const LEFT_SQUARE_BRACKET: i64 = 0x5b;
const REVERSE_SOLIDUS: i64 = 0x5c;
const RIGHT_SQUARE_BRACKET: i64 = 0x5d;
const CIRCUMFLEX_ACCENT: i64 = 0x5e;
const GRAVE_ACCENT: i64 = 0x60;
const LEFT_CURLY_BRACKET: i64 = 0x7b;
const VERTICAL_LINE: i64 = 0x7c;
const RIGHT_CURLY_BRACKET: i64 = 0x7d;
const TILDE: i64 = 0x7e;
const ZERO_WIDTH_NON_JOINER: i64 = 0x200c;
const ZERO_WIDTH_JOINER: i64 = 0x200d;
const MAX_CODE_POINT: f64 = 1_114_111.0;

fn is_in_range(cp: i64, ranges: &[u32]) -> bool {
    let (mut l, mut r) = (0usize, ranges.len() / 2);
    while l < r {
        let i = (l + r) / 2;
        let (min, max) = (ranges[2 * i] as i64, ranges[2 * i + 1] as i64);
        if cp < min {
            r = i;
        } else if cp > max {
            l = i + 1;
        } else {
            return true;
        }
    }
    false
}

pub fn is_id_start(cp: i64) -> bool {
    if cp < 0x41 {
        return false;
    }
    if cp < 0x5b {
        return true;
    }
    if cp < 0x61 {
        return false;
    }
    if cp < 0x7b {
        return true;
    }
    is_in_range(cp, data::LARGE_ID_START_RANGES)
}

pub fn is_id_continue(cp: i64) -> bool {
    if cp < 0x30 {
        return false;
    }
    if cp < 0x3a {
        return true;
    }
    if cp < 0x41 {
        return false;
    }
    if cp < 0x5b {
        return true;
    }
    if cp == 0x5f {
        return true;
    }
    if cp < 0x61 {
        return false;
    }
    if cp < 0x7b {
        return true;
    }
    is_in_range(cp, data::LARGE_ID_START_RANGES) || is_in_range(cp, data::LARGE_ID_CONTINUE_RANGES)
}

pub fn is_mark(cp: u32) -> bool {
    is_in_range(cp as i64, data::MARK_RANGES)
}

fn has(set: &[&str], v: &str) -> bool {
    set.contains(&v)
}

fn is_valid_unicode_property(version: u32, name: &str, value: &str) -> bool {
    if matches!(name, "General_Category" | "gc") {
        return version >= 2018 && has(data::GC_VALUES_2018, value);
    }
    if matches!(name, "Script" | "Script_Extensions" | "sc" | "scx") {
        return (version >= 2018 && has(data::SC_VALUES_2018, value))
            || (version >= 2019 && has(data::SC_VALUES_2019, value))
            || (version >= 2020 && has(data::SC_VALUES_2020, value))
            || (version >= 2021 && has(data::SC_VALUES_2021, value))
            || (version >= 2022 && has(data::SC_VALUES_2022, value))
            || (version >= 2023 && has(data::SC_VALUES_2023, value));
    }
    false
}

fn is_valid_lone_unicode_property(version: u32, value: &str) -> bool {
    (version >= 2018 && has(data::BIN_PROPERTIES_2018, value))
        || (version >= 2019 && has(data::BIN_PROPERTIES_2019, value))
        || (version >= 2021 && has(data::BIN_PROPERTIES_2021, value))
}

fn is_valid_lone_unicode_property_of_string(version: u32, value: &str) -> bool {
    version >= 2024 && has(data::BIN_PROPERTIES_OF_STRINGS_2024, value)
}

fn is_latin_letter(c: i64) -> bool {
    (LATIN_CAPITAL_LETTER_A..=LATIN_CAPITAL_LETTER_Z).contains(&c)
        || (LATIN_SMALL_LETTER_A..=LATIN_SMALL_LETTER_Z).contains(&c)
}

fn is_decimal_digit(c: i64) -> bool {
    (DIGIT_ZERO..=DIGIT_NINE).contains(&c)
}

fn is_octal_digit(c: i64) -> bool {
    (DIGIT_ZERO..=DIGIT_SEVEN).contains(&c)
}

fn is_hex_digit(c: i64) -> bool {
    (DIGIT_ZERO..=DIGIT_NINE).contains(&c)
        || (LATIN_CAPITAL_LETTER_A..=LATIN_CAPITAL_LETTER_F).contains(&c)
        || (LATIN_SMALL_LETTER_A..=LATIN_SMALL_LETTER_F).contains(&c)
}

fn digit_to_int(c: i64) -> i64 {
    if (LATIN_SMALL_LETTER_A..=LATIN_SMALL_LETTER_F).contains(&c) {
        return c - LATIN_SMALL_LETTER_A + 10;
    }
    if (LATIN_CAPITAL_LETTER_A..=LATIN_CAPITAL_LETTER_F).contains(&c) {
        return c - LATIN_CAPITAL_LETTER_A + 10;
    }
    c - DIGIT_ZERO
}

fn is_lead_surrogate(c: i64) -> bool {
    (0xd800..=0xdbff).contains(&c)
}

fn is_trail_surrogate(c: i64) -> bool {
    (0xdc00..=0xdfff).contains(&c)
}

fn combine_surrogate_pair(lead: i64, trail: i64) -> i64 {
    (lead - 0xd800) * 0x400 + (trail - 0xdc00) + 0x10000
}

fn is_syntax_character(cp: i64) -> bool {
    matches!(
        cp,
        CIRCUMFLEX_ACCENT
            | DOLLAR_SIGN
            | REVERSE_SOLIDUS
            | FULL_STOP
            | ASTERISK
            | PLUS_SIGN
            | QUESTION_MARK
            | LEFT_PARENTHESIS
            | RIGHT_PARENTHESIS
            | LEFT_SQUARE_BRACKET
            | RIGHT_SQUARE_BRACKET
            | LEFT_CURLY_BRACKET
            | RIGHT_CURLY_BRACKET
            | VERTICAL_LINE
    )
}

fn is_class_set_reserved_double_punctuator_character(cp: i64) -> bool {
    matches!(
        cp,
        AMPERSAND
            | EXCLAMATION_MARK
            | NUMBER_SIGN
            | DOLLAR_SIGN
            | PERCENT_SIGN
            | ASTERISK
            | PLUS_SIGN
            | COMMA
            | FULL_STOP
            | COLON
            | SEMICOLON
            | LESS_THAN_SIGN
            | EQUALS_SIGN
            | GREATER_THAN_SIGN
            | QUESTION_MARK
            | COMMERCIAL_AT
            | CIRCUMFLEX_ACCENT
            | GRAVE_ACCENT
            | TILDE
    )
}

fn is_class_set_syntax_character(cp: i64) -> bool {
    matches!(
        cp,
        LEFT_PARENTHESIS
            | RIGHT_PARENTHESIS
            | LEFT_SQUARE_BRACKET
            | RIGHT_SQUARE_BRACKET
            | LEFT_CURLY_BRACKET
            | RIGHT_CURLY_BRACKET
            | SOLIDUS
            | HYPHEN_MINUS
            | REVERSE_SOLIDUS
            | VERTICAL_LINE
    )
}

fn is_class_set_reserved_punctuator(cp: i64) -> bool {
    matches!(
        cp,
        AMPERSAND
            | HYPHEN_MINUS
            | EXCLAMATION_MARK
            | NUMBER_SIGN
            | PERCENT_SIGN
            | COMMA
            | COLON
            | SEMICOLON
            | LESS_THAN_SIGN
            | EQUALS_SIGN
            | GREATER_THAN_SIGN
            | COMMERCIAL_AT
            | GRAVE_ACCENT
            | TILDE
    )
}

fn is_identifier_start_char(cp: i64) -> bool {
    is_id_start(cp) || cp == DOLLAR_SIGN || cp == LOW_LINE
}

fn is_identifier_part_char(cp: i64) -> bool {
    is_id_continue(cp)
        || cp == DOLLAR_SIGN
        || cp == ZERO_WIDTH_NON_JOINER
        || cp == ZERO_WIDTH_JOINER
}

fn is_unicode_property_name_character(cp: i64) -> bool {
    is_latin_letter(cp) || cp == LOW_LINE
}

fn is_unicode_property_value_character(cp: i64) -> bool {
    is_unicode_property_name_character(cp) || is_decimal_digit(cp)
}

fn is_regular_expression_modifier(ch: i64) -> bool {
    ch == LATIN_SMALL_LETTER_I || ch == LATIN_SMALL_LETTER_M || ch == LATIN_SMALL_LETTER_S
}

fn from_code_point(cp: i64) -> Option<char> {
    u32::try_from(cp).ok().and_then(char::from_u32)
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Flags {
    pub global: bool,
    pub ignore_case: bool,
    pub multiline: bool,
    pub unicode: bool,
    pub sticky: bool,
    pub dot_all: bool,
    pub has_indices: bool,
    pub unicode_sets: bool,
}

impl Flags {
    fn get_mut(&mut self, flag: i64) -> Option<&mut bool> {
        Some(match flag {
            LATIN_SMALL_LETTER_G => &mut self.global,
            LATIN_SMALL_LETTER_I => &mut self.ignore_case,
            LATIN_SMALL_LETTER_M => &mut self.multiline,
            LATIN_SMALL_LETTER_U => &mut self.unicode,
            LATIN_SMALL_LETTER_Y => &mut self.sticky,
            LATIN_SMALL_LETTER_S => &mut self.dot_all,
            LATIN_SMALL_LETTER_D => &mut self.has_indices,
            LATIN_SMALL_LETTER_V => &mut self.unicode_sets,
            _ => return None,
        })
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum Ref {
    Num(f64),
    Name(String),
}

#[derive(Debug, Clone, PartialEq)]
pub struct SyntaxError {
    pub message: String,
    pub index: usize,
    pub lossy: bool,
}

pub type Res<T> = Result<T, SyntaxError>;

#[allow(unused_variables)]
pub trait Handler {
    fn on_regexp_flags(&mut self, start: usize, end: usize, flags: &Flags) {}
    fn on_pattern_enter(&mut self, start: usize) {}
    fn on_pattern_leave(&mut self, start: usize, end: usize) -> Res<()> {
        Ok(())
    }
    fn on_disjunction_enter(&mut self, start: usize) {}
    fn on_disjunction_leave(&mut self, start: usize, end: usize) {}
    fn on_alternative_enter(&mut self, start: usize, index: usize) {}
    fn on_alternative_leave(&mut self, start: usize, end: usize, index: usize) {}
    fn on_group_enter(&mut self, start: usize) {}
    fn on_group_leave(&mut self, start: usize, end: usize) {}
    fn on_modifiers_enter(&mut self, start: usize) {}
    fn on_modifiers_leave(&mut self, start: usize, end: usize) {}
    fn on_add_modifiers(&mut self, start: usize, end: usize, flags: &Flags) {}
    fn on_remove_modifiers(&mut self, start: usize, end: usize, flags: &Flags) {}
    fn on_capturing_group_enter(&mut self, start: usize, name: Option<&str>) {}
    fn on_capturing_group_leave(&mut self, start: usize, end: usize, name: Option<&str>) {}
    fn on_quantifier(&mut self, start: usize, end: usize, min: f64, max: f64, greedy: bool) {}
    fn on_lookaround_assertion_enter(&mut self, start: usize, kind: &'static str, negate: bool) {}
    fn on_lookaround_assertion_leave(
        &mut self,
        start: usize,
        end: usize,
        kind: &'static str,
        negate: bool,
    ) {
    }
    fn on_edge_assertion(&mut self, start: usize, end: usize, kind: &'static str) {}
    fn on_word_boundary_assertion(
        &mut self,
        start: usize,
        end: usize,
        kind: &'static str,
        negate: bool,
    ) {
    }
    fn on_any_character_set(&mut self, start: usize, end: usize, kind: &'static str) {}
    fn on_escape_character_set(
        &mut self,
        start: usize,
        end: usize,
        kind: &'static str,
        negate: bool,
    ) {
    }
    #[allow(clippy::too_many_arguments)]
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
    }
    fn on_character(&mut self, start: usize, end: usize, value: f64) {}
    fn on_backreference(&mut self, start: usize, end: usize, r: Ref) {}
    fn on_character_class_enter(&mut self, start: usize, negate: bool, unicode_sets: bool) {}
    fn on_character_class_leave(&mut self, start: usize, end: usize, negate: bool) {}
    fn on_character_class_range(&mut self, start: usize, end: usize, min: f64, max: f64) {}
    fn on_class_intersection(&mut self, start: usize, end: usize) {}
    fn on_class_subtraction(&mut self, start: usize, end: usize) {}
    fn on_class_string_disjunction_enter(&mut self, start: usize) {}
    fn on_class_string_disjunction_leave(&mut self, start: usize, end: usize) {}
    fn on_string_alternative_enter(&mut self, start: usize, index: usize) {}
    fn on_string_alternative_leave(&mut self, start: usize, end: usize, index: usize) {}
}

impl Handler for () {}

struct Reader {
    unicode: bool,
    s: Vec<u16>,
    i: usize,
    end: usize,
    cp1: i64,
    w1: usize,
    cp2: i64,
    w2: usize,
    cp3: i64,
    w3: usize,
    cp4: i64,
}

impl Reader {
    fn new() -> Reader {
        Reader {
            unicode: false,
            s: Vec::new(),
            i: 0,
            end: 0,
            cp1: -1,
            w1: 1,
            cp2: -1,
            w2: 1,
            cp3: -1,
            w3: 1,
            cp4: -1,
        }
    }

    fn at(&self, i: usize) -> i64 {
        if i >= self.end {
            return -1;
        }
        let Some(&c) = self.s.get(i) else { return -1 };
        let c = c as i64;
        if self.unicode
            && is_lead_surrogate(c)
            && let Some(&t) = self.s.get(i + 1)
        {
            let t = t as i64;
            if is_trail_surrogate(t) {
                return combine_surrogate_pair(c, t);
            }
        }
        c
    }

    fn width(&self, c: i64) -> usize {
        if self.unicode && c > 0xffff { 2 } else { 1 }
    }

    fn reset(&mut self, source: &[u16], start: usize, end: usize, u_flag: bool) {
        self.unicode = u_flag;
        self.s = source.to_vec();
        self.end = end;
        self.rewind(start);
    }

    fn rewind(&mut self, index: usize) {
        self.i = index;
        self.cp1 = self.at(index);
        self.w1 = self.width(self.cp1);
        self.cp2 = self.at(index + self.w1);
        self.w2 = self.width(self.cp2);
        self.cp3 = self.at(index + self.w1 + self.w2);
        self.w3 = self.width(self.cp3);
        self.cp4 = self.at(index + self.w1 + self.w2 + self.w3);
    }

    fn advance(&mut self) {
        if self.cp1 != -1 {
            self.i += self.w1;
            self.cp1 = self.cp2;
            self.w1 = self.w2;
            self.cp2 = self.cp3;
            self.w2 = self.width(self.cp2);
            self.cp3 = self.cp4;
            self.w3 = self.width(self.cp3);
            self.cp4 = self.at(self.i + self.w1 + self.w2 + self.w3);
        }
    }

    fn eat(&mut self, cp: i64) -> bool {
        if self.cp1 == cp {
            self.advance();
            return true;
        }
        false
    }

    fn eat2(&mut self, cp1: i64, cp2: i64) -> bool {
        if self.cp1 == cp1 && self.cp2 == cp2 {
            self.advance();
            self.advance();
            return true;
        }
        false
    }

    fn eat3(&mut self, cp1: i64, cp2: i64, cp3: i64) -> bool {
        if self.cp1 == cp1 && self.cp2 == cp2 && self.cp3 == cp3 {
            self.advance();
            self.advance();
            self.advance();
            return true;
        }
        false
    }
}

struct Branches {
    parent: Vec<Option<usize>>,
    base: Vec<usize>,
}

impl Branches {
    fn new_id(&mut self, parent: Option<usize>, base: Option<usize>) -> usize {
        let id = self.parent.len();
        self.parent.push(parent);
        self.base.push(base.unwrap_or(id));
        id
    }

    fn separated_from(&self, this: usize, other: usize) -> bool {
        if self.base[this] == self.base[other] && this != other {
            return true;
        }
        if let Some(p) = self.parent[other]
            && self.separated_from(this, p)
        {
            return true;
        }
        match self.parent[this] {
            Some(p) => self.separated_from(p, other),
            None => false,
        }
    }
}

enum GroupSpecifiers {
    Es2018(Vec<String>),
    Es2025 {
        branches: Branches,
        branch_id: Option<usize>,
        group_names: Vec<(String, Vec<Option<usize>>)>,
    },
}

impl GroupSpecifiers {
    fn new(ecma_version: u32) -> GroupSpecifiers {
        if ecma_version >= 2025 {
            let mut branches = Branches {
                parent: Vec::new(),
                base: Vec::new(),
            };
            let id = branches.new_id(None, None);
            GroupSpecifiers::Es2025 {
                branches,
                branch_id: Some(id),
                group_names: Vec::new(),
            }
        } else {
            GroupSpecifiers::Es2018(Vec::new())
        }
    }

    fn clear(&mut self) {
        match self {
            GroupSpecifiers::Es2018(v) => v.clear(),
            GroupSpecifiers::Es2025 {
                branches,
                branch_id,
                group_names,
            } => {
                branches.parent.clear();
                branches.base.clear();
                *branch_id = Some(branches.new_id(None, None));
                group_names.clear();
            }
        }
    }

    fn is_empty(&self) -> bool {
        match self {
            GroupSpecifiers::Es2018(v) => v.is_empty(),
            GroupSpecifiers::Es2025 { group_names, .. } => group_names.is_empty(),
        }
    }

    fn enter_disjunction(&mut self) {
        if let GroupSpecifiers::Es2025 {
            branches,
            branch_id,
            ..
        } = self
        {
            *branch_id = Some(branches.new_id(*branch_id, None));
        }
    }

    fn enter_alternative(&mut self, index: usize) {
        if index == 0 {
            return;
        }
        if let GroupSpecifiers::Es2025 {
            branches,
            branch_id,
            ..
        } = self
            && let Some(b) = *branch_id
        {
            let (p, base) = (branches.parent[b], branches.base[b]);
            *branch_id = Some(branches.new_id(p, Some(base)));
        }
    }

    fn leave_disjunction(&mut self) {
        if let GroupSpecifiers::Es2025 {
            branches,
            branch_id,
            ..
        } = self
        {
            *branch_id = branch_id.and_then(|b| branches.parent[b]);
        }
    }

    fn has_in_pattern(&self, name: &str) -> bool {
        match self {
            GroupSpecifiers::Es2018(v) => v.iter().any(|n| n == name),
            GroupSpecifiers::Es2025 { group_names, .. } => {
                group_names.iter().any(|(n, _)| n == name)
            }
        }
    }

    fn has_in_scope(&self, name: &str) -> bool {
        match self {
            GroupSpecifiers::Es2018(_) => self.has_in_pattern(name),
            GroupSpecifiers::Es2025 {
                branches,
                branch_id,
                group_names,
            } => {
                let Some((_, list)) = group_names.iter().find(|(n, _)| n == name) else {
                    return false;
                };
                let Some(cur) = *branch_id else { return false };
                list.iter().any(|b| match b {
                    Some(b) => !branches.separated_from(*b, cur),
                    None => false,
                })
            }
        }
    }

    fn add_to_scope(&mut self, name: &str) {
        match self {
            GroupSpecifiers::Es2018(v) => {
                if !v.iter().any(|n| n == name) {
                    v.push(name.to_string());
                }
            }
            GroupSpecifiers::Es2025 {
                branch_id,
                group_names,
                ..
            } => {
                if let Some((_, list)) = group_names.iter_mut().find(|(n, _)| n == name) {
                    list.push(*branch_id);
                    return;
                }
                group_names.push((name.to_string(), vec![*branch_id]));
            }
        }
    }
}

#[derive(Clone, Copy, PartialEq)]
enum SrcKind {
    Flags,
    Pattern,
}

struct SrcCtx {
    source: Vec<u16>,
    start: usize,
    end: usize,
    kind: SrcKind,
}

#[derive(Default)]
pub struct Options {
    pub strict: bool,
    pub ecma_version: Option<u32>,
}

type Strings = Option<bool>;

pub struct Validator<H: Handler> {
    pub h: H,
    reader: Reader,
    unicode_mode: bool,
    unicode_sets_mode: bool,
    n_flag: bool,
    last_int_value: f64,
    last_range: (f64, f64),
    last_str_value: String,
    last_assertion_is_quantifiable: bool,
    num_capturing_parens: f64,
    backreference_names: Vec<String>,
    src_ctx: Option<SrcCtx>,
    strict_opt: bool,
    ecma_version: u32,
    group_specifiers: GroupSpecifiers,
}

impl<H: Handler> Validator<H> {
    pub fn new(options: Options, h: H) -> Validator<H> {
        let ecma_version = options.ecma_version.unwrap_or(LATEST_ECMA_VERSION);
        Validator {
            h,
            reader: Reader::new(),
            unicode_mode: false,
            unicode_sets_mode: false,
            n_flag: false,
            last_int_value: 0.0,
            last_range: (0.0, f64::INFINITY),
            last_str_value: String::new(),
            last_assertion_is_quantifiable: false,
            num_capturing_parens: 0.0,
            backreference_names: Vec::new(),
            src_ctx: None,
            strict_opt: options.strict,
            ecma_version,
            group_specifiers: GroupSpecifiers::new(ecma_version),
        }
    }

    pub fn validate_flags(&mut self, source: &[u16], start: usize, end: usize) -> Res<()> {
        self.src_ctx = Some(SrcCtx {
            source: Vec::new(),
            start,
            end,
            kind: SrcKind::Flags,
        });
        self.validate_flags_internal(source, start, end)
    }

    pub fn validate_pattern(
        &mut self,
        source: &[u16],
        start: usize,
        end: usize,
        flags: Option<(bool, bool)>,
    ) -> Res<()> {
        self.src_ctx = Some(SrcCtx {
            source: source.to_vec(),
            start,
            end,
            kind: SrcKind::Pattern,
        });
        self.validate_pattern_internal(source, start, end, flags)
    }

    fn validate_pattern_internal(
        &mut self,
        source: &[u16],
        start: usize,
        end: usize,
        flags: Option<(bool, bool)>,
    ) -> Res<()> {
        let (unicode_mode, n_flag, unicode_sets_mode) =
            self.parse_flags_option_to_mode(flags, end)?;
        self.unicode_mode = unicode_mode;
        self.n_flag = n_flag;
        self.unicode_sets_mode = unicode_sets_mode;
        self.reader.reset(source, start, end, self.unicode_mode);
        self.consume_pattern()?;
        if !self.n_flag && self.ecma_version >= 2018 && !self.group_specifiers.is_empty() {
            self.n_flag = true;
            self.reader.rewind(start);
            self.consume_pattern()?;
        }
        Ok(())
    }

    fn validate_flags_internal(&mut self, source: &[u16], start: usize, end: usize) -> Res<()> {
        let flags = self.parse_flags(source, start, end)?;
        self.h.on_regexp_flags(start, end, &flags);
        Ok(())
    }

    fn parse_flags_option_to_mode(
        &mut self,
        flags: Option<(bool, bool)>,
        source_end: usize,
    ) -> Res<(bool, bool, bool)> {
        let mut unicode = false;
        let mut unicode_sets = false;
        if let Some((u, v)) = flags
            && self.ecma_version >= 2015
        {
            unicode = u;
            if self.ecma_version >= 2024 {
                unicode_sets = v;
            }
        }
        if unicode && unicode_sets {
            return Err(self.error(
                "Invalid regular expression flags",
                source_end + 1,
                Some((unicode, unicode_sets)),
            ));
        }
        let unicode_mode = unicode || unicode_sets;
        let n_flag = (unicode && self.ecma_version >= 2018)
            || unicode_sets
            || (self.strict_opt && self.ecma_version >= 2023);
        Ok((unicode_mode, n_flag, unicode_sets))
    }

    fn strict(&self) -> bool {
        self.strict_opt || self.unicode_mode
    }

    fn index(&self) -> usize {
        self.reader.i
    }

    fn cp(&self) -> i64 {
        self.reader.cp1
    }

    fn next_cp(&self) -> i64 {
        self.reader.cp2
    }

    fn next_cp2(&self) -> i64 {
        self.reader.cp3
    }

    fn next_cp3(&self) -> i64 {
        self.reader.cp4
    }

    fn advance(&mut self) {
        self.reader.advance();
    }

    fn rewind(&mut self, i: usize) {
        self.reader.rewind(i);
    }

    fn eat(&mut self, cp: i64) -> bool {
        self.reader.eat(cp)
    }

    fn eat2(&mut self, a: i64, b: i64) -> bool {
        self.reader.eat2(a, b)
    }

    fn eat3(&mut self, a: i64, b: i64, c: i64) -> bool {
        self.reader.eat3(a, b, c)
    }

    fn error(&self, message: &str, index: usize, flags: Option<(bool, bool)>) -> SyntaxError {
        let (unicode, unicode_sets) = flags.unwrap_or((
            self.unicode_mode && !self.unicode_sets_mode,
            self.unicode_sets_mode,
        ));
        let mut lossy = false;
        let mut source = String::new();
        if let Some(ctx) = &self.src_ctx
            && ctx.kind == SrcKind::Pattern
        {
            let s = &ctx.source;
            let (a, b) = (ctx.start.min(s.len()), ctx.end.min(s.len()));
            let pat = if a < b { &s[a..b] } else { &s[0..0] };
            lossy |= String::from_utf16(pat).is_err();
            let flags_text = format!(
                "{}{}",
                if unicode { "u" } else { "" },
                if unicode_sets { "v" } else { "" }
            );
            source = format!(": /{}/{}", String::from_utf16_lossy(pat), flags_text);
        }
        SyntaxError {
            message: format!("Invalid regular expression{source}: {message}"),
            index,
            lossy,
        }
    }

    fn raise<T>(&self, message: &str) -> Res<T> {
        Err(self.error(message, self.index(), None))
    }

    fn raise_char<T>(&self, prefix: &str, cp: i64, suffix: &str) -> Res<T> {
        let mut e = match from_code_point(cp) {
            Some(c) => self.error(&format!("{prefix}{c}{suffix}"), self.index(), None),
            None => self.error(&format!("{prefix}\u{fffd}{suffix}"), self.index(), None),
        };
        e.lossy |= from_code_point(cp).is_none();
        Err(e)
    }

    fn consume_pattern(&mut self) -> Res<()> {
        let start = self.index();
        self.num_capturing_parens = self.count_capturing_parens();
        self.group_specifiers.clear();
        self.backreference_names.clear();
        self.h.on_pattern_enter(start);
        self.consume_disjunction()?;
        let cp = self.cp();
        if cp != -1 {
            if cp == RIGHT_PARENTHESIS {
                return self.raise("Unmatched ')'");
            }
            if cp == REVERSE_SOLIDUS {
                return self.raise("\\ at end of pattern");
            }
            if cp == RIGHT_SQUARE_BRACKET || cp == RIGHT_CURLY_BRACKET {
                return self.raise("Lone quantifier brackets");
            }
            return self.raise_char("Unexpected character '", cp, "'");
        }
        for name in &self.backreference_names {
            if !self.group_specifiers.has_in_pattern(name) {
                return self.raise("Invalid named capture referenced");
            }
        }
        let end = self.index();
        self.h.on_pattern_leave(start, end)
    }

    fn count_capturing_parens(&mut self) -> f64 {
        let start = self.index();
        let mut in_class = false;
        let mut escaped = false;
        let mut count = 0.0;
        loop {
            let cp = self.cp();
            if cp == -1 {
                break;
            }
            if escaped {
                escaped = false;
            } else if cp == REVERSE_SOLIDUS {
                escaped = true;
            } else if cp == LEFT_SQUARE_BRACKET {
                in_class = true;
            } else if cp == RIGHT_SQUARE_BRACKET {
                in_class = false;
            } else if cp == LEFT_PARENTHESIS
                && !in_class
                && (self.next_cp() != QUESTION_MARK
                    || (self.next_cp2() == LESS_THAN_SIGN
                        && self.next_cp3() != EQUALS_SIGN
                        && self.next_cp3() != EXCLAMATION_MARK))
            {
                count += 1.0;
            }
            self.advance();
        }
        self.rewind(start);
        count
    }

    fn consume_disjunction(&mut self) -> Res<()> {
        let start = self.index();
        let mut i = 0;
        self.group_specifiers.enter_disjunction();
        self.h.on_disjunction_enter(start);
        loop {
            self.consume_alternative(i)?;
            i += 1;
            if !self.eat(VERTICAL_LINE) {
                break;
            }
        }
        if self.consume_quantifier(true)? {
            return self.raise("Nothing to repeat");
        }
        if self.eat(LEFT_CURLY_BRACKET) {
            return self.raise("Lone quantifier brackets");
        }
        let end = self.index();
        self.h.on_disjunction_leave(start, end);
        self.group_specifiers.leave_disjunction();
        Ok(())
    }

    fn consume_alternative(&mut self, i: usize) -> Res<()> {
        let start = self.index();
        self.group_specifiers.enter_alternative(i);
        self.h.on_alternative_enter(start, i);
        while self.cp() != -1 && self.consume_term()? {}
        let end = self.index();
        self.h.on_alternative_leave(start, end, i);
        Ok(())
    }

    fn consume_term(&mut self) -> Res<bool> {
        if self.unicode_mode || self.strict() {
            return Ok(self.consume_assertion()?
                || (self.consume_atom()? && self.consume_optional_quantifier()?));
        }
        Ok((self.consume_assertion()?
            && (!self.last_assertion_is_quantifiable || self.consume_optional_quantifier()?))
            || (self.consume_extended_atom()? && self.consume_optional_quantifier()?))
    }

    fn consume_optional_quantifier(&mut self) -> Res<bool> {
        self.consume_quantifier(false)?;
        Ok(true)
    }

    fn consume_assertion(&mut self) -> Res<bool> {
        let start = self.index();
        self.last_assertion_is_quantifiable = false;
        if self.eat(CIRCUMFLEX_ACCENT) {
            let end = self.index();
            self.h.on_edge_assertion(start, end, "start");
            return Ok(true);
        }
        if self.eat(DOLLAR_SIGN) {
            let end = self.index();
            self.h.on_edge_assertion(start, end, "end");
            return Ok(true);
        }
        if self.eat2(REVERSE_SOLIDUS, LATIN_CAPITAL_LETTER_B) {
            let end = self.index();
            self.h.on_word_boundary_assertion(start, end, "word", true);
            return Ok(true);
        }
        if self.eat2(REVERSE_SOLIDUS, LATIN_SMALL_LETTER_B) {
            let end = self.index();
            self.h.on_word_boundary_assertion(start, end, "word", false);
            return Ok(true);
        }
        if self.eat2(LEFT_PARENTHESIS, QUESTION_MARK) {
            let lookbehind = self.ecma_version >= 2018 && self.eat(LESS_THAN_SIGN);
            let mut negate = false;
            if self.eat(EQUALS_SIGN) || {
                negate = self.eat(EXCLAMATION_MARK);
                negate
            } {
                let kind = if lookbehind {
                    "lookbehind"
                } else {
                    "lookahead"
                };
                self.h.on_lookaround_assertion_enter(start, kind, negate);
                self.consume_disjunction()?;
                if !self.eat(RIGHT_PARENTHESIS) {
                    return self.raise("Unterminated group");
                }
                self.last_assertion_is_quantifiable = !lookbehind && !self.strict();
                let end = self.index();
                self.h
                    .on_lookaround_assertion_leave(start, end, kind, negate);
                return Ok(true);
            }
            self.rewind(start);
        }
        Ok(false)
    }

    fn consume_quantifier(&mut self, no_consume: bool) -> Res<bool> {
        let start = self.index();
        let (min, max);
        if self.eat(ASTERISK) {
            min = 0.0;
            max = f64::INFINITY;
        } else if self.eat(PLUS_SIGN) {
            min = 1.0;
            max = f64::INFINITY;
        } else if self.eat(QUESTION_MARK) {
            min = 0.0;
            max = 1.0;
        } else if self.eat_braced_quantifier(no_consume)? {
            (min, max) = self.last_range;
        } else {
            return Ok(false);
        }
        let greedy = !self.eat(QUESTION_MARK);
        if !no_consume {
            let end = self.index();
            self.h.on_quantifier(start, end, min, max, greedy);
        }
        Ok(true)
    }

    fn eat_braced_quantifier(&mut self, no_error: bool) -> Res<bool> {
        let start = self.index();
        if self.eat(LEFT_CURLY_BRACKET) {
            if self.eat_decimal_digits() {
                let min = self.last_int_value;
                let mut max = min;
                if self.eat(COMMA) {
                    max = if self.eat_decimal_digits() {
                        self.last_int_value
                    } else {
                        f64::INFINITY
                    };
                }
                if self.eat(RIGHT_CURLY_BRACKET) {
                    if !no_error && max < min {
                        return self.raise("numbers out of order in {} quantifier");
                    }
                    self.last_range = (min, max);
                    return Ok(true);
                }
            }
            if !no_error && (self.unicode_mode || self.strict()) {
                return self.raise("Incomplete quantifier");
            }
            self.rewind(start);
        }
        Ok(false)
    }

    fn consume_atom(&mut self) -> Res<bool> {
        Ok(self.consume_pattern_character()
            || self.consume_dot()
            || self.consume_reverse_solidus_atom_escape()?
            || self.consume_character_class()?.is_some()
            || self.consume_capturing_group()?
            || self.consume_uncapturing_group()?)
    }

    fn consume_dot(&mut self) -> bool {
        if self.eat(FULL_STOP) {
            let i = self.index();
            self.h.on_any_character_set(i - 1, i, "any");
            return true;
        }
        false
    }

    fn consume_reverse_solidus_atom_escape(&mut self) -> Res<bool> {
        let start = self.index();
        if self.eat(REVERSE_SOLIDUS) {
            if self.consume_atom_escape()? {
                return Ok(true);
            }
            self.rewind(start);
        }
        Ok(false)
    }

    fn consume_uncapturing_group(&mut self) -> Res<bool> {
        let start = self.index();
        if self.eat2(LEFT_PARENTHESIS, QUESTION_MARK) {
            self.h.on_group_enter(start);
            if self.ecma_version >= 2025 {
                self.consume_modifiers()?;
            }
            if !self.eat(COLON) {
                self.rewind(start + 1);
                return self.raise("Invalid group");
            }
            self.consume_disjunction()?;
            if !self.eat(RIGHT_PARENTHESIS) {
                return self.raise("Unterminated group");
            }
            let end = self.index();
            self.h.on_group_leave(start, end);
            return Ok(true);
        }
        Ok(false)
    }

    fn consume_modifiers(&mut self) -> Res<bool> {
        let start = self.index();
        let has_add_modifiers = self.eat_modifiers();
        let add_modifiers_end = self.index();
        let has_hyphen = self.eat(HYPHEN_MINUS);
        if !has_add_modifiers && !has_hyphen {
            return Ok(false);
        }
        self.h.on_modifiers_enter(start);
        let add_modifiers = self.parse_modifiers(start, add_modifiers_end)?;
        self.h
            .on_add_modifiers(start, add_modifiers_end, &add_modifiers);
        if has_hyphen {
            let modifiers_start = self.index();
            if !self.eat_modifiers() && !has_add_modifiers && self.cp() == COLON {
                return self.raise("Invalid empty flags");
            }
            let end = self.index();
            let modifiers = self.parse_modifiers(modifiers_start, end)?;
            for (on, add, c) in [
                (modifiers.ignore_case, add_modifiers.ignore_case, 'i'),
                (modifiers.multiline, add_modifiers.multiline, 'm'),
                (modifiers.dot_all, add_modifiers.dot_all, 's'),
            ] {
                if on && add {
                    return self.raise(&format!("Duplicated flag '{c}'"));
                }
            }
            self.h.on_remove_modifiers(modifiers_start, end, &modifiers);
        }
        let end = self.index();
        self.h.on_modifiers_leave(start, end);
        Ok(true)
    }

    fn consume_capturing_group(&mut self) -> Res<bool> {
        let start = self.index();
        if self.eat(LEFT_PARENTHESIS) {
            let mut name: Option<String> = None;
            if self.ecma_version >= 2018 {
                if self.consume_group_specifier()? {
                    name = Some(self.last_str_value.clone());
                } else if self.cp() == QUESTION_MARK {
                    self.rewind(start);
                    return Ok(false);
                }
            } else if self.cp() == QUESTION_MARK {
                self.rewind(start);
                return Ok(false);
            }
            self.h.on_capturing_group_enter(start, name.as_deref());
            self.consume_disjunction()?;
            if !self.eat(RIGHT_PARENTHESIS) {
                return self.raise("Unterminated group");
            }
            let end = self.index();
            self.h.on_capturing_group_leave(start, end, name.as_deref());
            return Ok(true);
        }
        Ok(false)
    }

    fn consume_extended_atom(&mut self) -> Res<bool> {
        Ok(self.consume_dot()
            || self.consume_reverse_solidus_atom_escape()?
            || self.consume_reverse_solidus_followed_by_c()
            || self.consume_character_class()?.is_some()
            || self.consume_capturing_group()?
            || self.consume_uncapturing_group()?
            || self.consume_invalid_braced_quantifier()?
            || self.consume_extended_pattern_character())
    }

    fn consume_reverse_solidus_followed_by_c(&mut self) -> bool {
        let start = self.index();
        if self.cp() == REVERSE_SOLIDUS && self.next_cp() == LATIN_SMALL_LETTER_C {
            self.last_int_value = self.cp() as f64;
            self.advance();
            let end = self.index();
            self.h.on_character(start, end, REVERSE_SOLIDUS as f64);
            return true;
        }
        false
    }

    fn consume_invalid_braced_quantifier(&mut self) -> Res<bool> {
        if self.eat_braced_quantifier(true)? {
            return self.raise("Nothing to repeat");
        }
        Ok(false)
    }

    fn consume_pattern_character(&mut self) -> bool {
        let start = self.index();
        let cp = self.cp();
        if cp != -1 && !is_syntax_character(cp) {
            self.advance();
            let end = self.index();
            self.h.on_character(start, end, cp as f64);
            return true;
        }
        false
    }

    fn consume_extended_pattern_character(&mut self) -> bool {
        let start = self.index();
        let cp = self.cp();
        if cp != -1
            && cp != CIRCUMFLEX_ACCENT
            && cp != DOLLAR_SIGN
            && cp != REVERSE_SOLIDUS
            && cp != FULL_STOP
            && cp != ASTERISK
            && cp != PLUS_SIGN
            && cp != QUESTION_MARK
            && cp != LEFT_PARENTHESIS
            && cp != RIGHT_PARENTHESIS
            && cp != LEFT_SQUARE_BRACKET
            && cp != VERTICAL_LINE
        {
            self.advance();
            let end = self.index();
            self.h.on_character(start, end, cp as f64);
            return true;
        }
        false
    }

    fn consume_group_specifier(&mut self) -> Res<bool> {
        let start = self.index();
        if self.eat(QUESTION_MARK) {
            if self.eat_group_name()? {
                if !self.group_specifiers.has_in_scope(&self.last_str_value) {
                    let name = self.last_str_value.clone();
                    self.group_specifiers.add_to_scope(&name);
                    return Ok(true);
                }
                return self.raise("Duplicate capture group name");
            }
            self.rewind(start);
        }
        Ok(false)
    }

    fn consume_atom_escape(&mut self) -> Res<bool> {
        if self.consume_backreference()?
            || self.consume_character_class_escape()?.is_some()
            || self.consume_character_escape()?
            || (self.n_flag && self.consume_k_group_name()?)
        {
            return Ok(true);
        }
        if self.strict() || self.unicode_mode {
            return self.raise("Invalid escape");
        }
        Ok(false)
    }

    fn consume_backreference(&mut self) -> Res<bool> {
        let start = self.index();
        if self.eat_decimal_escape() {
            let n = self.last_int_value;
            if n <= self.num_capturing_parens {
                let end = self.index();
                self.h.on_backreference(start - 1, end, Ref::Num(n));
                return Ok(true);
            }
            if self.strict() || self.unicode_mode {
                return self.raise("Invalid escape");
            }
            self.rewind(start);
        }
        Ok(false)
    }

    fn consume_character_class_escape(&mut self) -> Res<Strings> {
        let start = self.index();
        for (c, kind, negate) in [
            (LATIN_SMALL_LETTER_D, "digit", false),
            (LATIN_CAPITAL_LETTER_D, "digit", true),
            (LATIN_SMALL_LETTER_S, "space", false),
            (LATIN_CAPITAL_LETTER_S, "space", true),
            (LATIN_SMALL_LETTER_W, "word", false),
            (LATIN_CAPITAL_LETTER_W, "word", true),
        ] {
            if self.eat(c) {
                self.last_int_value = -1.0;
                let end = self.index();
                self.h.on_escape_character_set(start - 1, end, kind, negate);
                return Ok(Some(false));
            }
        }
        let mut negate = false;
        if self.unicode_mode
            && self.ecma_version >= 2018
            && (self.eat(LATIN_SMALL_LETTER_P) || {
                negate = self.eat(LATIN_CAPITAL_LETTER_P);
                negate
            })
        {
            self.last_int_value = -1.0;
            if self.eat(LEFT_CURLY_BRACKET)
                && let Some((key, value, strings)) = self.eat_unicode_property_value_expression()?
                && self.eat(RIGHT_CURLY_BRACKET)
            {
                if negate && strings {
                    return self.raise("Invalid property name");
                }
                let end = self.index();
                self.h.on_unicode_property_character_set(
                    start - 1,
                    end,
                    "property",
                    &key,
                    value.as_deref(),
                    negate,
                    strings,
                );
                return Ok(Some(strings));
            }
            return self.raise("Invalid property name");
        }
        Ok(None)
    }

    fn consume_character_escape(&mut self) -> Res<bool> {
        let start = self.index();
        if self.eat_control_escape()
            || self.eat_c_control_letter()
            || self.eat_zero()
            || self.eat_hex_escape_sequence()?
            || self.eat_regexp_unicode_escape_sequence(false)?
            || (!self.strict() && !self.unicode_mode && self.eat_legacy_octal_escape_sequence())
            || self.eat_identity_escape()
        {
            let end = self.index();
            self.h.on_character(start - 1, end, self.last_int_value);
            return Ok(true);
        }
        Ok(false)
    }

    fn consume_k_group_name(&mut self) -> Res<bool> {
        let start = self.index();
        if self.eat(LATIN_SMALL_LETTER_K) {
            if self.eat_group_name()? {
                let group_name = self.last_str_value.clone();
                if !self.backreference_names.contains(&group_name) {
                    self.backreference_names.push(group_name.clone());
                }
                let end = self.index();
                self.h
                    .on_backreference(start - 1, end, Ref::Name(group_name));
                return Ok(true);
            }
            return self.raise("Invalid named reference");
        }
        Ok(false)
    }

    fn consume_character_class(&mut self) -> Res<Strings> {
        let start = self.index();
        if self.eat(LEFT_SQUARE_BRACKET) {
            let negate = self.eat(CIRCUMFLEX_ACCENT);
            self.h
                .on_character_class_enter(start, negate, self.unicode_sets_mode);
            let result = self.consume_class_contents()?;
            if !self.eat(RIGHT_SQUARE_BRACKET) {
                if self.cp() == -1 {
                    return self.raise("Unterminated character class");
                }
                return self.raise("Invalid character in character class");
            }
            if negate && result {
                return self.raise("Negated character class may contain strings");
            }
            let end = self.index();
            self.h.on_character_class_leave(start, end, negate);
            return Ok(Some(result));
        }
        Ok(None)
    }

    fn consume_class_contents(&mut self) -> Res<bool> {
        if self.unicode_sets_mode {
            if self.cp() == RIGHT_SQUARE_BRACKET {
                return Ok(false);
            }
            return self.consume_class_set_expression();
        }
        let strict = self.strict() || self.unicode_mode;
        loop {
            let range_start = self.index();
            if !self.consume_class_atom()? {
                break;
            }
            let min = self.last_int_value;
            if !self.eat(HYPHEN_MINUS) {
                continue;
            }
            let i = self.index();
            self.h.on_character(i - 1, i, HYPHEN_MINUS as f64);
            if !self.consume_class_atom()? {
                break;
            }
            let max = self.last_int_value;
            if min == -1.0 || max == -1.0 {
                if strict {
                    return self.raise("Invalid character class");
                }
                continue;
            }
            if min > max {
                return self.raise("Range out of order in character class");
            }
            let end = self.index();
            self.h.on_character_class_range(range_start, end, min, max);
        }
        Ok(false)
    }

    fn consume_class_atom(&mut self) -> Res<bool> {
        let start = self.index();
        let cp = self.cp();
        if cp != -1 && cp != REVERSE_SOLIDUS && cp != RIGHT_SQUARE_BRACKET {
            self.advance();
            self.last_int_value = cp as f64;
            let end = self.index();
            self.h.on_character(start, end, self.last_int_value);
            return Ok(true);
        }
        if self.eat(REVERSE_SOLIDUS) {
            if self.consume_class_escape()? {
                return Ok(true);
            }
            if !self.strict() && self.cp() == LATIN_SMALL_LETTER_C {
                self.last_int_value = REVERSE_SOLIDUS as f64;
                let end = self.index();
                self.h.on_character(start, end, self.last_int_value);
                return Ok(true);
            }
            if self.strict() || self.unicode_mode {
                return self.raise("Invalid escape");
            }
            self.rewind(start);
        }
        Ok(false)
    }

    fn consume_class_escape(&mut self) -> Res<bool> {
        let start = self.index();
        if self.eat(LATIN_SMALL_LETTER_B) {
            self.last_int_value = BACKSPACE as f64;
            let end = self.index();
            self.h.on_character(start - 1, end, self.last_int_value);
            return Ok(true);
        }
        if self.unicode_mode && self.eat(HYPHEN_MINUS) {
            self.last_int_value = HYPHEN_MINUS as f64;
            let end = self.index();
            self.h.on_character(start - 1, end, self.last_int_value);
            return Ok(true);
        }
        if !self.strict() && !self.unicode_mode && self.cp() == LATIN_SMALL_LETTER_C {
            let cp = self.next_cp();
            if is_decimal_digit(cp) || cp == LOW_LINE {
                self.advance();
                self.advance();
                self.last_int_value = (cp % 0x20) as f64;
                let end = self.index();
                self.h.on_character(start - 1, end, self.last_int_value);
                return Ok(true);
            }
        }
        Ok(self.consume_character_class_escape()?.is_some() || self.consume_character_escape()?)
    }

    fn consume_class_set_expression(&mut self) -> Res<bool> {
        let start = self.index();
        let mut may_contain_strings;
        if self.consume_class_set_character()? {
            if self.consume_class_set_range_from_operator(start)? {
                self.consume_class_union_right(false)?;
                return Ok(false);
            }
            may_contain_strings = false;
        } else if let Some(r) = self.consume_class_set_operand()? {
            may_contain_strings = r;
        } else {
            let cp = self.cp();
            if cp == REVERSE_SOLIDUS {
                self.advance();
                return self.raise("Invalid escape");
            }
            if cp == self.next_cp() && is_class_set_reserved_double_punctuator_character(cp) {
                return self.raise("Invalid set operation in character class");
            }
            return self.raise("Invalid character in character class");
        }
        if self.eat2(AMPERSAND, AMPERSAND) {
            while self.cp() != AMPERSAND {
                let Some(r) = self.consume_class_set_operand()? else {
                    break;
                };
                let end = self.index();
                self.h.on_class_intersection(start, end);
                if !r {
                    may_contain_strings = false;
                }
                if self.eat2(AMPERSAND, AMPERSAND) {
                    continue;
                }
                return Ok(may_contain_strings);
            }
            return self.raise("Invalid character in character class");
        }
        if self.eat2(HYPHEN_MINUS, HYPHEN_MINUS) {
            while self.consume_class_set_operand()?.is_some() {
                let end = self.index();
                self.h.on_class_subtraction(start, end);
                if self.eat2(HYPHEN_MINUS, HYPHEN_MINUS) {
                    continue;
                }
                return Ok(may_contain_strings);
            }
            return self.raise("Invalid character in character class");
        }
        self.consume_class_union_right(may_contain_strings)
    }

    fn consume_class_union_right(&mut self, left: bool) -> Res<bool> {
        let mut may_contain_strings = left;
        loop {
            let start = self.index();
            if self.consume_class_set_character()? {
                self.consume_class_set_range_from_operator(start)?;
                continue;
            }
            if let Some(r) = self.consume_class_set_operand()? {
                if r {
                    may_contain_strings = true;
                }
                continue;
            }
            break;
        }
        Ok(may_contain_strings)
    }

    fn consume_class_set_range_from_operator(&mut self, start: usize) -> Res<bool> {
        let current_start = self.index();
        let min = self.last_int_value;
        if self.eat(HYPHEN_MINUS) {
            if self.consume_class_set_character()? {
                let max = self.last_int_value;
                if min == -1.0 || max == -1.0 {
                    return self.raise("Invalid character class");
                }
                if min > max {
                    return self.raise("Range out of order in character class");
                }
                let end = self.index();
                self.h.on_character_class_range(start, end, min, max);
                return Ok(true);
            }
            self.rewind(current_start);
        }
        Ok(false)
    }

    fn consume_class_set_operand(&mut self) -> Res<Strings> {
        if let Some(r) = self.consume_nested_class()? {
            return Ok(Some(r));
        }
        if let Some(r) = self.consume_class_string_disjunction()? {
            return Ok(Some(r));
        }
        if self.consume_class_set_character()? {
            return Ok(Some(false));
        }
        Ok(None)
    }

    fn consume_nested_class(&mut self) -> Res<Strings> {
        let start = self.index();
        if self.eat(LEFT_SQUARE_BRACKET) {
            let negate = self.eat(CIRCUMFLEX_ACCENT);
            self.h.on_character_class_enter(start, negate, true);
            let result = self.consume_class_contents()?;
            if !self.eat(RIGHT_SQUARE_BRACKET) {
                return self.raise("Unterminated character class");
            }
            if negate && result {
                return self.raise("Negated character class may contain strings");
            }
            let end = self.index();
            self.h.on_character_class_leave(start, end, negate);
            return Ok(Some(result));
        }
        if self.eat(REVERSE_SOLIDUS) {
            if let Some(r) = self.consume_character_class_escape()? {
                return Ok(Some(r));
            }
            self.rewind(start);
        }
        Ok(None)
    }

    fn consume_class_string_disjunction(&mut self) -> Res<Strings> {
        let start = self.index();
        if self.eat3(REVERSE_SOLIDUS, LATIN_SMALL_LETTER_Q, LEFT_CURLY_BRACKET) {
            self.h.on_class_string_disjunction_enter(start);
            let mut i = 0;
            let mut may_contain_strings = false;
            loop {
                if self.consume_class_string(i)? {
                    may_contain_strings = true;
                }
                i += 1;
                if !self.eat(VERTICAL_LINE) {
                    break;
                }
            }
            if self.eat(RIGHT_CURLY_BRACKET) {
                let end = self.index();
                self.h.on_class_string_disjunction_leave(start, end);
                return Ok(Some(may_contain_strings));
            }
            return self.raise("Unterminated class string disjunction");
        }
        Ok(None)
    }

    fn consume_class_string(&mut self, i: usize) -> Res<bool> {
        let start = self.index();
        let mut count = 0;
        self.h.on_string_alternative_enter(start, i);
        while self.cp() != -1 && self.consume_class_set_character()? {
            count += 1;
        }
        let end = self.index();
        self.h.on_string_alternative_leave(start, end, i);
        Ok(count != 1)
    }

    fn consume_class_set_character(&mut self) -> Res<bool> {
        let start = self.index();
        let cp = self.cp();
        if (cp != self.next_cp() || !is_class_set_reserved_double_punctuator_character(cp))
            && cp != -1
            && !is_class_set_syntax_character(cp)
        {
            self.last_int_value = cp as f64;
            self.advance();
            let end = self.index();
            self.h.on_character(start, end, self.last_int_value);
            return Ok(true);
        }
        if self.eat(REVERSE_SOLIDUS) {
            if self.consume_character_escape()? {
                return Ok(true);
            }
            if is_class_set_reserved_punctuator(self.cp()) {
                self.last_int_value = self.cp() as f64;
                self.advance();
                let end = self.index();
                self.h.on_character(start, end, self.last_int_value);
                return Ok(true);
            }
            if self.eat(LATIN_SMALL_LETTER_B) {
                self.last_int_value = BACKSPACE as f64;
                let end = self.index();
                self.h.on_character(start, end, self.last_int_value);
                return Ok(true);
            }
            self.rewind(start);
        }
        Ok(false)
    }

    fn eat_group_name(&mut self) -> Res<bool> {
        if self.eat(LESS_THAN_SIGN) {
            if self.eat_regexp_identifier_name()? && self.eat(GREATER_THAN_SIGN) {
                return Ok(true);
            }
            return self.raise("Invalid capture group name");
        }
        Ok(false)
    }

    fn eat_regexp_identifier_name(&mut self) -> Res<bool> {
        if self.eat_regexp_identifier_start()? {
            self.last_str_value = from_code_point(self.last_int_value as i64)
                .map(String::from)
                .unwrap_or_default();
            while self.eat_regexp_identifier_part()? {
                if let Some(c) = from_code_point(self.last_int_value as i64) {
                    self.last_str_value.push(c);
                }
            }
            return Ok(true);
        }
        Ok(false)
    }

    fn eat_regexp_identifier_start(&mut self) -> Res<bool> {
        let start = self.index();
        let force_u_flag = !self.unicode_mode && self.ecma_version >= 2020;
        let mut cp = self.cp();
        self.advance();
        if cp == REVERSE_SOLIDUS && self.eat_regexp_unicode_escape_sequence(force_u_flag)? {
            cp = self.last_int_value as i64;
        } else if force_u_flag && is_lead_surrogate(cp) && is_trail_surrogate(self.cp()) {
            cp = combine_surrogate_pair(cp, self.cp());
            self.advance();
        }
        if is_identifier_start_char(cp) {
            self.last_int_value = cp as f64;
            return Ok(true);
        }
        if self.index() != start {
            self.rewind(start);
        }
        Ok(false)
    }

    fn eat_regexp_identifier_part(&mut self) -> Res<bool> {
        let start = self.index();
        let force_u_flag = !self.unicode_mode && self.ecma_version >= 2020;
        let mut cp = self.cp();
        self.advance();
        if cp == REVERSE_SOLIDUS && self.eat_regexp_unicode_escape_sequence(force_u_flag)? {
            cp = self.last_int_value as i64;
        } else if force_u_flag && is_lead_surrogate(cp) && is_trail_surrogate(self.cp()) {
            cp = combine_surrogate_pair(cp, self.cp());
            self.advance();
        }
        if is_identifier_part_char(cp) {
            self.last_int_value = cp as f64;
            return Ok(true);
        }
        if self.index() != start {
            self.rewind(start);
        }
        Ok(false)
    }

    fn eat_c_control_letter(&mut self) -> bool {
        let start = self.index();
        if self.eat(LATIN_SMALL_LETTER_C) {
            if self.eat_control_letter() {
                return true;
            }
            self.rewind(start);
        }
        false
    }

    fn eat_zero(&mut self) -> bool {
        if self.cp() == DIGIT_ZERO && !is_decimal_digit(self.next_cp()) {
            self.last_int_value = 0.0;
            self.advance();
            return true;
        }
        false
    }

    fn eat_control_escape(&mut self) -> bool {
        for (c, v) in [
            (LATIN_SMALL_LETTER_F, FORM_FEED),
            (LATIN_SMALL_LETTER_N, LINE_FEED),
            (LATIN_SMALL_LETTER_R, CARRIAGE_RETURN),
            (LATIN_SMALL_LETTER_T, CHARACTER_TABULATION),
            (LATIN_SMALL_LETTER_V, LINE_TABULATION),
        ] {
            if self.eat(c) {
                self.last_int_value = v as f64;
                return true;
            }
        }
        false
    }

    fn eat_control_letter(&mut self) -> bool {
        let cp = self.cp();
        if is_latin_letter(cp) {
            self.advance();
            self.last_int_value = (cp % 0x20) as f64;
            return true;
        }
        false
    }

    fn eat_regexp_unicode_escape_sequence(&mut self, force_u_flag: bool) -> Res<bool> {
        let start = self.index();
        let u_flag = force_u_flag || self.unicode_mode;
        if self.eat(LATIN_SMALL_LETTER_U) {
            if (u_flag && self.eat_regexp_unicode_surrogate_pair_escape())
                || self.eat_fixed_hex_digits(4)
                || (u_flag && self.eat_regexp_unicode_code_point_escape())
            {
                return Ok(true);
            }
            if self.strict() || u_flag {
                return self.raise("Invalid unicode escape");
            }
            self.rewind(start);
        }
        Ok(false)
    }

    fn eat_regexp_unicode_surrogate_pair_escape(&mut self) -> bool {
        let start = self.index();
        if self.eat_fixed_hex_digits(4) {
            let lead = self.last_int_value as i64;
            if is_lead_surrogate(lead)
                && self.eat(REVERSE_SOLIDUS)
                && self.eat(LATIN_SMALL_LETTER_U)
                && self.eat_fixed_hex_digits(4)
            {
                let trail = self.last_int_value as i64;
                if is_trail_surrogate(trail) {
                    self.last_int_value = combine_surrogate_pair(lead, trail) as f64;
                    return true;
                }
            }
            self.rewind(start);
        }
        false
    }

    fn eat_regexp_unicode_code_point_escape(&mut self) -> bool {
        let start = self.index();
        if self.eat(LEFT_CURLY_BRACKET)
            && self.eat_hex_digits()
            && self.eat(RIGHT_CURLY_BRACKET)
            && (0.0..=MAX_CODE_POINT).contains(&self.last_int_value)
        {
            return true;
        }
        self.rewind(start);
        false
    }

    fn eat_identity_escape(&mut self) -> bool {
        let cp = self.cp();
        if self.is_valid_identity_escape(cp) {
            self.last_int_value = cp as f64;
            self.advance();
            return true;
        }
        false
    }

    fn is_valid_identity_escape(&self, cp: i64) -> bool {
        if cp == -1 {
            return false;
        }
        if self.unicode_mode {
            return is_syntax_character(cp) || cp == SOLIDUS;
        }
        if self.strict() {
            return !is_id_continue(cp);
        }
        if self.n_flag {
            return !(cp == LATIN_SMALL_LETTER_C || cp == LATIN_SMALL_LETTER_K);
        }
        cp != LATIN_SMALL_LETTER_C
    }

    fn eat_decimal_escape(&mut self) -> bool {
        self.last_int_value = 0.0;
        let mut cp = self.cp();
        if (DIGIT_ONE..=DIGIT_NINE).contains(&cp) {
            loop {
                self.last_int_value = 10.0 * self.last_int_value + (cp - DIGIT_ZERO) as f64;
                self.advance();
                cp = self.cp();
                if !(DIGIT_ZERO..=DIGIT_NINE).contains(&cp) {
                    break;
                }
            }
            return true;
        }
        false
    }

    #[allow(clippy::type_complexity)]
    fn eat_unicode_property_value_expression(
        &mut self,
    ) -> Res<Option<(String, Option<String>, bool)>> {
        let start = self.index();
        if self.eat_unicode_property_name() && self.eat(EQUALS_SIGN) {
            let key = self.last_str_value.clone();
            if self.eat_unicode_property_value() {
                let value = self.last_str_value.clone();
                if is_valid_unicode_property(self.ecma_version, &key, &value) {
                    let v = if value.is_empty() { None } else { Some(value) };
                    return Ok(Some((key, v, false)));
                }
                return self.raise("Invalid property name");
            }
        }
        self.rewind(start);
        if self.eat_unicode_property_value() {
            let name_or_value = self.last_str_value.clone();
            if is_valid_unicode_property(self.ecma_version, "General_Category", &name_or_value) {
                let v = if name_or_value.is_empty() {
                    None
                } else {
                    Some(name_or_value)
                };
                return Ok(Some(("General_Category".to_string(), v, false)));
            }
            if is_valid_lone_unicode_property(self.ecma_version, &name_or_value) {
                return Ok(Some((name_or_value, None, false)));
            }
            if self.unicode_sets_mode
                && is_valid_lone_unicode_property_of_string(self.ecma_version, &name_or_value)
            {
                return Ok(Some((name_or_value, None, true)));
            }
            return self.raise("Invalid property name");
        }
        Ok(None)
    }

    fn eat_unicode_property_name(&mut self) -> bool {
        self.last_str_value.clear();
        while is_unicode_property_name_character(self.cp()) {
            if let Some(c) = from_code_point(self.cp()) {
                self.last_str_value.push(c);
            }
            self.advance();
        }
        !self.last_str_value.is_empty()
    }

    fn eat_unicode_property_value(&mut self) -> bool {
        self.last_str_value.clear();
        while is_unicode_property_value_character(self.cp()) {
            if let Some(c) = from_code_point(self.cp()) {
                self.last_str_value.push(c);
            }
            self.advance();
        }
        !self.last_str_value.is_empty()
    }

    fn eat_hex_escape_sequence(&mut self) -> Res<bool> {
        let start = self.index();
        if self.eat(LATIN_SMALL_LETTER_X) {
            if self.eat_fixed_hex_digits(2) {
                return Ok(true);
            }
            if self.unicode_mode || self.strict() {
                return self.raise("Invalid escape");
            }
            self.rewind(start);
        }
        Ok(false)
    }

    fn eat_decimal_digits(&mut self) -> bool {
        let start = self.index();
        self.last_int_value = 0.0;
        while is_decimal_digit(self.cp()) {
            self.last_int_value = 10.0 * self.last_int_value + digit_to_int(self.cp()) as f64;
            self.advance();
        }
        self.index() != start
    }

    fn eat_hex_digits(&mut self) -> bool {
        let start = self.index();
        self.last_int_value = 0.0;
        while is_hex_digit(self.cp()) {
            self.last_int_value = 16.0 * self.last_int_value + digit_to_int(self.cp()) as f64;
            self.advance();
        }
        self.index() != start
    }

    fn eat_legacy_octal_escape_sequence(&mut self) -> bool {
        if self.eat_octal_digit() {
            let n1 = self.last_int_value;
            if self.eat_octal_digit() {
                let n2 = self.last_int_value;
                if n1 <= 3.0 && self.eat_octal_digit() {
                    self.last_int_value += n1 * 64.0 + n2 * 8.0;
                } else {
                    self.last_int_value = n1 * 8.0 + n2;
                }
            } else {
                self.last_int_value = n1;
            }
            return true;
        }
        false
    }

    fn eat_octal_digit(&mut self) -> bool {
        let cp = self.cp();
        if is_octal_digit(cp) {
            self.advance();
            self.last_int_value = (cp - DIGIT_ZERO) as f64;
            return true;
        }
        self.last_int_value = 0.0;
        false
    }

    fn eat_fixed_hex_digits(&mut self, length: usize) -> bool {
        let start = self.index();
        self.last_int_value = 0.0;
        for _ in 0..length {
            let cp = self.cp();
            if !is_hex_digit(cp) {
                self.rewind(start);
                return false;
            }
            self.last_int_value = 16.0 * self.last_int_value + digit_to_int(cp) as f64;
            self.advance();
        }
        true
    }

    fn eat_modifiers(&mut self) -> bool {
        let mut ate = false;
        while is_regular_expression_modifier(self.cp()) {
            self.advance();
            ate = true;
        }
        ate
    }

    fn parse_modifiers(&mut self, start: usize, end: usize) -> Res<Flags> {
        let source = self.reader.s.clone();
        let f = self.parse_flags(&source, start, end)?;
        Ok(Flags {
            ignore_case: f.ignore_case,
            multiline: f.multiline,
            dot_all: f.dot_all,
            ..Flags::default()
        })
    }

    fn parse_flags(&mut self, source: &[u16], start: usize, end: usize) -> Res<Flags> {
        let mut flags = Flags::default();
        let v = self.ecma_version;
        for i in start..end {
            let flag = source.get(i).copied().map_or(-1, |c| c as i64);
            let valid = matches!(
                flag,
                LATIN_SMALL_LETTER_G | LATIN_SMALL_LETTER_I | LATIN_SMALL_LETTER_M
            ) || (v >= 2015
                && matches!(flag, LATIN_SMALL_LETTER_U | LATIN_SMALL_LETTER_Y))
                || (v >= 2018 && flag == LATIN_SMALL_LETTER_S)
                || (v >= 2022 && flag == LATIN_SMALL_LETTER_D)
                || (v >= 2024 && flag == LATIN_SMALL_LETTER_V);
            let ch = from_code_point(flag);
            let shown = ch.unwrap_or('\u{fffd}');
            if valid {
                if let Some(slot) = flags.get_mut(flag) {
                    if *slot {
                        let mut e = self.error(&format!("Duplicated flag '{shown}'"), start, None);
                        e.lossy |= ch.is_none();
                        return Err(e);
                    }
                    *slot = true;
                }
            } else {
                let mut e = self.error(&format!("Invalid flag '{shown}'"), start, None);
                e.lossy |= ch.is_none();
                return Err(e);
            }
        }
        Ok(flags)
    }
}
