//! The tool's one error type.

use std::fmt;

/// Why a command failed.
///
/// `Display` is one line that is safe to print: it never carries a
/// credential, a URL or text the network chose.
#[derive(Debug)]
pub enum Error {
    /// The tool will not go on: the command line is wrong, or the file is
    /// not one it can use. The text is fixed, so it repeats nothing it was
    /// given.
    Refused(&'static str),
    /// A call to Last.fm failed, or a page of history broke a rule.
    Lastfm(scrobl::Error),
    /// SQLite refused something.
    Sqlite(rusqlite::Error),
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Refused(message) => f.write_str(message),
            Self::Lastfm(error) => write!(f, "Last.fm: {error}"),
            Self::Sqlite(error) => write!(f, "database: {error}"),
        }
    }
}

impl From<scrobl::Error> for Error {
    fn from(error: scrobl::Error) -> Self {
        Self::Lastfm(error)
    }
}

impl From<rusqlite::Error> for Error {
    fn from(error: rusqlite::Error) -> Self {
        Self::Sqlite(error)
    }
}
