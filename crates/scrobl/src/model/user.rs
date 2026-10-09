//! The `user` package: [`RecentTracksPage`], the reply to
//! `user.getRecentTracks`.

use std::fmt;

use serde_json::{Map, Value};

use crate::de;
use crate::error::Error;
use crate::protocol::{Raw, methods};

const ROOT: &str = "recenttracks";

/// One page of `user.getRecentTracks`.
///
/// A page holds the historical rows, the optional now-playing row and the
/// paging attributes. Decode it with [`decode`](Self::decode).
///
/// # Shapes
///
/// The same method answers in several shapes, and all of them decode:
///
/// - `artist` is `{"#text": .., "mbid": ..}` normally and `{"name": ..,
///   "mbid": .., "url": .., "image": [..]}` with `extended=1`.
/// - `track` is an array, a single object when the page has exactly one row,
///   and for an empty page absent, `[]` or `""`. Those three are accepted
///   only when the page's own `@attr` allows no rows: `total` is 0 or `page`
///   is beyond `totalPages`. Anything else, including `null`, a non-empty
///   string and a missing or misspelled `track` on a page that promises
///   rows, is an error.
/// - A row whose `@attr.nowplaying` is `"true"` or `"1"` is the now-playing
///   row, whether or not it also carries a `date`. `"false"` and `"0"` are
///   also read, as an ordinary historical row. Every row that is not
///   now-playing must have a valid `date.uts`.
/// - `artist` may carry `name`, `#text` or both. Each one present must be a
///   string, and two that are present must be equal.
/// - An object that repeats a member name, at any depth, is an error: a map
///   would keep one of the two and drop the other without a word.
///
/// ```
/// use scrobl::model::RecentTracksPage;
/// use scrobl::protocol::{self, HttpResponse, Request, methods};
///
/// let body = r##"{"recenttracks": {
///     "track": [
///         {"@attr": {"nowplaying": "true"},
///          "artist": {"#text": "A", "mbid": ""}, "name": "Playing", "mbid": "",
///          "album": {"#text": "", "mbid": ""}},
///         {"date": {"uts": "1700000000", "#text": "14 Nov 2023, 22:13"},
///          "artist": {"#text": "A", "mbid": ""}, "name": "Played", "mbid": "",
///          "album": {"#text": "Album", "mbid": ""}}
///     ],
///     "@attr": {"user": "rj", "page": "1", "perPage": "50",
///               "totalPages": "1", "total": "1"}
/// }}"##;
/// let request = Request::new(&methods::USER_GET_RECENT_TRACKS).param("user", "rj");
/// let raw = protocol::decode(&request, HttpResponse::new(200, body))?;
///
/// let page = RecentTracksPage::decode(&raw)?;
/// assert_eq!(page.attr().total(), 1);
/// assert_eq!(page.scrobbles().len(), 1);
/// assert_eq!(page.scrobbles()[0].timestamp(), 1_700_000_000);
/// assert_eq!(page.scrobbles()[0].album().title(), Some("Album"));
/// assert_eq!(page.now_playing().map(|n| n.track().name()), Some("Playing"));
/// # Ok::<(), scrobl::Error>(())
/// ```
#[derive(Clone)]
pub struct RecentTracksPage {
    attr: PageAttr,
    scrobbles: Vec<Scrobble>,
    now_playing: Option<NowPlaying>,
}

impl RecentTracksPage {
    /// Decodes a `user.getRecentTracks` response.
    ///
    /// # Errors
    ///
    /// [`ErrorKind::Decode`](crate::ErrorKind) when the response is for
    /// another method, when `recenttracks`, `@attr` or any of its five
    /// members is missing or malformed, when a row is not an object, when a
    /// row has neither the now-playing flag nor a valid `date.uts`, when more
    /// than one row is flagged now-playing, when `track` is absent or empty
    /// although `@attr` promises rows on the page, when an object repeats a
    /// member name, when `artist` has two names that differ, or when a field
    /// of a row has the wrong type. The message names the field, such as
    /// `recenttracks.@attr.total` or `recenttracks.track[3].date.uts`, and
    /// never repeats response text.
    pub fn decode(raw: &Raw) -> Result<Self, Error> {
        if raw.method() != methods::USER_GET_RECENT_TRACKS.name {
            return Err(Error::decode("the response is for a different method")
                .with_method(raw.spec())
                .with_response(raw.status(), raw.body()));
        }
        let root = raw.json_strict()?;
        parse(root).map_err(|e| {
            e.with_method(raw.spec())
                .with_response(raw.status(), raw.body())
        })
    }

    /// The paging attributes.
    pub fn attr(&self) -> &PageAttr {
        &self.attr
    }

