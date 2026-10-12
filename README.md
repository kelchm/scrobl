# scrobl

The full Last.fm API in async Rust: every method callable, history read and checked page by page, credentials kept out of logs. Unofficial.

Early development. Not affiliated with or endorsed by Last.fm.

What works today: a raw, signed call to any of the 57 methods, the ten that change an account only through a `Writer`, a typed, checked read of a user's scrobble history, and a typed read of a user's loved tracks. Not built yet: typed models for the other methods, the authentication flows and typed scrobbling. Only the history read has been run against the live service.

- [Design](docs/design.md)
- [Endpoints](docs/endpoints.md): every method and how far it is verified

The repository also holds [`scrobl`, a command-line tool](#the-command-line-tool) built on the library, which keeps a local copy of a listening history.

## Usage

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
    let mut scan = client
        .user("rj")
        .recent_tracks()
        .window(window)
        .extended(true)
        .scan()?;
    while let Some(page) = scan.next_page().await? {
        let bytes = page.raw().body(); // stage these as Last.fm sent them
        for scrobble in page.scrobbles() {
            println!("{} {}", scrobble.timestamp(), scrobble.track().name());
        }
    }
    // Only now is what you staged complete.
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

A sync makes the file agree with Last.fm. It asks Last.fm how many scrobbles a period holds and compares that with the file: where the two agree it moves on, and where they differ it halves the period and asks again, until Last.fm counts no more than 5000 in what is left. That it reads, every page checked by the library's scan, and the reading replaces the file's copy of the period in one step. The first sync reads the whole history this way, a second between requests. It can be stopped and run again at any point: what it stored is counted and, where it agrees, passed over.

Scrobbles arrive late, from a track that has just ended or a device that was offline for days. So every sync also reads the last fourteen days whatever the counts say. A scrobble that arrives later than that changes the count of its period, and the next sync is led to it. `--full` reads the whole history again, which also finds what no count shows, such as one scrobble removed and another added.

The file holds one row per scrobble in `scrobbles`, the table to query: its time, artist, track and album, the ids Last.fm sent, and in `raw` the whole row as JSON. Scrobbles are never merged, so two plays in the same second are two rows, and `ORDER BY timestamp, id` is the order of listening. A scrobble that was in the file and is no longer at Last.fm, because it was deleted or edited there, leaves `scrobbles` and is kept in `removed`; the sync says how many did. `loved` is the tracks the user has loved, each with the time it was loved, as the list stood at the last sync.

A sync exits with status 0 when it is done, 1 when it failed, 2 when the command line was wrong, and 3 when it finished but Last.fm and the file still count the history older than fourteen days differently. That last means the history changed while it was being read, which the next sync puts right, or that Last.fm's counts do not add up, which no reading can. A sync that fails keeps what it stored.

Nothing in the file is final or records how far a sync got, so a wrong clock or a second sync on the same file cannot put anything false on record. A clock that runs behind leaves the newest scrobbles to a later sync, and one that runs ahead reads all there is. Two syncs at once each write checked readings over whole periods, and the later write stands. Where that leaves an older reading over a newer one, the next sync finds it by its count or reads it again because it is recent; what neither shows waits for `--full`.

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

The minimum supported Rust version is 1.88, and CI runs the tests on it.

## License

[MIT](LICENSE). No fixture is copied from another project. Where each fixture came from is listed in [`crates/scrobl/fixtures/README.md`](crates/scrobl/fixtures/README.md).
