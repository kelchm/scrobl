//! The database on its own: what it accepts as its own file, whose history
//! it holds, which spans it lacks, and what it refuses to store.
//!
//! provenance: synthetic. The fake service generates every page, without a
//! socket.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]

mod common;

use common::*;
use scrobl::history::{RecentTracks, ScanPage, ScanSummary, Window};
use scrobl::model::LovedTracksPage;
use scrobl::protocol::{self, Credentials, HttpResponse, Request, methods};
use scrobl::{ApiKey, Response};
use scrobl_cli::Error;
use scrobl_cli::store::{Store, TailChange, TailWindow};

/// A database that belongs to [`USER`], as one must before it takes a
/// window.
fn bound(scratch: &Scratch) -> Store {
    let mut store = Store::open(scratch.db()).unwrap();
    store.bind_user(USER).unwrap();
    store
}

/// A finished scan of `window`, driven without a socket.
fn scan(dataset: &Dataset, window: Window, page_size: u32) -> (ScanSummary, Vec<ScanPage>) {
    let credentials = Credentials::new(ApiKey::new("SENTINEL_API_KEY_0001"));
    let mut scan = RecentTracks::new(USER)
        .window(window)
        .limit(page_size)
        .scan()
        .unwrap();
    let mut pages = Vec::new();
    while let Some(request) = scan.next_request() {
        let http = protocol::prepare(&credentials, &request).unwrap();
        let (status, body) = support::respond(dataset, &http);
        let raw = protocol::decode(&request, HttpResponse::new(status, body)).unwrap();
        pages.push(scan.accept(raw).unwrap());
    }
    (scan.finish().unwrap(), pages)
}

fn count(store: &Store, table: &str) -> u64 {
    store
        .connection()
        .query_row(&format!("SELECT count(*) FROM {table}"), [], |row| {
            row.get(0)
        })
        .unwrap()
}

fn database_error(result: Result<impl std::fmt::Debug, Error>) -> &'static str {
    match result {
        Err(Error::Database(message)) => message,
        other => panic!("expected a database error, got {other:?}"),
    }
}

#[test]
fn a_new_file_is_created_and_opens_again() {
    let scratch = Scratch::new();
    {
        let mut store = Store::open(scratch.db()).unwrap();
        assert_eq!(store.user().unwrap(), None);
        store.bind_user("rj").unwrap();
    }
    let store = Store::open(scratch.db()).unwrap();
    assert_eq!(store.user().unwrap().as_deref(), Some("rj"));
    assert_eq!(store.gaps(100).unwrap(), [(0, 100)]);
}

#[test]
fn another_programs_database_is_refused_and_left_alone() {
    let scratch = Scratch::new();
    let other = rusqlite::Connection::open(scratch.db()).unwrap();
    other.execute_batch("CREATE TABLE notes (text)").unwrap();
    drop(other);

    let message = database_error(Store::open(scratch.db()));
    assert!(message.contains("not one this tool made"));

    let other = rusqlite::Connection::open(scratch.db()).unwrap();
    let tables: u64 = other
        .query_row("SELECT count(*) FROM sqlite_schema", [], |row| row.get(0))
        .unwrap();
    assert_eq!(tables, 1);
}

#[test]
fn a_file_that_is_no_database_is_refused() {
    let scratch = Scratch::new();
    std::fs::write(scratch.db(), b"this is not a database, only a text file").unwrap();
    assert!(matches!(Store::open(scratch.db()), Err(Error::Sqlite(_))));
}

#[test]
fn a_database_of_another_version_is_refused() {
    let scratch = Scratch::new();
    Store::open(scratch.db())
        .unwrap()
        .connection()
        .pragma_update(None, "user_version", 4)
        .unwrap();

    let message = database_error(Store::open(scratch.db()));
    assert!(message.contains("another version"));
}

/// The layout of version 1 as it was first written, with a `loved` column
/// on `scrobbles` and no tables for the loved tracks.
const VERSION_1: &str = "
CREATE TABLE meta (key TEXT PRIMARY KEY, value TEXT NOT NULL) STRICT;
CREATE TABLE windows (
    id INTEGER PRIMARY KEY, from_ts INTEGER NOT NULL, to_ts INTEGER NOT NULL,
    total INTEGER NOT NULL, pages INTEGER NOT NULL, page_size INTEGER NOT NULL,
    read_at INTEGER NOT NULL, CHECK (from_ts >= 0 AND from_ts < to_ts)
) STRICT;
CREATE TABLE pages (
    window_id INTEGER NOT NULL REFERENCES windows (id), page INTEGER NOT NULL,
    body BLOB NOT NULL, PRIMARY KEY (window_id, page)
) STRICT;
CREATE TABLE scrobbles (
    id INTEGER PRIMARY KEY, window_id INTEGER NOT NULL REFERENCES windows (id),
    page INTEGER NOT NULL, position INTEGER NOT NULL, timestamp INTEGER NOT NULL,
    artist TEXT NOT NULL, artist_mbid TEXT, track TEXT NOT NULL, track_mbid TEXT,
    album TEXT, album_mbid TEXT, loved INTEGER, UNIQUE (window_id, page, position)
) STRICT;
CREATE INDEX scrobbles_timestamp ON scrobbles (timestamp);
INSERT INTO meta VALUES ('user', 'rj_Synthetic');
INSERT INTO windows VALUES (1, 0, 1700000100, 1, 1, 200, 1700000200);
INSERT INTO pages VALUES (1, 1, x'7b7d');
INSERT INTO scrobbles VALUES
    (1, 1, 1, 0, 1700000010, 'Artist', NULL, 'Track', NULL, 'Album', NULL, 1);
