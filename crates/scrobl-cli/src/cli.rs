//! The command line.
//!
//! ```text
//! scrobl sync --db <file> [--user <name>]
//! ```
//!
//! Exit codes: 0 when the command did what it was asked, 1 when it failed, 2
//! when the command line was wrong, and 3 when a sync finished but the
//! database and Last.fm disagree on how many scrobbles there are.

use std::ffi::OsString;
use std::io::Write;
use std::path::PathBuf;

use scrobl::{ApiKey, Client};

use crate::Error;
use crate::store::Store;
use crate::sync::{self, Options, Stored};
use crate::time;

/// The variable the API key is read from. It is never taken from the command
/// line, where other users of the machine could see it.
pub const API_KEY_VARIABLE: &str = "SCROBL_API_KEY";

/// How far this machine's clock may run ahead of Last.fm's before a sync
/// says so, in seconds.
pub const CLOCK_TOLERANCE: u64 = 30 * 60;

/// The exit code of a sync that finished with counts that differ.
pub const EXIT_MISMATCH: u8 = 3;

const USER_AGENT: &str = concat!(
    "scrobl-cli/",
    env!("CARGO_PKG_VERSION"),
    " (+https://github.com/kelchm/scrobl)"
);

const USAGE: &str = "\
Keeps a local copy of a Last.fm listening history.
Unofficial: not affiliated with or endorsed by Last.fm.

Usage:
  scrobl sync --db <file> [--user <name>] [--settle <time>]

Commands:
  sync    Read the history the database does not hold yet, the newest part
          of it again, and the loved tracks as they are now

Options:
  --db <file>      The SQLite database, created if it is missing
  --user <name>    The Last.fm user; needed the first time only
  --settle <time>  How old a scrobble must be before it is final: a number
                   and s, m, h or d, such as 12h or 14d (the default).
                   Everything newer is read again by every sync, because
                   scrobbles arrive late. 0 makes everything final at once
  -h, --help       Show this help
  -V, --version    Show the version

Environment:
  SCROBL_API_KEY   Your Last.fm API key
";

/// What the process was started with, apart from its arguments.
#[derive(Debug, Clone, Default)]
pub struct Environment {
    /// The value of [`API_KEY_VARIABLE`], if set. `Debug` does not show it.
    pub api_key: Option<ApiKey>,
    /// The clock, as Unix seconds.
    pub now: u64,
}

/// The arguments of `scrobl sync`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SyncArgs {
    /// The database file.
    pub db: PathBuf,
    /// The Last.fm user, if given.
    pub user: Option<String>,
    /// How old a scrobble must be before it is final, in seconds, if given.
    pub settle: Option<u64>,
}

/// Reads a length of time: a whole number and `s`, `m`, `h` or `d`, or a
/// bare `0`.
fn duration(text: &str) -> Option<u64> {
    if text == "0" {
        return Some(0);
    }
    let unit = match text.as_bytes().last()? {
        b's' => 1,
        b'm' => 60,
        b'h' => 3_600,
        b'd' => 86_400,
        _ => return None,
    };
    let digits = text.get(..text.len() - 1)?;
    if digits.is_empty() || !digits.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    digits.parse::<u64>().ok()?.checked_mul(unit)
}

/// A parsed command line.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Command {
    /// Print the usage text.
    Help,
    /// Print the version.
    Version,
    /// Read the history the database does not hold yet.
    Sync(SyncArgs),
}

/// Parses the arguments after the program name.
///
/// # Errors
///
/// [`Error::Usage`] for an unknown command or option, an option without its
/// value, an option given twice, a `--settle` that is not a length of time,
/// or a `sync` without `--db`.
pub fn parse(args: impl IntoIterator<Item = OsString>) -> Result<Command, Error> {
    let usage = |message: String| Error::Usage(format!("{message}; try `scrobl --help`"));
    let mut args = args.into_iter();
    let (mut command, mut db, mut user, mut settle) = (None, None, None, None);

    while let Some(arg) = args.next() {
        // Only a value that stands on its own may be something other than
        // Unicode; `--db=<value>` would have to be taken apart to be read.
        let text = arg.to_str().ok_or_else(|| {
            usage("an argument is not valid Unicode; give such a value on its own".to_owned())
        })?;
        let (name, inline) = match text.split_once('=') {
            Some((name, value)) if name.starts_with("--") => (name, Some(value)),
            _ => (text, None),
        };
        match name {
            "-h" | "--help" => return Ok(Command::Help),
            "-V" | "--version" => return Ok(Command::Version),
            "--db" | "--user" | "--settle" => {
                // The value as given, so that a path need not be UTF-8.
                let value = match inline {
                    Some(value) => OsString::from(value),
                    None => args
                        .next()
                        .ok_or_else(|| usage(format!("`{name}` needs a value")))?,
                };
                let slot = match name {
                    "--db" => &mut db,
                    "--user" => &mut user,
                    _ => &mut settle,
                };
                if slot.replace(value).is_some() {
                    return Err(usage(format!("`{name}` was given twice")));
                }
            }
            "sync" if command.is_none() => command = Some(()),
            _ if name.starts_with('-') => return Err(usage("unknown option".to_owned())),
            _ => return Err(usage("unknown command".to_owned())),
        }
    }

    if command.is_none() {
        return Err(usage("no command was given".to_owned()));
    }
    let db = db
        .filter(|db| !db.is_empty())
        .ok_or_else(|| usage("`sync` needs `--db <file>`".to_owned()))?;
    let user = user
        .map(|user| {
            user.into_string()
                .map_err(|_| usage("the user name is not valid Unicode".to_owned()))
        })
        .transpose()?;
    let settle = settle
        .map(|settle| {
            settle.to_str().and_then(duration).ok_or_else(|| {
                usage("`--settle` takes a number and s, m, h or d, such as 14d".to_owned())
            })
        })
        .transpose()?;
    Ok(Command::Sync(SyncArgs {
        db: PathBuf::from(db),
        user,
        settle,
    }))
}

