//! Typed reads of a user's profile and lists: what they play and tag most,
//! of all time or in one week, what they gave a tag, their friends, and
//! the artists in their library.

use super::{Client, User};
use crate::error::Error;
use crate::model::{
    Album, Artist, List, Page, Track, User as UserEntity, UserAlbum, UserArtist, UserTag,
    UserTrack, WeeklyAlbumChart, WeeklyArtistChart, WeeklyChartRange, WeeklyTrackChart, decode,
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
    /// Starts a `user.getInfo` query for this user.
    pub fn info(&self) -> UserInfoQuery {
        UserInfoQuery {
            list: Listing::of(self),
        }
    }

    /// Starts a `user.getFriends` query for this user.
    pub fn friends(&self) -> FriendsQuery {
        FriendsQuery {
            list: Listing::of(self),
        }
    }

    /// Starts a `user.getPersonalTags` query for the artists this user has
    /// given `tag`.
    pub fn tagged_artists(&self, tag: impl Into<String>) -> TaggedArtistsQuery {
        TaggedArtistsQuery {
            list: Listing::tagged(self, tag.into(), "artist"),
        }
    }

    /// Starts a `user.getPersonalTags` query for the albums this user has
    /// given `tag`.
    pub fn tagged_albums(&self, tag: impl Into<String>) -> TaggedAlbumsQuery {
        TaggedAlbumsQuery {
            list: Listing::tagged(self, tag.into(), "album"),
        }
    }

    /// Starts a `user.getPersonalTags` query for the tracks this user has
    /// given `tag`.
    pub fn tagged_tracks(&self, tag: impl Into<String>) -> TaggedTracksQuery {
        TaggedTracksQuery {
            list: Listing::tagged(self, tag.into(), "track"),
        }
    }

    /// Starts a `user.getTopArtists` query for this user.
    pub fn top_artists(&self) -> TopArtistsQuery {
        TopArtistsQuery {
            list: Listing::of(self),
        }
    }

    /// Starts a `user.getTopAlbums` query for this user.
    pub fn top_albums(&self) -> TopAlbumsQuery {
        TopAlbumsQuery {
            list: Listing::of(self),
        }
    }

    /// Starts a `user.getTopTracks` query for this user.
    pub fn top_tracks(&self) -> TopTracksQuery {
        TopTracksQuery {
            list: Listing::of(self),
        }
    }

    /// Starts a `user.getTopTags` query for this user.
    pub fn top_tags(&self) -> TopTagsQuery {
        TopTagsQuery {
            list: Listing::of(self),
        }
    }

    /// Starts a `user.getWeeklyChartList` query for this user.
    pub fn weekly_chart_list(&self) -> WeeklyChartListQuery {
        WeeklyChartListQuery {
            list: Listing::of(self),
        }
    }

    /// Starts a `user.getWeeklyArtistChart` query for this user.
    pub fn weekly_artist_chart(&self) -> WeeklyArtistChartQuery {
        WeeklyArtistChartQuery {
            list: Listing::of(self),
        }
    }

    /// Starts a `user.getWeeklyAlbumChart` query for this user.
    pub fn weekly_album_chart(&self) -> WeeklyAlbumChartQuery {
        WeeklyAlbumChartQuery {
            list: Listing::of(self),
        }
    }

    /// Starts a `user.getWeeklyTrackChart` query for this user.
    pub fn weekly_track_chart(&self) -> WeeklyTrackChartQuery {
        WeeklyTrackChartQuery {
            list: Listing::of(self),
        }
    }

    /// Starts a `library.getArtists` query for this user.
    pub fn library_artists(&self) -> LibraryArtistsQuery {
        LibraryArtistsQuery {
            list: Listing::of(self),
        }
    }
}

