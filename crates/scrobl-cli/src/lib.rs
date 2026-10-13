//! The `scrobl` command-line tool: a local SQLite copy of a Last.fm
//! listening history.
//!
//! Not affiliated with or endorsed by Last.fm.
//!
//! The tool is the binary. This library is the same code, split so that it
//! can be tested without a terminal:
//!
//! - [`store`] is the database: one row per scrobble, each with the row as
//!   Last.fm sent it.
//! - [`sync`] makes the database agree with the history at Last.fm.
//! - [`cli`] is the command line around the two.
//!
//! It reads through a [`scrobl::Client`], which cannot change an account.

pub mod cli;
mod error;
pub mod store;
pub mod sync;
pub mod time;

pub use error::Error;
