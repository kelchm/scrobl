//! Every method in the official Last.fm API index.
//!
//! The table follows the official method pages, snapshot of 2026-10-05, and is
//! checked against `fixtures/official/method-parameters.json` by the
//! `inventory` test. Differences between a page and the table are listed in
//! `docs/endpoints.md`.
//!
//! `track.ban`, `track.unban`, `user.getArtistTracks` and
//! `user.getNewReleases` are not in the index at that date and are left out.

use super::method::{Auth, MethodSpec, Paging, ParamSpec, Requirement, Verb};

const fn param(name: &'static str, requirement: Requirement) -> ParamSpec {
    ParamSpec {
        name,
        requirement,
        indexed: false,
    }
}

const fn indexed(name: &'static str, requirement: Requirement) -> ParamSpec {
    ParamSpec {
        name,
        requirement,
        indexed: true,
    }
}

/// Tags an album on behalf of the session user.
pub const ALBUM_ADD_TAGS: MethodSpec = MethodSpec {
    name: "album.addTags",
    verb: Verb::Post,
    auth: Auth::Session,
    write: true,
    paging: Paging::None,
    params: &[
        param("artist", Requirement::Required),
        param("album", Requirement::Required),
        param("tags", Requirement::Required),
    ],
};

/// Metadata for an album.
pub const ALBUM_GET_INFO: MethodSpec = MethodSpec {
    name: "album.getInfo",
    verb: Verb::Get,
    auth: Auth::ApiKey,
    write: false,
    paging: Paging::None,
    params: &[
        param("artist", Requirement::Conditional),
        param("album", Requirement::Conditional),
        param("mbid", Requirement::Optional),
        param("autocorrect", Requirement::Optional),
        param("username", Requirement::Optional),
        param("lang", Requirement::Optional),
    ],
};

/// The tags a user has applied to an album.
pub const ALBUM_GET_TAGS: MethodSpec = MethodSpec {
    name: "album.getTags",
    verb: Verb::Get,
    auth: Auth::ApiKey,
    write: false,
    paging: Paging::None,
    params: &[
        param("artist", Requirement::Conditional),
        param("album", Requirement::Conditional),
        param("mbid", Requirement::Optional),
        param("autocorrect", Requirement::Optional),
        param("user", Requirement::Optional),
    ],
};

/// The most applied tags for an album.
pub const ALBUM_GET_TOP_TAGS: MethodSpec = MethodSpec {
    name: "album.getTopTags",
    verb: Verb::Get,
    auth: Auth::ApiKey,
    write: false,
    paging: Paging::None,
    params: &[
        param("artist", Requirement::Conditional),
        param("album", Requirement::Conditional),
        param("autocorrect", Requirement::Optional),
        param("mbid", Requirement::Optional),
    ],
};

/// Removes a tag the session user applied to an album.
pub const ALBUM_REMOVE_TAG: MethodSpec = MethodSpec {
    name: "album.removeTag",
    verb: Verb::Post,
    auth: Auth::Session,
    write: true,
    paging: Paging::None,
    params: &[
        param("artist", Requirement::Required),
        param("album", Requirement::Required),
        param("tag", Requirement::Required),
    ],
};

/// Searches for an album by name.
pub const ALBUM_SEARCH: MethodSpec = MethodSpec {
    name: "album.search",
    verb: Verb::Get,
    auth: Auth::ApiKey,
    write: false,
    paging: Paging::PageAndLimit,
    params: &[
        param("limit", Requirement::Optional),
        param("page", Requirement::Optional),
        param("album", Requirement::Required),
    ],
};

/// Tags an artist on behalf of the session user.
pub const ARTIST_ADD_TAGS: MethodSpec = MethodSpec {
    name: "artist.addTags",
    verb: Verb::Post,
    auth: Auth::Session,
    write: true,
    paging: Paging::None,
    params: &[
        param("artist", Requirement::Required),
        param("tags", Requirement::Required),
    ],
};

