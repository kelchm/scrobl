//! What a user plays and tags most: the replies to `user.getTopArtists`,
//! `user.getTopAlbums`, `user.getTopTracks` and `user.getTopTags`.

use std::fmt;

use serde_json::{Map, Value};

use crate::de;
use crate::error::Error;
use crate::model::shared::{self, Album, Artist, Track, missing_or};
use crate::model::{List, Page};
use crate::protocol::{Raw, methods};

/// Decodes a `user.getTopArtists` response: one page of the artists a user
/// played most in a period. The page's subject is the user.
///
/// ```
/// use scrobl::model::decode;
/// use scrobl::protocol::{self, HttpResponse, Request, methods};
///
/// let body = r##"{"topartists": {
///     "artist": [
///         {"name": "A", "mbid": "", "url": "https://www.last.fm/music/A",
///          "playcount": "42", "@attr": {"rank": "1"}}
///     ],
///     "@attr": {"user": "rj", "page": "1", "perPage": "50",
///               "totalPages": "1", "total": "1"}
/// }}"##;
/// let request = Request::new(&methods::USER_GET_TOP_ARTISTS).param("user", "rj");
/// let raw = protocol::decode(&request, HttpResponse::new(200, body))?;
///
/// let page = decode::user_get_top_artists(&raw)?;
/// let top = &page[0];
/// assert_eq!((top.rank(), top.playcount()), (1, 42));
/// assert_eq!(top.artist().name(), "A");
/// assert_eq!(top.artist().mbid(), None);
/// # Ok::<(), scrobl::Error>(())
/// ```
///
/// # Errors
///
/// [`ErrorKind::Decode`](crate::ErrorKind) under [the rules every list
/// shares](crate::model::decode#errors). `@attr.user` is required, and a row
/// needs a `name`, a `playcount` and an `@attr.rank`.
pub fn user_get_top_artists(raw: &Raw) -> Result<Page<TopArtist>, Error> {
    shared::paged(
        raw,
        &methods::USER_GET_TOP_ARTISTS,
        "topartists",
        "artist",
        Some("user"),
        top_artist,
    )
}

/// An artist in a list of a user's most played, with its place and how
/// often the user played it. [`json`](Self::json) is the row exactly as
/// decoded.
#[derive(Clone)]
pub struct TopArtist {
    rank: u64,
    playcount: u64,
    artist: Artist,
    json: Value,
}

impl TopArtist {
    /// The place in the whole list, from 1, whatever page the row is on.
    pub fn rank(&self) -> u64 {
        self.rank
    }

    /// How many times the user played the artist in the period.
    pub fn playcount(&self) -> u64 {
        self.playcount
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

impl fmt::Debug for TopArtist {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("TopArtist")
            .field("rank", &self.rank)
            .field("playcount", &self.playcount)
            .finish_non_exhaustive()
    }
}

/// Decodes a `user.getTopAlbums` response: one page of the albums a user
/// played most in a period. The page's subject is the user.
///
/// # Errors
///
/// [`ErrorKind::Decode`](crate::ErrorKind) under [the rules every list
/// shares](crate::model::decode#errors). `@attr.user` is required, and a row
/// needs a `name`, an `artist`, a `playcount` and an `@attr.rank`.
pub fn user_get_top_albums(raw: &Raw) -> Result<Page<TopAlbum>, Error> {
    shared::paged(
        raw,
        &methods::USER_GET_TOP_ALBUMS,
        "topalbums",
        "album",
        Some("user"),
        top_album,
    )
}

/// An album in a list of a user's most played, with its place and how often
/// the user played it. [`json`](Self::json) is the row exactly as decoded.
#[derive(Clone)]
pub struct TopAlbum {
    rank: u64,
    playcount: u64,
    album: Album,
    artist: Artist,
    json: Value,
}

impl TopAlbum {
    /// The place in the whole list, from 1, whatever page the row is on.
    pub fn rank(&self) -> u64 {
        self.rank
    }

    /// How many times the user played a track of the album in the period.
    pub fn playcount(&self) -> u64 {
        self.playcount
    }

    /// The album.
    pub fn album(&self) -> &Album {
        &self.album
    }

    /// The album's artist.
    pub fn artist(&self) -> &Artist {
        &self.artist
    }

    /// The row exactly as decoded.
    pub fn json(&self) -> &Value {
        &self.json
    }
}

impl fmt::Debug for TopAlbum {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("TopAlbum")
            .field("rank", &self.rank)
            .field("playcount", &self.playcount)
            .finish_non_exhaustive()
    }
}

/// Decodes a `user.getTopTracks` response: one page of the tracks a user
/// played most in a period. The page's subject is the user.
///
/// # Errors
///
/// [`ErrorKind::Decode`](crate::ErrorKind) under [the rules every list
/// shares](crate::model::decode#errors). `@attr.user` is required, and a row
/// needs a `name`, an `artist`, a `playcount` and an `@attr.rank`.
pub fn user_get_top_tracks(raw: &Raw) -> Result<Page<TopTrack>, Error> {
    shared::paged(
        raw,
        &methods::USER_GET_TOP_TRACKS,
        "toptracks",
        "track",
        Some("user"),
        top_track,
    )
}

