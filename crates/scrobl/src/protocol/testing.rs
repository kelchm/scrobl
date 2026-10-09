//! Method descriptions for tests. The real table lives in
//! [`methods`](super::methods); these are small, local and stable.
//!
//! provenance: synthetic. The names follow real methods; the parameter lists
//! are trimmed to what a test needs.

use super::{Auth, MethodSpec, Paging, ParamSpec, Requirement, Verb};

const fn param(name: &'static str, requirement: Requirement, indexed: bool) -> ParamSpec {
    ParamSpec {
        name,
        requirement,
        indexed,
    }
}

/// A key-only GET read.
pub(crate) static READ: MethodSpec = MethodSpec {
    name: "user.getInfo",
    verb: Verb::Get,
    auth: Auth::ApiKey,
    write: false,
    paging: Paging::None,
    params: &[
        param("user", Requirement::Required, false),
        param("mbid", Requirement::Conditional, false),
        param("limit", Requirement::Optional, false),
    ],
};

/// A signed GET read that needs no session.
pub(crate) static SIGNED_GET: MethodSpec = MethodSpec {
    name: "auth.getSession",
    verb: Verb::Get,
    auth: Auth::Signed,
    write: false,
    paging: Paging::None,
    params: &[param("token", Requirement::Required, false)],
};

/// A session POST write with batch parameters.
pub(crate) static SCROBBLE: MethodSpec = MethodSpec {
    name: "track.scrobble",
    verb: Verb::Post,
    auth: Auth::Session,
    write: true,
    paging: Paging::None,
    params: &[
        param("artist", Requirement::Required, true),
        param("track", Requirement::Required, true),
        param("timestamp", Requirement::Required, true),
        param("album", Requirement::Optional, true),
    ],
};

/// The write that is never retried.
pub(crate) static NOW_PLAYING: MethodSpec = MethodSpec {
    name: "track.updateNowPlaying",
    verb: Verb::Post,
    auth: Auth::Session,
    write: true,
    paging: Paging::None,
    params: &[
        param("artist", Requirement::Required, false),
        param("track", Requirement::Required, false),
    ],
};

/// A session POST write without batch parameters.
pub(crate) static LOVE: MethodSpec = MethodSpec {
    name: "track.love",
    verb: Verb::Post,
    auth: Auth::Session,
    write: true,
    paging: Paging::None,
    params: &[
        param("artist", Requirement::Required, false),
        param("track", Requirement::Required, false),
    ],
};

pub(crate) const SENTINEL_API_KEY: &str = "SENTINEL_API_KEY_0001";
pub(crate) const SENTINEL_API_SECRET: &str = "SENTINEL_API_SECRET_0001";
pub(crate) const SENTINEL_SESSION_KEY: &str = "SENTINEL_SESSION_KEY_0001";
pub(crate) const SENTINEL_TOKEN: &str = "SENTINEL_TOKEN_0001";
pub(crate) const SENTINEL_PASSWORD: &str = "SENTINEL_PASSWORD_0001";

/// Every sentinel secret.
pub(crate) const SENTINELS: [&str; 5] = [
    SENTINEL_API_KEY,
    SENTINEL_API_SECRET,
    SENTINEL_SESSION_KEY,
    SENTINEL_TOKEN,
    SENTINEL_PASSWORD,
];

/// Credentials made of the sentinels, with secret and session.
pub(crate) fn credentials() -> crate::protocol::Credentials {
    use crate::{ApiKey, ApiSecret, SessionKey};
    crate::protocol::Credentials::new(ApiKey::new(SENTINEL_API_KEY))
        .with_secret(ApiSecret::new(SENTINEL_API_SECRET))
        .with_session(SessionKey::new(SENTINEL_SESSION_KEY))
}

/// Fails the test if any sentinel appears in `text`.
#[track_caller]
pub(crate) fn assert_no_sentinel(text: &str) {
    for sentinel in SENTINELS {
        assert!(!text.contains(sentinel), "`{sentinel}` leaked into: {text}");
    }
}
