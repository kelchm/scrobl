# scrobl

A Rust client for the full Last.fm API: reads, authentication and scrobbling.

Early development. Not affiliated with or endorsed by Last.fm.

- [Design](docs/design.md)
- [Endpoints](docs/endpoints.md): every method and how far it is verified

## Usage

The async client is on by default (feature `client`). It runs on the Tokio runtime that calls it, including a current-thread runtime, and spawns nothing of its own.

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

    // The whole window, every page checked, the exact bytes of each kept.
    let mut scan = client
        .user("rj")
        .recent_tracks()
        .window(window)
        .extended(true)
        .scan()?;
    while let Some(page) = scan.next_page().await? {
        let bytes = page.raw().body(); // store these as Last.fm sent them
        for scrobble in page.scrobbles() {
            println!("{} {}", scrobble.timestamp(), scrobble.track().name());
        }
    }
    let summary = scan.finish()?; // fails unless every rule held
    println!("{} scrobbles in {} pages", summary.total(), summary.pages());
    Ok(())
}
```

`Client::call` makes a raw call to any of the 57 methods. The client sends over HTTPS only, follows no redirect, keeps at least a second between requests, retries reads that failed in a way worth repeating, never retries a write, and bounds both time and response size. A failed write says whether it can have happened (`Error::delivery`). The module documentation of `scrobl::client` has the details. Without the `client` feature the crate is the I/O-free protocol core, for use with any other HTTP client.

## Development

Tool versions are pinned in `mise.toml`.

```sh
cargo fmt --all --check
cargo clippy --all-targets --all-features
cargo test --all-features
cargo test --no-default-features
```

Tests never call Last.fm. They use fixtures and a local fake server.
