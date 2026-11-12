//! Typed reads about one artist: who is like them, their albums and
//! tracks, the tags they are given, and the name the service files them
//! under.

use super::Client;
use super::query::{Ask, query};
use crate::model::{Album, Artist, List, SimilarArtist, TopTag, Track, decode};
use crate::protocol::{MethodSpec, methods};

/// One artist, to ask about. Made by [`Client::artist`] from a name, or by
/// [`Client::artist_mbid`] from a MusicBrainz id.
///
/// An artist the service does not know is an error of the call,
/// [`INVALID_PARAMETERS`](crate::ApiErrorCode::INVALID_PARAMETERS), and not
/// an empty answer.
#[derive(Debug, Clone)]
pub struct ArtistHandle {
    client: Client,
    artist: Named,
    autocorrect: bool,
}

/// How an artist is named to the service.
#[derive(Debug, Clone)]
enum Named {
    Name(String),
    Mbid(String),
}

impl ArtistHandle {
    pub(super) fn named(client: Client, name: String) -> Self {
        Self {
            client,
            artist: Named::Name(name),
            autocorrect: false,
        }
    }

    pub(super) fn with_mbid(client: Client, mbid: String) -> Self {
        Self {
            client,
            artist: Named::Mbid(mbid),
            autocorrect: false,
        }
    }

    /// Lets the service correct a misspelt name, on every call made
    /// through this handle: `Guns and Roses` is then read as
    /// `Guns N' Roses`. A list says what name it was read as, in its
    /// [`subject`](crate::model::Page::subject).
    ///
    /// Without it the service takes the name as given, and a misspelling
    /// can be an artist of its own with little to show.
    #[must_use]
    pub fn autocorrect(mut self) -> Self {
        self.autocorrect = true;
        self
    }

    fn ask(&self, spec: &'static MethodSpec) -> Ask {
        let ask = Ask::new(&self.client, spec);
        let ask = match &self.artist {
            Named::Name(name) => ask.about("artist", name),
            Named::Mbid(mbid) => ask.about("mbid", mbid),
        };
        if self.autocorrect {
            ask.about("autocorrect", "1")
        } else {
            ask
        }
    }

    /// The artist the service files this name under. A name that is
    /// already right comes back as itself.
    #[doc(alias = "artist.getCorrection")]
    pub fn correction(&self) -> ArtistCorrectionQuery {
        let ask = Ask::new(&self.client, &methods::ARTIST_GET_CORRECTION);
        ArtistCorrectionQuery::new(match &self.artist {
            Named::Name(name) => ask.about("artist", name),
            Named::Mbid(_) => ask.refusing("a correction is asked for by name, not by id"),
        })
    }

    /// The artists the service finds like this one, most alike first.
    #[doc(alias = "artist.getSimilar")]
    pub fn similar(&self) -> ArtistSimilarQuery {
        ArtistSimilarQuery::new(self.ask(&methods::ARTIST_GET_SIMILAR))
    }

    /// The artist's albums, in the service's order.
    #[doc(alias = "artist.getTopAlbums")]
    pub fn top_albums(&self) -> ArtistTopAlbumsQuery {
        ArtistTopAlbumsQuery::new(self.ask(&methods::ARTIST_GET_TOP_ALBUMS))
    }

    /// The artist's tracks, the one with most listeners first.
    #[doc(alias = "artist.getTopTracks")]
    pub fn top_tracks(&self) -> ArtistTopTracksQuery {
        ArtistTopTracksQuery::new(self.ask(&methods::ARTIST_GET_TOP_TRACKS))
    }

    /// The tags the artist is given most.
    #[doc(alias = "artist.getTopTags")]
    pub fn top_tags(&self) -> ArtistTopTagsQuery {
        ArtistTopTagsQuery::new(self.ask(&methods::ARTIST_GET_TOP_TAGS))
    }
}

query! {
    /// The artist the service files a name under. Made by
    /// [`ArtistHandle::correction`].
    ///
    /// It is asked for by name: on a handle made from an id, `send` fails
    /// with [`ErrorKind::InvalidRequest`](crate::ErrorKind) before anything
    /// is sent. [`ArtistHandle::autocorrect`] plays no part in it.
    ///
    /// ```no_run
    /// # async fn demo(client: scrobl::Client) -> Result<(), scrobl::Error> {
    /// let artist = client.artist("Guns and Roses").correction().send().await?;
    /// println!("{}", artist.name()); // Guns N' Roses
    /// # Ok(())
    /// # }
    /// ```
    ArtistCorrectionQuery => Artist = decode::artist_get_correction;
}

query! {
    /// The artists the service finds like one artist. Made by
    /// [`ArtistHandle::similar`].
    ///
    /// ```no_run
    /// # async fn demo(client: scrobl::Client) -> Result<(), scrobl::Error> {
    /// let similar = client.artist("Radiohead").similar().limit(10).send().await?;
    /// for row in similar.iter() {
    ///     println!("{} ({:.2})", row.artist().name(), row.similarity());
    /// }
    /// # Ok(())
    /// # }
    /// ```
    ArtistSimilarQuery => List<SimilarArtist> = decode::artist_get_similar;
    limit = "Sets `limit`, the most artists to read, from 1. The service sends 100 when none \
             is set, and no more than 250 whatever is asked."
}

query! {
    /// One page of an artist's albums. Made by
    /// [`ArtistHandle::top_albums`].
    ///
    /// The service answers a page beyond the last with the first page, not
    /// with an empty one.
    ArtistTopAlbumsQuery => Page<Album> = decode::artist_get_top_albums;
    limit = "Sets `limit`, the page size, from 1. The service has applied 1000 when asked; \
             the page says what size it applied, in \
             [`per_page`](crate::model::Page::per_page).",
    page
}

query! {
    /// One page of an artist's tracks, the one with most listeners first.
    /// Made by [`ArtistHandle::top_tracks`].
    ///
    /// ```no_run
    /// # async fn demo(client: scrobl::Client) -> Result<(), scrobl::Error> {
    /// let page = client.artist("Cher").top_tracks().limit(10).send().await?;
    /// for (place, track) in page.ranked() {
    ///     println!("{place}. {} ({} plays)", track.name(), track.playcount().unwrap_or_default());
    /// }
    /// # Ok(())
    /// # }
    /// ```
    ///
    /// The service answers a page beyond the last with the first page, not
    /// with an empty one.
    ArtistTopTracksQuery => Page<Track> = decode::artist_get_top_tracks;
    limit = "Sets `limit`, the page size, from 1. The service has applied 1000 when asked; \
             the page says what size it applied, in \
             [`per_page`](crate::model::Page::per_page).",
    page
}

query! {
    /// The tags an artist is given most. Made by
    /// [`ArtistHandle::top_tags`].
    ArtistTopTagsQuery => List<TopTag> = decode::artist_get_top_tags;
}