    /// The historical rows, in the order the service sent them. The
    /// now-playing row is never among them.
    pub fn scrobbles(&self) -> &[Scrobble] {
        &self.scrobbles
    }

    /// The row flagged now-playing, if the page has one.
    pub fn now_playing(&self) -> Option<&NowPlaying> {
        self.now_playing.as_ref()
    }
}

impl fmt::Debug for RecentTracksPage {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("RecentTracksPage")
            .field("attr", &self.attr)
            .field("scrobbles", &self.scrobbles.len())
            .field("now_playing", &self.now_playing.is_some())
            .finish()
    }
}

/// The `@attr` of a page: whose history, which page of how many.
///
/// The service sends these as strings. All five are required.
#[derive(Clone)]
pub struct PageAttr {
    user: String,
    page: u64,
    per_page: u64,
    total_pages: u64,
    total: u64,
}

impl PageAttr {
    /// The user the service says the page is for, in its own spelling.
    pub fn user(&self) -> &str {
        &self.user
    }

    /// The page number, from 1.
    pub fn page(&self) -> u64 {
        self.page
    }

    /// The page size the service applied.
    pub fn per_page(&self) -> u64 {
        self.per_page
    }

    /// The number of pages the service says there are.
    pub fn total_pages(&self) -> u64 {
        self.total_pages
    }

    /// The number of scrobbles the service says match the request, within
    /// the `from` and `to` bounds if any were sent.
    pub fn total(&self) -> u64 {
        self.total
    }
}

impl fmt::Debug for PageAttr {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("PageAttr")
            .field("page", &self.page)
            .field("per_page", &self.per_page)
            .field("total_pages", &self.total_pages)
            .field("total", &self.total)
            .finish_non_exhaustive()
    }
}

/// A track, as named in a row.
///
/// `mbid` and `url` are `None` when the field is missing or an empty string;
/// [`Scrobble::json`] tells the two apart.
#[derive(Clone)]
pub struct Track {
    name: String,
    mbid: Option<String>,
    url: Option<String>,
}

impl Track {
    /// The track name. May be empty.
    pub fn name(&self) -> &str {
        &self.name
    }

    /// The MusicBrainz id, if the service sent a non-empty one.
    pub fn mbid(&self) -> Option<&str> {
        self.mbid.as_deref()
    }

    /// The Last.fm page of the track, if the service sent a non-empty one.
    pub fn url(&self) -> Option<&str> {
        self.url.as_deref()
    }
}

impl fmt::Debug for Track {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Track").finish_non_exhaustive()
    }
}

/// An artist, as named in a row.
///
/// Both shapes decode: the name is `#text` normally and `name` with
/// `extended=1`. A row that carries both must carry the same string twice.
/// `url` is only sent with `extended=1`. `mbid` and `url` are
/// `None` when the field is missing or an empty string.
#[derive(Clone)]
pub struct Artist {
    name: String,
    mbid: Option<String>,
    url: Option<String>,
}

impl Artist {
    /// The artist name. May be empty.
    pub fn name(&self) -> &str {
        &self.name
    }

    /// The MusicBrainz id, if the service sent a non-empty one.
    pub fn mbid(&self) -> Option<&str> {
        self.mbid.as_deref()
    }

    /// The Last.fm page of the artist, if the service sent a non-empty one.
    pub fn url(&self) -> Option<&str> {
        self.url.as_deref()
    }
}

impl fmt::Debug for Artist {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Artist").finish_non_exhaustive()
    }
}

/// An album, as named in a row. A row without an album decodes to an album
/// with no title and no mbid.
///
/// Both fields are `None` when missing or an empty string.
#[derive(Clone)]
pub struct Album {
    title: Option<String>,
    mbid: Option<String>,
}

impl Album {
    /// The album title, if the service sent a non-empty one.
    pub fn title(&self) -> Option<&str> {
        self.title.as_deref()
    }

    /// The MusicBrainz id, if the service sent a non-empty one.
    pub fn mbid(&self) -> Option<&str> {
        self.mbid.as_deref()
    }
}

impl fmt::Debug for Album {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Album").finish_non_exhaustive()
    }
}

/// One historical row: a scrobble with its timestamp.
///
/// Rows are not unique. The same track can be scrobbled twice in a second,
/// and a timestamp is not an identity. [`json`](Self::json) is the row
/// exactly as decoded, so unknown fields and the difference between a
/// missing and an empty field stay reachable.
#[derive(Clone)]
pub struct Scrobble {
    timestamp: u64,
    track: Track,
    artist: Artist,
    album: Album,
    loved: Option<bool>,
    json: Value,
}

impl Scrobble {
    /// Seconds since the Unix epoch, from `date.uts`.
    pub fn timestamp(&self) -> u64 {
        self.timestamp
    }

