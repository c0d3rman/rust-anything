//! lib/rules/no-loss-of-precision.js

use crate::ast::{NodeId, Prop};
use crate::linter::Cx;
use crate::rule::Report;
use crate::rules::{Rule, RuleDef};
use crate::utils::literal_typeof;
use serde_json::Value;

pub static DEF: RuleDef = RuleDef {
    name: "no-loss-of-precision",
    messages: &[(
        "noLossOfPrecision",
        "This number literal will lose precision at runtime.",
    )],
    has_default_options: false,
    create,
};

struct R {
    unsupported: Option<String>,
}

fn create(options: &[Value]) -> Result<Box<dyn Rule>, String> {
    if !options.is_empty() {
        return Err("invalid option".into());
    }
    Ok(Box::new(R { unsupported: None }))
}

/// `(mantissa, exponent)` with `value == mantissa * 2^exponent`, for a finite positive double.
fn decompose(v: f64) -> (u64, i32) {
    let bits = v.to_bits();
    let exp = ((bits >> 52) & 0x7ff) as i32;
    let frac = bits & ((1u64 << 52) - 1);
    if exp == 0 {
        (frac, -1074)
    } else {
        (frac | (1u64 << 52), exp - 1075)
    }
}

/// The exact decimal digits of a finite positive double and the power of ten of its last digit.
fn exact_decimal(v: f64) -> (String, i64) {
    const BASE: u64 = 1_000_000_000;
    let (m, e) = decompose(v);
    let mut limbs: Vec<u64> = vec![m % BASE, (m / BASE) % BASE, m / BASE / BASE];
    let (factor, times, scale) = if e >= 0 {
        (2u64, e, 0i64)
    } else {
        (5u64, -e, e as i64)
    };
    for _ in 0..times {
        let mut carry = 0u64;
        for l in limbs.iter_mut() {
            let x = *l * factor + carry;
            *l = x % BASE;
            carry = x / BASE;
        }
        if carry > 0 {
            limbs.push(carry);
        }
    }
    while limbs.len() > 1 && limbs.last() == Some(&0) {
        limbs.pop();
    }
    let mut s = String::new();
    for (i, l) in limbs.iter().rev().enumerate() {
        if i == 0 {
            s.push_str(&l.to_string());
        } else {
            s.push_str(&format!("{l:09}"));
        }
    }
    (s, scale)
}

/// `Number.prototype.toPrecision(p)` for a finite positive double, 1 <= p <= 100.
pub fn to_precision(v: f64, p: usize) -> String {
    let (digits, scale) = exact_decimal(v);
    let mut e = digits.len() as i64 - 1 + scale;
    let mut d: Vec<u8> = digits.into_bytes();
    if d.len() > p {
        let round_up = d[p] >= b'5';
        d.truncate(p);
        if round_up {
            let mut i = p;
            loop {
                if i == 0 {
                    d.insert(0, b'1');
                    d.truncate(p);
                    e += 1;
                    break;
                }
                i -= 1;
                if d[i] == b'9' {
                    d[i] = b'0';
                } else {
                    d[i] += 1;
                    break;
                }
            }
        }
    }
    while d.len() < p {
        d.push(b'0');
    }
    let d = String::from_utf8(d).unwrap_or_default();
    let p = p as i64;
    if e < -6 || e >= p {
        let mut s = d[..1].to_string();
        if p > 1 {
            s.push('.');
            s.push_str(&d[1..]);
        }
        s.push('e');
        s.push(if e >= 0 { '+' } else { '-' });
        s.push_str(&e.abs().to_string());
        s
    } else if e >= 0 {
        let k = (e + 1) as usize;
        if (p as usize) > k {
            format!("{}.{}", &d[..k], &d[k..])
        } else {
            d
        }
    } else {
        format!("0.{}{}", "0".repeat((-e - 1) as usize), d)
    }
}

