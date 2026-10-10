//! A sync against the fake Last.fm on a real socket: what ends up in the
//! database, what is asked of the service, and what survives a failure.
//!
//! provenance: synthetic. The fake service generates every response.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]

mod common;

use common::*;
use scrobl::history::Window;
use scrobl::{ApiErrorCode, ErrorKind};
use scrobl_cli::Error;
use scrobl_cli::store::Store;
use scrobl_cli::sync::{Options, Report, Stored, sync};

fn options(span: u64, page_size: u32) -> Options {
    let mut options = Options::default();
    options.span = span;
    options.page_size = page_size;
    options
}

/// One sync of `dataset` into `store`, with the server it ran against and
/// the windows it reported, in the order they were stored.
async fn run(
    dataset: &Dataset,
    store: &mut Store,
    cutoff: u64,
    options: Options,
) -> (FakeLastfm, Result<Report, Error>, Vec<Stored>) {
    let server = FakeLastfm::start(dataset.clone()).await;
    let mut stored = Vec::new();
    let result = sync(&client(&server), store, USER, cutoff, options, |window| {
        stored.push(window);
    })
    .await;
    (server, result, stored)
}

/// Every stored timestamp, newest first.
fn timestamps(store: &Store) -> Vec<u64> {
    let mut statement = store
        .connection()
        .prepare("SELECT timestamp FROM scrobbles ORDER BY timestamp DESC, id")
        .unwrap();
    let rows = statement.query_map([], |row| row.get(0)).unwrap();
    rows.map(Result::unwrap).collect()
}

/// The marker of every stored scrobble, newest first.
fn markers(store: &Store) -> Vec<usize> {
    let mut statement = store
        .connection()
        .prepare("SELECT track_mbid FROM scrobbles ORDER BY timestamp DESC, id")
        .unwrap();
    let rows = statement
        .query_map([], |row| row.get::<_, Option<String>>(0))
        .unwrap();
    rows.map(|mbid| support::marker_of(mbid.unwrap().as_deref()))
        .collect()
}

/// Every window as `(from, to, total)`, oldest first.
fn windows(store: &Store) -> Vec<(u64, u64, u64)> {
    let mut statement = store
        .connection()
        .prepare("SELECT from_ts, to_ts, total FROM windows ORDER BY from_ts")
        .unwrap();
    let rows = statement
        .query_map([], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)))
        .unwrap();
    rows.map(Result::unwrap).collect()
}

/// The windows leave no part of `[0, cutoff)` out and none of it twice.
fn assert_covers(store: &Store, cutoff: u64) {
    let mut covered_to = 0;
    for (from, to, _) in windows(store) {
        assert_eq!(from, covered_to, "a gap or an overlap at {from}");
        covered_to = to;
    }
    assert_eq!(covered_to, cutoff);
    assert_eq!(store.gaps(cutoff).unwrap(), []);
}

#[tokio::test]
async fn a_first_sync_stores_the_whole_history_in_windows() {
    let scratch = Scratch::new();
    let mut store = Store::open(scratch.db()).unwrap();
    let dataset = Dataset::distinct(50);
    let cutoff = BASE + 1_000;

    let (server, result, stored) = run(&dataset, &mut store, cutoff, options(100, 4)).await;

    let report = result.unwrap();
    assert_eq!(
        report,
        Report {
            cutoff,
            added: 50,
            stored: 50,
            reported: 50
        }
    );
    assert!(report.matches());
    assert_eq!(markers(&store), dataset.expected(Window::before(cutoff)));
    assert_covers(&store, cutoff);

    // Five windows of ten scrobbles, newest first, then the empty remainder.
    let sizes: Vec<u64> = stored.iter().map(|window| window.scrobbles).collect();
    assert_eq!(sizes, [10, 10, 10, 10, 10, 0]);
    assert_eq!(stored[0].to, cutoff);
    assert_eq!(stored[5].from, 0);
    drop(server);
}

#[tokio::test]
async fn the_pages_are_stored_byte_for_byte() {
    let scratch = Scratch::new();
    let mut store = Store::open(scratch.db()).unwrap();
    let dataset = Dataset::distinct(23);
    let cutoff = BASE + 1_000;

    let (server, result, _) = run(&dataset, &mut store, cutoff, options(100, 4)).await;
    result.unwrap();

    // What the service sent for every page of a scan, in the order asked.
    let sent: Vec<Vec<u8>> = server
        .requests()
        .iter()
        .filter(|request| request.query_param("limit") == Some("4"))
        .map(|request| respond_to(&dataset, &request.query).1)
        .collect();
    let mut statement = store
        .connection()
        .prepare("SELECT body FROM pages ORDER BY window_id, page")
        .unwrap();
    let kept: Vec<Vec<u8>> = statement
        .query_map([], |row| row.get(0))
        .unwrap()
        .map(Result::unwrap)
        .collect();
    assert!(!kept.is_empty());
    assert_eq!(kept, sent);
}

