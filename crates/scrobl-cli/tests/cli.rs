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

use std::ffi::OsString;
use std::path::PathBuf;

use common::*;
use scrobl_cli::Error;
use scrobl_cli::cli::{
    Command, EXIT_MISMATCH, Environment, SETTLE, SyncArgs, parse, run, sync_command,
};
use scrobl_cli::sync::Options;

fn args(line: &[&str]) -> Vec<OsString> {
    line.iter().map(OsString::from).collect()
}

fn usage_error(line: &[&str]) -> String {
    match parse(args(line)) {
        Err(error @ Error::Usage(_)) => {
            assert_eq!(error.exit_code(), 2);
            error.to_string()
        }
        other => panic!("expected a usage error, got {other:?}"),
    }
}

/// Runs the tool and returns its exit code, standard output and standard
/// error.
async fn tool(line: &[&str], api_key: Option<&str>) -> (u8, String, String) {
    let environment = Environment {
        api_key: api_key.map(str::to_owned),
        now: BASE,
    };
    let (mut out, mut err) = (Vec::new(), Vec::new());
    let code = run(args(line), &environment, &mut out, &mut err).await;
    (
        code,
        String::from_utf8(out).unwrap(),
        String::from_utf8(err).unwrap(),
    )
}

/// `scrobl sync` against `dataset`, with the clock at `now`.
async fn sync_with(
    dataset: Dataset,
    args: &SyncArgs,
    now: u64,
) -> (Result<u8, Error>, String, String, usize) {
    let server = FakeLastfm::start(dataset).await;
    let (mut out, mut err) = (Vec::new(), Vec::new());
    let result = sync_command(
        args,
        &client(&server),
        now,
        Options::default(),
        &mut out,
        &mut err,
    )
    .await;
    (
        result,
        String::from_utf8(out).unwrap(),
        String::from_utf8(err).unwrap(),
        server.request_count(),
    )
}

#[test]
fn sync_is_parsed_with_its_options_in_any_order() {
    let expected = Command::Sync(SyncArgs {
        db: PathBuf::from("history.db"),
        user: Some("rj".to_owned()),
    });
    for line in [
        &["sync", "--db", "history.db", "--user", "rj"][..],
        &["--user", "rj", "sync", "--db", "history.db"],
        &["sync", "--db=history.db", "--user=rj"],
    ] {
        assert_eq!(parse(args(line)).unwrap(), expected);
    }
    assert_eq!(
        parse(args(&["sync", "--db", "a=b.db"])).unwrap(),
        Command::Sync(SyncArgs {
            db: PathBuf::from("a=b.db"),
            user: None
        })
    );
}

#[test]
fn help_and_version_win_wherever_they_stand() {
    for line in [&["--help"][..], &["-h"], &["sync", "--db", "x", "--help"]] {
        assert_eq!(parse(args(line)).unwrap(), Command::Help);
    }
    for line in [&["--version"][..], &["-V"]] {
        assert_eq!(parse(args(line)).unwrap(), Command::Version);
    }
}

#[test]
fn a_wrong_command_line_says_what_is_wrong() {
    assert!(usage_error(&[]).contains("no command"));
    assert!(usage_error(&["--db", "x"]).contains("no command"));
    assert!(usage_error(&["sync"]).contains("--db <file>"));
    assert!(usage_error(&["sync", "--db"]).contains("`--db` needs a value"));
    assert!(usage_error(&["sync", "--db", "a", "--db", "b"]).contains("given twice"));
    assert!(usage_error(&["sync", "--user=a", "--user", "b", "--db", "x"]).contains("twice"));
    assert!(usage_error(&["sync", "--verbose"]).contains("unknown option"));
    assert!(usage_error(&["serve"]).contains("unknown command"));
    assert!(usage_error(&["sync", "sync"]).contains("unknown command"));
    assert!(usage_error(&["sync"]).contains("scrobl --help"));
}

#[cfg(unix)]
#[test]
fn a_path_need_not_be_unicode_but_a_user_name_must() {
    use std::os::unix::ffi::OsStringExt;
    let odd = OsString::from_vec(vec![b'h', 0xff, b'.', b'd', b'b']);

    let parsed = parse([OsString::from("sync"), "--db".into(), odd.clone()]).unwrap();
    assert_eq!(
        parsed,
        Command::Sync(SyncArgs {
            db: PathBuf::from(odd.clone()),
            user: None
        })
    );

    let error = parse([
        OsString::from("sync"),
        "--db".into(),
        "x".into(),
        "--user".into(),
        odd,
    ])
    .unwrap_err();
    assert!(error.to_string().contains("not valid Unicode"));
}

#[tokio::test]
async fn help_and_version_print_and_succeed() {
    let (code, out, err) = tool(&["--help"], None).await;
    assert_eq!((code, err.as_str()), (0, ""));
    assert!(out.contains("scrobl sync --db <file>"));
    assert!(out.contains("not affiliated with or endorsed by Last.fm"));

    let (code, out, err) = tool(&["--version"], None).await;
    assert_eq!((code, err.as_str()), (0, ""));
    assert_eq!(out, format!("scrobl {}\n", env!("CARGO_PKG_VERSION")));
}

