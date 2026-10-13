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
use rusqlite::OptionalExtension;
use scrobl::history::Window;
use scrobl::{ApiErrorCode, ErrorKind};
use scrobl_cli::Error;
use scrobl_cli::store::{Change, Store};
use scrobl_cli::sync::{Options, Stored, sync};

const DAY: u64 = 86_400;

/// A sync on a small scale, so that a few dozen scrobbles are a history
/// worth halving: the last hundred seconds are read every time, a scan
/// holds ten scrobbles at most and a page four.
fn options() -> Options {
    Options {
        recent: 100,
        chunk: 10,
        page_size: 4,
        full: false,
    }
}

/// One sync of `dataset` into `store`, with the server it ran against and
/// the spans it reported, in the order they were stored.
async fn run(
    dataset: &Dataset,
    store: &mut Store,
    now: u64,
    options: Options,
) -> (FakeLastfm, Result<Change, Error>, Vec<Stored>) {
    let server = FakeLastfm::start(dataset.clone()).await;
    let mut stored = Vec::new();
    let result = sync(&client(&server), store, USER, now, options, |span| {
        stored.push(span);
    })
    .await;
    (server, result, stored)
}

/// Every timestamp in `table`, newest first.
fn timestamps(store: &Store, table: &str) -> Vec<u64> {
    let mut statement = store
        .connection()
        .prepare(&format!(
            "SELECT timestamp FROM {table} ORDER BY timestamp DESC, id DESC"
        ))
        .unwrap();
    let rows = statement.query_map([], |row| row.get(0)).unwrap();
    rows.map(Result::unwrap).collect()
}

/// The marker of every stored scrobble, in the order Last.fm lists them:
/// newest first.
fn markers(store: &Store) -> Vec<usize> {
    let mut statement = store
        .connection()
        .prepare("SELECT track_mbid FROM scrobbles ORDER BY timestamp DESC, id DESC")
        .unwrap();
    let rows = statement
        .query_map([], |row| row.get::<_, Option<String>>(0))
        .unwrap();
    rows.map(|mbid| support::marker_of(mbid.unwrap().as_deref()))
        .collect()
}

/// Every time in `dataset`, newest first.
fn times(dataset: &Dataset) -> Vec<u64> {
    dataset.entries.iter().map(|entry| entry.ts).collect()
}

/// `dataset` without the scrobbles at `gone` and with one more at each of
/// `late`. The ones that stay keep their names.
fn changed(dataset: &Dataset, gone: &[u64], late: &[u64]) -> Dataset {
    named(
        times(dataset)
            .into_iter()
            .filter(|time| !gone.contains(time))
            .chain(late.iter().copied()),
    )
}

/// When the file says its history was last read.
fn history_read_at(store: &Store) -> Option<String> {
    store
        .connection()
        .query_row(
            "SELECT value FROM meta WHERE key = 'history_read_at'",
            [],
            |row| row.get(0),
        )
        .optional()
        .unwrap()
}

/// The requests that asked for a page of a scan, not for a count.
fn pages_asked(server: &FakeLastfm) -> usize {
    server
        .requests()
        .iter()
        .filter(|request| request.query_param("limit") != Some("1"))
        .count()
}

