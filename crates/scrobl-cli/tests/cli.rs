//! The command line: what it parses, what it prints and how it exits.
//!
//! provenance: synthetic. The fake service generates every response. No
//! test here lets `run` reach a sync, because `run` builds a client for the
//! real service; the sync itself is driven through `sync_command`.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]

mod common;

use std::path::PathBuf;

use common::*;
use scrobl::ApiKey;
use scrobl_cli::Error;
use scrobl_cli::cli::{Command, SyncArgs, parse, run, sync_command};
use scrobl_cli::store::Store;

const DAY: u64 = 86_400;

fn args(line: &[&str]) -> Vec<String> {
    line.iter().map(|&arg| arg.to_owned()).collect()
}

/// The arguments of a plain `scrobl sync` on the database in `scratch`.
fn sync_args(scratch: &Scratch, user: Option<&str>) -> SyncArgs {
    SyncArgs {
        db: scratch.db(),
        user: user.map(str::to_owned),
        full: false,
    }
}

/// Runs the tool and returns its exit code, standard output and standard
/// error.
async fn tool(line: &[&str], api_key: Option<&str>) -> (u8, String, String) {
    let (mut out, mut err) = (Vec::new(), Vec::new());
    let code = run(args(line), api_key.map(ApiKey::new), &mut out, &mut err).await;
    (
        code,
        String::from_utf8(out).unwrap(),
        String::from_utf8(err).unwrap(),
    )
}

/// `scrobl sync` against `dataset`, with the clock at `now`: how it ended,
/// standard output, standard error and the requests it made.
async fn sync_with(
    dataset: Dataset,
    args: &SyncArgs,
    now: u64,
) -> (Result<(), Error>, String, String, usize) {
    let server = FakeLastfm::start(dataset).await;
    let (mut out, mut err) = (Vec::new(), Vec::new());
    let result = sync_command(args, &client(&server), now, &mut out, &mut err).await;
    (
        result,
        String::from_utf8(out).unwrap(),
        String::from_utf8(err).unwrap(),
        server.request_count(),
    )
}

#[test]
fn sync_is_parsed_with_its_options_in_any_order() {
    let expected = |user: Option<&str>, full| {
        Command::Sync(SyncArgs {
            db: PathBuf::from("history.db"),
            user: user.map(str::to_owned),
            full,
        })
    };
    for line in [
        &["sync", "--db", "history.db", "--user", "rj"][..],
        &["--user", "rj", "sync", "--db", "history.db"],
    ] {
        assert_eq!(parse(args(line)).unwrap(), expected(Some("rj"), false));
    }
    assert_eq!(
        parse(args(&["--full", "sync", "--db", "history.db"])).unwrap(),
        expected(None, true)
    );
}

#[tokio::test]
async fn help_and_version_print_wherever_they_stand() {
    for line in [&["--help"][..], &["-h"], &["sync", "--db", "x", "--help"]] {
        let (code, out, err) = tool(line, None).await;
        assert_eq!((code, err.as_str()), (0, ""));
        assert!(out.contains("scrobl sync --db <file> [--user <name>] [--full]"));
        assert!(out.contains("not affiliated with or endorsed by Last.fm"));
    }
    for line in [&["--version"][..], &["-V"]] {
        let (code, out, err) = tool(line, None).await;
        assert_eq!((code, err.as_str()), (0, ""));
        assert_eq!(out, format!("scrobl {}\n", env!("CARGO_PKG_VERSION")));
    }
}

#[tokio::test]
async fn a_wrong_command_line_fails_with_one_line_that_says_where_to_look() {
    let lines: [(&[&str], &str); 6] = [
        (&[], "expected `scrobl sync --db <file>`"),
        (&["--db", "x"], "expected `scrobl sync --db <file>`"),
        (&["sync"], "expected `scrobl sync --db <file>`"),
        (&["sync", "--db"], "expected `scrobl sync --db <file>`"),
        (&["sync", "--db", "x", "--verbose"], "unknown argument"),
        (&["serve"], "unknown argument"),
    ];
    for (line, says) in lines {
        let (code, out, err) = tool(line, Some("key")).await;
        assert_eq!((code, out.as_str()), (1, ""), "{line:?}");
        assert_eq!(err, format!("scrobl: {says}; try `scrobl --help`\n"));
    }
}

#[tokio::test]
async fn a_sync_without_an_api_key_says_which_variable_to_set() {
    let scratch = Scratch::new();
    let db = scratch.db();

    let (code, out, err) = tool(&["sync", "--db", db.to_str().unwrap()], None).await;

    assert_eq!((code, out.as_str()), (1, ""));
    assert!(err.contains("SCROBL_API_KEY"));
    // Nothing was opened, so nothing was created.
    assert!(!db.exists());
}

#[tokio::test]
async fn a_database_that_cannot_be_opened_fails_before_anything_is_asked() {
    let scratch = Scratch::new();
    // A directory is not a database. The failure comes before any request,
    // so this never reaches the real service.
    let directory = scratch.db().parent().unwrap().to_owned();

    let (code, out, err) = tool(
        &["sync", "--db", directory.to_str().unwrap(), "--user", "rj"],
        Some("SENTINEL_API_KEY_0001"),
    )
    .await;

    assert_eq!((code, out.as_str()), (1, ""));
    assert!(err.starts_with("scrobl: database: "));
    assert!(!err.contains("SENTINEL_API_KEY_0001"));
}

