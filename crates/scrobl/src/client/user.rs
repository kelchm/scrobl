//! Typed reads about one user: their history, their profile, what they
//! play and tag most, of all time or in one span, what they gave a tag,
//! their friends, and the artists in their library.

use super::Client;
use super::query::{Ask, query};
use super::scan::Scan;
use crate::error::Error;
use crate::history::{RecentTracks, Window};
use crate::model::{
    Album, Artist, HistoryPage, List, LovedTrack, Track, User, UserAlbum, UserArtist, UserTag,
    UserTrack, decode,
};
use crate::protocol::{MethodSpec, methods};
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

/// One user, to ask about. Made by [`Client::user`].
///
/// The handle owns a clone of the client, so it can be moved into a task.
/// It sends nothing until a call on it is sent.
#[derive(Debug, Clone)]
pub struct UserHandle {
    client: Client,
    name: String,
}

impl UserHandle {
    pub(super) fn new(client: Client, name: String) -> Self {
        Self { client, name }
    }

    fn ask(&self, spec: &'static MethodSpec) -> Ask {
        Ask::new(&self.client, spec).about("user", &self.name)
    }

    fn tagged(&self, tag: String, kind: &'static str) -> Ask {
        self.ask(&methods::USER_GET_PERSONAL_TAGS)
            .about("tag", tag)
            .about("taggingtype", kind)
    }

    /// The user name as given.
    pub fn name(&self) -> &str {
        &self.name
    }

    /// The user's profile.
    #[doc(alias = "user.getInfo")]
    pub fn info(&self) -> UserInfoQuery {
        UserInfoQuery::new(self.ask(&methods::USER_GET_INFO))
    }

    /// The users this user follows.
    #[doc(alias = "user.getFriends")]
    pub fn friends(&self) -> UserFriendsQuery {
        UserFriendsQuery::new(self.ask(&methods::USER_GET_FRIENDS))
    }

    /// The user's history, newest first: one page of it, or a whole window
    /// read and checked by a [`Scan`].
    #[doc(alias = "user.getRecentTracks")]
    pub fn recent_tracks(&self) -> UserRecentTracksQuery {
        UserRecentTracksQuery {
            client: self.client.clone(),
            user: self.name.clone(),
            window: None,
            limit: None,
            page: None,
            extended: false,
            as_user: false,
        }
    }

    /// The tracks the user has loved, newest first.
    #[doc(alias = "user.getLovedTracks")]
    pub fn loved_tracks(&self) -> UserLovedTracksQuery {
        UserLovedTracksQuery::new(
            self.ask(&methods::USER_GET_LOVED_TRACKS)
                .at_most(1000, "`limit` must be between 1 and 1000"),
        )
    }

    /// The artists the user played most, of all time or in a [`Period`].
    #[doc(alias = "user.getTopArtists")]
    pub fn top_artists(&self) -> UserTopArtistsQuery {
        UserTopArtistsQuery::new(self.ask(&methods::USER_GET_TOP_ARTISTS))
    }

    /// The albums the user played most, of all time or in a [`Period`].
    #[doc(alias = "user.getTopAlbums")]
    pub fn top_albums(&self) -> UserTopAlbumsQuery {
        UserTopAlbumsQuery::new(self.ask(&methods::USER_GET_TOP_ALBUMS))
    }

    /// The tracks the user played most, of all time or in a [`Period`].
    #[doc(alias = "user.getTopTracks")]
    pub fn top_tracks(&self) -> UserTopTracksQuery {
        UserTopTracksQuery::new(self.ask(&methods::USER_GET_TOP_TRACKS))
    }

    /// The tags the user applied most.
    #[doc(alias = "user.getTopTags")]
    pub fn top_tags(&self) -> UserTopTagsQuery {
        UserTopTagsQuery::new(self.ask(&methods::USER_GET_TOP_TAGS))
    }

    /// The weeks the service keeps the user's charts for, oldest first.
    /// The last is the most recent week that is complete.
    #[doc(alias = "user.getWeeklyChartList")]
    pub fn weeks(&self) -> UserWeeksQuery {
        UserWeeksQuery::new(self.ask(&methods::USER_GET_WEEKLY_CHART_LIST))
    }

    /// The artists the user played most in `window`, which may be one of
    /// their [`weeks`](Self::weeks) or any other span with both its ends.
    #[doc(alias = "user.getWeeklyArtistChart")]
    pub fn top_artists_in(&self, window: Window) -> UserTopArtistsInQuery {
        UserTopArtistsInQuery::new(
            self.ask(&methods::USER_GET_WEEKLY_ARTIST_CHART)
                .within(window),
        )
    }

