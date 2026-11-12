//! Typed reads about tags: what one tag is and what was given it most, and
//! the list of every tag.

use super::Client;
use super::charts::{Chart, page_query};
use super::lists::Listing;
use crate::error::Error;
use crate::model::{
    Album, Artist, List, Page, Tag as TagEntity, TagInfo, Track, WeeklyChartRange, decode,
};
use crate::protocol::{Request, methods};
use crate::response::Response;

/// One tag. Made by [`Client::tag`].
#[derive(Debug, Clone)]
pub struct Tag {
    client: Client,
    name: String,
}

impl Tag {
    pub(super) fn new(client: Client, name: String) -> Self {
        Self { client, name }
    }

    /// The tag as given.
    pub fn name(&self) -> &str {
        &self.name
    }

    /// Starts a `tag.getInfo` query for this tag.
    pub fn info(&self) -> TagInfoQuery {
        TagInfoQuery {
            client: self.client.clone(),
            tag: self.name.clone(),
            lang: None,
        }
    }

    /// Starts a `tag.getSimilar` query for this tag.
    pub fn similar(&self) -> SimilarTagsQuery {
        SimilarTagsQuery { list: self.list() }
    }

    /// Starts a `tag.getTopAlbums` query for this tag.
    pub fn top_albums(&self) -> TagTopAlbumsQuery {
        TagTopAlbumsQuery { list: self.list() }
    }

    /// Starts a `tag.getTopArtists` query for this tag.
    pub fn top_artists(&self) -> TagTopArtistsQuery {
        TagTopArtistsQuery { list: self.list() }
    }

    /// Starts a `tag.getTopTracks` query for this tag.
    pub fn top_tracks(&self) -> TagTopTracksQuery {
        TagTopTracksQuery { list: self.list() }
    }

    /// Starts a `tag.getWeeklyChartList` query for this tag.
    pub fn weekly_chart_list(&self) -> TagWeeklyChartListQuery {
        TagWeeklyChartListQuery { list: self.list() }
    }

    fn list(&self) -> Listing {
        Listing::new(self.client.clone(), Some(("tag", self.name.clone())))
    }
}

impl Chart {
    /// Starts a `tag.getTopTags` query: every tag of the service, most used
    /// first. It is here and not on a [`Tag`] because it is about no one
    /// tag.
    pub fn all_tags(&self) -> AllTagsQuery {
        AllTagsQuery {
            client: self.client().clone(),
            limit: None,
            offset: None,
        }
    }
}

/// A `tag.getInfo` query: what the service says about one tag. Made by
/// [`Tag::info`].
///
/// ```no_run
/// # async fn demo(client: scrobl::Client) -> Result<(), scrobl::Error> {
/// let tag = client.tag("indie").info().send().await?;
/// println!("{}: used {} times by {} users", tag.tag().name(), tag.taggings(), tag.reach());
/// # Ok(())
/// # }
/// ```
#[derive(Debug, Clone)]
#[must_use = "a query does nothing until it is sent"]
pub struct TagInfoQuery {
    client: Client,
    tag: String,
    lang: Option<String>,
}

impl TagInfoQuery {
    /// Sets `lang`, the language to give the description in, as an ISO
    /// 639 code of two letters such as `de`.
    pub fn lang(mut self, lang: impl Into<String>) -> Self {
        self.lang = Some(lang.into());
        self
    }

    /// Reads the tag. The exact response is on the result, next to the
    /// typed value. A tag nobody has used is not an error: its counts are
    /// 0.
    ///
    /// # Errors
    ///
    /// Any error of [`Client::call`], and
    /// [`ErrorKind::Decode`](crate::ErrorKind) when the reply is not the
    /// expected shape.
    pub async fn send(self) -> Result<Response<TagInfo>, Error> {
        let mut request = Request::new(&methods::TAG_GET_INFO).param("tag", &self.tag);
        if let Some(lang) = &self.lang {
            request = request.param("lang", lang);
        }
        let raw = self.client.call(&request).await?;
        let value = decode::tag_get_info(&raw)?;
        Ok(Response::new(value, raw))
    }
}

