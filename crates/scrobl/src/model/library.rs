//! The `library` package: the reply to `library.getArtists`.

use std::fmt;

use serde_json::{Map, Value};

use super::Page;
use super::shared::{self, Artist};
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
/// assert_eq!(page[0].playcount(), 42);
/// # Ok::<(), scrobl::Error>(())
/// ```
///
/// # Errors
///
/// [`ErrorKind::Decode`](crate::ErrorKind) under [the rules every list
/// shares](super::decode#errors). `@attr.user` is required, and a row
/// needs a `name` and a `playcount`.
pub fn library_get_artists(raw: &Raw) -> Result<Page<LibraryArtist>, Error> {
    shared::paged(
        raw,
        &methods::LIBRARY_GET_ARTISTS,
        "artists",
        "artist",
        Some("user"),
        parse_row,
    )
}

/// An artist in a user's library, with how often the user played it.
/// [`json`](Self::json) is the row exactly as decoded.
#[derive(Clone)]
pub struct LibraryArtist {
    playcount: u64,
    tagcount: Option<u64>,
    artist: Artist,
    json: Value,
}

impl LibraryArtist {
    /// How many times the user played the artist.
    pub fn playcount(&self) -> u64 {
        self.playcount
    }

    /// How many tags the user gave the artist. `None` when the row has no
    /// `tagcount`.
    pub fn tagcount(&self) -> Option<u64> {
        self.tagcount
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

impl fmt::Debug for LibraryArtist {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("LibraryArtist")
            .field("playcount", &self.playcount)
            .field("tagcount", &self.tagcount)
            .finish_non_exhaustive()
    }
}

fn parse_row(row: Map<String, Value>, at: &str) -> Result<LibraryArtist, Error> {
    Ok(LibraryArtist {
        playcount: shared::required_uint(&row, at, "playcount")?,
        tagcount: shared::optional_uint(&row, at, "tagcount")?,
        artist: shared::parse_artist_row(&row, at)?,
        json: Value::Object(row),
    })
}

#[cfg(test)]
mod tests;
