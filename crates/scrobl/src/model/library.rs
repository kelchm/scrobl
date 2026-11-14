//! The `library` package: the reply to `library.getArtists`.

use super::user::user_artist;
use super::{Page, UserArtist, shared};
use crate::error::Error;
use crate::protocol::{Raw, methods};

/// Decodes a `library.getArtists` response: one page of every artist a user
/// has played, most played first. The page's subject is the user.
///
/// `artist` is a single object when the page has exactly one row, which is
/// how the service answers `limit=1`.
///
/// ```
/// use scrobl::model::decode;
/// use scrobl::protocol::{self, HttpResponse, Request, methods};
///
/// let body = r##"{"artists": {
///     "artist": {"name": "A", "mbid": "", "url": "https://www.last.fm/music/A",
///                "playcount": "42", "tagcount": "0"},
///     "@attr": {"user": "rj", "page": "1", "perPage": "1",
///               "totalPages": "7", "total": "7"}
/// }}"##;
/// let request = Request::new(&methods::LIBRARY_GET_ARTISTS).param("user", "rj");
/// let raw = protocol::decode(&request, HttpResponse::new(200, body))?;
///
/// let page = decode::library_get_artists(&raw)?;
/// assert_eq!(page.total(), 7);
/// assert_eq!(page[0].artist().name(), "A");
/// assert_eq!((page[0].playcount(), page[0].tagcount()), (42, Some(0)));
/// # Ok::<(), scrobl::Error>(())
/// ```
///
/// # Errors
///
/// [`ErrorKind::Decode`](crate::ErrorKind) under [the rules every list
/// shares](super::decode#errors). `@attr.user` is required, and a row needs
/// a `name` and a `playcount`.
pub fn library_get_artists(raw: &Raw) -> Result<Page<UserArtist>, Error> {
    shared::paged(
        raw,
        &methods::LIBRARY_GET_ARTISTS,
        "artists",
        "artist",
        Some("user"),
        user_artist,
    )
}

#[cfg(test)]
mod tests;
