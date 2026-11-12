//! Tracks: the replies to `track.getCorrection`, `track.getInfo`,
//! `track.getSimilar`, `track.getTags` and `track.getTopTags`.

use std::fmt;

use serde_json::Value;

use super::List;
use super::artist::{TopTag, top_tag};
use super::entity::{self, Counts, Length, Tag, TagCounts, Track};
use super::shared::{self, Wiki, missing_or};
use crate::de;
use crate::error::Error;
use crate::protocol::{Raw, methods};

/// A track the service finds like another, with how alike it finds them.
#[derive(Clone)]
pub struct SimilarTrack {
    similarity: f64,
    track: Track,
}

impl SimilarTrack {
    /// How alike the service finds the two, as it sends it under `match`:
    /// 1 for the most alike, down towards 0, in every response recorded.
    pub fn similarity(&self) -> f64 {
        self.similarity
    }

    /// The track, with its `playcount` for everyone and, where the service
    /// knows it, its length.
    pub fn track(&self) -> &Track {
        &self.track
    }

    /// The track, to keep.
    pub fn into_track(self) -> Track {
        self.track
    }

    /// The row exactly as decoded.
    pub fn json(&self) -> &Value {
        self.track.json()
    }
}

impl AsRef<Track> for SimilarTrack {
    fn as_ref(&self) -> &Track {
        &self.track
    }
}

impl fmt::Debug for SimilarTrack {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("SimilarTrack")
            .field("similarity", &self.similarity)
            .finish_non_exhaustive()
    }
}

/// What the service says about one track: the reply to `track.getInfo`.
/// [`json`](Self::json) is the track exactly as decoded.
///
/// ```
/// use std::time::Duration;
///
/// use scrobl::model::decode;
/// use scrobl::protocol::{self, HttpResponse, Request, methods};
///
/// let body = r##"{"track": {
///     "name": "T", "url": "https://www.last.fm/music/A/_/T", "duration": "237000",
///     "listeners": "30", "playcount": "900",
///     "artist": {"name": "A", "url": "https://www.last.fm/music/A"},
///     "album": {"artist": "A", "title": "B", "url": "https://www.last.fm/music/A/B"},
///     "userplaycount": "10", "userloved": "1",
///     "toptags": {"tag": [{"name": "rock", "url": "https://www.last.fm/tag/rock"}]}
/// }}"##;
/// let request = Request::new(&methods::TRACK_GET_INFO).param("artist", "a").param("track", "t");
/// let raw = protocol::decode(&request, HttpResponse::new(200, body))?;
///
/// let info = decode::track_get_info(&raw)?;
/// assert_eq!(info.track().name(), "T");
/// assert_eq!(info.track().duration(), Some(Duration::from_secs(237)));
/// assert_eq!(info.track().album().map(|album| album.name()), Some("B"));
/// assert_eq!((info.listeners(), info.playcount()), (30, 900));
/// assert_eq!((info.user_playcount(), info.user_loved()), (Some(10), Some(true)));
/// assert_eq!(info.tags()[0].name(), "rock");
/// # Ok::<(), scrobl::Error>(())
/// ```
#[derive(Clone)]
pub struct TrackInfo {
    track: Track,
    listeners: u64,
    playcount: u64,
    user_playcount: Option<u64>,
    user_loved: Option<bool>,
    tags: Vec<Tag>,
    wiki: Wiki,
}

impl TrackInfo {
    /// The track, as the service spells its name, with its artist, its
    /// counts for everyone, its length where the service knows it, and the
    /// album the service files it on where it has one.
    pub fn track(&self) -> &Track {
        &self.track
    }

    /// The track, to keep.
    pub fn into_track(self) -> Track {
        self.track
    }

    /// How many users have played the track.
    pub fn listeners(&self) -> u64 {
        self.listeners
    }

    /// How many times the track has been played, by everyone.
    pub fn playcount(&self) -> u64 {
        self.playcount
    }

    /// How many times one user has played the track. `None` unless the
    /// call named a user; 0 for a user who never has.
    pub fn user_playcount(&self) -> Option<u64> {
        self.user_playcount
    }

    /// Whether one user has loved the track. `None` unless the call named
    /// a user.
    pub fn user_loved(&self) -> Option<bool> {
        self.user_loved
    }

    /// A few of the tags given to the track most: no more than five in
    /// any response recorded, and often none.
    pub fn tags(&self) -> &[Tag] {
        &self.tags
    }

    /// The start of what the service's users wrote about the track, if
    /// anything. It can hold HTML, and the service ends it with a link to
    /// its own page.
    pub fn summary(&self) -> Option<&str> {
        self.wiki.summary.as_deref()
    }

    /// All of what the service's users wrote about the track, if
    /// anything. It can hold HTML.
    pub fn content(&self) -> Option<&str> {
        self.wiki.content.as_deref()
    }

    /// When that text was last changed, as the service words it, such as
    /// `18 Jul 2008, 15:22`.
    pub fn published(&self) -> Option<&str> {
        self.wiki.published.as_deref()
    }

    /// The track exactly as decoded.
    pub fn json(&self) -> &Value {
        self.track.json()
    }
}

impl AsRef<Track> for TrackInfo {
    fn as_ref(&self) -> &Track {
        &self.track
    }
}

impl fmt::Debug for TrackInfo {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("TrackInfo")
            .field("listeners", &self.listeners)
            .field("playcount", &self.playcount)
            .field("user_playcount", &self.user_playcount)
            .field("user_loved", &self.user_loved)
            .field("tags", &self.tags.len())
            .finish_non_exhaustive()
    }
}

