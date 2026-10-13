//! The database: one SQLite file holding one user's history.
//!
//! - `scrobbles`: one row per scrobble, and the table to query. `raw` is the
//!   row as Last.fm sent it, as JSON. Rows are never merged: two scrobbles
//!   of the same track in the same second are two rows.
//! - `removed`: scrobbles the file held that a later reading of the same
//!   time did not, each with when that was noticed.
//! - `loved`: the tracks the user has loved, as they stood at the last
//!   sync, in the order Last.fm lists them.
//! - `meta`: whose history this is, and when it and the loved tracks were
//!   last read.
//!
//! Nothing in the file says how the history was read. [`Store::replace`]
//! makes it hold, for a span of time, exactly what one checked reading of
//! that span held. Writing the same reading again changes nothing, so spans
//! may be written twice and in any order.

use std::collections::HashMap;
use std::path::Path;

use rusqlite::{Connection, OptionalExtension, TransactionBehavior, params};
use scrobl::Response;
use scrobl::history::{ScanPage, ScanSummary};
use scrobl::model::LovedTracksPage;

use crate::Error;

/// `PRAGMA application_id`, which marks the file as this tool's. The bytes
/// spell "scbl".
const APPLICATION_ID: i64 = 0x7363_626c;

/// `PRAGMA user_version`, which says the file has the layout below.
const SCHEMA_VERSION: i64 = 4;

const SCHEMA: &str = "
CREATE TABLE meta (
    key   TEXT PRIMARY KEY,
    value TEXT NOT NULL
) STRICT;

CREATE TABLE scrobbles (
    id          INTEGER PRIMARY KEY,
    timestamp   INTEGER NOT NULL,
    artist      TEXT NOT NULL,
    artist_mbid TEXT,
    track       TEXT NOT NULL,
    track_mbid  TEXT,
    album       TEXT,
    album_mbid  TEXT,
    raw         TEXT NOT NULL
) STRICT;

CREATE INDEX scrobbles_timestamp ON scrobbles (timestamp);

CREATE TABLE removed (
    id          INTEGER PRIMARY KEY,
    removed_at  INTEGER NOT NULL,
    timestamp   INTEGER NOT NULL,
    artist      TEXT NOT NULL,
    artist_mbid TEXT,
    track       TEXT NOT NULL,
    track_mbid  TEXT,
    album       TEXT,
    album_mbid  TEXT,
    raw         TEXT NOT NULL
) STRICT;

CREATE TABLE loved (
    id          INTEGER PRIMARY KEY,
    loved_at    INTEGER NOT NULL,
    artist      TEXT NOT NULL,
    artist_mbid TEXT,
    track       TEXT NOT NULL,
    track_mbid  TEXT,
    raw         TEXT NOT NULL
) STRICT;
";

/// How a reading of a span differed from what the file held for it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Change {
    /// Scrobbles the reading has and the file had not.
    pub added: u64,
    /// Scrobbles the file had and the reading has not. They are now in
    /// `removed`.
    pub removed: u64,
}

/// An open database.
#[derive(Debug)]
pub struct Store {
    connection: Connection,
}

impl Store {
    /// Opens the database at `path`. A file with no tables is new and is set
    /// up. Any other file must carry both marks as this version writes them,
    /// or it is refused and left as it was: nothing is upgraded, because the
    /// history can be read again.
    pub fn open(path: impl AsRef<Path>) -> Result<Self, Error> {
        let mut connection = Connection::open(path)?;
        let tables: i64 =
            connection.query_row("SELECT count(*) FROM sqlite_schema", [], |row| row.get(0))?;
        let application: i64 =
            connection.pragma_query_value(None, "application_id", |row| row.get(0))?;
        let version: i64 = connection.pragma_query_value(None, "user_version", |row| row.get(0))?;
        if tables == 0 {
            let transaction = connection.transaction()?;
            transaction.execute_batch(SCHEMA)?;
            transaction.pragma_update(None, "application_id", APPLICATION_ID)?;
            transaction.pragma_update(None, "user_version", SCHEMA_VERSION)?;
            transaction.commit()?;
        } else if (application, version) != (APPLICATION_ID, SCHEMA_VERSION) {
            return Err(Error::Refused(
                "the file was written by another version of this tool, or by another program; \
                 sync into a new file",
            ));
        }
        Ok(Self { connection })
    }

    /// The user whose history the database holds, if one has been set.
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
    pub fn bind_user(&mut self, user: &str) -> Result<(), Error> {
        match self.user()? {
            Some(bound) if bound.eq_ignore_ascii_case(user) => Ok(()),
            Some(_) => Err(Error::Refused(
                "the database holds another user's history; use one file per user",
            )),
            None => {
                self.connection
                    .execute("INSERT INTO meta (key, value) VALUES ('user', ?1)", [user])?;
                Ok(())
            }
        }
    }

    /// The number of scrobbles stored with a timestamp in `[from, to)`.
    pub fn count(&self, from: u64, to: u64) -> Result<u64, Error> {
        Ok(self.connection.query_row(
            "SELECT count(*) FROM scrobbles WHERE timestamp >= ?1 AND timestamp < ?2",
            [from, to],
            |row| row.get(0),
        )?)
    }

