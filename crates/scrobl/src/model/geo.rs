//! What a country plays most: the replies to `geo.getTopArtists` and
//! `geo.getTopTracks`.

use serde_json::{Map, Value};

use super::entity::{self, Artist, Counts, Length, Track, counted_row};
use super::{Page, shared};
use crate::error::Error;
use crate::protocol::{Raw, methods};

counted_row! {
    /// An artist in a country's chart, with how many listeners it has there.
    CountryArtist {
        /// How many listeners the artist has in the country. This is the
        /// row's own number: the artist's `listeners` everywhere are not in
        /// this reply.
        listeners,
        artist / into_artist: Artist
    }
}

counted_row! {
    /// A track in a country's chart, with how many listeners it has there.
    CountryTrack {
        /// How many listeners the track has in the country. This is the
        /// row's own number: the track's `listeners` everywhere are not in
        /// this reply.
        listeners,
        track / into_track: Track
    }
}

/// Decodes a `geo.getTopArtists` response: one page of the artists a country
/// plays most. The page's subject is the country, as the service spells it.
///
/// The `@attr.rank` of a row is not read: the service counts it from the top
/// of each page, not of the list. [`Page::ranked`] gives a row's place.
///
/// ```
/// use scrobl::model::decode;
/// use scrobl::protocol::{self, HttpResponse, Request, methods};
///
/// let body = r##"{"topartists": {
///     "artist": [{"name": "A", "listeners": "30", "mbid": "",
///                 "url": "https://www.last.fm/music/A", "@attr": {"rank": "1"}}],
///     "@attr": {"country": "Germany", "page": "1", "perPage": "1",
///               "totalPages": "2650", "total": "2650"}
/// }}"##;
/// let request = Request::new(&methods::GEO_GET_TOP_ARTISTS).param("country", "germany");
/// let raw = protocol::decode(&request, HttpResponse::new(200, body))?;
///
/// let page = decode::geo_get_top_artists(&raw)?;
/// assert_eq!(page.subject(), Some("Germany"));
/// assert_eq!(page[0].artist().name(), "A");
/// assert_eq!(page[0].listeners(), 30);
/// // The count is the country's, so the artist's own stays unsaid.
/// assert_eq!(page[0].artist().listeners(), None);
/// # Ok::<(), scrobl::Error>(())
/// ```
///
/// # Errors
///
/// [`ErrorKind::Decode`](crate::ErrorKind) under [the rules every list
/// shares](super::decode#errors). `@attr.country` is required, and a row
/// needs a `name` and `listeners`.
pub fn geo_get_top_artists(raw: &Raw) -> Result<Page<CountryArtist>, Error> {
    shared::paged(
        raw,
        &methods::GEO_GET_TOP_ARTISTS,
        "topartists",
        "artist",
        Some("country"),
        country_artist,
    )
}

/// Decodes a `geo.getTopTracks` response: one page of the tracks a country
/// plays most. The page's subject is the country. A row's `@attr.rank`,
/// which the service counts from 0 on each page, is not read.
///
/// # Errors
///
/// [`ErrorKind::Decode`](crate::ErrorKind) under [the rules every list
/// shares](super::decode#errors). `@attr.country` is required, and a row
/// needs a `name`, an `artist` and `listeners`.
pub fn geo_get_top_tracks(raw: &Raw) -> Result<Page<CountryTrack>, Error> {
    shared::paged(
        raw,
        &methods::GEO_GET_TOP_TRACKS,
        "tracks",
        "track",
        Some("country"),
        country_track,
    )
}

fn country_artist(row: Map<String, Value>, at: &str) -> Result<CountryArtist, Error> {
    Ok(CountryArtist {
        listeners: shared::required_uint(&row, at, "listeners")?,
        artist: entity::artist(row, at, Counts::Neither)?,
    })
}

fn country_track(row: Map<String, Value>, at: &str) -> Result<CountryTrack, Error> {
    Ok(CountryTrack {
        listeners: shared::required_uint(&row, at, "listeners")?,
        track: entity::track(row, at, Counts::Neither, Length::Read)?,
    })
}

#[cfg(test)]
mod tests;
