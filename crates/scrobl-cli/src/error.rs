//! The tool's one error type.

use std::fmt;

/// Why a command failed.
///
/// `Display` is one line that is safe to print: it never carries a
/// credential, a URL or text the network chose.
#[derive(Debug)]
#[non_exhaustive]
pub enum Error {
    /// The command line or the environment was wrong. Nothing was read.
    Usage(String),
    /// A call to Last.fm failed, or a page of history broke a rule.
    Lastfm(scrobl::Error),
    /// SQLite refused something.
    Sqlite(rusqlite::Error),
    /// The file is not a database this version can use.
    Database(&'static str),
    /// The database holds another user's history.
    OtherUser,
    /// Last.fm answered a question in a way that cannot be acted on.
    Service(&'static str),
}

impl Error {
    /// The process exit code for this error: 2 for a usage error, 1 for
    /// anything else.
    pub fn exit_code(&self) -> u8 {
        match self {
            Self::Usage(_) => 2,
            _ => 1,
        }
    }
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Usage(message) => f.write_str(message),
            Self::Lastfm(error) => write!(f, "Last.fm: {error}"),
            Self::Sqlite(error) => write!(f, "database: {error}"),
            Self::Database(message) | Self::Service(message) => f.write_str(message),
            Self::OtherUser => {
                f.write_str("the database holds another user's history; use one file per user")
            }
        }
    }
}

impl std::error::Error for Error {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Lastfm(error) => Some(error),
            Self::Sqlite(error) => Some(error),
            _ => None,
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

#[cfg(test)]
mod tests {
    use std::error::Error as _;

    use super::Error;

    #[test]
    fn only_a_usage_error_exits_2() {
        assert_eq!(Error::Usage("wrong".to_owned()).exit_code(), 2);
        assert_eq!(Error::Database("wrong").exit_code(), 1);
        assert_eq!(Error::Service("wrong").exit_code(), 1);
        assert_eq!(Error::from(rusqlite::Error::InvalidQuery).exit_code(), 1);
    }

    #[test]
    fn display_says_where_the_failure_was() {
        assert_eq!(
            Error::Usage("no command".to_owned()).to_string(),
            "no command"
        );
        assert_eq!(Error::Database("not ours").to_string(), "not ours");
        assert_eq!(Error::Service("no answer").to_string(), "no answer");
        let sqlite = Error::from(rusqlite::Error::InvalidQuery);
        assert!(sqlite.to_string().starts_with("database: "));
    }

    #[test]
    fn the_cause_is_kept_for_errors_that_have_one() {
        assert!(
            Error::from(rusqlite::Error::InvalidQuery)
                .source()
                .is_some()
        );
        assert!(Error::Database("not ours").source().is_none());
        assert!(Error::OtherUser.source().is_none());
    }
}
