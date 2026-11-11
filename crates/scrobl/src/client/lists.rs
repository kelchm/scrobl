//! Typed reads of a user's lists: what they play and tag most, of all time
//! or in one week, and the artists in their library.

use super::{Client, User};
use crate::error::Error;
use crate::model::{
    LibraryArtistsPage, TopAlbumsPage, TopArtistsPage, TopTags, TopTracksPage, WeeklyAlbumChart,
    WeeklyArtistChart, WeeklyChartList, WeeklyTrackChart,
};
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

    /// Starts a `user.getWeeklyChartList` query for this user.
    pub fn weekly_chart_list(&self) -> WeeklyChartListQuery {
        WeeklyChartListQuery {
            list: List::of(self),
        }
    }

    /// Starts a `user.getWeeklyArtistChart` query for this user.
    pub fn weekly_artist_chart(&self) -> WeeklyArtistChartQuery {
        WeeklyArtistChartQuery {
            list: List::of(self),
        }
    }

    /// Starts a `user.getWeeklyAlbumChart` query for this user.
    pub fn weekly_album_chart(&self) -> WeeklyAlbumChartQuery {
        WeeklyAlbumChartQuery {
            list: List::of(self),
        }
    }

    /// Starts a `user.getWeeklyTrackChart` query for this user.
    pub fn weekly_track_chart(&self) -> WeeklyTrackChartQuery {
        WeeklyTrackChartQuery {
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
    range: Option<(u64, u64)>,
}

impl List {
    fn of(user: &User) -> Self {
        Self {
            client: user.client.clone(),
            user: user.name.clone(),
            limit: None,
            page: None,
            period: None,
            range: None,
        }
    }

    /// Sends the request for `spec`, with `user` and whatever else was set,
    /// and decodes the answer.
    async fn send<T>(
        self,
        spec: &'static MethodSpec,
        decode: fn(&Raw) -> Result<T, Error>,
    ) -> Result<Response<T>, Error> {
        // The service refuses a 0 too, but its answer is error 6 with the
        // reason in a message this library never prints.
        if self.limit == Some(0) {
            return Err(Error::invalid_request(spec, "`limit` starts at 1"));
        }
        if self.page == Some(0) {
            return Err(Error::invalid_request(spec, "`page` starts at 1"));
        }
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
        if let Some((from, to)) = self.range {
            request = request.param("from", from).param("to", to);
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
    /// larger one with
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
    /// [`ErrorKind::InvalidRequest`](crate::ErrorKind) for a `limit` or a
    /// `page` of 0, before anything is sent; any error of [`Client::call`],
    /// which is where a `limit` the service finds too large arrives, as
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
    /// [`ErrorKind::InvalidRequest`](crate::ErrorKind) for a `limit` of 0,
    /// before anything is sent; any error of [`Client::call`]; and
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

/// A `user.getWeeklyChartList` query: the weeks a user's charts can be
/// asked for. Made by [`User::weekly_chart_list`].
#[derive(Debug, Clone)]
#[must_use = "a query does nothing until it is sent"]
pub struct WeeklyChartListQuery {
    list: List,
}

impl WeeklyChartListQuery {
    /// Reads the list. The exact response is on the result, next to the
    /// typed list.
    ///
    /// # Errors
    ///
    /// Any error of [`Client::call`], and
    /// [`ErrorKind::Decode`](crate::ErrorKind) when the answer is not the
    /// expected shape.
    pub async fn send(self) -> Result<Response<WeeklyChartList>, Error> {
        self.list
            .send(
                &methods::USER_GET_WEEKLY_CHART_LIST,
                WeeklyChartList::decode,
            )
            .await
    }
}

/// A `user.getWeeklyArtistChart` query: the artists a user played in one
/// week. Made by [`User::weekly_artist_chart`].
///
/// ```no_run
/// # async fn demo(client: scrobl::Client) -> Result<(), scrobl::Error> {
/// let user = client.user("rj");
/// let weeks = user.weekly_chart_list().send().await?;
/// if let Some(week) = weeks.ranges().last() {
///     let chart = user
///         .weekly_artist_chart()
///         .range(week.from(), week.to())
///         .send()
///         .await?;
///     for top in chart.artists() {
///         println!("{}. {} ({})", top.rank(), top.artist().name(), top.playcount());
///     }
/// }
/// # Ok(())
/// # }
/// ```
#[derive(Debug, Clone)]
#[must_use = "a query does nothing until it is sent"]
pub struct WeeklyArtistChartQuery {
    list: List,
}

impl WeeklyArtistChartQuery {
    /// Sets `from` and `to`, the span to chart, in seconds since the Unix
    /// epoch, such as a week of [`WeeklyChartList`]. Without them the
    /// service chooses a recent span of seven days, which need not be one
    /// of the listed weeks.
    pub fn range(mut self, from: u64, to: u64) -> Self {
        self.list.range = Some((from, to));
        self
    }

    /// Reads the chart. The exact response is on the result, next to the
    /// typed chart.
    ///
    /// # Errors
    ///
    /// Any error of [`Client::call`], and
    /// [`ErrorKind::Decode`](crate::ErrorKind) when the answer is not the
    /// expected shape.
    pub async fn send(self) -> Result<Response<WeeklyArtistChart>, Error> {
        self.list
            .send(
                &methods::USER_GET_WEEKLY_ARTIST_CHART,
                WeeklyArtistChart::decode,
            )
            .await
    }
}

/// A `user.getWeeklyAlbumChart` query: the albums a user played in one
/// week. Made by [`User::weekly_album_chart`]. It is set and sent as a
/// [`WeeklyArtistChartQuery`] is.
#[derive(Debug, Clone)]
#[must_use = "a query does nothing until it is sent"]
pub struct WeeklyAlbumChartQuery {
    list: List,
}

impl WeeklyAlbumChartQuery {
    /// Sets `from` and `to`, the span to chart, as
    /// [`WeeklyArtistChartQuery::range`] does.
    pub fn range(mut self, from: u64, to: u64) -> Self {
        self.list.range = Some((from, to));
        self
    }

    /// Reads the chart. The exact response is on the result, next to the
    /// typed chart.
    ///
    /// # Errors
    ///
    /// As [`WeeklyArtistChartQuery::send`].
    pub async fn send(self) -> Result<Response<WeeklyAlbumChart>, Error> {
        self.list
            .send(
                &methods::USER_GET_WEEKLY_ALBUM_CHART,
                WeeklyAlbumChart::decode,
            )
            .await
    }
}

/// A `user.getWeeklyTrackChart` query: the tracks a user played in one
/// week. Made by [`User::weekly_track_chart`]. It is set and sent as a
/// [`WeeklyArtistChartQuery`] is.
#[derive(Debug, Clone)]
#[must_use = "a query does nothing until it is sent"]
pub struct WeeklyTrackChartQuery {
    list: List,
}

impl WeeklyTrackChartQuery {
    /// Sets `from` and `to`, the span to chart, as
    /// [`WeeklyArtistChartQuery::range`] does.
    pub fn range(mut self, from: u64, to: u64) -> Self {
        self.list.range = Some((from, to));
        self
    }

    /// Reads the chart. The exact response is on the result, next to the
    /// typed chart.
    ///
    /// # Errors
    ///
    /// As [`WeeklyArtistChartQuery::send`].
    pub async fn send(self) -> Result<Response<WeeklyTrackChart>, Error> {
        self.list
            .send(
                &methods::USER_GET_WEEKLY_TRACK_CHART,
                WeeklyTrackChart::decode,
            )
            .await
    }
}
