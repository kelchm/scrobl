//! What a country plays most: the replies to `geo.getTopArtists` and
//! `geo.getTopTracks`.

use std::fmt;

use serde_json::{Map, Value};

use crate::error::Error;
use crate::model::Page;
use crate::model::shared::{self, Artist, Track};
use crate::protocol::{Raw, methods};

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
/// # Ok::<(), scrobl::Error>(())
/// ```
///
/// # Errors
///
/// [`ErrorKind::Decode`](crate::ErrorKind) under [the rules every list
/// shares](super::decode#errors). `@attr.country` is required, and a row
/// needs a `name` and `listeners`.
pub fn geo_get_top_artists(raw: &Raw) -> Result<Page<GeoArtist>, Error> {
    shared::paged(
        raw,
        &methods::GEO_GET_TOP_ARTISTS,
        "topartists",
        "artist",
        Some("country"),
        geo_artist,
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
pub fn geo_get_top_tracks(raw: &Raw) -> Result<Page<GeoTrack>, Error> {
    shared::paged(
        raw,
        &methods::GEO_GET_TOP_TRACKS,
        "tracks",
        "track",
        Some("country"),
        geo_track,
    )
}

/// An artist in a country's chart. [`json`](Self::json) is the row exactly
/// as decoded.
#[derive(Clone)]
pub struct GeoArtist {
    listeners: u64,
    artist: Artist,
    json: Value,
}

impl GeoArtist {
    /// The service's `listeners` for the artist in this chart.
    pub fn listeners(&self) -> u64 {
        self.listeners
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

impl fmt::Debug for GeoArtist {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("GeoArtist")
            .field("listeners", &self.listeners)
            .finish_non_exhaustive()
    }
}

/// A track in a country's chart. [`json`](Self::json) is the row exactly as
/// decoded.
#[derive(Clone)]
pub struct GeoTrack {
    listeners: u64,
    duration: Option<u64>,
    track: Track,
    artist: Artist,
    json: Value,
}

impl GeoTrack {
    /// The service's `listeners` for the track in this chart.
    pub fn listeners(&self) -> u64 {
        self.listeners
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

impl fmt::Debug for GeoTrack {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("GeoTrack")
            .field("listeners", &self.listeners)
            .field("duration", &self.duration)
            .finish_non_exhaustive()
    }
}

fn geo_artist(row: Map<String, Value>, at: &str) -> Result<GeoArtist, Error> {
    Ok(GeoArtist {
        listeners: shared::required_uint(&row, at, "listeners")?,
        artist: shared::parse_artist_row(&row, at)?,
        json: Value::Object(row),
    })
}

fn geo_track(row: Map<String, Value>, at: &str) -> Result<GeoTrack, Error> {
    Ok(GeoTrack {
        listeners: shared::required_uint(&row, at, "listeners")?,
        duration: shared::duration(&row, at)?,
        track: shared::parse_track(&row, at)?,
        artist: shared::parse_artist(&row, at)?,
        json: Value::Object(row),
    })
}

#[cfg(test)]
mod tests;
