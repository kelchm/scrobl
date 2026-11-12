//! Typed reads of what one country plays most.

use super::Client;
use super::query::{Ask, query};
use crate::model::{CountryArtist, CountryTrack, Page, decode};
use crate::protocol::{MethodSpec, methods};

/// One country, to ask about. Made by [`Client::country`].
///
/// The country is a name as ISO 3166-1 spells it in English, such as
/// `Germany`, in any case. The service refuses a code such as `DE` and a
/// name it does not know with
/// [`INVALID_PARAMETERS`](crate::ApiErrorCode::INVALID_PARAMETERS).
#[derive(Debug, Clone)]
pub struct CountryHandle {
    client: Client,
    name: String,
}

impl CountryHandle {
    pub(super) fn new(client: Client, name: String) -> Self {
        Self { client, name }
    }

    fn ask(&self, spec: &'static MethodSpec) -> Ask {
        Ask::new(&self.client, spec).about("country", &self.name)
    }

    /// The country as given.
    pub fn name(&self) -> &str {
        &self.name
    }

    /// The artists the country plays most.
    #[doc(alias = "geo.getTopArtists")]
    pub fn top_artists(&self) -> CountryTopArtistsQuery {
        CountryTopArtistsQuery::new(self.ask(&methods::GEO_GET_TOP_ARTISTS))
    }

    /// The tracks the country plays most.
    #[doc(alias = "geo.getTopTracks")]
    pub fn top_tracks(&self) -> CountryTopTracksQuery {
        CountryTopTracksQuery::new(self.ask(&methods::GEO_GET_TOP_TRACKS))
    }
}

query! {
    /// The artists a country plays most. Made by
    /// [`CountryHandle::top_artists`].
    ///
    /// ```no_run
    /// # async fn demo(client: scrobl::Client) -> Result<(), scrobl::Error> {
    /// let page = client.country("Germany").top_artists().limit(10).send().await?;
    /// for (place, row) in page.ranked() {
    ///     println!("{place}. {} ({} listeners)", row.artist().name(), row.listeners());
    /// }
    /// # Ok(())
    /// # }
    /// ```
    CountryTopArtistsQuery => Page<CountryArtist> = decode::geo_get_top_artists;
    limit = "Sets `limit`, the page size, from 1 to 1000. The service refuses a larger one.",
    page
}

query! {
    /// The tracks a country plays most. Made by
    /// [`CountryHandle::top_tracks`].
    ///
    /// The method's `location` parameter, a city within the country, is not
    /// offered: on 2026-11-11 the service sent the same rows with it and
    /// without.
    CountryTopTracksQuery => Page<CountryTrack> = decode::geo_get_top_tracks;
    limit = "Sets `limit`, the page size, from 1 to 1000. The service refuses a larger one.",
    page
}
