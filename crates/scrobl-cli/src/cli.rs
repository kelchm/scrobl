//! The command line.
//!
//! ```text
//! scrobl sync --db <file> [--user <name>] [--full]
//! ```

use std::io::Write;
use std::path::PathBuf;

use scrobl::{ApiKey, Client};

use crate::Error;
use crate::store::Store;
use crate::sync::{self, Options, Stored};
use crate::time;

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

/// The arguments of `scrobl sync`.
#[derive(Debug, PartialEq, Eq)]
pub struct SyncArgs {
    pub db: PathBuf,
    pub user: Option<String>,
    pub full: bool,
}

/// A parsed command line.
#[derive(Debug, PartialEq, Eq)]
pub enum Command {
    Help,
    Version,
    Sync(SyncArgs),
}

/// Parses the arguments after the program name. The messages are fixed and
/// repeat no argument, since one given by mistake may be a key.
pub fn parse(args: impl IntoIterator<Item = String>) -> Result<Command, Error> {
    let mut args = args.into_iter();
    let (mut sync, mut db, mut user, mut full) = (false, None, None, false);
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "-h" | "--help" => return Ok(Command::Help),
            "-V" | "--version" => return Ok(Command::Version),
            "sync" => sync = true,
            "--db" => db = args.next(),
            "--user" => user = args.next(),
            "--full" => full = true,
            _ => return Err(Error::Refused("unknown argument; try `scrobl --help`")),
        }
    }
    match db {
        Some(db) if sync => Ok(Command::Sync(SyncArgs {
            db: PathBuf::from(db),
            user,
            full,
        })),
        _ => Err(Error::Refused(
            "expected `scrobl sync --db <file>`; try `scrobl --help`",
        )),
    }
}

/// Runs the tool and returns its exit code: 0, or 1 when it failed. Results
/// go to `out`, progress and errors to `err`.
///
/// The API key is the value of `SCROBL_API_KEY`. It is never taken from the
/// command line, where other users of the machine could see it.
pub async fn run(
    args: impl IntoIterator<Item = String>,
    api_key: Option<ApiKey>,
    out: &mut dyn Write,
    err: &mut dyn Write,
) -> u8 {
    match try_run(args, api_key, out, err).await {
        Ok(()) => 0,
        Err(error) => {
            // Nothing useful can be done if the terminal is gone.
            let _ = writeln!(err, "scrobl: {error}");
            1
        }
    }
}

async fn try_run(
    args: impl IntoIterator<Item = String>,
    api_key: Option<ApiKey>,
    out: &mut dyn Write,
    err: &mut dyn Write,
) -> Result<(), Error> {
    match parse(args)? {
        Command::Help => {
            let _ = out.write_all(USAGE.as_bytes());
        }
        Command::Version => {
            let _ = writeln!(out, "scrobl {}", env!("CARGO_PKG_VERSION"));
        }
        Command::Sync(args) => {
            let key =
                api_key.ok_or(Error::Refused("set SCROBL_API_KEY to your Last.fm API key"))?;
            let client = Client::builder(key).user_agent(USER_AGENT).build()?;
            sync_command(&args, &client, time::now(), out, err).await?;
        }
    }
    Ok(())
}

/// `scrobl sync` with a client already built: opens the database, makes it
/// agree with the history before `now`, reports, and then reads the loved
/// tracks afresh.
pub async fn sync_command(
    args: &SyncArgs,
    client: &Client,
    now: u64,
    out: &mut dyn Write,
    err: &mut dyn Write,
) -> Result<(), Error> {
    let mut store = Store::open(&args.db)?;
    let user = match (&args.user, store.user()?) {
        (Some(user), _) => user.clone(),
        (None, Some(user)) => user,
        (None, None) => {
            return Err(Error::Refused(
                "the database is new; say whose history it holds with `--user <name>`",
            ));
        }
    };

    let options = Options {
        full: args.full,
        ..Options::default()
    };
    let change = sync::sync(client, &mut store, &user, now, options, |span: Stored| {
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
        store.count(0, now)?,
        time::utc(now),
        change.added
    );
    if change.removed > 0 {
        let _ = writeln!(
            out,
            "Last.fm no longer lists {} of the scrobbles the file held. The table `removed` \
             keeps every one.",
            change.removed
        );
    }

    let loved = sync::loved(client, &mut store, &user).await?;
    let _ = writeln!(out, "{loved} loved tracks.");
    Ok(())
}