/// `value.toString(radix)` for a non-negative integral double and a radix of 2, 8 or 16.
fn to_radix_string(v: f64, radix: u32) -> String {
    if v == 0.0 {
        return "0".into();
    }
    let (m, e) = decompose(v);
    let mut bits: Vec<u8> = format!("{m:b}").into_bytes();
    if e >= 0 {
        bits.extend(std::iter::repeat_n(b'0', e as usize));
    } else {
        let keep = bits.len().saturating_sub((-e) as usize);
        bits.truncate(keep);
    }
    let per = match radix {
        2 => 1,
        8 => 3,
        _ => 4,
    };
    let pad = (per - bits.len() % per) % per;
    let mut padded = vec![b'0'; pad];
    padded.extend(bits);
    let mut out = String::new();
    for chunk in padded.chunks(per) {
        let n = chunk.iter().fold(0u32, |a, b| a * 2 + u32::from(b - b'0'));
        out.push(std::char::from_digit(n, radix).unwrap_or('0'));
    }
    let trimmed = out.trim_start_matches('0');
    if trimmed.is_empty() {
        "0".into()
    } else {
        trimmed.to_string()
    }
}

fn remove_leading_zeros(s: &str) -> &str {
    match s.find(|c| c != '0') {
        Some(i) => &s[i..],
        None => s,
    }
}

fn remove_trailing_zeros(s: &str) -> &str {
    match s.rfind(|c| c != '0') {
        Some(i) => &s[..=i],
        None => s,
    }
}

struct Scientific {
    coefficient: String,
    magnitude: i64,
}

fn normalize_integer(s: &str) -> Scientific {
    let trimmed = remove_leading_zeros(s);
    let significant = remove_trailing_zeros(trimmed);
    Scientific {
        coefficient: significant.to_string(),
        magnitude: trimmed.len() as i64 - 1,
    }
}

fn normalize_float(s: &str) -> Scientific {
    let trimmed = remove_leading_zeros(s);
    match trimmed.find('.') {
        Some(0) => {
            let significant = remove_leading_zeros(&trimmed[1..]);
            Scientific {
                coefficient: significant.to_string(),
                magnitude: significant.len() as i64 - trimmed.len() as i64,
            }
        }
        None => Scientific {
            coefficient: trimmed.to_string(),
            magnitude: trimmed.len() as i64 - 1,
        },
        Some(i) => Scientific {
            coefficient: trimmed.replacen('.', "", 1),
            magnitude: i as i64 - 1,
        },
    }
}

/// `parseInt(s, 10)` for the exponent part of a number literal or of `toPrecision` output.
fn parse_int(s: &str) -> Option<i64> {
    let (neg, digits) = match s.as_bytes().first() {
        Some(b'-') => (true, &s[1..]),
        Some(b'+') => (false, &s[1..]),
        _ => (false, s),
    };
    let end = digits
        .find(|c: char| !c.is_ascii_digit())
        .unwrap_or(digits.len());
    let n: i64 = digits[..end].parse().ok()?;
    Some(if neg { -n } else { n })
}

fn to_scientific(s: &str, parse_as_float: bool) -> Option<Scientific> {
    let mut parts = s.split('e');
    let coefficient = parts.next().unwrap_or("");
    let mut n = if parse_as_float || s.contains('.') {
        normalize_float(coefficient)
    } else {
        normalize_integer(coefficient)
    };
    if let Some(exp) = parts.next() {
        n.magnitude += parse_int(exp)?;
    }
    Some(n)
}

fn base_ten_loses_precision(raw: &str, value: f64) -> Option<bool> {
    let mut raw_number = raw.replace('_', "").to_ascii_lowercase();
    if let Some(i) = raw_number.find('.')
        && matches!(raw_number.as_bytes().get(i + 1), None | Some(b'e'))
    {
        raw_number.remove(i);
    }
    let normalized_raw = to_scientific(&raw_number, false)?;
    if value == 0.0 {
        let c = &normalized_raw.coefficient;
        return Some(!(!c.is_empty() && c.bytes().all(|b| b == b'0')));
    }
    let precision = normalized_raw.coefficient.len();
    if precision > 100 {
        return Some(true);
    }
    if precision == 0 || !value.is_finite() {
        return None;
    }
    let stored = to_precision(value, precision);
    let normalized_stored = to_scientific(&stored, true)?;
    Some(
        normalized_raw.magnitude != normalized_stored.magnitude
            || normalized_raw.coefficient != normalized_stored.coefficient,
    )
}