/// Decodes a `track.getInfo` response.
///
/// This reply gives the track's length in milliseconds, where every list
/// gives seconds, and names the track's album with a `title`.
///
/// # Errors
///
/// [`ErrorKind::Decode`](crate::ErrorKind) when the response is for another
/// method, when `track`, its `name`, its `listeners`, its `playcount` or
/// its `artist` with a name is missing or malformed, when a tag has no
/// `name`, when an object repeats a member name, or when another field has
/// the wrong type. The message names the field, such as
/// `track.userplaycount`, and never repeats response text.
pub fn track_get_info(raw: &Raw) -> Result<TrackInfo, Error> {
    const AT: &str = "track";
    shared::decode(raw, &methods::TRACK_GET_INFO, AT, |track| {
        let user_playcount = shared::optional_uint(&track, AT, "userplaycount")?;
        let user_loved = entity::optional_flag(&track, AT, "userloved")?;
        let tags = shared::nested(&track, AT, "toptags", "tag", |row, at| {
            entity::tag(row, at, TagCounts::Neither)
        })?;
        let wiki = shared::wiki(&track, AT, "wiki")?;
        let track = entity::track(track, AT, Counts::Both, Length::Millis)?;
        Ok(TrackInfo {
            // `Counts::Both` makes both required, so both are there.
            listeners: track.listeners().unwrap_or_default(),
            playcount: track.playcount().unwrap_or_default(),
            track,
            user_playcount,
            user_loved,
            tags,
            wiki,
        })
    })
}

/// Decodes a `track.getCorrection` response: the track the service files
/// an artist's name and a track's name under. Names that are already right
/// come back as themselves. A track the service does not know is an error
/// of the call, not a reply.
///
/// On 2026-11-12 the service corrected the artist's name and the letter
/// case of the track's, and nothing else of the track's name.
///
/// # Errors
///
/// [`ErrorKind::Decode`](crate::ErrorKind) when the response is for another
/// method, when `corrections.correction.track` is missing or not an
/// object, when it has no `name` or no `artist` with a name, or when an
/// object repeats a member name. The message names the field and never
/// repeats response text.
pub fn track_get_correction(raw: &Raw) -> Result<Track, Error> {
    shared::decode(
        raw,
        &methods::TRACK_GET_CORRECTION,
        "corrections",
        |mut corrections| {
            const AT: &str = "corrections.correction.track";
            let track = match corrections.remove("correction") {
                Some(Value::Object(mut correction)) => correction.remove("track"),
                _ => return Err(missing_or("corrections.correction", "an object")),
            };
            match track {
                Some(Value::Object(track)) => {
                    entity::track(track, AT, Counts::Neither, Length::Read)
                }
                _ => Err(missing_or(AT, "an object")),
            }
        },
    )
}

/// Decodes a `track.getSimilar` response: the tracks the service finds
/// like one track, most alike first. The list's subject is that track, as
/// the service spells its name.
///
/// Each track has its `playcount` for everyone and no `listeners`. The
/// service sends 100 rows when no limit is set, and sent 217 when asked
/// for 1000. A track it knows little about has none.
///
/// # Errors
///
/// [`ErrorKind::Decode`](crate::ErrorKind) under [the rules every list
/// shares](super::decode#errors). `@attr.track` is required, and a row
/// needs a `name`, a `playcount`, an `artist` with a name and a `match`
/// that is a decimal and not negative.
pub fn track_get_similar(raw: &Raw) -> Result<List<SimilarTrack>, Error> {
    shared::listed(
        raw,
        &methods::TRACK_GET_SIMILAR,
        "similartracks",
        "track",
        Some("track"),
        |row, at| {
            let similarity = row
                .get("match")
                .and_then(de::decimal)
                .ok_or_else(|| missing_or(&format!("{at}.match"), "a decimal"))?;
            Ok(SimilarTrack {
                similarity,
                track: entity::track(row, at, Counts::Playcount, Length::Read)?,
            })
        },
    )
}

/// Decodes a `track.getTags` response: the tags one user has given a
/// track. The list's subject is the track, as the service spells its name.
/// A user who has given none is an empty list.
///
/// # Errors
///
/// [`ErrorKind::Decode`](crate::ErrorKind) under [the rules every list
/// shares](super::decode#errors). `@attr.track` is required, and a row
/// needs a `name`.
pub fn track_get_tags(raw: &Raw) -> Result<List<Tag>, Error> {
    shared::listed(
        raw,
        &methods::TRACK_GET_TAGS,
        "tags",
        "tag",
        Some("track"),
        |row, at| entity::tag(row, at, TagCounts::Neither),
    )
}

/// Decodes a `track.getTopTags` response: the tags given to a track most.
/// The list's subject is the track, as the service spells its name.
///
/// # Errors
///
/// [`ErrorKind::Decode`](crate::ErrorKind) under [the rules every list
/// shares](super::decode#errors). `@attr.track` is required, and a row
/// needs a `name` and a `count`.
pub fn track_get_top_tags(raw: &Raw) -> Result<List<TopTag>, Error> {
    shared::listed(
        raw,
        &methods::TRACK_GET_TOP_TAGS,
        "toptags",
        "tag",
        Some("track"),
        top_tag,
    )
}

#[cfg(test)]
mod tests;
