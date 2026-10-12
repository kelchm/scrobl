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

/// A sync that seals everything it reads, as every sync did before there
/// was a tail.
fn options(span: u64, page_size: u32) -> Options {
    let mut options = sealing();
    options.span = span;
    options.page_size = page_size;
    options
}

/// The defaults, sealing everything at once.
fn sealing() -> Options {
    let mut options = Options::default();
    options.settle = 0;
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
            sealed_to: cutoff,
            added: 50,
            stored: 50,
            sealed: 50,
            reported: 50,
            lost: 0,
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

    let (_server, result, _) = run(&dataset, &mut store, BASE + 100, sealing()).await;
    result.unwrap();

    type Row = (
        u64,
        String,
        Option<String>,
        String,
        Option<String>,
        Option<String>,
    );
    let rows: Vec<Row> = store
        .connection()
        .prepare(
            "SELECT timestamp, artist, artist_mbid, track, album, album_mbid
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
            ))
        })
        .unwrap()
        .map(Result::unwrap)
        .collect();

    let album = Some("Synthetic Album".to_owned());
    let row = |ts, track: &str| {
        (
            ts,
            "Synthetic Artist".to_owned(),
            Some(support::ARTIST_MBID.to_owned()),
            track.to_owned(),
            album.clone(),
            None,
        )
    };
    // The artist's mbid is only sent in the plain form, so its being here
    // says which form was asked for. An empty mbid is stored as NULL.
    assert_eq!(rows, [row(BASE + 20, "Track 0"), row(BASE + 10, "Track 1")]);
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
            scrobbles: 10,
            sealed: true,
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
    let (_server, result, _) = run(&dataset, &mut store, cutoff, sealing()).await;
    result.unwrap();

    let (server, result, stored) = run(&dataset, &mut store, cutoff, sealing()).await;

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

    let (_server, result, _) = run(&dataset, &mut store, BASE + 500, sealing()).await;

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
    let (_server, result, _) = run(&Dataset::distinct(20), &mut store, cutoff, sealing()).await;
    result.unwrap();

    // The same history with one more scrobble in the middle of it.
    let mut times: Vec<u64> = (1..=20).map(|i| BASE + 10 * i).collect();
    times.push(BASE + 55);
    let (_server, result, stored) = run(
        &Dataset::from_timestamps(times),
        &mut store,
        cutoff,
        sealing(),
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

    let (server, result, _) = run(&Dataset::distinct(3), &mut store, 0, sealing()).await;

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
        sealing(),
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
    let (_server, result, _) = run(&Dataset::distinct(5), &mut store, cutoff, sealing()).await;
    result.unwrap();

    // Nothing is left to read, so the one request is the count, and its
    // answer names someone else.
    let dataset = Dataset::distinct(5).with_faults(Faults {
        wrong_user_on_page: Some(1),
        ..Faults::default()
    });
    let (server, result, _) = run(&dataset, &mut store, cutoff, sealing()).await;

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
async fn a_cutoff_before_what_is_stored_is_refused_before_anything_is_asked() {
    let scratch = Scratch::new();
    let mut store = Store::open(scratch.db()).unwrap();
    let dataset = Dataset::distinct(20);
    let (_server, result, _) = run(&dataset, &mut store, BASE + 205, sealing()).await;
    result.unwrap();

    // The clock has gone back: the file covers a time that is still to
    // come, and nothing a sync could do with that is safe.
    let (server, result, stored) = run(&dataset, &mut store, BASE + 101, sealing()).await;

    let Err(Error::Service(message)) = result else {
        panic!("expected a service error, got {result:?}");
    };
    assert!(message.contains("check the clock"), "{message}");
    assert_eq!(stored, []);
    assert_eq!(server.request_count(), 0);
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

#[tokio::test]
async fn a_database_of_another_user_is_refused_before_anything_is_asked() {
    let scratch = Scratch::new();
    let mut store = Store::open(scratch.db()).unwrap();
    store.bind_user("someone-else").unwrap();

    let (server, result, _) = run(&Dataset::distinct(3), &mut store, BASE + 100, sealing()).await;

    assert!(matches!(result, Err(Error::OtherUser)), "{result:?}");
    assert_eq!(server.request_count(), 0);
    assert_eq!(windows(&store), []);
    assert_eq!(store.user().unwrap().as_deref(), Some("someone-else"));
}

#[tokio::test]
async fn a_sync_ties_a_new_database_to_its_user() {
    let scratch = Scratch::new();
    let mut store = Store::open(scratch.db()).unwrap();

    let (_server, result, _) = run(&Dataset::distinct(3), &mut store, BASE + 100, sealing()).await;

    result.unwrap();
    assert_eq!(store.user().unwrap().as_deref(), Some(USER));
}

/// A database holding all of `dataset` before `cutoff`, and the question a
/// sync asks to count it.
async fn synced(dataset: &Dataset, store: &mut Store, cutoff: u64) -> Vec<(String, String)> {
    let (server, result, _) = run(dataset, store, cutoff, sealing()).await;
    result.unwrap();
    server.requests().last().unwrap().params()
}

#[tokio::test]
async fn a_count_of_none_that_comes_with_a_scrobble_is_refused() {
    let scratch = Scratch::new();
    let mut store = Store::open(scratch.db()).unwrap();
    let cutoff = BASE + 100;
    let dataset = Dataset::distinct(5);
    let question = synced(&dataset, &mut store, cutoff).await;

    // The answer to the count holds a scrobble and says there are none.
    let (_, body) = respond_to(&dataset, &question);
    let body = String::from_utf8(body)
        .unwrap()
        .replace(r#""total":"5""#, r#""total":"0""#)
        .replace(r#""totalPages":"5""#, r#""totalPages":"0""#);
    assert!(body.contains(r#""total":"0""#) && body.contains(r#""totalPages":"0""#));
    let server = FakeLastfm::start(dataset)
        .await
        .script([Behaviour::status(200, body)]);
    let result = sync(
        &client(&server),
        &mut store,
        USER,
        cutoff,
        sealing(),
        |_| {},
    )
    .await;

    let Err(Error::Service(message)) = result else {
        panic!("expected a service error, got {result:?}");
    };
    assert!(message.contains("counted no scrobbles"), "{message}");
    assert_eq!(server.request_count(), 1);
}

#[tokio::test]
async fn a_count_that_comes_with_a_scrobble_from_outside_the_period_is_refused() {
    let scratch = Scratch::new();
    let mut store = Store::open(scratch.db()).unwrap();
    let cutoff = BASE + 100;
    synced(&Dataset::distinct(10), &mut store, cutoff).await;

    // The service treats `to` as inclusive, and a scrobble sits exactly on
    // the cutoff.
    let dataset = Dataset::distinct(10).with_faults(Faults {
        inclusive_to: true,
        ..Faults::default()
    });
    let (server, result, _) = run(&dataset, &mut store, cutoff, sealing()).await;

    let Err(Error::Service(message)) = result else {
        panic!("expected a service error, got {result:?}");
    };
    assert!(message.contains("outside the period"), "{message}");
    assert_eq!(server.request_count(), 1);
}

// The tail: the newest history, which every sync reads again.

const DAY: u64 = 86_400;

/// The defaults with windows sealed after `settle` seconds.
fn settling(settle: u64) -> Options {
    let mut options = Options::default();
    options.settle = settle;
    options
}

/// One scrobble a day at noon, for the `days` days before the day `today`
/// is in, oldest first.
fn daily(today: u64, days: u64) -> Vec<u64> {
    let midnight = today / DAY * DAY;
    (1..=days)
        .rev()
        .map(|ago| midnight - ago * DAY + DAY / 2)
        .collect()
}

/// `dataset` without the scrobbles at `times`. The ones that stay keep
/// their rows, which a dataset made afresh from fewer times would not.
fn without(dataset: &Dataset, times: &[u64]) -> Dataset {
    let mut dataset = dataset.clone();
    dataset.entries.retain(|entry| !times.contains(&entry.ts));
    dataset
}

/// Every window as `(from, to, total, sealed)`, oldest first.
fn layout(store: &Store) -> Vec<(u64, u64, u64, bool)> {
    let mut statement = store
        .connection()
        .prepare("SELECT from_ts, to_ts, total, sealed FROM windows ORDER BY from_ts")
        .unwrap();
    let rows = statement
        .query_map([], |row| {
            Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?))
        })
        .unwrap();
    rows.map(Result::unwrap).collect()
}

fn count(store: &Store, table: &str) -> u64 {
    store
        .connection()
        .query_row(&format!("SELECT count(*) FROM {table}"), [], |row| {
            row.get(0)
        })
        .unwrap()
}

/// The windows lie end to end from 0 to `cutoff`, the sealed ones first.
fn assert_whole(store: &Store, cutoff: u64) {
    let layout = layout(store);
    let mut at = 0;
    let mut open = false;
    for (from, to, _, sealed) in &layout {
        assert_eq!(*from, at, "{layout:?}");
        assert!(
            !(open && *sealed),
            "a sealed window after the tail: {layout:?}"
        );
        open |= !sealed;
        at = *to;
    }
    assert_eq!(at, cutoff, "{layout:?}");
}

#[tokio::test]
async fn the_newest_history_is_stored_as_a_tail_and_the_rest_is_sealed() {
    let scratch = Scratch::new();
    let mut store = Store::open(scratch.db()).unwrap();
    let cutoff = BASE + 500;
    let midnight = cutoff / DAY * DAY;
    let dataset = Dataset::from_timestamps(daily(cutoff, 20));

    let (_server, result, stored) = run(&dataset, &mut store, cutoff, settling(3 * DAY)).await;

    // Three days back from the cutoff, to the midnight before.
    let edge = midnight - 3 * DAY;
    let report = result.unwrap();
    assert_eq!(report.sealed_to, edge);
    assert_eq!((report.added, report.stored), (20, 20));
    // Seventeen noons lie before the edge, three after it.
    assert_eq!((report.sealed, report.reported), (17, 17));
    assert!(report.matches());
    assert_eq!(report.lost, 0);
    assert_whole(&store, cutoff);
    assert_eq!(layout(&store).last(), Some(&(edge, cutoff, 3, false)));
    assert_eq!(store.sealed_to().unwrap(), edge);
    let tail = store.tail().unwrap().unwrap();
    assert_eq!((tail.from, tail.to), (edge, cutoff));
    // The tail is stored first, so the newest history is there whatever
    // becomes of the rest.
    assert_eq!(
        stored.first(),
        Some(&Stored {
            from: edge,
            to: cutoff,
            scrobbles: 3,
            sealed: false
        })
    );
    assert!(stored.iter().skip(1).all(|window| window.sealed));
    assert_eq!(markers(&store), dataset.expected(Window::before(cutoff)));
}

#[tokio::test]
async fn a_scrobble_that_arrives_late_is_there_after_the_next_sync() {
    let scratch = Scratch::new();
    let mut store = Store::open(scratch.db()).unwrap();
    let first = BASE + 500;
    let edge = first / DAY * DAY - 3 * DAY;
    // A day on, a device that was offline has sent two plays from inside
    // the old tail, and there is a new one from today.
    let second = first + DAY;
    let late = [edge + 100, edge + DAY + 100, second - 50];
    let mut times = daily(first, 20);
    times.extend(late);
    let dataset = Dataset::from_timestamps(times);
    let (_server, result, _) = run(
        &without(&dataset, &late),
        &mut store,
        first,
        settling(3 * DAY),
    )
    .await;
    result.unwrap();

    let (_server, result, stored) = run(&dataset, &mut store, second, settling(3 * DAY)).await;

    let report = result.unwrap();
    assert_eq!((report.added, report.stored), (3, 23));
    assert_eq!(report.lost, 0);
    // The edge moved on a day, and the day it passed was sealed with the
    // late play in it.
    assert_eq!(report.sealed_to, edge + DAY);
    assert_eq!((report.sealed, report.reported), (19, 19));
    assert!(report.matches());
    assert_eq!(
        stored,
        [
            Stored {
                from: edge,
                to: edge + DAY,
                scrobbles: 2,
                sealed: true
            },
            Stored {
                from: edge + DAY,
                to: second,
                scrobbles: 4,
                sealed: false
            },
        ]
    );
    assert_whole(&store, second);
    assert_eq!(markers(&store), dataset.expected(Window::before(second)));
    // Nothing was lost, so nothing was kept aside.
    assert_eq!(count(&store, "superseded_windows"), 0);
}

#[tokio::test]
async fn a_tail_that_has_not_changed_is_read_again_and_keeps_nothing_aside() {
    let scratch = Scratch::new();
    let mut store = Store::open(scratch.db()).unwrap();
    let cutoff = BASE + 500;
    let dataset = Dataset::from_timestamps(daily(cutoff, 20));
    run(&dataset, &mut store, cutoff, settling(3 * DAY))
        .await
        .1
        .unwrap();
    let before = (layout(&store), timestamps(&store));

    let (server, result, stored) = run(&dataset, &mut store, cutoff + 60, settling(3 * DAY)).await;

    let report = result.unwrap();
    assert_eq!((report.added, report.stored, report.lost), (0, 20, 0));
    // One page of the tail, and the count.
    assert_eq!(server.request_count(), 2);
    assert_eq!(stored.len(), 1);
    assert_eq!(timestamps(&store), before.1);
    assert_eq!(layout(&store).len(), before.0.len());
    assert_whole(&store, cutoff + 60);
    assert_eq!(count(&store, "superseded_windows"), 0);
}

#[tokio::test]
async fn a_scrobble_last_fm_no_longer_lists_goes_and_the_pages_that_had_it_are_kept() {
    let scratch = Scratch::new();
    let mut store = Store::open(scratch.db()).unwrap();
    let first = BASE + 500;
    let times = daily(first, 20);
    let dataset = Dataset::from_timestamps(times.clone());
    run(&dataset, &mut store, first, settling(3 * DAY))
        .await
        .1
        .unwrap();
    let old_tail: Vec<Vec<u8>> = store
        .connection()
        .prepare(
            "SELECT body FROM pages WHERE window_id IN
                 (SELECT id FROM windows WHERE sealed = 0) ORDER BY window_id, page",
        )
        .unwrap()
        .query_map([], |row| row.get(0))
        .unwrap()
        .map(Result::unwrap)
        .collect();

    // The newest play was deleted on the website.
    let deleted = *times.last().unwrap();
    let (_server, result, _) = run(
        &without(&dataset, &[deleted]),
        &mut store,
        first + 60,
        settling(3 * DAY),
    )
    .await;

    let report = result.unwrap();
    assert_eq!(report.lost, 1);
    assert_eq!((report.added, report.stored), (0, 19));
    assert!(report.matches());
    // The table to query follows Last.fm.
    assert!(!timestamps(&store).contains(&deleted));
    // The reading that had it is still in the file, byte for byte.
    assert_eq!(count(&store, "superseded_windows"), 1);
    let kept: Vec<Vec<u8>> = store
        .connection()
        .prepare("SELECT body FROM superseded_pages ORDER BY window_id, page")
        .unwrap()
        .query_map([], |row| row.get(0))
        .unwrap()
        .map(Result::unwrap)
        .collect();
    assert_eq!(kept, old_tail);
    let (from, to, total): (u64, u64, u64) = store
        .connection()
        .query_row(
            "SELECT from_ts, to_ts, total FROM superseded_windows",
            [],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )
        .unwrap();
    assert_eq!((to, total), (first, 3));
    assert_eq!(from, first / DAY * DAY - 3 * DAY);
}

#[tokio::test]
async fn a_tail_that_is_disturbed_while_it_is_read_is_read_again() {
    let scratch = Scratch::new();
    let mut store = Store::open(scratch.db()).unwrap();
    let cutoff = BASE + 500;
    let dataset = Dataset::from_timestamps(daily(cutoff, 6));
    let mut options = settling(30 * DAY);
    options.page_size = 2;
    // Long enough that the tail is one window, asked for without a question
    // about where its scrobbles are.
    options.span = 365 * DAY;

    // The first answer is the first page of a longer history: what the
    // tail looked like a moment before a scrobble arrived. The pages after
    // it disagree with it.
    let mut longer = daily(cutoff, 6);
    longer.push(cutoff - 10);
    let question = [
        ("method", "user.getRecentTracks".to_owned()),
        ("user", USER.to_owned()),
        ("api_key", "k".to_owned()),
        ("limit", "2".to_owned()),
        ("to", cutoff.to_string()),
    ]
    .map(|(name, value)| (name.to_owned(), value));
    let (_, body) = respond_to(&Dataset::from_timestamps(longer), &question);
    let server = FakeLastfm::start(dataset.clone())
        .await
        .script([Behaviour::status(200, body)]);
    let result = sync(&client(&server), &mut store, USER, cutoff, options, |_| {}).await;

    let report = result.unwrap();
    assert_eq!((report.added, report.stored), (6, 6));
    assert_eq!(markers(&store), dataset.expected(Window::before(cutoff)));
    assert_whole(&store, cutoff);
}

#[tokio::test]
async fn a_tail_that_never_holds_still_fails_and_the_one_before_stays() {
    let scratch = Scratch::new();
    let mut store = Store::open(scratch.db()).unwrap();
    let cutoff = BASE + 500;
    let dataset = Dataset::from_timestamps(daily(cutoff, 6));
    let mut options = settling(30 * DAY);
    options.page_size = 2;
    options.span = 365 * DAY;
    run(&dataset, &mut store, cutoff, options).await.1.unwrap();
    let before = (layout(&store), timestamps(&store));

    // A scrobble arrives during every reading.
    let restless = dataset.clone().with_faults(Faults {
        arrive_from_page: Some(2),
        ..Faults::default()
    });
    let (server, result, stored) = run(&restless, &mut store, cutoff + 60, options).await;

    let Err(Error::Lastfm(error)) = result else {
        panic!("expected a Last.fm error, got {result:?}");
    };
    assert_eq!(error.kind(), ErrorKind::Inconsistent);
    assert_eq!(stored, []);
    // Three readings, each stopped at its second page.
    assert_eq!(server.request_count(), 6);
    assert_eq!((layout(&store), timestamps(&store)), before);
}

#[tokio::test]
async fn a_tail_left_for_a_long_time_is_read_once_more_before_it_is_sealed() {
    let scratch = Scratch::new();
    let mut store = Store::open(scratch.db()).unwrap();
    let first = BASE + 500;
    let old_edge = first / DAY * DAY - 3 * DAY;
    // Forty days without a sync. A play from inside the old tail arrived
    // in the meantime, and the listening went on.
    let second = first + 40 * DAY;
    let mut after = vec![old_edge + 100];
    after.extend(daily(second, 30));
    let mut times = daily(first, 10);
    times.extend(&after);
    let dataset = Dataset::from_timestamps(times);
    run(
        &without(&dataset, &after),
        &mut store,
        first,
        settling(3 * DAY),
    )
    .await
    .1
    .unwrap();

    let (_server, result, _) = run(&dataset, &mut store, second, settling(3 * DAY)).await;

    let report = result.unwrap();
    let edge = second / DAY * DAY - 3 * DAY;
    assert_eq!(report.sealed_to, edge);
    assert_eq!((report.added, report.stored, report.lost), (31, 41, 0));
    assert!(report.matches());
    assert_whole(&store, second);
    // The old tail is one sealed window now, with the late play in it.
    assert!(layout(&store).contains(&(old_edge, first, 4, true)));
    assert_eq!(layout(&store).last(), Some(&(edge, second, 3, false)));
    assert_eq!(markers(&store), dataset.expected(Window::before(second)));
}

#[tokio::test]
async fn a_sync_that_seals_at_once_leaves_no_tail() {
    let scratch = Scratch::new();
    let mut store = Store::open(scratch.db()).unwrap();
    let first = BASE + 500;
    let dataset = Dataset::from_timestamps(daily(first, 10));
    run(&dataset, &mut store, first, settling(3 * DAY))
        .await
        .1
        .unwrap();
    assert!(store.tail().unwrap().is_some());

    let second = first + 60;
    let (_server, result, _) = run(&dataset, &mut store, second, sealing()).await;

    let report = result.unwrap();
    assert_eq!(report.sealed_to, second);
    assert_eq!((report.sealed, report.reported), (10, 10));
    assert_eq!(store.tail().unwrap(), None);
    assert_eq!(store.sealed_to().unwrap(), second);
    assert_whole(&store, second);
}

#[tokio::test]
async fn a_longer_settle_does_not_open_what_is_sealed() {
    let scratch = Scratch::new();
    let mut store = Store::open(scratch.db()).unwrap();
    let first = BASE + 500;
    let dataset = Dataset::from_timestamps(daily(first, 10));
    run(&dataset, &mut store, first, sealing()).await.1.unwrap();

    let second = first + 60;
    let (_server, result, stored) = run(&dataset, &mut store, second, settling(5 * DAY)).await;

    let report = result.unwrap();
    // Everything up to the first sync stays sealed; the tail is what came
    // after it.
    assert_eq!(report.sealed_to, first);
    assert_eq!(
        stored,
        [Stored {
            from: first,
            to: second,
            scrobbles: 0,
            sealed: false
        }]
    );
    assert_whole(&store, second);
}

#[tokio::test]
async fn an_answer_that_cannot_be_acted_on_while_the_tail_is_read_is_asked_again() {
    let scratch = Scratch::new();
    let mut store = Store::open(scratch.db()).unwrap();
    let cutoff = BASE + 500;
    let dataset = Dataset::from_timestamps(daily(cutoff, 40));
    // The tail is longer than a window, so its reading begins by asking
    // where the newest scrobble is. The first answer counts none and holds
    // one, as an answer made while a scrobble arrived might.
    let question = [
        ("method", "user.getRecentTracks".to_owned()),
        ("user", USER.to_owned()),
        ("api_key", "k".to_owned()),
        ("limit", "1".to_owned()),
        ("to", cutoff.to_string()),
    ]
    .map(|(name, value)| (name.to_owned(), value));
    let (_, body) = respond_to(&dataset, &question);
    let body = String::from_utf8(body)
        .unwrap()
        .replace(r#""total":"40""#, r#""total":"0""#)
        .replace(r#""totalPages":"40""#, r#""totalPages":"0""#);
    assert!(body.contains(r#""total":"0""#));
    let server = FakeLastfm::start(dataset.clone())
        .await
        .script([Behaviour::status(200, body)]);

    let result = sync(
        &client(&server),
        &mut store,
        USER,
        cutoff,
        settling(60 * DAY),
        |_| {},
    )
    .await;

    let report = result.unwrap();
    assert_eq!((report.added, report.stored), (40, 40));
    assert_whole(&store, cutoff);
}

#[tokio::test]
async fn a_sync_that_another_overtook_stores_nothing_of_its_tail() {
    let scratch = Scratch::new();
    let mut store = Store::open(scratch.db()).unwrap();
    let first = BASE + 500;
    let dataset = Dataset::from_timestamps(daily(first, 10));
    run(&dataset, &mut store, first, settling(3 * DAY))
        .await
        .1
        .unwrap();

    // A sync begins and reads the tail. Before it stores what it read,
    // another one, started later, finishes: the answer to the first sync's
    // request is held back until then.
    let edge = first / DAY * DAY - 3 * DAY;
    let question = [
        ("method", "user.getRecentTracks".to_owned()),
        ("user", USER.to_owned()),
        ("api_key", "k".to_owned()),
        ("limit", "200".to_owned()),
        ("from", edge.to_string()),
        ("to", (first + 60).to_string()),
    ]
    .map(|(name, value)| (name.to_owned(), value));
    let (_, body) = respond_to(&dataset, &question);
    let (release, until) = tokio::sync::watch::channel(false);
    let slow = FakeLastfm::start(dataset.clone())
        .await
        .script([Behaviour::Held { until, body }]);
    let mut other = Store::open(scratch.db()).unwrap();
    let slow_client = client(&slow);
    let overtaken = sync(
        &slow_client,
        &mut store,
        USER,
        first + 60,
        settling(3 * DAY),
        |_| {},
    );
    let overtaking = async {
        slow.wait_for_requests(1).await;
        let (_server, result, _) = run(&dataset, &mut other, first + 120, settling(3 * DAY)).await;
        result.unwrap();
        release.send(true).unwrap();
    };
    let (result, ()) = tokio::join!(overtaken, overtaking);

    // The first sync is refused for its stale reading, and the newer tail
    // is the one in the file.
    let Err(Error::Database(message)) = result else {
        panic!("expected a database error, got {result:?}");
    };
    assert!(message.contains("another sync"), "{message}");
    assert_eq!(other.covered_to().unwrap(), first + 120);
    assert_whole(&other, first + 120);
}
