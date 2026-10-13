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
use scrobl_cli::time;

/// One read of `dataset`'s loved tracks into `store`.
async fn run(dataset: &Dataset, store: &mut Store) -> (FakeLastfm, Result<u64, Error>) {
    let server = FakeLastfm::start(dataset.clone()).await;
    let result = loved(&client(&server), store, USER).await;
    (server, result)
}

/// The stored tracks, in the order they were written: `(loved_at, track)`.
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

/// The newest `n` of the fake's loved tracks, as Last.fm lists them.
fn listed(n: u64) -> Vec<(u64, String)> {
    (1..=n)
        .rev()
        .map(|n| (BASE + 100 * n, format!("Loved {n}")))
        .collect()
}

#[tokio::test]
async fn the_whole_list_is_read_page_after_page_and_stored_in_its_order() {
    let scratch = Scratch::new();
    let mut store = Store::open(scratch.db()).unwrap();
    // Two pages of a thousand, and one track more.
    let dataset = Dataset::distinct(1).with_loved(2_001);
    let started = time::now();

    let (server, result) = run(&dataset, &mut store).await;

    assert_eq!(result.unwrap(), 2_001);
    let asked: Vec<Option<String>> = server
        .requests()
        .iter()
        .map(|request| request.param("page"))
        .collect();
    assert_eq!(asked, ["1", "2", "3"].map(|page| Some(page.to_owned())));
    assert_eq!(tracks(&store), listed(2_001));

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
    // The row as it was sent, whatever the columns leave out.
    let raw: serde_json::Value = serde_json::from_str(&raw).unwrap();
    assert_eq!(raw["name"], "Loved 2001");
    assert_eq!(raw["url"], "https://example.invalid/music/Artist/_/Loved");

    let read_at: String = store
        .connection()
        .query_row(
            "SELECT value FROM meta WHERE key = 'loved_read_at'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert!(read_at.parse::<u64>().unwrap() >= started);
}

#[tokio::test]
async fn a_later_read_replaces_the_list() {
    let scratch = Scratch::new();
    let mut store = Store::open(scratch.db()).unwrap();
    run(&Dataset::distinct(1).with_loved(5), &mut store)
        .await
        .1
        .unwrap();

    // Three were unloved since.
    let (server, result) = run(&Dataset::distinct(1).with_loved(2), &mut store).await;

    assert_eq!(result.unwrap(), 2);
    assert_eq!(server.request_count(), 1);
    assert_eq!(tracks(&store), listed(2));

    // And none at all is a list too: one page with no rows.
    let (server, result) = run(&Dataset::distinct(1), &mut store).await;

    assert_eq!(result.unwrap(), 0);
    assert_eq!(server.request_count(), 1);
    assert!(tracks(&store).is_empty());
}

#[tokio::test]
async fn a_read_that_fails_part_way_keeps_the_list_before() {
    let scratch = Scratch::new();
    let mut store = Store::open(scratch.db()).unwrap();
    let dataset = Dataset::distinct(1).with_loved(1_001);
    run(&dataset, &mut store).await.1.unwrap();

    // The second page is answered with an error.
    let faults = Faults {
        api_error: Some((Some(2), 8)),
        ..Faults::default()
    };
    let (server, result) = run(&dataset.clone().with_faults(faults), &mut store).await;

    assert!(matches!(result, Err(Error::Lastfm(_))), "{result:?}");
    assert_eq!(server.request_count(), 2);
    assert_eq!(tracks(&store), listed(1_001));
}
