//! The database on its own: what it accepts as its own file, whose history
//! it holds, and what replacing a span of it does.
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
use rusqlite::OptionalExtension;
use scrobl::history::{RecentTracks, ScanPage, ScanSummary, Window};
use scrobl::model::LovedTracksPage;
use scrobl::protocol::{self, Credentials, HttpResponse, Request, methods};
use scrobl::{ApiKey, Response};
use scrobl_cli::Error;
use scrobl_cli::store::{Change, Store};

/// A database that belongs to [`USER`].
fn bound(scratch: &Scratch) -> Store {
    let mut store = Store::open(scratch.db()).unwrap();
    store.bind_user(USER).unwrap();
    store
}

/// A finished scan of `window`, driven without a socket.
fn scan(dataset: &Dataset, window: Window, page_size: u32) -> (ScanSummary, Vec<ScanPage>) {
    scan_by(dataset, window, page_size, "Synthetic Artist")
}

/// A finished scan of `window` of a history in which every play is by
/// `artist`.
fn scan_by(
    dataset: &Dataset,
    window: Window,
    page_size: u32,
    artist: &str,
) -> (ScanSummary, Vec<ScanPage>) {
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
        let body = String::from_utf8(body)
            .unwrap()
            .replace("Synthetic Artist", artist);
        let raw = protocol::decode(&request, HttpResponse::new(status, body)).unwrap();
        pages.push(scan.accept(raw).unwrap());
    }
    (scan.finish().unwrap(), pages)
}

/// Reads `window` of `dataset` and puts it in place of what `store` holds
/// for it.
fn replace(store: &mut Store, dataset: &Dataset, window: Window, now: u64) -> Change {
    let (summary, pages) = scan(dataset, window, 4);
    store.replace(&summary, &pages, now).unwrap()
}

fn count(store: &Store, table: &str) -> u64 {
    store
        .connection()
        .query_row(&format!("SELECT count(*) FROM {table}"), [], |row| {
            row.get(0)
        })
        .unwrap()
}

/// The rows of `table` as `(id, timestamp, track)`, in the order of
/// listening.
fn rows(store: &Store, table: &str) -> Vec<(i64, u64, String)> {
    store
        .connection()
        .prepare(&format!(
            "SELECT id, timestamp, track FROM {table} ORDER BY timestamp, id"
        ))
        .unwrap()
        .query_map([], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)))
        .unwrap()
        .map(Result::unwrap)
        .collect()
}

/// The times of the rows of `table`, in the order of listening.
fn times(store: &Store, table: &str) -> Vec<u64> {
    rows(store, table).into_iter().map(|row| row.1).collect()
}

/// Every time in `dataset`, oldest first.
fn dataset_times(dataset: &Dataset) -> Vec<u64> {
    let mut times: Vec<u64> = dataset.entries.iter().map(|entry| entry.ts).collect();
    times.sort_unstable();
    times
}