PRAGMA application_id = 0x7363626c;
PRAGMA user_version = 1;
";

#[test]
fn a_version_1_file_is_brought_up_to_date_and_keeps_its_history() {
    // The first form had a `loved` column; the second had already lost it.
    for layout in [
        VERSION_1.to_owned(),
        VERSION_1
            .replace("album_mbid TEXT, loved INTEGER,", "album_mbid TEXT,")
            .replace("'Album', NULL, 1);", "'Album', NULL);"),
    ] {
        let scratch = Scratch::new();
        rusqlite::Connection::open(scratch.db())
            .unwrap()
            .execute_batch(&layout)
            .unwrap();

        let mut store = Store::open(scratch.db()).unwrap();

        let version: i64 = store
            .connection()
            .pragma_query_value(None, "user_version", |row| row.get(0))
            .unwrap();
        assert_eq!(version, 3);
        // What it held was read once and is final; there is no tail yet.
        assert_eq!(store.tail().unwrap(), None);
        assert_eq!(store.sealed_to().unwrap(), store.covered_to().unwrap());
        assert_eq!(count(&store, "superseded_windows"), 0);
        assert_eq!(count(&store, "superseded_pages"), 0);
        let columns: Vec<String> = store
            .connection()
            .prepare("SELECT name FROM pragma_table_info('scrobbles')")
            .unwrap()
            .query_map([], |row| row.get(0))
            .unwrap()
            .map(Result::unwrap)
            .collect();
        assert!(!columns.contains(&"loved".to_owned()));
        assert_eq!(store.user().unwrap().as_deref(), Some(USER));
        assert_eq!(store.scrobbles_before(1_700_000_100).unwrap(), 1);
        assert_eq!(store.gaps(1_700_000_100).unwrap(), []);

        // It takes the loved tracks, and a window, like a new file.
        let dataset = Dataset::distinct(1).with_loved(3);
        store.replace_loved(&loved_pages(&dataset, 2), 100).unwrap();
        assert_eq!(count(&store, "loved"), 3);
        drop(store);
        // And opens again as it is.
        assert_eq!(count(&Store::open(scratch.db()).unwrap(), "loved"), 3);
    }
}

#[test]
fn a_database_holds_one_users_history() {
    let scratch = Scratch::new();
    let mut store = Store::open(scratch.db()).unwrap();
    store.bind_user("RJ").unwrap();

    // Last.fm ignores case in a user name, so this is the same user.
    store.bind_user("rj").unwrap();
    assert_eq!(store.user().unwrap().as_deref(), Some("RJ"));

    let error = store.bind_user("someone-else").unwrap_err();
    assert!(matches!(error, Error::OtherUser));
    assert_eq!(error.exit_code(), 1);
    assert!(error.to_string().contains("another user"));
}

#[test]
fn the_gaps_are_what_no_window_covers_newest_first() {
    let scratch = Scratch::new();
    let store = Store::open(scratch.db()).unwrap();
    store
        .connection()
        .execute_batch(
            "INSERT INTO windows (from_ts, to_ts, total, pages, page_size, read_at) VALUES
                 (100, 200, 0, 1, 200, 0),
                 (300, 400, 0, 1, 200, 0),
                 (400, 450, 0, 1, 200, 0)",
        )
        .unwrap();

    assert_eq!(store.gaps(500).unwrap(), [(450, 500), (200, 300), (0, 100)]);
    assert_eq!(store.gaps(450).unwrap(), [(200, 300), (0, 100)]);
    // A cutoff inside a window, inside a gap, and before every window.
    assert_eq!(store.gaps(350).unwrap(), [(200, 300), (0, 100)]);
    assert_eq!(store.gaps(250).unwrap(), [(200, 250), (0, 100)]);
    assert_eq!(store.gaps(50).unwrap(), [(0, 50)]);
    assert_eq!(store.gaps(0).unwrap(), []);
}