    /// The track.
    pub fn track(&self) -> &Track {
        &self.track
    }

    /// The artist.
    pub fn artist(&self) -> &Artist {
        &self.artist
    }

    /// The album.
    pub fn album(&self) -> &Album {
        &self.album
    }

    /// Whether the user loved the track, as the row returned it. The field is
    /// present only when the page was requested with `extended=1`; otherwise
    /// this is `None`, which is not the same as `Some(false)`.
    pub fn loved(&self) -> Option<bool> {
        self.loved
    }

    /// The row exactly as decoded.
    pub fn json(&self) -> &Value {
        &self.json
    }
}

impl fmt::Debug for Scrobble {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Scrobble")
            .field("timestamp", &self.timestamp)
            .field("loved", &self.loved)
            .finish_non_exhaustive()
    }
}

/// The row flagged now-playing: what the user is listening to, not yet a
/// scrobble.
///
/// It may carry a `date`, which is not exposed here and is never treated as
/// a scrobble time; it is still in [`json`](Self::json). A row whose
/// `@attr.nowplaying` flag is false is not this: it is an ordinary
/// [`Scrobble`].
#[derive(Clone)]
pub struct NowPlaying {
    track: Track,
    artist: Artist,
    album: Album,
    loved: Option<bool>,
    json: Value,
}

impl NowPlaying {
    /// The track.
    pub fn track(&self) -> &Track {
        &self.track
    }

    /// The artist.
    pub fn artist(&self) -> &Artist {
        &self.artist
    }

    /// The album.
    pub fn album(&self) -> &Album {
        &self.album
    }

    /// Whether the user loved the track, as the row returned it. The field is
    /// present only when the page was requested with `extended=1`; otherwise
    /// this is `None`, which is not the same as `Some(false)`.
    pub fn loved(&self) -> Option<bool> {
        self.loved
    }

    /// The row exactly as decoded.
    pub fn json(&self) -> &Value {
        &self.json
    }
}

impl fmt::Debug for NowPlaying {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("NowPlaying")
            .field("loved", &self.loved)
            .finish_non_exhaustive()
    }
}

// Decoding. Every error names a field path; none repeats response text.

fn missing_or(path: &str, expected: &str) -> Error {
    Error::decode_field(path, &format!("missing or not {expected}"))
}

fn parse(root: Value) -> Result<RecentTracksPage, Error> {
    let Value::Object(mut root) = root else {
        return Err(Error::decode("the body is not a JSON object"));
    };
    let Some(Value::Object(mut recent)) = root.remove(ROOT) else {
        return Err(missing_or(ROOT, "an object"));
    };
    let attr = match recent.remove("@attr") {
        Some(Value::Object(attr)) => parse_attr(&attr)?,
        _ => return Err(missing_or("recenttracks.@attr", "an object")),
    };

    let rows = match recent.remove("track") {
        None => Vec::new(),
        Some(Value::String(s)) if s.is_empty() => Vec::new(),
        Some(value) => de::one_or_many(value).ok_or_else(|| {
            Error::decode_field(
                "recenttracks.track",
                "expected an array, an object or an empty string",
            )
        })?,
    };
    if rows.is_empty() {
        check_rows_may_be_absent(&attr)?;
    }

    let mut scrobbles = Vec::with_capacity(rows.len());
    let mut now_playing = None;
    for (index, row) in rows.into_iter().enumerate() {
        match parse_row(index, row)? {
            Row::Scrobble(scrobble) => scrobbles.push(scrobble),
            Row::NowPlaying(row) => {
                if now_playing.replace(row).is_some() {
                    return Err(Error::decode_field(
                        "recenttracks.track",
                        "more than one row is flagged now-playing",
                    ));
                }
            }
        }
    }
    Ok(RecentTracksPage {
        attr,
        scrobbles,
        now_playing,
    })
}

/// An absent or empty `track` is an empty page only if the page's own `@attr`
/// allows it: no scrobbles match (`total` is 0) or the page lies beyond the
/// last one. Otherwise the rows are missing, not absent.
fn check_rows_may_be_absent(attr: &PageAttr) -> Result<(), Error> {
    if attr.total == 0 || attr.page > attr.total_pages {
        return Ok(());
    }
    Err(Error::decode_field(
        "recenttracks.track",
        "absent or empty, but `@attr` says the page holds rows",
    ))
}