    /// The albums the user played most in `window`, as
    /// [`top_artists_in`](Self::top_artists_in) gives the artists.
    #[doc(alias = "user.getWeeklyAlbumChart")]
    pub fn top_albums_in(&self, window: Window) -> UserTopAlbumsInQuery {
        UserTopAlbumsInQuery::new(
            self.ask(&methods::USER_GET_WEEKLY_ALBUM_CHART)
                .within(window),
        )
    }

    /// The tracks the user played most in `window`, as
    /// [`top_artists_in`](Self::top_artists_in) gives the artists.
    #[doc(alias = "user.getWeeklyTrackChart")]
    pub fn top_tracks_in(&self, window: Window) -> UserTopTracksInQuery {
        UserTopTracksInQuery::new(
            self.ask(&methods::USER_GET_WEEKLY_TRACK_CHART)
                .within(window),
        )
    }

    /// The artists the user has given `tag`.
    #[doc(alias = "user.getPersonalTags")]
    pub fn tagged_artists(&self, tag: impl Into<String>) -> UserTaggedArtistsQuery {
        UserTaggedArtistsQuery::new(self.tagged(tag.into(), "artist"))
    }

    /// The albums the user has given `tag`.
    #[doc(alias = "user.getPersonalTags")]
    pub fn tagged_albums(&self, tag: impl Into<String>) -> UserTaggedAlbumsQuery {
        UserTaggedAlbumsQuery::new(self.tagged(tag.into(), "album"))
    }

    /// The tracks the user has given `tag`.
    #[doc(alias = "user.getPersonalTags")]
    pub fn tagged_tracks(&self, tag: impl Into<String>) -> UserTaggedTracksQuery {
        UserTaggedTracksQuery::new(self.tagged(tag.into(), "track"))
    }

    /// Every artist the user has played, most played first.
    #[doc(alias = "library.getArtists")]
    pub fn library_artists(&self) -> UserLibraryArtistsQuery {
        UserLibraryArtistsQuery::new(self.ask(&methods::LIBRARY_GET_ARTISTS))
    }
}

query! {
    /// A user's profile. Made by [`UserHandle::info`].
    ///
    /// ```no_run
    /// # async fn demo(client: scrobl::Client) -> Result<(), scrobl::Error> {
    /// let user = client.user("rj").info().send().await?;
    /// println!("{} has {} scrobbles", user.name(), user.playcount().unwrap_or_default());
    /// # Ok(())
    /// # }
    /// ```
    UserInfoQuery => User = decode::user_get_info;
}

query! {
    /// The users a user follows. Made by [`UserHandle::friends`].
    ///
    /// The method's `recenttracks` parameter is not offered: the service
    /// answered with the same bytes with it and without.
    UserFriendsQuery => Page<User> = decode::user_get_friends;
    limit = "Sets `limit`, the page size, from 1 to 500. The service refuses a larger one.",
    page = "Sets `page`, from 1. The service answers a page beyond the last with \
            [`INVALID_PARAMETERS`](crate::ApiErrorCode::INVALID_PARAMETERS), not with an \
            empty page."
}

query! {
    /// The tracks a user has loved, newest first. Made by
    /// [`UserHandle::loved_tracks`].
    ///
    /// Nothing ties one page to the next: a track loved or unloved between
    /// two requests moves every later row, so a caller reading the whole
    /// list should compare each page's [`total`](crate::model::Page::total)
    /// and start again when it changes.
    ///
    /// ```no_run
    /// # async fn demo(client: scrobl::Client) -> Result<(), scrobl::Error> {
    /// let page = client.user("rj").loved_tracks().limit(200).send().await?;
    /// for loved in page.iter() {
    ///     println!("{} at {}", loved.track().name(), loved.loved_at());
    /// }
    /// # Ok(())
    /// # }
    /// ```
    UserLovedTracksQuery => Page<LovedTrack> = decode::user_get_loved_tracks;
    limit = "Sets `limit`, the page size, from 1 to 1000. The client refuses a larger one: \
             the method documents no maximum, and 1000 is the largest the service has been \
             seen to honour. The service uses 50 when none is sent.",
    page
}

query! {
    /// The artists a user played most. Made by [`UserHandle::top_artists`].
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
    UserTopArtistsQuery => Page<UserArtist> = decode::user_get_top_artists;
    period,
    limit = "Sets `limit`, the page size, from 1 to 1000. The service refuses a larger one.",
    page
}

query! {
    /// The albums a user played most. Made by [`UserHandle::top_albums`].
    UserTopAlbumsQuery => Page<UserAlbum> = decode::user_get_top_albums;
    period,
    limit = "Sets `limit`, the page size, from 1. The service has applied 1000 when asked; \
             the page says what size it applied, in \
             [`per_page`](crate::model::Page::per_page).",
    page
}

