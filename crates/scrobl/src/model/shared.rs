//! The decoding every list goes through.

use serde_json::{Map, Value};

use super::page::{List, Page, Paging};
use crate::de;
use crate::error::Error;
use crate::protocol::{MethodSpec, Raw};

// Decoding. Every error names a field path; none repeats response text.

pub(super) fn missing_or(path: &str, expected: &str) -> Error {
    Error::decode_field(path, &format!("missing or not {expected}"))
}

/// Decodes the response to `spec` with `parse`, which is given the object
/// under `root`. An error carries the method and the response.
pub(super) fn decode<T>(
    raw: &Raw,
    spec: &'static MethodSpec,
    root: &str,
    parse: impl FnOnce(Map<String, Value>) -> Result<T, Error>,
) -> Result<T, Error> {
    if raw.method() != spec.name() {
        return Err(Error::decode("the response is for a different method")
            .with_method(raw.spec())
            .with_response(raw.status(), raw.body()));
    }
    let parsed = match raw.json_strict()? {
        Value::Object(mut body) => match body.remove(root) {
            Some(Value::Object(list)) => parse(list),
            _ => Err(missing_or(root, "an object")),
        },
        _ => Err(Error::decode("the body is not a JSON object")),
    };
    parsed.map_err(|e| {
        e.with_method(raw.spec())
            .with_response(raw.status(), raw.body())
    })
}

/// One page of a paged list: the object under `root`, with its paging
/// attributes at `@attr` and its rows at `key`. `subject` names the member
/// of `@attr` that says who or what the list is about, where there is one.
pub(super) fn paged<T>(
    raw: &Raw,
    spec: &'static MethodSpec,
    root: &str,
    key: &str,
    subject: Option<&str>,
    parse: impl Fn(Map<String, Value>, &str) -> Result<T, Error>,
) -> Result<Page<T>, Error> {
    decode(raw, spec, root, |mut list| {
        let (paging, rows) = page(&mut list, root, key, subject)?;
        let items = each(rows, &format!("{root}.{key}"), parse)?;
        Ok(Page::new(items, paging))
    })
}

/// A whole list that the service does not page: the object under `root`,
/// with its rows at `key`, which may be none. `subject` names the member of
/// `@attr` that says who or what the list is about, where there is one.
pub(super) fn listed<T>(
    raw: &Raw,
    spec: &'static MethodSpec,
    root: &str,
    key: &str,
    subject: Option<&str>,
    parse: impl Fn(Map<String, Value>, &str) -> Result<T, Error>,
) -> Result<List<T>, Error> {
    decode(raw, spec, root, |mut list| {
        let subject = subject.map(|name| owner(&list, root, name)).transpose()?;
        let path = format!("{root}.{key}");
        let rows = rows(&mut list, &path, key)?;
        Ok(List::new(each(rows, &path, parse)?, subject))
    })
}

/// The paging attributes and the rows of a list: `list.@attr` and
/// `list.<key>`, where `root` is the name of `list` for the error.
pub(super) fn page(
    list: &mut Map<String, Value>,
    root: &str,
    key: &str,
    subject: Option<&str>,
) -> Result<(Paging, Vec<Value>), Error> {
    let paging = paging(list, root, subject)?;
    let rows = promised_rows(list, &paging, &format!("{root}.{key}"), key)?;
    Ok((paging, rows))
}

/// Who or what a list is about: `list.@attr.<key>`, such as its `user` or
/// its `tag`.
pub(super) fn owner(list: &Map<String, Value>, root: &str, key: &str) -> Result<String, Error> {
    let at = format!("{root}.@attr");
    match list.get("@attr") {
        Some(Value::Object(attr)) => Ok(required_str(attr, &at, key)?.to_owned()),
        _ => Err(missing_or(&at, "an object")),
    }
}

/// The paging attributes of a list, at `list.@attr`: the four numbers, which
/// the service sends as strings and which are all required, and the subject
/// where `subject` names its member.
pub(super) fn paging(
    list: &mut Map<String, Value>,
    root: &str,
    subject: Option<&str>,
) -> Result<Paging, Error> {
    let at = format!("{root}.@attr");
    let Some(Value::Object(attr)) = list.remove("@attr") else {
        return Err(missing_or(&at, "an object"));
    };
    Ok(Paging {
        subject: subject
            .map(|name| required_str(&attr, &at, name).map(str::to_owned))
            .transpose()?,
        number: required_u32(&attr, &at, "page")?,
        per_page: required_u32(&attr, &at, "perPage")?,
        total_pages: required_u32(&attr, &at, "totalPages")?,
        total: required_uint(&attr, &at, "total")?,
    })
}