#[test]
fn a_window_is_stored_whole() {
    let scratch = Scratch::new();
    let mut store = bound(&scratch);
    let dataset = Dataset::distinct(10);
    let (summary, pages) = scan(&dataset, Window::before(BASE + 1_000), 4);

    store.commit_window(&summary, &pages, 42).unwrap();

    assert_eq!(count(&store, "windows"), 1);
    assert_eq!(count(&store, "pages"), 3);
    assert_eq!(count(&store, "scrobbles"), 10);
    assert_eq!(store.scrobbles_before(BASE + 1_000).unwrap(), 10);
    assert_eq!(store.scrobbles_before(BASE + 51).unwrap(), 5);
    assert_eq!(
        store.gaps(BASE + 2_000).unwrap(),
        [(BASE + 1_000, BASE + 2_000)]
    );
    let window: (u64, u64, u64, u64, u64, u64) = store
        .connection()
        .query_row(
            "SELECT from_ts, to_ts, total, pages, page_size, read_at FROM windows",
            [],
            |row| {
                Ok((
                    row.get(0)?,
                    row.get(1)?,
                    row.get(2)?,
                    row.get(3)?,
                    row.get(4)?,
                    row.get(5)?,
                ))
            },
        )
        .unwrap();
    assert_eq!(window, (0, BASE + 1_000, 10, 3, 4, 42));
}

#[test]
fn a_window_that_overlaps_a_stored_one_is_refused() {
    let scratch = Scratch::new();
    let mut store = bound(&scratch);
    let dataset = Dataset::distinct(10);
    let (summary, pages) = scan(&dataset, Window::new(BASE, BASE + 60).unwrap(), 4);
    store.commit_window(&summary, &pages, 0).unwrap();

    // The same window again, as a second sync on the same file would send.
    let message = database_error(store.commit_window(&summary, &pages, 0));
    assert!(message.contains("overlaps"));

    // One second of overlap at either end is still an overlap.
    for window in [
        Window::new(BASE + 59, BASE + 200).unwrap(),
        Window::before(BASE + 1),
    ] {
        let (summary, pages) = scan(&dataset, window, 4);
        database_error(store.commit_window(&summary, &pages, 0));
    }
    assert_eq!(count(&store, "windows"), 1);
    assert_eq!(count(&store, "scrobbles"), 5);

    // The windows on either side touch it and are stored.
    for window in [
        Window::new(BASE + 60, BASE + 200).unwrap(),
        Window::before(BASE),
    ] {
        let (summary, pages) = scan(&dataset, window, 4);
        store.commit_window(&summary, &pages, 0).unwrap();
    }
    assert_eq!(count(&store, "scrobbles"), 10);
    assert_eq!(store.gaps(BASE + 200).unwrap(), []);
}

#[test]
fn pages_that_are_not_the_summarys_are_refused() {
    let scratch = Scratch::new();
    let mut store = bound(&scratch);
    let dataset = Dataset::distinct(10);
    let (summary, pages) = scan(&dataset, Window::before(BASE + 1_000), 4);

    let message = database_error(store.commit_window(&summary, &pages[..2], 0));
    assert!(message.contains("not the ones"));
    database_error(store.commit_window(&summary, &[], 0));

    // The right number of pages, holding the wrong number of rows.
    let (_, other) = scan(&Dataset::distinct(9), Window::before(BASE + 1_000), 4);
    database_error(store.commit_window(&summary, &other, 0));
    assert_eq!(count(&store, "windows"), 0);
    assert_eq!(count(&store, "pages"), 0);
}

#[test]
fn a_window_without_an_upper_bound_is_refused() {
    let scratch = Scratch::new();
    let mut store = bound(&scratch);
    let (summary, pages) = scan(&Dataset::distinct(3), Window::since(BASE), 4);

    let message = database_error(store.commit_window(&summary, &pages, 0));
    assert!(message.contains("upper bound"));
    assert_eq!(count(&store, "windows"), 0);
}

#[test]
fn a_path_that_names_no_file_is_refused() {
    // SQLite reads both as a database that is gone when it is closed.
    for path in ["", ":memory:"] {
        let message = database_error(Store::open(path));
        assert!(message.contains("does not name a file"));
    }
}

#[test]
fn a_uri_is_a_file_name_and_not_a_database_in_memory() {
    let scratch = Scratch::new();
    let path = scratch.db().with_file_name("file:history?mode=memory");
    Store::open(&path).unwrap().bind_user("rj").unwrap();

    assert!(path.exists());
    assert_eq!(
        Store::open(&path).unwrap().user().unwrap().as_deref(),
        Some("rj")
    );
}

