//! [`LovedTracksPage`], the reply to `user.getLovedTracks`.

use std::fmt;

use serde_json::{Map, Value};

use crate::de;
use crate::error::Error;
use crate::model::shared::{self, Artist, PageAttr, Track, missing_or};
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
        shared::decode(raw, &methods::USER_GET_LOVED_TRACKS, ROOT, parse)
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

fn parse(mut loved: Map<String, Value>) -> Result<LovedTracksPage, Error> {
    let (attr, rows) = shared::page(&mut loved, ROOT, "track")?;
    let tracks = shared::each(rows, "lovedtracks.track", parse_row)?;
    Ok(LovedTracksPage { attr, tracks })
}

fn parse_row(row: Map<String, Value>, at: &str) -> Result<LovedTrack, Error> {
    let path = format!("{at}.date.uts");
    let loved_at = row
        .get("date")
        .and_then(|date| date.get("uts"))
        .and_then(de::uint)
        .ok_or_else(|| missing_or(&path, "an unsigned integer"))?;
    Ok(LovedTrack {
        loved_at,
        track: shared::parse_track(&row, at)?,
        artist: shared::parse_artist(&row, at)?,
        json: Value::Object(row),
    })
}

#[cfg(test)]
mod tests;