#[tokio::test]
async fn a_row_keeps_what_the_service_said_about_it() {
    let scratch = Scratch::new();
    let mut store = Store::open(scratch.db()).unwrap();
    let dataset = Dataset::distinct(2);

    let (_server, result, _) = run(&dataset, &mut store, BASE + 100, Options::default()).await;
    result.unwrap();

    type Row = (
        u64,
        String,
        Option<String>,
        String,
        Option<String>,
        Option<String>,
        Option<bool>,
    );
    let rows: Vec<Row> = store
        .connection()
        .prepare(
            "SELECT timestamp, artist, artist_mbid, track, album, album_mbid, loved
             FROM scrobbles ORDER BY timestamp DESC",
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
            ))
        })
        .unwrap()
        .map(Result::unwrap)
        .collect();

    let album = Some("Synthetic Album".to_owned());
    let row = |ts, track: &str, loved| {
        (
            ts,
            "Synthetic Artist".to_owned(),
            None,
            track.to_owned(),
            album.clone(),
            None,
            Some(loved),
        )
    };
    // An empty mbid is stored as NULL; the fake loves every odd marker.
    assert_eq!(
        rows,
        [
            row(BASE + 20, "Track 0", false),
            row(BASE + 10, "Track 1", true)
        ]
    );
}

#[tokio::test]
async fn a_sync_that_fails_keeps_its_windows_and_the_next_one_finishes() {
    let scratch = Scratch::new();
    let mut store = Store::open(scratch.db()).unwrap();
    let dataset = Dataset::distinct(50);
    let cutoff = BASE + 1_000;

    // Connection 0 is the first probe, 1 to 3 the first window's pages, 4
    // the second probe; the second window's second page fails.
    let server = FakeLastfm::start(dataset.clone()).await.script([
        Behaviour::Normal,
        Behaviour::Normal,
        Behaviour::Normal,
        Behaviour::Normal,
        Behaviour::Normal,
        Behaviour::Normal,
        Behaviour::html(503),
    ]);
    let error = sync(
        &client(&server),
        &mut store,
        USER,
        cutoff,
        options(100, 4),
        |_| {},
    )
    .await
    .unwrap_err();
    let Error::Lastfm(error) = error else {
        panic!("expected a Last.fm error, got {error:?}");
    };
    assert_eq!(error.kind(), ErrorKind::Http);

    // The first window is whole; nothing of the second was kept.
    assert_eq!(windows(&store), [(BASE + 401, cutoff, 10)]);
    assert_eq!(timestamps(&store).len(), 10);

    let (server, result, stored) = run(&dataset, &mut store, cutoff, options(100, 4)).await;
    assert!(result.unwrap().matches());
    assert_eq!(markers(&store), dataset.expected(Window::before(cutoff)));
    assert_covers(&store, cutoff);
    // It carried on below the window it had, and read nothing twice.
    assert_eq!(stored[0].to, BASE + 401);
    assert!(
        server
            .requests()
            .iter()
            .filter(|request| request.query_param("limit") == Some("4"))
            .all(|request| request.query_param("to") != Some(&cutoff.to_string()))
    );
}

#[tokio::test]
async fn a_later_sync_reads_only_what_is_new() {
    let scratch = Scratch::new();
    let mut store = Store::open(scratch.db()).unwrap();
    let first_cutoff = BASE + 205;
    let (_server, result, _) = run(
        &Dataset::distinct(20),
        &mut store,
        first_cutoff,
        options(1_000, 200),
    )
    .await;
    result.unwrap();

    let dataset = Dataset::distinct(30);
    let cutoff = BASE + 305;
    let (server, result, stored) = run(&dataset, &mut store, cutoff, options(1_000, 200)).await;

    let report = result.unwrap();
    assert_eq!((report.added, report.stored, report.reported), (10, 30, 30));
    assert_eq!(
        stored,
        [Stored {
            from: first_cutoff,
            to: cutoff,
            scrobbles: 10
        }]
    );
    assert_eq!(
        timestamps(&store),
        dataset.expected_timestamps(Window::before(cutoff))
    );
    assert_covers(&store, cutoff);

    // One page of the new window, then the count: nothing else.
    let requests = server.requests();
    assert_eq!(requests.len(), 2);
    assert_eq!(
        requests[0].query_param("from"),
        Some(&*first_cutoff.to_string())
    );
    assert_eq!(requests[0].query_param("to"), Some(&*cutoff.to_string()));
    assert_eq!(requests[1].query_param("limit"), Some("1"));
}

