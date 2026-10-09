//! Decodes the recorded `user.getRecentTracks` fixtures and scans them.
//!
//! provenance: recorded and derived. The three `kelchm-*` bodies were
//! recorded by the repository owner from their own account, and the
//! now-playing page is derived with every value invented. See
//! `fixtures/README.md` for the details.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]

use scrobl::ErrorKind;
use scrobl::history::{RecentTracks, Window, WindowScan};
use scrobl::model::{RecentTracksPage, Scrobble};
use scrobl::protocol::{self, HttpResponse, Raw, Request};

/// The first page of the extended recording, `limit=2`.
const EXTENDED_1: &[u8] =
    include_bytes!("../fixtures/recorded/kelchm-recent-tracks-extended-page-1.json");
/// The second page of the same window.
const EXTENDED_2: &[u8] =
    include_bytes!("../fixtures/recorded/kelchm-recent-tracks-extended-page-2.json");
/// The same window without `extended`, as one page of `limit=200`.
const PLAIN: &[u8] = include_bytes!("../fixtures/recorded/kelchm-recent-tracks-plain.json");
/// A small extended page with a now-playing row. Every value is invented.
const NOW_PLAYING: &[u8] =
    include_bytes!("../fixtures/derived/recent-tracks-now-playing-extended.json");

/// The window of all three recordings: `[FROM, TO)`.
const FROM: u64 = 1_790_726_357;
const TO: u64 = 1_790_768_308;

fn window() -> Window {
    Window::new(FROM, TO).unwrap()
}

fn extended_scan() -> WindowScan {
    RecentTracks::new("kelchm")
        .window(window())
        .limit(2)
        .extended(true)
        .scan()
        .unwrap()
}

fn plain_scan() -> WindowScan {
    RecentTracks::new("kelchm")
        .window(window())
        .limit(200)
        .scan()
        .unwrap()
}

/// The body as the service would have answered `request`.
fn answer(request: &Request, body: &'static [u8]) -> Raw {
    protocol::decode(request, HttpResponse::new(200, body)).unwrap()
}

fn decode(body: &'static [u8]) -> RecentTracksPage {
    let request = Request::new(&protocol::methods::USER_GET_RECENT_TRACKS).param("user", "kelchm");
    let raw = answer(&request, body);
    assert_eq!(raw.body(), body, "the body is kept byte for byte");
    RecentTracksPage::decode(&raw).unwrap()
}

fn timestamps(scrobbles: &[Scrobble]) -> Vec<u64> {
    scrobbles.iter().map(Scrobble::timestamp).collect()
}

fn names(scrobbles: &[Scrobble]) -> Vec<&str> {
    scrobbles.iter().map(|s| s.track().name()).collect()
}

#[test]
fn the_first_extended_page_decodes() {
    let page = decode(EXTENDED_1);

    let attr = page.attr();
    assert_eq!(attr.user(), "kelchm");
    assert_eq!(attr.page(), 1);
    assert_eq!(attr.per_page(), 2);
    assert_eq!(attr.total_pages(), 2);
    assert_eq!(attr.total(), 4);

    assert!(page.now_playing().is_none());
    let scrobbles = page.scrobbles();
    assert_eq!(timestamps(scrobbles), [1_790_768_307, 1_790_726_910]);
    assert_eq!(names(scrobbles), ["1996", "Fool To Think"]);

    let first = &scrobbles[0];
    assert_eq!(first.artist().name(), "The Wombats");
    assert_eq!(
        first.artist().url(),
        Some("https://www.last.fm/music/The+Wombats")
    );
    assert_eq!(
        first.track().mbid(),
        Some("a771c5bd-0575-3f71-9c08-a6462f6f9662")
    );
    assert_eq!(
        first.album().title(),
        Some("The Wombats Proudly Present... This Modern Glitch")
    );
    assert_eq!(first.album().mbid(), None);
    assert_eq!(scrobbles[1].artist().name(), "Dave Matthews Band");
    assert!(scrobbles.iter().all(|s| s.loved() == Some(false)));
}

