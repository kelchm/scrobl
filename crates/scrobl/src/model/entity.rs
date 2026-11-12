//! The five things the service names everywhere: an artist, an album, a
//! track, a tag and a user. Wherever a reply names one, this is the type.
//!
//! Each carries what identifies it, what is true of it wherever it appears,
//! and the service's own counts for it where a reply has them. A count that
//! is true only inside one list, such as how often one user played an
//! artist, is not here: it is on the row of that list.

use std::fmt;
use std::ops::Deref;
use std::time::Duration;

use serde_json::{Map, Value};

use super::shared::{missing_or, optional_str, optional_uint, required_str, required_uint};
use crate::de;
use crate::error::Error;

/// One address of an image, in one of the sizes the service offers.
#[derive(Clone)]
pub struct Image {
    size: Option<String>,
    url: String,
}

impl Image {
    /// The size as the service names it, such as `large`, if it sent a
    /// non-empty one.
    pub fn size(&self) -> Option<&str> {
        self.size.as_deref()
    }

    /// The address of the image. Never empty.
    pub fn url(&self) -> &str {
        &self.url
    }
}

impl fmt::Debug for Image {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Image").finish_non_exhaustive()
    }
}

/// The image of an artist, an album, a track or a user: one address for
/// each size the service sent, in its order.
///
/// It dereferences to a slice, so `images.len()`, `images.iter()` and
/// `images.is_empty()` work. An entry whose address is missing or empty is
/// left out; the `json` of whatever the image belongs to still holds it.
///
/// In every response recorded the order runs from the smallest to the
/// largest: `small`, `medium` and `large`, then `extralarge` in most lists,
/// then `mega` where the row is itself an artist. What an image shows is the
/// service's choice: every artist recorded on 2026-11-10 had the same one, a
/// placeholder, and albums had their covers.
///
/// ```
/// use scrobl::model::decode;
/// use scrobl::protocol::{self, HttpResponse, Request, methods};
///
/// let body = r##"{"artists": {
///     "artist": [{"name": "Cher", "playcount": "9", "listeners": "3", "image": [
///         {"size": "small", "#text": "https://example.org/34.png"},
///         {"size": "large", "#text": "https://example.org/174.png"}
///     ]}],
///     "@attr": {"page": "1", "perPage": "1", "totalPages": "1", "total": "1"}
/// }}"##;
/// let request = Request::new(&methods::CHART_GET_TOP_ARTISTS);
/// let raw = protocol::decode(&request, HttpResponse::new(200, body))?;
///
/// let page = decode::chart_get_top_artists(&raw)?;
/// let images = page[0].images();
/// assert_eq!(images.largest().map(|image| image.url()), Some("https://example.org/174.png"));
/// assert_eq!(images.get("small").map(|image| image.url()), Some("https://example.org/34.png"));
/// assert!(images.get("mega").is_none());
/// # Ok::<(), scrobl::Error>(())
/// ```
#[derive(Clone)]
pub struct Images(Vec<Image>);

impl Images {
    /// The last address the service sent, which in every response recorded
    /// is the largest. `None` when there is no image.
    pub fn largest(&self) -> Option<&Image> {
        self.0.last()
    }

    /// The first address the service sent, which in every response recorded
    /// is the smallest. `None` when there is no image.
    pub fn smallest(&self) -> Option<&Image> {
        self.0.first()
    }

    /// The address of the size the service calls `size`, such as `large`.
    pub fn get(&self, size: &str) -> Option<&Image> {
        self.0.iter().find(|image| image.size() == Some(size))
    }
}

impl Deref for Images {
    type Target = [Image];

    fn deref(&self) -> &[Image] {
        &self.0
    }
}

impl<'a> IntoIterator for &'a Images {
    type Item = &'a Image;
    type IntoIter = std::slice::Iter<'a, Image>;

    fn into_iter(self) -> Self::IntoIter {
        self.0.iter()
    }
}

impl fmt::Debug for Images {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_tuple("Images").field(&self.0.len()).finish()
    }
}

/// An artist. [`json`](Self::json) is the artist exactly as decoded.
///
/// `mbid` and `url` are `None` when the member is missing or an empty
/// string. Where an artist is only named inside a row, as the artist of a
/// scrobble is, the service sends a name and an id and little else. In two
/// replies, `album.getInfo` and `album.search`, it names an album's artist
/// with a string alone: that artist has a name and nothing more.
#[derive(Clone)]
pub struct Artist {
    name: String,
    mbid: Option<String>,
    url: Option<String>,
    images: Images,
    listeners: Option<u64>,
    playcount: Option<u64>,
    json: Value,
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

