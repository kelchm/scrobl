//! Tags: the replies to `tag.getInfo`, `tag.getSimilar`, `tag.getTopAlbums`,
//! `tag.getTopArtists`, `tag.getTopTracks`, `tag.getTopTags` and
//! `tag.getWeeklyChartList`.

use std::fmt;

use serde_json::Value;

use super::entity::{self, Album, Artist, Counts, Length, Tag, TagCounts, Track};
use super::shared::{self, missing_or};
use super::user::chart_range;
use super::{List, Page, WeeklyChartRange};
use crate::error::Error;
use crate::protocol::{Raw, methods};

/// What the service says about one tag: the reply to `tag.getInfo`.
/// [`json`](Self::json) is the tag exactly as decoded.
///
/// A tag nobody has used is not an error: the service answers with counts
/// of 0.
///
/// ```
/// use scrobl::model::decode;
/// use scrobl::protocol::{self, HttpResponse, Request, methods};
///
/// let body = r##"{"tag": {
///     "name": "disco", "total": 68000, "reach": 21000,
///     "wiki": {"summary": "Disco is a genre.", "content": ""}
/// }}"##;
/// let request = Request::new(&methods::TAG_GET_INFO).param("tag", "disco");
/// let raw = protocol::decode(&request, HttpResponse::new(200, body))?;
///
/// let info = decode::tag_get_info(&raw)?;
/// assert_eq!(info.tag().name(), "disco");
/// assert_eq!((info.taggings(), info.reach()), (68_000, 21_000));
/// assert_eq!(info.summary(), Some("Disco is a genre."));
/// assert_eq!(info.content(), None);
/// # Ok::<(), scrobl::Error>(())
/// ```
#[derive(Clone)]
pub struct TagInfo {
    tag: Tag,
    taggings: u64,
    reach: u64,
    summary: Option<String>,
    content: Option<String>,
}

impl TagInfo {
    /// The tag, as the service spells it back.
    pub fn tag(&self) -> &Tag {
        &self.tag
    }

    /// The tag, to keep.
    pub fn into_tag(self) -> Tag {
        self.tag
    }

    /// How many times the tag has been used. The service calls this
    /// `total` here.
    pub fn taggings(&self) -> u64 {
        self.taggings
    }

    /// How many users have used the tag.
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
        self.tag.json()
    }
}

impl AsRef<Tag> for TagInfo {
    fn as_ref(&self) -> &Tag {
        &self.tag
    }
}

impl fmt::Debug for TagInfo {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("TagInfo")
            .field("taggings", &self.taggings)
            .field("reach", &self.reach)
            .finish_non_exhaustive()
    }
}

/// Decodes a `tag.getInfo` response.
///
/// # Errors
///
/// [`ErrorKind::Decode`](crate::ErrorKind) when the response is for another
/// method, when `tag`, its `name`, its `total` or its `reach` is missing or
/// malformed, when an object repeats a member name, or when another field
/// has the wrong type. The message names the field, such as `tag.reach`,
/// and never repeats response text.
pub fn tag_get_info(raw: &Raw) -> Result<TagInfo, Error> {
    shared::decode(raw, &methods::TAG_GET_INFO, "tag", |tag| {
        let (summary, content) = match tag.get("wiki") {
            None => (None, None),
            Some(Value::Object(wiki)) => (
                shared::optional_str(wiki, "tag.wiki", "summary")?,
                shared::optional_str(wiki, "tag.wiki", "content")?,
            ),
            Some(_) => return Err(Error::decode_field("tag.wiki", "not an object")),
        };
        let tag = entity::tag(tag, "tag", TagCounts::Total)?;
        Ok(TagInfo {
            // `TagCounts::Total` makes both required, so both are there.
            taggings: tag.taggings().unwrap_or_default(),
            reach: tag.reach().unwrap_or_default(),
            tag,
            summary,
            content,
        })
    })
}

/// Decodes a `tag.getSimilar` response: the tags the service finds like one
/// tag. The list's subject is the tag the service says the list is for.
///
/// On 2026-11-11 the service answered every tag asked about with no rows
/// and `n/a` for the subject. The rows are modelled on the method's official
/// sample: a `name` and a `url`.
///
/// # Errors
///
/// [`ErrorKind::Decode`](crate::ErrorKind) under [the rules every list
/// shares](super::decode#errors). `@attr.tag` is required, and a row needs
/// a `name`.
pub fn tag_get_similar(raw: &Raw) -> Result<List<Tag>, Error> {
    shared::listed(
        raw,
        &methods::TAG_GET_SIMILAR,
        "similartags",
        "tag",
        Some("tag"),
        |row, at| entity::tag(row, at, TagCounts::Neither),
    )
}