    /// Makes the file hold, for the span of one scan, exactly the scrobbles
    /// that scan read, in one transaction.
    ///
    /// `summary` is what [`Scan::finish`](scrobl::client::Scan::finish)
    /// returned for `pages`: the proof that every page passed the scan's
    /// checks, and the span they cover. Nothing here checks the pages again.
    ///
    /// The reading is compared with what the file held for the span by the
    /// time, artist and track of each row, counting rows that share all
    /// three: the ids and the album are what Last.fm says about a scrobble,
    /// not which scrobble it is. A scrobble only the file had is copied to
    /// `removed` with `now` as the time it was noticed. Then the span's rows
    /// are replaced by the reading's, oldest first, so that
    /// `ORDER BY timestamp, id` is the order of listening.
    pub fn replace(
        &mut self,
        summary: &ScanSummary,
        pages: &[ScanPage],
        now: u64,
    ) -> Result<Change, Error> {
        let span = summary.window();
        let from = span.from().unwrap_or(0);
        let Some(to) = span.to() else {
            return Err(Error::Refused(
                "a reading without an upper bound cannot be stored",
            ));
        };
        // Last.fm lists the newest first.
        let read = || {
            pages
                .iter()
                .rev()
                .flat_map(|page| page.scrobbles().iter().rev())
        };

        let transaction = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        let held: Vec<(i64, u64, String, String)> = transaction
            .prepare(
                "SELECT id, timestamp, artist, track FROM scrobbles
                 WHERE timestamp >= ?1 AND timestamp < ?2 ORDER BY id",
            )?
            .query_map([from, to], |row| {
                Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?))
            })?
            .collect::<Result<_, _>>()?;
        // How many of each scrobble the reading has that no row of the
        // file has answered for yet.
        let mut unmatched: HashMap<(u64, &str, &str), u64> = HashMap::new();
        for scrobble in read() {
            let row = (
                scrobble.timestamp(),
                scrobble.artist().name(),
                scrobble.track().name(),
            );
            *unmatched.entry(row).or_default() += 1;
        }
        let mut change = Change::default();
        {
            let mut set_aside = transaction.prepare(
                "INSERT INTO removed (removed_at, timestamp, artist, artist_mbid, track,
                     track_mbid, album, album_mbid, raw)
                 SELECT ?2, timestamp, artist, artist_mbid, track, track_mbid, album,
                     album_mbid, raw
                 FROM scrobbles WHERE id = ?1",
            )?;
            for (id, timestamp, artist, track) in &held {
                match unmatched.get_mut(&(*timestamp, artist.as_str(), track.as_str())) {
                    Some(left) if *left > 0 => *left -= 1,
                    _ => {
                        set_aside.execute(params![id, now])?;
                        change.removed += 1;
                    }
                }
            }
        }
        change.added = unmatched.values().sum();

        transaction.execute(
            "DELETE FROM scrobbles WHERE timestamp >= ?1 AND timestamp < ?2",
            [from, to],
        )?;
        {
            let mut insert = transaction.prepare(
                "INSERT INTO scrobbles (timestamp, artist, artist_mbid, track, track_mbid,
                     album, album_mbid, raw)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
            )?;
            for scrobble in read() {
                insert.execute(params![
                    scrobble.timestamp(),
                    scrobble.artist().name(),
                    scrobble.artist().mbid(),
                    scrobble.track().name(),
                    scrobble.track().mbid(),
                    scrobble.album().title(),
                    scrobble.album().mbid(),
                    scrobble.json().to_string(),
                ])?;
            }
        }
        transaction.commit()?;
        Ok(change)
    }

    /// Replaces the loved tracks with `pages`, page 1 first, in one
    /// transaction, and keeps the time of the read in `meta` as
    /// `loved_read_at`.
    pub(crate) fn replace_loved(
        &mut self,
        pages: &[Response<LovedTracksPage>],
        read_at: u64,
    ) -> Result<(), Error> {
        let transaction = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        transaction.execute("DELETE FROM loved", [])?;
        {
            let mut insert = transaction.prepare(
                "INSERT INTO loved (loved_at, artist, artist_mbid, track, track_mbid, raw)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            )?;
            for loved in pages.iter().flat_map(|page| page.tracks()) {
                insert.execute(params![
                    loved.loved_at(),
                    loved.artist().name(),
                    loved.artist().mbid(),
                    loved.track().name(),
                    loved.track().mbid(),
                    loved.json().to_string(),
                ])?;
            }
        }
        note(&transaction, "loved_read_at", read_at)?;
        transaction.commit()?;
        Ok(())
    }

    /// Notes in `meta`, as `history_read_at`, that a sync finished reading
    /// the history at `read_at`. Nothing in the tool goes by it: it is there
    /// for whoever asks how fresh the file is.
    pub(crate) fn note_history_read(&mut self, read_at: u64) -> Result<(), Error> {
        note(&self.connection, "history_read_at", read_at)
    }

    /// The connection, for reading the tables directly.
    pub fn connection(&self) -> &Connection {
        &self.connection
    }
}

/// Sets a time in `meta`.
fn note(connection: &Connection, key: &str, seconds: u64) -> Result<(), Error> {
    connection.execute(
        "INSERT INTO meta (key, value) VALUES (?1, ?2)
         ON CONFLICT (key) DO UPDATE SET value = excluded.value",
        [key, &seconds.to_string()],
    )?;
    Ok(())
}