/// Runs the tool and returns its exit code. Results go to `out`, progress
/// and errors to `err`.
pub async fn run(
    args: impl IntoIterator<Item = OsString>,
    environment: &Environment,
    out: &mut dyn Write,
    err: &mut dyn Write,
) -> u8 {
    match try_run(args, environment, out, err).await {
        Ok(code) => code,
        Err(error) => {
            // Nothing useful can be done if the terminal is gone.
            let _ = writeln!(err, "scrobl: {error}");
            error.exit_code()
        }
    }
}

async fn try_run(
    args: impl IntoIterator<Item = OsString>,
    environment: &Environment,
    out: &mut dyn Write,
    err: &mut dyn Write,
) -> Result<u8, Error> {
    match parse(args)? {
        Command::Help => {
            let _ = out.write_all(USAGE.as_bytes());
            Ok(0)
        }
        Command::Version => {
            let _ = writeln!(out, "scrobl {}", env!("CARGO_PKG_VERSION"));
            Ok(0)
        }
        Command::Sync(args) => {
            let key = environment
                .api_key
                .clone()
                .filter(|key| !key.expose().is_empty())
                .ok_or_else(|| {
                    Error::Usage(format!("set {API_KEY_VARIABLE} to your Last.fm API key"))
                })?;
            let client = Client::builder(key).user_agent(USER_AGENT).build()?;
            sync_command(
                &args,
                &client,
                environment.now,
                Options::default(),
                out,
                err,
            )
            .await
        }
    }
}

/// `scrobl sync` with a client already built: opens the database, reads
/// what it lacks up to `now` or Last.fm's own clock, whichever is earlier,
/// reads the newest part again, reports, and then reads the loved tracks
/// afresh. `--settle`, when given, replaces [`Options::settle`].
///
/// The second clock is there because a period that ends in the future
/// would be recorded as read, and what was then played in it never
/// fetched. A clock that runs ahead costs nothing this way, and one that
/// runs behind only delays the newest scrobbles to a later sync.
///
/// Returns 0, or [`EXIT_MISMATCH`] when the sync finished but the database
/// and Last.fm count the sealed history differently.
///
/// # Errors
///
/// [`Error::Usage`] when no user was given and the database has none yet,
/// and any error of [`Store::open`], [`Store::bind_user`],
/// [`sync::server_time`], [`sync::sync`] and [`sync::loved`]. The history stored before an error stays stored.
pub async fn sync_command(
    args: &SyncArgs,
    client: &Client,
    now: u64,
    options: Options,
    out: &mut dyn Write,
    err: &mut dyn Write,
) -> Result<u8, Error> {
    let mut store = Store::open(&args.db)?;
    let user = match (&args.user, store.user()?) {
        (Some(user), _) => user.clone(),
        (None, Some(user)) => user,
        (None, None) => {
            return Err(Error::Usage(
                "the database is new; say whose history it holds with `--user <name>`".to_owned(),
            ));
        }
    };
    store.bind_user(&user)?;

    let served = sync::server_time(client, &user).await?;
    if now > served.saturating_add(CLOCK_TOLERANCE) {
        let _ = writeln!(
            err,
            "This machine's clock is ahead of Last.fm's ({} against {}); reading up to Last.fm's.",
            time::utc(now),
            time::utc(served)
        );
    }
    let mut options = options;
    options.settle = args.settle.unwrap_or(options.settle);
    let report = sync::sync(
        client,
        &mut store,
        &user,
        now.min(served),
        options,
        |window: Stored| {
            let _ = writeln!(
                err,
                "{} to {}: {} scrobbles{}",
                time::utc(window.from),
                time::utc(window.to),
                window.scrobbles,
                if window.sealed {
                    ""
                } else {
                    ", to be read again"
                }
            );
        },
    )
    .await?;

    let _ = writeln!(
        out,
        "{} scrobbles up to {}, {} more than before.",
        report.stored,
        time::utc(report.cutoff),
        report.added
    );
    if report.lost > 0 {
        let _ = writeln!(
            out,
            "Last.fm no longer lists {} that were here at the last sync. The file follows \
             Last.fm and keeps the pages that had them.",
            report.lost
        );
    }
    let code = if report.matches() {
        0
    } else {
        let _ = writeln!(
            out,
            "Before {} Last.fm reports {} and the file holds {}. The number of scrobbles in a \
             period changed after it was final, and this version does not read such a period \
             twice.",
            time::utc(report.sealed_to),
            report.reported,
            report.sealed
        );
        EXIT_MISMATCH
    };

    let loved = sync::loved(client, &mut store, &user, options.loved_page_size).await?;
    let _ = writeln!(out, "{loved} loved tracks.");
    Ok(code)
}