#[tokio::test]
async fn a_usage_error_exits_2_and_prints_one_line() {
    let (code, out, err) = tool(&["sync"], Some("key")).await;
    assert_eq!((code, out.as_str()), (2, ""));
    assert!(err.starts_with("scrobl: `sync` needs `--db <file>`"));
    assert_eq!(err.lines().count(), 1);
}

#[tokio::test]
async fn a_sync_without_an_api_key_is_a_usage_error() {
    let scratch = Scratch::new();
    let db = scratch.db();
    for key in [None, Some("")] {
        let (code, out, err) = tool(&["sync", "--db", db.to_str().unwrap()], key).await;
        assert_eq!((code, out.as_str()), (2, ""));
        assert!(err.contains("SCROBL_API_KEY"));
    }
    // Nothing was opened, so nothing was created.
    assert!(!db.exists());
}

#[tokio::test]
async fn a_database_that_cannot_be_opened_exits_1_before_anything_is_asked() {
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
async fn a_sync_reports_its_windows_and_its_counts() {
    let scratch = Scratch::new();
    let args = SyncArgs {
        db: scratch.db(),
        user: Some(USER.to_owned()),
    };
    // The newest scrobble is at BASE + 200, half an hour before the clock.
    let now = BASE + 201 + SETTLE;

    let (result, out, err, _) = sync_with(Dataset::distinct(20), &args, now).await;

    assert_eq!(result.unwrap(), 0);
    assert_eq!(
        out,
        "20 scrobbles up to 2023-11-14T22:16:41Z, 20 of them new.\n"
    );
    // Thirty days of listening, then everything before it, which is empty.
    assert_eq!(
        err,
        "2023-10-15T22:16:41Z to 2023-11-14T22:16:41Z: 20 scrobbles\n\
         1970-01-01T00:00:00Z to 2023-10-15T22:16:41Z: 0 scrobbles\n"
    );
}

#[tokio::test]
async fn a_sync_stops_short_of_the_last_half_hour() {
    let scratch = Scratch::new();
    let args = SyncArgs {
        db: scratch.db(),
        user: Some(USER.to_owned()),
    };
    // The clock is one second past the newest scrobble, so the last half
    // hour (180 scrobbles, ten seconds apart) is left for a later sync.
    let now = BASE + 2_001;

    let (result, out, _, _) = sync_with(Dataset::distinct(200), &args, now).await;

    assert_eq!(result.unwrap(), 0);
    assert!(out.starts_with("20 scrobbles up to "));
}

#[tokio::test]
async fn the_user_is_needed_once_and_checked_afterwards() {
    let scratch = Scratch::new();
    let named = SyncArgs {
        db: scratch.db(),
        user: Some(USER.to_owned()),
    };
    let unnamed = SyncArgs {
        db: scratch.db(),
        user: None,
    };
    let now = BASE + 10_000;

    // A new database cannot guess whose history it is to hold.
    let (result, _, _, requests) = sync_with(Dataset::distinct(3), &unnamed, now).await;
    let error = result.unwrap_err();
    assert_eq!(error.exit_code(), 2);
    assert!(error.to_string().contains("--user <name>"));
    assert_eq!(requests, 0);

    let (result, _, _, _) = sync_with(Dataset::distinct(3), &named, now).await;
    assert_eq!(result.unwrap(), 0);

    // From then on it knows.
    let (result, out, _, _) = sync_with(Dataset::distinct(3), &unnamed, now).await;
    assert_eq!(result.unwrap(), 0);
    assert!(out.starts_with("3 scrobbles up to "));

    // And it refuses anyone else, before asking the service anything.
    let other = SyncArgs {
        db: scratch.db(),
        user: Some("someone-else".to_owned()),
    };
    let (result, _, _, requests) = sync_with(Dataset::distinct(3), &other, now).await;
    assert!(matches!(result, Err(Error::OtherUser)));
    assert_eq!(requests, 0);
}

#[tokio::test]
async fn counts_that_differ_exit_3_and_say_why() {
    let scratch = Scratch::new();
    let args = SyncArgs {
        db: scratch.db(),
        user: Some(USER.to_owned()),
    };
    let now = BASE + 10_000;
    let (result, _, _, _) = sync_with(Dataset::distinct(20), &args, now).await;
    assert_eq!(result.unwrap(), 0);

    // One more scrobble, dated inside the period already read.
    let mut times: Vec<u64> = (1..=20).map(|i| BASE + 10 * i).collect();
    times.push(BASE + 55);
    let (result, out, err, _) = sync_with(Dataset::from_timestamps(times), &args, now).await;

    assert_eq!(result.unwrap(), EXIT_MISMATCH);
    assert_eq!(err, "");
    let mut lines = out.lines();
    assert!(lines.next().unwrap().starts_with("20 scrobbles up to "));
    assert!(lines.next().unwrap().starts_with("Last.fm reports 21. "));
}
