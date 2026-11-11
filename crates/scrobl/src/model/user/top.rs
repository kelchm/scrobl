//! What a user plays and tags most: the replies to `user.getTopArtists`,
//! `user.getTopAlbums`, `user.getTopTracks` and `user.getTopTags`.

use std::fmt;

use serde_json::{Map, Value};

use crate::de;
use crate::error::Error;
use crate::model::shared::{self, Album, Artist, PageAttr, Track, missing_or};
use crate::protocol::{Raw, methods};

/// One page of `user.getTopArtists`: the artists a user played most in a
/// period, most played first, with the paging attributes.
///
/// `artist` is an array, a single object when the page has exactly one row,
/// and for an empty page absent, `[]` or `""`, under the same rule as
/// [`RecentTracksPage`](super::RecentTracksPage): only when `@attr` allows
/// no rows.
///
/// ```
/// use scrobl::model::TopArtistsPage;
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
/// let page = TopArtistsPage::decode(&raw)?;
/// let top = &page.artists()[0];
/// assert_eq!((top.rank(), top.playcount()), (1, 42));
/// assert_eq!(top.artist().name(), "A");
/// assert_eq!(top.artist().mbid(), None);
/// # Ok::<(), scrobl::Error>(())
/// ```
#[derive(Clone)]
pub struct TopArtistsPage {
    user: String,
    attr: PageAttr,
    artists: Vec<TopArtist>,
}

impl TopArtistsPage {
    /// Decodes a `user.getTopArtists` response.
    ///
    /// # Errors
    ///
    /// [`ErrorKind::Decode`](crate::ErrorKind) when the response is for
    /// another method, when `topartists`, `@attr` or any of its five members
    /// is missing or malformed, when a row is not an object or lacks a
    /// `name`, a `playcount` or an `@attr.rank`, when `artist` is absent or
    /// empty although `@attr` promises rows on the page, when an object
    /// repeats a member name, or when a field of a row has the wrong type.
    /// The message names the field, such as `topartists.artist[3].playcount`,
    /// and never repeats response text.
    pub fn decode(raw: &Raw) -> Result<Self, Error> {
        const ROOT: &str = "topartists";
        shared::decode(raw, &methods::USER_GET_TOP_ARTISTS, ROOT, |mut list| {
            let user = shared::owner(&list, ROOT, "user")?;
            let (attr, rows) = shared::page(&mut list, ROOT, "artist")?;
            let artists = shared::each(rows, "topartists.artist", top_artist)?;
            Ok(Self {
                user,
                attr,
                artists,
            })
        })
    }

    /// The user the service says the page is for, in its own spelling.
    pub fn user(&self) -> &str {
        &self.user
    }

    /// The paging attributes.
    pub fn attr(&self) -> &PageAttr {
        &self.attr
    }

    /// The artists, in the order the service sent them.
    pub fn artists(&self) -> &[TopArtist] {
        &self.artists
    }
}

impl fmt::Debug for TopArtistsPage {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("TopArtistsPage")
            .field("attr", &self.attr)
            .field("artists", &self.artists.len())
            .finish()
    }
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

/// One page of `user.getTopAlbums`: the albums a user played most in a
/// period, most played first, with the paging attributes.
///
/// `album` takes the forms `artist` takes in a [`TopArtistsPage`].
#[derive(Clone)]
pub struct TopAlbumsPage {
    user: String,
    attr: PageAttr,
    albums: Vec<TopAlbum>,
}

impl TopAlbumsPage {
    /// Decodes a `user.getTopAlbums` response.
    ///
    /// # Errors
    ///
    /// [`ErrorKind::Decode`](crate::ErrorKind), as
    /// [`TopArtistsPage::decode`] gives it, and also when a row has no
    /// `artist`. The message names the field, such as
    /// `topalbums.album[3].artist`, and never repeats response text.
    pub fn decode(raw: &Raw) -> Result<Self, Error> {
        const ROOT: &str = "topalbums";
        shared::decode(raw, &methods::USER_GET_TOP_ALBUMS, ROOT, |mut list| {
            let user = shared::owner(&list, ROOT, "user")?;
            let (attr, rows) = shared::page(&mut list, ROOT, "album")?;
            let albums = shared::each(rows, "topalbums.album", top_album)?;
            Ok(Self { user, attr, albums })
        })
    }

