//! What the packages share: the paging attributes, a track, an artist and an
//! album as a list names them, and the decoding every list goes through.

use std::fmt;

use serde_json::{Map, Value};

use crate::de;
use crate::error::Error;
use crate::protocol::{MethodSpec, Raw};

/// The `@attr` of a page: whose list, which page of how many.
///
/// The service sends these as strings. All five are required.
#[derive(Clone)]
pub struct PageAttr {
    user: String,
    page: u64,
    per_page: u64,
    total_pages: u64,
    total: u64,
}

impl PageAttr {
    /// The user the service says the page is for, in its own spelling.
    pub fn user(&self) -> &str {
        &self.user
    }

    /// The page number, from 1.
    pub fn page(&self) -> u64 {
        self.page
    }

    /// The page size the service applied.
    pub fn per_page(&self) -> u64 {
        self.per_page
    }

    /// The number of pages the service says there are.
    pub fn total_pages(&self) -> u64 {
        self.total_pages
    }

    /// The number of items the service says match the request: for a
    /// history, the scrobbles within the `from` and `to` bounds if any were
    /// sent.
    pub fn total(&self) -> u64 {
        self.total
    }
}

impl fmt::Debug for PageAttr {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("PageAttr")
            .field("page", &self.page)
            .field("per_page", &self.per_page)
            .field("total_pages", &self.total_pages)
            .field("total", &self.total)
            .finish_non_exhaustive()
    }
}

/// A track, as named in a row.
///
/// `mbid` and `url` are `None` when the field is missing or an empty string;
/// the row's own `json` tells the two apart.
#[derive(Clone)]
pub struct Track {
    name: String,
    mbid: Option<String>,
    url: Option<String>,
}

impl Track {
    /// The track name. May be empty.
    pub fn name(&self) -> &str {
        &self.name
    }

    /// The MusicBrainz id, if the service sent a non-empty one.
    pub fn mbid(&self) -> Option<&str> {
        self.mbid.as_deref()
    }

    /// The Last.fm page of the track, if the service sent a non-empty one.
    pub fn url(&self) -> Option<&str> {
        self.url.as_deref()
    }
}

impl fmt::Debug for Track {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Track").finish_non_exhaustive()
    }
}

/// An artist, as named in a row.
///
/// The name is `name` where the row gives the artist's details and `#text`
/// where it gives only a name and an id. A row that carries both must carry
/// the same string twice. `mbid` and `url` are `None` when the field is
/// missing or an empty string, and `url` is not sent in the `#text` shape.
#[derive(Clone)]
pub struct Artist {
    name: String,
    mbid: Option<String>,
    url: Option<String>,
}

impl Artist {
    /// The artist name. May be empty.
    pub fn name(&self) -> &str {
        &self.name
    }

    /// The MusicBrainz id, if the service sent a non-empty one.
    pub fn mbid(&self) -> Option<&str> {
        self.mbid.as_deref()
    }

    /// The Last.fm page of the artist, if the service sent a non-empty one.
    pub fn url(&self) -> Option<&str> {
        self.url.as_deref()
    }
}

impl fmt::Debug for Artist {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Artist").finish_non_exhaustive()
    }
}

/// An album, as named in a row. A row without an album decodes to an album
/// with no title, no mbid and no url.
///
/// Every field is `None` when missing or an empty string, but where the
/// row is itself an album, as in a list of top albums, a missing name is an
/// error. `url` is sent only in such a row.
#[derive(Clone)]
pub struct Album {
    title: Option<String>,
    mbid: Option<String>,
    url: Option<String>,
}

impl Album {
    /// The album title, if the service sent a non-empty one.
    pub fn title(&self) -> Option<&str> {
        self.title.as_deref()
    }

    /// The MusicBrainz id, if the service sent a non-empty one.
    pub fn mbid(&self) -> Option<&str> {
        self.mbid.as_deref()
    }

    /// The Last.fm page of the album, if the service sent a non-empty one.
    pub fn url(&self) -> Option<&str> {
        self.url.as_deref()
    }
}

impl fmt::Debug for Album {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Album").finish_non_exhaustive()
    }
}

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

/// The paging attributes and the rows of a list: `list.@attr` and
/// `list.<key>`, where `root` is the name of `list` for the error.
pub(super) fn page(
    list: &mut Map<String, Value>,
    root: &str,
    key: &str,
) -> Result<(PageAttr, Vec<Value>), Error> {
    let attr = page_attr(list, root)?;
    let rows = promised_rows(list, &attr, &format!("{root}.{key}"), key)?;
    Ok((attr, rows))
}

