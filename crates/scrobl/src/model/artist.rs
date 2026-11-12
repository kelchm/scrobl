//! Artists: the replies to `artist.getCorrection`, `artist.getInfo`,
//! `artist.getSimilar`, `artist.getTags`, `artist.getTopAlbums`,
//! `artist.getTopTags` and `artist.getTopTracks`.

use std::fmt;

use serde_json::{Map, Value};

use super::entity::{self, Album, Artist, Counts, Length, Tag, TagCounts, Track, counted_row};
use super::shared::{self, Wiki, missing_or};
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

/// What the service says about one artist: the reply to `artist.getInfo`.
/// [`json`](Self::json) is the artist exactly as decoded.
///
/// ```
/// use scrobl::model::decode;
/// use scrobl::protocol::{self, HttpResponse, Request, methods};
///
/// let body = r##"{"artist": {
///     "name": "A", "url": "https://www.last.fm/music/A", "ontour": "1",
///     "stats": {"listeners": "30", "playcount": "900", "userplaycount": "7"},
///     "similar": {"artist": [{"name": "B", "url": "https://www.last.fm/music/B"}]},
///     "tags": {"tag": [{"name": "rock", "url": "https://www.last.fm/tag/rock"}]},
///     "bio": {"published": "01 Feb 2006, 19:27", "summary": "A is a band.", "content": ""}
/// }}"##;
/// let request = Request::new(&methods::ARTIST_GET_INFO).param("artist", "a");
/// let raw = protocol::decode(&request, HttpResponse::new(200, body))?;
///
/// let info = decode::artist_get_info(&raw)?;
/// assert_eq!(info.artist().name(), "A");
/// assert_eq!((info.listeners(), info.playcount()), (30, 900));
/// assert_eq!(info.user_playcount(), Some(7));
/// assert!(info.on_tour());
/// assert_eq!(info.similar()[0].name(), "B");
/// assert_eq!(info.tags()[0].name(), "rock");
/// assert_eq!(info.summary(), Some("A is a band."));
/// # Ok::<(), scrobl::Error>(())
/// ```
#[derive(Clone)]
pub struct ArtistInfo {
    artist: Artist,
    listeners: u64,
    playcount: u64,
    user_playcount: Option<u64>,
    on_tour: bool,
    similar: Vec<Artist>,
    tags: Vec<Tag>,
    wiki: Wiki,
}

impl ArtistInfo {
    /// The artist, as the service spells the name, with its counts for
    /// everyone.
    pub fn artist(&self) -> &Artist {
        &self.artist
    }

    /// The artist, to keep.
    pub fn into_artist(self) -> Artist {
        self.artist
    }

    /// How many users have played the artist.
    pub fn listeners(&self) -> u64 {
        self.listeners
    }

    /// How many times the artist has been played, by everyone.
    pub fn playcount(&self) -> u64 {
        self.playcount
    }

    /// How many times one user has played the artist. `None` unless the
    /// call named a user; 0 for a user who never has.
    pub fn user_playcount(&self) -> Option<u64> {
        self.user_playcount
    }

    /// Whether the service lists the artist as on tour.
    pub fn on_tour(&self) -> bool {
        self.on_tour
    }

    /// A few artists the service finds like this one, most alike first:
    /// five at most, and none for an artist it knows little about. Each is
    /// a name, a link and images, without the likeness that
    /// `artist.getSimilar` gives.
    pub fn similar(&self) -> &[Artist] {
        &self.similar
    }

    /// A few of the tags given to the artist most: no more than five in
    /// any response recorded.
    pub fn tags(&self) -> &[Tag] {
        &self.tags
    }

    /// The start of what the service's users wrote about the artist. It
    /// can hold HTML, and the service ends it with a link to its own page.
    /// It sends that link for an artist nobody has written about too, so
    /// this is not the way to tell whether there is a description:
    /// [`content`](Self::content) is.
    pub fn summary(&self) -> Option<&str> {
        self.wiki.summary.as_deref()
    }

    /// All of what the service's users wrote about the artist, if
    /// anything. It can hold HTML.
    pub fn content(&self) -> Option<&str> {
        self.wiki.content.as_deref()
    }

    /// When that text was last changed, as the service words it, such as
    /// `01 Feb 2006, 19:27`. For an artist nobody has written about the
    /// service sent `01 Jan 1970, 00:00`.
    pub fn published(&self) -> Option<&str> {
        self.wiki.published.as_deref()
    }

    /// The artist exactly as decoded.
    pub fn json(&self) -> &Value {
        self.artist.json()
    }
}

impl AsRef<Artist> for ArtistInfo {
    fn as_ref(&self) -> &Artist {
        &self.artist
    }
}

impl fmt::Debug for ArtistInfo {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ArtistInfo")
            .field("listeners", &self.listeners)
            .field("playcount", &self.playcount)
            .field("user_playcount", &self.user_playcount)
            .field("on_tour", &self.on_tour)
            .field("similar", &self.similar.len())
            .field("tags", &self.tags.len())
            .finish_non_exhaustive()
    }
}

/// Decodes an `artist.getInfo` response.
///
/// # Errors
///
/// [`ErrorKind::Decode`](crate::ErrorKind) when the response is for another
/// method, when `artist`, its `name`, its `ontour`, or the `listeners` or
/// `playcount` of its `stats` is missing or malformed, when a similar
/// artist or a tag has no `name`, when an object repeats a member name, or
/// when another field has the wrong type. The message names the field,
/// such as `artist.stats.playcount`, and never repeats response text.
pub fn artist_get_info(raw: &Raw) -> Result<ArtistInfo, Error> {
    const AT: &str = "artist";
    shared::decode(raw, &methods::ARTIST_GET_INFO, AT, |artist| {
        let Some(Value::Object(stats)) = artist.get("stats") else {
            return Err(missing_or("artist.stats", "an object"));
        };
        let listeners = shared::required_uint(stats, "artist.stats", "listeners")?;
        let playcount = shared::required_uint(stats, "artist.stats", "playcount")?;
        let user_playcount = shared::optional_uint(stats, "artist.stats", "userplaycount")?;
        let on_tour = entity::optional_flag(&artist, AT, "ontour")?
            .ok_or_else(|| missing_or("artist.ontour", "a 0/1 flag"))?;
        let similar = shared::nested(&artist, AT, "similar", "artist", |row, at| {
            entity::artist(row, at, Counts::Neither)
        })?;
        let tags = shared::nested(&artist, AT, "tags", "tag", |row, at| {
            entity::tag(row, at, TagCounts::Neither)
        })?;
        let wiki = shared::wiki(&artist, AT, "bio")?;
        Ok(ArtistInfo {
            artist: entity::artist(artist, AT, Counts::Neither)?.counted(listeners, playcount),
            listeners,
            playcount,
            user_playcount,
            on_tour,
            similar,
            tags,
            wiki,
        })
    })
}

/// Decodes an `artist.getTags` response: the tags one user has given an
/// artist. The list's subject is the artist, as the service spells the
/// name. A user who has given none is an empty list.
///
/// # Errors
///
/// [`ErrorKind::Decode`](crate::ErrorKind) under [the rules every list
/// shares](super::decode#errors). `@attr.artist` is required, and a row
/// needs a `name`.
pub fn artist_get_tags(raw: &Raw) -> Result<List<Tag>, Error> {
    shared::listed(
        raw,
        &methods::ARTIST_GET_TAGS,
        "tags",
        "tag",
        Some("artist"),
        |row, at| entity::tag(row, at, TagCounts::Neither),
    )
}

#[cfg(test)]
mod tests;
