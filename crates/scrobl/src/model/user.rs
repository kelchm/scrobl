//! The `user` package: a page of the history, the reply to
//! `user.getRecentTracks`, here, and the replies to its other reads in the
//! modules below.

mod info;
mod loved;
mod tagged;
mod top;
mod weekly;

pub use info::{UserInfo, user_get_friends};
pub use loved::{LovedTrack, user_get_loved_tracks};
pub use tagged::{
    TaggedAlbum, TaggedArtist, TaggedTrack, user_get_personal_tags_albums,
    user_get_personal_tags_artists, user_get_personal_tags_tracks,
};
pub use top::{
    TopAlbum, TopArtist, TopTag, TopTrack, user_get_top_albums, user_get_top_artists,
    user_get_top_tags, user_get_top_tracks,
};
pub(super) use weekly::chart_range;
pub use weekly::{
    WeeklyAlbumChart, WeeklyArtistChart, WeeklyChartRange, WeeklyTrackChart,
    user_get_weekly_chart_list,
};

use std::fmt;

use serde_json::{Map, Value};

use super::page::Paging;
use super::shared::{self, Album, Artist, Track, missing_or};
use crate::de;
use crate::error::Error;
use crate::protocol::{Raw, methods};

const ROOT: &str = "recenttracks";

/// One page of a user's history: the reply to `user.getRecentTracks`.
///
/// A page holds the scrobbles, the track playing now if there is one, and
/// what a [`Page`](super::Page) says about its place in the list. Decode it
/// with [`user_get_recent_tracks`].
///
/// # Shapes
///
/// The same method answers in several shapes, and all of them decode:
///
/// - `artist` is `{"#text": .., "mbid": ..}` normally and `{"name": ..,
///   "mbid": .., "url": .., "image": [..]}` with `extended=1`.
/// - `track` is an array, a single object when the page has exactly one row,
///   and for an empty page absent, `[]` or `""`. Those three are accepted
///   only when the page's own `@attr` allows no rows: `total` is 0 or `page`
///   is beyond `totalPages`. Anything else, including `null`, a non-empty
///   string and a missing or misspelled `track` on a page that promises
///   rows, is an error.
/// - A row whose `@attr.nowplaying` is `"true"` or `"1"` is the now-playing
///   row, whether or not it also carries a `date`. `"false"` and `"0"` are
///   also read, as an ordinary historical row. Every row that is not
///   now-playing must have a valid `date.uts`.
/// - `artist` may carry `name`, `#text` or both. Each one present must be a
///   string, and two that are present must be equal.
/// - An object that repeats a member name, at any depth, is an error: a map
///   would keep one of the two and drop the other without a word.
///
/// ```
/// use scrobl::model::decode;
/// use scrobl::protocol::{self, HttpResponse, Request, methods};
///
/// let body = r##"{"recenttracks": {
///     "track": [
///         {"@attr": {"nowplaying": "true"},
///          "artist": {"#text": "A", "mbid": ""}, "name": "Playing", "mbid": "",
///          "album": {"#text": "", "mbid": ""}},
///         {"date": {"uts": "1700000000", "#text": "14 Nov 2023, 22:13"},
///          "artist": {"#text": "A", "mbid": ""}, "name": "Played", "mbid": "",
///          "album": {"#text": "Album", "mbid": ""}}
///     ],
///     "@attr": {"user": "rj", "page": "1", "perPage": "50",
///               "totalPages": "1", "total": "1"}
/// }}"##;
/// let request = Request::new(&methods::USER_GET_RECENT_TRACKS).param("user", "rj");
/// let raw = protocol::decode(&request, HttpResponse::new(200, body))?;
///
/// let page = decode::user_get_recent_tracks(&raw)?;
/// assert_eq!(page.total(), 1);
/// assert_eq!(page.scrobbles().len(), 1);
/// assert_eq!(page.scrobbles()[0].timestamp(), 1_700_000_000);
/// assert_eq!(page.scrobbles()[0].album().title(), Some("Album"));
/// assert_eq!(page.now_playing().map(|n| n.track().name()), Some("Playing"));
/// # Ok::<(), scrobl::Error>(())
/// ```
#[derive(Clone)]
pub struct HistoryPage {
    paging: Paging,
    scrobbles: Vec<Scrobble>,
    now_playing: Option<NowPlaying>,
}

