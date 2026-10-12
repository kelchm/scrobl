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
use scrobl::ApiKey;
use scrobl_cli::Error;
use scrobl_cli::cli::{Command, EXIT_MISMATCH, Environment, SyncArgs, parse, run, sync_command};
use scrobl_cli::store::Store;
use scrobl_cli::sync::Options;

const DAY: u64 = 86_400;

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
    let environment = Environment {
        api_key: api_key.map(ApiKey::new),
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
    sync_on(
        FakeLastfm::start(dataset).await,
        args,
        now,
        Options::default(),
    )
    .await
}

/// `scrobl sync` against a server already set up, with the clock at `now`.
async fn sync_on(
    server: FakeLastfm,
    args: &SyncArgs,
    now: u64,
    options: Options,
) -> (Result<u8, Error>, String, String, usize) {
    let (mut out, mut err) = (Vec::new(), Vec::new());
    let result = sync_command(args, &client(&server), now, options, &mut out, &mut err).await;
    (
        result,
        String::from_utf8(out).unwrap(),
        String::from_utf8(err).unwrap(),
        server.request_count(),
    )
}

#[test]
fn sync_is_parsed_with_its_options_in_any_order() {
    let expected = |full| {
        Command::Sync(SyncArgs {
            db: PathBuf::from("history.db"),
            user: Some("rj".to_owned()),
            full,
        })
    };
    for line in [
        &["sync", "--db", "history.db", "--user", "rj"][..],
        &["--user", "rj", "sync", "--db", "history.db"],
        &["sync", "--db=history.db", "--user=rj"],
    ] {
        assert_eq!(parse(args(line)).unwrap(), expected(false));
    }
    for line in [
        &["sync", "--db", "history.db", "--user", "rj", "--full"][..],
        &["--full", "sync", "--user=rj", "--db=history.db"],
    ] {
        assert_eq!(parse(args(line)).unwrap(), expected(true));
    }
    assert_eq!(
        parse(args(&["sync", "--db", "a=b.db"])).unwrap(),
        Command::Sync(SyncArgs {
            db: PathBuf::from("a=b.db"),
            user: None,
            full: false,
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
    // `--full` is said or not said; it takes no value.
    assert!(usage_error(&["sync", "--db", "x", "--full=yes"]).contains("unknown option"));
    assert!(usage_error(&["serve"]).contains("unknown command"));
    assert!(usage_error(&["sync", "sync"]).contains("unknown command"));
    assert!(usage_error(&["sync"]).contains("scrobl --help"));
    // An empty path names no file; SQLite would read it as a database to
    // throw away.
    assert!(usage_error(&["sync", "--db="]).contains("--db <file>"));
    assert!(usage_error(&["sync", "--db", ""]).contains("--db <file>"));
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
            user: None,
            full: false,
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

    // Joined to its option, such a value would have to be rewritten to be
    // read, and a rewritten path is another file.
    for option in [&b"--db=h"[..], b"--user=h"] {
        let mut joined = option.to_vec();
        joined.push(0xff);
        let error = parse([
            OsString::from("sync"),
            "--db".into(),
            "x".into(),
            OsString::from_vec(joined),
        ])
        .unwrap_err();
        assert_eq!(error.exit_code(), 2);
        assert!(error.to_string().contains("not valid Unicode"));
    }
}

#[test]
fn the_environment_does_not_show_the_api_key() {
    let environment = Environment {
        api_key: Some(ApiKey::new("SENTINEL_API_KEY_0001")),
        now: BASE,
    };
    let shown = format!("{environment:?} {environment:#?}");
    assert!(!shown.contains("SENTINEL"));
    assert!(shown.contains("now"));
}

#[tokio::test]
async fn help_and_version_print_and_succeed() {
    let (code, out, err) = tool(&["--help"], None).await;
    assert_eq!((code, err.as_str()), (0, ""));
    assert!(out.contains("scrobl sync --db <file> [--user <name>] [--full]"));
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
async fn a_database_of_an_earlier_version_is_refused_before_anything_is_asked() {
    let scratch = Scratch::new();
    // A file as an earlier version marked it, with a table of its own.
    let old = rusqlite::Connection::open(scratch.db()).unwrap();
    old.execute_batch(
        "CREATE TABLE windows (id INTEGER PRIMARY KEY);
         PRAGMA application_id = 0x7363626c;
         PRAGMA user_version = 2;",
    )
    .unwrap();
    drop(old);

    let (result, out, _, requests) =
        sync_with(Dataset::distinct(3), &sync_args(&scratch, Some(USER)), BASE).await;

    let error = result.unwrap_err();
    assert_eq!(error.exit_code(), 1);
    // It says what to do about it.
    assert_eq!(
        error.to_string(),
        "the database was written by an earlier version of this tool, which kept the history in \
         another form; sync into a new file"
    );
    assert_eq!((out.as_str(), requests), ("", 0));
}

#[tokio::test]
async fn a_sync_reports_its_spans_and_its_counts() {
    let scratch = Scratch::new();
    let args = sync_args(&scratch, Some(USER));
    // The newest scrobble is at BASE + 200, a second before the clock.
    let now = BASE + 201;

    let (result, out, err, _) = sync_with(Dataset::distinct(20).with_loved(3), &args, now).await;

    assert_eq!(result.unwrap(), 0);
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

    assert_eq!(result.unwrap(), 0);
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
async fn a_sync_reads_right_up_to_the_present() {
    let scratch = Scratch::new();
    let args = sync_args(&scratch, Some(USER));
    // The clock is one second past the newest scrobble.
    let now = BASE + 2_001;

    let (result, out, _, _) = sync_with(Dataset::distinct(200), &args, now).await;

    assert_eq!(result.unwrap(), 0);
    assert!(out.starts_with("200 scrobbles up to "), "{out}");
}

#[tokio::test]
async fn the_user_is_needed_once_and_checked_afterwards() {
    let scratch = Scratch::new();
    let named = sync_args(&scratch, Some(USER));
    let unnamed = sync_args(&scratch, None);
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
    let other = sync_args(&scratch, Some("someone-else"));
    let (result, _, _, requests) = sync_with(Dataset::distinct(3), &other, now).await;
    assert!(matches!(result, Err(Error::OtherUser)));
    assert_eq!(requests, 0);
}

#[tokio::test]
async fn what_changed_at_last_fm_is_followed_and_reported() {
    let scratch = Scratch::new();
    let args = sync_args(&scratch, Some(USER));
    let now = BASE + 201;
    let before = named((1..=20).map(|i| BASE + 10 * i));
    let (result, _, _, _) = sync_with(before.clone(), &args, now).await;
    assert_eq!(result.unwrap(), 0);

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

    assert_eq!(result.unwrap(), 0);
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
    assert_eq!(result.unwrap(), 0);

    // Without `--full`, the last fourteen days.
    let (result, out, err, _) = sync_with(dataset.clone(), &args, now).await;
    assert_eq!(result.unwrap(), 0);
    assert!(out.starts_with("30 scrobbles up to "), "{out}");
    assert_eq!(err.lines().count(), 1, "{err}");
    assert!(err.ends_with(": 14 scrobbles\n"), "{err}");

    // With it, those and everything before them.
    let full = SyncArgs { full: true, ..args };
    let (result, out, err, _) = sync_with(dataset, &full, now).await;
    assert_eq!(result.unwrap(), 0);
    assert!(out.contains(", 0 of them new.\n"), "{out}");
    let read: Vec<&str> = err.lines().collect();
    assert_eq!(read.len(), 2, "{err}");
    assert!(read[0].ends_with(": 14 scrobbles"), "{err}");
    assert!(read[1].starts_with("1970-01-01T00:00:00Z to "), "{err}");
    assert!(read[1].ends_with(": 16 scrobbles"), "{err}");
}

#[tokio::test]
async fn counts_that_still_differ_exit_3_and_say_why() {
    let scratch = Scratch::new();
    let args = sync_args(&scratch, Some(USER));
    let now = BASE + 10_000;
    let dataset = Dataset::distinct(20);

    // The count of the last fourteen days, their one page, and the count of
    // what is older, which is none, as in the file. Then a play from months
    // ago turns up, and the last count is one.
    let older = Dataset::from_timestamps(vec![BASE - 100 * DAY]);
    let (_, last) = respond_to(&older, &question(0, now - 14 * DAY, 1, 1));
    let server = FakeLastfm::start(dataset.clone()).await.script([
        Behaviour::Normal,
        Behaviour::Normal,
        Behaviour::Normal,
        Behaviour::status(200, last),
    ]);
    let (result, out, _, _) = sync_on(server, &args, now, Options::default()).await;

    assert_eq!(result.unwrap(), EXIT_MISMATCH);
    assert_eq!(
        out,
        "20 scrobbles up to 2023-11-15T01:00:00Z, 20 of them new.\n\
         Before 2023-11-01T01:00:00Z Last.fm counts 1 scrobbles and the file holds 0, after \
         every period whose count differed was read again. If the next sync says the same, the \
         counts at Last.fm do not add up.\n\
         0 loved tracks.\n"
    );
}

#[tokio::test]
async fn loved_tracks_that_cannot_be_read_fail_the_sync_and_keep_the_history() {
    let scratch = Scratch::new();
    let args = sync_args(&scratch, Some(USER));
    // A track is loved between the first page of the list and the second.
    let faults = Faults {
        love_from_page: Some(2),
        ..Faults::default()
    };
    let dataset = Dataset::distinct(20).with_loved(4).with_faults(faults);
    let server = FakeLastfm::start(dataset).await;
    let mut options = Options::default();
    options.loved_page_size = 2;
    let now = BASE + 10_000;

    let (result, out, _, _) = sync_on(server, &args, now, options).await;

    let error = result.unwrap_err();
    assert_eq!(error.exit_code(), 1);
    assert!(error.to_string().contains("loved tracks"), "{error}");
    // The history was stored and reported before the list was asked for.
    assert!(out.starts_with("20 scrobbles up to "));
    let store = Store::open(scratch.db()).unwrap();
    assert_eq!(store.count(0, now).unwrap(), 20);
}
