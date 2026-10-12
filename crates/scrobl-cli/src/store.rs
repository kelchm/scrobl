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
//!
//! A window is *sealed* or it is not. A sealed window is final: it is read
//! once and never replaced. The windows that are not sealed are the newest
//! ones, the *tail*, where scrobbles are still arriving. Every sync reads
//! the tail again and replaces it, in one transaction, so the file holds
//! the tail as it was read before or as it was read now. When the new
//! reading has lost a scrobble the old one had, the old windows and their
//! pages are kept in `superseded_windows` and `superseded_pages`: the
//! scrobbles table follows Last.fm, and nothing it once said is thrown away.
//!
//! The tracks the user has loved are kept beside the history, as they stood
//! at the last sync: `loved_pages` holds the responses and `loved` the rows
//! decoded from them. Each read replaces the one before.

use std::collections::HashMap;
use std::path::Path;

use rusqlite::{Connection, OpenFlags, OptionalExtension, TransactionBehavior, params};
use scrobl::Response;
use scrobl::history::{ScanPage, ScanSummary};
use scrobl::model::LovedTracksPage;

use crate::Error;

/// Marks the file as this tool's: `PRAGMA application_id`. The bytes spell
/// "scbl".
const APPLICATION_ID: i64 = 0x7363_626c;

/// The layout below: `PRAGMA user_version`. Version 1 had no tables for the
/// loved tracks and, in its first form, a `loved` column on `scrobbles`.
/// Version 2 had no windows that were not sealed, and so no column to say
/// which they are and nowhere to keep one that was replaced.
const SCHEMA_VERSION: i64 = 3;

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
    sealed    INTEGER NOT NULL DEFAULT 1 CHECK (sealed IN (0, 1)),
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
    UNIQUE (window_id, page, position)
) STRICT;

CREATE INDEX scrobbles_timestamp ON scrobbles (timestamp);
";

/// The tables of the loved tracks, which version 2 added.
const LOVED_SCHEMA: &str = "
CREATE TABLE loved_pages (
    page INTEGER PRIMARY KEY,
    body BLOB NOT NULL
) STRICT;

CREATE TABLE loved (
    id          INTEGER PRIMARY KEY,
    page        INTEGER NOT NULL REFERENCES loved_pages (page),
    position    INTEGER NOT NULL,
    loved_at    INTEGER NOT NULL,
    artist      TEXT NOT NULL,
    artist_mbid TEXT,
    track       TEXT NOT NULL,
    track_mbid  TEXT,
    UNIQUE (page, position)
) STRICT;
";

/// Where a window of the tail goes when a later reading has lost a scrobble
/// it had, which version 3 added. The pages are the record, as they are for
/// a window in use.
const SUPERSEDED_SCHEMA: &str = "
CREATE TABLE superseded_windows (
    id            INTEGER PRIMARY KEY,
    from_ts       INTEGER NOT NULL,
    to_ts         INTEGER NOT NULL,
    total         INTEGER NOT NULL,
    pages         INTEGER NOT NULL,
    page_size     INTEGER NOT NULL,
    read_at       INTEGER NOT NULL,
    superseded_at INTEGER NOT NULL
) STRICT;

CREATE TABLE superseded_pages (
    window_id INTEGER NOT NULL REFERENCES superseded_windows (id),
    page      INTEGER NOT NULL,
    body      BLOB NOT NULL,
    PRIMARY KEY (window_id, page)
) STRICT;
";

/// Brings a version 2 file to version 3, in one transaction: every window
/// it holds was read once and is sealed.
fn upgrade_from_2(connection: &mut Connection) -> Result<(), Error> {
    let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
    transaction.execute_batch(
        "ALTER TABLE windows
             ADD COLUMN sealed INTEGER NOT NULL DEFAULT 1 CHECK (sealed IN (0, 1))",
    )?;
    transaction.execute_batch(SUPERSEDED_SCHEMA)?;
    transaction.pragma_update(None, "user_version", 3)?;
    transaction.commit()?;
    Ok(())
}