#[tokio::test]
async fn a_sync_with_nothing_to_read_only_counts() {
    let scratch = Scratch::new();
    let mut store = Store::open(scratch.db()).unwrap();
    let dataset = Dataset::distinct(5);
    let cutoff = BASE + 100;
    let (_server, result, _) = run(&dataset, &mut store, cutoff, Options::default()).await;
    result.unwrap();

    let (server, result, stored) = run(&dataset, &mut store, cutoff, Options::default()).await;

    let report = result.unwrap();
    assert_eq!((report.added, report.stored, report.reported), (0, 5, 5));
    assert_eq!(stored, []);
    assert_eq!(server.request_count(), 1);
}

#[tokio::test]
async fn scrobbles_that_share_a_second_are_all_kept() {
    let scratch = Scratch::new();
    let mut store = Store::open(scratch.db()).unwrap();
    // Five identical rows in one second, split over three pages of two.
    let mut times = vec![BASE + 50; 5];
    times.extend([BASE + 10, BASE + 90]);
    let dataset = Dataset::from_timestamps(times).byte_identical();
    let cutoff = BASE + 100;

    let (_server, result, _) = run(&dataset, &mut store, cutoff, options(1_000, 2)).await;

    assert!(result.unwrap().matches());
    let kept = timestamps(&store);
    assert_eq!(kept, dataset.expected_timestamps(Window::before(cutoff)));
    assert_eq!(kept.iter().filter(|&&ts| ts == BASE + 50).count(), 5);
}

#[tokio::test]
async fn a_long_silence_costs_one_request() {
    let scratch = Scratch::new();
    let mut store = Store::open(scratch.db()).unwrap();
    let later = BASE + 300_000_000;
    let mut times: Vec<u64> = (1..=5).map(|i| BASE + 10 * i).collect();
    times.extend((1..=5).map(|i| later + 10 * i));
    let dataset = Dataset::from_timestamps(times);
    let cutoff = later + 1_000_000;

    let (server, result, stored) = run(&dataset, &mut store, cutoff, options(1_000, 200)).await;

    assert!(result.unwrap().matches());
    assert_covers(&store, cutoff);
    assert_eq!(markers(&store), dataset.expected(Window::before(cutoff)));
    let sizes: Vec<u64> = stored.iter().map(|window| window.scrobbles).collect();
    assert_eq!(sizes, [5, 5, 0]);
    // A probe and a page for each of the three windows, and the count.
    assert_eq!(server.request_count(), 7);
}

#[tokio::test]
async fn an_empty_history_is_one_empty_window() {
    let scratch = Scratch::new();
    let mut store = Store::open(scratch.db()).unwrap();
    let cutoff = BASE;

    let (_server, result, _) =
        run(&Dataset::distinct(0), &mut store, cutoff, options(100, 200)).await;

    let report = result.unwrap();
    assert_eq!((report.added, report.stored, report.reported), (0, 0, 0));
    assert!(report.matches());
    assert_eq!(windows(&store), [(0, cutoff, 0)]);
}

#[tokio::test]
async fn the_now_playing_row_is_not_a_scrobble() {
    let scratch = Scratch::new();
    let mut store = Store::open(scratch.db()).unwrap();
    let dataset = Dataset::distinct(5).with_now_playing(true, Pages::All);
    let cutoff = BASE + 100;

    let (_server, result, _) = run(&dataset, &mut store, cutoff, options(1_000, 2)).await;

    assert!(result.unwrap().matches());
    assert_eq!(markers(&store), dataset.expected(Window::before(cutoff)));
}

#[tokio::test]
async fn a_window_that_changes_while_it_is_read_is_not_stored() {
    let scratch = Scratch::new();
    let mut store = Store::open(scratch.db()).unwrap();
    // A scrobble arrives between the first page and the second.
    let dataset = Dataset::distinct(10).with_faults(Faults {
        arrive_from_page: Some(2),
        ..Faults::default()
    });

    let (_server, result, stored) =
        run(&dataset, &mut store, BASE + 5_000, options(10_000, 4)).await;

    let Err(Error::Lastfm(error)) = result else {
        panic!("expected a Last.fm error");
    };
    assert_eq!(error.kind(), ErrorKind::Inconsistent);
    assert_eq!(stored, []);
    assert_eq!(windows(&store), []);
    assert!(timestamps(&store).is_empty());
}