    /// The artist's image in the sizes the service sent.
    pub fn images(&self) -> &Images {
        &self.images
    }

    /// How many users have played the artist, by the service's count for
    /// everyone. `None` where the reply does not say.
    pub fn listeners(&self) -> Option<u64> {
        self.listeners
    }

    /// How many times the artist has been played, by the service's count
    /// for everyone. `None` where the reply does not say.
    pub fn playcount(&self) -> Option<u64> {
        self.playcount
    }

    /// The artist exactly as decoded: an object, or the string where a
    /// reply names the artist with nothing else.
    pub fn json(&self) -> &Value {
        &self.json
    }
}

impl AsRef<Artist> for Artist {
    fn as_ref(&self) -> &Artist {
        self
    }
}

impl fmt::Debug for Artist {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Artist")
            .field("listeners", &self.listeners)
            .field("playcount", &self.playcount)
            .finish_non_exhaustive()
    }
}

/// An album. [`json`](Self::json) is the object exactly as decoded.
///
/// `artist` is `None` where an album is only named inside a track's row:
/// the artist is the track's.
#[derive(Clone)]
pub struct Album {
    name: String,
    mbid: Option<String>,
    url: Option<String>,
    images: Images,
    artist: Option<Artist>,
    listeners: Option<u64>,
    playcount: Option<u64>,
    json: Value,
}

impl Album {
    /// The album's name. May be empty where the album is a row of its own;
    /// a track whose album has no name has no album.
    pub fn name(&self) -> &str {
        &self.name
    }

    /// The MusicBrainz id, if the service sent a non-empty one.
    pub fn mbid(&self) -> Option<&str> {
        self.mbid.as_deref()
    }

    /// The Last.fm page of the album, if the service sent a non-empty one.
    pub fn url(&self) -> Option<&str> {
        self.url.as_deref()
    }

    /// The album's image in the sizes the service sent.
    pub fn images(&self) -> &Images {
        &self.images
    }

    /// The album's artist, where the album is a row of its own.
    pub fn artist(&self) -> Option<&Artist> {
        self.artist.as_ref()
    }

    /// How many users have played the album, by the service's count for
    /// everyone. `None` where the reply does not say.
    pub fn listeners(&self) -> Option<u64> {
        self.listeners
    }

    /// How many times the album has been played, by the service's count for
    /// everyone. `None` where the reply does not say.
    pub fn playcount(&self) -> Option<u64> {
        self.playcount
    }

    /// The object exactly as decoded.
    pub fn json(&self) -> &Value {
        &self.json
    }
}

impl AsRef<Album> for Album {
    fn as_ref(&self) -> &Album {
        self
    }
}

impl fmt::Debug for Album {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Album")
            .field("listeners", &self.listeners)
            .field("playcount", &self.playcount)
            .finish_non_exhaustive()
    }
}

/// A track. [`json`](Self::json) is the object exactly as decoded.
#[derive(Clone)]
pub struct Track {
    name: String,
    mbid: Option<String>,
    url: Option<String>,
    images: Images,
    artist: Artist,
    album: Option<Album>,
    duration: Option<Duration>,
    listeners: Option<u64>,
    playcount: Option<u64>,
    json: Value,
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

    /// The image the service sent with the track, in the sizes it sent.
    pub fn images(&self) -> &Images {
        &self.images
    }

    /// The track's artist.
    pub fn artist(&self) -> &Artist {
        &self.artist
    }

    /// The album the track is on, where the reply names one. In a history
    /// it is `None` for a scrobble without an album.
    pub fn album(&self) -> Option<&Album> {
        self.album.as_ref()
    }

    /// The length of the track. `None` where the reply does not say, and
    /// where it gives 0, which is what the service sends for a length it
    /// does not know; [`json`](Self::json) tells the two apart.
    pub fn duration(&self) -> Option<Duration> {
        self.duration
    }

    /// How many users have played the track, by the service's count for
    /// everyone. `None` where the reply does not say.
    pub fn listeners(&self) -> Option<u64> {
        self.listeners
    }

    /// How many times the track has been played, by the service's count for
    /// everyone. `None` where the reply does not say.
    pub fn playcount(&self) -> Option<u64> {
        self.playcount
    }

    /// The object exactly as decoded.
    pub fn json(&self) -> &Value {
        &self.json
    }
}

impl AsRef<Track> for Track {
    fn as_ref(&self) -> &Track {
        self
    }
}