#[test]
fn an_empty_database_that_another_program_marked_is_refused_and_left_alone() {
    let scratch = Scratch::new();
    let other = rusqlite::Connection::open(scratch.db()).unwrap();
    other.pragma_update(None, "application_id", 12_345).unwrap();
    other.pragma_update(None, "user_version", 9).unwrap();
    drop(other);

    database_error(Store::open(scratch.db()));

    let other = rusqlite::Connection::open(scratch.db()).unwrap();
    let marks: (i64, i64, i64) = (
        other
            .pragma_query_value(None, "application_id", |row| row.get(0))
            .unwrap(),
        other
            .pragma_query_value(None, "user_version", |row| row.get(0))
            .unwrap(),
        other
            .query_row("SELECT count(*) FROM sqlite_schema", [], |row| row.get(0))
            .unwrap(),
    );
    assert_eq!(marks, (12_345, 9, 0));
}

#[test]
fn pages_of_another_window_are_refused_even_when_the_counts_agree() {
    let scratch = Scratch::new();
    let mut store = bound(&scratch);
    let dataset = Dataset::distinct(10);
    // Two windows of five scrobbles in two pages each.
    let (summary, _) = scan(&dataset, Window::new(BASE, BASE + 51).unwrap(), 4);
    let (_, other) = scan(&dataset, Window::new(BASE + 51, BASE + 200).unwrap(), 4);

    let message = database_error(store.commit_window(&summary, &other, 0));
    assert!(message.contains("not the ones"));

    // The right pages in the wrong order, and one of them twice.
    let (summary, pages) = scan(&dataset, Window::new(BASE, BASE + 81).unwrap(), 4);
    let swapped = [pages[1].clone(), pages[0].clone()];
    database_error(store.commit_window(&summary, &swapped, 0));
    let repeated = [pages[0].clone(), pages[0].clone()];
    database_error(store.commit_window(&summary, &repeated, 0));

    // Pages another user's history would give for the same window.
    let mut theirs = Dataset::distinct(10);
    theirs.user = "someone-else".to_owned();
    let credentials = Credentials::new(ApiKey::new("SENTINEL_API_KEY_0001"));
    let mut their_scan = RecentTracks::new("someone-else")
        .window(Window::new(BASE, BASE + 81).unwrap())
        .limit(4)
        .scan()
        .unwrap();
    let mut their_pages = Vec::new();
    while let Some(request) = their_scan.next_request() {
        let http = protocol::prepare(&credentials, &request).unwrap();
        let (status, body) = support::respond(&theirs, &http);
        let raw = protocol::decode(&request, HttpResponse::new(status, body)).unwrap();
        their_pages.push(their_scan.accept(raw).unwrap());
    }
    database_error(store.commit_window(&summary, &their_pages, 0));

    assert_eq!(count(&store, "windows"), 0);
    assert_eq!(count(&store, "pages"), 0);
    assert_eq!(count(&store, "scrobbles"), 0);
}

#[test]
fn a_window_that_fails_part_way_leaves_nothing_behind() {
    let scratch = Scratch::new();
    let mut store = bound(&scratch);
    // The write fails on the second page, after a window, a page and four
    // scrobbles have gone in.
    store
        .connection()
        .execute_batch(
            "CREATE TRIGGER stop AFTER INSERT ON scrobbles WHEN NEW.page = 2
             BEGIN SELECT RAISE(ABORT, 'stopped by the test'); END",
        )
        .unwrap();
    let (summary, pages) = scan(&Dataset::distinct(10), Window::before(BASE + 1_000), 4);

    let result = store.commit_window(&summary, &pages, 0);

    assert!(matches!(result, Err(Error::Sqlite(_))));
    assert_eq!(count(&store, "windows"), 0);
    assert_eq!(count(&store, "pages"), 0);
    assert_eq!(count(&store, "scrobbles"), 0);
    assert_eq!(store.gaps(BASE + 1_000).unwrap(), [(0, BASE + 1_000)]);
}

#[test]
fn two_syncs_on_one_file_cannot_store_a_period_twice() {
    let scratch = Scratch::new();
    let mut first = bound(&scratch);
    let mut second = bound(&scratch);
    // Both saw the same gap and read the same window.
    let (summary, pages) = scan(&Dataset::distinct(10), Window::before(BASE + 1_000), 4);

    first.commit_window(&summary, &pages, 0).unwrap();
    let message = database_error(second.commit_window(&summary, &pages, 0));

    assert!(message.contains("overlaps"));
    assert_eq!(count(&second, "windows"), 1);
    assert_eq!(count(&second, "scrobbles"), 10);
}

