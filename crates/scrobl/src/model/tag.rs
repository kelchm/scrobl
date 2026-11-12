//! Tags: the replies to `tag.getInfo`, `tag.getSimilar`, `tag.getTopAlbums`,
//! `tag.getTopArtists`, `tag.getTopTracks`, `tag.getTopTags` and
//! `tag.getWeeklyChartList`.

use std::fmt;

use serde_json::{Map, Value};

use super::user::chart_range;
use crate::error::Error;
use crate::model::WeeklyChartRange;
use crate::model::shared::{self, Album, Artist, Track, list_page, missing_or};
use crate::protocol::{Raw, methods};

/// What the service says about one tag: the reply to `tag.getInfo`.
/// [`json`](Self::json) is the tag exactly as decoded.
///
/// A tag nobody has used is not an error: the service answers with counts
/// of 0.
///
/// ```
/// use scrobl::model::TagInfo;
/// use scrobl::protocol::{self, HttpResponse, Request, methods};
///
/// let body = r##"{"tag": {
///     "name": "disco", "total": 68000, "reach": 21000,
///     "wiki": {"summary": "Disco is a genre.", "content": ""}
/// }}"##;
/// let request = Request::new(&methods::TAG_GET_INFO).param("tag", "disco");
/// let raw = protocol::decode(&request, HttpResponse::new(200, body))?;
///
/// let tag = TagInfo::decode(&raw)?;
/// assert_eq!(tag.name(), "disco");
/// assert_eq!((tag.total(), tag.reach()), (68_000, 21_000));
/// assert_eq!(tag.summary(), Some("Disco is a genre."));
/// assert_eq!(tag.content(), None);
/// # Ok::<(), scrobl::Error>(())
/// ```
#[derive(Clone)]
pub struct TagInfo {
    name: String,
    total: u64,
    reach: u64,
    summary: Option<String>,
    content: Option<String>,
    json: Value,
}

impl TagInfo {
    /// Decodes a `tag.getInfo` response.
    ///
    /// # Errors
    ///
    /// [`ErrorKind::Decode`](crate::ErrorKind) when the response is for
    /// another method, when `tag`, its `name`, its `total` or its `reach`
    /// is missing or malformed, when an object repeats a member name, or
    /// when another field has the wrong type. The message names the field,
    /// such as `tag.reach`, and never repeats response text.
    pub fn decode(raw: &Raw) -> Result<Self, Error> {
        shared::decode(raw, &methods::TAG_GET_INFO, "tag", |tag| {
            let (summary, content) = match tag.get("wiki") {
                None => (None, None),
                Some(Value::Object(wiki)) => (
                    shared::optional_str(wiki, "tag.wiki", "summary")?,
                    shared::optional_str(wiki, "tag.wiki", "content")?,
                ),
                Some(_) => return Err(Error::decode_field("tag.wiki", "not an object")),
            };
            Ok(Self {
                name: shared::required_str(&tag, "tag", "name")?.to_owned(),
                total: shared::required_uint(&tag, "tag", "total")?,
                reach: shared::required_uint(&tag, "tag", "reach")?,
                summary,
                content,
                json: Value::Object(tag),
            })
        })
    }

    /// The tag, as the service spells it back.
    pub fn name(&self) -> &str {
        &self.name
    }

    /// The service's `total`: how many times the tag has been used.
    pub fn total(&self) -> u64 {
        self.total
    }

    /// The service's `reach`: how many users have used the tag.
    pub fn reach(&self) -> u64 {
        self.reach
    }

    /// The start of the tag's description, if there is one. It is text
    /// that users of the service wrote, it can hold HTML, and the service
    /// ends it with a link to its own page, which it sends for a tag
    /// without a description too.
    pub fn summary(&self) -> Option<&str> {
        self.summary.as_deref()
    }

    /// The tag's whole description, if there is one. Text that users of
    /// the service wrote, which can hold HTML.
    pub fn content(&self) -> Option<&str> {
        self.content.as_deref()
    }

    /// The tag exactly as decoded.
    pub fn json(&self) -> &Value {
        &self.json
    }
}

impl fmt::Debug for TagInfo {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("TagInfo")
            .field("total", &self.total)
            .field("reach", &self.reach)
            .finish_non_exhaustive()
    }
}

/// The tags the service finds like one tag: the reply to `tag.getSimilar`.
///
/// On 2026-11-11 the service answered every tag asked about with no rows
/// and `n/a` for the tag's name, so [`tags`](Self::tags) was always empty.
/// The rows are modelled on the method's official sample: a `name` and a
/// `url`.
#[derive(Clone)]
pub struct SimilarTags {
    tag: String,
    tags: Vec<SimilarTag>,
}

