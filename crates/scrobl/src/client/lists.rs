//! Typed reads of a user's lists: what they play and tag most, and the
//! artists in their library.

use super::{Client, User};
use crate::error::Error;
use crate::model::{LibraryArtistsPage, TopAlbumsPage, TopArtistsPage, TopTags, TopTracksPage};
use crate::protocol::{MethodSpec, Raw, Request, methods};
use crate::response::Response;

/// The span of time a list of a user's most played covers, counted back
/// from now.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
#[non_exhaustive]
pub enum Period {
    /// Everything the user has scrobbled. What the service uses when no
    /// period is sent.
    #[default]
    Overall,
    /// `7day`.
    SevenDays,
    /// `1month`.
    OneMonth,
    /// `3month`.
    ThreeMonths,
    /// `6month`.
    SixMonths,
    /// `12month`.
    TwelveMonths,
}

impl Period {
    /// The value of the `period` parameter.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Overall => "overall",
            Self::SevenDays => "7day",
            Self::OneMonth => "1month",
            Self::ThreeMonths => "3month",
            Self::SixMonths => "6month",
            Self::TwelveMonths => "12month",
        }
    }
}

impl User {
    /// Starts a `user.getTopArtists` query for this user.
    pub fn top_artists(&self) -> TopArtistsQuery {
        TopArtistsQuery {
            list: List::of(self),
        }
    }

    /// Starts a `user.getTopAlbums` query for this user.
    pub fn top_albums(&self) -> TopAlbumsQuery {
        TopAlbumsQuery {
            list: List::of(self),
        }
    }

    /// Starts a `user.getTopTracks` query for this user.
    pub fn top_tracks(&self) -> TopTracksQuery {
        TopTracksQuery {
            list: List::of(self),
        }
    }

    /// Starts a `user.getTopTags` query for this user.
    pub fn top_tags(&self) -> TopTagsQuery {
        TopTagsQuery {
            list: List::of(self),
        }
    }

    /// Starts a `library.getArtists` query for this user.
    pub fn library_artists(&self) -> LibraryArtistsQuery {
        LibraryArtistsQuery {
            list: List::of(self),
        }
    }
}

/// What every query here holds: whose list, and which part of it.
#[derive(Debug, Clone)]
struct List {
    client: Client,
    user: String,
    limit: Option<u32>,
    page: Option<u32>,
    period: Option<Period>,
}

impl List {
    fn of(user: &User) -> Self {
        Self {
            client: user.client.clone(),
            user: user.name.clone(),
            limit: None,
            page: None,
            period: None,
        }
    }

    /// Sends the request for `spec`, with `user` and whatever else was set,
    /// and decodes the answer.
    async fn send<T>(
        self,
        spec: &'static MethodSpec,
        decode: fn(&Raw) -> Result<T, Error>,
    ) -> Result<Response<T>, Error> {
        let mut request = Request::new(spec).param("user", &self.user);
        if let Some(period) = self.period {
            request = request.param("period", period.as_str());
        }
        if let Some(limit) = self.limit {
            request = request.param("limit", limit);
        }
        if let Some(page) = self.page {
            request = request.param("page", page);
        }
        let raw = self.client.call(&request).await?;
        let value = decode(&raw)?;
        Ok(Response::new(value, raw))
    }
}

/// A `user.getTopArtists` query: the artists a user played most. Made by
/// [`User::top_artists`].
///
/// ```no_run
/// # use scrobl::client::Period;
/// # async fn demo(client: scrobl::Client) -> Result<(), scrobl::Error> {
/// let page = client
///     .user("rj")
///     .top_artists()
///     .period(Period::SevenDays)
///     .limit(10)
///     .send()
///     .await?;
/// for top in page.artists() {
///     println!("{}. {} ({})", top.rank(), top.artist().name(), top.playcount());
/// }
/// # Ok(())
/// # }
/// ```
#[derive(Debug, Clone)]
#[must_use = "a query does nothing until it is sent"]
pub struct TopArtistsQuery {
    list: List,
}

impl TopArtistsQuery {
    /// Sets `period`, the span of time to count plays in. The service
    /// counts all of the user's history when none is sent.
    pub fn period(mut self, period: Period) -> Self {
        self.list.period = Some(period);
        self
    }

    /// Sets `limit`, the page size, from 1 to 1000. The service answers a
    /// size outside that range with
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

    /// Reads one page. The exact response is on the result, next to the
    /// typed page.
    ///
    /// # Errors
    ///
    /// Any error of [`Client::call`], which is where a `limit` or a `page`
    /// the service refuses arrives, as
    /// [`INVALID_PARAMETERS`](crate::ApiErrorCode::INVALID_PARAMETERS); and
    /// [`ErrorKind::Decode`](crate::ErrorKind) when the page is not the
    /// expected shape.
    pub async fn send(self) -> Result<Response<TopArtistsPage>, Error> {
        self.list
            .send(&methods::USER_GET_TOP_ARTISTS, TopArtistsPage::decode)
            .await
    }
}

/// A `user.getTopAlbums` query: the albums a user played most. Made by
/// [`User::top_albums`]. It is set and sent as a [`TopArtistsQuery`] is.
#[derive(Debug, Clone)]
#[must_use = "a query does nothing until it is sent"]
pub struct TopAlbumsQuery {
    list: List,
}

