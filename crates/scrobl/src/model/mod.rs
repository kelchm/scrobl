//! Typed views of responses, one module per API package.
//!
//! A model is decoded from a [`Raw`](crate::protocol::Raw) on request and
//! never replaces it. Fields are private; each item keeps its own decoded
//! JSON, so unknown fields and the difference between a missing and an empty
//! field stay reachable. A malformed value is an error naming the field, and
//! is never dropped or defaulted.
//!
//! The `Debug` output of a model leaves out the text of the response.

mod user;

pub use user::{Album, Artist, NowPlaying, PageAttr, RecentTracksPage, Scrobble, Track};