#[test]
fn the_second_extended_page_decodes() {
    let page = decode(EXTENDED_2);

    let attr = page.attr();
    assert_eq!(attr.user(), "kelchm");
    assert_eq!(attr.page(), 2);
    assert_eq!(attr.per_page(), 2);
    assert_eq!(attr.total_pages(), 2);
    assert_eq!(attr.total(), 4);

    assert!(page.now_playing().is_none());
    let scrobbles = page.scrobbles();
    assert_eq!(timestamps(scrobbles), [1_790_726_590, 1_790_726_357]);
    assert_eq!(names(scrobbles), ["Hold Me Down", "Settle"]);
    assert_eq!(scrobbles[0].artist().name(), "Motion City Soundtrack");
    assert_eq!(scrobbles[1].artist().name(), "Two Door Cinema Club");
    assert!(scrobbles.iter().all(|s| s.loved() == Some(false)));
}

#[test]
fn the_plain_page_decodes() {
    let page = decode(PLAIN);

    let attr = page.attr();
    assert_eq!(attr.user(), "kelchm");
    assert_eq!(attr.page(), 1);
    assert_eq!(attr.per_page(), 200);
    assert_eq!(attr.total_pages(), 1);
    assert_eq!(attr.total(), 4);

    assert!(page.now_playing().is_none());
    let scrobbles = page.scrobbles();
    assert_eq!(
        timestamps(scrobbles),
        [1_790_768_307, 1_790_726_910, 1_790_726_590, 1_790_726_357]
    );
    assert_eq!(
        names(scrobbles),
        ["1996", "Fool To Think", "Hold Me Down", "Settle"]
    );
    // Without `extended` the artist is a name and an id, and there is no
    // `loved`.
    assert_eq!(scrobbles[0].artist().name(), "The Wombats");
    assert_eq!(
        scrobbles[0].artist().mbid(),
        Some("e105c272-b5d7-4135-82ef-d60bded54345")
    );
    assert_eq!(scrobbles[0].artist().url(), None);
    assert_eq!(scrobbles[3].artist().name(), "Two Door Cinema Club");
    assert!(scrobbles.iter().all(|s| s.loved().is_none()));
    assert!(scrobbles.iter().all(|s| s.json().get("loved").is_none()));
}

#[test]
fn the_extended_scan_reads_both_recorded_pages() {
    let mut scan = extended_scan();

    let first = scan.next_request().unwrap();
    assert_eq!(first.get("user"), Some("kelchm"));
    assert_eq!(first.get("from"), Some("1790726357"));
    assert_eq!(first.get("to"), Some("1790768308"));
    assert_eq!(first.get("limit"), Some("2"));
    assert_eq!(first.get("extended"), Some("1"));
    assert_eq!(first.get("page"), Some("1"));
    let page_1 = scan.accept(answer(&first, EXTENDED_1)).unwrap();
    assert_eq!(page_1.number(), 1);
    assert_eq!(
        timestamps(page_1.scrobbles()),
        [1_790_768_307, 1_790_726_910]
    );
    assert_eq!(page_1.raw().body(), EXTENDED_1);

    let second = scan.next_request().unwrap();
    assert_eq!(second.get("user"), Some("kelchm"));
    assert_eq!(second.get("from"), Some("1790726357"));
    assert_eq!(second.get("to"), Some("1790768308"));
    assert_eq!(second.get("limit"), Some("2"));
    assert_eq!(second.get("extended"), Some("1"));
    assert_eq!(second.get("page"), Some("2"));
    let page_2 = scan.accept(answer(&second, EXTENDED_2)).unwrap();
    assert_eq!(page_2.number(), 2);
    assert_eq!(
        timestamps(page_2.scrobbles()),
        [1_790_726_590, 1_790_726_357]
    );
    assert_eq!(page_2.raw().body(), EXTENDED_2);

    assert!(scan.next_request().is_none(), "two pages complete the scan");
    let summary = scan.finish().unwrap();
    assert_eq!(summary.user(), "kelchm");
    assert_eq!(summary.window(), window());
    assert_eq!(summary.total(), 4);
    assert_eq!(summary.pages(), 2);
}