impl TopAlbumsQuery {
    /// Sets `period`, the span of time to count plays in. The service
    /// counts all of the user's history when none is sent.
    pub fn period(mut self, period: Period) -> Self {
        self.list.period = Some(period);
        self
    }

    /// Sets `limit`, the page size, from 1. The service has applied 1000
    /// when asked; the page says what size it applied, in
    /// [`per_page`](crate::model::PageAttr::per_page).
    pub fn limit(mut self, limit: u32) -> Self {
        self.list.limit = Some(limit);
        self
    }

    /// Sets `page`, from 1.
    pub fn page(mut self, page: u32) -> Self {
        self.list.page = Some(page);
        self
    }

    /// Reads one page. The exact response is on the result, next to the
    /// typed page.
    ///
    /// # Errors
    ///
    /// As [`TopArtistsQuery::send`].
    pub async fn send(self) -> Result<Response<TopAlbumsPage>, Error> {
        self.list
            .send(&methods::USER_GET_TOP_ALBUMS, TopAlbumsPage::decode)
            .await
    }
}

/// A `user.getTopTracks` query: the tracks a user played most. Made by
/// [`User::top_tracks`]. It is set and sent as a [`TopArtistsQuery`] is.
#[derive(Debug, Clone)]
#[must_use = "a query does nothing until it is sent"]
pub struct TopTracksQuery {
    list: List,
}

impl TopTracksQuery {
    /// Sets `period`, the span of time to count plays in. The service
    /// counts all of the user's history when none is sent.
    pub fn period(mut self, period: Period) -> Self {
        self.list.period = Some(period);
        self
    }

    /// Sets `limit`, the page size, from 1. The service has applied 1000
    /// when asked; the page says what size it applied, in
    /// [`per_page`](crate::model::PageAttr::per_page).
    pub fn limit(mut self, limit: u32) -> Self {
        self.list.limit = Some(limit);
        self
    }

    /// Sets `page`, from 1.
    pub fn page(mut self, page: u32) -> Self {
        self.list.page = Some(page);
        self
    }

    /// Reads one page. The exact response is on the result, next to the
    /// typed page.
    ///
    /// # Errors
    ///
    /// As [`TopArtistsQuery::send`].
    pub async fn send(self) -> Result<Response<TopTracksPage>, Error> {
        self.list
            .send(&methods::USER_GET_TOP_TRACKS, TopTracksPage::decode)
            .await
    }
}

/// A `user.getTopTags` query: the tags a user applied most. Made by
/// [`User::top_tags`]. The method has no pages.
///
/// ```no_run
/// # async fn demo(client: scrobl::Client) -> Result<(), scrobl::Error> {
/// let top = client.user("rj").top_tags().limit(10).send().await?;
/// for tag in top.tags() {
///     println!("{} ({})", tag.name(), tag.count());
/// }
/// # Ok(())
/// # }
/// ```
#[derive(Debug, Clone)]
#[must_use = "a query does nothing until it is sent"]
pub struct TopTagsQuery {
    list: List,
}

impl TopTagsQuery {
    /// Sets `limit`, the most tags to return, from 1.
    pub fn limit(mut self, limit: u32) -> Self {
        self.list.limit = Some(limit);
        self
    }

    /// Reads the tags. The exact response is on the result, next to the
    /// typed list.
    ///
    /// # Errors
    ///
    /// Any error of [`Client::call`], and
    /// [`ErrorKind::Decode`](crate::ErrorKind) when the answer is not the
    /// expected shape.
    pub async fn send(self) -> Result<Response<TopTags>, Error> {
        self.list
            .send(&methods::USER_GET_TOP_TAGS, TopTags::decode)
            .await
    }
}

/// A `library.getArtists` query: every artist a user has played, most
/// played first. Made by [`User::library_artists`].
///
/// ```no_run
/// # async fn demo(client: scrobl::Client) -> Result<(), scrobl::Error> {
/// let page = client.user("rj").library_artists().limit(200).send().await?;
/// println!("{} artists", page.attr().total());
/// # Ok(())
/// # }
/// ```
#[derive(Debug, Clone)]
#[must_use = "a query does nothing until it is sent"]
pub struct LibraryArtistsQuery {
    list: List,
}

impl LibraryArtistsQuery {
    /// Sets `limit`, the page size, from 1. The service has applied 2000
    /// when asked; the page says what size it applied, in
    /// [`per_page`](crate::model::PageAttr::per_page).
    pub fn limit(mut self, limit: u32) -> Self {
        self.list.limit = Some(limit);
        self
    }

    /// Sets `page`, from 1.
    pub fn page(mut self, page: u32) -> Self {
        self.list.page = Some(page);
        self
    }

    /// Reads one page. The exact response is on the result, next to the
    /// typed page.
    ///
    /// # Errors
    ///
    /// As [`TopArtistsQuery::send`].
    pub async fn send(self) -> Result<Response<LibraryArtistsPage>, Error> {
        self.list
            .send(&methods::LIBRARY_GET_ARTISTS, LibraryArtistsPage::decode)
            .await
    }
}
