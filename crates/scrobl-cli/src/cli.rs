//! The command line.
//!
//! ```text
//! scrobl sync --db <file> [--user <name>] [--full]
//! ```
//!
//! Exit codes: 0 when the command did what it was asked, 1 when it failed, 2
//! when the command line was wrong, and 3 when a sync finished but the
//! database and Last.fm still disagree on how many scrobbles there are.

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
  scrobl sync --db <file> [--user <name>] [--full]

Commands:
  sync    Make the database agree with the history at Last.fm, and with
          the loved tracks as they are now

Options:
  --db <file>      The SQLite database, created if it is missing
  --user <name>    The Last.fm user; needed the first time only
  --full           Read the whole history again. Without it a sync reads
                   the last fourteen days, and any older period whose
                   number of scrobbles differs from Last.fm's
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
    /// Whether to read the whole history again.
    pub full: bool,
}

/// A parsed command line.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Command {
    /// Print the usage text.
    Help,
    /// Print the version.
    Version,
    /// Make the database agree with the history at Last.fm.
    Sync(SyncArgs),
}

/// Parses the arguments after the program name.
///
/// # Errors
///
/// [`Error::Usage`] for an unknown command or option, an option without its
/// value, an option given twice, or a `sync` without `--db`.
pub fn parse(args: impl IntoIterator<Item = OsString>) -> Result<Command, Error> {
    let usage = |message: String| Error::Usage(format!("{message}; try `scrobl --help`"));
    let mut args = args.into_iter();
    let (mut command, mut db, mut user, mut full) = (None, None, None, false);

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
            "--db" | "--user" => {
                // The value as given, so that a path need not be UTF-8.
                let value = match inline {
                    Some(value) => OsString::from(value),
                    None => args
                        .next()
                        .ok_or_else(|| usage(format!("`{name}` needs a value")))?,
                };
                let slot = if name == "--db" { &mut db } else { &mut user };
                if slot.replace(value).is_some() {
                    return Err(usage(format!("`{name}` was given twice")));
                }
            }
            "--full" if inline.is_none() => full = true,
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
    Ok(Command::Sync(SyncArgs {
        db: PathBuf::from(db),
        user,
        full,
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

/// `scrobl sync` with a client already built: opens the database, makes it
/// agree with the history before `now`, reports, and then reads the loved
/// tracks afresh. `--full` switches [`Options::full`] on.
///
/// `now` is this machine's clock and nothing checks it, because nothing a
/// sync stores depends on it: a clock that runs behind leaves the newest
/// scrobbles to a later sync, and one that runs ahead reads all there is.
///
/// Returns 0, or [`EXIT_MISMATCH`] when the sync finished but the database
/// and Last.fm still count the older history differently.
///
/// # Errors
///
/// [`Error::Usage`] when no user was given and the database has none yet,
/// and any error of [`Store::open`], [`sync::sync`] and [`sync::loved`].
/// The history stored before an error stays stored.
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

    let mut options = options;
    options.full |= args.full;
    let report = sync::sync(client, &mut store, &user, now, options, |span: Stored| {
        let mut line = format!(
            "{} to {}: {} scrobbles",
            time::utc(span.from),
            time::utc(span.to),
            span.scrobbles
        );
        if span.added > 0 {
            line.push_str(&format!(", {} new", span.added));
        }
        if span.removed > 0 {
            line.push_str(&format!(", {} gone", span.removed));
        }
        let _ = writeln!(err, "{line}");
    })
    .await?;

    let _ = writeln!(
        out,
        "{} scrobbles up to {}, {} of them new.",
        report.stored,
        time::utc(report.to),
        report.added
    );
    if report.removed > 0 {
        let _ = writeln!(
            out,
            "Last.fm no longer lists {} of the scrobbles the file held. The table `removed` \
             keeps every one.",
            report.removed
        );
    }
    let code = if report.matches() {
        0
    } else {
        let _ = writeln!(
            out,
            "Before {} Last.fm counts {} scrobbles and the file holds {}, after every period \
             whose count differed was read again. If the next sync says the same, the counts at \
             Last.fm do not add up.",
            time::utc(report.recent_from),
            report.reported,
            report.older
        );
        EXIT_MISMATCH
    };

    let loved = sync::loved(client, &mut store, &user, options.loved_page_size).await?;
    let _ = writeln!(out, "{loved} loved tracks.");
    Ok(code)
}
