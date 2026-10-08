//! Tolerant scalar decoding for the typed views.
//!
//! Last.fm sends counts and timestamps as strings in some places and as
//! numbers in others, and an object where a list of one is meant. Each helper
//! here accepts exactly what its documentation lists and returns `None` for
//! anything else, so a malformed value is an error at the call site and is
//! never coerced into a default.

use serde_json::Value;

/// An unsigned integer: a JSON integer, or a string of ASCII digits and
/// nothing else.
///
/// Rejected: floats (`1.0`, `1e3`), negative numbers, signs, whitespace, the
/// empty string, booleans, `null`, arrays and objects. A value that does not
/// fit in a `u64` is rejected.
pub(crate) fn uint(value: &Value) -> Option<u64> {
    match value {
        Value::Number(n) => n.as_u64(),
        Value::String(s) if !s.is_empty() && s.bytes().all(|b| b.is_ascii_digit()) => {
            s.parse().ok()
        }
        _ => None,
    }
}

/// A 0/1 flag: the strings `"0"` and `"1"`, or the integers `0` and `1`.
///
/// Rejected: every other number or string, JSON booleans, `null`.
pub(crate) fn flag(value: &Value) -> Option<bool> {
    match value {
        Value::String(s) => match s.as_str() {
            "0" => Some(false),
            "1" => Some(true),
            _ => None,
        },
        Value::Number(n) => match n.as_u64() {
            Some(0) => Some(false),
            Some(1) => Some(true),
            _ => None,
        },
        _ => None,
    }
}

/// A [`flag`] that may also be spelled `"true"` or `"false"`, as the
/// `nowplaying` attribute of a recorded recent-tracks row is. Only those two
/// lowercase strings are added; a JSON boolean is still rejected.
pub(crate) fn flag_or_word(value: &Value) -> Option<bool> {
    match value {
        Value::String(s) if s == "true" => Some(true),
        Value::String(s) if s == "false" => Some(false),
        _ => flag(value),
    }
}

/// An array, or a single object standing for an array of one.
///
/// Anything else, including `null` and strings, is `None`: the caller decides
/// whether an absent or empty value is acceptable for its field.
pub(crate) fn one_or_many(value: Value) -> Option<Vec<Value>> {
    match value {
        Value::Array(items) => Some(items),
        Value::Object(_) => Some(vec![value]),
        _ => None,
    }
}

#[cfg(test)]
mod tests;
