# scrobl

An async Rust client for the Last.fm API. Unofficial.

Early development. Not affiliated with or endorsed by Last.fm.

[Documentation](https://docs.rs/scrobl) · [Design](https://github.com/kelchm/scrobl/blob/main/docs/design.md) · [Endpoint coverage](https://github.com/kelchm/scrobl/blob/main/docs/endpoints.md)

This crate stores nothing. To keep a listening history in a SQLite file, see [`scrobl-cli`](https://crates.io/crates/scrobl-cli), which is built on it.

## Status

- Any of the 57 API methods can be called raw, signed where the method needs it.
- Typed so far: every read in `user` (scrobble history, profile, friends, loved tracks, top lists, weekly charts, tagged items) and `library.getArtists`. All fourteen have been run against the live service.
- Not built yet: typed models for the other methods, the authentication flows, typed scrobbling.

## Install

```toml
[dependencies]
scrobl = "0.1"
tokio = { version = "1", features = ["macros", "rt-multi-thread"] }
```

The client is the default feature `client` and runs on Tokio. It spawns nothing, and a current-thread runtime with I/O and time enabled is enough. Without the feature, the crate is the protocol alone, with no I/O, for use with another HTTP client.

## Example

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

    // The whole window, every page checked against the others.
    let mut scan = client.user("rj").recent_tracks().window(window).scan()?;
    while let Some(page) = scan.next_page().await? {
        for scrobble in page.scrobbles() {
            println!("{} {}", scrobble.timestamp(), scrobble.track().name());
        }
    }
    let summary = scan.finish()?; // fails unless every check held
    println!("{} scrobbles in {} pages", summary.total(), summary.pages());
    Ok(())
}
```

A scan hands out pages before it can know the window is consistent. Treat them as provisional until `finish()` succeeds. [`WindowScan`](https://docs.rs/scrobl/latest/scrobl/history/struct.WindowScan.html) documents what a completed scan does and does not prove.

A program you can run is in [`examples/recent_tracks.rs`](https://github.com/kelchm/scrobl/blob/main/crates/scrobl/examples/recent_tracks.rs).

## What the client does

- Sends over HTTPS only and follows no redirect.
- Waits at least a second between requests by default. That is this library's caution, not a limit Last.fm documents.
- Retries a read that failed in a way worth repeating. Never retries a write.
- Limits how long it waits and how much of a response it keeps.
- Keeps each response body exactly as the service sent it.
- Shows no credential in an error message or in `Debug` output. `Error::body()` is the raw response, which can hold one.

The [`client` module](https://docs.rs/scrobl/latest/scrobl/client/index.html) has the details.

## Writing

A `Client` cannot change an account. Last.fm has no read-only keys, so a session key that can read a hidden history can also scrobble, love and tag. The library keeps the two apart itself: a `Client` refuses the ten methods that write before anything is signed or sent, and only a `Writer` sends them.

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

A `Writer` also reads, and code given a `&Client` cannot write. A write that fails says whether it can have happened (`Error::delivery`). This guards against a bug, not an attacker: the service enforces none of it.

## Last.fm's terms

Using the API is subject to Last.fm's [API Terms of Service](https://www.last.fm/api/tos). You need your own API key, which you can [create here](https://www.last.fm/api/account/create), and you should send a User-Agent that identifies your application. The terms ask applications to cache according to the response headers, and a raw response keeps them. The MIT licence covers this library, not the data the API returns.

## License

[MIT](https://github.com/kelchm/scrobl/blob/main/LICENSE). Where each test fixture came from is listed in [`fixtures/README.md`](https://github.com/kelchm/scrobl/blob/main/crates/scrobl/fixtures/README.md).
