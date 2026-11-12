//! The reply to `user.getLovedTracks`.

use std::fmt;

use serde_json::{Map, Value};

use crate::de;
use crate::error::Error;
use crate::model::Page;
use crate::model::shared::{self, Artist, Track, missing_or};
use crate::protocol::{Raw, methods};

/// Decodes a `user.getLovedTracks` response: one page of the tracks a user
/// has loved, newest first. The page's subject is the user.
///
/// The list is what is loved now. A track that was loved and later unloved
/// is not in it, and nothing says it ever was.
///
/// ```
/// use scrobl::model::decode;
/// use scrobl::protocol::{self, HttpResponse, Request, methods};
///
/// let body = r##"{"lovedtracks": {
///     "track": [{"name": "Loved", "mbid": "", "url": "https://www.last.fm/music/A/_/Loved",
///                "date": {"uts": "1700000000", "#text": "14 Nov 2023, 22:13"},
///                "artist": {"name": "A", "mbid": "", "url": "https://www.last.fm/music/A"}}],
///     "@attr": {"user": "rj", "page": "1", "perPage": "50", "totalPages": "1", "total": "1"}
/// }}"##;
/// let request = Request::new(&methods::USER_GET_LOVED_TRACKS).param("user", "rj");
/// let raw = protocol::decode(&request, HttpResponse::new(200, body))?;
///
/// let page = decode::user_get_loved_tracks(&raw)?;
/// assert_eq!(page.total(), 1);
/// assert_eq!(page[0].loved_at(), 1_700_000_000);
/// assert_eq!(page[0].track().name(), "Loved");
/// assert_eq!(page[0].artist().mbid(), None);
/// # Ok::<(), scrobl::Error>(())
/// ```
///
/// # Errors
///
/// [`ErrorKind::Decode`](crate::ErrorKind) under [the rules every list
/// shares](crate::model::decode#errors). `@attr.user` is required, and a row
/// needs a `name`, an `artist` and a `date.uts`.
pub fn user_get_loved_tracks(raw: &Raw) -> Result<Page<LovedTrack>, Error> {
    shared::paged(
        raw,
        &methods::USER_GET_LOVED_TRACKS,
        "lovedtracks",
        "track",
        Some("user"),
        parse_row,
    )
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

fn parse_row(row: Map<String, Value>, at: &str) -> Result<LovedTrack, Error> {
    let track = shared::parse_track(&row, at)?;
    let artist = shared::parse_artist(&row, at)?;
    let path = format!("{at}.date.uts");
    let loved_at = row
        .get("date")
        .and_then(|date| date.get("uts"))
        .and_then(de::uint)
        .ok_or_else(|| missing_or(&path, "an unsigned integer"))?;
    Ok(LovedTrack {
        loved_at,
        track,
        artist,
        json: Value::Object(row),
    })
}

#[cfg(test)]
mod tests;
