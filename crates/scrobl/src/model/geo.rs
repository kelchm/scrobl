//! What a country plays most: the replies to `geo.getTopArtists` and
//! `geo.getTopTracks`.

use std::fmt;

use serde_json::{Map, Value};

use crate::error::Error;
use crate::model::shared::{self, Artist, Track, list_page};

list_page! {
    /// One page of `geo.getTopArtists`: the artists a country plays most,
    /// with the paging attributes.
    ///
    /// `artist` is an array, for a page of one too, and `[]` for a page
    /// beyond the last. The `@attr.rank` of a row is not read: the service
    /// counts it from the top of each page, not of the list, so a row's
    /// place is its position, counted on from the pages before it.
    ///
    /// ```
    /// use scrobl::model::GeoArtistsPage;
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
    /// let page = GeoArtistsPage::decode(&raw)?;
    /// assert_eq!(page.country(), "Germany");
    /// assert_eq!(page.artists()[0].artist().name(), "A");
    /// assert_eq!(page.artists()[0].listeners(), 30);
    /// # Ok::<(), scrobl::Error>(())
    /// ```
    GeoArtistsPage, "geo.getTopArtists", GEO_GET_TOP_ARTISTS, "topartists", "artist",
    @of country = "country", "The country, as the service spells it back.";
    artists: GeoArtist = geo_artist
}

list_page! {
    /// One page of `geo.getTopTracks`: the tracks a country plays most,
    /// with the paging attributes. It is laid out as a [`GeoArtistsPage`]
    /// is, and its rows' `@attr.rank`, which the service counts from 0 on
    /// each page, is not read either.
    GeoTracksPage, "geo.getTopTracks", GEO_GET_TOP_TRACKS, "tracks", "track",
    @of country = "country", "The country, as the service spells it back.";
    tracks: GeoTrack = geo_track
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
