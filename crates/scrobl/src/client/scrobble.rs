//! Scrobbling, on a [`Writer`].

use std::time::Duration;

use super::Writer;
use crate::error::Error;
use crate::model::{Outcome, ScrobbleReport, decode};
use crate::protocol::{Request, methods};
use crate::response::Response;

/// The most plays one scrobble call takes.
pub const MAX_BATCH: usize = 50;

/// One play of a track, to scrobble.
///
/// ```
/// let play = scrobl::client::Play::new("Cher", "Believe", 1_700_000_000).album("Believe");
/// ```
#[derive(Debug, Clone)]
pub struct Play {
    artist: String,
    track: String,
    timestamp: u64,
    album: Option<String>,
    album_artist: Option<String>,
    track_number: Option<u32>,
    mbid: Option<String>,
    duration: Option<Duration>,
    chosen_by_user: Option<bool>,
}

impl Play {
    /// A play that started at `timestamp`, in seconds since the Unix epoch.
    pub fn new(artist: impl Into<String>, track: impl Into<String>, timestamp: u64) -> Self {
        Self {
            artist: artist.into(),
            track: track.into(),
            timestamp,
            album: None,
            album_artist: None,
            track_number: None,
            mbid: None,
            duration: None,
            chosen_by_user: None,
        }
    }

    /// The album.
    #[must_use]
    pub fn album(mut self, album: impl Into<String>) -> Self {
        self.album = Some(album.into());
        self
    }

    /// The album's artist, where it is not the track's.
    #[must_use]
    pub fn album_artist(mut self, artist: impl Into<String>) -> Self {
        self.album_artist = Some(artist.into());
        self
    }

    /// The track's place on the album.
    #[must_use]
    pub fn track_number(mut self, number: u32) -> Self {
        self.track_number = Some(number);
        self
    }

    /// The track's MusicBrainz id.
    #[must_use]
    pub fn mbid(mut self, mbid: impl Into<String>) -> Self {
        self.mbid = Some(mbid.into());
        self
    }

    /// The track's length, sent in whole seconds.
    #[must_use]
    pub fn duration(mut self, duration: Duration) -> Self {
        self.duration = Some(duration);
        self
    }

    /// Whether the user chose the track, rather than a radio or a
    /// recommendation.
    #[must_use]
    pub fn chosen_by_user(mut self, chosen: bool) -> Self {
        self.chosen_by_user = Some(chosen);
        self
    }
}

impl Writer {
    /// Scrobbles up to [`MAX_BATCH`] plays at once. Sent once and never
    /// again.
    #[doc(alias = "track.scrobble")]
    pub fn scrobble(&self, plays: &[Play]) -> ScrobbleQuery {
        ScrobbleQuery {
            writer: self.clone(),
            plays: plays.to_vec(),
        }
    }

    /// Tells the service what the user is playing now.
    #[doc(alias = "track.updateNowPlaying")]
    pub fn now_playing(
        &self,
        artist: impl Into<String>,
        track: impl Into<String>,
    ) -> NowPlayingQuery {
        NowPlayingQuery {
            writer: self.clone(),
            play: Play::new(artist, track, 0),
        }
    }
}

/// Made by [`Writer::scrobble`].
#[derive(Debug, Clone)]
#[must_use = "a query does nothing until it is sent"]
pub struct ScrobbleQuery {
    writer: Writer,
    plays: Vec<Play>,
}

impl ScrobbleQuery {
    /// Makes the call.
    ///
    /// # Errors
    ///
    /// [`ErrorKind::InvalidRequest`](crate::ErrorKind) for no plays or more
    /// than [`MAX_BATCH`]. Any error of [`Writer::call`]; check
    /// [`delivery`](Error::delivery). A reply that does not account for
    /// every play is an error whose delivery is unknown.
    pub async fn send(self) -> Result<Response<ScrobbleReport>, Error> {
        let spec = &methods::TRACK_SCROBBLE;
        if self.plays.is_empty() || self.plays.len() > MAX_BATCH {
            return Err(Error::invalid_request(spec, "a batch holds 1 to 50 plays"));
        }
        let mut request = Request::new(spec);
        for (i, play) in self.plays.iter().enumerate() {
            request = request
                .indexed("artist", i, play.artist.as_str())
                .indexed("track", i, play.track.as_str())
                .indexed("timestamp", i, play.timestamp);
            request = optional(request, play, Some(i));
        }
        let raw = self.writer.call(&request).await?;
        let report = decode::track_scrobble(&raw)?;
        Ok(Response::new(report, raw))
    }
}

/// Made by [`Writer::now_playing`].
#[derive(Debug, Clone)]
#[must_use = "a query does nothing until it is sent"]
pub struct NowPlayingQuery {
    writer: Writer,
    play: Play,
}

impl NowPlayingQuery {
    /// The album.
    pub fn album(mut self, album: impl Into<String>) -> Self {
        self.play = self.play.album(album);
        self
    }

    /// The album's artist, where it is not the track's.
    pub fn album_artist(mut self, artist: impl Into<String>) -> Self {
        self.play = self.play.album_artist(artist);
        self
    }

    /// The track's place on the album.
    pub fn track_number(mut self, number: u32) -> Self {
        self.play = self.play.track_number(number);
        self
    }

    /// The track's MusicBrainz id.
    pub fn mbid(mut self, mbid: impl Into<String>) -> Self {
        self.play = self.play.mbid(mbid);
        self
    }

    /// The track's length, sent in whole seconds.
    pub fn duration(mut self, duration: Duration) -> Self {
        self.play = self.play.duration(duration);
        self
    }

    /// Makes the call.
    ///
    /// # Errors
    ///
    /// Any error of [`Writer::call`].
    pub async fn send(self) -> Result<Response<Outcome>, Error> {
        let request = Request::new(&methods::TRACK_UPDATE_NOW_PLAYING)
            .param("artist", self.play.artist.as_str())
            .param("track", self.play.track.as_str());
        let request = optional(request, &self.play, None);
        let raw = self.writer.call(&request).await?;
        let outcome = decode::track_update_now_playing(&raw)?;
        Ok(Response::new(outcome, raw))
    }
}

/// Adds what is set of `play`, indexed for a batch or plain for now playing.
fn optional(mut request: Request, play: &Play, index: Option<usize>) -> Request {
    let add = |request: Request, name: &str, value: String| match index {
        Some(i) => request.indexed(name, i, value),
        None => request.param(name, value),
    };
    if let Some(album) = &play.album {
        request = add(request, "album", album.clone());
    }
    if let Some(artist) = &play.album_artist {
        request = add(request, "albumArtist", artist.clone());
    }
    if let Some(number) = play.track_number {
        request = add(request, "trackNumber", number.to_string());
    }
    if let Some(mbid) = &play.mbid {
        request = add(request, "mbid", mbid.clone());
    }
    if let Some(duration) = play.duration {
        request = add(request, "duration", duration.as_secs().to_string());
    }
    if let Some(chosen) = play.chosen_by_user.filter(|_| index.is_some()) {
        request = add(request, "chosenByUser", u8::from(chosen).to_string());
    }
    request
}