fn not_base_ten_loses_precision(raw: &str, value: f64) -> Option<bool> {
    let raw_string = raw.replace('_', "").to_ascii_uppercase();
    let base = if raw_string.starts_with("0B") {
        2
    } else if raw_string.starts_with("0X") {
        16
    } else {
        8
    };
    if !value.is_finite() || value.fract() != 0.0 || value < 0.0 {
        return None;
    }
    Some(!raw_string.ends_with(&to_radix_string(value, base).to_ascii_uppercase()))
}

fn is_base_ten(raw: &str) -> bool {
    let prefixed = ["0x", "0X", "0b", "0B", "0o", "0O"]
        .iter()
        .any(|p| raw.starts_with(p));
    let legacy_octal = raw.len() > 1
        && raw.starts_with('0')
        && raw[1..].bytes().all(|b| (b'0'..=b'7').contains(&b));
    !prefixed && !legacy_octal
}

impl Rule for R {
    fn listeners(&self) -> &'static [&'static str] {
        &["Literal"]
    }

    fn on(&mut self, _: usize, node: NodeId, cx: &mut Cx) {
        let ast = &cx.sc.ast;
        if literal_typeof(ast, node) != "number" {
            return;
        }
        let Some(Prop::Num(value)) = ast.prop(node, "value") else {
            return;
        };
        let raw = ast.str(node, "raw").unwrap_or("");
        let loses = if is_base_ten(raw) {
            base_ten_loses_precision(raw, *value)
        } else {
            not_base_ten_loses_precision(raw, *value)
        };
        match loses {
            Some(true) => cx.report(Report::node(node, "noLossOfPrecision")),
            Some(false) => {}
            None => self.unsupported = Some(format!("number literal {raw}")),
        }
    }

    fn unsupported(&self) -> Option<&str> {
        self.unsupported.as_deref()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn precision() {
        assert_eq!(to_precision(1.0, 1), "1");
        assert_eq!(to_precision(123.456, 4), "123.5");
        assert_eq!(to_precision(0.000001, 2), "0.0000010");
        assert_eq!(to_precision(0.0000001, 2), "1.0e-7");
        assert_eq!(to_precision(1e21, 3), "1.00e+21");
        assert_eq!(to_precision(9.99, 2), "10");
        assert_eq!(to_precision(99.9, 2), "1.0e+2");
        assert_eq!(to_precision(0.1, 20), "0.10000000000000000555");
        assert_eq!(to_precision(9007199254740993.0, 16), "9007199254740992");
        assert_eq!(to_precision(5e-324, 3), "4.94e-324");
    }

    #[test]
    fn radix() {
        assert_eq!(to_radix_string(255.0, 16), "ff");
        assert_eq!(to_radix_string(8.0, 8), "10");
        assert_eq!(to_radix_string(5.0, 2), "101");
        assert_eq!(to_radix_string(2f64.powi(60), 16), "1000000000000000");
    }

    #[test]
    fn literals() {
        assert_eq!(
            base_ten_loses_precision("9007199254740993", 9007199254740993.0),
            Some(true)
        );
        assert_eq!(
            base_ten_loses_precision("9007199254740992", 9007199254740992.0),
            Some(false)
        );
        assert_eq!(base_ten_loses_precision("1.50", 1.5), Some(false));
        assert_eq!(base_ten_loses_precision("0.0", 0.0), Some(false));
        assert_eq!(base_ten_loses_precision("1e-7", 1e-7), Some(false));
        assert_eq!(base_ten_loses_precision("123.", 123.0), Some(false));
        assert_eq!(
            base_ten_loses_precision(".1230000000000000000000004", 0.123),
            Some(true)
        );
        assert_eq!(
            not_base_ten_loses_precision("0x20000000000001", 9007199254740992.0),
            Some(true)
        );
        assert_eq!(not_base_ten_loses_precision("0b1_01", 5.0), Some(false));
        assert!(!is_base_ten("0777"));
        assert!(is_base_ten("0789"));
    }
}
