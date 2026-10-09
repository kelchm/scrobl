//! Acceptance against the live Last.fm service. Read-only.
//!
//! Nothing here runs unless asked for: the test is ignored, and it fails at
//! once without `SCROBL_LIVE_API_KEY` and `SCROBL_LIVE_USER`. It is never
//! run in CI.
//!
//! ```sh
//! SCROBL_LIVE_API_KEY=... SCROBL_LIVE_USER=... \
//!     cargo test --test live -- --ignored --nocapture
//! ```
//!
//! It sends about a dozen key-only `user.getRecentTracks` requests, a
//! second apart: no signature, no session, no write. With
//! `SCROBL_LIVE_CAPTURE_DIR` set, the exact body of every response is
//! written there, for the owner to turn into fixtures by hand. The bodies
//! are that user's listening history; the test commits nothing.

#![cfg(feature = "client")]
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing,
    clippy::print_stdout
)]

use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

use scrobl::history::Window;
use scrobl::protocol::Raw;
use scrobl::{ApiErrorCode, ApiKey, Client, ErrorKind};

const USER_AGENT: &str = "scrobl-live-acceptance/0 (+https://github.com/kelchm/scrobl)";

fn env(name: &str) -> String {
    std::env::var(name).unwrap_or_else(|_| panic!("set {name} to run the live acceptance test"))
}

fn client(key: &str) -> Client {
    Client::builder(ApiKey::new(key))
        .user_agent(USER_AGENT)
        .build()
        .unwrap()
}

/// Writes the exact response body to the capture directory, if one is set.
fn capture(name: &str, raw: &Raw) {
    let Ok(dir) = std::env::var("SCROBL_LIVE_CAPTURE_DIR") else {
        return;
    };
    let dir = PathBuf::from(dir);
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join(format!("{name}.json")), raw.body()).unwrap();
}

/// Reads a whole window and returns its total, having checked every page.
async fn scan(client: &Client, user: &str, window: Window, limit: u32, name: &str) -> u64 {
    let mut scan = client
        .user(user)
        .recent_tracks()
        .window(window)
        .limit(limit)
        .extended(true)
        .scan()
        .unwrap();
    let mut rows = 0;
    while let Some(page) = scan.next_page().await.unwrap() {
        capture(&format!("{name}-page-{}", page.number()), page.raw());
        rows += page.scrobbles().len() as u64;
    }
    let summary = scan.finish().unwrap();
    assert_eq!(summary.total(), rows, "{name}: rows seen");
    println!("{name}: {rows} scrobbles in {} pages", summary.pages());
    rows
}

async fn total(client: &Client, user: &str, window: Window, name: &str) -> u64 {
    let page = client
        .user(user)
        .recent_tracks()
        .window(window)
        .limit(1)
        .send()
        .await
        .unwrap();
    capture(name, page.raw());
    page.attr().total()
}

#[tokio::test]
#[ignore = "calls Last.fm; needs SCROBL_LIVE_API_KEY and SCROBL_LIVE_USER"]
async fn recent_tracks_against_the_live_service() {
    let key = env("SCROBL_LIVE_API_KEY");
    let user = env("SCROBL_LIVE_USER");
    let client = client(&key);

    // The week that ends at the user's latest scrobble, or an hour ago if
    // that is earlier: a window in the past cannot gain a scrobble while it
    // is read.
    let latest = client
        .user(&user)
        .recent_tracks()
        .limit(1)
        .send()
        .await
        .unwrap();
    capture("latest", latest.raw());
    println!("history: {} scrobbles", latest.attr().total());
    let latest = latest
        .scrobbles()
        .first()
        .expect("the user has no scrobbles")
        .timestamp();
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_secs();
    let to = (latest + 1).min(now - 3600);
    let week = Window::new(to - 7 * 86_400, to).unwrap();

    // One page decodes, and the service echoes what was asked.
    let first = client
        .user(&user)
        .recent_tracks()
        .window(week)
        .limit(5)
        .extended(true)
        .send()
        .await
        .unwrap();
    capture("week-first-page-extended", first.raw());
    assert!(first.attr().user().eq_ignore_ascii_case(&user));
    assert_eq!(first.attr().per_page(), 5);
    let newest = first
        .scrobbles()
        .first()
        .expect("the window before the latest scrobble is empty")
        .timestamp();
    assert!(week.contains(newest));

    // The whole week at the largest page size and at a small one, which
    // takes several pages: the scan's
    // rules hold on real pages, and the two readings agree.
    let large = scan(&client, &user, week, 200, "week-200").await;
    let small = scan(&client, &user, week, 7, "week-7").await;
    assert_eq!(large, small, "the same window at two page sizes");

    // The bounds are `[from, to)`: `from` is inclusive and `to` exclusive.
    let at = total(
        &client,
        &user,
        Window::new(newest, newest + 1).unwrap(),
        "bound-at",
    )
    .await;
    assert!(
        at >= 1,
        "`from` is inclusive: [t, t+1) holds the scrobble at t"
    );
    let through = total(
        &client,
        &user,
        Window::new(newest, to).unwrap(),
        "bound-from",
    )
    .await;
    let after = total(
        &client,
        &user,
        Window::new(newest + 1, to + 1).unwrap(),
        "bound-after",
    )
    .await;
    assert_eq!(through, at + after, "[t, to) is [t, t+1) plus [t+1, to)");
    let before = total(
        &client,
        &user,
        Window::new(to - 7 * 86_400, newest).unwrap(),
        "bound-before",
    )
    .await;
    assert_eq!(
        before + through,
        large,
        "`to` is exclusive: [from, t) leaves out t"
    );

    // An error arrives as an envelope, whatever the HTTP status.
    let error = client
        .user("scrobl-no-such-user-0a1b2c3d4e5f")
        .recent_tracks()
        .limit(1)
        .send()
        .await
        .unwrap_err();
    println!("unknown user: {error} (http {:?})", error.http_status());
    assert_eq!(error.kind(), ErrorKind::Api);
    assert_eq!(error.api_code(), Some(ApiErrorCode::INVALID_PARAMETERS));

    let error = self::client("00000000000000000000000000000000")
        .user(&user)
        .recent_tracks()
        .limit(1)
        .send()
        .await
        .unwrap_err();
    println!("bad key: {error} (http {:?})", error.http_status());
    assert_eq!(error.kind(), ErrorKind::Api);
    assert_eq!(error.api_code(), Some(ApiErrorCode::INVALID_API_KEY));
}