/// The canonical spelling of an artist name.
pub const ARTIST_GET_CORRECTION: MethodSpec = MethodSpec {
    name: "artist.getCorrection",
    verb: Verb::Get,
    auth: Auth::ApiKey,
    write: false,
    paging: Paging::None,
    params: &[param("artist", Requirement::Required)],
};

/// Metadata for an artist.
pub const ARTIST_GET_INFO: MethodSpec = MethodSpec {
    name: "artist.getInfo",
    verb: Verb::Get,
    auth: Auth::ApiKey,
    write: false,
    paging: Paging::None,
    params: &[
        param("artist", Requirement::Conditional),
        param("mbid", Requirement::Optional),
        param("lang", Requirement::Optional),
        param("autocorrect", Requirement::Optional),
        param("username", Requirement::Optional),
    ],
};

/// Artists similar to an artist.
pub const ARTIST_GET_SIMILAR: MethodSpec = MethodSpec {
    name: "artist.getSimilar",
    verb: Verb::Get,
    auth: Auth::ApiKey,
    write: false,
    paging: Paging::LimitOnly,
    params: &[
        param("limit", Requirement::Optional),
        param("artist", Requirement::Conditional),
        param("autocorrect", Requirement::Optional),
        param("mbid", Requirement::Optional),
    ],
};

/// The tags a user has applied to an artist.
pub const ARTIST_GET_TAGS: MethodSpec = MethodSpec {
    name: "artist.getTags",
    verb: Verb::Get,
    auth: Auth::ApiKey,
    write: false,
    paging: Paging::None,
    params: &[
        param("artist", Requirement::Conditional),
        param("mbid", Requirement::Optional),
        param("user", Requirement::Optional),
        param("autocorrect", Requirement::Optional),
    ],
};

/// The most popular albums by an artist.
pub const ARTIST_GET_TOP_ALBUMS: MethodSpec = MethodSpec {
    name: "artist.getTopAlbums",
    verb: Verb::Get,
    auth: Auth::ApiKey,
    write: false,
    paging: Paging::PageAndLimit,
    params: &[
        param("artist", Requirement::Conditional),
        param("mbid", Requirement::Optional),
        param("autocorrect", Requirement::Optional),
        param("page", Requirement::Optional),
        param("limit", Requirement::Optional),
    ],
};

/// The most applied tags for an artist.
pub const ARTIST_GET_TOP_TAGS: MethodSpec = MethodSpec {
    name: "artist.getTopTags",
    verb: Verb::Get,
    auth: Auth::ApiKey,
    write: false,
    paging: Paging::None,
    params: &[
        param("artist", Requirement::Conditional),
        param("mbid", Requirement::Optional),
        param("autocorrect", Requirement::Optional),
    ],
};

/// The most popular tracks by an artist.
pub const ARTIST_GET_TOP_TRACKS: MethodSpec = MethodSpec {
    name: "artist.getTopTracks",
    verb: Verb::Get,
    auth: Auth::ApiKey,
    write: false,
    paging: Paging::PageAndLimit,
    params: &[
        param("artist", Requirement::Conditional),
        param("mbid", Requirement::Optional),
        param("autocorrect", Requirement::Optional),
        param("page", Requirement::Optional),
        param("limit", Requirement::Optional),
    ],
};

/// Removes a tag the session user applied to an artist.
pub const ARTIST_REMOVE_TAG: MethodSpec = MethodSpec {
    name: "artist.removeTag",
    verb: Verb::Post,
    auth: Auth::Session,
    write: true,
    paging: Paging::None,
    params: &[
        param("artist", Requirement::Required),
        param("tag", Requirement::Required),
    ],
};

/// Searches for an artist by name.
pub const ARTIST_SEARCH: MethodSpec = MethodSpec {
    name: "artist.search",
    verb: Verb::Get,
    auth: Auth::ApiKey,
    write: false,
    paging: Paging::PageAndLimit,
    params: &[
        param("limit", Requirement::Optional),
        param("page", Requirement::Optional),
        param("artist", Requirement::Required),
    ],
};