query! {
    /// The tracks a user played most. Made by [`UserHandle::top_tracks`].
    UserTopTracksQuery => Page<UserTrack> = decode::user_get_top_tracks;
    period,
    limit = "Sets `limit`, the page size, from 1. The service has applied 1000 when asked; \
             the page says what size it applied, in \
             [`per_page`](crate::model::Page::per_page).",
    page
}

query! {
    /// The tags a user applied most. Made by [`UserHandle::top_tags`]. The
    /// service does not page this list.
    ///
    /// ```no_run
    /// # async fn demo(client: scrobl::Client) -> Result<(), scrobl::Error> {
    /// let top = client.user("rj").top_tags().limit(10).send().await?;
    /// for row in top.iter() {
    ///     println!("{} ({})", row.tag().name(), row.count());
    /// }
    /// # Ok(())
    /// # }
    /// ```
    UserTopTagsQuery => List<UserTag> = decode::user_get_top_tags;
    limit = "Sets `limit`, the most tags to read, from 1."
}

query! {
    /// The weeks the service keeps a user's charts for. Made by
    /// [`UserHandle::weeks`].
    ///
    /// The list can start before the user's first scrobble.
    UserWeeksQuery => List<Window> = decode::user_get_weekly_chart_list;
}

query! {
    /// The artists a user played most in one span of time. Made by
    /// [`UserHandle::top_artists_in`].
    ///
    /// The window needs both its ends: with one or none the service charts
    /// a span of its own choosing, so the client refuses to send. To count
    /// all of a history, or a span back from now, use
    /// [`UserHandle::top_artists`].
    ///
    /// ```no_run
    /// # async fn demo(client: scrobl::Client) -> Result<(), scrobl::Error> {
    /// let user = client.user("rj");
    /// let weeks = user.weeks().send().await?;
    /// if let Some(week) = weeks.last() {
    ///     let chart = user.top_artists_in(*week).limit(10).send().await?;
    ///     for (place, top) in chart.ranked() {
    ///         println!("{place}. {} ({})", top.artist().name(), top.playcount());
    ///     }
    /// }
    /// # Ok(())
    /// # }
    /// ```
    UserTopArtistsInQuery => List<UserArtist> = decode::user_get_weekly_artist_chart;
    limit = "Sets `limit`, the most rows to read, from 1. The method's page documents no \
             such parameter; the service applied it on 2026-11-10."
}

query! {
    /// The albums a user played most in one span of time. Made by
    /// [`UserHandle::top_albums_in`]. The window is as for a
    /// [`UserTopArtistsInQuery`].
    UserTopAlbumsInQuery => List<UserAlbum> = decode::user_get_weekly_album_chart;
    limit = "Sets `limit`, as [`UserTopArtistsInQuery::limit`] does."
}

query! {
    /// The tracks a user played most in one span of time. Made by
    /// [`UserHandle::top_tracks_in`]. The window is as for a
    /// [`UserTopArtistsInQuery`].
    ///
    /// The list can stop at 1000 rows without saying so: a week of 1390
    /// plays came back as 1000 tracks holding 1167 of them.
    UserTopTracksInQuery => List<UserTrack> = decode::user_get_weekly_track_chart;
    limit = "Sets `limit`, as [`UserTopArtistsInQuery::limit`] does."
}

query! {
    /// The artists a user has given one tag. Made by
    /// [`UserHandle::tagged_artists`].
    ///
    /// ```no_run
    /// # async fn demo(client: scrobl::Client) -> Result<(), scrobl::Error> {
    /// let page = client.user("rj").tagged_artists("rock").send().await?;
    /// for artist in page.iter() {
    ///     println!("{}", artist.name());
    /// }
    /// # Ok(())
    /// # }
    /// ```
    UserTaggedArtistsQuery => Page<Artist> = decode::user_get_personal_tags_artists;
    limit = "Sets `limit`, the page size, from 1 to 1000. The service refuses a larger one.",
    page
}

query! {
    /// The albums a user has given one tag. Made by
    /// [`UserHandle::tagged_albums`].
    UserTaggedAlbumsQuery => Page<Album> = decode::user_get_personal_tags_albums;
    limit = "Sets `limit`, the page size, from 1 to 1000. The service refuses a larger one.",
    page
}

query! {
    /// The tracks a user has given one tag. Made by
    /// [`UserHandle::tagged_tracks`].
    UserTaggedTracksQuery => Page<Track> = decode::user_get_personal_tags_tracks;
    limit = "Sets `limit`, the page size, from 1 to 1000. The service refuses a larger one.",
    page
}