fn parse_attr(attr: &Map<String, Value>) -> Result<PageAttr, Error> {
    let uint = |key: &str| {
        let path = format!("recenttracks.@attr.{key}");
        attr.get(key)
            .and_then(de::uint)
            .ok_or_else(|| missing_or(&path, "an unsigned integer"))
    };
    let user = attr
        .get("user")
        .and_then(Value::as_str)
        .ok_or_else(|| missing_or("recenttracks.@attr.user", "a string"))?;
    Ok(PageAttr {
        user: user.to_owned(),
        page: uint("page")?,
        per_page: uint("perPage")?,
        total_pages: uint("totalPages")?,
        total: uint("total")?,
    })
}

enum Row {
    Scrobble(Scrobble),
    NowPlaying(NowPlaying),
}

fn parse_row(index: usize, json: Value) -> Result<Row, Error> {
    let at = format!("recenttracks.track[{index}]");
    let Value::Object(map) = &json else {
        return Err(Error::decode_field(&at, "the row is not an object"));
    };

    let flagged = match map.get("@attr") {
        None => false,
        Some(Value::Object(attr)) => match attr.get("nowplaying") {
            None => false,
            Some(value) => de::flag_or_word(value).ok_or_else(|| {
                Error::decode_field(&format!("{at}.@attr.nowplaying"), "not a recognised flag")
            })?,
        },
        Some(_) => return Err(Error::decode_field(&format!("{at}.@attr"), "not an object")),
    };

    let track = Track {
        name: required_str(map, &at, "name")?.to_owned(),
        mbid: optional_str(map, &at, "mbid")?,
        url: optional_str(map, &at, "url")?,
    };
    let artist = parse_artist(map, &at)?;
    let album = parse_album(map, &at)?;
    let loved = match map.get("loved") {
        None => None,
        Some(value) => Some(
            de::flag(value)
                .ok_or_else(|| Error::decode_field(&format!("{at}.loved"), "not a 0/1 flag"))?,
        ),
    };

    if flagged {
        return Ok(Row::NowPlaying(NowPlaying {
            track,
            artist,
            album,
            loved,
            json,
        }));
    }

    let path = format!("{at}.date.uts");
    let timestamp = map
        .get("date")
        .and_then(|date| date.get("uts"))
        .and_then(de::uint)
        .ok_or_else(|| missing_or(&path, "an unsigned integer"))?;
    Ok(Row::Scrobble(Scrobble {
        timestamp,
        track,
        artist,
        album,
        loved,
        json,
    }))
}

fn parse_artist(row: &Map<String, Value>, at: &str) -> Result<Artist, Error> {
    let path = format!("{at}.artist");
    let Some(Value::Object(artist)) = row.get("artist") else {
        return Err(missing_or(&path, "an object"));
    };
    // `name` with extended=1, `#text` without. Each one present must be a
    // string, and two that are present must agree.
    let name_in = |key: &str| match artist.get(key) {
        None => Ok(None),
        Some(Value::String(name)) => Ok(Some(name)),
        Some(_) => Err(Error::decode_field(
            &format!("{path}.{key}"),
            "not a string",
        )),
    };
    let name = match (name_in("name")?, name_in("#text")?) {
        (Some(name), Some(text)) if name != text => {
            return Err(Error::decode_field(
                &path,
                "`name` and `#text` are both present and disagree",
            ));
        }
        (Some(name), _) | (None, Some(name)) => name.clone(),
        (None, None) => {
            return Err(Error::decode_field(
                &format!("{path}.name"),
                "neither `name` nor `#text` is present",
            ));
        }
    };
    Ok(Artist {
        name,
        mbid: optional_str(artist, &path, "mbid")?,
        url: optional_str(artist, &path, "url")?,
    })
}

fn parse_album(row: &Map<String, Value>, at: &str) -> Result<Album, Error> {
    let path = format!("{at}.album");
    match row.get("album") {
        None => Ok(Album {
            title: None,
            mbid: None,
        }),
        Some(Value::Object(album)) => Ok(Album {
            title: optional_str(album, &path, "#text")?,
            mbid: optional_str(album, &path, "mbid")?,
        }),
        Some(_) => Err(Error::decode_field(&path, "not an object")),
    }
}

fn required_str<'a>(map: &'a Map<String, Value>, at: &str, key: &str) -> Result<&'a str, Error> {
    map.get(key)
        .and_then(Value::as_str)
        .ok_or_else(|| missing_or(&format!("{at}.{key}"), "a string"))
}

/// A string field that may be missing or empty, both reading as `None`. A
/// value of any other type is an error.
fn optional_str(map: &Map<String, Value>, at: &str, key: &str) -> Result<Option<String>, Error> {
    match map.get(key) {
        None => Ok(None),
        Some(Value::String(s)) if s.is_empty() => Ok(None),
        Some(Value::String(s)) => Ok(Some(s.clone())),
        Some(_) => Err(Error::decode_field(&format!("{at}.{key}"), "not a string")),
    }
}

#[cfg(test)]
mod tests;