/// Every page of `dataset`'s loved tracks, as the service would send them.
fn loved_pages(dataset: &Dataset, page_size: u32) -> Vec<Response<LovedTracksPage>> {
    let credentials = Credentials::new(ApiKey::new("SENTINEL_API_KEY_0001"));
    let mut pages = Vec::new();
    loop {
        let request = Request::new(&methods::USER_GET_LOVED_TRACKS)
            .param("user", &dataset.user)
            .param("limit", page_size)
            .param("page", pages.len() + 1);
        let http = protocol::prepare(&credentials, &request).unwrap();
        let (status, body) = support::respond(dataset, &http);
        let raw = protocol::decode(&request, HttpResponse::new(status, body)).unwrap();
        let page = LovedTracksPage::decode(&raw).unwrap();
        let last = pages.len() as u64 + 1 >= page.attr().total_pages();
        pages.push(Response::new(page, raw));
        if last {
            return pages;
        }
    }
}

/// The loved tracks as stored: `(page, position, loved_at, track)`.
fn loved_rows(store: &Store) -> Vec<(u64, u64, u64, String)> {
    store
        .connection()
        .prepare("SELECT page, position, loved_at, track FROM loved ORDER BY id")
        .unwrap()
        .query_map([], |row| {
            Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?))
        })
        .unwrap()
        .map(Result::unwrap)
        .collect()
}

fn loved_read_at(store: &Store) -> Option<String> {
    use rusqlite::OptionalExtension;
    store
        .connection()
        .query_row(
            "SELECT value FROM meta WHERE key = 'loved_read_at'",
            [],
            |row| row.get(0),
        )
        .optional()
        .unwrap()
}

#[test]
fn loved_tracks_are_stored_with_the_pages_they_came_on() {
    let scratch = Scratch::new();
    let mut store = Store::open(scratch.db()).unwrap();
    store.bind_user(USER).unwrap();
    let dataset = Dataset::distinct(1).with_loved(3);
    let pages = loved_pages(&dataset, 2);

    store.replace_loved(&pages, 1_800_000_000).unwrap();

    assert_eq!(
        loved_rows(&store),
        [
            (1, 0, BASE + 300, "Loved 3".to_owned()),
            (1, 1, BASE + 200, "Loved 2".to_owned()),
            (2, 0, BASE + 100, "Loved 1".to_owned()),
        ]
    );
    let (artist, artist_mbid, track_mbid): (String, Option<String>, Option<String>) = store
        .connection()
        .query_row(
            "SELECT artist, artist_mbid, track_mbid FROM loved LIMIT 1",
            [],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )
        .unwrap();
    assert_eq!(artist, "Synthetic Artist");
    assert_eq!(artist_mbid.as_deref(), Some(support::ARTIST_MBID));
    // An empty mbid is stored as NULL.
    assert_eq!(track_mbid, None);

    let bodies: Vec<Vec<u8>> = store
        .connection()
        .prepare("SELECT body FROM loved_pages ORDER BY page")
        .unwrap()
        .query_map([], |row| row.get(0))
        .unwrap()
        .map(Result::unwrap)
        .collect();
    let sent: Vec<&[u8]> = pages.iter().map(|page| page.raw().body()).collect();
    assert_eq!(bodies, sent);
    assert_eq!(loved_read_at(&store).as_deref(), Some("1800000000"));
}

#[test]
fn a_read_of_the_loved_tracks_replaces_the_one_before() {
    let scratch = Scratch::new();
    let mut store = Store::open(scratch.db()).unwrap();
    store.bind_user(USER).unwrap();
    let before = Dataset::distinct(1).with_loved(5);
    store.replace_loved(&loved_pages(&before, 2), 100).unwrap();
    assert_eq!(
        (count(&store, "loved"), count(&store, "loved_pages")),
        (5, 3)
    );

    // Three were unloved since.
    let after = Dataset::distinct(1).with_loved(2);
    store.replace_loved(&loved_pages(&after, 2), 200).unwrap();

    assert_eq!(
        (count(&store, "loved"), count(&store, "loved_pages")),
        (2, 1)
    );
    assert_eq!(loved_read_at(&store).as_deref(), Some("200"));

    // And none at all is a list too: one page with no rows.
    let none = Dataset::distinct(1);
    store.replace_loved(&loved_pages(&none, 2), 300).unwrap();
    assert_eq!(
        (count(&store, "loved"), count(&store, "loved_pages")),
        (0, 1)
    );
}

#[test]
fn pages_that_are_not_one_whole_list_of_loved_tracks_are_refused() {
    let scratch = Scratch::new();
    let mut store = Store::open(scratch.db()).unwrap();
    let dataset = Dataset::distinct(1).with_loved(5);
    let pages = loved_pages(&dataset, 2);

    // A database that belongs to no one yet cannot say whose list this is.
    database_error(store.replace_loved(&pages, 100));

    store.bind_user(USER).unwrap();
    store.replace_loved(&pages, 100).unwrap();

    let mut swapped = pages.clone();
    swapped.swap(0, 1);
    let mut theirs = dataset.clone();
    theirs.user = "someone-else".to_owned();
    // Pages of two sizes, which is two reads.
    let mut mixed = pages.clone();
    mixed[2] = loved_pages(&dataset, 1).swap_remove(2);
    // A list that grew between the first page and the rest.
    let mut grown = pages.clone();
    grown[0] = loved_pages(&Dataset::distinct(1).with_loved(6), 2).swap_remove(0);
    for wrong in [
        &[][..],
        &pages[..2],
        &pages[1..],
        &swapped,
        &loved_pages(&theirs, 2),
        &mixed,
        &grown,
    ] {
        let message = database_error(store.replace_loved(wrong, 200));
        assert!(message.contains("loved tracks"), "{message}");
    }

    // Nothing of any of them was written.
    assert_eq!(
        (count(&store, "loved"), count(&store, "loved_pages")),
        (5, 3)
    );
    assert_eq!(loved_read_at(&store).as_deref(), Some("100"));
}