/// Brings a version 1 file to version 2, in one transaction: the tables of
/// the loved tracks are added, and the `loved` column, which the first
/// version 1 files have on `scrobbles`, is dropped. What it held is still in
/// the pages.
fn upgrade_from_1(connection: &mut Connection) -> Result<(), Error> {
    let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
    let has_loved: bool = transaction.query_row(
        "SELECT EXISTS (SELECT 1 FROM pragma_table_info('scrobbles') WHERE name = 'loved')",
        [],
        |row| row.get(0),
    )?;
    if has_loved {
        transaction.execute_batch("ALTER TABLE scrobbles DROP COLUMN loved")?;
    }
    transaction.execute_batch(LOVED_SCHEMA)?;
    transaction.pragma_update(None, "user_version", 2)?;
    transaction.commit()?;
    Ok(())
}

/// The windows that are not sealed: the newest part of the history, which
/// the next sync reads again.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Tail {
    /// The inclusive lower bound of the oldest.
    pub from: u64,
    /// The exclusive upper bound of the newest.
    pub to: u64,
}

/// One window of a new reading of the tail.
///
/// `Debug` shows the window's bounds, its total and whether it is sealed,
/// and nothing of the pages or of the user, which are text the network
/// chose.
#[derive(Clone, Copy)]
pub struct TailWindow<'a> {
    /// What the scan returned when it finished.
    pub summary: &'a ScanSummary,
    /// The pages that summary came with.
    pub pages: &'a [ScanPage],
    /// Whether the window is final from now on.
    pub sealed: bool,
}

impl std::fmt::Debug for TailWindow<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let window = self.summary.window();
        f.debug_struct("TailWindow")
            .field("from", &window.from())
            .field("to", &window.to())
            .field("total", &self.summary.total())
            .field("sealed", &self.sealed)
            .finish()
    }
}

/// How a new reading of the tail differs from the one it replaced, over the
/// time both cover.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct TailChange {
    /// Scrobbles the old reading had and the new one has not. When there
    /// are any, the old windows were kept as superseded.
    pub lost: u64,
    /// Scrobbles the new reading has and the old one had not: they arrived
    /// after their own time.
    pub gained: u64,
}

/// What two scrobbles must share to be the same one in two readings: the
/// time, the artist and the track. The ids and the album are what Last.fm
/// says about a scrobble and may fill in or change while it stays the same
/// scrobble.
type Row = (u64, String, String);

/// Writes one checked window inside `transaction`: the window, its pages
/// and its scrobbles. Refuses one that overlaps a window already there.
fn insert_window(
    transaction: &rusqlite::Transaction<'_>,
    summary: &ScanSummary,
    pages: &[ScanPage],
    sealed: bool,
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
        "INSERT INTO windows (from_ts, to_ts, total, pages, page_size, read_at, sealed)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
        params![
            from,
            to,
            summary.total(),
            summary.pages(),
            page_size,
            read_at,
            sealed
        ],
    )?;
    let window_id = transaction.last_insert_rowid();
    let mut insert_page =
        transaction.prepare("INSERT INTO pages (window_id, page, body) VALUES (?1, ?2, ?3)")?;
    let mut insert_scrobble = transaction.prepare(
        "INSERT INTO scrobbles (window_id, page, position, timestamp, artist,
             artist_mbid, track, track_mbid, album, album_mbid)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
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
            ])?;
        }
    }
    Ok(())
}

/// Ends a transaction that wrote windows or the loved tracks: counts the
/// write, and refuses to
/// commit a layout in which a sealed window lies above one that is not.
/// The tail is the newest history or it is nothing.
fn finish_write(transaction: rusqlite::Transaction<'_>) -> Result<(), Error> {
    let ordered: bool = transaction.query_row(
        "SELECT coalesce((SELECT max(to_ts) FROM windows WHERE sealed = 1), 0)
             <= coalesce((SELECT min(from_ts) FROM windows WHERE sealed = 0),
                         9223372036854775807)",
        [],
        |row| row.get(0),
    )?;
    if !ordered {
        return Err(Error::Database(
            "the window would leave final history above the part still being read; is another \
             sync running?",
        ));
    }
    transaction.execute(
        "INSERT INTO meta (key, value) VALUES ('writes', '1')
         ON CONFLICT (key) DO UPDATE SET value = CAST(value AS INTEGER) + 1",
        [],
    )?;
    transaction.commit()?;
    Ok(())
}

/// An open database.
#[derive(Debug)]
pub struct Store {
    connection: Connection,
}