/// Decodes a `tag.getTopAlbums` response: one page of the albums given a tag
/// most. The page's subject is the tag, as the service spells it back.
///
/// A tag nobody has used is an empty page with a `total` of 0, not an error.
/// The `@attr.rank` of a row is not read: the service counts it from the top
/// of each page, not of the list. [`Page::ranked`] gives a row's place.
///
/// ```
/// use scrobl::model::decode;
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
/// let page = decode::tag_get_top_albums(&raw)?;
/// assert_eq!(page.subject(), Some("disco"));
/// assert_eq!(page[0].name(), "Record");
/// assert_eq!(page[0].artist().map(|artist| artist.name()), Some("A"));
/// # Ok::<(), scrobl::Error>(())
/// ```
///
/// # Errors
///
/// [`ErrorKind::Decode`](crate::ErrorKind) under [the rules every list
/// shares](super::decode#errors). `@attr.tag` is required, and a row needs
/// a `name` and an `artist`.
pub fn tag_get_top_albums(raw: &Raw) -> Result<Page<Album>, Error> {
    shared::paged(
        raw,
        &methods::TAG_GET_TOP_ALBUMS,
        "albums",
        "album",
        Some("tag"),
        |row, at| entity::album(row, at, Counts::Neither),
    )
}

/// Decodes a `tag.getTopArtists` response: one page of the artists given a
/// tag most, laid out as [`tag_get_top_albums`] is.
///
/// # Errors
///
/// [`ErrorKind::Decode`](crate::ErrorKind) under [the rules every list
/// shares](super::decode#errors). `@attr.tag` is required, and a row needs
/// a `name`.
pub fn tag_get_top_artists(raw: &Raw) -> Result<Page<Artist>, Error> {
    shared::paged(
        raw,
        &methods::TAG_GET_TOP_ARTISTS,
        "topartists",
        "artist",
        Some("tag"),
        |row, at| entity::artist(row, at, Counts::Neither),
    )
}

/// Decodes a `tag.getTopTracks` response: one page of the tracks given a tag
/// most, laid out as [`tag_get_top_albums`] is.
///
/// # Errors
///
/// [`ErrorKind::Decode`](crate::ErrorKind) under [the rules every list
/// shares](super::decode#errors). `@attr.tag` is required, and a row needs
/// a `name` and an `artist`.
pub fn tag_get_top_tracks(raw: &Raw) -> Result<Page<Track>, Error> {
    shared::paged(
        raw,
        &methods::TAG_GET_TOP_TRACKS,
        "tracks",
        "track",
        Some("tag"),
        |row, at| entity::track(row, at, Counts::Neither, Length::Read),
    )
}

/// Decodes a `tag.getTopTags` response: the tags of the service, most used
/// first, each with its `taggings` and `reach`.
///
/// The list is read from an offset, not by page. Its `@attr` holds the
/// offset, the number of rows asked for and the number of tags there are;
/// the list keeps none of the three.
///
/// ```
/// use scrobl::model::decode;
/// use scrobl::protocol::{self, HttpResponse, Request, methods};
///
/// let body = r##"{"toptags": {
///     "@attr": {"offset": 0, "num_res": 1, "total": 2896879},
///     "tag": [{"name": "rock", "count": 4077917, "reach": 403467}]
/// }}"##;
/// let request = Request::new(&methods::TAG_GET_TOP_TAGS);
/// let raw = protocol::decode(&request, HttpResponse::new(200, body))?;
///
/// let tags = decode::tag_get_top_tags(&raw)?;
/// assert_eq!(tags[0].name(), "rock");
/// assert_eq!(tags[0].taggings(), Some(4_077_917));
/// # Ok::<(), scrobl::Error>(())
/// ```
///
/// # Errors
///
/// [`ErrorKind::Decode`](crate::ErrorKind) when the response is for another
/// method, when `toptags`, `@attr.offset` or `@attr.total` is missing or
/// malformed, when a row is not an object or lacks a `name`, a `count` or a
/// `reach`, when `tag` is absent or empty although `@attr` puts the offset
/// before the last tag and does not say that none was asked for, or when an
/// object repeats a member name. The message names the field, such as
/// `toptags.tag[3].count`, and never repeats response text.
pub fn tag_get_top_tags(raw: &Raw) -> Result<List<Tag>, Error> {
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
        // As for a page: no rows where `@attr` says there are some is a
        // member gone missing, not an empty list.
        if rows.is_empty() && offset < total && asked != Some(0) {
            return Err(Error::decode_field(
                "toptags.tag",
                "absent or empty, but `@attr` says there are tags to list",
            ));
        }
        let tags = shared::each(rows, "toptags.tag", |row, at| {
            entity::tag(row, at, TagCounts::Count)
        })?;
        Ok(List::new(tags, None))
    })
}

/// Decodes a `tag.getWeeklyChartList` response: the weeks the service keeps
/// charts for, oldest first. The list's subject is the tag.
///
/// The weeks are the service's own, the same for every tag and for a tag
/// nobody has used: on 2026-11-11 it sent the list a user gets.
///
/// # Errors
///
/// [`ErrorKind::Decode`](crate::ErrorKind) under [the rules every list
/// shares](super::decode#errors). `@attr.tag` is required, and a row needs
/// a `from` and a `to` that are numbers.
pub fn tag_get_weekly_chart_list(raw: &Raw) -> Result<List<WeeklyChartRange>, Error> {
    shared::listed(
        raw,
        &methods::TAG_GET_WEEKLY_CHART_LIST,
        "weeklychartlist",
        "chart",
        Some("tag"),
        chart_range,
    )
}

#[cfg(test)]
mod tests;
