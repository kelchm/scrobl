//! A user's weekly charts: the replies to `user.getWeeklyChartList`,
//! `user.getWeeklyArtistChart`, `user.getWeeklyAlbumChart` and
//! `user.getWeeklyTrackChart`.

use std::fmt;

use serde_json::{Map, Value};

use super::top::{self, TopAlbum, TopArtist, TopTrack};
use crate::error::Error;
use crate::model::List;
use crate::model::shared::{self, missing_or};
use crate::protocol::{Raw, methods};

/// Decodes a `user.getWeeklyChartList` response: the weeks a user's charts
/// can be asked for, oldest first. The list's subject is the user.
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
/// assert_eq!((weeks[0].from(), weeks[0].to()), (1_108_296_000, 1_108_900_800));
/// # Ok::<(), scrobl::Error>(())
/// ```
///
/// # Errors
///
/// [`ErrorKind::Decode`](crate::ErrorKind) under [the rules every list
/// shares](crate::model::decode#errors). `@attr.user` is required, and a row
/// needs a `from` and a `to` that are numbers.
pub fn user_get_weekly_chart_list(raw: &Raw) -> Result<List<WeeklyChartRange>, Error> {
    shared::listed(
        raw,
        &methods::USER_GET_WEEKLY_CHART_LIST,
        "weeklychartlist",
        "chart",
        Some("user"),
        chart_range,
    )
}

/// One week a chart can be asked for: `from` and `to`, in seconds since the
/// Unix epoch. [`json`](Self::json) is the row exactly as decoded.
#[derive(Clone)]
pub struct WeeklyChartRange {
    from: u64,
    to: u64,
    json: Value,
}

impl WeeklyChartRange {
    /// The start of the week.
    pub fn from(&self) -> u64 {
        self.from
    }

    /// The end of the week, which is the start of the next one in the list.
    pub fn to(&self) -> u64 {
        self.to
    }

    /// The row exactly as decoded.
    pub fn json(&self) -> &Value {
        &self.json
    }
}

/// One row of a list of weeks, a user's or a tag's.
pub(in crate::model) fn chart_range(
    row: Map<String, Value>,
    at: &str,
) -> Result<WeeklyChartRange, Error> {
    Ok(WeeklyChartRange {
        from: shared::required_uint(&row, at, "from")?,
        to: shared::required_uint(&row, at, "to")?,
        json: Value::Object(row),
    })
}

impl fmt::Debug for WeeklyChartRange {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("WeeklyChartRange")
            .field("from", &self.from)
            .field("to", &self.to)
            .finish_non_exhaustive()
    }
}

/// Whose chart a reply is and the span it covers: `@attr.user`, `from` and
/// `to`.
#[derive(Clone)]
struct Span {
    user: String,
    from: u64,
    to: u64,
}

/// The span of a chart and its rows, which may be none: a week without a
/// scrobble is `[]`, and the reply carries no total to hold it to.
fn chart(
    list: &mut Map<String, Value>,
    root: &str,
    key: &str,
) -> Result<(Span, Vec<Value>), Error> {
    let at = format!("{root}.@attr");
    let span = match list.get("@attr") {
        Some(Value::Object(attr)) => Span {
            user: shared::required_str(attr, &at, "user")?.to_owned(),
            from: shared::required_uint(attr, &at, "from")?,
            to: shared::required_uint(attr, &at, "to")?,
        },
        _ => return Err(missing_or(&at, "an object")),
    };
    let rows = shared::rows(list, &format!("{root}.{key}"), key)?;
    Ok((span, rows))
}

/// Defines the reply to one of the three weekly chart methods. They differ
/// in the method, the names of the root and of the rows, and the row type.
macro_rules! weekly_chart {
    (
        $(#[$doc:meta])*
        $name:ident, $method:literal, $spec:ident, $root:literal, $key:literal,
        $rows:ident: $row:ty = $parse:path
    ) => {
        $(#[$doc])*
        #[derive(Clone)]
        pub struct $name {
            span: Span,
            $rows: Vec<$row>,
        }

        impl $name {
            #[doc = concat!("Decodes a `", $method, "` response.")]
            ///
            /// # Errors
            ///
            /// [`ErrorKind::Decode`](crate::ErrorKind) when the response is
            /// for another method, when the root or any of `@attr.user`,
            /// `@attr.from` and `@attr.to` is missing or malformed, when a
            /// row is not an object or lacks what its type requires, or
            /// when an object repeats a member name. The message names the
            #[doc = concat!("field, such as `", $root, ".", $key, "[3].playcount`, and never")]
            /// repeats response text.
            pub fn decode(raw: &Raw) -> Result<Self, Error> {
                shared::decode(raw, &methods::$spec, $root, |mut list| {
                    let (span, rows) = chart(&mut list, $root, $key)?;
                    let $rows = shared::each(rows, concat!($root, ".", $key), $parse)?;
                    Ok(Self { span, $rows })
                })
            }

            /// The user the service says the chart is of, in its own
            /// spelling.
            pub fn user(&self) -> &str {
                &self.span.user
            }

            /// The start of the span the chart covers, in seconds since the
            /// Unix epoch.
            pub fn from(&self) -> u64 {
                self.span.from
            }

            /// The end of the span the chart covers.
            pub fn to(&self) -> u64 {
                self.span.to
            }

            /// The rows, most played first. Empty for a span without a
            /// scrobble.
            pub fn $rows(&self) -> &[$row] {
                &self.$rows
            }
        }

        impl fmt::Debug for $name {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.debug_struct(stringify!($name))
                    .field("from", &self.span.from)
                    .field("to", &self.span.to)
                    .field(stringify!($rows), &self.$rows.len())
                    .finish_non_exhaustive()
            }
        }
    };
}

weekly_chart! {
    /// The reply to `user.getWeeklyArtistChart`: the artists a user played
    /// in one week, most played first. The method has no pages.
    ///
    /// ```
    /// use scrobl::model::WeeklyArtistChart;
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
    /// let chart = WeeklyArtistChart::decode(&raw)?;
    /// assert_eq!((chart.from(), chart.to()), (1_793_534_400, 1_794_139_200));
    /// assert_eq!(chart.artists()[0].artist().name(), "A");
    /// assert_eq!(chart.artists()[0].playcount(), 21);
    /// # Ok::<(), scrobl::Error>(())
    /// ```
    WeeklyArtistChart, "user.getWeeklyArtistChart", USER_GET_WEEKLY_ARTIST_CHART,
    "weeklyartistchart", "artist", artists: TopArtist = top::top_artist
}

weekly_chart! {
    /// The reply to `user.getWeeklyAlbumChart`: the albums a user played in
    /// one week, most played first. The method has no pages. Each album's
    /// artist comes as a name and an id, without a `url`.
    WeeklyAlbumChart, "user.getWeeklyAlbumChart", USER_GET_WEEKLY_ALBUM_CHART,
    "weeklyalbumchart", "album", albums: TopAlbum = top::top_album
}

weekly_chart! {
    /// The reply to `user.getWeeklyTrackChart`: the tracks a user played in
    /// one week, most played first. Each track's artist comes as a name and
    /// an id, without a `url`, and no row has a `duration`.
    ///
    /// The method has no pages, and the service has sent no more than 1000
    /// rows: a week of 1390 plays came back as 1000 tracks holding 1167 of
    /// them, with nothing in the reply to say the rest were left out.
    WeeklyTrackChart, "user.getWeeklyTrackChart", USER_GET_WEEKLY_TRACK_CHART,
    "weeklytrackchart", "track", tracks: TopTrack = top::top_track
}

#[cfg(test)]
mod tests;
