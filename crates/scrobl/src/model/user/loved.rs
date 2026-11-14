//! The reply to `user.getLovedTracks`.

use serde_json::{Map, Value};

use crate::de;
use crate::error::Error;
use crate::model::Page;
use crate::model::entity::{self, Counts, Length, Track, counted_row};
use crate::model::shared::{self, missing_or};
use crate::protocol::{Raw, methods};

counted_row! {
    /// A track a user has loved, with when they loved it.
    LovedTrack {
        /// When the user loved the track, in seconds since the Unix epoch,
        /// from `date.uts`.
        loved_at,
        track / into_track: Track
    }
}

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
/// assert_eq!(page[0].track().artist().mbid(), None);
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

fn parse_row(row: Map<String, Value>, at: &str) -> Result<LovedTrack, Error> {
    // The track and its artist are held to their shape first, then the
    // date: a row that is wrong twice names the track.
    let loved_at = row
        .get("date")
        .and_then(|date| date.get("uts"))
        .and_then(de::uint)
        .ok_or_else(|| missing_or(&format!("{at}.date.uts"), "an unsigned integer"));
    let track = entity::track(row, at, Counts::Neither, Length::Read)?;
    Ok(LovedTrack {
        loved_at: loved_at?,
        track,
    })
}

#[cfg(test)]
mod tests;
