//! The database: one SQLite file holding one user's history.
//!
//! The history is stored as *windows*. A window is a span of time
//! `[from, to)` that was read from Last.fm completely, with every page
//! checked by the library's scan. Three tables hold it:
//!
//! - `windows`: each span, with the total Last.fm reported for it.
//! - `pages`: the body of every response of that read, byte for byte. These
//!   are the record. Everything else can be rebuilt from them.
//! - `scrobbles`: one row per scrobble, decoded from the pages. This is the
//!   table to query. Rows are never merged: two scrobbles of the same track
//!   in the same second are two rows.
//!
//! A window is written in one transaction, and only from the summary of a
//! scan that finished, so the file holds whole windows or nothing of them.
//! Windows never overlap.

use std::path::Path;

use rusqlite::{Connection, OptionalExtension, TransactionBehavior, params};
use scrobl::history::{ScanPage, ScanSummary};

use crate::Error;

/// Marks the file as this tool's: `PRAGMA application_id`. The bytes spell
/// "scbl".
const APPLICATION_ID: i64 = 0x7363_626c;

/// The layout below: `PRAGMA user_version`.
const SCHEMA_VERSION: i64 = 1;

const SCHEMA: &str = "
CREATE TABLE meta (
    key   TEXT PRIMARY KEY,
    value TEXT NOT NULL
) STRICT;

CREATE TABLE windows (
    id        INTEGER PRIMARY KEY,
    from_ts   INTEGER NOT NULL,
    to_ts     INTEGER NOT NULL,
    total     INTEGER NOT NULL,
    pages     INTEGER NOT NULL,
    page_size INTEGER NOT NULL,
    read_at   INTEGER NOT NULL,
    CHECK (from_ts >= 0 AND from_ts < to_ts)
) STRICT;

CREATE TABLE pages (
    window_id INTEGER NOT NULL REFERENCES windows (id),
    page      INTEGER NOT NULL,
    body      BLOB NOT NULL,
    PRIMARY KEY (window_id, page)
) STRICT;

CREATE TABLE scrobbles (
    id          INTEGER PRIMARY KEY,
    window_id   INTEGER NOT NULL REFERENCES windows (id),
    page        INTEGER NOT NULL,
    position    INTEGER NOT NULL,
    timestamp   INTEGER NOT NULL,
    artist      TEXT NOT NULL,
    artist_mbid TEXT,
    track       TEXT NOT NULL,
    track_mbid  TEXT,
    album       TEXT,
    album_mbid  TEXT,
    loved       INTEGER,
    UNIQUE (window_id, page, position)
) STRICT;

CREATE INDEX scrobbles_timestamp ON scrobbles (timestamp);
";

/// An open database.
#[derive(Debug)]
pub struct Store {
    connection: Connection,
}

impl Store {
    /// Opens the database at `path`, creating it if the file is missing or
    /// empty.
    ///
    /// # Errors
    ///
    /// [`Error::Database`] when the file is some other SQLite database or was
    /// written by a newer version of this tool, and [`Error::Sqlite`] when it
    /// cannot be opened or is not a database at all.
    pub fn open(path: impl AsRef<Path>) -> Result<Self, Error> {
        let mut connection = Connection::open(path)?;
        connection.busy_timeout(std::time::Duration::from_secs(5))?;
        connection.pragma_update(None, "foreign_keys", true)?;

        let objects: i64 =
            connection.query_row("SELECT count(*) FROM sqlite_schema", [], |row| row.get(0))?;
        if objects == 0 {
            let transaction = connection.transaction()?;
            transaction.execute_batch(SCHEMA)?;
            transaction.pragma_update(None, "application_id", APPLICATION_ID)?;
            transaction.pragma_update(None, "user_version", SCHEMA_VERSION)?;
            transaction.commit()?;
            return Ok(Self { connection });
        }

        let application: i64 =
            connection.pragma_query_value(None, "application_id", |row| row.get(0))?;
        if application != APPLICATION_ID {
            return Err(Error::Database(
                "the file is a database, but not one this tool made",
            ));
        }
        let version: i64 = connection.pragma_query_value(None, "user_version", |row| row.get(0))?;
        if version != SCHEMA_VERSION {
            return Err(Error::Database(
                "the database was written by another version of this tool",
            ));
        }
        Ok(Self { connection })
    }

    /// The user whose history the database holds, if one has been set.
    ///
    /// # Errors
    ///
    /// [`Error::Sqlite`].
    pub fn user(&self) -> Result<Option<String>, Error> {
        Ok(self
            .connection
            .query_row("SELECT value FROM meta WHERE key = 'user'", [], |row| {
                row.get(0)
            })
            .optional()?)
    }

