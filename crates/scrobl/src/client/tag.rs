//! Typed reads about one tag: what it is, and what was given it most.

use super::Client;
use super::query::{Ask, query};
use crate::history::Window;
use crate::model::{Album, Artist, List, Page, Tag, TagInfo, Track, decode};
use crate::protocol::{MethodSpec, methods};

/// One tag, to ask about. Made by [`Client::tag`].
///
/// A tag nobody has used is not an error: its lists are empty and its
/// counts 0.
#[derive(Debug, Clone)]
pub struct TagHandle {
    client: Client,
    name: String,
}

impl TagHandle {
    pub(super) fn new(client: Client, name: String) -> Self {
        Self { client, name }
    }

    fn ask(&self, spec: &'static MethodSpec) -> Ask {
        Ask::new(&self.client, spec).about("tag", &self.name)
    }

    /// The tag as given.
    pub fn name(&self) -> &str {
        &self.name
    }

    /// What the service says about the tag.
    #[doc(alias = "tag.getInfo")]
    pub fn info(&self) -> TagInfoQuery {
        TagInfoQuery::new(self.ask(&methods::TAG_GET_INFO))
    }

    /// The tags the service finds like this one.
    #[doc(alias = "tag.getSimilar")]
    pub fn similar(&self) -> TagSimilarQuery {
        TagSimilarQuery::new(self.ask(&methods::TAG_GET_SIMILAR))
    }

    /// The artists given the tag most.
    #[doc(alias = "tag.getTopArtists")]
    pub fn top_artists(&self) -> TagTopArtistsQuery {
        TagTopArtistsQuery::new(self.ask(&methods::TAG_GET_TOP_ARTISTS))
    }

    /// The albums given the tag most.
    #[doc(alias = "tag.getTopAlbums")]
    pub fn top_albums(&self) -> TagTopAlbumsQuery {
        TagTopAlbumsQuery::new(self.ask(&methods::TAG_GET_TOP_ALBUMS))
    }

    /// The tracks given the tag most.
    #[doc(alias = "tag.getTopTracks")]
    pub fn top_tracks(&self) -> TagTopTracksQuery {
        TagTopTracksQuery::new(self.ask(&methods::TAG_GET_TOP_TRACKS))
    }

    /// The weeks the service keeps charts for, oldest first.
    #[doc(alias = "tag.getWeeklyChartList")]
    pub fn weeks(&self) -> TagWeeksQuery {
        TagWeeksQuery::new(self.ask(&methods::TAG_GET_WEEKLY_CHART_LIST))
    }
}

query! {
    /// What the service says about one tag. Made by [`TagHandle::info`].
    ///
    /// ```no_run
    /// # async fn demo(client: scrobl::Client) -> Result<(), scrobl::Error> {
    /// let tag = client.tag("indie").info().send().await?;
    /// println!("{}: used {} times by {} users", tag.tag().name(), tag.taggings(), tag.reach());
    /// # Ok(())
    /// # }
    /// ```
    TagInfoQuery => TagInfo = decode::tag_get_info;
    lang
}

query! {
    /// The tags the service finds like one tag. Made by
    /// [`TagHandle::similar`]. The service has answered every tag tried
    /// with an empty list; see [`decode::tag_get_similar`].
    TagSimilarQuery => List<Tag> = decode::tag_get_similar;
}

query! {
    /// The artists given a tag most. Made by [`TagHandle::top_artists`].
    TagTopArtistsQuery => Page<Artist> = decode::tag_get_top_artists;
    limit = "Sets `limit`, the page size, from 1 to 1000. The service refuses a larger one.",
    page
}

query! {
    /// The albums given a tag most. Made by [`TagHandle::top_albums`].
    ///
    /// ```no_run
    /// # async fn demo(client: scrobl::Client) -> Result<(), scrobl::Error> {
    /// let page = client.tag("indie").top_albums().limit(10).send().await?;
    /// for album in page.iter() {
    ///     println!("{} by {}", album.name(), album.artist().map_or("?", |artist| artist.name()));
    /// }
    /// # Ok(())
    /// # }
    /// ```
    TagTopAlbumsQuery => Page<Album> = decode::tag_get_top_albums;
    limit = "Sets `limit`, the page size, from 1 to 1000. The service refuses a larger one.",
    page
}

query! {
    /// The tracks given a tag most. Made by [`TagHandle::top_tracks`].
    TagTopTracksQuery => Page<Track> = decode::tag_get_top_tracks;
    limit = "Sets `limit`, the page size, from 1 to 1000. The service refuses a larger one.",
    page
}

query! {
    /// The weeks the service keeps charts for. Made by
    /// [`TagHandle::weeks`].
    ///
    /// The weeks are the service's own: it sent the same list for every tag
    /// tried, and the list a user gets.
    TagWeeksQuery => List<Window> = decode::tag_get_weekly_chart_list;
}