/// Exchanges a username and password for a session key.
pub const AUTH_GET_MOBILE_SESSION: MethodSpec = MethodSpec {
    name: "auth.getMobileSession",
    verb: Verb::Post,
    auth: Auth::Signed,
    write: false,
    paging: Paging::None,
    params: &[
        param("password", Requirement::Required),
        param("username", Requirement::Required),
    ],
};

/// Exchanges an authorised token for a session key.
pub const AUTH_GET_SESSION: MethodSpec = MethodSpec {
    name: "auth.getSession",
    verb: Verb::Get,
    auth: Auth::Signed,
    write: false,
    paging: Paging::None,
    params: &[param("token", Requirement::Required)],
};

/// Fetches an unauthorised request token.
pub const AUTH_GET_TOKEN: MethodSpec = MethodSpec {
    name: "auth.getToken",
    verb: Verb::Get,
    auth: Auth::Signed,
    write: false,
    paging: Paging::None,
    params: &[],
};

/// The most popular artists on Last.fm.
pub const CHART_GET_TOP_ARTISTS: MethodSpec = MethodSpec {
    name: "chart.getTopArtists",
    verb: Verb::Get,
    auth: Auth::ApiKey,
    write: false,
    paging: Paging::PageAndLimit,
    params: &[
        param("page", Requirement::Optional),
        param("limit", Requirement::Optional),
    ],
};

/// The most applied tags on Last.fm.
pub const CHART_GET_TOP_TAGS: MethodSpec = MethodSpec {
    name: "chart.getTopTags",
    verb: Verb::Get,
    auth: Auth::ApiKey,
    write: false,
    paging: Paging::PageAndLimit,
    params: &[
        param("page", Requirement::Optional),
        param("limit", Requirement::Optional),
    ],
};

/// The most popular tracks on Last.fm.
pub const CHART_GET_TOP_TRACKS: MethodSpec = MethodSpec {
    name: "chart.getTopTracks",
    verb: Verb::Get,
    auth: Auth::ApiKey,
    write: false,
    paging: Paging::PageAndLimit,
    params: &[
        param("page", Requirement::Optional),
        param("limit", Requirement::Optional),
    ],
};

/// The most popular artists in a country.
pub const GEO_GET_TOP_ARTISTS: MethodSpec = MethodSpec {
    name: "geo.getTopArtists",
    verb: Verb::Get,
    auth: Auth::ApiKey,
    write: false,
    paging: Paging::PageAndLimit,
    params: &[
        param("country", Requirement::Required),
        param("limit", Requirement::Optional),
        param("page", Requirement::Optional),
    ],
};

/// The most popular tracks in a country.
pub const GEO_GET_TOP_TRACKS: MethodSpec = MethodSpec {
    name: "geo.getTopTracks",
    verb: Verb::Get,
    auth: Auth::ApiKey,
    write: false,
    paging: Paging::PageAndLimit,
    params: &[
        param("country", Requirement::Required),
        param("location", Requirement::Optional),
        param("limit", Requirement::Optional),
        param("page", Requirement::Optional),
    ],
};

/// The artists in a user's library.
pub const LIBRARY_GET_ARTISTS: MethodSpec = MethodSpec {
    name: "library.getArtists",
    verb: Verb::Get,
    auth: Auth::ApiKey,
    write: false,
    paging: Paging::PageAndLimit,
    params: &[
        param("user", Requirement::Required),
        param("limit", Requirement::Optional),
        param("page", Requirement::Optional),
    ],
};

/// Metadata for a tag.
pub const TAG_GET_INFO: MethodSpec = MethodSpec {
    name: "tag.getInfo",
    verb: Verb::Get,
    auth: Auth::ApiKey,
    write: false,
    paging: Paging::None,
    params: &[
        param("lang", Requirement::Optional),
        param("tag", Requirement::Required),
    ],
};

/// Tags similar to a tag.
pub const TAG_GET_SIMILAR: MethodSpec = MethodSpec {
    name: "tag.getSimilar",
    verb: Verb::Get,
    auth: Auth::ApiKey,
    write: false,
    paging: Paging::None,
    params: &[param("tag", Requirement::Required)],
};