#[test]
fn the_plain_scan_reads_the_recorded_page() {
    let mut scan = plain_scan();

    let request = scan.next_request().unwrap();
    assert_eq!(request.get("user"), Some("kelchm"));
    assert_eq!(request.get("from"), Some("1790726357"));
    assert_eq!(request.get("to"), Some("1790768308"));
    assert_eq!(request.get("limit"), Some("200"));
    assert_eq!(request.get("extended"), None);
    assert_eq!(request.get("page"), Some("1"));
    let page = scan.accept(answer(&request, PLAIN)).unwrap();
    assert_eq!(page.number(), 1);
    assert_eq!(page.scrobbles().len(), 4);

    assert!(scan.next_request().is_none(), "one page completes the scan");
    let summary = scan.finish().unwrap();
    assert_eq!(summary.total(), 4);
    assert_eq!(summary.pages(), 1);
}

#[test]
fn the_plain_and_extended_readings_agree() {
    let extended: Vec<Scrobble> = [EXTENDED_1, EXTENDED_2]
        .into_iter()
        .flat_map(|body| decode(body).scrobbles().to_vec())
        .collect();
    let plain = decode(PLAIN);

    assert_eq!(timestamps(&extended), timestamps(plain.scrobbles()));
    assert_eq!(names(&extended), names(plain.scrobbles()));
    for (a, b) in extended.iter().zip(plain.scrobbles()) {
        assert_eq!(a.artist().name(), b.artist().name());
        assert_eq!(a.track().mbid(), b.track().mbid());
        assert_eq!(a.album().title(), b.album().title());
    }
}

#[test]
fn page_2_where_page_1_is_expected_breaks_rule_2() {
    // The response is labelled as the answer to page 1, so the request check
    // passes and the body's own `@attr.page` of 2 is what gives it away.
    let mut scan = extended_scan();
    let request = scan.next_request().unwrap();
    let error = scan.accept(answer(&request, EXTENDED_2)).unwrap_err();
    assert_eq!(error.kind(), ErrorKind::Inconsistent);
    assert!(error.to_string().contains("rule 2"), "{error}");
    assert!(error.to_string().contains("`page`"), "{error}");

    assert!(
        scan.next_request().is_none(),
        "a failed scan issues nothing"
    );
    assert_eq!(scan.finish().unwrap_err().kind(), ErrorKind::Inconsistent);
}

#[test]
fn a_response_to_the_page_2_request_is_refused_while_page_1_is_outstanding() {
    // A second scan gives the request for page 2 without disturbing the first.
    let mut ahead = extended_scan();
    let first = ahead.next_request().unwrap();
    ahead.accept(answer(&first, EXTENDED_1)).unwrap();
    let page_2_request = ahead.next_request().unwrap();

    let mut scan = extended_scan();
    let error = scan
        .accept(answer(&page_2_request, EXTENDED_2))
        .unwrap_err();
    assert_eq!(error.kind(), ErrorKind::Inconsistent);
    assert!(
        error.to_string().contains("answers a different request"),
        "{error}"
    );
    assert!(
        scan.next_request().is_none(),
        "a failed scan issues nothing"
    );
}

#[test]
fn a_narrower_window_refuses_a_recorded_scrobble_outside_it() {
    // `to` is exclusive, so 1790768307 falls outside [FROM, 1790768307).
    let mut scan = RecentTracks::new("kelchm")
        .window(Window::new(FROM, 1_790_768_307).unwrap())
        .limit(2)
        .extended(true)
        .scan()
        .unwrap();
    let request = scan.next_request().unwrap();
    let error = scan.accept(answer(&request, EXTENDED_1)).unwrap_err();
    assert_eq!(error.kind(), ErrorKind::Inconsistent);
    assert!(error.to_string().contains("rule 5"), "{error}");
    assert!(
        scan.next_request().is_none(),
        "a failed scan issues nothing"
    );
    assert_eq!(scan.finish().unwrap_err().kind(), ErrorKind::Inconsistent);
}

