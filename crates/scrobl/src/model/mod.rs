//! Typed views of responses, one module per API package.
//!
//! A model is decoded from a [`Raw`](crate::protocol::Raw) on request and
//! never replaces it. Fields are private; each item keeps its own decoded
//! JSON, so unknown fields and the difference between a missing and an empty
//! field stay reachable. A malformed value is an error naming the field, and
//! is never dropped or defaulted.
//!
//! The `Debug` output of a model leaves out the text of the response.

mod album;
mod artist;
mod chart;
mod entity;
mod geo;
mod library;
mod page;
mod search;
mod shared;
mod tag;
mod track;
mod user;

pub use album::AlbumInfo;
pub use artist::{ArtistInfo, SimilarArtist, TopTag};
pub use entity::{Album, Artist, Image, Images, Tag, Track};
pub use geo::{CountryArtist, CountryTrack};
pub use page::{List, Page};
pub use tag::TagInfo;
pub use track::{SimilarTrack, TrackInfo};
pub use user::{
    HistoryPage, LovedTrack, NowPlaying, Scrobble, User, UserAlbum, UserArtist, UserTag, UserTrack,
};

pub mod decode {
    //! One function for each typed method, named after the method:
    //! [`user_get_top_artists`] decodes a `user.getTopArtists` response.
    //!
    //! The client calls these for its typed queries. They are public for a
    //! caller that runs the protocol core with another HTTP client.
    //!
    //! # Errors
    //!
    //! Every function here fails with
    //! [`ErrorKind::Decode`](crate::ErrorKind) when:
    //!
    //! - the response is for another method;
    //! - the list, its `@attr` or a member read from it is missing or
    //!   malformed, including one of the four paging numbers of a paged
    //!   list;
    //! - a row is not an object, or lacks what the function says a row
    //!   needs, or has a field of the wrong type;
    //! - a paged list holds no rows although its `@attr` promises some on
    //!   that page. Rows may be an array, a single object when there is
    //!   exactly one, and absent, `[]` or `""` only where `total` is 0 or
    //!   the page lies beyond the last;
    //! - an object repeats a member name.
    //!
    //! The message names the field, such as `topartists.artist[3].playcount`,
    //! and never repeats response text.

    pub use super::album::{album_get_info, album_get_tags, album_get_top_tags};
    pub use super::artist::{
        artist_get_correction, artist_get_info, artist_get_similar, artist_get_tags,
        artist_get_top_albums, artist_get_top_tags, artist_get_top_tracks,
    };
    pub use super::chart::{chart_get_top_artists, chart_get_top_tags, chart_get_top_tracks};
    pub use super::geo::{geo_get_top_artists, geo_get_top_tracks};
    pub use super::library::library_get_artists;
    pub use super::search::{album_search, artist_search, track_search};
    pub use super::tag::{
        tag_get_info, tag_get_similar, tag_get_top_albums, tag_get_top_artists, tag_get_top_tags,
        tag_get_top_tracks, tag_get_weekly_chart_list,
    };
    pub use super::track::{
        track_get_correction, track_get_info, track_get_similar, track_get_tags, track_get_top_tags,
    };
    pub use super::user::{
        user_get_friends, user_get_info, user_get_loved_tracks, user_get_personal_tags_albums,
        user_get_personal_tags_artists, user_get_personal_tags_tracks, user_get_recent_tracks,
        user_get_top_albums, user_get_top_artists, user_get_top_tags, user_get_top_tracks,
        user_get_weekly_album_chart, user_get_weekly_artist_chart, user_get_weekly_chart_list,
        user_get_weekly_track_chart,
    };
}