    /// The user the service says the page is for, in its own spelling.
    pub fn user(&self) -> &str {
        &self.user
    }

    /// The paging attributes.
    pub fn attr(&self) -> &PageAttr {
        &self.attr
    }

    /// The albums, in the order the service sent them.
    pub fn albums(&self) -> &[TopAlbum] {
        &self.albums
    }
}

impl fmt::Debug for TopAlbumsPage {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("TopAlbumsPage")
            .field("attr", &self.attr)
            .field("albums", &self.albums.len())
            .finish()
    }
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

/// One page of `user.getTopTracks`: the tracks a user played most in a
/// period, most played first, with the paging attributes.
///
/// `track` takes the forms `artist` takes in a [`TopArtistsPage`].
#[derive(Clone)]
pub struct TopTracksPage {
    user: String,
    attr: PageAttr,
    tracks: Vec<TopTrack>,
}

impl TopTracksPage {
    /// Decodes a `user.getTopTracks` response.
    ///
    /// # Errors
    ///
    /// [`ErrorKind::Decode`](crate::ErrorKind), as
    /// [`TopArtistsPage::decode`] gives it, and also when a row has no
    /// `artist` or a `duration` that is not a number. The message names the
    /// field, such as `toptracks.track[3].duration`, and never repeats
    /// response text.
    pub fn decode(raw: &Raw) -> Result<Self, Error> {
        const ROOT: &str = "toptracks";
        shared::decode(raw, &methods::USER_GET_TOP_TRACKS, ROOT, |mut list| {
            let user = shared::owner(&list, ROOT, "user")?;
            let (attr, rows) = shared::page(&mut list, ROOT, "track")?;
            let tracks = shared::each(rows, "toptracks.track", top_track)?;
            Ok(Self { user, attr, tracks })
        })
    }

    /// The user the service says the page is for, in its own spelling.
    pub fn user(&self) -> &str {
        &self.user
    }

    /// The paging attributes.
    pub fn attr(&self) -> &PageAttr {
        &self.attr
    }

    /// The tracks, in the order the service sent them.
    pub fn tracks(&self) -> &[TopTrack] {
        &self.tracks
    }
}

impl fmt::Debug for TopTracksPage {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("TopTracksPage")
            .field("attr", &self.attr)
            .field("tracks", &self.tracks.len())
            .finish()
    }
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

/// The reply to `user.getTopTags`: the tags a user applied most, most used
/// first.
///
/// The method has no pages and reports no total, so an absent or empty
/// `tag` is a user with no tags. `tag` is otherwise an array, or a single
/// object for one tag.
#[derive(Clone)]
pub struct TopTags {
    user: String,
    tags: Vec<TopTag>,
}

impl TopTags {
    /// Decodes a `user.getTopTags` response.
    ///
    /// # Errors
    ///
    /// [`ErrorKind::Decode`](crate::ErrorKind) when the response is for
    /// another method, when `toptags` or `@attr.user` is missing or
    /// malformed, when a row is not an object or lacks a `name` or a
    /// `count`, when an object repeats a member name, or when a field of a
    /// row has the wrong type. The message names the field, such as
    /// `toptags.tag[3].count`, and never repeats response text.
    pub fn decode(raw: &Raw) -> Result<Self, Error> {
        const ROOT: &str = "toptags";
        shared::decode(raw, &methods::USER_GET_TOP_TAGS, ROOT, |mut list| {
            let user = match list.get("@attr") {
                Some(Value::Object(attr)) => {
                    shared::required_str(attr, "toptags.@attr", "user")?.to_owned()
                }
                _ => return Err(missing_or("toptags.@attr", "an object")),
            };
            let rows = shared::rows(&mut list, "toptags.tag", "tag")?;
            let tags = shared::each(rows, "toptags.tag", top_tag)?;
            Ok(Self { user, tags })
        })
    }

    /// The user the service says the tags are of, in its own spelling.
    pub fn user(&self) -> &str {
        &self.user
    }

    /// The tags, in the order the service sent them.
    pub fn tags(&self) -> &[TopTag] {
        &self.tags
    }
}

impl fmt::Debug for TopTags {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("TopTags")
            .field("tags", &self.tags.len())
            .finish_non_exhaustive()
    }
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
        duration: shared::optional_uint(&row, at, "duration")?.filter(|&seconds| seconds != 0),
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
