//! The writes that answer with nothing but success: loving, unloving, and
//! adding and removing tags.

use serde_json::Value;

use crate::error::Error;
use crate::protocol::{MethodSpec, Raw, methods};

macro_rules! acknowledged {
    ($($name:ident: $spec:ident, $method:literal;)*) => {$(
        #[doc = concat!("Decodes a `", $method, "` response, which carries nothing.")]
        ///
        /// # Errors
        ///
        /// [`ErrorKind::Decode`](crate::ErrorKind) for a body that is not a
        /// JSON object.
        pub fn $name(raw: &Raw) -> Result<(), Error> {
            acknowledgement(raw, &methods::$spec)
        }
    )*};
}

acknowledged! {
    track_love: TRACK_LOVE, "track.love";
    track_unlove: TRACK_UNLOVE, "track.unlove";
    artist_add_tags: ARTIST_ADD_TAGS, "artist.addTags";
    album_add_tags: ALBUM_ADD_TAGS, "album.addTags";
    track_add_tags: TRACK_ADD_TAGS, "track.addTags";
    artist_remove_tag: ARTIST_REMOVE_TAG, "artist.removeTag";
    album_remove_tag: ALBUM_REMOVE_TAG, "album.removeTag";
    track_remove_tag: TRACK_REMOVE_TAG, "track.removeTag";
}

fn acknowledgement(raw: &Raw, spec: &'static MethodSpec) -> Result<(), Error> {
    if raw.method() != spec.name() {
        return Err(Error::decode("the response is for a different method")
            .with_method(raw.spec())
            .with_response(raw.status(), raw.body()));
    }
    match raw.json_strict()? {
        Value::Object(_) => Ok(()),
        _ => Err(Error::decode("the body is not a JSON object")
            .with_method(raw.spec())
            .with_response(raw.status(), raw.body())),
    }
}

#[cfg(test)]
mod tests;