/// The albums most tagged with a tag.
pub const TAG_GET_TOP_ALBUMS: MethodSpec = MethodSpec {
    name: "tag.getTopAlbums",
    verb: Verb::Get,
    auth: Auth::ApiKey,
    write: false,
    paging: Paging::PageAndLimit,
    params: &[
        param("tag", Requirement::Required),
        param("limit", Requirement::Optional),
        param("page", Requirement::Optional),
    ],
};

/// The artists most tagged with a tag.
pub const TAG_GET_TOP_ARTISTS: MethodSpec = MethodSpec {
    name: "tag.getTopArtists",
    verb: Verb::Get,
    auth: Auth::ApiKey,
    write: false,
    paging: Paging::PageAndLimit,
    params: &[
        param("tag", Requirement::Required),
        param("limit", Requirement::Optional),
        param("page", Requirement::Optional),
    ],
};

/// The most popular tags on Last.fm.
pub const TAG_GET_TOP_TAGS: MethodSpec = MethodSpec {
    name: "tag.getTopTags",
    verb: Verb::Get,
    auth: Auth::ApiKey,
    write: false,
    paging: Paging::None,
    params: &[],
};

/// The tracks most tagged with a tag.
pub const TAG_GET_TOP_TRACKS: MethodSpec = MethodSpec {
    name: "tag.getTopTracks",
    verb: Verb::Get,
    auth: Auth::ApiKey,
    write: false,
    paging: Paging::PageAndLimit,
    params: &[
        param("tag", Requirement::Required),
        param("limit", Requirement::Optional),
        param("page", Requirement::Optional),
    ],
};

/// The weekly chart ranges available for a tag.
pub const TAG_GET_WEEKLY_CHART_LIST: MethodSpec = MethodSpec {
    name: "tag.getWeeklyChartList",
    verb: Verb::Get,
    auth: Auth::ApiKey,
    write: false,
    paging: Paging::None,
    params: &[param("tag", Requirement::Required)],
};

/// Tags a track on behalf of the session user.
pub const TRACK_ADD_TAGS: MethodSpec = MethodSpec {
    name: "track.addTags",
    verb: Verb::Post,
    auth: Auth::Session,
    write: true,
    paging: Paging::None,
    params: &[
        param("artist", Requirement::Required),
        param("track", Requirement::Required),
        param("tags", Requirement::Required),
    ],
};

/// The canonical spelling of a track and artist name.
pub const TRACK_GET_CORRECTION: MethodSpec = MethodSpec {
    name: "track.getCorrection",
    verb: Verb::Get,
    auth: Auth::ApiKey,
    write: false,
    paging: Paging::None,
    params: &[
        param("artist", Requirement::Required),
        param("track", Requirement::Required),
    ],
};

/// Metadata for a track.
pub const TRACK_GET_INFO: MethodSpec = MethodSpec {
    name: "track.getInfo",
    verb: Verb::Get,
    auth: Auth::ApiKey,
    write: false,
    paging: Paging::None,
    params: &[
        param("mbid", Requirement::Optional),
        param("track", Requirement::Conditional),
        param("artist", Requirement::Conditional),
        param("username", Requirement::Optional),
        param("autocorrect", Requirement::Optional),
    ],
};

/// Tracks similar to a track.
pub const TRACK_GET_SIMILAR: MethodSpec = MethodSpec {
    name: "track.getSimilar",
    verb: Verb::Get,
    auth: Auth::ApiKey,
    write: false,
    paging: Paging::LimitOnly,
    params: &[
        param("track", Requirement::Conditional),
        param("artist", Requirement::Conditional),
        param("mbid", Requirement::Optional),
        param("autocorrect", Requirement::Optional),
        param("limit", Requirement::Optional),
    ],
};

/// The tags a user has applied to a track.
pub const TRACK_GET_TAGS: MethodSpec = MethodSpec {
    name: "track.getTags",
    verb: Verb::Get,
    auth: Auth::ApiKey,
    write: false,
    paging: Paging::None,
    params: &[
        param("artist", Requirement::Conditional),
        param("track", Requirement::Conditional),
        param("mbid", Requirement::Optional),
        param("autocorrect", Requirement::Optional),
        param("user", Requirement::Optional),
    ],
};

