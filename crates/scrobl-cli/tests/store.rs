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
use scrobl::ApiKey;
use scrobl::history::{RecentTracks, ScanPage, ScanSummary, Window};
use scrobl::protocol::{self, Credentials, HttpResponse};
use scrobl_cli::Error;
use scrobl_cli::store::Store;

/// A finished scan of `window`, driven without a socket.
fn scan(dataset: &Dataset, window: Window, page_size: u32) -> (ScanSummary, Vec<ScanPage>) {
    let credentials = Credentials::new(ApiKey::new("SENTINEL_API_KEY_0001"));
    let mut scan = RecentTracks::new(USER)
        .window(window)
        .limit(page_size)
        .extended(true)
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
        .pragma_update(None, "user_version", 2)
        .unwrap();

    let message = database_error(Store::open(scratch.db()));
    assert!(message.contains("another version"));
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
    let mut store = Store::open(scratch.db()).unwrap();
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
    let mut store = Store::open(scratch.db()).unwrap();
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
    let mut store = Store::open(scratch.db()).unwrap();
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
    let mut store = Store::open(scratch.db()).unwrap();
    let (summary, pages) = scan(&Dataset::distinct(3), Window::since(BASE), 4);

    let message = database_error(store.commit_window(&summary, &pages, 0));
    assert!(message.contains("upper bound"));
    assert_eq!(count(&store, "windows"), 0);
}
