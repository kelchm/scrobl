//! Scrobbling: `track.scrobble` and `track.updateNowPlaying`.

use std::fmt;

use serde_json::{Map, Value};

use super::shared::{self, missing_or};
use crate::de;
use crate::error::Error;
use crate::protocol::{Raw, methods};

/// What the service did with a batch of scrobbles: one [`Outcome`] for each
/// play sent, in the order sent.
#[derive(Clone)]
pub struct ScrobbleReport {
    outcomes: Vec<Outcome>,
}

impl ScrobbleReport {
    /// One for each play, in the order sent.
    pub fn outcomes(&self) -> &[Outcome] {
        &self.outcomes
    }

    /// How many were recorded.
    pub fn accepted(&self) -> usize {
        self.outcomes.iter().filter(|o| o.is_accepted()).count()
    }

    /// How many were not.
    pub fn ignored(&self) -> usize {
        self.outcomes.len() - self.accepted()
    }
}

impl fmt::Debug for ScrobbleReport {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ScrobbleReport")
            .field("accepted", &self.accepted())
            .field("ignored", &self.ignored())
            .finish()
    }
}

/// What the service did with one play: the names as it recorded them, and
/// why it ignored the play if it did.
#[derive(Clone)]
pub struct Outcome {
    artist: Corrected,
    track: Corrected,
    album: Option<Corrected>,
    album_artist: Option<Corrected>,
    timestamp: Option<u64>,
    ignored: Option<Ignored>,
}

impl Outcome {
    /// Whether the play was recorded.
    pub fn is_accepted(&self) -> bool {
        self.ignored.is_none()
    }

    /// Why the play was not recorded.
    pub fn ignored(&self) -> Option<&Ignored> {
        self.ignored.as_ref()
    }

    /// The artist as the service recorded it.
    pub fn artist(&self) -> &Corrected {
        &self.artist
    }

    /// The track as the service recorded it.
    pub fn track(&self) -> &Corrected {
        &self.track
    }

    /// The album, if one was sent.
    pub fn album(&self) -> Option<&Corrected> {
        self.album.as_ref()
    }

    /// The album's artist, if one was sent.
    pub fn album_artist(&self) -> Option<&Corrected> {
        self.album_artist.as_ref()
    }

    /// When the play started, in seconds since the Unix epoch. `None` for
    /// now playing.
    pub fn timestamp(&self) -> Option<u64> {
        self.timestamp
    }
}

impl fmt::Debug for Outcome {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Outcome")
            .field("timestamp", &self.timestamp)
            .field("ignored", &self.ignored.as_ref().map(Ignored::code))
            .finish_non_exhaustive()
    }
}

/// A name as the service recorded it.
#[derive(Clone)]
pub struct Corrected {
    name: String,
    corrected: bool,
}

impl Corrected {
    /// The name.
    pub fn name(&self) -> &str {
        &self.name
    }

    /// Whether the service changed the name that was sent.
    pub fn corrected(&self) -> bool {
        self.corrected
    }
}

impl fmt::Debug for Corrected {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Corrected")
            .field("corrected", &self.corrected)
            .finish_non_exhaustive()
    }
}

/// Why the service did not record a play.
#[derive(Clone)]
pub struct Ignored {
    code: IgnoredCode,
    message: String,
}

impl Ignored {
    /// The reason, as a code.
    pub fn code(&self) -> IgnoredCode {
        self.code
    }

    /// The service's message. Text from the network.
    pub fn message(&self) -> &str {
        &self.message
    }
}

impl fmt::Debug for Ignored {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_tuple("Ignored").field(&self.code).finish()
    }
}

/// Why a play was ignored. Codes this library does not know are kept.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct IgnoredCode(u32);

impl IgnoredCode {
    /// 1: the artist is filtered. The service also gave it for plays from
    /// 2001, where 3 was documented.
    pub const ARTIST: Self = Self(1);
    /// 2: the track is filtered.
    pub const TRACK: Self = Self(2);
    /// 3: the timestamp is too far in the past.
    pub const TOO_OLD: Self = Self(3);
    /// 4: the timestamp is too far in the future.
    pub const TOO_NEW: Self = Self(4);
    /// 5: the user's daily scrobble limit is reached.
    pub const DAILY_LIMIT: Self = Self(5);

    /// The number the service sent.
    pub const fn get(self) -> u32 {
        self.0
    }
}

/// Decodes a `track.scrobble` response and checks it against the request:
/// one outcome for each play sent, with its timestamp, and `accepted` plus
/// `ignored` equal to the batch. The service lists accepted plays first;
/// the outcomes are put back in the order sent.
///
/// # Errors
///
/// [`ErrorKind::Decode`](crate::ErrorKind) for a malformed reply, and
/// [`ErrorKind::Inconsistent`](crate::ErrorKind) for one that does not
/// match the request. Either way the error's
/// [`delivery`](crate::Error::delivery) is unknown.
pub fn track_scrobble(raw: &Raw) -> Result<ScrobbleReport, Error> {
    shared::decode(raw, &methods::TRACK_SCROBBLE, "scrobbles", |mut list| {
        let sent = sent_timestamps(raw)?;
        let attr = match list.remove("@attr") {
            Some(Value::Object(attr)) => attr,
            _ => return Err(missing_or("scrobbles.@attr", "an object")),
        };
        let accepted = shared::required_uint(&attr, "scrobbles.@attr", "accepted")?;
        let ignored = shared::required_uint(&attr, "scrobbles.@attr", "ignored")?;
        let rows = list
            .remove("scrobble")
            .and_then(de::one_or_many)
            .ok_or_else(|| missing_or("scrobbles.scrobble", "an object or an array"))?;
        let outcomes = shared::each(rows, "scrobbles.scrobble", |row, at| {
            outcome(&row, at, true)
        })?;
        if outcomes.len() != sent.len() {
            return Err(Error::inconsistent("one outcome for each play sent"));
        }
        let report = ScrobbleReport {
            outcomes: in_sent_order(outcomes, &sent)?,
        };
        if accepted != report.accepted() as u64 || ignored != report.ignored() as u64 {
            return Err(Error::inconsistent(
                "`accepted` and `ignored` add up to the batch",
            ));
        }
        Ok(report)
    })
}

