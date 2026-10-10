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
    assert_eq!(server.request_count(), 3);
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
    for (request, page) in server.requests().iter().zip(1..) {
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
