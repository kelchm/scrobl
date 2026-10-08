//! A fake Last.fm for `user.getRecentTracks`: a pure function from a
//! [`Dataset`] and an [`HttpRequest`] to `(status, body)`. No sockets, so a
//! later socket-based server can serve the same bytes.
//!
//! provenance: synthetic. Nothing here was recorded. The row and `@attr`
//! shapes follow the recorded and documented ones (string-typed `@attr`
//! values, the plain and extended artist, `@attr.nowplaying` as `"true"`, a
//! single object where one row is returned). The data are invented.
//!
//! Real Last.fm has no event id. Each synthetic scrobble carries a unique
//! marker in its track `mbid`, so a test can see a duplicated or omitted
//! row that a correct row count would hide. Names, artists and timestamps
//! can repeat freely.
//!
//! The service holds its scrobbles newest first and honours `user` (case
//! insensitive, echoing its own spelling), `page`, `limit` (up to a
//! configurable cap), `from` (inclusive), `to` (exclusive) and `extended`.
//! [`Faults`] switch on the ways a real reply could go wrong.

#![allow(
    dead_code,
    unreachable_pub,
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]

use scrobl::history::Window;
use scrobl::protocol::HttpRequest;
use serde_json::{Value, json};

/// A timestamp near the start of every synthetic history.
pub const BASE: u64 = 1_700_000_000;

/// The marker of the now-playing row, which is no scrobble of the dataset.
pub const NOW_PLAYING_MARKER: usize = 9_999_999;

/// One synthetic scrobble.
#[derive(Debug, Clone)]
pub struct Entry {
    pub ts: u64,
    /// Unique within a dataset. Sent as the track `mbid`.
    pub marker: usize,
    pub name: String,
}

/// Which pages carry the now-playing row.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Pages {
    First,
    Last,
    All,
}

#[derive(Debug, Clone, Copy)]
pub struct NowPlayingSpec {
    /// Also send a `date`, which is later than every scrobble.
    pub with_date: bool,
    pub on: Pages,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BadDate {
    Missing,
    NonNumeric,
}

/// The ways a reply can go wrong. Every switch is off by default.
#[derive(Debug, Clone, Default)]
pub struct Faults {
    /// Echo another user on this page.
    pub wrong_user_on_page: Option<u32>,
    /// From this page on, a scrobble has arrived: the history has a new
    /// newest row and `total` is one higher.
    pub arrive_from_page: Option<u32>,
    /// From this page on, the newest row of the window was deleted and
    /// `total` is one lower.
    pub delete_from_page: Option<u32>,
    /// Leave out the last row of this page, keeping `total`.
    pub drop_row_on_page: Option<u32>,
    /// Serve the rows of the previous page again.
    pub repeat_page: Option<u32>,
    /// Make the last row of this page older than `from`. Needs a `from`.
    pub stray_row_on_page: Option<u32>,
    /// Serve every page oldest first.
    pub increasing_order: bool,
    /// Leave `@attr` out of this page.
    pub missing_attr_on_page: Option<u32>,
    /// Leave `@attr.total` out of this page.
    pub missing_total_on_page: Option<u32>,
    /// Break the date of one row: `(page, row on that page, how)`.
    pub bad_date: Option<(u32, usize, BadDate)>,
    /// Treat `to` as inclusive.
    pub inclusive_to: bool,
    /// Answer with an error envelope and HTTP 200: `(page or every page, code)`.
    pub api_error: Option<(Option<u32>, u32)>,
    /// Cut the body of this page in half.
    pub truncate_on_page: Option<u32>,
    /// Serve this page with no rows, keeping the totals.
    pub empty_page: Option<u32>,
}

#[derive(Debug, Clone)]
pub struct Dataset {
    /// The user's canonical spelling.
    pub user: String,
    /// Newest first.
    pub entries: Vec<Entry>,
    pub now_playing: Option<NowPlayingSpec>,
    /// The most rows to a page the service gives, whatever `limit` says. It
    /// reports the size it applied in `perPage`.
    pub cap: u32,
    /// What `totalPages` says for an empty result.
    pub empty_total_pages: u64,
    pub faults: Faults,
}

impl Dataset {
    /// Scrobbles at the given timestamps, sorted newest first. Markers count
    /// up in that order, so a window's expected markers ascend.
    pub fn from_timestamps(mut timestamps: Vec<u64>) -> Self {
        timestamps.sort_unstable_by(|a, b| b.cmp(a));
        let entries = timestamps
            .into_iter()
            .enumerate()
            .map(|(marker, ts)| Entry {
                ts,
                marker,
                name: format!("Track {marker}"),
            })
            .collect();
        Self {
            user: "rj_Synthetic".to_owned(),
            entries,
            now_playing: None,
            cap: 200,
            empty_total_pages: 0,
            faults: Faults::default(),
        }
    }