/// The rows of a page, at `list.<key>`, where `path` names them for the
/// error.
///
/// The rows are an array, a single object when there is exactly one, and
/// for an empty page absent, `[]` or `""`. Those three are accepted only
/// when the paging attributes allow no rows: nothing matches (`total` is 0)
/// or the page lies beyond the last one. Otherwise the rows are missing,
/// not absent.
pub(super) fn promised_rows(
    list: &mut Map<String, Value>,
    paging: &Paging,
    path: &str,
    key: &str,
) -> Result<Vec<Value>, Error> {
    let rows = rows(list, path, key)?;
    if rows.is_empty() && paging.total != 0 && paging.number <= paging.total_pages {
        return Err(Error::decode_field(
            path,
            "absent or empty, but `@attr` says the page holds rows",
        ));
    }
    Ok(rows)
}

/// The rows under `key`: an array, a single object, or none when the member
/// is absent, `[]` or `""`. `path` names the member for the error.
pub(super) fn rows(
    list: &mut Map<String, Value>,
    path: &str,
    key: &str,
) -> Result<Vec<Value>, Error> {
    match list.remove(key) {
        None => Ok(Vec::new()),
        Some(Value::String(s)) if s.is_empty() => Ok(Vec::new()),
        Some(value) => de::one_or_many(value).ok_or_else(|| {
            Error::decode_field(path, "expected an array, an object or an empty string")
        }),
    }
}

/// Decodes every row with `parse`, which is given the row, to read and then
/// keep, and its path, such as `topartists.artist[3]`.
pub(super) fn each<T>(
    rows: Vec<Value>,
    path: &str,
    parse: impl Fn(Map<String, Value>, &str) -> Result<T, Error>,
) -> Result<Vec<T>, Error> {
    rows.into_iter()
        .enumerate()
        .map(|(index, row)| {
            let at = format!("{path}[{index}]");
            match row {
                Value::Object(row) => parse(row, &at),
                _ => Err(Error::decode_field(&at, "the row is not an object")),
            }
        })
        .collect()
}

pub(super) fn required_str<'a>(
    map: &'a Map<String, Value>,
    at: &str,
    key: &str,
) -> Result<&'a str, Error> {
    map.get(key)
        .and_then(Value::as_str)
        .ok_or_else(|| missing_or(&format!("{at}.{key}"), "a string"))
}

pub(super) fn required_uint(map: &Map<String, Value>, at: &str, key: &str) -> Result<u64, Error> {
    map.get(key)
        .and_then(de::uint)
        .ok_or_else(|| missing_or(&format!("{at}.{key}"), "an unsigned integer"))
}

/// A count that must fit 32 bits, as a page number and a page size do.
fn required_u32(map: &Map<String, Value>, at: &str, key: &str) -> Result<u32, Error> {
    u32::try_from(required_uint(map, at, key)?)
        .map_err(|_| Error::decode_field(&format!("{at}.{key}"), "too large"))
}

/// A string field that may be missing or empty, both reading as `None`. A
/// value of any other type is an error.
pub(super) fn optional_str(
    map: &Map<String, Value>,
    at: &str,
    key: &str,
) -> Result<Option<String>, Error> {
    match map.get(key) {
        None => Ok(None),
        Some(Value::String(s)) if s.is_empty() => Ok(None),
        Some(Value::String(s)) => Ok(Some(s.clone())),
        Some(_) => Err(Error::decode_field(&format!("{at}.{key}"), "not a string")),
    }
}

/// An unsigned integer that may be missing, which reads as `None`. A value
/// that is present and not one is an error.
pub(super) fn optional_uint(
    map: &Map<String, Value>,
    at: &str,
    key: &str,
) -> Result<Option<u64>, Error> {
    match map.get(key) {
        None => Ok(None),
        Some(value) => de::uint(value)
            .map(Some)
            .ok_or_else(|| Error::decode_field(&format!("{at}.{key}"), "not an unsigned integer")),
    }
}
