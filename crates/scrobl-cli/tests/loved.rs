//! Reading the loved tracks from the fake service into a database.
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
use scrobl_cli::Error;
use scrobl_cli::store::Store;
use scrobl_cli::sync::loved;

/// A database that belongs to [`USER`].
fn store(scratch: &Scratch) -> Store {
    let mut store = Store::open(scratch.db()).unwrap();
    store.bind_user(USER).unwrap();
    store
}

/// One read of `dataset`'s loved tracks into `store`.
async fn run(
    dataset: &Dataset,
    store: &mut Store,
    page_size: u32,
) -> (FakeLastfm, Result<u64, Error>) {
    let server = FakeLastfm::start(dataset.clone()).await;
    let result = loved(&client(&server), store, USER, page_size).await;
    (server, result)
}

/// The first page of `dataset`'s loved tracks, as an answer to script.
fn first_page(dataset: &Dataset, page_size: u32) -> Behaviour {
    Behaviour::status(200, page_body(dataset, page_size, 1))
}

/// The body of one page of `dataset`'s loved tracks.
fn page_body(dataset: &Dataset, page_size: u32, number: u32) -> String {
    let params = [
        ("method", "user.getLovedTracks".to_owned()),
        ("user", USER.to_owned()),
        ("api_key", "k".to_owned()),
        ("limit", page_size.to_string()),
        ("page", number.to_string()),
    ]
    .map(|(name, value)| (name.to_owned(), value));
    let (status, body) = respond_to(dataset, &params);
    assert_eq!(status, 200);
    String::from_utf8(body).unwrap()
}

/// A read of four tracks, two to a page, whose first page is answered
/// with `again` when it is asked for the second time.
async fn run_with_second_look(store: &mut Store, again: Behaviour) -> Result<u64, Error> {
    let dataset = Dataset::distinct(1).with_loved(4);
    let server =
        FakeLastfm::start(dataset)
            .await
            .script([Behaviour::Normal, Behaviour::Normal, again]);
    loved(&client(&server), store, USER, 2).await
}

/// The stored body of a page.
fn kept_body(store: &Store, page: u32) -> String {
    let body: Vec<u8> = store
        .connection()
        .query_row(
            "SELECT body FROM loved_pages WHERE page = ?1",
            [page],
            |row| row.get(0),
        )
        .unwrap();
    String::from_utf8(body).unwrap()
}

fn changed(result: Result<u64, Error>) {
    match result {
        Err(error @ Error::Service(_)) => {
            assert!(error.to_string().contains("changed while"), "{error}");
        }
        other => panic!("expected a service error, got {other:?}"),
    }
}

/// The stored tracks, in the order the service listed them.
fn tracks(store: &Store) -> Vec<(u64, String)> {
    store
        .connection()
        .prepare("SELECT loved_at, track FROM loved ORDER BY page, position")
        .unwrap()
        .query_map([], |row| Ok((row.get(0)?, row.get(1)?)))
        .unwrap()
        .map(Result::unwrap)
        .collect()
}

fn names(store: &Store) -> Vec<String> {
    tracks(store).into_iter().map(|(_, name)| name).collect()
}

