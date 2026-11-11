//! Typed reads of what is played most: on the whole service, and in one
//! country.

use super::Client;
use super::lists::List;
use crate::error::Error;
use crate::model::{
    ChartArtistsPage, ChartTagsPage, ChartTracksPage, GeoArtistsPage, GeoTracksPage,
};
use crate::protocol::methods;
use crate::response::Response;

/// The charts of the whole service. Made by [`Client::chart`].
#[derive(Debug, Clone)]
pub struct Chart {
    client: Client,
}

impl Chart {
    pub(super) fn new(client: Client) -> Self {
        Self { client }
    }

    /// Starts a `chart.getTopArtists` query.
    pub fn top_artists(&self) -> ChartArtistsQuery {
        ChartArtistsQuery {
            list: List::new(self.client.clone(), None),
        }
    }

    /// Starts a `chart.getTopTracks` query.
    pub fn top_tracks(&self) -> ChartTracksQuery {
        ChartTracksQuery {
            list: List::new(self.client.clone(), None),
        }
    }

    /// Starts a `chart.getTopTags` query.
    pub fn top_tags(&self) -> ChartTagsQuery {
        ChartTagsQuery {
            list: List::new(self.client.clone(), None),
        }
    }
}

/// The charts of one country. Made by [`Client::geo`].
#[derive(Debug, Clone)]
pub struct Geo {
    client: Client,
    country: String,
}

impl Geo {
    pub(super) fn new(client: Client, country: String) -> Self {
        Self { client, country }
    }

    /// The country as given.
    pub fn country(&self) -> &str {
        &self.country
    }

    /// Starts a `geo.getTopArtists` query for this country.
    pub fn top_artists(&self) -> GeoArtistsQuery {
        GeoArtistsQuery { list: self.list() }
    }

    /// Starts a `geo.getTopTracks` query for this country.
    pub fn top_tracks(&self) -> GeoTracksQuery {
        GeoTracksQuery { list: self.list() }
    }

    fn list(&self) -> List {
        List::new(self.client.clone(), Some(("country", self.country.clone())))
    }
}

/// Defines a query for one page of a list: `limit`, `page` and `send`.
macro_rules! page_query {
    (
        $(#[$doc:meta])*
        $name:ident, $spec:ident, $page:ty
    ) => {
        $(#[$doc])*
        #[derive(Debug, Clone)]
        #[must_use = "a query does nothing until it is sent"]
        pub struct $name {
            list: List,
        }

        impl $name {
            /// Sets `limit`, the page size, from 1 to 1000. The service
            /// answers a larger one with
            /// [`INVALID_PARAMETERS`](crate::ApiErrorCode::INVALID_PARAMETERS).
            pub fn limit(mut self, limit: u32) -> Self {
                self.list.limit = Some(limit);
                self
            }

            /// Sets `page`, from 1.
            pub fn page(mut self, page: u32) -> Self {
                self.list.page = Some(page);
                self
            }

            /// Reads one page. The exact response is on the result, next
            /// to the typed page.
            ///
            /// # Errors
            ///
            /// [`ErrorKind::InvalidRequest`](crate::ErrorKind) for a
            /// `limit` or a `page` of 0, before anything is sent; any error
            /// of [`Client::call`]; and
            /// [`ErrorKind::Decode`](crate::ErrorKind) when the page is not
            /// the expected shape.
            pub async fn send(self) -> Result<Response<$page>, Error> {
                self.list.send(&methods::$spec, <$page>::decode).await
            }
        }
    };
}

page_query! {
    /// A `chart.getTopArtists` query: the artists played most on the whole
    /// service. Made by [`Chart::top_artists`].
    ///
    /// ```no_run
    /// # async fn demo(client: scrobl::Client) -> Result<(), scrobl::Error> {
    /// let page = client.chart().top_artists().limit(10).send().await?;
    /// for (place, row) in page.artists().iter().enumerate() {
    ///     println!("{}. {} ({} plays)", place + 1, row.artist().name(), row.playcount());
    /// }
    /// # Ok(())
    /// # }
    /// ```
    ChartArtistsQuery, CHART_GET_TOP_ARTISTS, ChartArtistsPage
}

page_query! {
    /// A `chart.getTopTracks` query: the tracks played most on the whole
    /// service. Made by [`Chart::top_tracks`].
    ChartTracksQuery, CHART_GET_TOP_TRACKS, ChartTracksPage
}

page_query! {
    /// A `chart.getTopTags` query: the tags used most on the whole service.
    /// Made by [`Chart::top_tags`].
    ///
    /// The service has not paged this list: see [`ChartTagsPage`]. A larger
    /// [`limit`](Self::limit) is the way to read further.
    ChartTagsQuery, CHART_GET_TOP_TAGS, ChartTagsPage
}

page_query! {
    /// A `geo.getTopArtists` query: the artists a country plays most. Made
    /// by [`Geo::top_artists`].
    ///
    /// The country is a name as ISO 3166-1 spells it in English, such as
    /// `Germany`, in any case. The service refuses a code such as `DE` and
    /// a name it does not know with
    /// [`INVALID_PARAMETERS`](crate::ApiErrorCode::INVALID_PARAMETERS).
    ///
    /// ```no_run
    /// # async fn demo(client: scrobl::Client) -> Result<(), scrobl::Error> {
    /// let page = client.geo("Germany").top_artists().limit(10).send().await?;
    /// println!("{}: {} artists", page.country(), page.attr().total());
    /// # Ok(())
    /// # }
    /// ```
    GeoArtistsQuery, GEO_GET_TOP_ARTISTS, GeoArtistsPage
}

page_query! {
    /// A `geo.getTopTracks` query: the tracks a country plays most. Made by
    /// [`Geo::top_tracks`]. The country is given as for a
    /// [`GeoArtistsQuery`].
    ///
    /// The method's `location` parameter, a city within the country, is not
    /// offered: on 2026-11-11 the service sent the same rows with it and
    /// without.
    GeoTracksQuery, GEO_GET_TOP_TRACKS, GeoTracksPage
}