#[tokio::test]
async fn a_refusal_is_an_error_and_never_an_empty_history() {
    let scratch = Scratch::new();
    let mut store = Store::open(scratch.db()).unwrap();
    let dataset = Dataset::distinct(10).with_faults(Faults {
        api_error: Some((None, 17)),
        ..Faults::default()
    });

    let (_server, result, _) = run(&dataset, &mut store, BASE + 500, Options::default()).await;

    let Err(Error::Lastfm(error)) = result else {
        panic!("expected a Last.fm error");
    };
    assert_eq!(error.api_code(), Some(ApiErrorCode::LOGIN_REQUIRED));
    assert_eq!(windows(&store), []);
}

#[tokio::test]
async fn scrobbles_added_to_a_period_already_read_show_in_the_counts() {
    let scratch = Scratch::new();
    let mut store = Store::open(scratch.db()).unwrap();
    let cutoff = BASE + 205;
    let (_server, result, _) = run(
        &Dataset::distinct(20),
        &mut store,
        cutoff,
        Options::default(),
    )
    .await;
    result.unwrap();

    // The same history with one more scrobble in the middle of it.
    let mut times: Vec<u64> = (1..=20).map(|i| BASE + 10 * i).collect();
    times.push(BASE + 55);
    let (_server, result, stored) = run(
        &Dataset::from_timestamps(times),
        &mut store,
        cutoff,
        Options::default(),
    )
    .await;

    let report = result.unwrap();
    assert_eq!((report.added, report.stored, report.reported), (0, 20, 21));
    assert!(!report.matches());
    assert_eq!(stored, []);
}

#[tokio::test]
async fn a_cutoff_at_the_epoch_is_refused_before_anything_is_asked() {
    let scratch = Scratch::new();
    let mut store = Store::open(scratch.db()).unwrap();

    let (server, result, _) = run(&Dataset::distinct(3), &mut store, 0, Options::default()).await;

    assert!(matches!(result, Err(Error::Service(_))));
    assert_eq!(server.request_count(), 0);
}

#[tokio::test]
async fn a_scrobble_from_outside_the_period_asked_for_stops_the_sync() {
    let scratch = Scratch::new();
    let mut store = Store::open(scratch.db()).unwrap();
    // The service treats `to` as inclusive, and a scrobble sits exactly on
    // the cutoff, so the first answer names a scrobble the sync did not ask
    // about.
    let dataset = Dataset::distinct(10).with_faults(Faults {
        inclusive_to: true,
        ..Faults::default()
    });

    let (_server, result, _) = run(&dataset, &mut store, BASE + 100, options(50, 200)).await;

    let Err(Error::Service(message)) = result else {
        panic!("expected a service error");
    };
    assert!(message.contains("outside the period"));
    assert_eq!(windows(&store), []);
}

#[tokio::test]
async fn a_failed_call_is_a_lastfm_error_with_its_cause() {
    use std::error::Error as _;

    let scratch = Scratch::new();
    let mut store = Store::open(scratch.db()).unwrap();
    let server = FakeLastfm::start(Dataset::distinct(3))
        .await
        .then(Behaviour::html(503));

    let error = sync(
        &client(&server),
        &mut store,
        USER,
        BASE + 100,
        Options::default(),
        |_| {},
    )
    .await
    .unwrap_err();

    assert_eq!(error.exit_code(), 1);
    assert!(error.to_string().starts_with("Last.fm: "));
    assert!(error.source().is_some());
    assert_eq!(windows(&store), []);
}

#[tokio::test]
async fn a_count_that_answers_for_another_user_is_refused() {
    let scratch = Scratch::new();
    let mut store = Store::open(scratch.db()).unwrap();
    let cutoff = BASE + 100;
    let (_server, result, _) = run(
        &Dataset::distinct(5),
        &mut store,
        cutoff,
        Options::default(),
    )
    .await;
    result.unwrap();

    // Nothing is left to read, so the one request is the count, and its
    // answer names someone else.
    let dataset = Dataset::distinct(5).with_faults(Faults {
        wrong_user_on_page: Some(1),
        ..Faults::default()
    });
    let (server, result, _) = run(&dataset, &mut store, cutoff, Options::default()).await;

    let Err(Error::Service(message)) = result else {
        panic!("expected a service error");
    };
    assert!(message.contains("not the one asked for"));
    assert_eq!(server.request_count(), 1);
}