impl SimilarTags {
    /// Decodes a `tag.getSimilar` response.
    ///
    /// # Errors
    ///
    /// [`ErrorKind::Decode`](crate::ErrorKind) when the response is for
    /// another method, when `similartags` or `@attr.tag` is missing or
    /// malformed, when a row is not an object or lacks a `name`, or when
    /// an object repeats a member name. The message names the field, such
    /// as `similartags.tag[3].name`, and never repeats response text.
    pub fn decode(raw: &Raw) -> Result<Self, Error> {
        const ROOT: &str = "similartags";
        shared::decode(raw, &methods::TAG_GET_SIMILAR, ROOT, |mut list| {
            let tag = shared::owner(&list, ROOT, "tag")?;
            let rows = shared::rows(&mut list, "similartags.tag", "tag")?;
            let tags = shared::each(rows, "similartags.tag", |row, at| {
                Ok(SimilarTag {
                    name: shared::required_str(&row, at, "name")?.to_owned(),
                    url: shared::optional_str(&row, at, "url")?,
                    json: Value::Object(row),
                })
            })?;
            Ok(Self { tag, tags })
        })
    }

    /// The tag the service says the list is for. It has sent `n/a` here
    /// whatever tag was asked about.
    pub fn tag(&self) -> &str {
        &self.tag
    }

    /// The similar tags, in the order the service sent them.
    pub fn tags(&self) -> &[SimilarTag] {
        &self.tags
    }
}

impl fmt::Debug for SimilarTags {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("SimilarTags")
            .field("tags", &self.tags.len())
            .finish_non_exhaustive()
    }
}

/// A tag in a list of similar tags. [`json`](Self::json) is the row exactly
/// as decoded.
#[derive(Clone)]
pub struct SimilarTag {
    name: String,
    url: Option<String>,
    json: Value,
}

impl SimilarTag {
    /// The tag.
    pub fn name(&self) -> &str {
        &self.name
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

impl fmt::Debug for SimilarTag {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("SimilarTag").finish_non_exhaustive()
    }
}

list_page! {
    /// One page of `tag.getTopAlbums`: the albums given a tag most, with
    /// the paging attributes.
    ///
    /// `album` is an array, for a page of one too, and `[]` for a page
    /// beyond the last. A tag nobody has used is an empty page with a
    /// `total` of 0, not an error. The `@attr.rank` of a row is not read:
    /// the service counts it from the top of each page, not of the list, so
    /// a row's place is its position, counted on from the pages before it.
    ///
    /// ```
    /// use scrobl::model::TagTopAlbumsPage;
    /// use scrobl::protocol::{self, HttpResponse, Request, methods};
    ///
    /// let body = r##"{"albums": {
    ///     "album": [{"name": "Record", "mbid": "", "url": "https://www.last.fm/music/A/Record",
    ///                "artist": {"name": "A", "mbid": "", "url": "https://www.last.fm/music/A"},
    ///                "@attr": {"rank": "1"}}],
    ///     "@attr": {"tag": "disco", "page": "1", "perPage": "1",
    ///               "totalPages": "900", "total": "900"}
    /// }}"##;
    /// let request = Request::new(&methods::TAG_GET_TOP_ALBUMS).param("tag", "disco");
    /// let raw = protocol::decode(&request, HttpResponse::new(200, body))?;
    ///
    /// let page = TagTopAlbumsPage::decode(&raw)?;
    /// assert_eq!(page.tag(), "disco");
    /// assert_eq!(page.albums()[0].album().title(), Some("Record"));
    /// assert_eq!(page.albums()[0].artist().name(), "A");
    /// # Ok::<(), scrobl::Error>(())
    /// ```
    TagTopAlbumsPage, "tag.getTopAlbums", TAG_GET_TOP_ALBUMS, "albums", "album",
    @of tag = "tag", "The tag, as the service spells it back.";
    albums: TagTopAlbum = top_album
}

list_page! {
    /// One page of `tag.getTopArtists`: the artists given a tag most, with
    /// the paging attributes. It is laid out as a [`TagTopAlbumsPage`] is.
    TagTopArtistsPage, "tag.getTopArtists", TAG_GET_TOP_ARTISTS, "topartists", "artist",
    @of tag = "tag", "The tag, as the service spells it back.";
    artists: TagTopArtist = top_artist
}

list_page! {
    /// One page of `tag.getTopTracks`: the tracks given a tag most, with
    /// the paging attributes. It is laid out as a [`TagTopAlbumsPage`] is.
    TagTopTracksPage, "tag.getTopTracks", TAG_GET_TOP_TRACKS, "tracks", "track",
    @of tag = "tag", "The tag, as the service spells it back.";
    tracks: TagTopTrack = top_track
}

/// An album in a list of those given a tag most. [`json`](Self::json) is
/// the row exactly as decoded.
#[derive(Clone)]
pub struct TagTopAlbum {
    album: Album,
    artist: Artist,
    json: Value,
}

impl TagTopAlbum {
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

impl fmt::Debug for TagTopAlbum {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("TagTopAlbum").finish_non_exhaustive()
    }
}

/// An artist in a list of those given a tag most. [`json`](Self::json) is
/// the row exactly as decoded.
#[derive(Clone)]
pub struct TagTopArtist {
    artist: Artist,
    json: Value,
}

impl TagTopArtist {
    /// The artist.
    pub fn artist(&self) -> &Artist {
        &self.artist
    }

