//! Typed reads of what the whole service plays and tags most.

use super::Client;
use super::query::{Ask, query};
use crate::error::Error;
use crate::model::{Artist, List, Page, Tag, Track, decode};
use crate::protocol::{Request, methods};
use crate::response::Response;

/// The charts of the whole service. Made by [`Client::charts`].
#[derive(Debug, Clone)]
pub struct ChartsHandle {
    client: Client,
}

impl ChartsHandle {
    pub(super) fn new(client: Client) -> Self {
        Self { client }
    }

    /// The artists played most.
    #[doc(alias = "chart.getTopArtists")]
    pub fn top_artists(&self) -> ChartsTopArtistsQuery {
        ChartsTopArtistsQuery::new(Ask::new(&self.client, &methods::CHART_GET_TOP_ARTISTS))
    }

    /// The tracks played most.
    #[doc(alias = "chart.getTopTracks")]
    pub fn top_tracks(&self) -> ChartsTopTracksQuery {
        ChartsTopTracksQuery::new(Ask::new(&self.client, &methods::CHART_GET_TOP_TRACKS))
    }

    /// The tags used most, each with a link to its page.
    #[doc(alias = "chart.getTopTags")]
    pub fn top_tags(&self) -> ChartsTopTagsQuery {
        ChartsTopTagsQuery::new(Ask::new(&self.client, &methods::CHART_GET_TOP_TAGS))
    }

    /// The tags used most, by the service's other method for it: this one
    /// can start part of the way down the list, and its tags have no link.
    #[doc(alias = "tag.getTopTags")]
    pub fn tags(&self) -> ChartsTagsQuery {
        ChartsTagsQuery {
            client: self.client.clone(),
            limit: None,
            offset: None,
        }
    }
}

query! {
    /// The artists played most on the whole service. Made by
    /// [`ChartsHandle::top_artists`].
    ///
    /// ```no_run
    /// # async fn demo(client: scrobl::Client) -> Result<(), scrobl::Error> {
    /// let page = client.charts().top_artists().limit(10).send().await?;
    /// for (place, artist) in page.ranked() {
    ///     println!("{place}. {} ({:?} plays)", artist.name(), artist.playcount());
    /// }
    /// # Ok(())
    /// # }
    /// ```
    ChartsTopArtistsQuery => Page<Artist> = decode::chart_get_top_artists;
    limit = "Sets `limit`, the page size, from 1 to 1000. The service refuses a larger one.",
    page
}

query! {
    /// The tracks played most on the whole service. Made by
    /// [`ChartsHandle::top_tracks`].
    ChartsTopTracksQuery => Page<Track> = decode::chart_get_top_tracks;
    limit = "Sets `limit`, the page size, from 1 to 1000. The service refuses a larger one.",
    page
}

query! {
    /// The tags used most on the whole service. Made by
    /// [`ChartsHandle::top_tags`].
    ///
    /// It has no `page`: the service sends the same first rows whatever
    /// page is asked for. A larger [`limit`](Self::limit) is the way to
    /// read further.
    ChartsTopTagsQuery => List<Tag> = decode::chart_get_top_tags;
    limit = "Sets `limit`, how many tags to read, from 1 to 1000. The service refuses a \
             larger one."
}

/// The tags used most on the whole service, by `tag.getTopTags`. Made by
/// [`ChartsHandle::tags`].
///
/// The method's page documents no parameter, and the service sends the 50
/// most used tags when none is sent. It ignores a `limit`. What it goes by
/// are two parameters its page does not list, `num_res` and `offset`,
/// which [`limit`](Self::limit) and [`offset`](Self::offset) send. They
/// worked on 2026-11-11, and the service owes nobody that they go on
/// working.
///
/// ```no_run
/// # async fn demo(client: scrobl::Client) -> Result<(), scrobl::Error> {
/// let list = client.charts().tags().limit(100).offset(100).send().await?;
/// println!("tags 101 to {}", 100 + list.len());
/// # Ok(())
/// # }
/// ```
#[derive(Debug, Clone)]
#[must_use = "a query does nothing until it is sent"]
pub struct ChartsTagsQuery {
    client: Client,
    limit: Option<u32>,
    offset: Option<u64>,
}

impl ChartsTagsQuery {
    /// Sets how many tags to read, sent as `num_res`.
    pub fn limit(mut self, limit: u32) -> Self {
        self.limit = Some(limit);
        self
    }

    /// Sets how many of the most used tags to pass over first, sent as
    /// `offset`.
    pub fn offset(mut self, offset: u64) -> Self {
        self.offset = Some(offset);
        self
    }

    /// Makes the call. The exact response is on the result, next to the
    /// typed value.
    ///
    /// # Errors
    ///
    /// Any error of [`Client::call`], and
    /// [`ErrorKind::Decode`](crate::ErrorKind) when the reply is not the
    /// expected shape.
    pub async fn send(self) -> Result<Response<List<Tag>>, Error> {
        let mut request = Request::new(&methods::TAG_GET_TOP_TAGS);
        if let Some(limit) = self.limit {
            request = request.param("num_res", limit);
        }
        if let Some(offset) = self.offset {
            request = request.param("offset", offset);
        }
        let raw = self.client.call(&request).await?;
        let value = decode::tag_get_top_tags(&raw)?;
        Ok(Response::new(value, raw))
    }
}
