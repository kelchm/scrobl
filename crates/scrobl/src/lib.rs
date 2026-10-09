//! The full Last.fm API in async Rust: every method callable, history read
//! and checked page by page, credentials kept out of logs. Unofficial.
//!
//! Early development. Not affiliated with or endorsed by Last.fm.
//!
//! What works today: a raw, signed call to any of the 57 methods, and a
//! typed, checked read of a user's scrobble history. Not built yet: typed
//! models for the other methods, the authentication flows and typed
//! scrobbling. Only the history read has been run against the live service.
//!
//! The crate is built in layers:
//!
//! - [`protocol`] is I/O-free. It builds signed HTTP requests and decodes
//!   responses, and never touches a socket, a clock or a runtime.
//! - [`model`] holds typed views decoded from a response, and [`history`]
//!   reads one window of a user's scrobbles completely and checks every
//!   page. Both are I/O-free too.
//! - The async client, `Client`, behind the default `client` feature,
//!   executes those requests with `reqwest` on the caller's Tokio runtime.
//!   It paces requests, retries reads, and bounds time and size.
//!   [`Response`] pairs a typed value with the exact response it came from.

// Labels the items behind `client` on docs.rs, which builds with nightly.
#![cfg_attr(docsrs, feature(doc_cfg))]

#[cfg(feature = "client")]
pub mod client;
mod de;
mod error;
pub mod history;
pub mod model;
pub mod protocol;
mod response;
mod secret;

#[cfg(feature = "client")]
pub use client::{Client, ClientBuilder};
pub use error::{ApiErrorCode, Delivery, Error, ErrorKind, Retry};
pub use response::Response;
pub use secret::{ApiKey, ApiSecret, SessionKey};