impl fmt::Debug for Track {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Track")
            .field("duration", &self.duration)
            .field("listeners", &self.listeners)
            .field("playcount", &self.playcount)
            .finish_non_exhaustive()
    }
}

/// A tag. [`json`](Self::json) is the object exactly as decoded.
#[derive(Clone)]
pub struct Tag {
    name: String,
    url: Option<String>,
    taggings: Option<u64>,
    reach: Option<u64>,
    json: Value,
}

impl Tag {
    /// The tag.
    pub fn name(&self) -> &str {
        &self.name
    }

    /// The Last.fm page of the tag, if the service sent a non-empty one.
    pub fn url(&self) -> Option<&str> {
        self.url.as_deref()
    }

    /// How many times the tag has been used, by everyone. `None` where the
    /// reply does not say. The service calls this `taggings`, `count` or
    /// `total`, by method.
    pub fn taggings(&self) -> Option<u64> {
        self.taggings
    }

    /// How many users have used the tag. `None` where the reply does not
    /// say. The service calls this `reach`.
    pub fn reach(&self) -> Option<u64> {
        self.reach
    }

    /// The object exactly as decoded.
    pub fn json(&self) -> &Value {
        &self.json
    }
}

impl AsRef<Tag> for Tag {
    fn as_ref(&self) -> &Tag {
        self
    }
}

impl fmt::Debug for Tag {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Tag")
            .field("taggings", &self.taggings)
            .field("reach", &self.reach)
            .finish_non_exhaustive()
    }
}

