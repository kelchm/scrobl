//! The full Last.fm API in Rust: reads, authentication and scrobbling.
//!
//! Early development. Not affiliated with or endorsed by Last.fm.
//!
//! The crate is built in layers:
//!
//! - [`protocol`] is I/O-free. It builds signed HTTP requests and decodes
//!   responses, and never touches a socket, a clock or a runtime.
//! - [`model`] holds typed views decoded from a response, and [`history`]
//!   reads one window of a user's scrobbles completely and checks every
//!   page. Both are I/O-free too.
//! - The async client, behind the default `client` feature, executes those
//!   requests with `reqwest` on the caller's Tokio runtime.

mod de;
mod error;
pub mod history;
pub mod model;
pub mod protocol;
mod secret;

pub use error::{ApiErrorCode, Delivery, Error, ErrorKind, Retry};
pub use secret::{ApiKey, ApiSecret, SessionKey};