/// Decodes a `track.updateNowPlaying` response.
///
/// # Errors
///
/// [`ErrorKind::Decode`](crate::ErrorKind) for a malformed reply. Its
/// [`delivery`](crate::Error::delivery) is unknown.
pub fn track_update_now_playing(raw: &Raw) -> Result<Outcome, Error> {
    shared::decode(
        raw,
        &methods::TRACK_UPDATE_NOW_PLAYING,
        "nowplaying",
        |row| outcome(&row, "nowplaying", false),
    )
}

/// Matches each play to an outcome with its timestamp. Plays sharing a
/// timestamp keep the reply's order.
fn in_sent_order(mut outcomes: Vec<Outcome>, sent: &[u64]) -> Result<Vec<Outcome>, Error> {
    let mut ordered = Vec::with_capacity(sent.len());
    for &timestamp in sent {
        let at = outcomes
            .iter()
            .position(|o| o.timestamp == Some(timestamp))
            .ok_or_else(|| {
                Error::inconsistent("each play sent has an outcome with its timestamp")
            })?;
        ordered.push(outcomes.remove(at));
    }
    Ok(ordered)
}

/// The timestamps the request sent, in batch order. Every play must have
/// one, numbered from 0 with no gap.
fn sent_timestamps(raw: &Raw) -> Result<Vec<u64>, Error> {
    let params = raw.request().params();
    let plays = params
        .iter()
        .filter(|(name, _)| name.starts_with("artist["))
        .count();
    let sent: Option<Vec<u64>> = (0..plays)
        .map(|i| raw.request().get(&format!("timestamp[{i}]"))?.parse().ok())
        .collect();
    let stamped = params
        .iter()
        .filter(|(name, _)| name.starts_with("timestamp["))
        .count();
    match sent {
        Some(sent) if stamped == plays => Ok(sent),
        _ => Err(Error::inconsistent(
            "the request numbers its plays from 0, each with a timestamp",
        )),
    }
}

fn outcome(row: &Map<String, Value>, at: &str, scrobble: bool) -> Result<Outcome, Error> {
    let artist = corrected(row, at, "artist")?
        .ok_or_else(|| missing_or(&format!("{at}.artist"), "a name"))?;
    let track =
        corrected(row, at, "track")?.ok_or_else(|| missing_or(&format!("{at}.track"), "a name"))?;
    let timestamp = if scrobble {
        Some(shared::required_uint(row, at, "timestamp")?)
    } else {
        None
    };
    Ok(Outcome {
        artist,
        track,
        album: corrected(row, at, "album")?,
        album_artist: corrected(row, at, "albumArtist")?,
        timestamp,
        ignored: ignored(row, at)?,
    })
}

/// `{"corrected": "0", "#text": "..."}`. An empty or missing name is `None`.
fn corrected(row: &Map<String, Value>, at: &str, key: &str) -> Result<Option<Corrected>, Error> {
    let path = format!("{at}.{key}");
    let Some(value) = row.get(key) else {
        return Ok(None);
    };
    let Value::Object(field) = value else {
        return Err(missing_or(&path, "an object"));
    };
    let corrected = field
        .get("corrected")
        .and_then(de::flag)
        .ok_or_else(|| missing_or(&format!("{path}.corrected"), "a 0/1 flag"))?;
    Ok(shared::optional_str(field, &path, "#text")?.map(|name| Corrected { name, corrected }))
}

/// `{"code": "0", "#text": ""}` under either spelling. Code 0 is not ignored.
fn ignored(row: &Map<String, Value>, at: &str) -> Result<Option<Ignored>, Error> {
    let (key, value) = match (row.get("ignoredMessage"), row.get("ignoredmessage")) {
        (Some(value), None) => ("ignoredMessage", value),
        (None, Some(value)) => ("ignoredmessage", value),
        (Some(_), Some(_)) => {
            return Err(Error::decode_field(
                &format!("{at}.ignoredMessage"),
                "given twice",
            ));
        }
        (None, None) => return Err(missing_or(&format!("{at}.ignoredMessage"), "an object")),
    };
    let path = format!("{at}.{key}");
    let Value::Object(field) = value else {
        return Err(missing_or(&path, "an object"));
    };
    let code = shared::required_uint(field, &path, "code")?;
    if code == 0 {
        return Ok(None);
    }
    let code = u32::try_from(code)
        .map_err(|_| Error::decode_field(&format!("{path}.code"), "too large"))?;
    Ok(Some(Ignored {
        code: IgnoredCode(code),
        message: shared::optional_str(field, &path, "#text")?.unwrap_or_default(),
    }))
}

#[cfg(test)]
mod tests;
