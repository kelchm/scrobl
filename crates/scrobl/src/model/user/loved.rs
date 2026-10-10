//! [`LovedTracksPage`], the reply to `user.getLovedTracks`.

use std::fmt;

use serde_json::Value;

use super::{Artist, PageAttr, Track, missing_or, optional_str, parse_artist, parse_attr};
use super::{check_rows_may_be_absent, required_str};
use crate::de;
use crate::error::Error;
use crate::protocol::{Raw, methods};

const ROOT: &str = "lovedtracks";

/// One page of `user.getLovedTracks`: tracks the user has loved, newest
/// first, with the paging attributes.
///
/// The list is what is loved now. A track that was loved and later unloved
/// is not in it, and nothing says it ever was.
///
/// `track` is an array, a single object when the page has exactly one row,
/// and for an empty page absent, `[]` or `""`, under the same rule as
/// [`RecentTracksPage`](super::RecentTracksPage): only when `@attr` allows
/// no rows.
///
/// ```
/// use scrobl::model::LovedTracksPage;
/// use scrobl::protocol::{self, HttpResponse, Request, methods};
///
/// let body = r##"{"lovedtracks": {
///     "track": [
///         {"date": {"uts": "1700000000", "#text": "14 Nov 2023, 22:13"},
///          "artist": {"name": "A", "mbid": "", "url": ""},
///          "name": "Loved", "mbid": "", "url": ""}
///     ],
///     "@attr": {"user": "rj", "page": "1", "perPage": "50",
///               "totalPages": "1", "total": "1"}
/// }}"##;
/// let request = Request::new(&methods::USER_GET_LOVED_TRACKS).param("user", "rj");
/// let raw = protocol::decode(&request, HttpResponse::new(200, body))?;
///
/// let page = LovedTracksPage::decode(&raw)?;
/// assert_eq!(page.attr().total(), 1);
/// assert_eq!(page.tracks()[0].loved_at(), 1_700_000_000);
/// assert_eq!(page.tracks()[0].track().name(), "Loved");
/// assert_eq!(page.tracks()[0].artist().mbid(), None);
/// # Ok::<(), scrobl::Error>(())
/// ```
#[derive(Clone)]
pub struct LovedTracksPage {
    attr: PageAttr,
    tracks: Vec<LovedTrack>,
}

impl LovedTracksPage {
    /// Decodes a `user.getLovedTracks` response.
    ///
    /// # Errors
    ///
    /// [`ErrorKind::Decode`](crate::ErrorKind) when the response is for
    /// another method, when `lovedtracks`, `@attr` or any of its five
    /// members is missing or malformed, when a row is not an object or has
    /// no valid `date.uts`, when `track` is absent or empty although `@attr`
    /// promises rows on the page, when an object repeats a member name, or
    /// when a field of a row has the wrong type. The message names the
    /// field, such as `lovedtracks.track[3].date.uts`, and never repeats
    /// response text.
    pub fn decode(raw: &Raw) -> Result<Self, Error> {
        if raw.method() != methods::USER_GET_LOVED_TRACKS.name() {
            return Err(Error::decode("the response is for a different method")
                .with_method(raw.spec())
                .with_response(raw.status(), raw.body()));
        }
        let root = raw.json_strict()?;
        parse(root).map_err(|e| {
            e.with_method(raw.spec())
                .with_response(raw.status(), raw.body())
        })
    }

    /// The paging attributes.
    pub fn attr(&self) -> &PageAttr {
        &self.attr
    }

    /// The loved tracks, in the order the service sent them.
    pub fn tracks(&self) -> &[LovedTrack] {
        &self.tracks
    }
}

impl fmt::Debug for LovedTracksPage {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("LovedTracksPage")
            .field("attr", &self.attr)
            .field("tracks", &self.tracks.len())
            .finish()
    }
}

/// A track the user has loved, and when.
///
/// The service names no album here. Two loved tracks can share a second,
/// so the time is not an identity. [`json`](Self::json) is the row exactly
/// as decoded.
#[derive(Clone)]
pub struct LovedTrack {
    loved_at: u64,
    track: Track,
    artist: Artist,
    json: Value,
}

impl LovedTrack {
    /// When the track was loved, in seconds since the Unix epoch, from
    /// `date.uts`.
    pub fn loved_at(&self) -> u64 {
        self.loved_at
    }

    /// The track.
    pub fn track(&self) -> &Track {
        &self.track
    }

    /// The artist.
    pub fn artist(&self) -> &Artist {
        &self.artist
    }

    /// The row exactly as decoded.
    pub fn json(&self) -> &Value {
        &self.json
    }
}

impl fmt::Debug for LovedTrack {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("LovedTrack")
            .field("loved_at", &self.loved_at)
            .finish_non_exhaustive()
    }
}

fn parse(root: Value) -> Result<LovedTracksPage, Error> {
    let Value::Object(mut root) = root else {
        return Err(Error::decode("the body is not a JSON object"));
    };
    let Some(Value::Object(mut loved)) = root.remove(ROOT) else {
        return Err(missing_or(ROOT, "an object"));
    };
    let attr = match loved.remove("@attr") {
        Some(Value::Object(attr)) => parse_attr(ROOT, &attr)?,
        _ => return Err(missing_or("lovedtracks.@attr", "an object")),
    };

    let rows = match loved.remove("track") {
        None => Vec::new(),
        Some(Value::String(s)) if s.is_empty() => Vec::new(),
        Some(value) => de::one_or_many(value).ok_or_else(|| {
            Error::decode_field(
                "lovedtracks.track",
                "expected an array, an object or an empty string",
            )
        })?,
    };
    if rows.is_empty() {
        check_rows_may_be_absent(ROOT, &attr)?;
    }

    let tracks = rows
        .into_iter()
        .enumerate()
        .map(|(index, row)| parse_row(index, row))
        .collect::<Result<_, _>>()?;
    Ok(LovedTracksPage { attr, tracks })
}

fn parse_row(index: usize, json: Value) -> Result<LovedTrack, Error> {
    let at = format!("lovedtracks.track[{index}]");
    let Value::Object(map) = &json else {
        return Err(Error::decode_field(&at, "the row is not an object"));
    };
    let track = Track {
        name: required_str(map, &at, "name")?.to_owned(),
        mbid: optional_str(map, &at, "mbid")?,
        url: optional_str(map, &at, "url")?,
    };
    let artist = parse_artist(map, &at)?;
    let path = format!("{at}.date.uts");
    let loved_at = map
        .get("date")
        .and_then(|date| date.get("uts"))
        .and_then(de::uint)
        .ok_or_else(|| missing_or(&path, "an unsigned integer"))?;
    Ok(LovedTrack {
        loved_at,
        track,
        artist,
        json,
    })
}

#[cfg(test)]
mod tests;