    /// The row exactly as decoded.
    pub fn json(&self) -> &Value {
        &self.json
    }
}

impl fmt::Debug for TagTopArtist {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("TagTopArtist").finish_non_exhaustive()
    }
}

/// A track in a list of those given a tag most. [`json`](Self::json) is the
/// row exactly as decoded.
#[derive(Clone)]
pub struct TagTopTrack {
    duration: Option<u64>,
    track: Track,
    artist: Artist,
    json: Value,
}

impl TagTopTrack {
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

impl fmt::Debug for TagTopTrack {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("TagTopTrack")
            .field("duration", &self.duration)
            .finish_non_exhaustive()
    }
}

fn top_album(row: Map<String, Value>, at: &str) -> Result<TagTopAlbum, Error> {
    Ok(TagTopAlbum {
        album: shared::parse_album_row(&row, at)?,
        artist: shared::parse_artist(&row, at)?,
        json: Value::Object(row),
    })
}

fn top_artist(row: Map<String, Value>, at: &str) -> Result<TagTopArtist, Error> {
    Ok(TagTopArtist {
        artist: shared::parse_artist_row(&row, at)?,
        json: Value::Object(row),
    })
}

fn top_track(row: Map<String, Value>, at: &str) -> Result<TagTopTrack, Error> {
    Ok(TagTopTrack {
        duration: shared::duration(&row, at)?,
        track: shared::parse_track(&row, at)?,
        artist: shared::parse_artist(&row, at)?,
        json: Value::Object(row),
    })
}

/// Every tag of the service, most used first: the reply to
/// `tag.getTopTags`.
///
/// The list is read from an offset, not by page. Its `@attr` holds the
/// offset, the number of rows asked for and the number of tags there are.
///
/// ```
/// use scrobl::model::AllTags;
/// use scrobl::protocol::{self, HttpResponse, Request, methods};
///
/// let body = r##"{"toptags": {
///     "@attr": {"offset": 0, "num_res": 1, "total": 2896879},
///     "tag": [{"name": "rock", "count": 4077917, "reach": 403467}]
/// }}"##;
/// let request = Request::new(&methods::TAG_GET_TOP_TAGS);
/// let raw = protocol::decode(&request, HttpResponse::new(200, body))?;
///
/// let list = AllTags::decode(&raw)?;
/// assert_eq!((list.offset(), list.total()), (0, 2_896_879));
/// assert_eq!(list.tags()[0].name(), "rock");
/// assert_eq!(list.tags()[0].count(), 4_077_917);
/// # Ok::<(), scrobl::Error>(())
/// ```
#[derive(Clone)]
pub struct AllTags {
    offset: u64,
    total: u64,
    tags: Vec<AllTag>,
}