impl Store {
    /// Opens the database at `path`, creating it if the file is missing or
    /// empty, and bringing a file of an earlier layout up to this one.
    ///
    /// # Errors
    ///
    /// [`Error::Database`] when the path names no file (it is empty, or is
    /// SQLite's `:memory:`), when the file is some other SQLite database or
    /// was written by another version of this tool, and [`Error::Sqlite`]
    /// when it cannot be opened or is not a database at all.
    pub fn open(path: impl AsRef<Path>) -> Result<Self, Error> {
        let path = path.as_ref();
        // SQLite reads both of these as a database that is thrown away when
        // it is closed, which is the one thing a backup must not be.
        if path.as_os_str().is_empty() || path.as_os_str() == ":memory:" {
            return Err(Error::Database("the database path does not name a file"));
        }
        // Without the URI flag a path is only ever a path, so `file:` names
        // a file and cannot ask for a database in memory.
        let mut connection = Connection::open_with_flags(
            path,
            OpenFlags::SQLITE_OPEN_READ_WRITE
                | OpenFlags::SQLITE_OPEN_CREATE
                | OpenFlags::SQLITE_OPEN_NO_MUTEX,
        )?;
        connection.busy_timeout(std::time::Duration::from_secs(5))?;
        connection.pragma_update(None, "foreign_keys", true)?;

        let objects: i64 =
            connection.query_row("SELECT count(*) FROM sqlite_schema", [], |row| row.get(0))?;
        let application: i64 =
            connection.pragma_query_value(None, "application_id", |row| row.get(0))?;
        let version: i64 = connection.pragma_query_value(None, "user_version", |row| row.get(0))?;
        // New means untouched: no tables and neither mark set. A file that
        // another program has marked is that program's, even while empty.
        if objects == 0 && application == 0 && version == 0 {
            let transaction = connection.transaction()?;
            transaction.execute_batch(SCHEMA)?;
            transaction.execute_batch(LOVED_SCHEMA)?;
            transaction.execute_batch(SUPERSEDED_SCHEMA)?;
            transaction.pragma_update(None, "application_id", APPLICATION_ID)?;
            transaction.pragma_update(None, "user_version", SCHEMA_VERSION)?;
            transaction.commit()?;
            return Ok(Self { connection });
        }
        if application != APPLICATION_ID {
            return Err(Error::Database(
                "the file is a database, but not one this tool made",
            ));
        }
        if !(1..=SCHEMA_VERSION).contains(&version) {
            return Err(Error::Database(
                "the database was written by another version of this tool",
            ));
        }
        // One step at a time, each in a transaction of its own, so a file
        // is always at a whole version.
        if version == 1 {
            upgrade_from_1(&mut connection)?;
        }
        if version <= 2 {
            upgrade_from_2(&mut connection)?;
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

    /// The spans of `[0, cutoff)` that no sealed window covers, newest
    /// first, each as `(from, to)` with `from` inclusive and `to` exclusive.
    /// The tail does not count: it is read again whatever it covers.
    ///
    /// # Errors
    ///
    /// [`Error::Sqlite`].
    pub fn gaps(&self, cutoff: u64) -> Result<Vec<(u64, u64)>, Error> {
        let mut statement = self
            .connection
            .prepare("SELECT from_ts, to_ts FROM windows WHERE sealed = 1 ORDER BY from_ts")?;
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
    /// scan's checks. The pages are checked against it before anything is
    /// written: their number and order, the user and total each one reports,
    /// and that every scrobble lies in the window. That is not the scan's
    /// checks over again: pages from another scan of the same window with
    /// the same total would pass, so `pages` must be the ones `summary`
    /// came with. A window with no lower bound is stored from 0. All of it
    /// is written in one transaction.
    ///
    /// # Errors
    ///
    /// [`Error::Database`] when the window has no upper bound, when it
    /// overlaps a window already stored (another run got there first), when
    /// `pages` are not the pages the summary describes, or when the scan was
    /// not of the user the database belongs to, which has to be set first.
    /// Nothing is written in any of those cases.
    pub fn commit_window(
        &mut self,
        summary: &ScanSummary,
        pages: &[ScanPage],
        read_at: u64,
    ) -> Result<(), Error> {
        self.check_window(summary, pages)?;
        let transaction = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        insert_window(&transaction, summary, pages, true, read_at)?;
        finish_write(transaction)
    }

    /// Whether `pages` are the pages `summary` describes, of a scan of the
    /// user the database belongs to, with an upper bound.
    fn check_window(&self, summary: &ScanSummary, pages: &[ScanPage]) -> Result<(), Error> {
        let window = summary.window();
        if window.to().is_none() {
            return Err(Error::Database(
                "a window without an upper bound cannot be stored",
            ));
        }
        let page_size = pages.first().map_or(0, |page| page.attr().per_page());
        let scrobbles: usize = pages.iter().map(|page| page.scrobbles().len()).sum();
        let belong = pages.iter().zip(1..).all(|(page, number)| {
            page.number() == number
                && page.attr().total() == summary.total()
                && page.attr().per_page() == page_size
                && page.attr().user().eq_ignore_ascii_case(summary.user())
                && page
                    .scrobbles()
                    .iter()
                    .all(|scrobble| window.contains(scrobble.timestamp()))
        });
        if !belong
            || pages.len() as u64 != u64::from(summary.pages())
            || scrobbles as u64 != summary.total()
        {
            return Err(Error::Database(
                "the pages are not the ones the scan summary describes",
            ));
        }
        if !self
            .user()?
            .is_some_and(|bound| bound.eq_ignore_ascii_case(summary.user()))
        {
            return Err(Error::Database(
                "the scan is not of the user the database belongs to",
            ));
        }
        Ok(())
    }

    /// The windows that are not sealed, if there are any.
    ///
    /// # Errors
    ///
    /// [`Error::Sqlite`].
    pub fn tail(&self) -> Result<Option<Tail>, Error> {
        let span: (Option<u64>, Option<u64>) = self.connection.query_row(
            "SELECT min(from_ts), max(to_ts) FROM windows WHERE sealed = 0",
            [],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )?;
        Ok(match span {
            (Some(from), Some(to)) => Some(Tail { from, to }),
            _ => None,
        })
    }

    /// The tail and the count of writes, read as one: what a sync notes
    /// before it reads anything else, so that no write can fall between the
    /// two.
    ///
    /// # Errors
    ///
    /// [`Error::Sqlite`].
    pub fn tail_and_writes(&self) -> Result<(Option<Tail>, u64), Error> {
        let (from, to, writes): (Option<u64>, Option<u64>, u64) = self.connection.query_row(
            "SELECT (SELECT min(from_ts) FROM windows WHERE sealed = 0),
                    (SELECT max(to_ts) FROM windows WHERE sealed = 0),
                    coalesce((SELECT CAST(value AS INTEGER) FROM meta WHERE key = 'writes'), 0)",
            [],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )?;
        let tail = match (from, to) {
            (Some(from), Some(to)) => Some(Tail { from, to }),
            _ => None,
        };
        Ok((tail, writes))
    }

    /// How many times the history or the loved tracks have been written to
    /// the file, by any process. A reading that is to replace something
    /// notes this before it starts, and [`replace_tail`](Self::replace_tail)
    /// and [`replace_loved`](Self::replace_loved) refuse it if the number
    /// has moved since: a window's own number will not do, because SQLite
    /// uses one again once its window is gone.
    ///
    /// # Errors
    ///
    /// [`Error::Sqlite`].
    pub fn writes(&self) -> Result<u64, Error> {
        Ok(self
            .connection
            .query_row(
                "SELECT CAST(value AS INTEGER) FROM meta WHERE key = 'writes'",
                [],
                |row| row.get(0),
            )
            .optional()?
            .unwrap_or(0))
    }

    /// Where the stored history ends: the upper bound of the newest window,
    /// sealed or not, or 0 when there is none.
    ///
    /// # Errors
    ///
    /// [`Error::Sqlite`].
    pub fn covered_to(&self) -> Result<u64, Error> {
        Ok(self
            .connection
            .query_row("SELECT coalesce(max(to_ts), 0) FROM windows", [], |row| {
                row.get(0)
            })?)
    }

    /// Where the sealed history ends: the upper bound of the newest sealed
    /// window, or 0 when there is none.
    ///
    /// # Errors
    ///
    /// [`Error::Sqlite`].
    pub fn sealed_to(&self) -> Result<u64, Error> {
        Ok(self.connection.query_row(
            "SELECT coalesce(max(to_ts), 0) FROM windows WHERE sealed = 1",
            [],
            |row| row.get(0),
        )?)
    }

    /// Replaces the tail with a new reading of it, in one transaction.
    ///
    /// `was` is the tail as [`tail`](Self::tail) gave it before the reading
    /// began, or `None` if there was none, and `writes` is what
    /// [`writes`](Self::writes) gave then. `windows` are the new reading,
    /// in any order: they must lie end to end, start where the old tail
    /// started and reach at least as far, and the sealed ones must come
    /// before the others. Each is checked as
    /// [`commit_window`](Self::commit_window) checks one.
    ///
    /// The two readings are compared over the time the old one covered, by
    /// the time, artist and track of each row, counting rows that share all
    /// three. A
    /// scrobble the old reading had and the new one has not is *lost*, and
    /// when any is, the old windows and their pages are moved to
    /// `superseded_windows` and `superseded_pages` instead of being dropped.
    /// A scrobble that only the new reading has arrived late, and nothing
    /// needs keeping for it.
    ///
    /// # Errors
    ///
    /// [`Error::Database`] when windows were written to the file since
    /// `writes` was read (another sync was at work), when `windows` do not
    /// fit as said above, or when one overlaps a sealed window or would lie
    /// below one. Nothing is written in any of those cases: the tail stays
    /// as it was.
    pub fn replace_tail(
        &mut self,
        was: Option<&Tail>,
        writes: u64,
        windows: &[TailWindow<'_>],
        read_at: u64,
    ) -> Result<TailChange, Error> {
        for window in windows {
            self.check_window(window.summary, window.pages)?;
        }
        let mut windows = windows.to_vec();
        windows.sort_by_key(|window| window.summary.window().from().unwrap_or(0));
        let bounds = |window: &TailWindow<'_>| {
            let span = window.summary.window();
            (span.from().unwrap_or(0), span.to().unwrap_or(u64::MAX))
        };
        let joined = windows
            .iter()
            .zip(windows.iter().skip(1))
            .all(|(older, newer)| {
                bounds(older).1 == bounds(newer).0 && older.sealed >= newer.sealed
            });
        let covers = match (was, windows.first(), windows.last()) {
            (None, ..) => true,
            (Some(was), Some(first), Some(last)) => {
                bounds(first).0 == was.from && bounds(last).1 >= was.to
            }
            (Some(_), ..) => false,
        };
        if !joined || !covers {
            return Err(Error::Database(
                "the new reading of the recent history does not cover the one it replaces",
            ));
        }

        let transaction = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        let written: u64 = transaction
            .query_row(
                "SELECT CAST(value AS INTEGER) FROM meta WHERE key = 'writes'",
                [],
                |row| row.get(0),
            )
            .optional()?
            .unwrap_or(0);
        let now: Vec<i64> = transaction
            .prepare("SELECT id FROM windows WHERE sealed = 0 ORDER BY from_ts")?
            .query_map([], |row| row.get(0))?
            .collect::<Result<_, _>>()?;
        if written != writes || now.is_empty() != was.is_none() {
            return Err(Error::Database(
                "the history in the file changed while its newest part was read; is another \
                 sync running?",
            ));
        }

        // What the old reading held, each row counted, less what the new
        // one holds for the same time.
        let mut change = TailChange::default();
        if let Some(was) = was {
            let mut counts: HashMap<Row, i64> = HashMap::new();
            {
                let mut statement = transaction.prepare(
                    "SELECT timestamp, artist, track FROM scrobbles
                     WHERE window_id IN (SELECT id FROM windows WHERE sealed = 0)",
                )?;
                let rows =
                    statement.query_map([], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)))?;
                for row in rows {
                    *counts.entry(row?).or_default() += 1;
                }
            }
            for scrobble in windows
                .iter()
                .flat_map(|window| window.pages)
                .flat_map(|page| page.scrobbles())
                .filter(|scrobble| scrobble.timestamp() < was.to)
            {
                let row = (
                    scrobble.timestamp(),
                    scrobble.artist().name().to_owned(),
                    scrobble.track().name().to_owned(),
                );
                *counts.entry(row).or_default() -= 1;
            }
            for count in counts.into_values() {
                if count > 0 {
                    change.lost += count.unsigned_abs();
                } else {
                    change.gained += count.unsigned_abs();
                }
            }
        }

        if change.lost > 0 {
            // Numbered afresh: a window's number is used again once the
            // window is gone.
            for id in &now {
                transaction.execute(
                    "INSERT INTO superseded_windows
                         (from_ts, to_ts, total, pages, page_size, read_at, superseded_at)
                     SELECT from_ts, to_ts, total, pages, page_size, read_at, ?2
                     FROM windows WHERE id = ?1",
                    params![id, read_at],
                )?;
                transaction.execute(
                    "INSERT INTO superseded_pages (window_id, page, body)
                     SELECT ?2, page, body FROM pages WHERE window_id = ?1",
                    params![id, transaction.last_insert_rowid()],
                )?;
            }
        }
        transaction.execute(
            "DELETE FROM scrobbles WHERE window_id IN (SELECT id FROM windows WHERE sealed = 0)",
            [],
        )?;
        transaction.execute(
            "DELETE FROM pages WHERE window_id IN (SELECT id FROM windows WHERE sealed = 0)",
            [],
        )?;
        transaction.execute("DELETE FROM windows WHERE sealed = 0", [])?;
        for window in &windows {
            insert_window(
                &transaction,
                window.summary,
                window.pages,
                window.sealed,
                read_at,
            )?;
        }
        finish_write(transaction)?;
        Ok(change)
    }

    /// Replaces the loved tracks with `pages`, the whole list as one read
    /// returned it, page 1 first. The time of the read is kept in `meta` as
    /// `loved_read_at`. All of it is written in one transaction. `writes` is
    /// what [`writes`](Self::writes) gave before the read began.
    ///
    /// # Errors
    ///
    /// [`Error::Database`] when `pages` are not one whole list: when there
    /// are none, when they are out of order, when they disagree about the
    /// total, the page size or the user, when the user is not the one the
    /// database belongs to, or when they do not hold as many tracks as the
    /// total says, or when the file was written to since `writes` was read
    /// (another sync was at work, and its list may be the newer one).
    /// Nothing is written in any of those cases.
    pub fn replace_loved(
        &mut self,
        pages: &[Response<LovedTracksPage>],
        read_at: u64,
        writes: u64,
    ) -> Result<(), Error> {
        let Some(first) = pages.first().map(|page| page.attr()) else {
            return Err(Error::Database("a list of loved tracks has a first page"));
        };
        let bound = self.user()?;
        let tracks: usize = pages.iter().map(|page| page.tracks().len()).sum();
        let belong = pages.iter().zip(1..).all(|(page, number)| {
            let attr = page.attr();
            attr.page() == number
                && attr.total() == first.total()
                && attr.per_page() == first.per_page()
                && bound
                    .as_deref()
                    .is_some_and(|bound| attr.user().eq_ignore_ascii_case(bound))
        });
        if !belong || tracks as u64 != first.total() {
            return Err(Error::Database(
                "the pages are not one whole list of this user's loved tracks",
            ));
        }

        let transaction = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        let written: u64 = transaction
            .query_row(
                "SELECT CAST(value AS INTEGER) FROM meta WHERE key = 'writes'",
                [],
                |row| row.get(0),
            )
            .optional()?
            .unwrap_or(0);
        if written != writes {
            return Err(Error::Database(
                "the file changed while the loved tracks were read; is another sync running?",
            ));
        }
        transaction.execute("DELETE FROM loved", [])?;
        transaction.execute("DELETE FROM loved_pages", [])?;
        {
            let mut insert_page =
                transaction.prepare("INSERT INTO loved_pages (page, body) VALUES (?1, ?2)")?;
            let mut insert_track = transaction.prepare(
                "INSERT INTO loved (page, position, loved_at, artist, artist_mbid, track,
                     track_mbid)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            )?;
            for page in pages {
                let number = page.attr().page();
                insert_page.execute(params![number, page.raw().body()])?;
                for (position, loved) in page.tracks().iter().enumerate() {
                    insert_track.execute(params![
                        number,
                        position,
                        loved.loved_at(),
                        loved.artist().name(),
                        loved.artist().mbid(),
                        loved.track().name(),
                        loved.track().mbid(),
                    ])?;
                }
            }
        }
        transaction.execute(
            "INSERT INTO meta (key, value) VALUES ('loved_read_at', ?1)
             ON CONFLICT (key) DO UPDATE SET value = excluded.value",
            [read_at.to_string()],
        )?;
        finish_write(transaction)
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
