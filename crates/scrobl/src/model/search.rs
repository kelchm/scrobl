//! Searches: the replies to `artist.search` and `album.search`.
//!
//! A search answers in a shape of its own. Where a list has `@attr` with
//! four paging numbers, a search has three members named after OpenSearch
//! and its matches one level further down.

use serde_json::{Map, Value};

use super::Page;
use super::entity::{self, Album, Artist, Counts};
use super::page::Paging;
use super::shared::{self, missing_or};
use crate::error::Error;
use crate::protocol::{MethodSpec, Raw, methods};

const ROOT: &str = "results";

/// One page of what a search found: the rows at `results.<matches>.<key>`.
fn found<T>(
    raw: &Raw,
    spec: &'static MethodSpec,
    matches: &str,
    key: &str,
    parse: impl Fn(Map<String, Value>, &str) -> Result<T, Error>,
) -> Result<Page<T>, Error> {
    shared::decode(raw, spec, ROOT, |mut results| {
        let paging = paging(&results)?;
        let path = format!("{ROOT}.{matches}.{key}");
        let mut matches = match results.remove(matches) {
            Some(Value::Object(matches)) => matches,
            _ => return Err(missing_or(&format!("{ROOT}.{matches}"), "an object")),
        };
        let rows = shared::promised_rows(&mut matches, &paging, &path, key)?;
        Ok(Page::new(shared::each(rows, &path, parse)?, paging))
    })
}

/// Where a page of results stands. The service sends the page's number,
/// its size and the number of matches. The number of pages is worked out
/// from the last two.
fn paging(results: &Map<String, Value>) -> Result<Paging, Error> {
    let query = "results.opensearch:Query";
    let number = match results.get("opensearch:Query") {
        Some(Value::Object(asked)) => shared::required_u32(asked, query, "startPage")?,
        _ => return Err(missing_or(query, "an object")),
    };
    let per_page = shared::required_u32(results, ROOT, "opensearch:itemsPerPage")?;
    if per_page == 0 {
        // No number of pages can be worked out from a page of nothing.
        return Err(Error::decode_field(
            "results.opensearch:itemsPerPage",
            "a page size of 0",
        ));
    }
    let total = shared::required_uint(results, ROOT, "opensearch:totalResults")?;
    let total_pages = u32::try_from(total.div_ceil(u64::from(per_page))).unwrap_or(u32::MAX);
    Ok(Paging {
        number,
        per_page,
        total_pages,
        total,
        subject: Some(shared::owner(results, ROOT, "for")?),
    })
}

/// Decodes an `artist.search` response: one page of the artists a search
/// found, the best match first. The page's subject is what was searched
/// for.
///
/// Each artist has its `listeners` for everyone and no `playcount`.
/// [`total_pages`](Page::total_pages) is worked out from the number of
/// matches and the page size, which is all the service sends. It answers a
/// page beyond 10000 with an error, however many matches there are.
///
/// ```
/// use scrobl::model::decode;
/// use scrobl::protocol::{self, HttpResponse, Request, methods};
///
/// let body = r##"{"results": {
///     "opensearch:Query": {"#text": "", "role": "request", "searchTerms": "a", "startPage": "2"},
///     "opensearch:totalResults": "9", "opensearch:startIndex": "2",
///     "opensearch:itemsPerPage": "2",
///     "artistmatches": {"artist": [{"name": "A", "listeners": "30", "mbid": "", "url": ""}]},
///     "@attr": {"for": "a"}
/// }}"##;
/// let request = Request::new(&methods::ARTIST_SEARCH).param("artist", "a");
/// let raw = protocol::decode(&request, HttpResponse::new(200, body))?;
///
/// let page = decode::artist_search(&raw)?;
/// assert_eq!((page.number(), page.per_page(), page.total()), (2, 2, 9));
/// assert_eq!((page.total_pages(), page.next_page()), (5, Some(3)));
/// assert_eq!(page.subject(), Some("a"));
/// assert_eq!((page[0].name(), page[0].listeners()), ("A", Some(30)));
/// # Ok::<(), scrobl::Error>(())
/// ```
///
/// # Errors
///
/// [`ErrorKind::Decode`](crate::ErrorKind) when the response is for another
/// method, when `results`, its `opensearch:Query.startPage`, its
/// `opensearch:itemsPerPage`, its `opensearch:totalResults`, its
/// `@attr.for` or its `artistmatches` is missing or malformed, when the
/// page size is 0, when a row
/// is not an object or lacks a `name` or `listeners`, when there are no
/// rows on a page that the numbers say holds some, or when an object
/// repeats a member name. The message names the field and never repeats
/// response text.
pub fn artist_search(raw: &Raw) -> Result<Page<Artist>, Error> {
    found(
        raw,
        &methods::ARTIST_SEARCH,
        "artistmatches",
        "artist",
        |row, at| entity::artist(row, at, Counts::Listeners),
    )
}

/// Decodes an `album.search` response: one page of the albums a search
/// found, the best match first. The page's subject is what was searched
/// for.
///
/// A match names its artist with a string and nothing else, and has no
/// counts. The paging is as for [`artist_search`].
///
/// # Errors
///
/// As [`artist_search`], with `albummatches` for `artistmatches`. A row
/// needs a `name` and an `artist` that is a string and not empty.
pub fn album_search(raw: &Raw) -> Result<Page<Album>, Error> {
    found(
        raw,
        &methods::ALBUM_SEARCH,
        "albummatches",
        "album",
        |row, at| entity::album_by_name(row, at, Counts::Neither),
    )
}

#[cfg(test)]
mod tests;
