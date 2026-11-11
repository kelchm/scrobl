# scrobl

A Rust library for the Last.fm API, and a command-line tool built on it that keeps a local copy of a listening history. Unofficial.

Early development. Not affiliated with or endorsed by Last.fm.

| | What it is | Use it to |
|---|---|---|
| [`scrobl`](#the-library) | A Rust crate | Call the Last.fm API from a program of your own. It stores nothing |
| [`scrobl-cli`](#the-command-line-tool) | A program named `scrobl`, built on the crate | Keep one user's listening history in a SQLite file |
| [`ghcr.io/kelchm/scrobl`](#the-container-image) | That program as a container image | Run it under Docker or in a cluster, with nothing to install |

## The library

The full Last.fm API in async Rust: every method callable, history read and checked page by page, credentials kept out of logs.

What works today: a raw, signed call to any of the 57 methods, the ten that change an account only through a `Writer`, a typed, checked read of a user's scrobble history, and typed reads of everything else the `user` package offers (a profile, friends, loved tracks, the artists, albums, tracks and tags played and used most, the weekly charts, and what was given a tag) and of the artists in a user's library. Not built yet: typed models for the other methods, the authentication flows and typed scrobbling. All fourteen have been run against the live service; [Endpoints](docs/endpoints.md) has the level of every method.

- [Design](docs/design.md)
- [Endpoints](docs/endpoints.md): every method and how far it is verified

The crate is not published yet, so depend on it from Git. The client runs on Tokio:

```toml
[dependencies]
scrobl = { git = "https://github.com/kelchm/scrobl" }
tokio = { version = "1", features = ["macros", "rt-multi-thread"] }
```

The async client is on by default (feature `client`). It runs on the Tokio runtime that calls it, including a current-thread runtime with its I/O and time drivers enabled, and spawns nothing of its own.

```rust
use scrobl::history::Window;
use scrobl::{ApiKey, Client};

async fn backup() -> Result<(), scrobl::Error> {
    let client = Client::builder(ApiKey::new("your-api-key"))
        .user_agent("example-backup/0.1 (+https://example.org)")
        .build()?;

    // One page: how many scrobbles does a window hold?
    let window = Window::new(1_700_000_000, 1_700_086_400)?;
    let page = client.user("rj").recent_tracks().window(window).limit(1).send().await?;
    println!("{} scrobbles", page.attr().total());

    // The whole window, every page checked. Pages come as they are read, so
    // what the loop sees is provisional until `finish` succeeds.
    let mut scan = client.user("rj").recent_tracks().window(window).scan()?;
    while let Some(page) = scan.next_page().await? {
        for scrobble in page.scrobbles() {
            println!("{} {}", scrobble.timestamp(), scrobble.track().name());
        }
    }
    // Only now is what the loop saw known to be the whole window.
    let summary = scan.finish()?; // fails unless every rule held
    println!("{} scrobbles in {} pages", summary.total(), summary.pages());
    Ok(())
}
```

`Client::call` makes a raw call to any method that reads or authenticates. The client sends over HTTPS only, follows no redirect, keeps at least a second between requests by default (measured when each is admitted to the transport, however late the runtime polls it; the pacing is per client and its clones, and the second is this library's conservative choice, not a limit Last.fm documents), sends and decodes no compressed responses so the body is exactly what the service sent, retries reads that failed in a way worth repeating, never retries a write, and bounds how long it waits and how much of a response it keeps. A failed write says whether it can have happened (`Error::delivery`). The module documentation of `scrobl::client` has the details. Without the `client` feature the crate is the I/O-free protocol core, for use with any other HTTP client.

A scan yields pages before it can know the whole window is consistent, so what it has yielded is provisional until `finish()` succeeds. Stage what you write and mark it complete only then. The section "What a completed scan does not prove" in the documentation of `scrobl::history::WindowScan` says what success does and does not mean. Run `cargo doc --open` for it until the crate is published.

An example that prints a user's ten most recent scrobbles is in `crates/scrobl/examples/recent_tracks.rs`. It calls the live service with your own API key.

### Reading and writing

A `Client` cannot change an account. Last.fm has no read-only keys or scopes, so a session key that can read a hidden history can also scrobble, love and tag; the library keeps the two apart itself. The ten methods that write are refused by a `Client`, before anything is signed or sent, and are sent only by a `Writer`:

```rust
use scrobl::protocol::{Request, methods};
use scrobl::{ApiKey, ApiSecret, Client, SessionKey};

async fn love() -> Result<(), scrobl::Error> {
    let writer = Client::builder(ApiKey::new("your-api-key"))
        .secret(ApiSecret::new("your-api-secret"))
        .session(SessionKey::new("a-session-key"))
        .build_writer()?; // the one place a program asks to write

    let love = Request::new(&methods::TRACK_LOVE)
        .param("track", "Believe")
        .param("artist", "Cher");
    writer.call(&love).await?;
    Ok(())
}
```

A `Writer` dereferences to a `Client`, so it reads too, and code given a `&Client` cannot write. This guards against a bug or a careless call; it is not a security boundary, because the service enforces none of it.

## The command-line tool

`crates/scrobl-cli` builds a binary named `scrobl` that keeps one user's listening history in a SQLite file. It reads with a `Client`, so it cannot change the account, and it needs only an API key, which is enough for a history that is public.

```sh
cargo install --git https://github.com/kelchm/scrobl scrobl-cli
export SCROBL_API_KEY=your-api-key
scrobl sync --db history.db --user rj    # the first time
scrobl sync --db history.db              # every time after
```

The first sync reads the whole history, a second between requests: about a minute for every 10,000 scrobbles. It can be stopped and run again at any point. Every sync after it reads again from fourteen days before the newest scrobble it holds, because scrobbles arrive late, and any older period in which Last.fm counts another number of scrobbles than the file holds. `--full` reads everything again, which also finds what no count shows, such as an old scrobble that was edited.

A sync that fails says why in one line, exits with status 1 and keeps what it stored.

The file holds four tables:

| Table | Holds |
|---|---|
| `scrobbles` | One row per scrobble: `timestamp` in Unix seconds, `artist`, `track`, `album`, and the id Last.fm sent for each in `artist_mbid`, `track_mbid` and `album_mbid` |
| `removed` | Scrobbles the file held that Last.fm no longer lists, because they were deleted or edited there, each with `removed_at` |
| `loved` | The tracks the user has loved, each with `loved_at`, as the list stood at the last sync |
| `meta` | The user, and when the history and the loved tracks were last read |

Anything that reads SQLite can query it:

```sh
# The ten artists played most
sqlite3 history.db "SELECT artist, count(*) FROM scrobbles GROUP BY artist ORDER BY 2 DESC LIMIT 10"

# The last twenty plays, newest first
sqlite3 history.db "SELECT datetime(timestamp, 'unixepoch'), artist, track FROM scrobbles ORDER BY timestamp DESC, id DESC LIMIT 20"
```

Scrobbles are never merged: two plays in the same second are two rows, and `ORDER BY timestamp, id` is the order of listening. The file keeps what was listened to and when. The addresses of a cover image and of a track's page, which Last.fm also sends with every scrobble, are not kept.

### The container image

Each [release](https://github.com/kelchm/scrobl/releases) of the tool is also an image for `linux/amd64`, tagged with its version. It holds the same program, which takes the same arguments:

```sh
docker run --rm --read-only -e SCROBL_API_KEY -v scrobl:/data \
  ghcr.io/kelchm/scrobl:0.1.0 sync --db /data/history.db --user rj
```

It runs as user and group 1000 and writes only under `/data`, so the rest of its filesystem can be read-only. Mount a volume there that this user can write to.

## Last.fm's terms

Using the Last.fm API is subject to Last.fm's [API Terms of Service](https://www.last.fm/api/tos). The MIT licence below covers this library, not the data the API returns. You need your own API key, which you can [create here](https://www.last.fm/api/account/create), and you should send a User-Agent that identifies your application (`ClientBuilder::user_agent`). The terms ask applications to cache responses according to the response headers; `Raw` keeps the ones a cache goes by (`cache-control`, `expires`, `etag`, `last-modified` and `age`), along with `date`, `retry-after` and `content-type`. Read the terms for the limits that apply to you.

## Development

Tool versions are pinned in `mise.toml`.

```sh
cargo fmt --all --check
cargo clippy --all-targets --all-features
cargo test --all-features
cargo test --no-default-features -p scrobl
cargo llvm-cov --all-features --fail-under-lines 95
```

Tests never call Last.fm. They use fixtures and a local fake server.

How changes are made, for people and for agents, is in [`AGENTS.md`](AGENTS.md).

The minimum supported Rust version is 1.88 for the library and 1.89 for the command-line tool, and CI runs the tests of each on it.

## License

[MIT](LICENSE). No fixture is copied from another project. Where each fixture came from is listed in [`crates/scrobl/fixtures/README.md`](crates/scrobl/fixtures/README.md).