#[tokio::test]
async fn a_sync_reports_its_spans_and_its_counts() {
    let scratch = Scratch::new();
    let args = sync_args(&scratch, Some(USER));
    // The newest scrobble is at BASE + 200, a second before the clock.
    let now = BASE + 201;

    let (result, out, err, _) = sync_with(Dataset::distinct(20).with_loved(3), &args, now).await;

    result.unwrap();
    assert_eq!(
        out,
        "20 scrobbles up to 2023-11-14T22:16:41Z, 20 of them new.\n\
         3 loved tracks.\n"
    );
    // The last fourteen days. Before them the file and Last.fm both hold
    // nothing, so nothing was read.
    assert_eq!(
        err,
        "2023-10-31T22:16:41Z to 2023-11-14T22:16:41Z: 20 scrobbles, 20 new\n"
    );

    // The same again, a minute later: read, and nothing in it new.
    let (result, out, err, _) =
        sync_with(Dataset::distinct(20).with_loved(3), &args, now + 60).await;

    result.unwrap();
    assert_eq!(
        out,
        "20 scrobbles up to 2023-11-14T22:17:41Z, 0 of them new.\n\
         3 loved tracks.\n"
    );
    assert_eq!(
        err,
        "2023-10-31T22:17:41Z to 2023-11-14T22:17:41Z: 20 scrobbles\n"
    );
}

#[tokio::test]
async fn the_user_is_needed_once_and_checked_afterwards() {
    let scratch = Scratch::new();
    let named = sync_args(&scratch, Some(USER));
    let unnamed = sync_args(&scratch, None);
    let now = BASE + 10_000;

    // A new database cannot guess whose history it is to hold.
    let (result, _, _, requests) = sync_with(Dataset::distinct(3), &unnamed, now).await;
    assert!(result.unwrap_err().to_string().contains("--user <name>"));
    assert_eq!(requests, 0);

    let (result, _, _, _) = sync_with(Dataset::distinct(3), &named, now).await;
    result.unwrap();

    // From then on it knows.
    let (result, out, _, _) = sync_with(Dataset::distinct(3), &unnamed, now).await;
    result.unwrap();
    assert!(out.starts_with("3 scrobbles up to "));

    // And it refuses anyone else, before asking the service anything.
    let other = sync_args(&scratch, Some("someone-else"));
    let (result, _, _, requests) = sync_with(Dataset::distinct(3), &other, now).await;
    assert!(result.unwrap_err().to_string().contains("another user"));
    assert_eq!(requests, 0);
}

#[tokio::test]
async fn what_changed_at_last_fm_is_followed_and_reported() {
    let scratch = Scratch::new();
    let args = sync_args(&scratch, Some(USER));
    let now = BASE + 201;
    let before = named((1..=20).map(|i| BASE + 10 * i));
    let (result, _, _, _) = sync_with(before.clone(), &args, now).await;
    result.unwrap();

    // One play was deleted on the website, and one arrived late.
    let after = named(
        before
            .entries
            .iter()
            .map(|entry| entry.ts)
            .filter(|time| *time != BASE + 50)
            .chain([BASE + 55]),
    );
    let (result, out, err, _) = sync_with(after, &args, now + 60).await;

    result.unwrap();
    assert_eq!(
        out,
        "20 scrobbles up to 2023-11-14T22:17:41Z, 1 of them new.\n\
         Last.fm no longer lists 1 of the scrobbles the file held. The table `removed` keeps \
         every one.\n\
         0 loved tracks.\n"
    );
    assert_eq!(
        err,
        "2023-10-31T22:17:41Z to 2023-11-14T22:17:41Z: 20 scrobbles, 1 new, 1 gone\n"
    );
    let store = Store::open(scratch.db()).unwrap();
    let removed: u64 = store
        .connection()
        .query_row("SELECT timestamp FROM removed", [], |row| row.get(0))
        .unwrap();
    assert_eq!(removed, BASE + 50);
}

#[tokio::test]
async fn full_reads_the_whole_history_again() {
    let scratch = Scratch::new();
    let args = sync_args(&scratch, Some(USER));
    // A month of plays, one a day, the newest an hour ago.
    let now = BASE + 30 * DAY;
    let dataset = Dataset::from_timestamps((0..30).map(|day| now - 3_600 - day * DAY).collect());
    let (result, ..) = sync_with(dataset.clone(), &args, now).await;
    result.unwrap();

    // Without `--full`, the last fourteen days.
    let (result, out, err, _) = sync_with(dataset.clone(), &args, now).await;
    result.unwrap();
    assert!(out.starts_with("30 scrobbles up to "), "{out}");
    assert_eq!(err.lines().count(), 1, "{err}");
    assert!(err.ends_with(": 14 scrobbles\n"), "{err}");

    // With it, those and everything before them.
    let full = SyncArgs { full: true, ..args };
    let (result, out, err, _) = sync_with(dataset, &full, now).await;
    result.unwrap();
    assert!(out.contains(", 0 of them new.\n"), "{out}");
    let read: Vec<&str> = err.lines().collect();
    assert_eq!(read.len(), 2, "{err}");
    assert!(read[0].ends_with(": 14 scrobbles"), "{err}");
    assert!(read[1].starts_with("1970-01-01T00:00:00Z to "), "{err}");
    assert!(read[1].ends_with(": 16 scrobbles"), "{err}");
}