/// A track in a list of a user's most played, with its place and how often
/// the user played it. [`json`](Self::json) is the row exactly as decoded.
#[derive(Clone)]
pub struct TopTrack {
    rank: u64,
    playcount: u64,
    duration: Option<u64>,
    track: Track,
    artist: Artist,
    json: Value,
}

impl TopTrack {
    /// The place in the whole list, from 1, whatever page the row is on.
    pub fn rank(&self) -> u64 {
        self.rank
    }

    /// How many times the user played the track in the period.
    pub fn playcount(&self) -> u64 {
        self.playcount
    }

    /// The length of the track in seconds. `None` when the row has no
    /// `duration` or gives 0, which is read as a length the service does
    /// not know; [`json`](Self::json) tells the two apart.
    pub fn duration(&self) -> Option<u64> {
        self.duration
    }

    /// The track.
    pub fn track(&self) -> &Track {
        &self.track
    }

    /// The track's artist.
    pub fn artist(&self) -> &Artist {
        &self.artist
    }

    /// The row exactly as decoded.
    pub fn json(&self) -> &Value {
        &self.json
    }
}

impl fmt::Debug for TopTrack {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("TopTrack")
            .field("rank", &self.rank)
            .field("playcount", &self.playcount)
            .field("duration", &self.duration)
            .finish_non_exhaustive()
    }
}

/// Decodes a `user.getTopTags` response: the tags a user has used most, most
/// used first. The list's subject is the user.
///
/// The reply is not paged: its `@attr` holds the user and nothing else.
///
/// # Errors
///
/// [`ErrorKind::Decode`](crate::ErrorKind) under [the rules every list
/// shares](crate::model::decode#errors). `@attr.user` is required, and a row
/// needs a `name` and a `count`.
pub fn user_get_top_tags(raw: &Raw) -> Result<List<TopTag>, Error> {
    shared::listed(
        raw,
        &methods::USER_GET_TOP_TAGS,
        "toptags",
        "tag",
        Some("user"),
        top_tag,
    )
}

/// A tag in a list of the most used, with how often it was applied.
/// [`json`](Self::json) is the row exactly as decoded.
#[derive(Clone)]
pub struct TopTag {
    name: String,
    count: u64,
    url: Option<String>,
    json: Value,
}

impl TopTag {
    /// The tag.
    pub fn name(&self) -> &str {
        &self.name
    }

    /// How many times the tag was applied.
    pub fn count(&self) -> u64 {
        self.count
    }

    /// The Last.fm page of the tag, if the service sent a non-empty one.
    pub fn url(&self) -> Option<&str> {
        self.url.as_deref()
    }

    /// The row exactly as decoded.
    pub fn json(&self) -> &Value {
        &self.json
    }
}

impl fmt::Debug for TopTag {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("TopTag")
            .field("count", &self.count)
            .finish_non_exhaustive()
    }
}

fn rank(row: &Map<String, Value>, at: &str) -> Result<u64, Error> {
    row.get("@attr")
        .and_then(|attr| attr.get("rank"))
        .and_then(de::uint)
        .ok_or_else(|| missing_or(&format!("{at}.@attr.rank"), "an unsigned integer"))
}

pub(super) fn top_artist(row: Map<String, Value>, at: &str) -> Result<TopArtist, Error> {
    Ok(TopArtist {
        rank: rank(&row, at)?,
        playcount: shared::required_uint(&row, at, "playcount")?,
        artist: shared::parse_artist_row(&row, at)?,
        json: Value::Object(row),
    })
}

pub(super) fn top_album(row: Map<String, Value>, at: &str) -> Result<TopAlbum, Error> {
    Ok(TopAlbum {
        rank: rank(&row, at)?,
        playcount: shared::required_uint(&row, at, "playcount")?,
        album: shared::parse_album_row(&row, at)?,
        artist: shared::parse_artist(&row, at)?,
        json: Value::Object(row),
    })
}

pub(super) fn top_track(row: Map<String, Value>, at: &str) -> Result<TopTrack, Error> {
    Ok(TopTrack {
        rank: rank(&row, at)?,
        playcount: shared::required_uint(&row, at, "playcount")?,
        duration: shared::duration(&row, at)?,
        track: shared::parse_track(&row, at)?,
        artist: shared::parse_artist(&row, at)?,
        json: Value::Object(row),
    })
}

fn top_tag(row: Map<String, Value>, at: &str) -> Result<TopTag, Error> {
    Ok(TopTag {
        name: shared::required_str(&row, at, "name")?.to_owned(),
        count: shared::required_uint(&row, at, "count")?,
        url: shared::optional_str(&row, at, "url")?,
        json: Value::Object(row),
    })
}

#[cfg(test)]
mod tests;