/// The paging attributes of a list, at `list.@attr`.
pub(super) fn page_attr(list: &mut Map<String, Value>, root: &str) -> Result<PageAttr, Error> {
    match list.remove("@attr") {
        Some(Value::Object(attr)) => parse_attr(root, &attr),
        _ => Err(missing_or(&format!("{root}.@attr"), "an object")),
    }
}

/// The rows of a page, at `list.<key>`, where `path` names them for the
/// error.
///
/// The rows are an array, a single object when there is exactly one, and
/// for an empty page absent, `[]` or `""`. Those three are accepted only
/// when `attr` allows no rows: nothing matches (`total` is 0) or the page
/// lies beyond the last one. Otherwise the rows are missing, not absent.
pub(super) fn promised_rows(
    list: &mut Map<String, Value>,
    attr: &PageAttr,
    path: &str,
    key: &str,
) -> Result<Vec<Value>, Error> {
    let rows = rows(list, path, key)?;
    if rows.is_empty() && attr.total != 0 && attr.page <= attr.total_pages {
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

fn parse_attr(root: &str, attr: &Map<String, Value>) -> Result<PageAttr, Error> {
    let at = format!("{root}.@attr");
    Ok(PageAttr {
        user: required_str(attr, &at, "user")?.to_owned(),
        page: required_uint(attr, &at, "page")?,
        per_page: required_uint(attr, &at, "perPage")?,
        total_pages: required_uint(attr, &at, "totalPages")?,
        total: required_uint(attr, &at, "total")?,
    })
}

/// The track a row is: its own `name`, `mbid` and `url`.
pub(super) fn parse_track(row: &Map<String, Value>, at: &str) -> Result<Track, Error> {
    Ok(Track {
        name: required_str(row, at, "name")?.to_owned(),
        mbid: optional_str(row, at, "mbid")?,
        url: optional_str(row, at, "url")?,
    })
}

/// The artist a row is: its own `name`, `mbid` and `url`.
pub(super) fn parse_artist_row(row: &Map<String, Value>, at: &str) -> Result<Artist, Error> {
    Ok(Artist {
        name: required_str(row, at, "name")?.to_owned(),
        mbid: optional_str(row, at, "mbid")?,
        url: optional_str(row, at, "url")?,
    })
}

/// The album a row is: its own `name`, `mbid` and `url`.
pub(super) fn parse_album_row(row: &Map<String, Value>, at: &str) -> Result<Album, Error> {
    let name = required_str(row, at, "name")?;
    Ok(Album {
        title: (!name.is_empty()).then(|| name.to_owned()),
        mbid: optional_str(row, at, "mbid")?,
        url: optional_str(row, at, "url")?,
    })
}

/// The artist a row names, under `artist`.
pub(super) fn parse_artist(row: &Map<String, Value>, at: &str) -> Result<Artist, Error> {
    let path = format!("{at}.artist");
    let Some(Value::Object(artist)) = row.get("artist") else {
        return Err(missing_or(&path, "an object"));
    };
    // `name` where the row gives the artist's details, `#text` where it
    // gives a name and an id. Each one present must be a string, and two
    // that are present must agree.
    let name_in = |key: &str| match artist.get(key) {
        None => Ok(None),
        Some(Value::String(name)) => Ok(Some(name)),
        Some(_) => Err(Error::decode_field(
            &format!("{path}.{key}"),
            "not a string",
        )),
    };
    let name = match (name_in("name")?, name_in("#text")?) {
        (Some(name), Some(text)) if name != text => {
            return Err(Error::decode_field(
                &path,
                "`name` and `#text` are both present and disagree",
            ));
        }
        (Some(name), _) | (None, Some(name)) => name.clone(),
        (None, None) => {
            return Err(Error::decode_field(
                &format!("{path}.name"),
                "neither `name` nor `#text` is present",
            ));
        }
    };
    Ok(Artist {
        name,
        mbid: optional_str(artist, &path, "mbid")?,
        url: optional_str(artist, &path, "url")?,
    })
}

/// The album a row names, under `album`. A row without one names an album
/// with nothing in it.
pub(super) fn parse_album(row: &Map<String, Value>, at: &str) -> Result<Album, Error> {
    let path = format!("{at}.album");
    match row.get("album") {
        None => Ok(Album {
            title: None,
            mbid: None,
            url: None,
        }),
        Some(Value::Object(album)) => Ok(Album {
            title: optional_str(album, &path, "#text")?,
            mbid: optional_str(album, &path, "mbid")?,
            url: None,
        }),
        Some(_) => Err(Error::decode_field(&path, "not an object")),
    }
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