impl HistoryPage {
    /// The scrobbles, in the order the service sent them, newest first. The
    /// now-playing row is never among them.
    pub fn scrobbles(&self) -> &[Scrobble] {
        &self.scrobbles
    }

    /// The scrobbles, to keep.
    pub fn into_scrobbles(self) -> Vec<Scrobble> {
        self.scrobbles
    }

    /// The row flagged now-playing, if the page has one.
    pub fn now_playing(&self) -> Option<&NowPlaying> {
        self.now_playing.as_ref()
    }

    /// Which page this is, from 1.
    pub fn number(&self) -> u32 {
        self.paging.number
    }

    /// The page size the service applied.
    pub fn per_page(&self) -> u32 {
        self.paging.per_page
    }

    /// The number of pages the service says there are.
    pub fn total_pages(&self) -> u32 {
        self.paging.total_pages
    }

    /// The number of scrobbles the service says match the request: those
    /// within the `from` and `to` bounds, if any were sent.
    pub fn total(&self) -> u64 {
        self.paging.total
    }

    /// The number of the page after this one. `None` on the last page and
    /// on any page beyond it.
    pub fn next_page(&self) -> Option<u32> {
        self.paging.next_page()
    }

    /// The user the service says the page is for, in its own spelling.
    pub fn subject(&self) -> Option<&str> {
        self.paging.subject.as_deref()
    }
}

impl fmt::Debug for HistoryPage {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("HistoryPage")
            .field("paging", &self.paging)
            .field("scrobbles", &self.scrobbles.len())
            .field("now_playing", &self.now_playing.is_some())
            .finish()
    }
}

/// Decodes a `user.getRecentTracks` response.
///
/// # Errors
///
/// [`ErrorKind::Decode`](crate::ErrorKind) under [the rules every list
/// shares](super::decode#errors), and when a row has neither the now-playing
/// flag nor a valid `date.uts`, when more than one row is flagged
/// now-playing, or when `artist` has two names that differ. The message
/// names the field, such as `recenttracks.@attr.total` or
/// `recenttracks.track[3].date.uts`.
pub fn user_get_recent_tracks(raw: &Raw) -> Result<HistoryPage, Error> {
    shared::decode(raw, &methods::USER_GET_RECENT_TRACKS, ROOT, parse)
}

/// One historical row: a scrobble with its timestamp.
///
/// Rows are not unique. The same track can be scrobbled twice in a second,
/// and a timestamp is not an identity. [`json`](Self::json) is the row
/// exactly as decoded, so unknown fields and the difference between a
/// missing and an empty field stay reachable.
#[derive(Clone)]
pub struct Scrobble {
    timestamp: u64,
    track: Track,
    artist: Artist,
    album: Album,
    loved: Option<bool>,
    json: Value,
}

impl Scrobble {
    /// Seconds since the Unix epoch, from `date.uts`.
    pub fn timestamp(&self) -> u64 {
        self.timestamp
    }

    /// The track.
    pub fn track(&self) -> &Track {
        &self.track
    }

    /// The artist.
    pub fn artist(&self) -> &Artist {
        &self.artist
    }

    /// The album.
    pub fn album(&self) -> &Album {
        &self.album
    }

    /// Whether the user loved the track, as the row returned it. The field is
    /// present only when the page was requested with `extended=1`; otherwise
    /// this is `None`, which is not the same as `Some(false)`.
    pub fn loved(&self) -> Option<bool> {
        self.loved
    }

    /// The row exactly as decoded.
    pub fn json(&self) -> &Value {
        &self.json
    }
}

