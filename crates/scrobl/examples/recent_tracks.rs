//! Prints a user's total scrobbles and their ten most recent.
//!
//! This calls the live Last.fm service. You need your own API key:
//!
//! ```sh
//! LASTFM_API_KEY=your-key cargo run --example recent_tracks -- <user>
//! ```

#![allow(clippy::print_stdout, clippy::print_stderr)]

use scrobl::{ApiKey, Client};

#[tokio::main]
async fn main() {
    if let Err(message) = run().await {
        eprintln!("error: {message}");
        std::process::exit(1);
    }
}

async fn run() -> Result<(), String> {
    let key = std::env::var("LASTFM_API_KEY").map_err(|_| "set LASTFM_API_KEY".to_owned())?;
    let user = std::env::args()
        .nth(1)
        .ok_or("usage: recent_tracks <user>")?;

    let client = Client::builder(ApiKey::new(key))
        .user_agent("scrobl-example/0.0 (+https://github.com/kelchm/scrobl)")
        .build()
        .map_err(|e| e.to_string())?;
    let page = client
        .user(user)
        .recent_tracks()
        .limit(10)
        .send()
        .await
        .map_err(|e| e.to_string())?;

    println!("{} scrobbles", page.attr().total());
    for scrobble in page.scrobbles() {
        let (artist, track) = (scrobble.artist().name(), scrobble.track().name());
        println!("{} {artist} - {track}", scrobble.timestamp());
    }
    Ok(())
}