fn meta(store: &Store, key: &str) -> Option<String> {
    store
        .connection()
        .query_row("SELECT value FROM meta WHERE key = ?1", [key], |row| {
            row.get(0)
        })
        .optional()
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
    assert_eq!(store.count(0, u64::from(u32::MAX)).unwrap(), 0);
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
fn a_file_that_is_no_database_is_refused() {
    let scratch = Scratch::new();
    std::fs::write(scratch.db(), b"this is not a database, only a text file").unwrap();
    assert!(matches!(Store::open(scratch.db()), Err(Error::Sqlite(_))));
}

/// What a file of version 2 held, in short: the history as windows of pages.
const VERSION_2: &str = "
CREATE TABLE meta (key TEXT PRIMARY KEY, value TEXT NOT NULL) STRICT;
CREATE TABLE windows (
    id INTEGER PRIMARY KEY, from_ts INTEGER NOT NULL, to_ts INTEGER NOT NULL,
    total INTEGER NOT NULL, pages INTEGER NOT NULL, page_size INTEGER NOT NULL,
    read_at INTEGER NOT NULL
) STRICT;
CREATE TABLE pages (
    window_id INTEGER NOT NULL REFERENCES windows (id), page INTEGER NOT NULL,
    body BLOB NOT NULL, PRIMARY KEY (window_id, page)
) STRICT;
CREATE TABLE scrobbles (
    id INTEGER PRIMARY KEY, window_id INTEGER NOT NULL REFERENCES windows (id),
    page INTEGER NOT NULL, position INTEGER NOT NULL, timestamp INTEGER NOT NULL,
    artist TEXT NOT NULL, artist_mbid TEXT, track TEXT NOT NULL, track_mbid TEXT,
    album TEXT, album_mbid TEXT
) STRICT;
INSERT INTO meta VALUES ('user', 'rj_Synthetic');
INSERT INTO windows VALUES (1, 0, 1700000100, 1, 1, 200, 1700000200);
INSERT INTO pages VALUES (1, 1, x'7b7d');
INSERT INTO scrobbles VALUES
    (1, 1, 1, 0, 1700000010, 'Artist', NULL, 'Track', NULL, 'Album', NULL);
PRAGMA application_id = 0x7363626c;
PRAGMA user_version = 2;
";

#[test]
fn a_database_of_another_version_is_refused_and_left_alone() {
    // Versions 1 and 2 kept the history as pages, 3 was never merged, and 5
    // is yet to come.
    for (version, says) in [
        (1, "earlier version"),
        (2, "earlier version"),
        (3, "earlier version"),
        (5, "later version"),
    ] {
        let scratch = Scratch::new();
        let old = rusqlite::Connection::open(scratch.db()).unwrap();
        old.execute_batch(VERSION_2).unwrap();
        old.pragma_update(None, "user_version", version).unwrap();
        drop(old);
        let before = std::fs::read(scratch.db()).unwrap();

        let message = database_error(Store::open(scratch.db()));

        assert!(message.contains(says), "{version}: {message}");
        assert_eq!(std::fs::read(scratch.db()).unwrap(), before);
    }
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
fn a_span_is_stored_as_it_was_read() {
    let scratch = Scratch::new();
    let mut store = bound(&scratch);
    let dataset = Dataset::distinct(10);
    let (summary, pages) = scan(&dataset, Window::before(BASE + 1_000), 4);

    let change = store.replace(&summary, &pages, 42).unwrap();

    assert_eq!(
        change,
        Change {
            added: 10,
            removed: 0
        }
    );
    assert_eq!(count(&store, "scrobbles"), 10);
    assert_eq!(count(&store, "removed"), 0);
    // `from` counts and `to` does not.
    assert_eq!(store.count(0, BASE + 1_000).unwrap(), 10);
    assert_eq!(store.count(BASE + 10, BASE + 51).unwrap(), 5);
    assert_eq!(store.count(BASE + 11, BASE + 50).unwrap(), 3);

    type Row = (
        u64,
        String,
        Option<String>,
        String,
        Option<String>,
        Option<String>,
        Option<String>,
        String,
    );
    let kept: Vec<Row> = store
        .connection()
        .prepare(
            "SELECT timestamp, artist, artist_mbid, track, track_mbid, album, album_mbid, raw
             FROM scrobbles ORDER BY timestamp DESC, id DESC",
        )
        .unwrap()
        .query_map([], |row| {
            Ok((
                row.get(0)?,
                row.get(1)?,
                row.get(2)?,
                row.get(3)?,
                row.get(4)?,
                row.get(5)?,
                row.get(6)?,
                row.get(7)?,
            ))
        })
        .unwrap()
        .map(Result::unwrap)
        .collect();
    let read: Vec<_> = pages.iter().flat_map(|page| page.scrobbles()).collect();
    assert_eq!(kept.len(), read.len());
    for (kept, read) in kept.iter().zip(read) {
        let (timestamp, artist, artist_mbid, track, track_mbid, album, album_mbid, raw) = kept;
        assert_eq!(*timestamp, read.timestamp());
        assert_eq!(artist, "Synthetic Artist");
        assert_eq!(artist_mbid.as_deref(), Some(support::ARTIST_MBID));
        assert_eq!(track, read.track().name());
        assert_eq!(track_mbid.as_deref(), read.track().mbid());
        assert_eq!(album.as_deref(), Some("Synthetic Album"));
        // An empty mbid is stored as NULL.
        assert_eq!(*album_mbid, None);
        // The row as it was sent, whatever the columns leave out.
        let raw: serde_json::Value = serde_json::from_str(raw).unwrap();
        assert_eq!(&raw, read.json());
        assert_eq!(raw["url"], "https://example.invalid/music/Artist/_/Track");
    }
    // And it can be asked as JSON where it lies.
    let url: String = store
        .connection()
        .query_row(
            "SELECT json_extract(raw, '$.url') FROM scrobbles LIMIT 1",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(url, "https://example.invalid/music/Artist/_/Track");
}

#[test]
fn rows_are_numbered_in_the_order_of_listening() {
    let scratch = Scratch::new();
    let mut store = bound(&scratch);
    // Three scrobbles in one second, between two others. Last.fm lists the
    // newest first, so of those three the one it lists last came first.
    let dataset =
        Dataset::from_timestamps(vec![BASE + 90, BASE + 50, BASE + 50, BASE + 50, BASE + 10]);
    let listed = dataset.expected(Window::before(BASE + 100));

    replace(&mut store, &dataset, Window::before(BASE + 100), 0);

    let markers: Vec<usize> = store
        .connection()
        .prepare("SELECT track_mbid FROM scrobbles ORDER BY timestamp, id")
        .unwrap()
        .query_map([], |row| row.get::<_, Option<String>>(0))
        .unwrap()
        .map(|mbid| support::marker_of(mbid.unwrap().as_deref()))
        .collect();
    let oldest_first: Vec<usize> = listed.into_iter().rev().collect();
    assert_eq!(markers, oldest_first);
}

#[test]
fn the_same_reading_again_changes_nothing() {
    let scratch = Scratch::new();
    let mut store = bound(&scratch);
    let dataset = Dataset::distinct(10);
    let window = Window::before(BASE + 1_000);
    replace(&mut store, &dataset, window, 1);
    let before = times(&store, "scrobbles");

    for now in [2, 3] {
        let change = replace(&mut store, &dataset, window, now);
        assert_eq!(change, Change::default());
    }

    assert_eq!(times(&store, "scrobbles"), before);
    assert_eq!(count(&store, "removed"), 0);
}

#[test]
fn a_reading_replaces_its_own_span_and_nothing_else() {
    let scratch = Scratch::new();
    let mut store = bound(&scratch);
    let dataset = Dataset::distinct(10);
    replace(&mut store, &dataset, Window::before(BASE + 1_000), 1);
    let before = rows(&store, "scrobbles");

    // The middle of it, read again: BASE + 40 up to and not including
    // BASE + 70.
    let middle = Window::new(BASE + 40, BASE + 70).unwrap();
    let change = replace(&mut store, &dataset, middle, 2);

    assert_eq!(change, Change::default());
    let after = rows(&store, "scrobbles");
    assert_eq!(times(&store, "scrobbles"), dataset_times(&dataset));
    // The rows on either side are the rows they were.
    let outside = |rows: &[(i64, u64, String)]| -> Vec<(i64, u64, String)> {
        rows.iter()
            .filter(|row| !middle.contains(row.1))
            .cloned()
            .collect()
    };
    assert_eq!(outside(&after), outside(&before));
    assert_eq!(outside(&after).len(), 7);
}

#[test]
fn a_scrobble_the_reading_lacks_is_moved_to_removed() {
    let scratch = Scratch::new();
    let mut store = bound(&scratch);
    let window = Window::before(BASE + 1_000);
    let before = named((1..=10).map(|i| BASE + 10 * i));
    replace(&mut store, &before, window, 100);

    // Two were deleted at Last.fm, and one arrived late.
    let mut left = dataset_times(&before);
    left.retain(|time| ![BASE + 30, BASE + 80].contains(time));
    left.push(BASE + 55);
    let after = named(left);
    let change = replace(&mut store, &after, window, 200);

    assert_eq!(
        change,
        Change {
            added: 1,
            removed: 2
        }
    );
    assert_eq!(times(&store, "scrobbles"), dataset_times(&after));
    assert_eq!(times(&store, "removed"), [BASE + 30, BASE + 80]);
    let (removed_at, track, raw): (u64, String, String) = store
        .connection()
        .query_row(
            "SELECT removed_at, track, raw FROM removed WHERE timestamp = ?1",
            [BASE + 30],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )
        .unwrap();
    assert_eq!((removed_at, track), (200, format!("At {}", BASE + 30)));
    let raw: serde_json::Value = serde_json::from_str(&raw).unwrap();
    assert_eq!(raw["date"]["uts"], (BASE + 30).to_string());

    // It was noticed once: the next reading has nothing more to remove.
    assert_eq!(replace(&mut store, &after, window, 300), Change::default());
    assert_eq!(count(&store, "removed"), 2);
}

#[test]
fn a_scrobble_that_changed_only_in_what_last_fm_says_about_it_is_the_same_one() {
    let scratch = Scratch::new();
    let mut store = bound(&scratch);
    let window = Window::before(BASE + 1_000);
    let before = named((1..=3).map(|i| BASE + 10 * i));
    replace(&mut store, &before, window, 100);

    // Last.fm now gives the newest one another id.
    let mut after = before.clone();
    after.entries[0].mbid = "11111111-1111-4111-8111-111111111111".to_owned();
    let change = replace(&mut store, &after, window, 200);

    assert_eq!(change, Change::default());
    assert_eq!(count(&store, "removed"), 0);
    let mbid: Option<String> = store
        .connection()
        .query_row(
            "SELECT track_mbid FROM scrobbles WHERE timestamp = ?1",
            [BASE + 30],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(
        mbid.as_deref(),
        Some("11111111-1111-4111-8111-111111111111")
    );
}

#[test]
fn a_scrobble_is_told_apart_by_its_time_its_artist_and_its_track() {
    let window = Window::before(BASE + 1_000);
    // One play of one song, and then a reading that has in its place a play
    // that differs from it in one thing only.
    let song = |time: u64| Dataset::from_timestamps(vec![time]).identical_rows();
    for (time, artist, track) in [
        (BASE + 20, "Artist", "Same Song"),
        (BASE + 10, "Other Artist", "Same Song"),
        (BASE + 10, "Artist", "Other Song"),
    ] {
        let scratch = Scratch::new();
        let mut store = bound(&scratch);
        let (summary, pages) = scan_by(&song(BASE + 10), window, 4, "Artist");
        store.replace(&summary, &pages, 100).unwrap();

        let mut other = song(time);
        other.entries[0].name = track.to_owned();
        let (summary, pages) = scan_by(&other, window, 4, artist);
        let change = store.replace(&summary, &pages, 200).unwrap();

        // Another scrobble: the one that was there went, and this one came.
        assert_eq!(
            change,
            Change {
                added: 1,
                removed: 1
            },
            "{time} {artist} {track}"
        );
        let row = |table: &str| -> (u64, String, String) {
            store
                .connection()
                .query_row(
                    &format!("SELECT timestamp, artist, track FROM {table}"),
                    [],
                    |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
                )
                .unwrap()
        };
        assert_eq!(
            row("removed"),
            (BASE + 10, "Artist".to_owned(), "Same Song".to_owned())
        );
        assert_eq!(
            row("scrobbles"),
            (time, artist.to_owned(), track.to_owned())
        );
    }
}

#[test]
fn of_scrobbles_that_are_alike_only_as_many_as_went_are_removed() {
    let scratch = Scratch::new();
    let mut store = bound(&scratch);
    let window = Window::before(BASE + 1_000);
    // The same track three times in one second, which a player can send.
    let alike = |n: usize| named(std::iter::repeat_n(BASE + 50, n).chain([BASE + 10]));
    replace(&mut store, &alike(3), window, 100);

    let change = replace(&mut store, &alike(2), window, 200);
    assert_eq!(
        change,
        Change {
            added: 0,
            removed: 1
        }
    );
    assert_eq!(store.count(BASE + 50, BASE + 51).unwrap(), 2);
    assert_eq!(times(&store, "removed"), [BASE + 50]);

    // Four where the file holds two are two added, not four.
    let change = replace(&mut store, &alike(4), window, 300);
    assert_eq!(
        change,
        Change {
            added: 2,
            removed: 0
        }
    );
    assert_eq!(store.count(BASE + 50, BASE + 51).unwrap(), 4);
    assert_eq!(count(&store, "removed"), 1);
}

#[test]
fn a_replacement_that_fails_part_way_leaves_the_span_as_it_was() {
    let scratch = Scratch::new();
    let mut store = bound(&scratch);
    let window = Window::before(BASE + 1_000);
    let before = named((1..=10).map(|i| BASE + 10 * i));
    replace(&mut store, &before, window, 100);
    let kept = rows(&store, "scrobbles");

    // The new reading lacks two scrobbles and has one more. Its write fails
    // at the row for BASE + 60, after the two were set aside, the span was
    // emptied and half of the reading was written.
    store
        .connection()
        .execute_batch(&format!(
            "CREATE TRIGGER stop AFTER INSERT ON scrobbles WHEN NEW.timestamp = {}
             BEGIN SELECT RAISE(ABORT, 'stopped by the test'); END",
            BASE + 60
        ))
        .unwrap();
    let mut left = dataset_times(&before);
    left.retain(|time| ![BASE + 30, BASE + 80].contains(time));
    left.push(BASE + 55);
    let (summary, pages) = scan(&named(left), window, 4);

    let result = store.replace(&summary, &pages, 200);

    assert!(matches!(result, Err(Error::Sqlite(_))), "{result:?}");
    assert_eq!(rows(&store, "scrobbles"), kept);
    assert_eq!(count(&store, "removed"), 0);
}

#[test]
fn a_reading_without_an_upper_bound_is_refused() {
    let scratch = Scratch::new();
    let mut store = bound(&scratch);
    let (summary, pages) = scan(&Dataset::distinct(3), Window::since(BASE), 4);

    let message = database_error(store.replace(&summary, &pages, 0));

    assert!(message.contains("upper bound"));
    assert_eq!(count(&store, "scrobbles"), 0);
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

/// The loved tracks as stored, in the order they were written:
/// `(loved_at, track)`.
fn loved_rows(store: &Store) -> Vec<(u64, String)> {
    store
        .connection()
        .prepare("SELECT loved_at, track FROM loved ORDER BY id")
        .unwrap()
        .query_map([], |row| Ok((row.get(0)?, row.get(1)?)))
        .unwrap()
        .map(Result::unwrap)
        .collect()
}

#[test]
fn loved_tracks_are_stored_in_the_order_last_fm_lists_them() {
    let scratch = Scratch::new();
    let mut store = bound(&scratch);
    let dataset = Dataset::distinct(1).with_loved(3);
    let pages = loved_pages(&dataset, 2);

    store.replace_loved(&pages, 1_800_000_000).unwrap();

    assert_eq!(
        loved_rows(&store),
        [
            (BASE + 300, "Loved 3".to_owned()),
            (BASE + 200, "Loved 2".to_owned()),
            (BASE + 100, "Loved 1".to_owned()),
        ]
    );
    let (artist, artist_mbid, track_mbid, raw): (String, Option<String>, Option<String>, String) =
        store
            .connection()
            .query_row(
                "SELECT artist, artist_mbid, track_mbid, raw FROM loved ORDER BY id LIMIT 1",
                [],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
            )
            .unwrap();
    assert_eq!(artist, "Synthetic Artist");
    assert_eq!(artist_mbid.as_deref(), Some(support::ARTIST_MBID));
    // An empty mbid is stored as NULL.
    assert_eq!(track_mbid, None);
    let raw: serde_json::Value = serde_json::from_str(&raw).unwrap();
    assert_eq!(&raw, pages[0].tracks()[0].json());
    assert_eq!(meta(&store, "loved_read_at").as_deref(), Some("1800000000"));
}

#[test]
fn a_read_of_the_loved_tracks_replaces_the_one_before() {
    let scratch = Scratch::new();
    let mut store = bound(&scratch);
    let before = Dataset::distinct(1).with_loved(5);
    store.replace_loved(&loved_pages(&before, 2), 100).unwrap();
    assert_eq!(count(&store, "loved"), 5);

    // Three were unloved since.
    let after = Dataset::distinct(1).with_loved(2);
    store.replace_loved(&loved_pages(&after, 2), 200).unwrap();

    assert_eq!(
        loved_rows(&store),
        [
            (BASE + 200, "Loved 2".to_owned()),
            (BASE + 100, "Loved 1".to_owned())
        ]
    );
    assert_eq!(meta(&store, "loved_read_at").as_deref(), Some("200"));

    // And none at all is a list too: one page with no rows.
    let none = Dataset::distinct(1);
    store.replace_loved(&loved_pages(&none, 2), 300).unwrap();
    assert_eq!(count(&store, "loved"), 0);
    assert_eq!(meta(&store, "loved_read_at").as_deref(), Some("300"));
}

#[test]
fn a_replacement_of_the_loved_tracks_that_fails_part_way_keeps_the_list_before() {
    let scratch = Scratch::new();
    let mut store = bound(&scratch);
    let before = Dataset::distinct(1).with_loved(3);
    store.replace_loved(&loved_pages(&before, 2), 100).unwrap();
    let kept = loved_rows(&store);

    // The second track's time is more than a column holds: the insert of
    // it fails after the list was emptied and a track of the new one went
    // in.
    let mut after = Dataset::distinct(1).with_loved(4);
    after.loved[1] = u64::MAX;
    let failed = store.replace_loved(&loved_pages(&after, 2), 200);

    assert!(matches!(failed, Err(Error::Sqlite(_))), "{failed:?}");
    assert_eq!(loved_rows(&store), kept);
    assert_eq!(meta(&store, "loved_read_at").as_deref(), Some("100"));
}

#[test]
fn the_file_says_when_its_history_was_last_read() {
    let scratch = Scratch::new();
    let mut store = bound(&scratch);
    assert_eq!(meta(&store, "history_read_at"), None);

    store.note_history_read(100).unwrap();
    store.note_history_read(200).unwrap();

    assert_eq!(meta(&store, "history_read_at").as_deref(), Some("200"));
    // Whose history it is stays as it was.
    assert_eq!(store.user().unwrap().as_deref(), Some(USER));
}