query! {
    /// Every artist a user has played, most played first. Made by
    /// [`UserHandle::library_artists`].
    ///
    /// ```no_run
    /// # async fn demo(client: scrobl::Client) -> Result<(), scrobl::Error> {
    /// let page = client.user("rj").library_artists().limit(200).send().await?;
    /// println!("{} artists", page.total());
    /// # Ok(())
    /// # }
    /// ```
    UserLibraryArtistsQuery => Page<UserArtist> = decode::library_get_artists;
    limit = "Sets `limit`, the page size, from 1. The service has applied 2000 when asked; \
             the page says what size it applied, in \
             [`per_page`](crate::model::Page::per_page).",
    page
}

/// A user's history, newest first. Made by [`UserHandle::recent_tracks`].
///
/// [`send`](Self::send) reads one page. [`scan`](Self::scan) reads a whole
/// window, page after page, and checks every page.
///
/// ```no_run
/// # async fn demo(client: scrobl::Client) -> Result<(), scrobl::Error> {
/// let page = client.user("rj").recent_tracks().limit(50).send().await?;
/// for scrobble in page.scrobbles() {
///     println!("{}", scrobble.track().name());
/// }
/// # Ok(())
/// # }
/// ```
#[derive(Debug, Clone)]
#[must_use = "a query does nothing until it is sent or scanned"]
pub struct UserRecentTracksQuery {
    client: Client,
    user: String,
    window: Option<Window>,
    limit: Option<u32>,
    page: Option<u32>,
    extended: bool,
    as_user: bool,
}

impl UserRecentTracksQuery {
    /// Restricts the query to a window: `from` inclusive, `to` exclusive.
    /// Applies to a scan too; a scan without one reads
    /// [`Window::ALL`].
    pub fn window(mut self, window: Window) -> Self {
        self.window = Some(window);
        self
    }

    /// The page size, 1 to 200. Checked by [`send`](Self::send) and
    /// [`scan`](Self::scan). A scan uses it as the page size of every
    /// request, 200 when unset.
    pub fn limit(mut self, limit: u32) -> Self {
        self.limit = Some(limit);
        self
    }

    /// The page to read, counting from 1. Checked by [`send`](Self::send).
    /// A scan chooses its own pages, so [`scan`](Self::scan) refuses a query
    /// that set one.
    pub fn page(mut self, page: u32) -> Self {
        self.page = Some(page);
        self
    }

    /// Asks for the extended row shape: artist details and the loved flag.
    /// Applies to a scan too.
    pub fn extended(mut self, extended: bool) -> Self {
        self.extended = extended;
        self
    }

    /// Makes the call as the session's user, which is how a hidden history
    /// is read. Needs a client with a secret and a session. Applies to a
    /// scan too. See [`Request::as_user`](crate::protocol::Request::as_user).
    pub fn as_user(mut self) -> Self {
        self.as_user = true;
        self
    }

    /// Reads one page.
    ///
    /// The page is decoded but not checked against any other page; use
    /// [`scan`](Self::scan) to read a window completely. The exact response
    /// is on the result, next to the typed page.
    ///
    /// # Errors
    ///
    /// [`ErrorKind::InvalidRequest`](crate::ErrorKind) for a `limit` outside
    /// 1 to 200 or a `page` of 0, before anything is sent; any error of
    /// [`Client::call`]; and [`ErrorKind::Decode`](crate::ErrorKind) when the
    /// page is not the expected shape.
    pub async fn send(self) -> Result<Response<HistoryPage>, Error> {
        let (client, query) = self.into_core();
        let raw = client.call(&query.request()?).await?;
        let page = decode::user_get_recent_tracks(&raw)?;
        Ok(Response::new(page, raw))
    }

    /// Starts reading the whole window, page after page.
    ///
    /// The scan takes the window ([`Window::ALL`] when unset), the page size
    /// ([`limit`](Self::limit), 200 when unset), [`extended`](Self::extended)
    /// and [`as_user`](Self::as_user) of this query, and cannot be changed
    /// afterwards. Nothing is sent until the first [`Scan::next_page`].
    ///
    /// # Errors
    ///
    /// [`ErrorKind::InvalidRequest`](crate::ErrorKind), before anything is
    /// sent, for a `limit` outside 1 to 200 or when [`page`](Self::page) was
    /// set: a scan chooses its own pages.
    pub fn scan(self) -> Result<Scan, Error> {
        let (client, query) = self.into_core();
        Ok(Scan::new(client, query.scan()?))
    }

    fn into_core(self) -> (Client, RecentTracks) {
        let mut query = RecentTracks::new(self.user)
            .extended(self.extended)
            .window(self.window.unwrap_or(Window::ALL));
        if let Some(limit) = self.limit {
            query = query.limit(limit);
        }
        if let Some(page) = self.page {
            query = query.page(page);
        }
        if self.as_user {
            query = query.as_user();
        }
        (self.client, query)
    }
}
