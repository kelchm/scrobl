//! The `scrobl` command-line tool: a local SQLite copy of a Last.fm
//! listening history.
//!
//! Not affiliated with or endorsed by Last.fm.
//!
//! The tool is the binary. This library is the same code, split so that it
//! can be tested without a terminal:
//!
//! - [`store`] is the database: the exact pages Last.fm sent, and the
//!   scrobbles derived from them.
//! - [`sync`] reads the history that the database does not hold yet.
//! - [`cli`] is the command line around the two.
//!
//! It reads through a [`scrobl::Client`], which cannot change an account.

pub mod cli;
mod error;
pub mod store;
pub mod sync;
pub mod time;

pub use error::Error;