/// The most applied tags for a track.
pub const TRACK_GET_TOP_TAGS: MethodSpec = MethodSpec {
    name: "track.getTopTags",
    verb: Verb::Get,
    auth: Auth::ApiKey,
    write: false,
    paging: Paging::None,
    params: &[
        param("track", Requirement::Conditional),
        param("artist", Requirement::Conditional),
        param("mbid", Requirement::Optional),
        param("autocorrect", Requirement::Optional),
    ],
};

/// Marks a track as loved by the session user.
pub const TRACK_LOVE: MethodSpec = MethodSpec {
    name: "track.love",
    verb: Verb::Post,
    auth: Auth::Session,
    write: true,
    paging: Paging::None,
    params: &[
        param("track", Requirement::Required),
        param("artist", Requirement::Required),
    ],
};

/// Removes a tag the session user applied to a track.
pub const TRACK_REMOVE_TAG: MethodSpec = MethodSpec {
    name: "track.removeTag",
    verb: Verb::Post,
    auth: Auth::Session,
    write: true,
    paging: Paging::None,
    params: &[
        param("artist", Requirement::Required),
        param("track", Requirement::Required),
        param("tag", Requirement::Required),
    ],
};

/// Submits up to 50 scrobbles for the session user.
pub const TRACK_SCROBBLE: MethodSpec = MethodSpec {
    name: "track.scrobble",
    verb: Verb::Post,
    auth: Auth::Session,
    write: true,
    paging: Paging::None,
    params: &[
        indexed("artist", Requirement::Required),
        indexed("track", Requirement::Required),
        indexed("timestamp", Requirement::Required),
        indexed("album", Requirement::Optional),
        indexed("context", Requirement::Optional),
        indexed("streamId", Requirement::Optional),
        indexed("chosenByUser", Requirement::Optional),
        indexed("trackNumber", Requirement::Optional),
        indexed("mbid", Requirement::Optional),
        indexed("albumArtist", Requirement::Optional),
        indexed("duration", Requirement::Optional),
    ],
};

/// Searches for a track by name.
pub const TRACK_SEARCH: MethodSpec = MethodSpec {
    name: "track.search",
    verb: Verb::Get,
    auth: Auth::ApiKey,
    write: false,
    paging: Paging::PageAndLimit,
    params: &[
        param("limit", Requirement::Optional),
        param("page", Requirement::Optional),
        param("track", Requirement::Required),
        param("artist", Requirement::Optional),
    ],
};

/// Removes a track from the session user's loved tracks.
pub const TRACK_UNLOVE: MethodSpec = MethodSpec {
    name: "track.unlove",
    verb: Verb::Post,
    auth: Auth::Session,
    write: true,
    paging: Paging::None,
    params: &[
        param("track", Requirement::Required),
        param("artist", Requirement::Required),
    ],
};

/// Sets the session user's now-playing track.
pub const TRACK_UPDATE_NOW_PLAYING: MethodSpec = MethodSpec {
    name: "track.updateNowPlaying",
    verb: Verb::Post,
    auth: Auth::Session,
    write: true,
    paging: Paging::None,
    params: &[
        param("artist", Requirement::Required),
        param("track", Requirement::Required),
        param("album", Requirement::Optional),
        param("trackNumber", Requirement::Optional),
        param("context", Requirement::Optional),
        param("mbid", Requirement::Optional),
        param("duration", Requirement::Optional),
        param("albumArtist", Requirement::Optional),
    ],
};

/// The friends of a user.
pub const USER_GET_FRIENDS: MethodSpec = MethodSpec {
    name: "user.getFriends",
    verb: Verb::Get,
    auth: Auth::ApiKey,
    write: false,
    paging: Paging::PageAndLimit,
    params: &[
        param("user", Requirement::Required),
        param("recenttracks", Requirement::Optional),
        param("limit", Requirement::Optional),
        param("page", Requirement::Optional),
    ],
};