impl AllTags {
    /// Decodes a `tag.getTopTags` response.
    ///
    /// # Errors
    ///
    /// [`ErrorKind::Decode`](crate::ErrorKind) when the response is for
    /// another method, when `toptags`, `@attr.offset` or `@attr.total` is
    /// missing or malformed, when a row is not an object or lacks a
    /// `name`, a `count` or a `reach`, when `tag` is absent or empty
    /// although `@attr` puts the offset before the last tag and does not
    /// say that none was asked for, or when an object repeats a member
    /// name. The message names the field, such as `toptags.tag[3].count`,
    /// and never repeats response text.
    pub fn decode(raw: &Raw) -> Result<Self, Error> {
        const ROOT: &str = "toptags";
        shared::decode(raw, &methods::TAG_GET_TOP_TAGS, ROOT, |mut list| {
            let (offset, asked, total) = match list.get("@attr") {
                Some(Value::Object(attr)) => (
                    shared::required_uint(attr, "toptags.@attr", "offset")?,
                    shared::optional_uint(attr, "toptags.@attr", "num_res")?,
                    shared::required_uint(attr, "toptags.@attr", "total")?,
                ),
                _ => return Err(missing_or("toptags.@attr", "an object")),
            };
            let rows = shared::rows(&mut list, "toptags.tag", "tag")?;
            // As for a page: no rows where `@attr` says there are some is
            // a member gone missing, not an empty list.
            if rows.is_empty() && offset < total && asked != Some(0) {
                return Err(Error::decode_field(
                    "toptags.tag",
                    "absent or empty, but `@attr` says there are tags to list",
                ));
            }
            let tags = shared::each(rows, "toptags.tag", |row, at| {
                Ok(AllTag {
                    name: shared::required_str(&row, at, "name")?.to_owned(),
                    count: shared::required_uint(&row, at, "count")?,
                    reach: shared::required_uint(&row, at, "reach")?,
                    json: Value::Object(row),
                })
            })?;
            Ok(Self {
                offset,
                total,
                tags,
            })
        })
    }

    /// How many tags come before the first row.
    pub fn offset(&self) -> u64 {
        self.offset
    }

    /// How many tags the service says there are.
    pub fn total(&self) -> u64 {
        self.total
    }

    /// The tags, most used first.
    pub fn tags(&self) -> &[AllTag] {
        &self.tags
    }
}

impl fmt::Debug for AllTags {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("AllTags")
            .field("offset", &self.offset)
            .field("total", &self.total)
            .field("tags", &self.tags.len())
            .finish()
    }
}

/// A tag in the list of every tag. [`json`](Self::json) is the row exactly
/// as decoded.
#[derive(Clone)]
pub struct AllTag {
    name: String,
    count: u64,
    reach: u64,
    json: Value,
}

impl AllTag {
    /// The tag.
    pub fn name(&self) -> &str {
        &self.name
    }

    /// The service's `count`: how many times the tag has been used.
    pub fn count(&self) -> u64 {
        self.count
    }

    /// The service's `reach`: how many users have used the tag.
    pub fn reach(&self) -> u64 {
        self.reach
    }

    /// The row exactly as decoded.
    pub fn json(&self) -> &Value {
        &self.json
    }
}

impl fmt::Debug for AllTag {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("AllTag")
            .field("count", &self.count)
            .field("reach", &self.reach)
            .finish_non_exhaustive()
    }
}

/// The reply to `tag.getWeeklyChartList`: the weeks the service keeps
/// charts for, oldest first.
///
/// The weeks are the service's own, the same for every tag and for a tag
/// nobody has used: on 2026-11-11 it sent the list a user gets.
#[derive(Clone)]
pub struct TagWeeklyChartList {
    tag: String,
    ranges: Vec<WeeklyChartRange>,
}

impl TagWeeklyChartList {
    /// Decodes a `tag.getWeeklyChartList` response.
    ///
    /// # Errors
    ///
    /// [`ErrorKind::Decode`](crate::ErrorKind) when the response is for
    /// another method, when `weeklychartlist` or `@attr.tag` is missing or
    /// malformed, when a row is not an object or lacks a `from` or a `to`
    /// that is a number, or when an object repeats a member name. The
    /// message names the field, such as `weeklychartlist.chart[3].from`, and
    /// never repeats response text.
    pub fn decode(raw: &Raw) -> Result<Self, Error> {
        const ROOT: &str = "weeklychartlist";
        shared::decode(
            raw,
            &methods::TAG_GET_WEEKLY_CHART_LIST,
            ROOT,
            |mut list| {
                let tag = shared::owner(&list, ROOT, "tag")?;
                let rows = shared::rows(&mut list, "weeklychartlist.chart", "chart")?;
                let ranges = shared::each(rows, "weeklychartlist.chart", chart_range)?;
                Ok(Self { tag, ranges })
            },
        )
    }

    /// The tag, as the service spells it back.
    pub fn tag(&self) -> &str {
        &self.tag
    }

    /// The weeks, in the order the service sent them.
    pub fn ranges(&self) -> &[WeeklyChartRange] {
        &self.ranges
    }
}

impl fmt::Debug for TagWeeklyChartList {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("TagWeeklyChartList")
            .field("ranges", &self.ranges.len())
            .finish_non_exhaustive()
    }
}

#[cfg(test)]
mod tests;
