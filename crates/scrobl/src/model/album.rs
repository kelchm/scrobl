//! Albums: the replies to `album.getInfo`, `album.getTags` and
//! `album.getTopTags`.

use std::fmt;

use serde_json::Value;

use super::List;
use super::artist::{TopTag, top_tag};
use super::entity::{self, Album, Counts, Length, Tag, TagCounts, Track};
use super::shared::{self, Wiki};
use crate::error::Error;
use crate::protocol::{Raw, methods};

/// What the service says about one album: the reply to `album.getInfo`.
/// [`json`](Self::json) is the album exactly as decoded.
///
/// ```
/// use std::time::Duration;
///
/// use scrobl::model::decode;
/// use scrobl::protocol::{self, HttpResponse, Request, methods};
///
/// let body = r##"{"album": {
///     "name": "B", "artist": "A", "mbid": "", "url": "https://www.last.fm/music/A/B",
///     "listeners": "30", "playcount": "900", "tags": "",
///     "tracks": {"track": [{"name": "One", "duration": 207, "@attr": {"rank": 1},
///                           "artist": {"name": "A", "url": "https://www.last.fm/music/A"}}]}
/// }}"##;
/// let request = Request::new(&methods::ALBUM_GET_INFO).param("artist", "a").param("album", "b");
/// let raw = protocol::decode(&request, HttpResponse::new(200, body))?;
///
/// let info = decode::album_get_info(&raw)?;
/// assert_eq!(info.album().name(), "B");
/// assert_eq!(info.album().artist().map(|artist| artist.name()), Some("A"));
/// assert_eq!((info.listeners(), info.playcount()), (30, 900));
/// assert_eq!(info.tracks()[0].duration(), Some(Duration::from_secs(207)));
/// assert!(info.tags().is_empty());
/// assert_eq!(info.summary(), None);
/// # Ok::<(), scrobl::Error>(())
/// ```
#[derive(Clone)]
pub struct AlbumInfo {
    album: Album,
    listeners: u64,
    playcount: u64,
    user_playcount: Option<u64>,
    tracks: Vec<Track>,
    tags: Vec<Tag>,
    wiki: Wiki,
}

impl AlbumInfo {
    /// The album, as the service spells its name, with its counts for
    /// everyone. Its artist is a name alone: this reply has no more.
    pub fn album(&self) -> &Album {
        &self.album
    }

    /// The album, to keep.
    pub fn into_album(self) -> Album {
        self.album
    }

    /// How many users have played the album.
    pub fn listeners(&self) -> u64 {
        self.listeners
    }

    /// How many times the album has been played, by everyone.
    pub fn playcount(&self) -> u64 {
        self.playcount
    }

    /// How many times one user has played the album. `None` unless the
    /// call named a user; 0 for a user who never has.
    pub fn user_playcount(&self) -> Option<u64> {
        self.user_playcount
    }

    /// The album's tracks, in the album's order. Each has its artist and,
    /// where the service knows it, its length.
    pub fn tracks(&self) -> &[Track] {
        &self.tracks
    }

    /// A few of the tags given to the album most: no more than five in
    /// any response recorded, and none for an album nobody has tagged.
    pub fn tags(&self) -> &[Tag] {
        &self.tags
    }

    /// The start of what the service's users wrote about the album, if
    /// anything. It can hold HTML, and the service ends it with a link to
    /// its own page.
    pub fn summary(&self) -> Option<&str> {
        self.wiki.summary.as_deref()
    }

    /// All of what the service's users wrote about the album, if
    /// anything. It can hold HTML.
    pub fn content(&self) -> Option<&str> {
        self.wiki.content.as_deref()
    }

    /// When that text was last changed, as the service words it, such as
    /// `11 Nov 2022, 15:31`.
    pub fn published(&self) -> Option<&str> {
        self.wiki.published.as_deref()
    }

    /// The album exactly as decoded.
    pub fn json(&self) -> &Value {
        self.album.json()
    }
}

impl AsRef<Album> for AlbumInfo {
    fn as_ref(&self) -> &Album {
        &self.album
    }
}

impl fmt::Debug for AlbumInfo {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("AlbumInfo")
            .field("listeners", &self.listeners)
            .field("playcount", &self.playcount)
            .field("user_playcount", &self.user_playcount)
            .field("tracks", &self.tracks.len())
            .field("tags", &self.tags.len())
            .finish_non_exhaustive()
    }
}

/// Decodes an `album.getInfo` response.
///
/// The reply names the album's artist with a string and nothing else, so
/// the album's [`artist`](Album::artist) has a name and no id or link.
///
/// # Errors
///
/// [`ErrorKind::Decode`](crate::ErrorKind) when the response is for another
/// method, when `album`, its `name`, its `artist`, its `listeners` or its
/// `playcount` is missing or malformed, when a track has no `name` or no
/// `artist` with a name, when a tag has no `name`, when an object repeats a
/// member name, or when another field has the wrong type. The message
/// names the field, such as `album.tracks.track[3].duration`, and never
/// repeats response text.
pub fn album_get_info(raw: &Raw) -> Result<AlbumInfo, Error> {
    const AT: &str = "album";
    shared::decode(raw, &methods::ALBUM_GET_INFO, AT, |album| {
        let user_playcount = shared::optional_uint(&album, AT, "userplaycount")?;
        let tracks = shared::nested(&album, AT, "tracks", "track", |row, at| {
            entity::track(row, at, Counts::Neither, Length::Read)
        })?;
        let tags = shared::nested(&album, AT, "tags", "tag", |row, at| {
            entity::tag(row, at, TagCounts::Neither)
        })?;
        let wiki = shared::wiki(&album, AT, "wiki")?;
        let album = entity::album_by_name(album, AT, Counts::Both)?;
        Ok(AlbumInfo {
            // `Counts::Both` makes both required, so both are there.
            listeners: album.listeners().unwrap_or_default(),
            playcount: album.playcount().unwrap_or_default(),
            album,
            user_playcount,
            tracks,
            tags,
            wiki,
        })
    })
}

/// Decodes an `album.getTags` response: the tags one user has given an
/// album. The list's subject is the album, as the service spells its name.
/// A user who has given none is an empty list.
///
/// # Errors
///
/// [`ErrorKind::Decode`](crate::ErrorKind) under [the rules every list
/// shares](super::decode#errors). `@attr.album` is required, and a row
/// needs a `name`.
pub fn album_get_tags(raw: &Raw) -> Result<List<Tag>, Error> {
    shared::listed(
        raw,
        &methods::ALBUM_GET_TAGS,
        "tags",
        "tag",
        Some("album"),
        |row, at| entity::tag(row, at, TagCounts::Neither),
    )
}

/// Decodes an `album.getTopTags` response: the tags given to an album
/// most. The list's subject is the album, as the service spells its name.
///
/// # Errors
///
/// [`ErrorKind::Decode`](crate::ErrorKind) under [the rules every list
/// shares](super::decode#errors). `@attr.album` is required, and a row
/// needs a `name` and a `count`.
pub fn album_get_top_tags(raw: &Raw) -> Result<List<TopTag>, Error> {
    shared::listed(
        raw,
        &methods::ALBUM_GET_TOP_TAGS,
        "toptags",
        "tag",
        Some("album"),
        top_tag,
    )
}

#[cfg(test)]
mod tests;
