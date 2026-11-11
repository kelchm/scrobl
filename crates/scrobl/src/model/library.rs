//! The `library` package: [`LibraryArtistsPage`], the reply to
//! `library.getArtists`.

use std::fmt;

use serde_json::{Map, Value};

use super::shared::{self, Artist, PageAttr};
use crate::error::Error;
use crate::protocol::{Raw, methods};

const ROOT: &str = "artists";

/// One page of `library.getArtists`: every artist a user has played, most
/// played first, with the paging attributes.
///
/// `artist` is an array, a single object when the page has exactly one row,
/// which is how the service answers `limit=1`, and for an empty page
/// absent, `[]` or `""`, under the same rule as
/// [`RecentTracksPage`](super::RecentTracksPage): only when `@attr` allows
/// no rows.
///
/// ```
/// use scrobl::model::LibraryArtistsPage;
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
/// let page = LibraryArtistsPage::decode(&raw)?;
/// assert_eq!(page.attr().total(), 7);
/// assert_eq!(page.artists()[0].artist().name(), "A");
/// assert_eq!(page.artists()[0].playcount(), 42);
/// # Ok::<(), scrobl::Error>(())
/// ```
#[derive(Clone)]
pub struct LibraryArtistsPage {
    attr: PageAttr,
    artists: Vec<LibraryArtist>,
}

impl LibraryArtistsPage {
    /// Decodes a `library.getArtists` response.
    ///
    /// # Errors
    ///
    /// [`ErrorKind::Decode`](crate::ErrorKind) when the response is for
    /// another method, when `artists`, `@attr` or any of its five members is
    /// missing or malformed, when a row is not an object or lacks a `name`
    /// or a `playcount`, when `artist` is absent or empty although `@attr`
    /// promises rows on the page, when an object repeats a member name, or
    /// when a field of a row has the wrong type. The message names the
    /// field, such as `artists.artist[3].playcount`, and never repeats
    /// response text.
    pub fn decode(raw: &Raw) -> Result<Self, Error> {
        shared::decode(raw, &methods::LIBRARY_GET_ARTISTS, ROOT, |mut list| {
            let (attr, rows) = shared::page(&mut list, ROOT, "artist")?;
            let artists = shared::each(rows, "artists.artist", parse_row)?;
            Ok(Self { attr, artists })
        })
    }

    /// The paging attributes.
    pub fn attr(&self) -> &PageAttr {
        &self.attr
    }

    /// The artists, in the order the service sent them.
    pub fn artists(&self) -> &[LibraryArtist] {
        &self.artists
    }
}

impl fmt::Debug for LibraryArtistsPage {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("LibraryArtistsPage")
            .field("attr", &self.attr)
            .field("artists", &self.artists.len())
            .finish()
    }
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
