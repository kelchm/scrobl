//! Tolerant scalar decoding for the typed views.
//!
//! Last.fm sends counts and timestamps as strings in some places and as
//! numbers in others, and an object where a list of one is meant. Each helper
//! here accepts exactly what its documentation lists and returns `None` for
//! anything else, so a malformed value is an error at the call site and is
//! never coerced into a default.

use std::cell::Cell;
use std::fmt;

use serde::de::{DeserializeSeed, MapAccess, SeqAccess, Visitor};
use serde_json::{Map, Number, Value};

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

/// Why [`strict_value`] failed.
#[derive(Debug)]
pub(crate) enum StrictError {
    /// An object repeats a member name. The position is where the parser was
    /// when it saw the second one. The name is not kept: it is response text.
    Duplicate { line: usize, column: usize },
    /// The bytes are not valid JSON, or nest deeper than `serde_json` allows.
    Json(serde_json::Error),
}

/// Parses one JSON document into a [`Value`], rejecting an object that
/// repeats a member name at any depth.
///
/// `serde_json::from_slice::<Value>` keeps the last of two members with one
/// name and says nothing, which would let a response lose a row unnoticed.
/// Everything else is as `from_slice`: the same value for any document
/// without repeats, the same recursion limit (the deserializer enforces it
/// before it calls this code), the same errors for malformed JSON.
pub(crate) fn strict_value(bytes: &[u8]) -> Result<Value, StrictError> {
    let duplicate = Cell::new(false);
    let mut deserializer = serde_json::Deserializer::from_slice(bytes);
    let parsed = Strict(&duplicate)
        .deserialize(&mut deserializer)
        .and_then(|value| deserializer.end().map(|()| value));
    parsed.map_err(|error| {
        if duplicate.get() {
            StrictError::Duplicate {
                line: error.line(),
                column: error.column(),
            }
        } else {
            StrictError::Json(error)
        }
    })
}

/// Builds a [`Value`] and raises the flag, with an error, on a repeated name.
struct Strict<'a>(&'a Cell<bool>);

impl<'de> DeserializeSeed<'de> for Strict<'_> {
    type Value = Value;

    fn deserialize<D: serde::Deserializer<'de>>(self, deserializer: D) -> Result<Value, D::Error> {
        deserializer.deserialize_any(self)
    }
}

impl<'de> Visitor<'de> for Strict<'_> {
    type Value = Value;

    fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("any JSON value")
    }

    fn visit_unit<E>(self) -> Result<Value, E> {
        Ok(Value::Null)
    }

    fn visit_bool<E>(self, v: bool) -> Result<Value, E> {
        Ok(Value::Bool(v))
    }

    fn visit_i64<E>(self, v: i64) -> Result<Value, E> {
        Ok(Value::Number(v.into()))
    }

    fn visit_u64<E>(self, v: u64) -> Result<Value, E> {
        Ok(Value::Number(v.into()))
    }

    fn visit_f64<E: serde::de::Error>(self, v: f64) -> Result<Value, E> {
        Number::from_f64(v)
            .map(Value::Number)
            .ok_or_else(|| E::custom("a number is not finite"))
    }

    fn visit_str<E>(self, v: &str) -> Result<Value, E> {
        Ok(Value::String(v.to_owned()))
    }

    fn visit_string<E>(self, v: String) -> Result<Value, E> {
        Ok(Value::String(v))
    }

    fn visit_seq<A: SeqAccess<'de>>(self, mut seq: A) -> Result<Value, A::Error> {
        let mut items = Vec::with_capacity(seq.size_hint().unwrap_or(0).min(4096));
        while let Some(item) = seq.next_element_seed(Strict(self.0))? {
            items.push(item);
        }
        Ok(Value::Array(items))
    }

    fn visit_map<A: MapAccess<'de>>(self, mut map: A) -> Result<Value, A::Error> {
        let mut members = Map::new();
        while let Some(name) = map.next_key::<String>()? {
            if members.contains_key(&name) {
                self.0.set(true);
                return Err(serde::de::Error::custom("an object repeats a member name"));
            }
            members.insert(name, map.next_value_seed(Strict(self.0))?);
        }
        Ok(Value::Object(members))
    }
}

#[cfg(test)]
mod tests;