#[test]
fn a_later_start_refuses_the_last_recorded_scrobble() {
    // `from` is inclusive: the scrobble at exactly FROM is inside the real
    // window and outside one that starts a second later. Page 1 holds
    // nothing that old, so the failure comes on page 2.
    let mut scan = RecentTracks::new("kelchm")
        .window(Window::new(FROM + 1, TO).unwrap())
        .limit(2)
        .extended(true)
        .scan()
        .unwrap();
    let first = scan.next_request().unwrap();
    scan.accept(answer(&first, EXTENDED_1)).unwrap();
    let second = scan.next_request().unwrap();
    let error = scan.accept(answer(&second, EXTENDED_2)).unwrap_err();
    assert_eq!(error.kind(), ErrorKind::Inconsistent);
    assert!(error.to_string().contains("rule 5"), "{error}");
}

#[test]
fn the_now_playing_page_decodes_with_its_now_playing_row() {
    let page = decode(NOW_PLAYING);

    let attr = page.attr();
    assert_eq!(attr.user(), "example_listener");
    assert_eq!(attr.page(), 1);
    assert_eq!(attr.per_page(), 200);
    assert_eq!(attr.total_pages(), 1);
    assert_eq!(attr.total(), 2);

    let playing = page.now_playing().expect("the first row is now-playing");
    assert_eq!(playing.track().name(), "Slow Harbour");
    assert_eq!(
        playing.track().mbid(),
        Some("00000000-0000-4000-8000-000000000001")
    );
    assert_eq!(playing.artist().name(), "Paper Lanterns Choir");
    assert_eq!(playing.artist().mbid(), None);
    assert_eq!(
        playing.artist().url(),
        Some("https://www.last.fm/music/Paper+Lanterns+Choir")
    );
    assert_eq!(playing.loved(), Some(false));
    assert_eq!(playing.json()["@attr"]["nowplaying"], "true");
    assert!(playing.json().get("date").is_none());

    let scrobbles = page.scrobbles();
    assert_eq!(scrobbles.len(), 2, "the now-playing row is not a scrobble");
    assert_eq!(timestamps(scrobbles), [1_700_000_300, 1_700_000_100]);
    assert_eq!(names(scrobbles), ["Salt Flats", "Second Lantern"]);
}

#[test]
fn the_now_playing_page_keeps_empty_mbids_distinct_from_missing_ones() {
    let page = decode(NOW_PLAYING);
    let salt_flats = &page.scrobbles()[0];

    assert_eq!(salt_flats.track().name(), "Salt Flats");
    assert_eq!(salt_flats.track().mbid(), None);
    assert_eq!(salt_flats.artist().name(), "Quiet Meridian");
    assert_eq!(salt_flats.artist().mbid(), None);
    assert_eq!(salt_flats.album().title(), Some("Long Weather"));
    assert_eq!(salt_flats.album().mbid(), None);
    assert_eq!(salt_flats.loved(), Some(false));
    // The service sent empty strings; they are present in the row.
    assert_eq!(salt_flats.json()["mbid"], "");
    assert_eq!(salt_flats.json()["artist"]["mbid"], "");
    assert_eq!(salt_flats.json()["album"]["mbid"], "");

    let with_ids = &page.scrobbles()[1];
    assert_eq!(with_ids.track().name(), "Second Lantern");
    assert_eq!(
        with_ids.track().mbid(),
        Some("00000000-0000-4000-8000-000000000003")
    );
    assert_eq!(
        with_ids.album().mbid(),
        Some("00000000-0000-4000-8000-000000000004")
    );
    assert_eq!(with_ids.loved(), Some(true));
}

#[test]
fn a_scan_does_not_count_the_now_playing_row() {
    let mut scan = RecentTracks::new("example_listener")
        .extended(true)
        .scan()
        .unwrap();
    let request = scan.next_request().unwrap();
    let page = scan.accept(answer(&request, NOW_PLAYING)).unwrap();
    assert_eq!(page.scrobbles().len(), 2);
    assert!(page.now_playing().is_some());

    assert!(scan.next_request().is_none());
    let summary = scan.finish().unwrap();
    assert_eq!(summary.total(), 2);
    assert_eq!(summary.pages(), 1);
}