    /// `n` scrobbles ten seconds apart. The newest is `BASE + 10 * n`, the
    /// oldest `BASE + 10`.
    pub fn distinct(n: usize) -> Self {
        Self::from_timestamps((1..=n as u64).map(|i| BASE + 10 * i).collect())
    }

    /// Gives every scrobble the same track name, so rows differ only by
    /// their marker.
    #[must_use]
    pub fn identical_rows(mut self) -> Self {
        for entry in &mut self.entries {
            entry.name = "Same Song".to_owned();
        }
        self
    }

    #[must_use]
    pub fn with_now_playing(mut self, with_date: bool, on: Pages) -> Self {
        self.now_playing = Some(NowPlayingSpec { with_date, on });
        self
    }

    #[must_use]
    pub fn with_cap(mut self, cap: u32) -> Self {
        self.cap = cap;
        self
    }

    #[must_use]
    pub fn with_faults(mut self, faults: Faults) -> Self {
        self.faults = faults;
        self
    }

    /// The markers a correct scan of `window` must yield, in order.
    pub fn expected(&self, window: Window) -> Vec<usize> {
        self.entries
            .iter()
            .filter(|e| window.contains(e.ts))
            .map(|e| e.marker)
            .collect()
    }

    fn newest(&self) -> u64 {
        self.entries.first().map_or(BASE, |e| e.ts)
    }
}

/// The query string of a request as pairs.
pub fn query(request: &HttpRequest) -> Vec<(String, String)> {
    let query = request.url().split_once('?').map_or("", |(_, q)| q);
    form_urlencoded::parse(query.as_bytes())
        .map(|(k, v)| (k.into_owned(), v.into_owned()))
        .collect()
}

/// One query parameter of a request.
pub fn param(request: &HttpRequest, name: &str) -> Option<String> {
    query(request)
        .into_iter()
        .find(|(k, _)| k == name)
        .map(|(_, v)| v)
}

/// The marker of a decoded scrobble, from its track `mbid`.
pub fn marker_of(mbid: Option<&str>) -> usize {
    let mbid = mbid.expect("a synthetic row carries its marker");
    mbid.rsplit('-')
        .next()
        .and_then(|tail| tail.parse().ok())
        .expect("the marker is the last group of the mbid")
}

fn marker_mbid(marker: usize) -> String {
    format!("00000000-0000-4000-8000-{marker:012}")
}

fn error_body(code: u32, message: &str) -> Vec<u8> {
    json!({"error": code, "message": message})
        .to_string()
        .into_bytes()
}

/// Answers one request: `(HTTP status, body)`.
pub fn respond(dataset: &Dataset, request: &HttpRequest) -> (u16, Vec<u8>) {
    let params = query(request);
    let get = |name: &str| {
        params
            .iter()
            .find(|(k, _)| k == name)
            .map(|(_, v)| v.as_str())
    };

    if !get("method").is_some_and(|m| m.eq_ignore_ascii_case("user.getRecentTracks")) {
        return (400, error_body(3, "Invalid Method"));
    }
    if get("api_key").is_none() {
        return (403, error_body(10, "Invalid API key"));
    }
    let Some(user) = get("user") else {
        return (400, error_body(6, "Invalid parameters"));
    };
    if !user.eq_ignore_ascii_case(&dataset.user) {
        return (404, error_body(6, "User not found"));
    }

    let number = |name: &str, default: u64| -> Option<u64> {
        get(name).map_or(Some(default), |v| v.parse().ok())
    };
    let bound = |name: &str| -> Result<Option<u64>, ()> {
        get(name).map_or(Ok(None), |v| v.parse().map(Some).map_err(|_| ()))
    };
    let (Some(page), Some(limit), Ok(from), Ok(to)) = (
        number("page", 1),
        number("limit", 50),
        bound("from"),
        bound("to"),
    ) else {
        return (400, error_body(6, "Invalid parameters"));
    };
    if page == 0 || limit == 0 {
        return (400, error_body(6, "Invalid parameters"));
    }
    let page = page as u32;
    let extended = get("extended") == Some("1");
    let faults = &dataset.faults;

    if let Some((on, code)) = faults.api_error
        && on.is_none_or(|p| p == page)
    {
        return (200, error_body(code, "Synthetic error"));
    }

    // The history as it stands when this request arrives.
    let mut entries: Vec<Entry> = dataset.entries.clone();
    if faults.arrive_from_page.is_some_and(|p| page >= p) {
        entries.insert(
            0,
            Entry {
                ts: dataset.newest() + 1_000,
                marker: NOW_PLAYING_MARKER - 1,
                name: "Arrived".to_owned(),
            },
        );
    }
    let in_window = |e: &Entry| {
        from.is_none_or(|f| e.ts >= f)
            && to.is_none_or(|t| {
                if faults.inclusive_to {
                    e.ts <= t
                } else {
                    e.ts < t
                }
            })
    };
    let mut windowed: Vec<Entry> = entries.into_iter().filter(in_window).collect();
    if faults.delete_from_page.is_some_and(|p| page >= p) && !windowed.is_empty() {
        windowed.remove(0);
    }
    if faults.increasing_order {
        windowed.reverse();
    }

    let size = limit.min(u64::from(dataset.cap));
    let total = windowed.len() as u64;
    let total_pages = if total == 0 {
        dataset.empty_total_pages
    } else {
        total.div_ceil(size)
    };

    let slice_for = |page: u32| -> Vec<Entry> {
        let start = (u64::from(page) - 1).saturating_mul(size) as usize;
        windowed
            .iter()
            .skip(start)
            .take(size as usize)
            .cloned()
            .collect()
    };
    let mut slice = match faults.repeat_page {
        Some(p) if p == page && page > 1 => slice_for(page - 1),
        _ => slice_for(page),
    };
    if faults.empty_page == Some(page) {
        slice.clear();
    }
    if faults.drop_row_on_page == Some(page) {
        slice.pop();
    }
    if faults.stray_row_on_page == Some(page) {
        let from = from.expect("the stray row fault needs a `from`");
        if let Some(last) = slice.last_mut() {
            last.ts = from - 1;
        }
    }

    let mut rows: Vec<Value> = slice.iter().map(|e| row_json(e, extended)).collect();
    if let Some((bad_page, index, how)) = faults.bad_date
        && bad_page == page
        && let Some(row) = rows.get_mut(index).and_then(Value::as_object_mut)
    {
        match how {
            BadDate::Missing => {
                row.remove("date");
            }
            BadDate::NonNumeric => {
                row.insert("date".to_owned(), json!({"uts": "soon", "#text": "soon"}));
            }
        }
    }
    if let Some(spec) = dataset.now_playing {
        let last_page = total_pages.max(1) as u32;
        let here = match spec.on {
            Pages::First => page == 1,
            Pages::Last => page == last_page,
            Pages::All => true,
        };
        if here {
            let ts = spec.with_date.then(|| dataset.newest() + 10_000);
            rows.insert(0, now_playing_json(extended, ts));
        }
    }

    let track = match rows.len() {
        1 => rows.remove(0),
        _ => Value::Array(rows),
    };
    let echoed = if faults.wrong_user_on_page == Some(page) {
        "someone-else".to_owned()
    } else {
        dataset.user.clone()
    };
    let mut attr = json!({
        "user": echoed,
        "page": page.to_string(),
        "perPage": size.to_string(),
        "totalPages": total_pages.to_string(),
        "total": total.to_string(),
    });
    if faults.missing_total_on_page == Some(page) {
        attr.as_object_mut().unwrap().remove("total");
    }
    let mut recent = json!({"track": track, "@attr": attr});
    if faults.missing_attr_on_page == Some(page) {
        recent.as_object_mut().unwrap().remove("@attr");
    }
    let mut body = json!({"recenttracks": recent}).to_string().into_bytes();
    if faults.truncate_on_page == Some(page) {
        body.truncate(body.len() / 2);
    }
    (200, body)
}

fn images() -> Value {
    json!([
        {"size": "small", "#text": "https://example.invalid/34s/x.png"},
        {"size": "extralarge", "#text": "https://example.invalid/300x300/x.png"}
    ])
}

fn row_json(entry: &Entry, extended: bool) -> Value {
    let artist = if extended {
        json!({
            "url": "https://example.invalid/music/Artist",
            "name": "Synthetic Artist",
            "mbid": "",
            "image": images(),
        })
    } else {
        json!({"mbid": "", "#text": "Synthetic Artist"})
    };
    let mut row = json!({
        "artist": artist,
        "streamable": "0",
        "image": images(),
        "mbid": marker_mbid(entry.marker),
        "album": {"mbid": "", "#text": "Synthetic Album"},
        "name": entry.name,
        "url": "https://example.invalid/music/Artist/_/Track",
        "date": {"uts": entry.ts.to_string(), "#text": "synthetic"},
    });
    if extended {
        row["loved"] = json!(if entry.marker.is_multiple_of(2) {
            "0"
        } else {
            "1"
        });
    }
    row
}

fn now_playing_json(extended: bool, date: Option<u64>) -> Value {
    let artist = if extended {
        json!({
            "url": "https://example.invalid/music/Artist",
            "name": "Synthetic Artist",
            "mbid": "",
            "image": images(),
        })
    } else {
        json!({"mbid": "", "#text": "Synthetic Artist"})
    };
    let mut row = json!({
        "@attr": {"nowplaying": "true"},
        "artist": artist,
        "streamable": "0",
        "image": images(),
        "mbid": marker_mbid(NOW_PLAYING_MARKER),
        "album": {"mbid": "", "#text": ""},
        "name": "Now Playing Track",
        "url": "https://example.invalid/music/Artist/_/Playing",
    });
    if extended {
        row["loved"] = json!("0");
    }
    if let Some(ts) = date {
        row["date"] = json!({"uts": ts.to_string(), "#text": "synthetic"});
    }
    row
}
