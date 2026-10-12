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

A sync makes the file agree with Last.fm, and finds where the two differ by counting. It asks Last.fm how many scrobbles a period holds and compares that with the file. Where they agree it moves on. Where they differ it halves the period and asks again, until what is left holds at most 5000 scrobbles; then it reads them, every page checked by the library's scan, and puts them in place of the file's copy of that period in one step. The first sync is that and nothing more: a new file agrees with Last.fm only where there is nothing, so everything is read, a second between requests. It can be stopped at any point and run again, because what it stored is counted, found to agree and passed over.

A scrobble is dated when its track started and reaches Last.fm when it ended, or days later from a device that was offline. A count that agrees cannot show what is still to come, so every sync also reads the last fourteen days whatever the counts say, and a scrobble that arrived late is there after the next sync. One that arrives later still changes the count of its period, and the next sync is led to it. Nothing in the file is final: `--full` reads the whole history again, which also finds what no count shows, such as one scrobble removed and another added.

The file follows Last.fm, and keeps what Last.fm drops. When a scrobble that was in the file is missing from a new reading of its period, because it was deleted or edited at Last.fm, it goes from `scrobbles`, the sync says how many did, and its row is kept in `removed` with the time that was noticed.

The file holds one row per scrobble (`scrobbles`, the table to query): its time, artist, track and album, the ids Last.fm sent, and in `raw` the whole row as Last.fm sent it, as JSON. Scrobbles are never merged, so two plays in the same second are two rows, and `ORDER BY timestamp, id` is the order of listening.

Every sync ends by reading the tracks the user has loved and replacing the copy in the file (`loved`). That list is what is loved now, each track with the time it was loved; a track that was unloved since the last sync is gone from it. If that read fails, the sync exits with status 1 and the history it stored is kept.

Before it ends, a sync counts once more. If Last.fm and the file still disagree about how many scrobbles are older than fourteen days, after every period that differed was read again, it says so and exits with status 3, if nothing else failed. That happens when the history changed while it was being read, which the next sync puts right, or when Last.fm's counts do not add up, which no reading can. Equal counts are not proof: a scrobble removed and another added leave a count as it was, and only `--full` finds them.

Nothing a sync stores is final, and nothing in the file records how far a sync got, so little can go lastingly wrong. A clock on this machine that runs behind leaves the newest scrobbles to a later sync, and one that runs ahead reads all there is. Two syncs at once on one file each put checked readings in place of whole periods; if the older reading undoes something the newer one stored, the next sync finds it by its count or reads it again because it is recent.

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
