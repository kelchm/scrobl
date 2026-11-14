//! Loving and tagging, on a [`Writer`].

use super::Writer;
use crate::error::Error;
use crate::model::decode;
use crate::protocol::{Raw, Request, methods};
use crate::response::Response;

/// The most tags one call adds.
pub const MAX_TAGS: usize = 10;

impl Writer {
    /// Loves a track for the user.
    #[doc(alias = "track.love")]
    pub fn love(&self, artist: impl Into<String>, track: impl Into<String>) -> ChangeQuery {
        self.change(
            Request::new(&methods::TRACK_LOVE)
                .param("artist", artist.into())
                .param("track", track.into()),
            decode::track_love,
        )
    }

    /// Unloves a track for the user.
    #[doc(alias = "track.unlove")]
    pub fn unlove(&self, artist: impl Into<String>, track: impl Into<String>) -> ChangeQuery {
        self.change(
            Request::new(&methods::TRACK_UNLOVE)
                .param("artist", artist.into())
                .param("track", track.into()),
            decode::track_unlove,
        )
    }

    /// Adds up to [`MAX_TAGS`] of the user's tags to an artist.
    #[doc(alias = "artist.addTags")]
    pub fn tag_artist<T: Into<String>>(
        &self,
        artist: impl Into<String>,
        tags: impl IntoIterator<Item = T>,
    ) -> ChangeQuery {
        let request = Request::new(&methods::ARTIST_ADD_TAGS).param("artist", artist.into());
        self.tags(request, tags, decode::artist_add_tags)
    }

    /// Adds up to [`MAX_TAGS`] of the user's tags to an album.
    #[doc(alias = "album.addTags")]
    pub fn tag_album<T: Into<String>>(
        &self,
        artist: impl Into<String>,
        album: impl Into<String>,
        tags: impl IntoIterator<Item = T>,
    ) -> ChangeQuery {
        let request = Request::new(&methods::ALBUM_ADD_TAGS)
            .param("artist", artist.into())
            .param("album", album.into());
        self.tags(request, tags, decode::album_add_tags)
    }

    /// Adds up to [`MAX_TAGS`] of the user's tags to a track.
    #[doc(alias = "track.addTags")]
    pub fn tag_track<T: Into<String>>(
        &self,
        artist: impl Into<String>,
        track: impl Into<String>,
        tags: impl IntoIterator<Item = T>,
    ) -> ChangeQuery {
        let request = Request::new(&methods::TRACK_ADD_TAGS)
            .param("artist", artist.into())
            .param("track", track.into());
        self.tags(request, tags, decode::track_add_tags)
    }

    /// Removes one of the user's tags from an artist.
    #[doc(alias = "artist.removeTag")]
    pub fn untag_artist(&self, artist: impl Into<String>, tag: impl Into<String>) -> ChangeQuery {
        self.change(
            Request::new(&methods::ARTIST_REMOVE_TAG)
                .param("artist", artist.into())
                .param("tag", tag.into()),
            decode::artist_remove_tag,
        )
    }

    /// Removes one of the user's tags from an album.
    #[doc(alias = "album.removeTag")]
    pub fn untag_album(
        &self,
        artist: impl Into<String>,
        album: impl Into<String>,
        tag: impl Into<String>,
    ) -> ChangeQuery {
        self.change(
            Request::new(&methods::ALBUM_REMOVE_TAG)
                .param("artist", artist.into())
                .param("album", album.into())
                .param("tag", tag.into()),
            decode::album_remove_tag,
        )
    }

    /// Removes one of the user's tags from a track.
    #[doc(alias = "track.removeTag")]
    pub fn untag_track(
        &self,
        artist: impl Into<String>,
        track: impl Into<String>,
        tag: impl Into<String>,
    ) -> ChangeQuery {
        self.change(
            Request::new(&methods::TRACK_REMOVE_TAG)
                .param("artist", artist.into())
                .param("track", track.into())
                .param("tag", tag.into()),
            decode::track_remove_tag,
        )
    }

    fn change(&self, request: Request, decode: Decode) -> ChangeQuery {
        ChangeQuery {
            writer: self.clone(),
            request,
            refused: None,
            decode,
        }
    }

    /// The service takes tags as one comma-separated list.
    fn tags<T: Into<String>>(
        &self,
        request: Request,
        tags: impl IntoIterator<Item = T>,
        decode: Decode,
    ) -> ChangeQuery {
        let tags: Vec<String> = tags.into_iter().map(Into::into).collect();
        let refused = if tags.is_empty() {
            Some("no tags")
        } else if tags.len() > MAX_TAGS {
            Some("more than 10 tags")
        } else if tags.iter().any(|tag| tag.contains(',')) {
            Some("a tag holds a comma, which would split it in two")
        } else {
            None
        };
        ChangeQuery {
            refused,
            ..self.change(request.param("tags", tags.join(",")), decode)
        }
    }
}

type Decode = fn(&Raw) -> Result<(), Error>;

/// A love, an unlove, or tags added or removed. Made by [`Writer::love`] and
/// the methods beside it.
#[derive(Debug, Clone)]
#[must_use = "a query does nothing until it is sent"]
pub struct ChangeQuery {
    writer: Writer,
    request: Request,
    refused: Option<&'static str>,
    decode: Decode,
}

impl ChangeQuery {
    /// Makes the call, once and never again.
    ///
    /// # Errors
    ///
    /// [`ErrorKind::InvalidRequest`](crate::ErrorKind) for no tags, more
    /// than [`MAX_TAGS`], or a tag with a comma. Any error of
    /// [`Writer::call`]; check [`delivery`](Error::delivery).
    pub async fn send(self) -> Result<Response<()>, Error> {
        if let Some(problem) = self.refused {
            return Err(Error::invalid_request(self.request.spec(), problem));
        }
        let raw = self.writer.call(&self.request).await?;
        (self.decode)(&raw)?;
        Ok(Response::new((), raw))
    }
}