#[tokio::test]
async fn a_first_sync_stores_the_whole_history_newest_first() {
    let scratch = Scratch::new();
    let mut store = Store::open(scratch.db()).unwrap();
    // Fifty scrobbles ten seconds apart, the newest five seconds ago.
    let dataset = Dataset::distinct(50);
    let now = BASE + 505;

    let (_server, result, stored) = run(&dataset, &mut store, now, options()).await;

    assert_eq!(
        result.unwrap(),
        Change {
            added: 50,
            removed: 0
        }
    );
    assert_eq!(markers(&store), dataset.expected(Window::before(now)));
    assert_eq!(history_read_at(&store), Some(now.to_string()));
    // A new database is tied to the user it was first synced for.
    assert_eq!(store.user().unwrap().as_deref(), Some(USER));

    // The recent part first, then the rest from its newest end backwards,
    // no scan larger than it should be and no time read twice.
    assert_eq!(
        stored[0],
        Stored {
            from: now - 100,
            to: now,
            scrobbles: 10,
            added: 10,
            removed: 0
        }
    );
    assert!(stored.len() > 4, "{stored:?}");
    for (newer, older) in stored.iter().zip(stored.iter().skip(1)) {
        assert!(older.to <= newer.from, "{stored:?}");
    }
    assert!(stored.iter().all(|span| span.scrobbles <= 10));
    assert_eq!(stored.iter().map(|span| span.scrobbles).sum::<u64>(), 50);
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
async fn a_sync_that_fails_keeps_what_it_stored_and_the_next_one_finishes() {
    let scratch = Scratch::new();
    let mut store = Store::open(scratch.db()).unwrap();
    let dataset = Dataset::distinct(50);
    let now = BASE + 505;
    let (whole, ..) = run(
        &dataset,
        &mut Store::open(Scratch::new().db()).unwrap(),
        now,
        options(),
    )
    .await;

    // Last.fm gives out half way through.
    let server = FakeLastfm::start(dataset.clone()).await.script(
        std::iter::repeat_n(Behaviour::Normal, whole.request_count() / 2)
            .chain([Behaviour::html(503)]),
    );
    let mut first = Vec::new();
    let error = sync(&client(&server), &mut store, USER, now, options(), |span| {
        first.push(span);
    })
    .await
    .unwrap_err();
    assert!(error.to_string().starts_with("Last.fm: "), "{error}");
    let Error::Lastfm(error) = error else {
        panic!("expected a Last.fm error, got {error:?}");
    };
    assert_eq!(error.kind(), ErrorKind::Http);

    // What was stored is whole spans, and the file does not claim to have
    // been read through.
    let held = store.count(0, now).unwrap();
    assert!(held > 10 && held < 50, "{held}");
    assert_eq!(held, first.iter().map(|span| span.scrobbles).sum::<u64>());
    assert_eq!(history_read_at(&store), None);

    // The next sync starts a little later, as it would, so it halves the
    // history at other times than the first did.
    let later = now + 7;
    let (server, result, second) = run(&dataset, &mut store, later, options()).await;

    result.unwrap();
    assert_eq!(markers(&store), dataset.expected(Window::before(later)));
    assert_eq!(history_read_at(&store), Some(later.to_string()));
    // Apart from the recent part, which every sync reads, it read again no
    // more of what the first one had stored than the one span the stop cut
    // across.
    let again: u64 = second
        .iter()
        .filter(|span| span.to != later)
        .map(|span| span.scrobbles - span.added)
        .sum();
    assert!(again <= 10, "{second:?}");
    assert!(pages_asked(&server) < pages_asked(&whole));
}

#[tokio::test]
async fn a_later_sync_reads_the_recent_part_and_counts_the_rest() {
    let scratch = Scratch::new();
    let mut store = Store::open(scratch.db()).unwrap();
    let options = Options {
        page_size: 200,
        ..options()
    };
    let first_now = BASE + 205;
    let (_server, result, _) = run(&Dataset::distinct(20), &mut store, first_now, options).await;
    result.unwrap();

    // Ten more scrobbles since, all in the last hundred seconds.
    let dataset = Dataset::distinct(30);
    let now = BASE + 305;
    let (server, result, stored) = run(&dataset, &mut store, now, options).await;

    assert_eq!(
        result.unwrap(),
        Change {
            added: 10,
            removed: 0
        }
    );
    assert_eq!(
        stored,
        [Stored {
            from: now - 100,
            to: now,
            scrobbles: 10,
            added: 10,
            removed: 0
        }]
    );
    assert_eq!(
        timestamps(&store, "scrobbles"),
        dataset.expected_timestamps(Window::before(now))
    );

    // How many are recent, and a page of them; and how many are older, which
    // is as many as the file holds.
    let asked: Vec<(Option<String>, Option<String>, Option<String>)> = server
        .requests()
        .iter()
        .map(|request| {
            (
                request.param("limit"),
                request.param("from"),
                request.param("to"),
            )
        })
        .collect();
    let text = |value: u64| Some(value.to_string());
    assert_eq!(
        asked,
        [
            (text(1), text(now - 100), text(now)),
            (text(200), text(now - 100), text(now)),
            (text(1), None, text(now - 100)),
        ]
    );
}

#[tokio::test]
async fn a_sync_with_nothing_new_changes_nothing() {
    let scratch = Scratch::new();
    let mut store = Store::open(scratch.db()).unwrap();
    let dataset = Dataset::distinct(50);
    let now = BASE + 505;
    run(&dataset, &mut store, now, options()).await.1.unwrap();

    let (server, result, stored) = run(&dataset, &mut store, now + 7, options()).await;

    assert_eq!(result.unwrap(), Change::default());
    // Only the recent part was read, and it held what the file held.
    assert_eq!(stored.len(), 1);
    assert_eq!((stored[0].added, stored[0].removed), (0, 0));
    assert_eq!(server.request_count() - pages_asked(&server), 2);
    assert_eq!(markers(&store), dataset.expected(Window::before(now)));
    assert!(timestamps(&store, "removed").is_empty());
}

#[tokio::test]
async fn scrobbles_that_share_a_second_are_all_kept_however_often_they_are_read() {
    let scratch = Scratch::new();
    let mut store = Store::open(scratch.db()).unwrap();
    // Five identical rows in one second, split over three pages of two.
    let mut all = vec![BASE + 50; 5];
    all.extend([BASE + 10, BASE + 90]);
    let dataset = Dataset::from_timestamps(all).byte_identical();
    let now = BASE + 100;
    let options = Options {
        page_size: 2,
        ..options()
    };

    for added in [7, 0, 0] {
        let (_server, result, _) = run(&dataset, &mut store, now, options).await;

        assert_eq!(result.unwrap(), Change { added, removed: 0 });
        let kept = timestamps(&store, "scrobbles");
        assert_eq!(kept, dataset.expected_timestamps(Window::before(now)));
        assert_eq!(kept.iter().filter(|&&ts| ts == BASE + 50).count(), 5);
    }
}

#[tokio::test]
async fn a_second_that_holds_more_than_a_scan_should_is_read_as_one_scan() {
    let scratch = Scratch::new();
    let mut store = Store::open(scratch.db()).unwrap();
    // Seven scrobbles in one second, told apart only by their markers, where
    // a scan is to hold three.
    let mut all = vec![BASE + 50; 7];
    all.extend([BASE + 10, BASE + 20, BASE + 90]);
    let dataset = Dataset::from_timestamps(all).identical_rows();
    let now = BASE + 100;
    let options = Options {
        chunk: 3,
        page_size: 2,
        ..options()
    };

    // A second cannot be halved, and a sync that tried would never end.
    let (_server, result, stored) = tokio::time::timeout(
        std::time::Duration::from_secs(30),
        run(&dataset, &mut store, now, options),
    )
    .await
    .expect("the sync ends");

    result.unwrap();
    assert_eq!(markers(&store), dataset.expected(Window::before(now)));
    assert!(
        stored.contains(&Stored {
            from: BASE + 50,
            to: BASE + 51,
            scrobbles: 7,
            added: 7,
            removed: 0
        }),
        "{stored:?}"
    );
    assert!(
        stored
            .iter()
            .all(|span| span.scrobbles <= 3 || span.to - span.from == 1)
    );
}

#[tokio::test]
async fn a_long_silence_costs_a_few_counts() {
    let scratch = Scratch::new();
    let mut store = Store::open(scratch.db()).unwrap();
    // Five scrobbles, nine and a half years of nothing, and five more.
    let later = BASE + 300_000_000;
    let mut all: Vec<u64> = (1..=5).map(|i| BASE + 10 * i).collect();
    all.extend((1..=5).map(|i| later + 10 * i));
    let dataset = Dataset::from_timestamps(all);
    let now = later + 1_000_000;
    let options = Options {
        chunk: 5,
        ..options()
    };

    let (server, result, stored) = run(&dataset, &mut store, now, options).await;

    result.unwrap();
    assert_eq!(markers(&store), dataset.expected(Window::before(now)));
    // The recent part, which is empty, and then each end of the silence as
    // one scan. Nothing was read where there is nothing.
    let sizes: Vec<u64> = stored.iter().map(|span| span.scrobbles).collect();
    assert_eq!(sizes, [0, 5, 5]);
    assert_eq!(pages_asked(&server), 5);
    assert!(server.request_count() < 30, "{}", server.request_count());
}

#[tokio::test]
async fn an_empty_history_is_no_error() {
    let scratch = Scratch::new();
    let mut store = Store::open(scratch.db()).unwrap();
    let now = BASE;

    let (server, result, stored) = run(&Dataset::distinct(0), &mut store, now, options()).await;

    assert_eq!(result.unwrap(), Change::default());
    assert_eq!(stored.len(), 1);
    assert_eq!(server.request_count(), 3);
    assert!(timestamps(&store, "scrobbles").is_empty());
}

#[tokio::test]
async fn the_now_playing_row_is_not_a_scrobble() {
    let scratch = Scratch::new();
    let mut store = Store::open(scratch.db()).unwrap();
    let dataset = Dataset::distinct(25).with_now_playing(true, Pages::All);
    let now = BASE + 255;
    let options = Options {
        page_size: 2,
        ..options()
    };

    let (_server, result, _) = run(&dataset, &mut store, now, options).await;

    assert_eq!(result.unwrap().added, 25);
    assert_eq!(markers(&store), dataset.expected(Window::before(now)));
}

#[tokio::test]
async fn a_span_that_is_disturbed_while_it_is_read_is_read_again() {
    let scratch = Scratch::new();
    let mut store = Store::open(scratch.db()).unwrap();
    let dataset = Dataset::distinct(6);
    let now = BASE + 100;
    let options = Options {
        recent: 1_000,
        page_size: 2,
        ..options()
    };

    // The first page of the first reading is the first page of a longer
    // history: what Last.fm held a moment before a scrobble was deleted.
    // The pages after it count one fewer and disagree with it.
    let mut longer = times(&dataset);
    longer.push(BASE + 95);
    let (_, body) = respond_to(
        &Dataset::from_timestamps(longer),
        &question(now - 1_000, now, 2, 1),
    );
    let server = FakeLastfm::start(dataset.clone())
        .await
        .script([Behaviour::Normal, Behaviour::status(200, body)]);
    let result = sync(&client(&server), &mut store, USER, now, options, |_| {}).await;

    assert_eq!(result.unwrap().added, 6);
    assert_eq!(markers(&store), dataset.expected(Window::before(now)));
    // Two readings: one stopped at its second page, and one of three pages.
    assert_eq!(pages_asked(&server), 5);
}

#[tokio::test]
async fn a_span_that_never_holds_still_fails_and_the_file_stays_as_it_was() {
    let scratch = Scratch::new();
    let mut store = Store::open(scratch.db()).unwrap();
    let dataset = Dataset::distinct(6);
    let now = BASE + 5_000;
    let options = Options {
        recent: 10_000,
        page_size: 2,
        ..options()
    };
    run(&dataset, &mut store, now, options).await.1.unwrap();
    let before = markers(&store);

    // A scrobble arrives during every reading.
    let restless = dataset.clone().with_faults(Faults {
        arrive_from_page: Some(2),
        ..Faults::default()
    });
    let (server, result, stored) = run(&restless, &mut store, now + 60, options).await;

    let Err(Error::Lastfm(error)) = result else {
        panic!("expected a Last.fm error, got {result:?}");
    };
    assert_eq!(error.kind(), ErrorKind::Inconsistent);
    assert_eq!(stored, []);
    // Three readings, each stopped at its second page.
    assert_eq!(pages_asked(&server), 6);
    assert_eq!(markers(&store), before);
    assert!(timestamps(&store, "removed").is_empty());
    assert_eq!(history_read_at(&store), Some(now.to_string()));
}

#[tokio::test]
async fn a_refusal_is_an_error_and_never_an_empty_history() {
    let scratch = Scratch::new();
    let mut store = Store::open(scratch.db()).unwrap();
    let dataset = Dataset::distinct(10);
    let now = BASE + 500;
    run(&dataset, &mut store, now, options()).await.1.unwrap();

    // The history was made private: every answer is "login required".
    let refusing = dataset.clone().with_faults(Faults {
        api_error: Some((None, 17)),
        ..Faults::default()
    });
    let (_server, result, stored) = run(&refusing, &mut store, now + 60, options()).await;

    let Err(Error::Lastfm(error)) = result else {
        panic!("expected a Last.fm error");
    };
    assert_eq!(error.api_code(), Some(ApiErrorCode::LOGIN_REQUIRED));
    assert_eq!(stored, []);
    // Nothing went from the file because Last.fm would not say.
    assert_eq!(markers(&store), dataset.expected(Window::before(now)));
    assert!(timestamps(&store, "removed").is_empty());
}

#[tokio::test]
async fn a_database_of_another_user_is_refused_before_anything_is_asked() {
    let scratch = Scratch::new();
    let mut store = Store::open(scratch.db()).unwrap();
    store.bind_user("someone-else").unwrap();

    let (server, result, _) = run(
        &Dataset::distinct(3),
        &mut store,
        BASE + 100,
        Options::default(),
    )
    .await;

    let error = result.unwrap_err();
    assert_eq!(
        error.to_string(),
        "the database holds another user's history; use one file per user"
    );
    assert_eq!(server.request_count(), 0);
    assert!(timestamps(&store, "scrobbles").is_empty());
    assert_eq!(store.user().unwrap().as_deref(), Some("someone-else"));
}

/// A database holding twenty scrobbles ten seconds apart, synced five
/// seconds after the newest, and that history.
async fn synced(store: &mut Store) -> Dataset {
    let dataset = named((1..=20).map(|i| BASE + 10 * i));
    run(&dataset, store, BASE + 205, options()).await.1.unwrap();
    dataset
}

#[tokio::test]
async fn a_scrobble_that_arrives_late_is_there_after_the_next_sync() {
    let scratch = Scratch::new();
    let mut store = Store::open(scratch.db()).unwrap();
    let before = synced(&mut store).await;

    // A device that was offline sends a play from a minute ago.
    let now = BASE + 215;
    let after = changed(&before, &[], &[BASE + 155]);
    let (_server, result, stored) = run(&after, &mut store, now, options()).await;

    assert_eq!(
        result.unwrap(),
        Change {
            added: 1,
            removed: 0
        }
    );
    assert_eq!(timestamps(&store, "scrobbles"), times(&after));
    // It was in the recent part, which is all that was read.
    assert_eq!(stored.len(), 1);
}

#[tokio::test]
async fn a_late_scrobble_in_old_history_is_found_by_its_count() {
    let scratch = Scratch::new();
    let mut store = Store::open(scratch.db()).unwrap();
    let before = synced(&mut store).await;

    // A play from long before the recent part turns up.
    let now = BASE + 215;
    let late = BASE + 55;
    let after = changed(&before, &[], &[late]);
    let (_server, result, stored) = run(&after, &mut store, now, options()).await;

    assert_eq!(
        result.unwrap(),
        Change {
            added: 1,
            removed: 0
        }
    );
    assert_eq!(timestamps(&store, "scrobbles"), times(&after));
    // The recent part, and the one span the halving led to.
    assert_eq!(stored.len(), 2, "{stored:?}");
    let found = stored[1];
    assert!(found.from <= late && late < found.to, "{found:?}");
    assert_eq!((found.added, found.removed), (1, 0));
    assert!(found.scrobbles <= 10);
}

#[tokio::test]
async fn a_scrobble_last_fm_no_longer_lists_is_set_aside_wherever_it_was() {
    let scratch = Scratch::new();
    let mut store = Store::open(scratch.db()).unwrap();
    let before = synced(&mut store).await;

    // One old play and one recent play were deleted on the website.
    let now = BASE + 215;
    let after = changed(&before, &[BASE + 50, BASE + 190], &[]);
    let (_server, result, stored) = run(&after, &mut store, now, options()).await;

    assert_eq!(
        result.unwrap(),
        Change {
            added: 0,
            removed: 2
        }
    );
    assert_eq!(stored.iter().map(|span| span.removed).sum::<u64>(), 2);
    // The table to query follows Last.fm, and what went is kept.
    assert_eq!(timestamps(&store, "scrobbles"), times(&after));
    assert_eq!(timestamps(&store, "removed"), [BASE + 190, BASE + 50]);
    let noticed: Vec<u64> = store
        .connection()
        .prepare("SELECT DISTINCT removed_at FROM removed")
        .unwrap()
        .query_map([], |row| row.get(0))
        .unwrap()
        .map(Result::unwrap)
        .collect();
    assert_eq!(noticed, [now]);

    // The next sync has nothing more to say about them.
    let (_server, result, _) = run(&after, &mut store, now + 60, options()).await;
    assert_eq!(result.unwrap().removed, 0);
    assert_eq!(timestamps(&store, "removed").len(), 2);
}

#[tokio::test]
async fn a_change_that_leaves_the_count_as_it_was_waits_for_a_full_sync() {
    let scratch = Scratch::new();
    let mut store = Store::open(scratch.db()).unwrap();
    let before = synced(&mut store).await;

    // Long before the recent part, one play was deleted and another added.
    let now = BASE + 215;
    let after = changed(&before, &[BASE + 50], &[BASE + 55]);
    let (_server, result, stored) = run(&after, &mut store, now, options()).await;

    // The counts agree, so nothing leads a sync there.
    assert_eq!(result.unwrap(), Change::default());
    assert_eq!(stored.len(), 1);
    let mut stale = times(&before);
    stale.retain(|time| *time < now);
    assert_eq!(timestamps(&store, "scrobbles"), stale);

    // A full sync reads everything, and finds both.
    let full = Options {
        full: true,
        ..options()
    };
    let (_server, result, _) = run(&after, &mut store, now, full).await;

    assert_eq!(
        result.unwrap(),
        Change {
            added: 1,
            removed: 1
        }
    );
    assert_eq!(timestamps(&store, "scrobbles"), times(&after));
    assert_eq!(timestamps(&store, "removed"), [BASE + 50]);
}

#[tokio::test]
async fn two_syncs_at_once_leave_one_copy_of_everything() {
    let scratch = Scratch::new();
    let mut first = Store::open(scratch.db()).unwrap();
    let mut second = Store::open(scratch.db()).unwrap();
    let dataset = Dataset::distinct(50);
    let now = BASE + 505;
    let server = FakeLastfm::start(dataset.clone()).await;
    let client = client(&server);

    // Started seven seconds apart, so they halve the history at different
    // times and their spans overlap.
    let (one, other) = tokio::join!(
        sync(&client, &mut first, USER, now, options(), |_| {}),
        sync(&client, &mut second, USER, now + 7, options(), |_| {}),
    );

    one.unwrap();
    other.unwrap();
    assert_eq!(markers(&first), dataset.expected(Window::before(now)));
    assert!(timestamps(&first, "removed").is_empty());
}

#[tokio::test]
async fn a_clock_that_runs_ahead_leaves_nothing_wrong_behind() {
    let scratch = Scratch::new();
    let mut store = Store::open(scratch.db()).unwrap();
    let before = named((1..=20).map(|i| BASE + 10 * i));
    // It is five seconds after the newest play, and this machine says it is
    // a month later.
    let truly = BASE + 205;
    let (_server, result, _) = run(&before, &mut store, truly + 30 * DAY, options()).await;
    assert_eq!(result.unwrap().added, 20);

    // Two tracks are played at times the wrong clock had already passed.
    // Then the clock is put right, and the next sync has both: one because
    // it is recent, one because the count before the recent part moved.
    let now = truly + 200;
    let after = changed(&before, &[], &[truly + 60, truly + 120]);
    let (_server, result, _) = run(&after, &mut store, now, options()).await;

    assert_eq!(
        result.unwrap(),
        Change {
            added: 2,
            removed: 0
        }
    );
    assert_eq!(timestamps(&store, "scrobbles"), times(&after));
}

#[tokio::test]
async fn a_clock_that_runs_behind_leaves_the_newest_for_a_later_sync() {
    let scratch = Scratch::new();
    let mut store = Store::open(scratch.db()).unwrap();
    let dataset = Dataset::distinct(50);
    // The newest play is at BASE + 500, and this machine says it is four
    // minutes earlier than that.
    let behind = BASE + 255;

    let (_server, result, _) = run(&dataset, &mut store, behind, options()).await;

    assert_eq!(result.unwrap().added, 25);
    assert_eq!(markers(&store), dataset.expected(Window::before(behind)));

    let now = BASE + 505;
    let (_server, result, _) = run(&dataset, &mut store, now, options()).await;
    assert_eq!(result.unwrap().added, 25);
    assert_eq!(markers(&store), dataset.expected(Window::before(now)));
}

#[tokio::test]
async fn a_clock_at_the_start_of_time_reads_nothing_and_harms_nothing() {
    let scratch = Scratch::new();
    let mut store = Store::open(scratch.db()).unwrap();
    let dataset = Dataset::distinct(50);
    let now = BASE + 505;
    run(&dataset, &mut store, now, options()).await.1.unwrap();

    // A machine with no clock of its own says it is 1970: at the epoch
    // itself there is no time to ask about, and fifty seconds on there is
    // nothing in it.
    for (early, requests) in [(0, 0), (50, 2)] {
        let (server, result, _) = run(&dataset, &mut store, early, options()).await;

        assert_eq!(result.unwrap(), Change::default());
        assert_eq!(server.request_count(), requests);
        // What the file held stays, for the sync that knows what time it is.
        assert_eq!(markers(&store), dataset.expected(Window::before(now)));
        assert!(timestamps(&store, "removed").is_empty());
    }
}