/// A `tag.getSimilar` query: the tags the service finds like one tag. Made
/// by [`Tag::similar`]. See [`decode::tag_get_similar`] for what the service has
/// answered.
#[derive(Debug, Clone)]
#[must_use = "a query does nothing until it is sent"]
pub struct SimilarTagsQuery {
    list: Listing,
}

impl SimilarTagsQuery {
    /// Reads the list. The exact response is on the result, next to the
    /// typed value.
    ///
    /// # Errors
    ///
    /// Any error of [`Client::call`], and
    /// [`ErrorKind::Decode`](crate::ErrorKind) when the reply is not the
    /// expected shape.
    pub async fn send(self) -> Result<Response<List<TagEntity>>, Error> {
        self.list
            .send(&methods::TAG_GET_SIMILAR, decode::tag_get_similar)
            .await
    }
}

page_query! {
    /// A `tag.getTopAlbums` query: the albums given a tag most. Made by
    /// [`Tag::top_albums`].
    ///
    /// ```no_run
    /// # async fn demo(client: scrobl::Client) -> Result<(), scrobl::Error> {
    /// let page = client.tag("indie").top_albums().limit(10).send().await?;
    /// for row in page.items() {
    ///     println!("{} by {}", row.name(), row.artist().map_or("?", |artist| artist.name()));
    /// }
    /// # Ok(())
    /// # }
    /// ```
    TagTopAlbumsQuery, TAG_GET_TOP_ALBUMS, Album, decode::tag_get_top_albums
}

page_query! {
    /// A `tag.getTopArtists` query: the artists given a tag most. Made by
    /// [`Tag::top_artists`].
    TagTopArtistsQuery, TAG_GET_TOP_ARTISTS, Artist, decode::tag_get_top_artists
}

page_query! {
    /// A `tag.getTopTracks` query: the tracks given a tag most. Made by
    /// [`Tag::top_tracks`].
    TagTopTracksQuery, TAG_GET_TOP_TRACKS, Track, decode::tag_get_top_tracks
}

/// A `tag.getWeeklyChartList` query: the weeks the service keeps charts
/// for. Made by [`Tag::weekly_chart_list`].
#[derive(Debug, Clone)]
#[must_use = "a query does nothing until it is sent"]
pub struct TagWeeklyChartListQuery {
    list: Listing,
}

impl TagWeeklyChartListQuery {
    /// Reads the list. The exact response is on the result, next to the
    /// typed value.
    ///
    /// # Errors
    ///
    /// Any error of [`Client::call`], and
    /// [`ErrorKind::Decode`](crate::ErrorKind) when the reply is not the
    /// expected shape.
    pub async fn send(self) -> Result<Response<List<WeeklyChartRange>>, Error> {
        self.list
            .send(
                &methods::TAG_GET_WEEKLY_CHART_LIST,
                decode::tag_get_weekly_chart_list,
            )
            .await
    }
}

/// A `tag.getTopTags` query: every tag of the service, most used first.
/// Made by [`Chart::all_tags`].
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
/// let list = client.chart().all_tags().limit(100).offset(100).send().await?;
/// println!("tags 101 to {}", 100 + list.len());
/// # Ok(())
/// # }
/// ```
#[derive(Debug, Clone)]
#[must_use = "a query does nothing until it is sent"]
pub struct AllTagsQuery {
    client: Client,
    limit: Option<u32>,
    offset: Option<u64>,
}

impl AllTagsQuery {
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

    /// Reads the list. The exact response is on the result, next to the
    /// typed value.
    ///
    /// # Errors
    ///
    /// Any error of [`Client::call`], and
    /// [`ErrorKind::Decode`](crate::ErrorKind) when the reply is not the
    /// expected shape.
    pub async fn send(self) -> Result<Response<List<TagEntity>>, Error> {
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