/// Profile information for a user.
pub const USER_GET_INFO: MethodSpec = MethodSpec {
    name: "user.getInfo",
    verb: Verb::Get,
    auth: Auth::ApiKey,
    write: false,
    paging: Paging::None,
    params: &[param("user", Requirement::Optional)],
};

/// The tracks a user has loved.
pub const USER_GET_LOVED_TRACKS: MethodSpec = MethodSpec {
    name: "user.getLovedTracks",
    verb: Verb::Get,
    auth: Auth::ApiKey,
    write: false,
    paging: Paging::PageAndLimit,
    params: &[
        param("user", Requirement::Required),
        param("limit", Requirement::Optional),
        param("page", Requirement::Optional),
    ],
};

/// The items a user has tagged with a given tag.
pub const USER_GET_PERSONAL_TAGS: MethodSpec = MethodSpec {
    name: "user.getPersonalTags",
    verb: Verb::Get,
    auth: Auth::ApiKey,
    write: false,
    paging: Paging::PageAndLimit,
    params: &[
        param("user", Requirement::Required),
        param("tag", Requirement::Required),
        param("taggingtype", Requirement::Required),
        param("limit", Requirement::Optional),
        param("page", Requirement::Optional),
    ],
};

/// A user's scrobble history, newest first.
pub const USER_GET_RECENT_TRACKS: MethodSpec = MethodSpec {
    name: "user.getRecentTracks",
    verb: Verb::Get,
    auth: Auth::ApiKey,
    write: false,
    paging: Paging::PageAndLimit,
    params: &[
        param("limit", Requirement::Optional),
        param("user", Requirement::Required),
        param("page", Requirement::Optional),
        param("from", Requirement::Optional),
        param("extended", Requirement::Optional),
        param("to", Requirement::Optional),
    ],
};

/// A user's most played albums.
pub const USER_GET_TOP_ALBUMS: MethodSpec = MethodSpec {
    name: "user.getTopAlbums",
    verb: Verb::Get,
    auth: Auth::ApiKey,
    write: false,
    paging: Paging::PageAndLimit,
    params: &[
        param("user", Requirement::Required),
        param("period", Requirement::Optional),
        param("limit", Requirement::Optional),
        param("page", Requirement::Optional),
    ],
};

/// A user's most played artists.
pub const USER_GET_TOP_ARTISTS: MethodSpec = MethodSpec {
    name: "user.getTopArtists",
    verb: Verb::Get,
    auth: Auth::ApiKey,
    write: false,
    paging: Paging::PageAndLimit,
    params: &[
        param("user", Requirement::Required),
        param("period", Requirement::Optional),
        param("limit", Requirement::Optional),
        param("page", Requirement::Optional),
    ],
};

/// The tags a user applies most.
pub const USER_GET_TOP_TAGS: MethodSpec = MethodSpec {
    name: "user.getTopTags",
    verb: Verb::Get,
    auth: Auth::ApiKey,
    write: false,
    paging: Paging::LimitOnly,
    params: &[
        param("user", Requirement::Required),
        param("limit", Requirement::Optional),
    ],
};

/// A user's most played tracks.
pub const USER_GET_TOP_TRACKS: MethodSpec = MethodSpec {
    name: "user.getTopTracks",
    verb: Verb::Get,
    auth: Auth::ApiKey,
    write: false,
    paging: Paging::PageAndLimit,
    params: &[
        param("user", Requirement::Required),
        param("period", Requirement::Optional),
        param("limit", Requirement::Optional),
        param("page", Requirement::Optional),
    ],
};

/// A user's album chart for a date range.
pub const USER_GET_WEEKLY_ALBUM_CHART: MethodSpec = MethodSpec {
    name: "user.getWeeklyAlbumChart",
    verb: Verb::Get,
    auth: Auth::ApiKey,
    write: false,
    paging: Paging::None,
    params: &[
        param("user", Requirement::Required),
        param("from", Requirement::Optional),
        param("to", Requirement::Optional),
    ],
};

