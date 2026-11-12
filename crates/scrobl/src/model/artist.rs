//! Artists: the replies to `artist.getCorrection`, `artist.getSimilar`,
//! `artist.getTopAlbums`, `artist.getTopTags` and `artist.getTopTracks`.

use std::fmt;

use serde_json::{Map, Value};

use super::entity::{self, Album, Artist, Counts, Length, Tag, TagCounts, Track, counted_row};
use super::shared::{self, missing_or};
use super::{List, Page};
use crate::de;
use crate::error::Error;
use crate::protocol::{Raw, methods};

/// An artist the service finds like another, with how alike it finds them.
#[derive(Clone)]
pub struct SimilarArtist {
    similarity: f64,
    artist: Artist,
}

impl SimilarArtist {
    /// How alike the service finds the two, as it sends it under `match`:
    /// 1 for the most alike, down towards 0, in every response recorded.
    pub fn similarity(&self) -> f64 {
        self.similarity
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

impl AsRef<Artist> for SimilarArtist {
    fn as_ref(&self) -> &Artist {
        &self.artist
    }
}

impl fmt::Debug for SimilarArtist {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("SimilarArtist")
            .field("similarity", &self.similarity)
            .finish_non_exhaustive()
    }
}

counted_row! {
    /// A tag given to an artist, an album or a track, with how much it was
    /// given compared with the others.
    TopTag {
        /// How much the tag was given: 100 for the tag given most, and less
        /// for the others in proportion. It is not a number of uses.
        count,
        tag / into_tag: Tag
    }
}

/// One row of a list of the tags given to something most.
pub(super) fn top_tag(row: Map<String, Value>, at: &str) -> Result<TopTag, Error> {
    Ok(TopTag {
        count: shared::required_uint(&row, at, "count")?,
        tag: entity::tag(row, at, TagCounts::Neither)?,
    })
}

/// Decodes an `artist.getSimilar` response: the artists the service finds
/// like one artist, most alike first. The list's subject is that artist, as
/// the service spells the name.
///
/// The service sends at most 250 rows, whatever limit is asked for.
///
/// ```
/// use scrobl::model::decode;
/// use scrobl::protocol::{self, HttpResponse, Request, methods};
///
/// let body = r##"{"similarartists": {
///     "artist": [{"name": "B", "match": "0.944133", "url": "https://www.last.fm/music/B"}],
///     "@attr": {"artist": "A"}
/// }}"##;
/// let request = Request::new(&methods::ARTIST_GET_SIMILAR).param("artist", "a");
/// let raw = protocol::decode(&request, HttpResponse::new(200, body))?;
///
/// let similar = decode::artist_get_similar(&raw)?;
/// assert_eq!(similar.subject(), Some("A"));
/// assert_eq!(similar[0].artist().name(), "B");
/// assert!(similar[0].similarity() > 0.9);
/// # Ok::<(), scrobl::Error>(())
/// ```
///
/// # Errors
///
/// [`ErrorKind::Decode`](crate::ErrorKind) under [the rules every list
/// shares](super::decode#errors). `@attr.artist` is required, and a row
/// needs a `name` and a `match` that is a decimal and not negative.
pub fn artist_get_similar(raw: &Raw) -> Result<List<SimilarArtist>, Error> {
    shared::listed(
        raw,
        &methods::ARTIST_GET_SIMILAR,
        "similarartists",
        "artist",
        Some("artist"),
        |row, at| {
            let similarity = row
                .get("match")
                .and_then(de::decimal)
                .ok_or_else(|| missing_or(&format!("{at}.match"), "a decimal"))?;
            Ok(SimilarArtist {
                similarity,
                artist: entity::artist(row, at, Counts::Neither)?,
            })
        },
    )
}

/// Decodes an `artist.getTopAlbums` response: one page of an artist's
/// albums, in the service's order. The page's subject is the artist, as
/// the service spells the name.
///
/// The order is the service's own and not by `playcount`: of the first
/// three albums recorded, the second had been played least. It may be by
/// listeners, as the tracks are, but an album's row has no such count to
/// tell by. Each album has
/// its `playcount` for everyone and no `listeners`. The
/// service answers a page beyond the last with the first page, not with an
/// empty one, and [`number`](Page::number) says which page came.
///
/// # Errors
///
/// [`ErrorKind::Decode`](crate::ErrorKind) under [the rules every list
/// shares](super::decode#errors). `@attr.artist` is required, and a row
/// needs a `name`, a `playcount` and an `artist` with a name.
pub fn artist_get_top_albums(raw: &Raw) -> Result<Page<Album>, Error> {
    shared::paged(
        raw,
        &methods::ARTIST_GET_TOP_ALBUMS,
        "topalbums",
        "album",
        Some("artist"),
        |row, at| entity::album(row, at, Counts::Playcount),
    )
}

/// Decodes an `artist.getTopTracks` response: one page of an artist's
/// tracks, the one with most listeners first. The page's subject is the
/// artist, as the service spells the name.
///
/// The order is by `listeners`, not by `playcount`: in 1000 rows recorded
/// no track had more listeners than the one before it, and 491 had more
/// plays. Each track has both counts, for everyone, and no length. The service answers a page beyond the last with the first page,
/// as it does for the albums.
///
/// # Errors
///
/// [`ErrorKind::Decode`](crate::ErrorKind) under [the rules every list
/// shares](super::decode#errors). `@attr.artist` is required, and a row
/// needs a `name`, `listeners`, a `playcount` and an `artist` with a name.
pub fn artist_get_top_tracks(raw: &Raw) -> Result<Page<Track>, Error> {
    shared::paged(
        raw,
        &methods::ARTIST_GET_TOP_TRACKS,
        "toptracks",
        "track",
        Some("artist"),
        |row, at| entity::track(row, at, Counts::Both, Length::Read),
    )
}

/// Decodes an `artist.getTopTags` response: the tags given to an artist
/// most. The list's subject is the artist, as the service spells the name.
///
/// ```
/// use scrobl::model::decode;
/// use scrobl::protocol::{self, HttpResponse, Request, methods};
///
/// let body = r##"{"toptags": {
///     "tag": [{"count": 100, "name": "rock", "url": "https://www.last.fm/tag/rock"}],
///     "@attr": {"artist": "A"}
/// }}"##;
/// let request = Request::new(&methods::ARTIST_GET_TOP_TAGS).param("artist", "a");
/// let raw = protocol::decode(&request, HttpResponse::new(200, body))?;
///
/// let tags = decode::artist_get_top_tags(&raw)?;
/// assert_eq!((tags[0].tag().name(), tags[0].count()), ("rock", 100));
/// # Ok::<(), scrobl::Error>(())
/// ```
///
/// # Errors
///
/// [`ErrorKind::Decode`](crate::ErrorKind) under [the rules every list
/// shares](super::decode#errors). `@attr.artist` is required, and a row
/// needs a `name` and a `count`.
pub fn artist_get_top_tags(raw: &Raw) -> Result<List<TopTag>, Error> {
    shared::listed(
        raw,
        &methods::ARTIST_GET_TOP_TAGS,
        "toptags",
        "tag",
        Some("artist"),
        top_tag,
    )
}

/// Decodes an `artist.getCorrection` response: the artist the service files
/// a name under. A name that is already right comes back as itself. A name
/// the service does not know is an error of the call, not a reply.
///
/// ```
/// use scrobl::model::decode;
/// use scrobl::protocol::{self, HttpResponse, Request, methods};
///
/// let body = r##"{"corrections": {"correction": {
///     "artist": {"name": "Guns N' Roses", "url": "https://www.last.fm/music/Guns+N%27+Roses"},
///     "@attr": {"index": "0"}
/// }}}"##;
/// let request = Request::new(&methods::ARTIST_GET_CORRECTION).param("artist", "Guns and Roses");
/// let raw = protocol::decode(&request, HttpResponse::new(200, body))?;
///
/// assert_eq!(decode::artist_get_correction(&raw)?.name(), "Guns N' Roses");
/// # Ok::<(), scrobl::Error>(())
/// ```
///
/// # Errors
///
/// [`ErrorKind::Decode`](crate::ErrorKind) when the response is for another
/// method, when `corrections.correction.artist` is missing or not an
/// object, when it has no `name`, or when an object repeats a member name.
/// The message names the field and never repeats response text.
pub fn artist_get_correction(raw: &Raw) -> Result<Artist, Error> {
    shared::decode(
        raw,
        &methods::ARTIST_GET_CORRECTION,
        "corrections",
        |mut corrections| {
            const AT: &str = "corrections.correction.artist";
            let artist = match corrections.remove("correction") {
                Some(Value::Object(mut correction)) => correction.remove("artist"),
                _ => return Err(missing_or("corrections.correction", "an object")),
            };
            match artist {
                Some(Value::Object(artist)) => entity::artist(artist, AT, Counts::Neither),
                _ => Err(missing_or(AT, "an object")),
            }
        },
    )
}

#[cfg(test)]
mod tests;
