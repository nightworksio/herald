//! JSON written one way only, as RFC 8785 (the JSON Canonicalization
//! Scheme) writes it, so two instances hash and sign the same bytes.

use std::cmp::Ordering;
use std::fmt::{self, Write as _};

use serde_json::{Number, Value};

/// The decimal exponent from which a double is written in exponent form.
const WIDEST: i32 = 21;
/// The decimal exponent below which a fraction is written in exponent form.
const NARROWEST: i32 = -6;

/// Why a value has no canonical form.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Uncanonical {
    /// An integer that reads as a double ECMAScript writes otherwise, so
    /// RFC 8785 would write another number.
    Inexact {
        /// The integer as written.
        number: String,
    },
}

impl fmt::Display for Uncanonical {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Inexact { number } => write!(
                f,
                "the integer {number} reads as a double written otherwise, so it has no canonical JSON form; write it as a string"
            ),
        }
    }
}

/// `value` written canonically: members sorted by their names' UTF-16 code
/// units, no whitespace, strings escaped as ECMAScript's `JSON.stringify`
/// escapes them, and numbers written as ECMAScript writes a double.
///
/// # Errors
///
/// An integer that reads as a double written otherwise has no canonical
/// form.
pub fn canonical(value: &Value) -> Result<String, Uncanonical> {
    let mut out = String::new();
    write_value(&mut out, value)?;
    Ok(out)
}

fn write_value(out: &mut String, value: &Value) -> Result<(), Uncanonical> {
    match value {
        Value::Null => out.push_str("null"),
        Value::Bool(flag) => out.push_str(if *flag { "true" } else { "false" }),
        Value::Number(number) => out.push_str(&number_text(number)?),
        Value::String(text) => write_string(out, text),
        Value::Array(items) => {
            out.push('[');
            for (at, item) in items.iter().enumerate() {
                if at > 0 {
                    out.push(',');
                }
                write_value(out, item)?;
            }
            out.push(']');
        }
        Value::Object(members) => {
            let mut sorted: Vec<(&String, &Value)> = members.iter().collect();
            sorted.sort_by(|(left, _), (right, _)| utf16_order(left, right));
            out.push('{');
            for (at, (name, member)) in sorted.into_iter().enumerate() {
                if at > 0 {
                    out.push(',');
                }
                write_string(out, name);
                out.push(':');
                write_value(out, member)?;
            }
            out.push('}');
        }
    }
    Ok(())
}

fn utf16_order(left: &str, right: &str) -> Ordering {
    left.encode_utf16().cmp(right.encode_utf16())
}

fn write_string(out: &mut String, text: &str) {
    out.push('"');
    for character in text.chars() {
        match character {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\u{8}' => out.push_str("\\b"),
            '\u{c}' => out.push_str("\\f"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            control if u32::from(control) < 0x20 => {
                let _ = write!(out, "\\u{:04x}", u32::from(control));
            }
            other => out.push(other),
        }
    }
    out.push('"');
}

fn number_text(number: &Number) -> Result<String, Uncanonical> {
    let text = double_text(number.as_f64().unwrap_or_default());
    if !number.is_f64() && text != number.to_string() {
        return Err(Uncanonical::Inexact {
            number: number.to_string(),
        });
    }
    Ok(text)
}

/// The finite double `value` as ECMAScript's `Number.prototype.toString`
/// writes it: its shortest round-tripping digits, in exponent form only
/// past 10^21 or under 10^-6.
fn double_text(value: f64) -> String {
    let sign = if value < 0.0 { "-" } else { "" };
    let scientific = format!("{:e}", value.abs());
    let (mantissa, exponent) = scientific
        .split_once('e')
        .unwrap_or((scientific.as_str(), "0"));
    let digits: String = mantissa.chars().filter(char::is_ascii_digit).collect();
    let point = exponent.parse::<i32>().unwrap_or_default() + 1;
    format!("{sign}{}", placed(&digits, point))
}

/// The significant `digits` with the decimal point `point` places from
/// their start, as ECMAScript places it.
fn placed(digits: &str, point: i32) -> String {
    let count = i32::try_from(digits.len()).unwrap_or(i32::MAX);
    let (head, tail) = digits.split_at(digits.len().min(1));
    if count <= point && point <= WIDEST {
        format!("{digits}{}", zeros(point - count))
    } else if 0 < point && point <= WIDEST {
        let (whole, fraction) = digits.split_at(usize::try_from(point).unwrap_or_default());
        format!("{whole}.{fraction}")
    } else if NARROWEST < point && point <= 0 {
        format!("0.{}{digits}", zeros(-point))
    } else {
        let power = point - 1;
        let fraction = if tail.is_empty() {
            String::new()
        } else {
            format!(".{tail}")
        };
        format!("{head}{fraction}e{power:+}")
    }
}

fn zeros(count: i32) -> String {
    "0".repeat(usize::try_from(count).unwrap_or_default())
}

#[cfg(test)]
mod tests;
