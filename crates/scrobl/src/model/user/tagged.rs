//! What a user has given one tag: the three replies of
//! `user.getPersonalTags`, one for each `taggingtype`.

use std::fmt;

use serde_json::{Map, Value};

use crate::error::Error;
use crate::model::Page;
use crate::model::shared::{self, Album, Artist, Track, missing_or};
use crate::protocol::{Raw, methods};

const ROOT: &str = "taggings";

/// One page of what a user has given a tag: the rows at
/// `taggings.<kind>.<key>`. A reply holds one kind only, the one its request
/// asked for. The page's subject is the user.
fn tagged<T>(
    raw: &Raw,
    kind: &str,
    key: &str,
    parse: impl Fn(Map<String, Value>, &str) -> Result<T, Error>,
) -> Result<Page<T>, Error> {
    shared::decode(
        raw,
        &methods::USER_GET_PERSONAL_TAGS,
        ROOT,
        |mut taggings| {
            // The tag is required too, though a page has no place for it.
            shared::owner(&taggings, ROOT, "tag")?;
            let paging = shared::paging(&mut taggings, ROOT, Some("user"))?;
            let Some(Value::Object(mut inner)) = taggings.remove(kind) else {
                return Err(missing_or(&format!("taggings.{kind}"), "an object"));
            };
            let path = format!("taggings.{kind}.{key}");
            let rows = shared::promised_rows(&mut inner, &paging, &path, key)?;
            Ok(Page::new(shared::each(rows, &path, parse)?, paging))
        },
    )
}

/// Decodes a `user.getPersonalTags` response to a request with
/// `taggingtype=artist`: one page of the artists a user has given a tag.
/// The page's subject is the user.
///
/// ```
/// use scrobl::model::decode;
/// use scrobl::protocol::{self, HttpResponse, Request, methods};
///
/// let body = r##"{"taggings": {
///     "artists": {"artist": [
///         {"name": "A", "mbid": "", "url": "https://www.last.fm/music/A"}
///     ]},
///     "@attr": {"user": "rj", "tag": "rock", "page": "1", "perPage": "50",
///               "totalPages": "1", "total": "1"}
/// }}"##;
/// let request = Request::new(&methods::USER_GET_PERSONAL_TAGS)
///     .param("user", "rj")
///     .param("tag", "rock")
///     .param("taggingtype", "artist");
/// let raw = protocol::decode(&request, HttpResponse::new(200, body))?;
///
/// let page = decode::user_get_personal_tags_artists(&raw)?;
/// assert_eq!(page.subject(), Some("rj"));
/// assert_eq!(page.total(), 1);
/// assert_eq!(page[0].artist().name(), "A");
/// # Ok::<(), scrobl::Error>(())
/// ```
///
/// # Errors
///
/// [`ErrorKind::Decode`](crate::ErrorKind) under [the rules every list
/// shares](crate::model::decode#errors), and when the reply is for another
/// `taggingtype`. `@attr.user` and `@attr.tag` are required, and a row needs
/// a `name`.
pub fn user_get_personal_tags_artists(raw: &Raw) -> Result<Page<TaggedArtist>, Error> {
    tagged(raw, "artists", "artist", tagged_artist)
}

/// Decodes a `user.getPersonalTags` response to a request with
/// `taggingtype=album`: one page of the albums a user has given a tag.
///
/// # Errors
///
/// [`ErrorKind::Decode`](crate::ErrorKind) under [the rules every list
/// shares](crate::model::decode#errors), and when the reply is for another
/// `taggingtype`. A row needs a `name` and an `artist`.
pub fn user_get_personal_tags_albums(raw: &Raw) -> Result<Page<TaggedAlbum>, Error> {
    tagged(raw, "albums", "album", tagged_album)
}

/// Decodes a `user.getPersonalTags` response to a request with
/// `taggingtype=track`: one page of the tracks a user has given a tag.
///
/// # Errors
///
/// [`ErrorKind::Decode`](crate::ErrorKind) under [the rules every list
/// shares](crate::model::decode#errors), and when the reply is for another
/// `taggingtype`. A row needs a `name` and an `artist`.
pub fn user_get_personal_tags_tracks(raw: &Raw) -> Result<Page<TaggedTrack>, Error> {
    tagged(raw, "tracks", "track", tagged_track)
}

/// An artist a user has given a tag. [`json`](Self::json) is the row
/// exactly as decoded.
#[derive(Clone)]
pub struct TaggedArtist {
    artist: Artist,
    json: Value,
}

impl TaggedArtist {
    /// The artist.
    pub fn artist(&self) -> &Artist {
        &self.artist
    }

    /// The row exactly as decoded.
    pub fn json(&self) -> &Value {
        &self.json
    }
}

impl fmt::Debug for TaggedArtist {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("TaggedArtist").finish_non_exhaustive()
    }
}

/// An album a user has given a tag. [`json`](Self::json) is the row exactly
/// as decoded.
#[derive(Clone)]
pub struct TaggedAlbum {
    album: Album,
    artist: Artist,
    json: Value,
}

impl TaggedAlbum {
    /// The album.
    pub fn album(&self) -> &Album {
        &self.album
    }

    /// The album's artist.
    pub fn artist(&self) -> &Artist {
        &self.artist
    }

    /// The row exactly as decoded.
    pub fn json(&self) -> &Value {
        &self.json
    }
}

impl fmt::Debug for TaggedAlbum {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("TaggedAlbum").finish_non_exhaustive()
    }
}

/// A track a user has given a tag. [`json`](Self::json) is the row exactly
/// as decoded.
///
/// The row has a `duration`, which is not read: in every row recorded the
/// service sent the word `FIXME` there.
#[derive(Clone)]
pub struct TaggedTrack {
    track: Track,
    artist: Artist,
    json: Value,
}

impl TaggedTrack {
    /// The track.
    pub fn track(&self) -> &Track {
        &self.track
    }

    /// The track's artist.
    pub fn artist(&self) -> &Artist {
        &self.artist
    }

    /// The row exactly as decoded.
    pub fn json(&self) -> &Value {
        &self.json
    }
}

impl fmt::Debug for TaggedTrack {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("TaggedTrack").finish_non_exhaustive()
    }
}

fn tagged_artist(row: Map<String, Value>, at: &str) -> Result<TaggedArtist, Error> {
    Ok(TaggedArtist {
        artist: shared::parse_artist_row(&row, at)?,
        json: Value::Object(row),
    })
}

fn tagged_album(row: Map<String, Value>, at: &str) -> Result<TaggedAlbum, Error> {
    Ok(TaggedAlbum {
        album: shared::parse_album_row(&row, at)?,
        artist: shared::parse_artist(&row, at)?,
        json: Value::Object(row),
    })
}

fn tagged_track(row: Map<String, Value>, at: &str) -> Result<TaggedTrack, Error> {
    Ok(TaggedTrack {
        track: shared::parse_track(&row, at)?,
        artist: shared::parse_artist(&row, at)?,
        json: Value::Object(row),
    })
}

#[cfg(test)]
mod tests;
