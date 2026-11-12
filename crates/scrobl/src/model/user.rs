//! The `user` package: [`RecentTracksPage`], the reply to
//! `user.getRecentTracks`, here, and the replies to its other reads in the
//! modules below.

mod info;
mod loved;
mod tagged;
mod top;
mod weekly;

pub use info::{FriendsPage, UserInfo};
pub use loved::{LovedTrack, LovedTracksPage};
pub use tagged::{
    TaggedAlbum, TaggedAlbumsPage, TaggedArtist, TaggedArtistsPage, TaggedTrack, TaggedTracksPage,
};
pub use top::{
    TopAlbum, TopAlbumsPage, TopArtist, TopArtistsPage, TopTag, TopTags, TopTrack, TopTracksPage,
};
pub use weekly::{
    WeeklyAlbumChart, WeeklyArtistChart, WeeklyChartList, WeeklyChartRange, WeeklyTrackChart,
};

use std::fmt;

use serde_json::{Map, Value};

use super::shared::{self, Album, Artist, PageAttr, Track, missing_or};
use crate::de;
use crate::error::Error;
use crate::protocol::{Raw, methods};

const ROOT: &str = "recenttracks";

/// One page of `user.getRecentTracks`.
///
/// A page holds the historical rows, the optional now-playing row and the
/// paging attributes. Decode it with [`decode`](Self::decode).
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
/// use scrobl::model::RecentTracksPage;
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
/// let page = RecentTracksPage::decode(&raw)?;
/// assert_eq!(page.attr().total(), 1);
/// assert_eq!(page.scrobbles().len(), 1);
/// assert_eq!(page.scrobbles()[0].timestamp(), 1_700_000_000);
/// assert_eq!(page.scrobbles()[0].album().title(), Some("Album"));
/// assert_eq!(page.now_playing().map(|n| n.track().name()), Some("Playing"));
/// # Ok::<(), scrobl::Error>(())
/// ```
#[derive(Clone)]
pub struct RecentTracksPage {
    user: String,
    attr: PageAttr,
    scrobbles: Vec<Scrobble>,
    now_playing: Option<NowPlaying>,
}

impl RecentTracksPage {
    /// Decodes a `user.getRecentTracks` response.
    ///
    /// # Errors
    ///
    /// [`ErrorKind::Decode`](crate::ErrorKind) when the response is for
    /// another method, when `recenttracks`, `@attr` or any of its five
    /// members is missing or malformed, when a row is not an object, when a
    /// row has neither the now-playing flag nor a valid `date.uts`, when more
    /// than one row is flagged now-playing, when `track` is absent or empty
    /// although `@attr` promises rows on the page, when an object repeats a
    /// member name, when `artist` has two names that differ, or when a field
    /// of a row has the wrong type. The message names the field, such as
    /// `recenttracks.@attr.total` or `recenttracks.track[3].date.uts`, and
    /// never repeats response text.
    pub fn decode(raw: &Raw) -> Result<Self, Error> {
        shared::decode(raw, &methods::USER_GET_RECENT_TRACKS, ROOT, parse)
    }

    /// The user the service says the page is for, in its own spelling.
    pub fn user(&self) -> &str {
        &self.user
    }

    /// The paging attributes.
    pub fn attr(&self) -> &PageAttr {
        &self.attr
    }

    /// The historical rows, in the order the service sent them. The
    /// now-playing row is never among them.
    pub fn scrobbles(&self) -> &[Scrobble] {
        &self.scrobbles
    }

    /// The row flagged now-playing, if the page has one.
    pub fn now_playing(&self) -> Option<&NowPlaying> {
        self.now_playing.as_ref()
    }
}

impl fmt::Debug for RecentTracksPage {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("RecentTracksPage")
            .field("attr", &self.attr)
            .field("scrobbles", &self.scrobbles.len())
            .field("now_playing", &self.now_playing.is_some())
            .finish()
    }
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

fn parse(mut recent: Map<String, Value>) -> Result<RecentTracksPage, Error> {
    let user = shared::owner(&recent, ROOT, "user")?;
    let (attr, rows) = shared::page(&mut recent, ROOT, "track")?;

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
    Ok(RecentTracksPage {
        user,
        attr,
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
