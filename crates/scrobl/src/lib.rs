//! The full Last.fm API in Rust: reads, authentication and scrobbling.
//!
//! Early development. Not affiliated with or endorsed by Last.fm.
//!
//! The crate has two layers:
//!
//! - [`protocol`] is I/O-free. It builds signed HTTP requests and decodes
//!   responses, and never touches a socket, a clock or a runtime.
//! - The async client, behind the default `client` feature, executes those
//!   requests with `reqwest` on the caller's Tokio runtime.

mod error;
pub mod protocol;
mod secret;

pub use error::{ApiErrorCode, Delivery, Error, ErrorKind, Retry};
pub use secret::{ApiKey, ApiSecret, SessionKey};
