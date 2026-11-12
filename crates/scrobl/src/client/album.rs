//! Typed reads about one album: what the service says about it, and the
//! tags it is given.

use super::Client;
use super::query::{Ask, query};
use crate::model::{AlbumInfo, List, Tag, TopTag, decode};
use crate::protocol::{MethodSpec, methods};

/// One album, to ask about. Made by [`Client::album`] from the names of
/// its artist and of the album, or by [`Client::album_mbid`] from a
/// MusicBrainz id.
///
/// An album the service does not know is an error of the call,
/// [`INVALID_PARAMETERS`](crate::ApiErrorCode::INVALID_PARAMETERS), and not
/// an empty answer.
#[derive(Debug, Clone)]
pub struct AlbumHandle {
    client: Client,
    album: Named,
    autocorrect: bool,
}

/// How an album is named to the service.
#[derive(Debug, Clone)]
enum Named {
    Name { artist: String, title: String },
    Mbid(String),
}

impl AlbumHandle {
    pub(super) fn named(client: Client, artist: String, title: String) -> Self {
        Self {
            client,
            album: Named::Name { artist, title },
            autocorrect: false,
        }
    }

    pub(super) fn with_mbid(client: Client, mbid: String) -> Self {
        Self {
            client,
            album: Named::Mbid(mbid),
            autocorrect: false,
        }
    }

    /// Lets the service correct a misspelt artist's name, on every call
    /// made through this handle, as
    /// [`ArtistHandle::autocorrect`](super::ArtistHandle::autocorrect)
    /// does.
    #[must_use]
    pub fn autocorrect(mut self) -> Self {
        self.autocorrect = true;
        self
    }

    fn ask(&self, spec: &'static MethodSpec) -> Ask {
        let ask = Ask::new(&self.client, spec);
        let ask = match &self.album {
            Named::Name { artist, title } => ask.about("artist", artist).about("album", title),
            Named::Mbid(mbid) => ask.about("mbid", mbid),
        };
        if self.autocorrect {
            ask.about("autocorrect", "1")
        } else {
            ask
        }
    }

    /// What the service says about the album: counts, its tracks, a few
    /// tags, and what its users wrote.
    #[doc(alias = "album.getInfo")]
    pub fn info(&self) -> AlbumInfoQuery {
        AlbumInfoQuery::new(self.ask(&methods::ALBUM_GET_INFO))
    }

    /// The tags the album is given most.
    #[doc(alias = "album.getTopTags")]
    pub fn top_tags(&self) -> AlbumTopTagsQuery {
        AlbumTopTagsQuery::new(self.ask(&methods::ALBUM_GET_TOP_TAGS))
    }

    /// The tags `user` has given the album.
    #[doc(alias = "album.getTags")]
    pub fn tags_of(&self, user: impl Into<String>) -> AlbumTagsOfQuery {
        AlbumTagsOfQuery::new(self.ask(&methods::ALBUM_GET_TAGS).about("user", user))
    }
}

query! {
    /// What the service says about one album. Made by
    /// [`AlbumHandle::info`].
    ///
    /// ```no_run
    /// # async fn demo(client: scrobl::Client) -> Result<(), scrobl::Error> {
    /// let info = client.album("Radiohead", "OK Computer").info().send().await?;
    /// println!("{} by {} listeners", info.album().name(), info.listeners());
    /// for (number, track) in info.tracks().iter().enumerate() {
    ///     println!("{}. {}", number + 1, track.name());
    /// }
    /// # Ok(())
    /// # }
    /// ```
    AlbumInfoQuery => AlbumInfo = decode::album_get_info;
    lang, for_user
}

query! {
    /// The tags an album is given most. Made by [`AlbumHandle::top_tags`].
    ///
    /// On 2026-11-11 the service answered this call for an album named by
    /// id with HTTP 400 and the body `{}`, where `info` by the same id
    /// worked. That arrives as [`ErrorKind::Http`](crate::ErrorKind).
    AlbumTopTagsQuery => List<TopTag> = decode::album_get_top_tags;
}

query! {
    /// The tags one user has given an album. Made by
    /// [`AlbumHandle::tags_of`]. A user who has given none is an empty
    /// list.
    AlbumTagsOfQuery => List<Tag> = decode::album_get_tags;
}
