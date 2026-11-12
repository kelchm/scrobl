//! What a user played in one span of time: the replies to
//! `user.getWeeklyChartList`, `user.getWeeklyArtistChart`,
//! `user.getWeeklyAlbumChart` and `user.getWeeklyTrackChart`.

use serde_json::{Map, Value};

use super::top::{self, UserAlbum, UserArtist, UserTrack};
use crate::error::Error;
use crate::history::Window;
use crate::model::List;
use crate::model::shared::{self, missing_or};
use crate::protocol::{Raw, methods};

/// Decodes a `user.getWeeklyChartList` response: the weeks the service
/// keeps a user's charts for, oldest first. The list's subject is the user.
///
/// The list can start before the user's first scrobble, and a week without
/// one has an empty chart.
///
/// ```
/// use scrobl::model::decode;
/// use scrobl::protocol::{self, HttpResponse, Request, methods};
///
/// let body = r##"{"weeklychartlist": {
///     "chart": [{"#text": "", "from": "1108296000", "to": "1108900800"}],
///     "@attr": {"user": "rj"}
/// }}"##;
/// let request = Request::new(&methods::USER_GET_WEEKLY_CHART_LIST).param("user", "rj");
/// let raw = protocol::decode(&request, HttpResponse::new(200, body))?;
///
/// let weeks = decode::user_get_weekly_chart_list(&raw)?;
/// assert_eq!(weeks[0].from(), Some(1_108_296_000));
/// assert_eq!(weeks[0].to(), Some(1_108_900_800));
/// # Ok::<(), scrobl::Error>(())
/// ```
///
/// # Errors
///
/// [`ErrorKind::Decode`](crate::ErrorKind) under [the rules every list
/// shares](crate::model::decode#errors). `@attr.user` is required, and a row
/// needs a `from` and a later `to` that are numbers.
pub fn user_get_weekly_chart_list(raw: &Raw) -> Result<List<Window>, Error> {
    shared::listed(
        raw,
        &methods::USER_GET_WEEKLY_CHART_LIST,
        "weeklychartlist",
        "chart",
        Some("user"),
        week,
    )
}

/// One row of a list of weeks, a user's or a tag's.
pub(in crate::model) fn week(row: Map<String, Value>, at: &str) -> Result<Window, Error> {
    let from = shared::required_uint(&row, at, "from")?;
    let to = shared::required_uint(&row, at, "to")?;
    Window::new(from, to).map_err(|_| missing_or(&format!("{at}.to"), "later than `from`"))
}

/// Defines the decoding of one of the three chart methods. They differ in
/// the method, the names of the root and of the rows, and the row type.
macro_rules! chart {
    (
        $(#[$doc:meta])*
        $name:ident, $spec:ident, $root:literal, $key:literal, $row:ty = $parse:path
    ) => {
        $(#[$doc])*
        ///
        /// The list's subject is the user. The span the chart covers is in
        /// the response's `@attr`, which this does not read: a caller knows
        /// the span it asked for.
        ///
        /// # Errors
        ///
        /// [`ErrorKind::Decode`](crate::ErrorKind) under [the rules every
        /// list shares](crate::model::decode#errors). `@attr.user` is
        /// required, and a row needs a `name` and a `playcount`.
        pub fn $name(raw: &Raw) -> Result<List<$row>, Error> {
            shared::listed(raw, &methods::$spec, $root, $key, Some("user"), $parse)
        }
    };
}

chart! {
    /// Decodes a `user.getWeeklyArtistChart` response: the artists a user
    /// played in one span of time, most played first. A span without a
    /// scrobble is an empty list.
    ///
    /// ```
    /// use scrobl::model::decode;
    /// use scrobl::protocol::{self, HttpResponse, Request, methods};
    ///
    /// let body = r##"{"weeklyartistchart": {
    ///     "artist": [{"name": "A", "mbid": "", "url": "https://www.last.fm/music/A",
    ///                 "playcount": "21", "@attr": {"rank": "1"}}],
    ///     "@attr": {"user": "rj", "from": "1793534400", "to": "1794139200"}
    /// }}"##;
    /// let request = Request::new(&methods::USER_GET_WEEKLY_ARTIST_CHART).param("user", "rj");
    /// let raw = protocol::decode(&request, HttpResponse::new(200, body))?;
    ///
    /// let chart = decode::user_get_weekly_artist_chart(&raw)?;
    /// assert_eq!(chart.subject(), Some("rj"));
    /// assert_eq!((chart[0].artist().name(), chart[0].playcount()), ("A", 21));
    /// # Ok::<(), scrobl::Error>(())
    /// ```
    user_get_weekly_artist_chart, USER_GET_WEEKLY_ARTIST_CHART,
    "weeklyartistchart", "artist", UserArtist = top::user_artist
}

chart! {
    /// Decodes a `user.getWeeklyAlbumChart` response: the albums a user
    /// played in one span of time, most played first. Each album's artist
    /// comes as a name and an id, without a `url`.
    user_get_weekly_album_chart, USER_GET_WEEKLY_ALBUM_CHART,
    "weeklyalbumchart", "album", UserAlbum = top::user_album
}

chart! {
    /// Decodes a `user.getWeeklyTrackChart` response: the tracks a user
    /// played in one span of time, most played first. Each track's artist
    /// comes as a name and an id, without a `url`, and no row has a
    /// `duration`.
    ///
    /// The service has sent no more than 1000 rows: a week of 1390 plays
    /// came back as 1000 tracks holding 1167 of them, with nothing in the
    /// reply to say the rest were left out.
    user_get_weekly_track_chart, USER_GET_WEEKLY_TRACK_CHART,
    "weeklytrackchart", "track", UserTrack = top::user_track
}

#[cfg(test)]
mod tests;