impl fmt::Debug for Scrobble {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Scrobble")
            .field("timestamp", &self.timestamp)
            .field("loved", &self.loved)
            .finish_non_exhaustive()
    }
}

/// The row flagged now-playing: what the user is listening to, not yet a
/// scrobble.
///
/// It may carry a `date`, which is not exposed here and is never treated as
/// a scrobble time; it is still in [`json`](Self::json). A row whose
/// `@attr.nowplaying` flag is false is not this: it is an ordinary
/// [`Scrobble`].
#[derive(Clone)]
pub struct NowPlaying {
    track: Track,
    artist: Artist,
    album: Album,
    loved: Option<bool>,
    json: Value,
}

impl NowPlaying {
    /// The track.
    pub fn track(&self) -> &Track {
        &self.track
    }

    /// The artist.
    pub fn artist(&self) -> &Artist {
        &self.artist
    }

    /// The album.
    pub fn album(&self) -> &Album {
        &self.album
    }

    /// Whether the user loved the track, as the row returned it. The field is
    /// present only when the page was requested with `extended=1`; otherwise
    /// this is `None`, which is not the same as `Some(false)`.
    pub fn loved(&self) -> Option<bool> {
        self.loved
    }

    /// The row exactly as decoded.
    pub fn json(&self) -> &Value {
        &self.json
    }
}

impl fmt::Debug for NowPlaying {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("NowPlaying")
            .field("loved", &self.loved)
            .finish_non_exhaustive()
    }
}

fn parse(mut recent: Map<String, Value>) -> Result<HistoryPage, Error> {
    let (paging, rows) = shared::page(&mut recent, ROOT, "track", Some("user"))?;

    let mut scrobbles = Vec::with_capacity(rows.len());
    let mut now_playing = None;
    for (index, row) in rows.into_iter().enumerate() {
        match parse_row(index, row)? {
            Row::Scrobble(scrobble) => scrobbles.push(scrobble),
            Row::NowPlaying(row) => {
                if now_playing.replace(row).is_some() {
                    return Err(Error::decode_field(
                        "recenttracks.track",
                        "more than one row is flagged now-playing",
                    ));
                }
            }
        }
    }
    Ok(HistoryPage {
        paging,
        scrobbles,
        now_playing,
    })
}

enum Row {
    Scrobble(Scrobble),
    NowPlaying(NowPlaying),
}

fn parse_row(index: usize, json: Value) -> Result<Row, Error> {
    let at = format!("recenttracks.track[{index}]");
    let Value::Object(map) = &json else {
        return Err(Error::decode_field(&at, "the row is not an object"));
    };

    let flagged = match map.get("@attr") {
        None => false,
        Some(Value::Object(attr)) => match attr.get("nowplaying") {
            None => false,
            Some(value) => de::flag_or_word(value).ok_or_else(|| {
                Error::decode_field(&format!("{at}.@attr.nowplaying"), "not a recognised flag")
            })?,
        },
        Some(_) => return Err(Error::decode_field(&format!("{at}.@attr"), "not an object")),
    };

    let track = shared::parse_track(map, &at)?;
    let artist = shared::parse_artist(map, &at)?;
    let album = shared::parse_album(map, &at)?;
    let loved = match map.get("loved") {
        None => None,
        Some(value) => Some(
            de::flag(value)
                .ok_or_else(|| Error::decode_field(&format!("{at}.loved"), "not a 0/1 flag"))?,
        ),
    };

    if flagged {
        return Ok(Row::NowPlaying(NowPlaying {
            track,
            artist,
            album,
            loved,
            json,
        }));
    }

    let path = format!("{at}.date.uts");
    let timestamp = map
        .get("date")
        .and_then(|date| date.get("uts"))
        .and_then(de::uint)
        .ok_or_else(|| missing_or(&path, "an unsigned integer"))?;
    Ok(Row::Scrobble(Scrobble {
        timestamp,
        track,
        artist,
        album,
        loved,
        json,
    }))
}

#[cfg(test)]
mod tests;