/// Defines a row that wraps one of the five things and adds one number that
/// is true only in that row's list.
macro_rules! counted_row {
    (
        $(#[$doc:meta])*
        $name:ident {
            $(#[$count_doc:meta])*
            $count:ident,
            $thing:ident / $into:ident: $entity:ident
        }
    ) => {
        $(#[$doc])*
        #[derive(Clone)]
        pub struct $name {
            $count: u64,
            $thing: $entity,
        }

        impl $name {
            $(#[$count_doc])*
            pub fn $count(&self) -> u64 {
                self.$count
            }

            #[doc = concat!("The ", stringify!($thing), ".")]
            pub fn $thing(&self) -> &$entity {
                &self.$thing
            }

            #[doc = concat!("The ", stringify!($thing), ", to keep.")]
            pub fn $into(self) -> $entity {
                self.$thing
            }

            /// The row exactly as decoded.
            pub fn json(&self) -> &serde_json::Value {
                self.$thing.json()
            }
        }

        impl AsRef<$entity> for $name {
            fn as_ref(&self) -> &$entity {
                &self.$thing
            }
        }

        impl ::std::fmt::Debug for $name {
            fn fmt(&self, f: &mut ::std::fmt::Formatter<'_>) -> ::std::fmt::Result {
                f.debug_struct(stringify!($name))
                    .field(stringify!($count), &self.$count)
                    .finish_non_exhaustive()
            }
        }
    };
}
pub(super) use counted_row;

// Decoding. Every error names a field path; none repeats response text.

/// Which of the service's counts for everyone a row carries. A row that
/// carries one must carry it: a count that is promised and missing is an
/// error, not `None`.
///
/// A row of a list about one user or one country has members of the same
/// names that mean something else. They are that row's to read, and this is
/// `Neither` for it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Counts {
    /// The row has no count of its own for everyone.
    Neither,
    /// `listeners` and `playcount`, as the charts of the service send them.
    Both,
    /// `playcount` alone, as an artist's albums have it.
    Playcount,
    /// `listeners` alone, as an artist found by a search has it.
    Listeners,
}

impl Counts {
    fn read(self, row: &Map<String, Value>, at: &str) -> Result<(Option<u64>, Option<u64>), Error> {
        match self {
            Self::Neither => Ok((None, None)),
            Self::Both => Ok((
                Some(required_uint(row, at, "listeners")?),
                Some(required_uint(row, at, "playcount")?),
            )),
            Self::Playcount => Ok((None, Some(required_uint(row, at, "playcount")?))),
            Self::Listeners => Ok((Some(required_uint(row, at, "listeners")?), None)),
        }
    }
}

impl Artist {
    /// The same artist with its counts for everyone, for a reply that keeps
    /// them apart from the artist's other members.
    pub(super) fn counted(mut self, listeners: u64, playcount: u64) -> Self {
        self.listeners = Some(listeners);
        self.playcount = Some(playcount);
        self
    }
}

/// The artist a row is: its own `name`, `mbid`, `url` and `image`.
pub(super) fn artist(row: Map<String, Value>, at: &str, counts: Counts) -> Result<Artist, Error> {
    let (listeners, playcount) = counts.read(&row, at)?;
    Ok(Artist {
        name: required_str(&row, at, "name")?.to_owned(),
        mbid: optional_str(&row, at, "mbid")?,
        url: optional_str(&row, at, "url")?,
        images: images(&row, at)?,
        listeners,
        playcount,
        json: Value::Object(row),
    })
}

/// The artist a row names, under `artist`.
///
/// The name is `name` where the row gives the artist's details and `#text`
/// where it gives a name and an id. Each one present must be a string, and
/// two that are present must agree.
pub(super) fn named_artist(row: &Map<String, Value>, at: &str) -> Result<Artist, Error> {
    let path = format!("{at}.artist");
    let Some(Value::Object(artist)) = row.get("artist") else {
        return Err(missing_or(&path, "an object"));
    };
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
        images: images(artist, &path)?,
        listeners: None,
        playcount: None,
        json: Value::Object(artist.clone()),
    })
}

/// The artist a row names with nothing but a string under `artist`, as
/// `album.getInfo` does. The artist's JSON is that string.
fn artist_by_name(row: &Map<String, Value>, at: &str) -> Result<Artist, Error> {
    match row.get("artist") {
        Some(Value::String(name)) if !name.is_empty() => Ok(Artist {
            name: name.clone(),
            mbid: None,
            url: None,
            images: Images(Vec::new()),
            listeners: None,
            playcount: None,
            json: Value::String(name.clone()),
        }),
        _ => Err(missing_or(
            &format!("{at}.artist"),
            "a string that is not empty",
        )),
    }
}

/// The album a row is: its own `name`, `mbid`, `url` and `image`, and the
/// artist it names.
pub(super) fn album(row: Map<String, Value>, at: &str, counts: Counts) -> Result<Album, Error> {
    let artist = named_artist(&row, at)?;
    album_of(row, at, counts, artist)
}

/// The album a row is, where the row names its artist with a string and
/// not an object.
pub(super) fn album_by_name(
    row: Map<String, Value>,
    at: &str,
    counts: Counts,
) -> Result<Album, Error> {
    let artist = artist_by_name(&row, at)?;
    album_of(row, at, counts, artist)
}

fn album_of(
    row: Map<String, Value>,
    at: &str,
    counts: Counts,
    artist: Artist,
) -> Result<Album, Error> {
    let (listeners, playcount) = counts.read(&row, at)?;
    Ok(Album {
        name: required_str(&row, at, "name")?.to_owned(),
        mbid: optional_str(&row, at, "mbid")?,
        url: optional_str(&row, at, "url")?,
        images: images(&row, at)?,
        artist: Some(artist),
        listeners,
        playcount,
        json: Value::Object(row),
    })
}

/// The album a track's row names, under `album`. `None` for a row without
/// the member and for one whose album has no name.
///
/// The name is `#text` where the row gives a name and an id, as a scrobble
/// does, and `title` in `track.getInfo`, which also gives the album a
/// `url`, an `image` and its artist's name as a string.
fn named_album(row: &Map<String, Value>, at: &str) -> Result<Option<Album>, Error> {
    let path = format!("{at}.album");
    match row.get("album") {
        None => Ok(None),
        Some(Value::Object(album)) => {
            let name = match optional_str(album, &path, "#text")? {
                Some(name) => Some(name),
                None => optional_str(album, &path, "title")?,
            };
            let artist = match album.get("artist") {
                None => None,
                Some(_) => Some(artist_by_name(album, &path)?),
            };
            let mbid = optional_str(album, &path, "mbid")?;
            let url = optional_str(album, &path, "url")?;
            let images = images(album, &path)?;
            Ok(name.map(|name| Album {
                name,
                mbid,
                url,
                images,
                artist,
                listeners: None,
                playcount: None,
                json: Value::Object(album.clone()),
            }))
        }
        Some(_) => Err(Error::decode_field(&path, "not an object")),
    }
}

/// How a track's row says how long the track is, under `duration`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Length {
    /// In seconds, as the lists do. A row without the member has no
    /// length.
    Read,
    /// In milliseconds, as `track.getInfo` does.
    Millis,
    /// Not at all: one method sends a word there.
    Leave,
}

/// The track a row is: its own `name`, `mbid`, `url` and `image`, the artist
/// it names, and the album where it names one.
pub(super) fn track(
    row: Map<String, Value>,
    at: &str,
    counts: Counts,
    length: Length,
) -> Result<Track, Error> {
    track_of(row, at, counts, length, named_artist)
}

