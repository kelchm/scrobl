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

    /// How many the service reported accepted.
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

/// What the service reported for one play: the names as it took them, and
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
    /// Whether the service reported the play accepted.
    pub fn is_accepted(&self) -> bool {
        self.ignored.is_none()
    }

    /// Why the service ignored the play.
    pub fn ignored(&self) -> Option<&Ignored> {
        self.ignored.as_ref()
    }

    /// The artist as the service took it.
    pub fn artist(&self) -> &Corrected {
        &self.artist
    }

    /// The track as the service took it.
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

/// A name as the service took it.
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

/// Why a play was ignored. The meanings are Last.fm's documented ones, which
/// the service does not keep to; codes this library does not know are kept.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct IgnoredCode(u32);

impl IgnoredCode {
    /// 1: the artist is filtered. In practice also given for a timestamp far
    /// in the past or future, or a malformed MusicBrainz id.
    pub const ARTIST: Self = Self(1);
    /// 2: the track is filtered.
    pub const TRACK: Self = Self(2);
    /// 3: the timestamp is too far in the past.
    pub const TOO_OLD: Self = Self(3);
    /// 4: the timestamp is too far in the future.
    pub const TOO_NEW: Self = Self(4);
    /// 5: the user's daily scrobble limit is reached. The limit may instead
    /// come as error 29, rate limit exceeded.
    pub const DAILY_LIMIT: Self = Self(5);

    /// The number the service sent.
    pub const fn get(self) -> u32 {
        self.0
    }
}

/// Decodes a `track.scrobble` response and checks it against the request:
/// one outcome for each play sent, with its timestamp, and `accepted` plus
/// `ignored` equal to the batch. The service lists accepted plays first;
/// each outcome is matched to its play by timestamp and names, and put back
/// in the order sent. An outcome that fits no play, or plays that differ,
/// is an error.
///
/// # Errors
///
/// [`ErrorKind::Decode`](crate::ErrorKind) for a malformed reply, and
/// [`ErrorKind::Inconsistent`](crate::ErrorKind) for one that does not
/// match the request. Either way the error's
/// [`delivery`](crate::Error::delivery) is unknown.
pub fn track_scrobble(raw: &Raw) -> Result<ScrobbleReport, Error> {
    shared::decode(raw, &methods::TRACK_SCROBBLE, "scrobbles", |mut list| {
        let sent = sent_plays(raw)?;
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

/// Matches each play to the outcome with its timestamp and names. Identical
/// plays take their outcomes in the reply's order.
fn in_sent_order(outcomes: Vec<Outcome>, sent: &[Sent]) -> Result<Vec<Outcome>, Error> {
    // Each play's class: the first play identical to it.
    let classes: Vec<usize> = sent
        .iter()
        .enumerate()
        .map(|(i, play)| sent.iter().position(|p| p == play).unwrap_or(i))
        .collect();
    let mut rows = Vec::with_capacity(outcomes.len());
    for outcome in outcomes {
        let mut fits = sent
            .iter()
            .zip(&classes)
            .filter(|(play, _)| play.fits(&outcome))
            .map(|(_, class)| *class);
        let class = fits
            .next()
            .ok_or_else(|| Error::inconsistent("each outcome fits a play sent"))?;
        if fits.any(|other| other != class) {
            return Err(Error::inconsistent("each outcome fits one play sent"));
        }
        rows.push((class, outcome));
    }
    let mut ordered = Vec::with_capacity(sent.len());
    for class in &classes {
        let at = rows
            .iter()
            .position(|(c, _)| c == class)
            .ok_or_else(|| Error::inconsistent("each play sent has an outcome"))?;
        ordered.push(rows.remove(at).1);
    }
    Ok(ordered)
}

/// A play as the request sent it. Plays are identical only if every
/// parameter is, since any of them can get a play ignored.
#[derive(PartialEq)]
struct Sent {
    timestamp: u64,
    artist: String,
    track: String,
    params: Vec<(String, String)>,
}

impl Sent {
    /// A name fits if it is the one sent or the service says it corrected it.
    fn fits(&self, outcome: &Outcome) -> bool {
        let name = |c: &Corrected, sent: &str| c.corrected || c.name == sent;
        outcome.timestamp == Some(self.timestamp)
            && name(&outcome.artist, &self.artist)
            && name(&outcome.track, &self.track)
    }
}

/// The plays the request sent, in batch order, numbered from 0 with no gap.
fn sent_plays(raw: &Raw) -> Result<Vec<Sent>, Error> {
    let request = raw.request();
    let count = |prefix: &str| {
        request
            .params()
            .iter()
            .filter(|(name, _)| name.starts_with(prefix))
            .count()
    };
    let plays = count("artist[");
    let sent: Option<Vec<Sent>> = (0..plays)
        .map(|i| {
            let index = format!("[{i}]");
            let mut params: Vec<_> = request
                .params()
                .iter()
                .filter_map(|(name, value)| {
                    Some((name.strip_suffix(&index)?.to_owned(), value.clone()))
                })
                .collect();
            params.sort();
            Some(Sent {
                timestamp: request.get(&format!("timestamp{index}"))?.parse().ok()?,
                artist: request.get(&format!("artist{index}"))?.to_owned(),
                track: request.get(&format!("track{index}"))?.to_owned(),
                params,
            })
        })
        .collect();
    match sent {
        Some(sent) if count("timestamp[") == plays && count("track[") == plays => Ok(sent),
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
