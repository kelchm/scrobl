//! What the tests share: the fake Last.fm of the library's own tests, and a
//! database in a directory of its own.
//!
//! provenance: synthetic. The fake service generates every response.
//!
//! The fake is included from `crates/scrobl/tests/support` by path, not
//! copied, so both crates are tested against the same service. `server.rs`
//! reaches the dataset through `super`, which here is this module.

#![allow(
    dead_code,
    unused_imports,
    unreachable_pub,
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]

// The library gates its socket server on a feature this crate does not have,
// so the server is included on its own below.
#[allow(unexpected_cfgs)]
#[path = "../../../scrobl/tests/support/mod.rs"]
pub mod support;

#[path = "../../../scrobl/tests/support/server.rs"]
pub mod server;

use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;

use scrobl::{ApiKey, Client};
pub use server::{Behaviour, FakeLastfm};
pub use support::{BASE, Dataset, Faults, Pages, respond_to};

/// The user of every synthetic dataset, in the service's own spelling.
pub const USER: &str = "rj_Synthetic";

/// A client for the fake service: no pacing and a single attempt, so a
/// scripted failure is the failure the test sees.
pub fn client(server: &FakeLastfm) -> Client {
    Client::builder(ApiKey::new("SENTINEL_API_KEY_0001"))
        .base_url(server.base_url())
        .min_interval(Duration::ZERO)
        .read_attempts(1)
        .build()
        .unwrap()
}

/// A history with a scrobble at each of `times`, each named after its time.
/// A dataset made from fewer or more times names its rows afresh, and to a
/// sync a scrobble with another name is another scrobble; these keep theirs
/// when others come and go.
pub fn named(times: impl IntoIterator<Item = u64>) -> Dataset {
    let mut dataset = Dataset::from_timestamps(times.into_iter().collect());
    for entry in &mut dataset.entries {
        entry.name = format!("At {}", entry.ts);
    }
    dataset
}

/// What the fake is asked for one page of the history in `[from, to)`, to
/// work out an answer a test wants to script. A `from` of 0 is no lower
/// bound, as a sync sends it.
pub fn question(from: u64, to: u64, limit: u32, page: u32) -> Vec<(String, String)> {
    let mut question = vec![
        ("method", "user.getRecentTracks".to_owned()),
        ("user", USER.to_owned()),
        ("api_key", "k".to_owned()),
        ("limit", limit.to_string()),
        ("page", page.to_string()),
        ("to", to.to_string()),
    ];
    if from > 0 {
        question.push(("from", from.to_string()));
    }
    question
        .into_iter()
        .map(|(name, value)| (name.to_owned(), value))
        .collect()
}

/// A directory that is removed when the test ends.
pub struct Scratch(PathBuf);

impl Scratch {
    pub fn new() -> Self {
        static NEXT: AtomicUsize = AtomicUsize::new(0);
        let path = std::env::temp_dir().join(format!(
            "scrobl-cli-test-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::SeqCst)
        ));
        std::fs::create_dir_all(&path).unwrap();
        Self(path)
    }

    /// The path of a database in the directory.
    pub fn db(&self) -> PathBuf {
        self.0.join("history.db")
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}