/// The track a row is, where the row names its artist with a string and
/// not an object, as a track found by a search does.
pub(super) fn track_by_name(
    row: Map<String, Value>,
    at: &str,
    counts: Counts,
    length: Length,
) -> Result<Track, Error> {
    track_of(row, at, counts, length, artist_by_name)
}

fn track_of(
    row: Map<String, Value>,
    at: &str,
    counts: Counts,
    length: Length,
    artist: fn(&Map<String, Value>, &str) -> Result<Artist, Error>,
) -> Result<Track, Error> {
    let name = required_str(&row, at, "name")?.to_owned();
    let mbid = optional_str(&row, at, "mbid")?;
    let url = optional_str(&row, at, "url")?;
    let images = images(&row, at)?;
    let artist = artist(&row, at)?;
    let album = named_album(&row, at)?;
    let (listeners, playcount) = counts.read(&row, at)?;
    // 0 is what the service sends for a length it does not know.
    let known = |length: Option<u64>| length.filter(|&length| length != 0);
    let duration = match length {
        Length::Read => known(optional_uint(&row, at, "duration")?).map(Duration::from_secs),
        Length::Millis => known(optional_uint(&row, at, "duration")?).map(Duration::from_millis),
        Length::Leave => None,
    };
    Ok(Track {
        name,
        mbid,
        url,
        images,
        artist,
        album,
        duration,
        listeners,
        playcount,
        json: Value::Object(row),
    })
}

/// Which members hold a tag's counts for everyone. The service names the
/// number of uses three ways.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum TagCounts {
    /// A name and perhaps a `url`: no counts.
    Neither,
    /// `taggings` and `reach`, as `chart.getTopTags` sends them.
    Taggings,
    /// `count` and `reach`, as `tag.getTopTags` sends them.
    Count,
    /// `total` and `reach`, as `tag.getInfo` sends them.
    Total,
}

/// The tag a row is: its `name`, its `url`, and its counts where `counts`
/// says the row has them.
pub(super) fn tag(row: Map<String, Value>, at: &str, counts: TagCounts) -> Result<Tag, Error> {
    let uses = match counts {
        TagCounts::Neither => None,
        TagCounts::Taggings => Some("taggings"),
        TagCounts::Count => Some("count"),
        TagCounts::Total => Some("total"),
    };
    let (taggings, reach) = match uses {
        Some(key) => (
            Some(required_uint(&row, at, key)?),
            Some(required_uint(&row, at, "reach")?),
        ),
        None => (None, None),
    };
    Ok(Tag {
        name: required_str(&row, at, "name")?.to_owned(),
        url: optional_str(&row, at, "url")?,
        taggings,
        reach,
        json: Value::Object(row),
    })
}

/// The image of whatever `map` is, under `image`: an array of sizes, a
/// single one, or none when the member is absent or `""`, as the rows of a
/// page are read.
///
/// An entry whose address is missing or empty is left out, as a missing or
/// empty `mbid` reads as none: the service sends an empty address where it
/// has no image. An address or a size of another type is an error.
pub(super) fn images(map: &Map<String, Value>, at: &str) -> Result<Images, Error> {
    let path = format!("{at}.image");
    let entries = match map.get("image") {
        None => return Ok(Images(Vec::new())),
        Some(Value::String(s)) if s.is_empty() => return Ok(Images(Vec::new())),
        Some(Value::Array(entries)) => entries.as_slice(),
        Some(entry @ Value::Object(_)) => std::slice::from_ref(entry),
        Some(_) => {
            return Err(Error::decode_field(
                &path,
                "expected an array, an object or an empty string",
            ));
        }
    };
    let mut images = Vec::with_capacity(entries.len());
    for (index, entry) in entries.iter().enumerate() {
        let at = format!("{path}[{index}]");
        let Value::Object(entry) = entry else {
            return Err(Error::decode_field(&at, "not an object"));
        };
        let size = optional_str(entry, &at, "size")?;
        if let Some(url) = optional_str(entry, &at, "#text")? {
            images.push(Image { size, url });
        }
    }
    Ok(Images(images))
}

/// A 0/1 flag that may be missing, which reads as `None`.
pub(super) fn optional_flag(
    map: &Map<String, Value>,
    at: &str,
    key: &str,
) -> Result<Option<bool>, Error> {
    match map.get(key) {
        None => Ok(None),
        Some(value) => de::flag(value)
            .map(Some)
            .ok_or_else(|| Error::decode_field(&format!("{at}.{key}"), "not a 0/1 flag")),
    }
}
