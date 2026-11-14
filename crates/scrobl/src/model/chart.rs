//! The charts of the whole service: the replies to `chart.getTopArtists`,
//! `chart.getTopTracks` and `chart.getTopTags`.

use super::entity::{self, Artist, Counts, Length, Tag, TagCounts, Track};
use super::{List, Page, shared};
use crate::error::Error;
use crate::protocol::{Raw, methods};

/// Decodes a `chart.getTopArtists` response: one page of the artists played
/// most on the whole service, each with its `listeners` and `playcount`.
///
/// The rows carry no place in the list: [`Page::ranked`] gives it.
///
/// ```
/// use scrobl::model::decode;
/// use scrobl::protocol::{self, HttpResponse, Request, methods};
///
/// let body = r##"{"artists": {
///     "artist": [{"name": "A", "playcount": "900", "listeners": "30",
///                 "mbid": "", "url": "https://www.last.fm/music/A"}],
///     "@attr": {"page": "1", "perPage": "1", "totalPages": "10000", "total": "10000"}
/// }}"##;
/// let request = Request::new(&methods::CHART_GET_TOP_ARTISTS).param("limit", 1);
/// let raw = protocol::decode(&request, HttpResponse::new(200, body))?;
///
/// let page = decode::chart_get_top_artists(&raw)?;
/// assert_eq!(page.total(), 10_000);
/// assert_eq!(page[0].name(), "A");
/// assert_eq!((page[0].playcount(), page[0].listeners()), (Some(900), Some(30)));
/// # Ok::<(), scrobl::Error>(())
/// ```
///
/// # Errors
///
/// [`ErrorKind::Decode`](crate::ErrorKind) under [the rules every list
/// shares](super::decode#errors). A row needs a `name`, a `playcount` and
/// `listeners`.
pub fn chart_get_top_artists(raw: &Raw) -> Result<Page<Artist>, Error> {
    shared::paged(
        raw,
        &methods::CHART_GET_TOP_ARTISTS,
        "artists",
        "artist",
        None,
        |row, at| entity::artist(row, at, Counts::Both),
    )
}

/// Decodes a `chart.getTopTracks` response: one page of the tracks played
/// most on the whole service, each with its `listeners` and `playcount`.
///
/// # Errors
///
/// [`ErrorKind::Decode`](crate::ErrorKind) under [the rules every list
/// shares](super::decode#errors). A row needs a `name`, an `artist`, a
/// `playcount` and `listeners`.
pub fn chart_get_top_tracks(raw: &Raw) -> Result<Page<Track>, Error> {
    shared::paged(
        raw,
        &methods::CHART_GET_TOP_TRACKS,
        "tracks",
        "track",
        None,
        |row, at| entity::track(row, at, Counts::Both, Length::Read),
    )
}

/// Decodes a `chart.getTopTags` response: the tags used most on the whole
/// service, each with its `taggings` and `reach`.
///
/// It is a [`List`] and not a [`Page`] because the service does not page
/// it: on 2026-11-11 it sent the same first rows for page 2, and for a page
/// far beyond the last, as for page 1. Its paging attributes are not read.
///
/// # Errors
///
/// [`ErrorKind::Decode`](crate::ErrorKind) under [the rules every list
/// shares](super::decode#errors). A row needs a `name`, a `reach` and
/// `taggings`.
pub fn chart_get_top_tags(raw: &Raw) -> Result<List<Tag>, Error> {
    shared::listed(
        raw,
        &methods::CHART_GET_TOP_TAGS,
        "tags",
        "tag",
        None,
        |row, at| entity::tag(row, at, TagCounts::Taggings),
    )
}

#[cfg(test)]
mod tests;
