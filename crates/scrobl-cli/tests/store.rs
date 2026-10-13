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
use scrobl::ApiKey;
use scrobl::history::{RecentTracks, ScanPage, ScanSummary, Window};
use scrobl::protocol::{self, Credentials, HttpResponse};
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
fn a_file_this_version_did_not_make_is_refused_and_left_as_it_was() {
    // An earlier version's file, and some other program's database.
    for marks in [
        "PRAGMA application_id = 0x7363626c; PRAGMA user_version = 2;",
        "",
    ] {
        let scratch = Scratch::new();
        let other = rusqlite::Connection::open(scratch.db()).unwrap();
        other
            .execute_batch(&format!(
                "CREATE TABLE windows (id INTEGER PRIMARY KEY); {marks}"
            ))
            .unwrap();
        drop(other);
        let before = std::fs::read(scratch.db()).unwrap();

        let error = Store::open(scratch.db()).unwrap_err();

        // It says what to do about it.
        assert_eq!(
            error.to_string(),
            "the file was written by another version of this tool, or by another program; sync \
             into a new file"
        );
        assert_eq!(std::fs::read(scratch.db()).unwrap(), before);
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

    let error = store.replace(&summary, &pages, 0).unwrap_err();

    assert!(error.to_string().contains("upper bound"));
    assert_eq!(count(&store, "scrobbles"), 0);
}
