//! Typed searches: artists, albums and tracks by name.

use super::Client;
use super::query::{Ask, query};
use crate::model::{Album, Artist, Track, decode};
use crate::protocol::{MethodSpec, methods};

/// The service's search. Made by [`Client::search`].
#[derive(Debug, Clone)]
pub struct SearchHandle {
    client: Client,
}

impl SearchHandle {
    pub(super) fn new(client: Client) -> Self {
        Self { client }
    }

    /// A search for `query`, sent under `name`. The service answers an
    /// empty query with an empty object, which says nothing, so that one is
    /// refused before it is sent.
    fn ask(&self, spec: &'static MethodSpec, name: &'static str, query: String) -> Ask {
        let ask = Ask::new(&self.client, spec);
        if query.is_empty() {
            ask.refusing("the search is for nothing: the query is empty")
        } else {
            ask.about(name, query)
        }
    }

    /// The artists whose names match `query`, the best match first.
    #[doc(alias = "artist.search")]
    pub fn artists(&self, query: impl Into<String>) -> SearchArtistsQuery {
        SearchArtistsQuery::new(self.ask(&methods::ARTIST_SEARCH, "artist", query.into()))
    }

    /// The albums whose names match `query`, the best match first.
    #[doc(alias = "album.search")]
    pub fn albums(&self, query: impl Into<String>) -> SearchAlbumsQuery {
        SearchAlbumsQuery::new(self.ask(&methods::ALBUM_SEARCH, "album", query.into()))
    }

    /// The tracks whose names match `query`, the best match first.
    #[doc(alias = "track.search")]
    pub fn tracks(&self, query: impl Into<String>) -> SearchTracksQuery {
        SearchTracksQuery::new(self.ask(&methods::TRACK_SEARCH, "track", query.into()))
    }
}

query! {
    /// One page of the artists a search found. Made by
    /// [`SearchHandle::artists`].
    ///
    /// ```no_run
    /// # async fn demo(client: scrobl::Client) -> Result<(), scrobl::Error> {
    /// let page = client.search().artists("radiohead").limit(5).send().await?;
    /// println!("{} artists match `{}`", page.total(), page.subject().unwrap_or_default());
    /// for artist in page.iter() {
    ///     println!("{} ({} listeners)", artist.name(), artist.listeners().unwrap_or_default());
    /// }
    /// # Ok(())
    /// # }
    /// ```
    ///
    /// A search that finds nothing is an empty page, not an error.
    SearchArtistsQuery => Page<Artist> = decode::artist_search;
    limit = "Sets `limit`, the page size, from 1 to 10000. The service refuses a larger one, \
             and uses 30 when none is set.",
    page = "Sets `page`, from 1 to 10000. The service refuses a larger one, however many \
            matches there are."
}

query! {
    /// One page of the albums a search found. Made by
    /// [`SearchHandle::albums`]. Each names its artist, and has no counts.
    SearchAlbumsQuery => Page<Album> = decode::album_search;
    limit = "Sets `limit`, the page size, from 1 to 10000. The service refuses a larger one, \
             and uses 30 when none is set.",
    page = "Sets `page`, from 1 to 10000. The service refuses a larger one, however many \
            matches there are."
}

query! {
    /// One page of the tracks a search found. Made by
    /// [`SearchHandle::tracks`]. Each names its artist and has its
    /// listeners.
    ///
    /// ```no_run
    /// # async fn demo(client: scrobl::Client) -> Result<(), scrobl::Error> {
    /// let page = client.search().tracks("creep").artist("radiohead").limit(5).send().await?;
    /// for track in page.iter() {
    ///     println!("{} by {}", track.name(), track.artist().name());
    /// }
    /// # Ok(())
    /// # }
    /// ```
    SearchTracksQuery => Page<Track> = decode::track_search;
    artist,
    limit = "Sets `limit`, the page size, from 1 to 1000. The service refuses a larger one, \
             and uses 30 when none is set.",
    page = "Sets `page`, from 1 to 1000. The service refuses a larger one, however many \
            matches there are: a tenth of what the other two searches allow."
}
