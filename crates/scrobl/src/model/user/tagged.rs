//! What a user has given one tag: the three replies of
//! `user.getPersonalTags`, one for each `taggingtype`.

use serde_json::{Map, Value};

use crate::error::Error;
use crate::model::Page;
use crate::model::entity::{self, Album, Artist, Counts, Length, Track};
use crate::model::shared::{self, missing_or};
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
/// assert_eq!(page[0].name(), "A");
/// # Ok::<(), scrobl::Error>(())
/// ```
///
/// # Errors
///
/// [`ErrorKind::Decode`](crate::ErrorKind) under [the rules every list
/// shares](crate::model::decode#errors), and when the reply is for another
/// `taggingtype`. `@attr.user` and `@attr.tag` are required, and a row needs
/// a `name`.
pub fn user_get_personal_tags_artists(raw: &Raw) -> Result<Page<Artist>, Error> {
    tagged(raw, "artists", "artist", |row, at| {
        entity::artist(row, at, Counts::Neither)
    })
}

/// Decodes a `user.getPersonalTags` response to a request with
/// `taggingtype=album`: one page of the albums a user has given a tag.
///
/// # Errors
///
/// [`ErrorKind::Decode`](crate::ErrorKind) under [the rules every list
/// shares](crate::model::decode#errors), and when the reply is for another
/// `taggingtype`. A row needs a `name` and an `artist`.
pub fn user_get_personal_tags_albums(raw: &Raw) -> Result<Page<Album>, Error> {
    tagged(raw, "albums", "album", |row, at| {
        entity::album(row, at, Counts::Neither)
    })
}

/// Decodes a `user.getPersonalTags` response to a request with
/// `taggingtype=track`: one page of the tracks a user has given a tag.
///
/// A track's `duration` is not read here: on 2026-11-10 the service sent
/// the word `FIXME` for every one. The row's `json` still holds it.
///
/// # Errors
///
/// [`ErrorKind::Decode`](crate::ErrorKind) under [the rules every list
/// shares](crate::model::decode#errors), and when the reply is for another
/// `taggingtype`. A row needs a `name` and an `artist`.
pub fn user_get_personal_tags_tracks(raw: &Raw) -> Result<Page<Track>, Error> {
    tagged(raw, "tracks", "track", |row, at| {
        entity::track(row, at, Counts::Neither, Length::Leave)
    })
}

#[cfg(test)]
mod tests;