/// A user's artist chart for a date range.
pub const USER_GET_WEEKLY_ARTIST_CHART: MethodSpec = MethodSpec {
    name: "user.getWeeklyArtistChart",
    verb: Verb::Get,
    auth: Auth::ApiKey,
    write: false,
    paging: Paging::None,
    params: &[
        param("user", Requirement::Required),
        param("from", Requirement::Optional),
        param("to", Requirement::Optional),
    ],
};

/// The weekly chart ranges available for a user.
pub const USER_GET_WEEKLY_CHART_LIST: MethodSpec = MethodSpec {
    name: "user.getWeeklyChartList",
    verb: Verb::Get,
    auth: Auth::ApiKey,
    write: false,
    paging: Paging::None,
    params: &[param("user", Requirement::Required)],
};

/// A user's track chart for a date range.
pub const USER_GET_WEEKLY_TRACK_CHART: MethodSpec = MethodSpec {
    name: "user.getWeeklyTrackChart",
    verb: Verb::Get,
    auth: Auth::ApiKey,
    write: false,
    paging: Paging::None,
    params: &[
        param("user", Requirement::Required),
        param("from", Requirement::Optional),
        param("to", Requirement::Optional),
    ],
};

/// Every method, in the order of the official index.
pub const ALL: &[&MethodSpec] = &[
    &ALBUM_ADD_TAGS,
    &ALBUM_GET_INFO,
    &ALBUM_GET_TAGS,
    &ALBUM_GET_TOP_TAGS,
    &ALBUM_REMOVE_TAG,
    &ALBUM_SEARCH,
    &ARTIST_ADD_TAGS,
    &ARTIST_GET_CORRECTION,
    &ARTIST_GET_INFO,
    &ARTIST_GET_SIMILAR,
    &ARTIST_GET_TAGS,
    &ARTIST_GET_TOP_ALBUMS,
    &ARTIST_GET_TOP_TAGS,
    &ARTIST_GET_TOP_TRACKS,
    &ARTIST_REMOVE_TAG,
    &ARTIST_SEARCH,
    &AUTH_GET_MOBILE_SESSION,
    &AUTH_GET_SESSION,
    &AUTH_GET_TOKEN,
    &CHART_GET_TOP_ARTISTS,
    &CHART_GET_TOP_TAGS,
    &CHART_GET_TOP_TRACKS,
    &GEO_GET_TOP_ARTISTS,
    &GEO_GET_TOP_TRACKS,
    &LIBRARY_GET_ARTISTS,
    &TAG_GET_INFO,
    &TAG_GET_SIMILAR,
    &TAG_GET_TOP_ALBUMS,
    &TAG_GET_TOP_ARTISTS,
    &TAG_GET_TOP_TAGS,
    &TAG_GET_TOP_TRACKS,
    &TAG_GET_WEEKLY_CHART_LIST,
    &TRACK_ADD_TAGS,
    &TRACK_GET_CORRECTION,
    &TRACK_GET_INFO,
    &TRACK_GET_SIMILAR,
    &TRACK_GET_TAGS,
    &TRACK_GET_TOP_TAGS,
    &TRACK_LOVE,
    &TRACK_REMOVE_TAG,
    &TRACK_SCROBBLE,
    &TRACK_SEARCH,
    &TRACK_UNLOVE,
    &TRACK_UPDATE_NOW_PLAYING,
    &USER_GET_FRIENDS,
    &USER_GET_INFO,
    &USER_GET_LOVED_TRACKS,
    &USER_GET_PERSONAL_TAGS,
    &USER_GET_RECENT_TRACKS,
    &USER_GET_TOP_ALBUMS,
    &USER_GET_TOP_ARTISTS,
    &USER_GET_TOP_TAGS,
    &USER_GET_TOP_TRACKS,
    &USER_GET_WEEKLY_ALBUM_CHART,
    &USER_GET_WEEKLY_ARTIST_CHART,
    &USER_GET_WEEKLY_CHART_LIST,
    &USER_GET_WEEKLY_TRACK_CHART,
];

/// Finds a method by name, ignoring ASCII case, as the service does.
pub fn by_name(name: &str) -> Option<&'static MethodSpec> {
    ALL.iter()
        .copied()
        .find(|method| method.name.eq_ignore_ascii_case(name))
}
