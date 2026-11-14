//! What a user plays and tags most: the replies to `user.getTopArtists`,
//! `user.getTopAlbums`, `user.getTopTracks` and `user.getTopTags`, and the
//! rows they share with a user's weekly charts and library.

use std::fmt;

use serde_json::{Map, Value};

use crate::error::Error;
use crate::model::entity::{
    self, Album, Artist, Counts, Length, Tag, TagCounts, Track, counted_row,
};
use crate::model::{List, Page, shared};
use crate::protocol::{Raw, methods};

/// An artist in one of a user's lists, with how often the user played it.
/// [`json`](Self::json) is the row exactly as decoded.
#[derive(Clone)]
pub struct UserArtist {
    playcount: u64,
    tagcount: Option<u64>,
    artist: Artist,
}

impl UserArtist {
    /// How many times the user played the artist: in the period of a top
    /// list, in the span of a weekly chart, or in all for the library. This
    /// is the row's own number, not the artist's plays by everyone.
    pub fn playcount(&self) -> u64 {
        self.playcount
    }

    /// How many tags the user has given the artist. Only the library says,
    /// and it has said 0 for every artist recorded.
    pub fn tagcount(&self) -> Option<u64> {
        self.tagcount
    }

    /// The artist.
    pub fn artist(&self) -> &Artist {
        &self.artist
    }

    /// The artist, to keep.
    pub fn into_artist(self) -> Artist {
        self.artist
    }

    /// The row exactly as decoded.
    pub fn json(&self) -> &Value {
        self.artist.json()
    }
}

impl AsRef<Artist> for UserArtist {
    fn as_ref(&self) -> &Artist {
        &self.artist
    }
}

impl fmt::Debug for UserArtist {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("UserArtist")
            .field("playcount", &self.playcount)
            .field("tagcount", &self.tagcount)
            .finish_non_exhaustive()
    }
}

counted_row! {
    /// An album in one of a user's lists, with how often the user played
    /// it.
    UserAlbum {
        /// How many times the user played the album, in the period or span
        /// of the list. This is the row's own number, not the album's plays
        /// by everyone.
        playcount,
        album / into_album: Album
    }
}

counted_row! {
    /// A track in one of a user's lists, with how often the user played it.
    UserTrack {
        /// How many times the user played the track, in the period or span
        /// of the list. This is the row's own number, not the track's plays
        /// by everyone.
        playcount,
        track / into_track: Track
    }
}

counted_row! {
    /// A tag in the list of those a user has used most, with how often the
    /// user used it.
    UserTag {
        /// How many times the user has used the tag. This is the row's own
        /// number, not the tag's use by everyone.
        count,
        tag / into_tag: Tag
    }
}

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
/// assert_eq!(top.playcount(), 42);
/// assert_eq!(top.artist().name(), "A");
/// assert_eq!(top.artist().mbid(), None);
/// // The user's plays are the row's. The artist's own are not in this reply.
/// assert_eq!(top.artist().playcount(), None);
/// # Ok::<(), scrobl::Error>(())
/// ```
///
/// # Errors
///
/// [`ErrorKind::Decode`](crate::ErrorKind) under [the rules every list
/// shares](crate::model::decode#errors). `@attr.user` is required, and a row
/// needs a `name` and a `playcount`.
pub fn user_get_top_artists(raw: &Raw) -> Result<Page<UserArtist>, Error> {
    shared::paged(
        raw,
        &methods::USER_GET_TOP_ARTISTS,
        "topartists",
        "artist",
        Some("user"),
        user_artist,
    )
}

/// Decodes a `user.getTopAlbums` response: one page of the albums a user
/// played most in a period. The page's subject is the user.
///
/// # Errors
///
/// [`ErrorKind::Decode`](crate::ErrorKind) under [the rules every list
/// shares](crate::model::decode#errors). `@attr.user` is required, and a row
/// needs a `name`, an `artist` and a `playcount`.
pub fn user_get_top_albums(raw: &Raw) -> Result<Page<UserAlbum>, Error> {
    shared::paged(
        raw,
        &methods::USER_GET_TOP_ALBUMS,
        "topalbums",
        "album",
        Some("user"),
        user_album,
    )
}

/// Decodes a `user.getTopTracks` response: one page of the tracks a user
/// played most in a period. The page's subject is the user.
///
/// # Errors
///
/// [`ErrorKind::Decode`](crate::ErrorKind) under [the rules every list
/// shares](crate::model::decode#errors). `@attr.user` is required, and a row
/// needs a `name`, an `artist` and a `playcount`.
pub fn user_get_top_tracks(raw: &Raw) -> Result<Page<UserTrack>, Error> {
    shared::paged(
        raw,
        &methods::USER_GET_TOP_TRACKS,
        "toptracks",
        "track",
        Some("user"),
        user_track,
    )
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
pub fn user_get_top_tags(raw: &Raw) -> Result<List<UserTag>, Error> {
    shared::listed(
        raw,
        &methods::USER_GET_TOP_TAGS,
        "toptags",
        "tag",
        Some("user"),
        |row, at| {
            Ok(UserTag {
                count: shared::required_uint(&row, at, "count")?,
                tag: entity::tag(row, at, TagCounts::Neither)?,
            })
        },
    )
}

/// A row of a user's artists: the user's `playcount`, and the `tagcount`
/// where the list sends one.
pub(in crate::model) fn user_artist(
    row: Map<String, Value>,
    at: &str,
) -> Result<UserArtist, Error> {
    Ok(UserArtist {
        playcount: shared::required_uint(&row, at, "playcount")?,
        tagcount: shared::optional_uint(&row, at, "tagcount")?,
        artist: entity::artist(row, at, Counts::Neither)?,
    })
}

pub(super) fn user_album(row: Map<String, Value>, at: &str) -> Result<UserAlbum, Error> {
    Ok(UserAlbum {
        playcount: shared::required_uint(&row, at, "playcount")?,
        album: entity::album(row, at, Counts::Neither)?,
    })
}

pub(super) fn user_track(row: Map<String, Value>, at: &str) -> Result<UserTrack, Error> {
    Ok(UserTrack {
        playcount: shared::required_uint(&row, at, "playcount")?,
        track: entity::track(row, at, Counts::Neither, Length::Read)?,
    })
}

#[cfg(test)]
mod tests;