/// A sync whose first answer, the question of where the newest scrobble
/// is, has been replaced by `body`.
async fn run_with_first_answer(
    dataset: &Dataset,
    store: &mut Store,
    cutoff: u64,
    body: String,
) -> Result<Report, Error> {
    let server = FakeLastfm::start(dataset.clone())
        .await
        .script([Behaviour::status(200, body)]);
    sync(
        &client(&server),
        store,
        USER,
        cutoff,
        options(100, 4),
        |_| {},
    )
    .await
}

#[tokio::test]
async fn a_wrong_answer_about_where_the_scrobbles_are_loses_nothing() {
    let dataset = Dataset::distinct(50);
    let cutoff = BASE + 1_000;
    let attr = |total: u32| {
        format!(
            r#""@attr": {{"user": "{USER}", "page": "1", "perPage": "1",
                "totalPages": "{total}", "total": "{total}"}}"#
        )
    };
    // The service says the history is empty, which it is not; then that it
    // holds scrobbles but shows only what is playing now.
    let empty = format!(r#"{{"recenttracks": {{"track": [], {}}}}}"#, attr(0));
    let playing = format!(
        r##"{{"recenttracks": {{"track": {{"@attr": {{"nowplaying": "true"}},
            "artist": {{"#text": "A", "mbid": ""}}, "name": "Playing", "mbid": "",
            "album": {{"#text": "", "mbid": ""}}}}, {}}}}}"##,
        attr(50)
    );

    for body in [empty, playing] {
        let scratch = Scratch::new();
        let mut store = Store::open(scratch.db()).unwrap();

        let report = run_with_first_answer(&dataset, &mut store, cutoff, body)
            .await
            .unwrap();

        assert!(report.matches());
        assert_eq!(report.stored, 50);
        assert_eq!(markers(&store), dataset.expected(Window::before(cutoff)));
        assert_covers(&store, cutoff);
    }
}

#[tokio::test]
async fn a_cutoff_before_what_is_stored_reads_nothing_and_counts_up_to_it() {
    let scratch = Scratch::new();
    let mut store = Store::open(scratch.db()).unwrap();
    let dataset = Dataset::distinct(20);
    let (_server, result, _) = run(&dataset, &mut store, BASE + 205, Options::default()).await;
    result.unwrap();

    // The clock has gone back: ten of the twenty scrobbles lie before it.
    let (server, result, stored) = run(&dataset, &mut store, BASE + 101, Options::default()).await;

    let report = result.unwrap();
    assert_eq!((report.added, report.stored, report.reported), (0, 10, 10));
    assert!(report.matches());
    assert_eq!(stored, []);
    assert_eq!(server.request_count(), 1);
    assert_eq!(timestamps(&store).len(), 20);
}

#[tokio::test]
async fn different_scrobbles_in_one_second_are_each_kept_once() {
    let scratch = Scratch::new();
    let mut store = Store::open(scratch.db()).unwrap();
    // Seven scrobbles in one second, told apart only by their markers, over
    // pages of two and windows that end on that second.
    let mut times = vec![BASE + 50; 7];
    times.extend([BASE + 10, BASE + 20, BASE + 90]);
    let dataset = Dataset::from_timestamps(times).identical_rows();
    let cutoff = BASE + 100;

    let (_server, result, _) = run(&dataset, &mut store, cutoff, options(1, 2)).await;

    assert!(result.unwrap().matches());
    let mut kept = markers(&store);
    kept.sort_unstable();
    assert_eq!(kept, (0..10).collect::<Vec<_>>());
    assert_covers(&store, cutoff);
}

#[tokio::test]
async fn a_span_of_zero_still_ends() {
    let scratch = Scratch::new();
    let mut store = Store::open(scratch.db()).unwrap();
    let dataset = Dataset::distinct(3);
    let cutoff = BASE + 35;

    let (_server, result, stored) = run(&dataset, &mut store, cutoff, options(0, 200)).await;

    assert!(result.unwrap().matches());
    assert_covers(&store, cutoff);
    let sizes: Vec<u64> = stored.iter().map(|window| window.scrobbles).collect();
    assert_eq!(sizes, [1, 1, 1, 0]);
}