#[test]
fn a_replacement_that_fails_part_way_keeps_the_list_before() {
    let scratch = Scratch::new();
    let mut store = Store::open(scratch.db()).unwrap();
    store.bind_user(USER).unwrap();
    let before = Dataset::distinct(1).with_loved(3);
    store.replace_loved(&loved_pages(&before, 2), 100).unwrap();
    let bodies = |store: &Store| -> Vec<Vec<u8>> {
        store
            .connection()
            .prepare("SELECT body FROM loved_pages ORDER BY page")
            .unwrap()
            .query_map([], |row| row.get(0))
            .unwrap()
            .map(Result::unwrap)
            .collect()
    };
    let (kept, kept_bodies) = (loved_rows(&store), bodies(&store));

    // The second track's time is more than a column holds: the insert of
    // it fails after a page and a track of the new list went in.
    let mut after = Dataset::distinct(1).with_loved(4);
    after.loved[1] = u64::MAX;
    let failed = store.replace_loved(&loved_pages(&after, 2), 200);

    assert!(matches!(failed, Err(Error::Sqlite(_))), "{failed:?}");
    assert_eq!(loved_rows(&store), kept);
    // Byte for byte: the new list has two pages as well.
    assert_eq!(bodies(&store), kept_bodies);
    assert_eq!(loved_read_at(&store).as_deref(), Some("100"));
}

#[test]
fn a_window_is_stored_only_for_the_user_the_database_belongs_to() {
    let dataset = Dataset::distinct(3);
    let (summary, pages) = scan(&dataset, Window::before(BASE + 100), 200);

    // No user yet: nothing says whose history this is.
    let scratch = Scratch::new();
    let mut store = Store::open(scratch.db()).unwrap();
    let message = database_error(store.commit_window(&summary, &pages, 0));
    assert!(message.contains("belongs to"), "{message}");

    // Someone else's.
    let scratch = Scratch::new();
    let mut store = Store::open(scratch.db()).unwrap();
    store.bind_user("someone-else").unwrap();
    database_error(store.commit_window(&summary, &pages, 0));
    assert_eq!(count(&store, "windows"), 0);
    assert_eq!(count(&store, "scrobbles"), 0);

    // The same user in another case.
    let scratch = Scratch::new();
    let mut store = Store::open(scratch.db()).unwrap();
    store.bind_user(&USER.to_uppercase()).unwrap();
    store.commit_window(&summary, &pages, 0).unwrap();
    assert_eq!(count(&store, "scrobbles"), 3);
}

// The tail: windows that are not sealed, and what replaces them.

/// The windows as `(from, to, total, sealed)`, oldest first.
fn layout(store: &Store) -> Vec<(u64, u64, u64, bool)> {
    store
        .connection()
        .prepare("SELECT from_ts, to_ts, total, sealed FROM windows ORDER BY from_ts")
        .unwrap()
        .query_map([], |row| {
            Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?))
        })
        .unwrap()
        .map(Result::unwrap)
        .collect()
}