/// What every list query holds: whose list, and which part of it.
#[derive(Debug, Clone)]
pub(super) struct Listing {
    client: Client,
    /// The parameter that says whose list it is, such as `user` or `tag`,
    /// with its value. A chart of the whole service has none.
    owner: Option<(&'static str, String)>,
    pub(super) limit: Option<u32>,
    pub(super) page: Option<u32>,
    period: Option<Period>,
    range: Option<(u64, u64)>,
    /// The tag and the `taggingtype` of a `user.getPersonalTags` query.
    tagged: Option<(String, &'static str)>,
}

impl Listing {
    fn of(user: &User) -> Self {
        Self::new(user.client.clone(), Some(("user", user.name.clone())))
    }

    pub(super) fn new(client: Client, owner: Option<(&'static str, String)>) -> Self {
        Self {
            client,
            owner,
            limit: None,
            page: None,
            period: None,
            range: None,
            tagged: None,
        }
    }

    fn tagged(user: &User, tag: String, kind: &'static str) -> Self {
        Self {
            tagged: Some((tag, kind)),
            ..Self::of(user)
        }
    }

    /// Sends the request for `spec`, with the owner and whatever else was
    /// set, and decodes the answer.
    pub(super) async fn send<T>(
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
        let mut request = Request::new(spec);
        if let Some((name, value)) = &self.owner {
            request = request.param(name, value);
        }
        if let Some((tag, kind)) = &self.tagged {
            request = request.param("tag", tag).param("taggingtype", *kind);
        }
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
/// for (place, top) in page.ranked() {
///     println!("{place}. {} ({})", top.artist().name(), top.playcount());
/// }
/// # Ok(())
/// # }
/// ```
#[derive(Debug, Clone)]
#[must_use = "a query does nothing until it is sent"]
pub struct TopArtistsQuery {
    list: Listing,
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
    pub async fn send(self) -> Result<Response<Page<UserArtist>>, Error> {
        self.list
            .send(&methods::USER_GET_TOP_ARTISTS, decode::user_get_top_artists)
            .await
    }
}

/// A `user.getTopAlbums` query: the albums a user played most. Made by
/// [`User::top_albums`]. It is set and sent as a [`TopArtistsQuery`] is.
#[derive(Debug, Clone)]
#[must_use = "a query does nothing until it is sent"]
pub struct TopAlbumsQuery {
    list: Listing,
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
    /// [`per_page`](crate::model::Page::per_page).
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
    pub async fn send(self) -> Result<Response<Page<UserAlbum>>, Error> {
        self.list
            .send(&methods::USER_GET_TOP_ALBUMS, decode::user_get_top_albums)
            .await
    }
}

/// A `user.getTopTracks` query: the tracks a user played most. Made by
/// [`User::top_tracks`]. It is set and sent as a [`TopArtistsQuery`] is.
#[derive(Debug, Clone)]
#[must_use = "a query does nothing until it is sent"]
pub struct TopTracksQuery {
    list: Listing,
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
    /// [`per_page`](crate::model::Page::per_page).
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
    pub async fn send(self) -> Result<Response<Page<UserTrack>>, Error> {
        self.list
            .send(&methods::USER_GET_TOP_TRACKS, decode::user_get_top_tracks)
            .await
    }
}

/// A `user.getTopTags` query: the tags a user applied most. Made by
/// [`User::top_tags`]. The method has no pages.
///
/// ```no_run
/// # async fn demo(client: scrobl::Client) -> Result<(), scrobl::Error> {
/// let top = client.user("rj").top_tags().limit(10).send().await?;
/// for tag in top.items() {
///     println!("{} ({})", tag.tag().name(), tag.count());
/// }
/// # Ok(())
/// # }
/// ```
#[derive(Debug, Clone)]
#[must_use = "a query does nothing until it is sent"]
pub struct TopTagsQuery {
    list: Listing,
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
    pub async fn send(self) -> Result<Response<List<UserTag>>, Error> {
        self.list
            .send(&methods::USER_GET_TOP_TAGS, decode::user_get_top_tags)
            .await
    }
}

/// A `library.getArtists` query: every artist a user has played, most
/// played first. Made by [`User::library_artists`].
///
/// ```no_run
/// # async fn demo(client: scrobl::Client) -> Result<(), scrobl::Error> {
/// let page = client.user("rj").library_artists().limit(200).send().await?;
/// println!("{} artists", page.total());
/// # Ok(())
/// # }
/// ```
#[derive(Debug, Clone)]
#[must_use = "a query does nothing until it is sent"]
pub struct LibraryArtistsQuery {
    list: Listing,
}

impl LibraryArtistsQuery {
    /// Sets `limit`, the page size, from 1. The service has applied 2000
    /// when asked; the page says what size it applied, in
    /// [`per_page`](crate::model::Page::per_page).
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
    pub async fn send(self) -> Result<Response<Page<UserArtist>>, Error> {
        self.list
            .send(&methods::LIBRARY_GET_ARTISTS, decode::library_get_artists)
            .await
    }
}

/// A `user.getWeeklyChartList` query: the weeks a user's charts can be
/// asked for. Made by [`User::weekly_chart_list`].
#[derive(Debug, Clone)]
#[must_use = "a query does nothing until it is sent"]
pub struct WeeklyChartListQuery {
    list: Listing,
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
    pub async fn send(self) -> Result<Response<List<WeeklyChartRange>>, Error> {
        self.list
            .send(
                &methods::USER_GET_WEEKLY_CHART_LIST,
                decode::user_get_weekly_chart_list,
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
/// if let Some(week) = weeks.last() {
///     let chart = user
///         .weekly_artist_chart()
///         .range(week.from(), week.to())
///         .send()
///         .await?;
///     for (place, top) in chart.artists().iter().enumerate() {
///         println!("{}. {} ({})", place + 1, top.artist().name(), top.playcount());
///     }
/// }
/// # Ok(())
/// # }
/// ```
#[derive(Debug, Clone)]
#[must_use = "a query does nothing until it is sent"]
pub struct WeeklyArtistChartQuery {
    list: Listing,
}

impl WeeklyArtistChartQuery {
    /// Sets `from` and `to`, the span to chart, in seconds since the Unix
    /// epoch, such as a week from [`User::weekly_chart_list`]. Without them the
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
    list: Listing,
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
    list: Listing,
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

/// A `user.getInfo` query: a user's profile. Made by [`User::info`].
///
/// ```no_run
/// # async fn demo(client: scrobl::Client) -> Result<(), scrobl::Error> {
/// let user = client.user("rj").info().send().await?;
/// println!("{} has {} scrobbles", user.name(), user.playcount().unwrap_or_default());
/// # Ok(())
/// # }
/// ```
#[derive(Debug, Clone)]
#[must_use = "a query does nothing until it is sent"]
pub struct UserInfoQuery {
    list: Listing,
}

impl UserInfoQuery {
    /// Reads the profile. The exact response is on the result, next to the
    /// typed profile.
    ///
    /// # Errors
    ///
    /// Any error of [`Client::call`], and
    /// [`ErrorKind::Decode`](crate::ErrorKind) when the answer is not the
    /// expected shape.
    pub async fn send(self) -> Result<Response<UserEntity>, Error> {
        self.list
            .send(&methods::USER_GET_INFO, decode::user_get_info)
            .await
    }
}

/// A `user.getFriends` query: the users a user follows. Made by
/// [`User::friends`].
///
/// The method's `recenttracks` parameter is not offered: the service
/// answered with the same bytes with it and without.
#[derive(Debug, Clone)]
#[must_use = "a query does nothing until it is sent"]
pub struct FriendsQuery {
    list: Listing,
}

impl FriendsQuery {
    /// Sets `limit`, the page size, from 1 to 500. The service answers a
    /// larger one with
    /// [`INVALID_PARAMETERS`](crate::ApiErrorCode::INVALID_PARAMETERS).
    pub fn limit(mut self, limit: u32) -> Self {
        self.list.limit = Some(limit);
        self
    }

    /// Sets `page`, from 1. The service answers a page beyond the last
    /// with [`INVALID_PARAMETERS`](crate::ApiErrorCode::INVALID_PARAMETERS),
    /// not with an empty page.
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
    pub async fn send(self) -> Result<Response<Page<UserEntity>>, Error> {
        self.list
            .send(&methods::USER_GET_FRIENDS, decode::user_get_friends)
            .await
    }
}

/// A `user.getPersonalTags` query for artists: the artists a user has given
/// one tag. Made by [`User::tagged_artists`].
///
/// ```no_run
/// # async fn demo(client: scrobl::Client) -> Result<(), scrobl::Error> {
/// let page = client.user("rj").tagged_artists("rock").send().await?;
/// for tagged in page.items() {
///     println!("{}", tagged.name());
/// }
/// # Ok(())
/// # }
/// ```
#[derive(Debug, Clone)]
#[must_use = "a query does nothing until it is sent"]
pub struct TaggedArtistsQuery {
    list: Listing,
}

impl TaggedArtistsQuery {
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
    /// As [`TopArtistsQuery::send`].
    pub async fn send(self) -> Result<Response<Page<Artist>>, Error> {
        self.list
            .send(
                &methods::USER_GET_PERSONAL_TAGS,
                decode::user_get_personal_tags_artists,
            )
            .await
    }
}

/// A `user.getPersonalTags` query for albums: the albums a user has given
/// one tag. Made by [`User::tagged_albums`]. It is set and sent as a
/// [`TaggedArtistsQuery`] is.
#[derive(Debug, Clone)]
#[must_use = "a query does nothing until it is sent"]
pub struct TaggedAlbumsQuery {
    list: Listing,
}

impl TaggedAlbumsQuery {
    /// Sets `limit`, the page size, as [`TaggedArtistsQuery::limit`] does.
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
    pub async fn send(self) -> Result<Response<Page<Album>>, Error> {
        self.list
            .send(
                &methods::USER_GET_PERSONAL_TAGS,
                decode::user_get_personal_tags_albums,
            )
            .await
    }
}

/// A `user.getPersonalTags` query for tracks: the tracks a user has given
/// one tag. Made by [`User::tagged_tracks`]. It is set and sent as a
/// [`TaggedArtistsQuery`] is.
#[derive(Debug, Clone)]
#[must_use = "a query does nothing until it is sent"]
pub struct TaggedTracksQuery {
    list: Listing,
}

impl TaggedTracksQuery {
    /// Sets `limit`, the page size, as [`TaggedArtistsQuery::limit`] does.
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
    pub async fn send(self) -> Result<Response<Page<Track>>, Error> {
        self.list
            .send(
                &methods::USER_GET_PERSONAL_TAGS,
                decode::user_get_personal_tags_tracks,
            )
            .await
    }
}
