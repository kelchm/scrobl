//! Decodes the recorded `user.getRecentTracks` fixtures.
//!
//! provenance: recorded. See `fixtures/README.md` for the source, licence and
//! what was trimmed.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]

use scrobl::ErrorKind;
use scrobl::history::RecentTracks;
use scrobl::model::RecentTracksPage;
use scrobl::protocol::{self, HttpResponse, Raw, Request, methods};

/// An extended page of user `loige` at `perPage` 200, trimmed to its first
/// four rows: the now-playing row and three scrobbles. Rows and `@attr` are
/// verbatim. The `@attr` still describes the whole page (200 rows, total
/// 290860), so the trimmed file is **not** a consistent page.
const LASTFM_EXCERPT: &[u8] =
    include_bytes!("../fixtures/recorded/lastfm-0.10.0-recent-tracks-extended-trimmed.json");

fn raw_of(body: &'static [u8]) -> Raw {
    let request = Request::new(&methods::USER_GET_RECENT_TRACKS).param("user", "loige");
    protocol::decode(&request, HttpResponse::new(200, body)).unwrap()
}

#[test]
fn lastfm_excerpt_decodes_with_its_now_playing_row() {
    let raw = raw_of(LASTFM_EXCERPT);
    assert_eq!(raw.body(), LASTFM_EXCERPT, "the body is kept byte for byte");
    let page = RecentTracksPage::decode(&raw).unwrap();

    let attr = page.attr();
    assert_eq!(attr.user(), "loige");
    assert_eq!(attr.page(), 1);
    assert_eq!(attr.per_page(), 200);
    assert_eq!(attr.total_pages(), 1455);
    assert_eq!(attr.total(), 290_860);

    let playing = page.now_playing().expect("the first row is now-playing");
    assert_eq!(playing.track().name(), "Do You Feel");
    assert_eq!(
        playing.track().mbid(),
        Some("7ece0813-7980-442f-a801-38379723a540")
    );
    assert_eq!(playing.artist().name(), "The Rocket Summer");
    assert_eq!(playing.artist().mbid(), None);
    assert_eq!(
        playing.artist().url(),
        Some("https://www.last.fm/music/The+Rocket+Summer")
    );
    assert_eq!(playing.loved(), Some(false));
    assert_eq!(playing.json()["@attr"]["nowplaying"], "true");
    assert!(playing.json().get("date").is_none());

    let scrobbles = page.scrobbles();
    assert_eq!(scrobbles.len(), 3, "the now-playing row is not a scrobble");
    let timestamps: Vec<_> = scrobbles.iter().map(|s| s.timestamp()).collect();
    assert_eq!(timestamps, [1_677_005_663, 1_677_005_487, 1_677_005_302]);
    assert!(timestamps.is_sorted_by(|a, b| a >= b));
}

#[test]
fn lastfm_excerpt_keeps_empty_mbids_distinct_from_missing_ones() {
    let page = RecentTracksPage::decode(&raw_of(LASTFM_EXCERPT)).unwrap();
    let cornflakes = &page.scrobbles()[0];

    assert_eq!(cornflakes.track().name(), "CORNFLAKES");
    assert_eq!(cornflakes.track().mbid(), None);
    assert_eq!(cornflakes.artist().name(), "Comete");
    assert_eq!(cornflakes.artist().mbid(), None);
    assert_eq!(cornflakes.album().title(), Some("Solo cose belle"));
    assert_eq!(cornflakes.album().mbid(), None);
    assert_eq!(cornflakes.loved(), Some(false));
    // The service sent empty strings; they are present in the row.
    assert_eq!(cornflakes.json()["mbid"], "");
    assert_eq!(cornflakes.json()["artist"]["mbid"], "");
    assert_eq!(cornflakes.json()["album"]["mbid"], "");

    let with_ids = &page.scrobbles()[1];
    assert_eq!(with_ids.track().name(), "Majoring In Minors");
    assert_eq!(
        with_ids.track().mbid(),
        Some("8f5264b7-258e-3f5c-bfb0-e98e2a92adec")
    );
    assert_eq!(
        with_ids.album().mbid(),
        Some("6dece74b-df9b-4263-a151-b0942d55bec0")
    );
    assert_eq!(with_ids.loved(), Some(true));
}

#[test]
fn a_trimmed_excerpt_is_not_a_consistent_page() {
    // The @attr promises 200 rows; the excerpt holds 3. A scan must refuse
    // it, which is also why it is used only for row-shape decoding.
    let mut scan = RecentTracks::new("loige").extended(true).scan().unwrap();
    let request = scan.next_request().unwrap();
    let raw = protocol::decode(&request, HttpResponse::new(200, LASTFM_EXCERPT)).unwrap();
    let error = scan.accept(raw).unwrap_err();
    assert_eq!(error.kind(), ErrorKind::Inconsistent);
    assert!(error.to_string().contains("rule 4"), "{error}");
}
