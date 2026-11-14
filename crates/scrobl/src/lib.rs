//! The full Last.fm API in async Rust: every method callable, history read
//! and checked page by page, credentials kept out of logs. Unofficial.
//!
//! Early development. Not affiliated with or endorsed by Last.fm.
//!
//! What works today: a raw, signed call to any of the 57 methods, the ten
//! that change an account only through a `Writer`, and a typed call for
//! every method that reads: a checked read of a user's scrobble history,
//! everything else about a user, an artist, an album, a track and a tag,
//! the charts of a country and of the whole service, and the searches. Not
//! built yet: the authentication flows and typed scrobbling.
//!
//! A typed call is who or what, which question, then how to run it:
//!
#![cfg_attr(feature = "client", doc = "```no_run")]
#![cfg_attr(not(feature = "client"), doc = "```ignore")]
//! use scrobl::client::Period;
//! use scrobl::{ApiKey, Client};
//!
//! # async fn demo() -> Result<(), scrobl::Error> {
//! let client = Client::builder(ApiKey::new("your-api-key"))
//!     .user_agent("example/0.1 (+https://example.org)")
//!     .build()?;
//!
//! // One page of a user's most played artists.
//! let page = client
//!     .user("rj")
//!     .top_artists()
//!     .period(Period::SevenDays)
//!     .limit(10)
//!     .send()
//!     .await?;
//! for (place, row) in page.ranked() {
//!     println!("{place}. {} ({} plays)", row.artist().name(), row.playcount());
//! }
//!
//! // Every row of a list, page after page.
//! let mut rows = client.tag("indie").top_tracks().all();
//! while let Some(track) = rows.next().await? {
//!     println!("{} by {}", track.name(), track.artist().name());
//! }
//! # Ok(())
//! # }
//! ```
//!
//! Each call has the name of its Last.fm method as a search alias: looking
//! for `user.getTopArtists` in these pages finds `top_artists`.
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
//!   It paces requests, retries reads, and bounds time and size. A `Client`
//!   never changes an account; a `Writer`, built on purpose, is the only
//!   thing that scrobbles, loves or tags.
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
pub use client::{Client, ClientBuilder, Writer};
pub use error::{ApiErrorCode, Delivery, Error, ErrorKind, Retry};
pub use response::Response;
pub use secret::{ApiKey, ApiSecret, AuthToken, Password, SessionKey};