#[test]
fn the_tail_is_replaced_whole_and_only_by_a_reading_that_covers_it() {
    let scratch = Scratch::new();
    let mut store = bound(&scratch);
    let dataset = Dataset::distinct(10);
    let scan = |from: u64, to: u64| scan(&dataset, Window::new(from, to).unwrap(), 200);
    let (old, a, b, c) = (
        scan(BASE, BASE + 60),
        scan(BASE, BASE + 30),
        scan(BASE + 30, BASE + 60),
        scan(BASE + 60, BASE + 200),
    );
    fn window(scan: &(ScanSummary, Vec<ScanPage>), sealed: bool) -> TailWindow<'_> {
        TailWindow {
            summary: &scan.0,
            pages: &scan.1,
            sealed,
        }
    }
    let (old, a, b, c) = (&old, &a, &b, &c);

    // The first tail, where there was none.
    assert_eq!(store.tail().unwrap(), None);
    let change = store
        .replace_tail(None, 0, &[window(old, false)], 1)
        .unwrap();
    assert_eq!(change, TailChange::default());
    let tail = store.tail().unwrap().unwrap();
    let writes = store.writes().unwrap();
    assert_eq!(writes, 1);
    assert_eq!((tail.from, tail.to), (BASE, BASE + 60));
    assert_eq!(store.sealed_to().unwrap(), 0);
    assert_eq!(store.covered_to().unwrap(), BASE + 60);
    // The tail is no gap closed: what is sealed is.
    assert_eq!(store.gaps(BASE + 60).unwrap(), [(0, BASE + 60)]);

    // Readings that do not fit, each refused with the tail left as it was.
    for wrong in [
        // Nothing at all.
        vec![],
        // It starts later than the tail did.
        vec![window(b, false), window(c, false)],
        // It ends sooner.
        vec![window(a, false)],
        // A hole in it.
        vec![window(a, true), window(c, false)],
        // A sealed window after one that is not.
        vec![window(a, false), window(b, true)],
    ] {
        let message = database_error(store.replace_tail(Some(&tail), writes, &wrong, 2));
        assert!(message.contains("does not cover"), "{message}");
        assert_eq!(layout(&store), [(BASE, BASE + 60, 5, false)]);
    }

    // One that fits, in any order: part sealed, the rest the new tail.
    let change = store
        .replace_tail(
            Some(&tail),
            writes,
            &[window(c, false), window(a, true), window(b, false)],
            3,
        )
        .unwrap();
    assert_eq!(change, TailChange::default());
    assert_eq!(
        layout(&store),
        [
            (BASE, BASE + 30, 2, true),
            (BASE + 30, BASE + 60, 3, false),
            (BASE + 60, BASE + 200, 5, false),
        ]
    );
    assert_eq!(count(&store, "scrobbles"), 10);
    assert_eq!(count(&store, "superseded_windows"), 0);
    assert_eq!(store.sealed_to().unwrap(), BASE + 30);

    // A reading made before that write is stale, whatever it holds: another
    // sync got there first.
    assert_eq!(store.writes().unwrap(), 2);
    let now = store.tail().unwrap().unwrap();
    for stale in [0, 1, 3] {
        let message = database_error(store.replace_tail(
            Some(&now),
            stale,
            &[window(b, false), window(c, false)],
            4,
        ));
        assert!(message.contains("another sync"), "{message}");
    }
    // So is one that takes there to be no tail when there is one, or one
    // when there is none.
    let message = database_error(store.replace_tail(None, 2, &[window(c, false)], 4));
    assert!(message.contains("another sync"), "{message}");
    assert_eq!(layout(&store).len(), 3);
    assert_eq!(store.writes().unwrap(), 2);
}

#[test]
fn a_tail_put_back_just_as_it_was_still_counts_as_a_write() {
    let scratch = Scratch::new();
    let mut store = bound(&scratch);
    let dataset = Dataset::distinct(10);
    let (summary, pages) = scan(&dataset, Window::new(BASE, BASE + 200).unwrap(), 200);
    let window = [TailWindow {
        summary: &summary,
        pages: &pages,
        sealed: false,
    }];
    store.replace_tail(None, 0, &window, 1).unwrap();
    let (tail, writes) = (store.tail().unwrap().unwrap(), store.writes().unwrap());
    let id = |store: &Store| -> i64 {
        store
            .connection()
            .query_row("SELECT id FROM windows", [], |row| row.get(0))
            .unwrap()
    };
    let first = id(&store);

    // Another sync replaces the tail with the same span. SQLite gives the
    // new window the number the old one had, so nothing about the tail
    // itself shows that it was replaced.
    store.replace_tail(Some(&tail), writes, &window, 2).unwrap();
    assert_eq!(id(&store), first);
    assert_eq!(store.tail().unwrap().unwrap(), tail);

    // The reading made before that is refused all the same.
    let message = database_error(store.replace_tail(Some(&tail), writes, &window, 3));
    assert!(message.contains("another sync"), "{message}");
}

#[test]
fn final_history_never_lies_above_the_tail() {
    let dataset = Dataset::distinct(10);
    let scan = |from: u64, to: u64| scan(&dataset, Window::new(from, to).unwrap(), 200);
    let (low, high) = (scan(BASE, BASE + 50), scan(BASE + 50, BASE + 200));
    fn tail(scan: &(ScanSummary, Vec<ScanPage>)) -> Vec<TailWindow<'_>> {
        vec![TailWindow {
            summary: &scan.0,
            pages: &scan.1,
            sealed: false,
        }]
    }

    // A tail below a sealed window that another sync stored meanwhile.
    let scratch = Scratch::new();
    let mut store = bound(&scratch);
    store.commit_window(&high.0, &high.1, 1).unwrap();
    let writes = store.writes().unwrap();
    let message = database_error(store.replace_tail(None, writes, &tail(&low), 2));
    assert!(message.contains("final history above"), "{message}");
    assert_eq!(layout(&store), [(BASE + 50, BASE + 200, 6, true)]);
    assert_eq!(store.writes().unwrap(), writes);

    // A sealed window above the tail.
    let scratch = Scratch::new();
    let mut store = bound(&scratch);
    store.replace_tail(None, 0, &tail(&low), 1).unwrap();
    let message = database_error(store.commit_window(&high.0, &high.1, 2));
    assert!(message.contains("final history above"), "{message}");
    assert_eq!(layout(&store), [(BASE, BASE + 50, 4, false)]);

    // Below it is where sealed history belongs.
    let (older, _) = (scan(BASE - 100, BASE), ());
    store.commit_window(&older.0, &older.1, 3).unwrap();
    assert_eq!(layout(&store).len(), 2);
}

