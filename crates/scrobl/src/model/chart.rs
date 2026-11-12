//! The charts of the whole service: the replies to `chart.getTopArtists`,
//! `chart.getTopTracks` and `chart.getTopTags`.

use std::fmt;

use serde_json::{Map, Value};

use crate::error::Error;
use crate::model::shared::{self, Artist, Track};
use crate::model::{List, Page};
use crate::protocol::{Raw, methods};

/// Decodes a `chart.getTopArtists` response: one page of the artists played
/// most on the whole service.
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
/// assert_eq!(page[0].artist().name(), "A");
/// assert_eq!((page[0].playcount(), page[0].listeners()), (900, 30));
/// # Ok::<(), scrobl::Error>(())
/// ```
///
/// # Errors
///
/// [`ErrorKind::Decode`](crate::ErrorKind) under [the rules every list
/// shares](super::decode#errors). A row needs a `name`, a `playcount` and
/// `listeners`.
pub fn chart_get_top_artists(raw: &Raw) -> Result<Page<ChartArtist>, Error> {
    shared::paged(
        raw,
        &methods::CHART_GET_TOP_ARTISTS,
        "artists",
        "artist",
        None,
        chart_artist,
    )
}

/// Decodes a `chart.getTopTracks` response: one page of the tracks played
/// most on the whole service.
///
/// # Errors
///
/// [`ErrorKind::Decode`](crate::ErrorKind) under [the rules every list
/// shares](super::decode#errors). A row needs a `name`, an `artist`, a
/// `playcount` and `listeners`.
pub fn chart_get_top_tracks(raw: &Raw) -> Result<Page<ChartTrack>, Error> {
    shared::paged(
        raw,
        &methods::CHART_GET_TOP_TRACKS,
        "tracks",
        "track",
        None,
        chart_track,
    )
}

/// Decodes a `chart.getTopTags` response: the tags used most on the whole
/// service.
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
pub fn chart_get_top_tags(raw: &Raw) -> Result<List<ChartTag>, Error> {
    shared::listed(
        raw,
        &methods::CHART_GET_TOP_TAGS,
        "tags",
        "tag",
        None,
        chart_tag,
    )
}

/// An artist in the chart of the whole service. [`json`](Self::json) is the
/// row exactly as decoded.
#[derive(Clone)]
pub struct ChartArtist {
    playcount: u64,
    listeners: u64,
    artist: Artist,
    json: Value,
}

impl ChartArtist {
    /// How many times the service says the artist has been played.
    pub fn playcount(&self) -> u64 {
        self.playcount
    }

    /// How many users the service says have played the artist.
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

impl fmt::Debug for ChartArtist {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ChartArtist")
            .field("playcount", &self.playcount)
            .field("listeners", &self.listeners)
            .finish_non_exhaustive()
    }
}

/// A track in the chart of the whole service. [`json`](Self::json) is the
/// row exactly as decoded.
#[derive(Clone)]
pub struct ChartTrack {
    playcount: u64,
    listeners: u64,
    duration: Option<u64>,
    track: Track,
    artist: Artist,
    json: Value,
}

impl ChartTrack {
    /// How many times the service says the track has been played.
    pub fn playcount(&self) -> u64 {
        self.playcount
    }

    /// How many users the service says have played the track.
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

impl fmt::Debug for ChartTrack {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ChartTrack")
            .field("playcount", &self.playcount)
            .field("listeners", &self.listeners)
            .field("duration", &self.duration)
            .finish_non_exhaustive()
    }
}

/// A tag in the chart of the whole service. [`json`](Self::json) is the row
/// exactly as decoded.
#[derive(Clone)]
pub struct ChartTag {
    name: String,
    url: Option<String>,
    reach: u64,
    taggings: u64,
    json: Value,
}

impl ChartTag {
    /// The tag.
    pub fn name(&self) -> &str {
        &self.name
    }

    /// The Last.fm page of the tag, if the service sent a non-empty one.
    pub fn url(&self) -> Option<&str> {
        self.url.as_deref()
    }

    /// The service's `reach` of the tag: how many users have used it.
    pub fn reach(&self) -> u64 {
        self.reach
    }

    /// The service's `taggings`: how many times the tag has been used.
    pub fn taggings(&self) -> u64 {
        self.taggings
    }

    /// The row exactly as decoded.
    pub fn json(&self) -> &Value {
        &self.json
    }
}

impl fmt::Debug for ChartTag {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ChartTag")
            .field("reach", &self.reach)
            .field("taggings", &self.taggings)
            .finish_non_exhaustive()
    }
}

fn chart_artist(row: Map<String, Value>, at: &str) -> Result<ChartArtist, Error> {
    Ok(ChartArtist {
        playcount: shared::required_uint(&row, at, "playcount")?,
        listeners: shared::required_uint(&row, at, "listeners")?,
        artist: shared::parse_artist_row(&row, at)?,
        json: Value::Object(row),
    })
}

fn chart_track(row: Map<String, Value>, at: &str) -> Result<ChartTrack, Error> {
    Ok(ChartTrack {
        playcount: shared::required_uint(&row, at, "playcount")?,
        listeners: shared::required_uint(&row, at, "listeners")?,
        duration: shared::duration(&row, at)?,
        track: shared::parse_track(&row, at)?,
        artist: shared::parse_artist(&row, at)?,
        json: Value::Object(row),
    })
}

fn chart_tag(row: Map<String, Value>, at: &str) -> Result<ChartTag, Error> {
    Ok(ChartTag {
        name: shared::required_str(&row, at, "name")?.to_owned(),
        url: shared::optional_str(&row, at, "url")?,
        reach: shared::required_uint(&row, at, "reach")?,
        taggings: shared::required_uint(&row, at, "taggings")?,
        json: Value::Object(row),
    })
}

#[cfg(test)]
mod tests;
