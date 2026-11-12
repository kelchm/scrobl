//! Typed reads about one track: what the service says about it, what is
//! like it, the tags it is given, and the names the service files it
//! under.

use super::Client;
use super::query::{Ask, query};
use crate::model::{List, SimilarTrack, Tag, TopTag, Track, TrackInfo, decode};
use crate::protocol::{MethodSpec, methods};

/// One track, to ask about. Made by [`Client::track`] from the names of
/// its artist and of the track, or by [`Client::track_mbid`] from a
/// MusicBrainz id.
///
/// A track the service does not know is an error of the call,
/// [`INVALID_PARAMETERS`](crate::ApiErrorCode::INVALID_PARAMETERS), and not
/// an empty answer.
#[derive(Debug, Clone)]
pub struct TrackHandle {
    client: Client,
    track: Named,
    autocorrect: bool,
}

/// How a track is named to the service.
#[derive(Debug, Clone)]
enum Named {
    Name { artist: String, title: String },
    Mbid(String),
}

impl TrackHandle {
    pub(super) fn named(client: Client, artist: String, title: String) -> Self {
        Self {
            client,
            track: Named::Name { artist, title },
            autocorrect: false,
        }
    }

    pub(super) fn with_mbid(client: Client, mbid: String) -> Self {
        Self {
            client,
            track: Named::Mbid(mbid),
            autocorrect: false,
        }
    }

    /// Lets the service correct a misspelt artist's name, on every call
    /// made through this handle, as
    /// [`ArtistHandle::autocorrect`](super::ArtistHandle::autocorrect)
    /// does.
    #[must_use]
    pub fn autocorrect(mut self) -> Self {
        self.autocorrect = true;
        self
    }

    fn ask(&self, spec: &'static MethodSpec) -> Ask {
        let ask = Ask::new(&self.client, spec);
        let ask = match &self.track {
            Named::Name { artist, title } => ask.about("artist", artist).about("track", title),
            Named::Mbid(mbid) => ask.about("mbid", mbid),
        };
        if self.autocorrect {
            ask.about("autocorrect", "1")
        } else {
            ask
        }
    }

    /// What the service says about the track: counts, its length, the
    /// album it is filed on, a few tags, and what its users wrote.
    #[doc(alias = "track.getInfo")]
    pub fn info(&self) -> TrackInfoQuery {
        TrackInfoQuery::new(self.ask(&methods::TRACK_GET_INFO))
    }

    /// The track the service files these names under. Names that are
    /// already right come back as themselves.
    #[doc(alias = "track.getCorrection")]
    pub fn correction(&self) -> TrackCorrectionQuery {
        let ask = Ask::new(&self.client, &methods::TRACK_GET_CORRECTION);
        TrackCorrectionQuery::new(match &self.track {
            Named::Name { artist, title } => ask.about("artist", artist).about("track", title),
            Named::Mbid(_) => ask.refusing("a correction is asked for by name, not by id"),
        })
    }

    /// The tracks the service finds like this one, most alike first.
    #[doc(alias = "track.getSimilar")]
    pub fn similar(&self) -> TrackSimilarQuery {
        TrackSimilarQuery::new(self.ask(&methods::TRACK_GET_SIMILAR))
    }

    /// The tags the track is given most.
    #[doc(alias = "track.getTopTags")]
    pub fn top_tags(&self) -> TrackTopTagsQuery {
        TrackTopTagsQuery::new(self.ask(&methods::TRACK_GET_TOP_TAGS))
    }

    /// The tags `user` has given the track.
    #[doc(alias = "track.getTags")]
    pub fn tags_of(&self, user: impl Into<String>) -> TrackTagsOfQuery {
        TrackTagsOfQuery::new(self.ask(&methods::TRACK_GET_TAGS).about("user", user))
    }
}

query! {
    /// What the service says about one track. Made by
    /// [`TrackHandle::info`].
    ///
    /// ```no_run
    /// # async fn demo(client: scrobl::Client) -> Result<(), scrobl::Error> {
    /// let info = client.track("Cher", "Believe").info().for_user("rj").send().await?;
    /// println!(
    ///     "{} ({:?}): {} plays, {:?} by rj, loved: {:?}",
    ///     info.track().name(),
    ///     info.track().duration(),
    ///     info.playcount(),
    ///     info.user_playcount(),
    ///     info.user_loved()
    /// );
    /// # Ok(())
    /// # }
    /// ```
    TrackInfoQuery => TrackInfo = decode::track_get_info;
    for_user
}

query! {
    /// The track the service files an artist's name and a track's name
    /// under. Made by [`TrackHandle::correction`].
    ///
    /// It is asked for by name: on a handle made from an id, `send` fails
    /// with [`ErrorKind::InvalidRequest`](crate::ErrorKind) before anything
    /// is sent. [`TrackHandle::autocorrect`] plays no part in it.
    TrackCorrectionQuery => Track = decode::track_get_correction;
}

query! {
    /// The tracks the service finds like one track. Made by
    /// [`TrackHandle::similar`].
    ///
    /// ```no_run
    /// # async fn demo(client: scrobl::Client) -> Result<(), scrobl::Error> {
    /// let similar = client.track("Radiohead", "Creep").similar().limit(10).send().await?;
    /// for row in similar.iter() {
    ///     let track = row.track();
    ///     println!("{} by {} ({:.2})", track.name(), track.artist().name(), row.similarity());
    /// }
    /// # Ok(())
    /// # }
    /// ```
    TrackSimilarQuery => List<SimilarTrack> = decode::track_get_similar;
    limit = "Sets `limit`, the most tracks to read, from 1. The service sends 100 when none \
             is set, and sent 217 when asked for 1000."
}

query! {
    /// The tags a track is given most. Made by [`TrackHandle::top_tags`].
    ///
    /// On 2026-11-12 the service answered this call for a track named by
    /// id with HTTP 400 and an empty body, where `info` by the same id
    /// worked. That arrives as [`ErrorKind::Http`](crate::ErrorKind).
    TrackTopTagsQuery => List<TopTag> = decode::track_get_top_tags;
}

query! {
    /// The tags one user has given a track. Made by
    /// [`TrackHandle::tags_of`]. A user who has given none is an empty
    /// list.
    TrackTagsOfQuery => List<Tag> = decode::track_get_tags;
}