#[test]
fn a_tail_window_shows_nothing_the_network_chose() {
    let dataset = Dataset::distinct(3);
    let (summary, pages) = scan(&dataset, Window::new(BASE, BASE + 100).unwrap(), 200);
    let shown = format!(
        "{:?}",
        TailWindow {
            summary: &summary,
            pages: &pages,
            sealed: false,
        }
    );
    assert_eq!(
        shown,
        format!(
            "TailWindow {{ from: Some({BASE}), to: Some({}), total: 3, sealed: false }}",
            BASE + 100
        )
    );
}

#[test]
fn a_new_tail_may_not_overlap_what_is_sealed() {
    let scratch = Scratch::new();
    let mut store = bound(&scratch);
    let dataset = Dataset::distinct(10);
    let (summary, pages) = scan(&dataset, Window::before(BASE + 60), 200);
    store.commit_window(&summary, &pages, 0).unwrap();

    let (summary, pages) = scan(&dataset, Window::new(BASE + 30, BASE + 200).unwrap(), 200);
    let writes = store.writes().unwrap();
    let message = database_error(store.replace_tail(
        None,
        writes,
        &[TailWindow {
            summary: &summary,
            pages: &pages,
            sealed: false,
        }],
        1,
    ));

    assert!(message.contains("overlaps"), "{message}");
    assert_eq!(layout(&store), [(0, BASE + 60, 5, true)]);
}

#[test]
fn every_reading_that_lost_a_scrobble_is_kept_under_a_number_of_its_own() {
    let scratch = Scratch::new();
    let mut store = bound(&scratch);
    let full = Dataset::distinct(6);
    let reading = |dataset: &Dataset| scan(dataset, Window::new(BASE, BASE + 100).unwrap(), 200);
    let without = |times: &[u64]| {
        let mut dataset = full.clone();
        dataset.entries.retain(|entry| !times.contains(&entry.ts));
        dataset
    };
    let mut replace = |dataset: &Dataset, read_at| {
        let (summary, pages) = reading(dataset);
        let was = store.tail().unwrap();
        store
            .replace_tail(
                was.as_ref(),
                store.writes().unwrap(),
                &[TailWindow {
                    summary: &summary,
                    pages: &pages,
                    sealed: false,
                }],
                read_at,
            )
            .unwrap()
    };

    replace(&full, 1);
    // One goes, then another: the window's number is used again each time,
    // and each reading is kept all the same.
    let change = replace(&without(&[BASE + 10]), 2);
    assert_eq!((change.lost, change.gained), (1, 0));
    let change = replace(&without(&[BASE + 10, BASE + 20]), 3);
    assert_eq!((change.lost, change.gained), (1, 0));
    // One comes back and another goes at once.
    let change = replace(&without(&[BASE + 20, BASE + 30]), 4);
    assert_eq!((change.lost, change.gained), (1, 1));
    // A reading that only gained keeps nothing.
    let change = replace(&full, 5);
    assert_eq!((change.lost, change.gained), (0, 2));
    // Two plays of one track in one second are two scrobbles, and one of
    // them going is a loss. Nothing but their number tells them apart.
    let mut twice = full.clone();
    let mut again = twice.entries[0].clone();
    again.marker = 99;
    twice.entries.insert(0, again);
    let change = replace(&twice, 6);
    assert_eq!((change.lost, change.gained), (0, 1));
    let change = replace(&full, 7);
    assert_eq!((change.lost, change.gained), (1, 0));

    let kept: Vec<(u64, u64, u64)> = store
        .connection()
        .prepare("SELECT total, read_at, superseded_at FROM superseded_windows ORDER BY id")
        .unwrap()
        .query_map([], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)))
        .unwrap()
        .map(Result::unwrap)
        .collect();
    assert_eq!(kept, [(6, 1, 2), (5, 2, 3), (4, 3, 4), (7, 6, 7)]);
    assert_eq!(count(&store, "superseded_pages"), 4);
    assert_eq!(count(&store, "scrobbles"), 6);
}
