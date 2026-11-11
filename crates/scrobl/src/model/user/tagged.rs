//! What a user has given one tag: the three replies of
//! `user.getPersonalTags`, one for each `taggingtype`.

use std::fmt;

use serde_json::{Map, Value};

use crate::error::Error;
use crate::model::shared::{self, Album, Artist, PageAttr, Track, missing_or};
use crate::protocol::{Raw, methods};

const ROOT: &str = "taggings";

/// The paging attributes, the tag, and the rows at `taggings.<kind>.<key>`.
/// A reply holds one kind only, the one its request asked for.
fn tagged(
    mut taggings: Map<String, Value>,
    kind: &str,
    key: &str,
) -> Result<(PageAttr, String, Vec<Value>), Error> {
    let tag = match taggings.get("@attr") {
        Some(Value::Object(attr)) => {
            shared::required_str(attr, "taggings.@attr", "tag")?.to_owned()
        }
        _ => return Err(missing_or("taggings.@attr", "an object")),
    };
    let attr = shared::page_attr(&mut taggings, ROOT)?;
    let Some(Value::Object(mut inner)) = taggings.remove(kind) else {
        return Err(missing_or(&format!("taggings.{kind}"), "an object"));
    };
    let rows = shared::promised_rows(&mut inner, &attr, &format!("taggings.{kind}.{key}"), key)?;
    Ok((attr, tag, rows))
}

/// Defines the reply for one `taggingtype`. The three differ in the names
/// of the two levels the rows sit under, and in the row type.
macro_rules! tagged_page {
    (
        $(#[$doc:meta])*
        $name:ident, $kind:literal, $key:literal, $rows:ident: $row:ty = $parse:path
    ) => {
        $(#[$doc])*
        #[derive(Clone)]
        pub struct $name {
            attr: PageAttr,
            tag: String,
            $rows: Vec<$row>,
        }

        impl $name {
            #[doc = concat!(
                "Decodes a `user.getPersonalTags` response to a request with `taggingtype=",
                $key, "`."
            )]
            ///
            /// # Errors
            ///
            /// [`ErrorKind::Decode`](crate::ErrorKind) when the response is
            /// for another method or another `taggingtype`, when `taggings`,
            #[doc = concat!("`taggings.", $kind, "`, `@attr` or any of its six members is missing or")]
            /// malformed, when a row is not an object or lacks what its type
            /// requires, when the rows are absent or empty although `@attr`
            /// promises rows on the page, or when an object repeats a member
            #[doc = concat!("name. The message names the field, such as `taggings.", $kind, ".", $key, "[3].name`,")]
            /// and never repeats response text.
            pub fn decode(raw: &Raw) -> Result<Self, Error> {
                shared::decode(raw, &methods::USER_GET_PERSONAL_TAGS, ROOT, |taggings| {
                    let (attr, tag, rows) = tagged(taggings, $kind, $key)?;
                    let $rows = shared::each(rows, concat!("taggings.", $kind, ".", $key), $parse)?;
                    Ok(Self { attr, tag, $rows })
                })
            }

            /// The paging attributes.
            pub fn attr(&self) -> &PageAttr {
                &self.attr
            }

            /// The tag, as the service spells it back.
            pub fn tag(&self) -> &str {
                &self.tag
            }

            /// The rows, in the order the service sent them.
            pub fn $rows(&self) -> &[$row] {
                &self.$rows
            }
        }

        impl fmt::Debug for $name {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.debug_struct(stringify!($name))
                    .field("attr", &self.attr)
                    .field(stringify!($rows), &self.$rows.len())
                    .finish_non_exhaustive()
            }
        }
    };
}

tagged_page! {
    /// One page of the artists a user has given a tag: `user.getPersonalTags`
    /// with `taggingtype=artist`.
    ///
    /// ```
    /// use scrobl::model::TaggedArtistsPage;
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
    /// let page = TaggedArtistsPage::decode(&raw)?;
    /// assert_eq!(page.tag(), "rock");
    /// assert_eq!(page.attr().total(), 1);
    /// assert_eq!(page.artists()[0].artist().name(), "A");
    /// # Ok::<(), scrobl::Error>(())
    /// ```
    TaggedArtistsPage, "artists", "artist", artists: TaggedArtist = tagged_artist
}

tagged_page! {
    /// One page of the albums a user has given a tag: `user.getPersonalTags`
    /// with `taggingtype=album`.
    TaggedAlbumsPage, "albums", "album", albums: TaggedAlbum = tagged_album
}

tagged_page! {
    /// One page of the tracks a user has given a tag: `user.getPersonalTags`
    /// with `taggingtype=track`.
    TaggedTracksPage, "tracks", "track", tracks: TaggedTrack = tagged_track
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
