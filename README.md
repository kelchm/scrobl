# scrobl

The full Last.fm API in async Rust: every method callable, history read and checked page by page, credentials kept out of logs. Unofficial.

Early development. Not affiliated with or endorsed by Last.fm.

What works today: a raw, signed call to any of the 57 methods, and a typed, checked read of a user's scrobble history. Not built yet: typed models for the other methods, the authentication flows and typed scrobbling. Only the history read has been run against the live service.

- [Design](docs/design.md)
- [Endpoints](docs/endpoints.md): every method and how far it is verified

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

`Client::call` makes a raw call to any of the 57 methods. The client sends over HTTPS only, follows no redirect, keeps at least a second between requests by default (measured when each is admitted to the transport, however late the runtime polls it; the pacing is per client and its clones, and the second is this library's conservative choice, not a limit Last.fm documents), sends and decodes no compressed responses so the body is exactly what the service sent, retries reads that failed in a way worth repeating, never retries a write, and bounds how long it waits and how much of a response it keeps. A failed write says whether it can have happened (`Error::delivery`). The module documentation of `scrobl::client` has the details. Without the `client` feature the crate is the I/O-free protocol core, for use with any other HTTP client.

A scan yields pages before it can know the whole window is consistent, so what it has yielded is provisional until `finish()` succeeds. Stage what you write and mark it complete only then. The section "What a completed scan does not prove" in the documentation of `scrobl::history::WindowScan` says what success does and does not mean. Run `cargo doc --open` for it until the crate is published.

An example that prints a user's ten most recent scrobbles is in `crates/scrobl/examples/recent_tracks.rs`. It calls the live service with your own API key.

## Last.fm's terms

Using the Last.fm API is subject to Last.fm's [API Terms of Service](https://www.last.fm/api/tos). The MIT licence below covers this library, not the data the API returns. You need your own API key, which you can [create here](https://www.last.fm/api/account/create), and you should send a User-Agent that identifies your application (`ClientBuilder::user_agent`). The terms ask applications to cache responses according to the response headers; `Raw` keeps the ones a cache goes by (`cache-control`, `expires`, `etag`, `last-modified` and `age`), along with `date`, `retry-after` and `content-type`. Read the terms for the limits that apply to you.

## Development

Tool versions are pinned in `mise.toml`.

```sh
cargo fmt --all --check
cargo clippy --all-targets --all-features
cargo test --all-features
cargo test --no-default-features
cargo llvm-cov --all-features --fail-under-lines 95
```

Tests never call Last.fm. They use fixtures and a local fake server.

The minimum supported Rust version is 1.88, and CI runs the tests on it.

## License

[MIT](LICENSE). Recorded fixtures from other projects keep their own licences, listed in [`crates/scrobl/fixtures/README.md`](crates/scrobl/fixtures/README.md).