#[tokio::test]
async fn the_whole_list_is_read_page_after_page() {
    let scratch = Scratch::new();
    let mut store = store(&scratch);
    let dataset = Dataset::distinct(1).with_loved(5);

    let (server, result) = run(&dataset, &mut store, 2).await;

    assert_eq!(result.unwrap(), 5);
    // Three pages, and the first again to see that it held still.
    assert_eq!(server.request_count(), 4);
    assert_eq!(server.requests()[3].param("page").as_deref(), Some("1"));
    assert_eq!(
        tracks(&store),
        [
            (BASE + 500, "Loved 5".to_owned()),
            (BASE + 400, "Loved 4".to_owned()),
            (BASE + 300, "Loved 3".to_owned()),
            (BASE + 200, "Loved 2".to_owned()),
            (BASE + 100, "Loved 1".to_owned()),
        ]
    );
    // Each stored body is what the server would send for that page again.
    for (request, page) in server.requests().iter().take(3).zip(1..) {
        assert_eq!(
            request.param("page").as_deref(),
            Some(page.to_string().as_str())
        );
        let (_, sent) = respond_to(&dataset, &request.params());
        let kept: Vec<u8> = store
            .connection()
            .query_row(
                "SELECT body FROM loved_pages WHERE page = ?1",
                [page],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(kept, sent);
    }
}

#[tokio::test]
async fn a_list_that_fits_one_page_is_one_request() {
    let scratch = Scratch::new();
    let mut store = store(&scratch);

    let (server, result) = run(&Dataset::distinct(1).with_loved(4), &mut store, 4).await;
    assert_eq!(result.unwrap(), 4);
    assert_eq!(server.request_count(), 1);

    let (server, result) = run(&Dataset::distinct(1), &mut store, 4).await;
    assert_eq!(result.unwrap(), 0);
    assert_eq!(server.request_count(), 1);
    assert!(tracks(&store).is_empty());
}

#[tokio::test]
async fn a_later_read_drops_what_was_unloved() {
    let scratch = Scratch::new();
    let mut store = store(&scratch);
    run(&Dataset::distinct(1).with_loved(5), &mut store, 2)
        .await
        .1
        .unwrap();

    let (_, result) = run(&Dataset::distinct(1).with_loved(2), &mut store, 2).await;

    assert_eq!(result.unwrap(), 2);
    assert_eq!(names(&store), ["Loved 2", "Loved 1"]);
}

#[tokio::test]
async fn a_list_that_changes_while_it_is_read_is_not_stored() {
    let scratch = Scratch::new();
    let mut store = store(&scratch);
    let dataset = Dataset::distinct(1).with_loved(4);
    run(&dataset, &mut store, 2).await.1.unwrap();

    // A track is loved after the first page was sent: the second page
    // repeats a row of the first, and the total is one higher.
    let faults = Faults {
        love_from_page: Some(2),
        ..Faults::default()
    };
    let (server, result) = run(&dataset.clone().with_faults(faults), &mut store, 2).await;

    match result {
        Err(error @ Error::Service(_)) => {
            assert!(error.to_string().contains("changed while"), "{error}");
        }
        other => panic!("expected a service error, got {other:?}"),
    }
    assert_eq!(server.request_count(), 2);
    // The copy from the read before is whole.
    assert_eq!(names(&store), ["Loved 4", "Loved 3", "Loved 2", "Loved 1"]);
}

#[tokio::test]
async fn a_page_for_another_user_is_refused() {
    let scratch = Scratch::new();
    let mut store = store(&scratch);
    let faults = Faults {
        wrong_user_on_page: Some(2),
        ..Faults::default()
    };
    let dataset = Dataset::distinct(1).with_loved(4).with_faults(faults);

    let (_, result) = run(&dataset, &mut store, 2).await;

    assert!(matches!(result, Err(Error::Service(_))), "{result:?}");
    assert!(tracks(&store).is_empty());
}

#[tokio::test]
async fn a_failed_call_is_a_lastfm_error_and_stores_nothing() {
    let scratch = Scratch::new();
    let mut store = store(&scratch);
    let faults = Faults {
        api_error: Some((Some(2), 8)),
        ..Faults::default()
    };
    let dataset = Dataset::distinct(1).with_loved(4).with_faults(faults);

    let (_, result) = run(&dataset, &mut store, 2).await;

    assert!(matches!(result, Err(Error::Lastfm(_))), "{result:?}");
    assert!(tracks(&store).is_empty());
}

#[tokio::test]
async fn a_track_swapped_for_another_while_the_list_is_read_is_noticed() {
    let scratch = Scratch::new();
    let mut store = store(&scratch);
    let dataset = Dataset::distinct(1).with_loved(4);
    run(&dataset, &mut store, 2).await.1.unwrap();

    // After the first page the oldest track is unloved and a new one is
    // loved. The total is the same, and the second page starts with the
    // last row of the first.
    let faults = Faults {
        swap_loved_from_page: Some(2),
        ..Faults::default()
    };
    let (server, result) = run(&dataset.clone().with_faults(faults), &mut store, 2).await;

    match result {
        Err(error @ Error::Service(_)) => {
            assert!(error.to_string().contains("changed while"), "{error}");
        }
        other => panic!("expected a service error, got {other:?}"),
    }
    assert_eq!(server.request_count(), 2);
    assert_eq!(names(&store), ["Loved 4", "Loved 3", "Loved 2", "Loved 1"]);
}

#[tokio::test]
async fn the_list_ends_where_its_rows_add_up_whatever_total_pages_says() {
    for (loved_tracks, requests) in [(0, 1), (4, 3), (3, 3)] {
        let scratch = Scratch::new();
        let mut store = store(&scratch);
        let faults = Faults {
            loved_total_pages: Some(10_000),
            ..Faults::default()
        };
        let dataset = Dataset::distinct(1)
            .with_loved(loved_tracks)
            .with_faults(faults);

        let (server, result) = run(&dataset, &mut store, 2).await;

        assert_eq!(result.unwrap(), loved_tracks as u64);
        assert_eq!(server.request_count(), requests);
        assert_eq!(tracks(&store).len(), loved_tracks);
    }
}

#[tokio::test]
async fn a_total_pages_that_is_too_small_does_not_end_the_list_early() {
    let scratch = Scratch::new();
    let mut store = store(&scratch);
    // `totalPages` says 1, but the first page holds 2 of 4.
    let faults = Faults {
        loved_total_pages: Some(1),
        ..Faults::default()
    };
    let dataset = Dataset::distinct(1).with_loved(4).with_faults(faults);

    let (server, result) = run(&dataset, &mut store, 2).await;

    assert_eq!(result.unwrap(), 4);
    assert_eq!(server.request_count(), 3);
    assert_eq!(tracks(&store).len(), 4);
}

#[tokio::test]
async fn a_swap_on_a_page_already_read_shows_when_the_first_page_is_read_again() {
    let scratch = Scratch::new();
    let mut store = store(&scratch);
    let dataset = Dataset::distinct(1).with_loved(4);
    run(&dataset, &mut store, 2).await.1.unwrap();

    // After both pages were sent, "Loved 3" on the first is unloved and a
    // new track is loved. The second page would be the same again, the
    // total is the same and nothing repeats: only the first page differs.
    let after = Faults {
        swap_loved_from_page: Some(1),
        ..Faults::default()
    };
    let again = first_page(&dataset.clone().with_faults(after), 2);
    let server =
        FakeLastfm::start(dataset)
            .await
            .script([Behaviour::Normal, Behaviour::Normal, again]);
    let result = loved(&client(&server), &mut store, USER, 2).await;

    changed(result);
    assert_eq!(server.request_count(), 3);
    assert_eq!(names(&store), ["Loved 4", "Loved 3", "Loved 2", "Loved 1"]);
}

#[tokio::test]
async fn a_first_page_that_reads_differently_the_second_time_stores_nothing() {
    let dataset = Dataset::distinct(1).with_loved(4);
    // One more track, and a page that claims to be someone else's.
    let grown = Faults {
        love_from_page: Some(1),
        ..Faults::default()
    };
    let theirs = Faults {
        wrong_user_on_page: Some(1),
        ..Faults::default()
    };
    for (faults, is_change) in [(grown, true), (theirs, false)] {
        let scratch = Scratch::new();
        let mut store = store(&scratch);
        let again = first_page(&dataset.clone().with_faults(faults), 2);
        let server = FakeLastfm::start(dataset.clone()).await.script([
            Behaviour::Normal,
            Behaviour::Normal,
            again,
        ]);

        let result = loved(&client(&server), &mut store, USER, 2).await;

        if is_change {
            changed(result);
        } else {
            assert!(matches!(result, Err(Error::Service(_))), "{result:?}");
        }
        assert!(tracks(&store).is_empty());
    }
}

#[tokio::test]
async fn the_second_look_must_be_the_first_page_and_nothing_else() {
    let dataset = Dataset::distinct(1).with_loved(4);
    for (again, message) in [
        // The second page, where the first was asked for.
        (page_body(&dataset, 2, 2), "not the one asked for"),
        // Pages of another size.
        (page_body(&dataset, 3, 1), "not the one asked for"),
        // The same tracks, the newest of them loved a second later: it
        // was unloved and loved again.
        (
            page_body(&dataset, 2, 1).replace(
                &format!(r#""uts":"{}""#, BASE + 400),
                &format!(r#""uts":"{}""#, BASE + 401),
            ),
            "changed while",
        ),
    ] {
        assert_ne!(again, page_body(&dataset, 2, 1));
        let scratch = Scratch::new();
        let mut store = store(&scratch);

        let result = run_with_second_look(&mut store, Behaviour::status(200, again)).await;

        let Err(error @ Error::Service(_)) = result else {
            panic!("expected a service error, got {result:?}");
        };
        assert!(error.to_string().contains(message), "{error}");
        assert!(tracks(&store).is_empty());
    }
}

#[tokio::test]
async fn a_second_look_that_fails_keeps_the_list_before() {
    let scratch = Scratch::new();
    let mut store = store(&scratch);
    run(&Dataset::distinct(1).with_loved(3), &mut store, 2)
        .await
        .1
        .unwrap();

    let result = run_with_second_look(&mut store, Behaviour::api_error(8)).await;

    assert!(matches!(result, Err(Error::Lastfm(_))), "{result:?}");
    assert_eq!(names(&store), ["Loved 3", "Loved 2", "Loved 1"]);
}

#[tokio::test]
async fn what_is_stored_is_the_first_reading_of_the_first_page() {
    let scratch = Scratch::new();
    let mut store = store(&scratch);
    let dataset = Dataset::distinct(1).with_loved(4);
    // The same rows, in bytes that differ where no check looks.
    let first = page_body(&dataset, 2, 1);
    let again = first.replace(r##""#text":"synthetic""##, r##""#text":"another""##);
    assert_ne!(again, first);

    let result = run_with_second_look(&mut store, Behaviour::status(200, again)).await;

    assert_eq!(result.unwrap(), 4);
    assert_eq!(kept_body(&store, 1), first);
}

#[tokio::test]
async fn a_track_listed_twice_with_two_times_is_refused() {
    let scratch = Scratch::new();
    let mut store = store(&scratch);
    let dataset = Dataset::distinct(1).with_loved(2);
    // One page that names the same track on both rows, loved at two times.
    let Behaviour::Reply { status, body, .. } = first_page(&dataset, 2) else {
        unreachable!()
    };
    let body = String::from_utf8(body)
        .unwrap()
        .replace("Loved 1", "Loved 2");
    assert_eq!(body.matches("Loved 2").count(), 2);
    let server = FakeLastfm::start(dataset)
        .await
        .script([Behaviour::status(status, body)]);

    let result = loved(&client(&server), &mut store, USER, 2).await;

    changed(result);
    assert!(tracks(&store).is_empty());
}