    /// Ties the database to `user`. The first call records the name; a later
    /// one checks it, ignoring ASCII case as Last.fm does.
    ///
    /// # Errors
    ///
    /// [`Error::OtherUser`] when the database already holds another user's
    /// history.
    pub fn bind_user(&mut self, user: &str) -> Result<(), Error> {
        match self.user()? {
            Some(bound) if bound.eq_ignore_ascii_case(user) => Ok(()),
            Some(_) => Err(Error::OtherUser),
            None => {
                self.connection
                    .execute("INSERT INTO meta (key, value) VALUES ('user', ?1)", [user])?;
                Ok(())
            }
        }
    }

    /// The spans of `[0, cutoff)` that no window covers, newest first, each
    /// as `(from, to)` with `from` inclusive and `to` exclusive.
    ///
    /// # Errors
    ///
    /// [`Error::Sqlite`].
    pub fn gaps(&self, cutoff: u64) -> Result<Vec<(u64, u64)>, Error> {
        let mut statement = self
            .connection
            .prepare("SELECT from_ts, to_ts FROM windows ORDER BY from_ts")?;
        let windows = statement.query_map([], |row| Ok((row.get::<_, u64>(0)?, row.get(1)?)))?;

        let mut gaps = Vec::new();
        let mut covered_to = 0;
        for window in windows {
            let (from, to): (u64, u64) = window?;
            if from > covered_to && covered_to < cutoff {
                gaps.push((covered_to, from.min(cutoff)));
            }
            covered_to = covered_to.max(to);
        }
        if covered_to < cutoff {
            gaps.push((covered_to, cutoff));
        }
        gaps.reverse();
        Ok(gaps)
    }

    /// Stores one window: its pages as received and the scrobbles on them.
    ///
    /// `summary` is what [`Scan::finish`](scrobl::client::Scan::finish)
    /// returned for `pages`, which is the proof that every page passed the
    /// scan's checks. A window with no lower bound is stored from 0. All of
    /// it is written in one transaction.
    ///
    /// # Errors
    ///
    /// [`Error::Database`] when the window has no upper bound, when it
    /// overlaps a window already stored (another run got there first), or
    /// when `pages` are not the pages the summary describes. Nothing is
    /// written in any of those cases.
    pub fn commit_window(
        &mut self,
        summary: &ScanSummary,
        pages: &[ScanPage],
        read_at: u64,
    ) -> Result<(), Error> {
        let window = summary.window();
        let from = window.from().unwrap_or(0);
        let Some(to) = window.to() else {
            return Err(Error::Database(
                "a window without an upper bound cannot be stored",
            ));
        };
        let page_size = pages.first().map_or(0, |page| page.attr().per_page());
        let scrobbles: usize = pages.iter().map(|page| page.scrobbles().len()).sum();
        if pages.len() as u64 != u64::from(summary.pages()) || scrobbles as u64 != summary.total() {
            return Err(Error::Database(
                "the pages are not the ones the scan summary describes",
            ));
        }

        let transaction = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        let overlaps: bool = transaction.query_row(
            "SELECT EXISTS (SELECT 1 FROM windows WHERE from_ts < ?2 AND to_ts > ?1)",
            params![from, to],
            |row| row.get(0),
        )?;
        if overlaps {
            return Err(Error::Database(
                "the window overlaps one already stored; is another sync running?",
            ));
        }

        transaction.execute(
            "INSERT INTO windows (from_ts, to_ts, total, pages, page_size, read_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            params![
                from,
                to,
                summary.total(),
                summary.pages(),
                page_size,
                read_at
            ],
        )?;
        let window_id = transaction.last_insert_rowid();
        {
            let mut insert_page = transaction
                .prepare("INSERT INTO pages (window_id, page, body) VALUES (?1, ?2, ?3)")?;
            let mut insert_scrobble = transaction.prepare(
                "INSERT INTO scrobbles (window_id, page, position, timestamp, artist,
                     artist_mbid, track, track_mbid, album, album_mbid, loved)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)",
            )?;
            for page in pages {
                insert_page.execute(params![window_id, page.number(), page.raw().body()])?;
                for (position, scrobble) in page.scrobbles().iter().enumerate() {
                    insert_scrobble.execute(params![
                        window_id,
                        page.number(),
                        position,
                        scrobble.timestamp(),
                        scrobble.artist().name(),
                        scrobble.artist().mbid(),
                        scrobble.track().name(),
                        scrobble.track().mbid(),
                        scrobble.album().title(),
                        scrobble.album().mbid(),
                        scrobble.loved(),
                    ])?;
                }
            }
        }
        transaction.commit()?;
        Ok(())
    }

    /// The number of scrobbles stored with a timestamp before `cutoff`.
    ///
    /// # Errors
    ///
    /// [`Error::Sqlite`].
    pub fn scrobbles_before(&self, cutoff: u64) -> Result<u64, Error> {
        Ok(self.connection.query_row(
            "SELECT count(*) FROM scrobbles WHERE timestamp < ?1",
            [cutoff],
            |row| row.get(0),
        )?)
    }

    /// The connection, for reading the tables directly.
    pub fn connection(&self) -> &Connection {
        &self.connection
    }
}
