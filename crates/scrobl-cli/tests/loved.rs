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
        .prepare("SELECT loved_at, track FROM loved ORDER BY id")
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
    let asked: Vec<Option<String>> = server
        .requests()
        .iter()
        .map(|request| request.param("page"))
        .collect();
    assert_eq!(asked, ["1", "2", "3"].map(|page| Some(page.to_owned())));
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
async fn a_list_that_grows_while_it_is_read_is_not_stored() {
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

    changed(result);
    assert_eq!(server.request_count(), 2);
    // The copy from the read before is whole.
    assert_eq!(names(&store), ["Loved 4", "Loved 3", "Loved 2", "Loved 1"]);
}

#[tokio::test]
async fn a_second_page_that_does_not_go_with_the_first_stores_nothing() {
    let five = Dataset::distinct(1).with_loved(5);
    let three = Dataset::distinct(1).with_loved(3);
    for (dataset, second) in [
        // Two tracks were unloved: the total is lower.
        (&five, page_body(&three, 2, 2)),
        // A page that is not full, with the end of the list still to come.
        (&five, page_body(&five, 2, 3)),
        // More rows than the total allows.
        (&three, page_body(&three, 2, 1)),
    ] {
        let scratch = Scratch::new();
        let mut store = store(&scratch);
        let server = FakeLastfm::start(dataset.clone())
            .await
            .script([Behaviour::Normal, Behaviour::status(200, second)]);

        let result = loved(&client(&server), &mut store, USER, 2).await;

        changed(result);
        assert_eq!(server.request_count(), 2);
        assert!(tracks(&store).is_empty());
    }
}

#[tokio::test]
async fn a_swap_between_two_pages_goes_unnoticed_until_the_next_read() {
    let scratch = Scratch::new();
    let mut store = store(&scratch);
    let dataset = Dataset::distinct(1).with_loved(4);

    // After the first page the oldest track is unloved and a new one is
    // loved. The total is the same, and the second page starts with the
    // last row of the first. Nothing here looks for that: this pins what
    // the read does not catch, so that catching it is a decision.
    let faults = Faults {
        swap_loved_from_page: Some(2),
        ..Faults::default()
    };
    let swapped = dataset.clone().with_faults(faults);
    let (_, result) = run(&swapped, &mut store, 2).await;

    assert_eq!(result.unwrap(), 4);
    assert_eq!(names(&store), ["Loved 4", "Loved 3", "Loved 3", "Loved 2"]);

    // The list as it is after the swap, read at one go by the next sync.
    let after = Faults {
        swap_loved_from_page: Some(1),
        ..Faults::default()
    };
    let (_, result) = run(&dataset.with_faults(after), &mut store, 2).await;

    assert_eq!(result.unwrap(), 4);
    assert_eq!(names(&store), ["Loved 5", "Loved 4", "Loved 3", "Loved 2"]);
}

#[tokio::test]
async fn the_list_ends_where_its_rows_add_up_whatever_total_pages_says() {
    // `totalPages` far too large, and too small to reach the end.
    for (total_pages, loved_tracks, requests) in
        [(10_000, 0, 1), (10_000, 4, 2), (10_000, 3, 2), (1, 4, 2)]
    {
        let scratch = Scratch::new();
        let mut store = store(&scratch);
        let faults = Faults {
            loved_total_pages: Some(total_pages),
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
async fn a_page_for_another_user_is_refused() {
    let scratch = Scratch::new();
    let mut store = store(&scratch);
    let faults = Faults {
        wrong_user_on_page: Some(2),
        ..Faults::default()
    };
    let dataset = Dataset::distinct(1).with_loved(4).with_faults(faults);

    let (_, result) = run(&dataset, &mut store, 2).await;

    let Err(error @ Error::Service(_)) = result else {
        panic!("expected a service error, got {result:?}");
    };
    assert!(error.to_string().contains("another user"), "{error}");
    assert!(tracks(&store).is_empty());
}

#[tokio::test]
async fn a_failed_call_is_a_lastfm_error_and_keeps_the_list_before() {
    let scratch = Scratch::new();
    let mut store = store(&scratch);
    let dataset = Dataset::distinct(1).with_loved(4);
    run(&dataset, &mut store, 2).await.1.unwrap();

    let faults = Faults {
        api_error: Some((Some(2), 8)),
        ..Faults::default()
    };
    let (_, result) = run(&dataset.clone().with_faults(faults), &mut store, 2).await;

    assert!(matches!(result, Err(Error::Lastfm(_))), "{result:?}");
    assert_eq!(names(&store), ["Loved 4", "Loved 3", "Loved 2", "Loved 1"]);
}

#[tokio::test]
async fn the_loved_tracks_of_another_users_database_are_not_asked_for() {
    let scratch = Scratch::new();
    let mut other = Store::open(scratch.db()).unwrap();
    other.bind_user("someone-else").unwrap();

    let (server, result) = run(&Dataset::distinct(1).with_loved(3), &mut other, 2).await;

    assert!(matches!(result, Err(Error::OtherUser)), "{result:?}");
    assert_eq!(server.request_count(), 0);
    assert!(tracks(&other).is_empty());
}

#[tokio::test]
async fn a_read_ties_a_new_database_to_its_user() {
    let scratch = Scratch::new();
    let mut new = Store::open(scratch.db()).unwrap();
    assert_eq!(new.user().unwrap(), None);

    let (_, result) = run(&Dataset::distinct(1).with_loved(3), &mut new, 2).await;

    assert_eq!(result.unwrap(), 3);
    assert_eq!(new.user().unwrap().as_deref(), Some(USER));
    assert_eq!(tracks(&new).len(), 3);
}
