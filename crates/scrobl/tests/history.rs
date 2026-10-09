//! Regression gates for the history read path.
//!
//! Every test drives the public API only: `protocol::prepare`, the fake
//! service in `support`, `protocol::decode` and `WindowScan::accept`. Each
//! gate exists because an audited Last.fm crate gets the case wrong in a way
//! that looks right: duplicated and omitted rows behind a correct row count,
//! a bound dropped after an empty page, now-playing rows displacing real
//! scrobbles, API errors read as empty history.
//!
//! provenance: synthetic. The fake service generates every response; the few
//! hand-written pages are derived from the shapes in the recorded and
//! community-documented responses.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]

mod support;

use std::collections::BTreeSet;

use proptest::prelude::*;
use scrobl::history::{RecentTracks, ScanPage, ScanSummary, Window, WindowScan};
use scrobl::model::RecentTracksPage;
use scrobl::protocol::{self, Credentials, HttpRequest, HttpResponse, Raw, Request};
use scrobl::{ApiErrorCode, ApiKey, Error, ErrorKind};
use serde_json::{Value, json};
use support::*;

const USER: &str = "RJ_SYNTHETIC";

fn credentials() -> Credentials {
    Credentials::new(ApiKey::new("SENTINEL_API_KEY_0001"))
}

fn window(from: u64, to: u64) -> Window {
    Window::new(from, to).unwrap()
}

/// prepare -> fake service -> decode, for one request of a scan.
fn fetch(dataset: &Dataset, request: &Request) -> (HttpRequest, Result<Raw, Error>) {
    let http = protocol::prepare(&credentials(), request).unwrap();
    let (status, body) = respond(dataset, &http);
    let raw = protocol::decode(request, HttpResponse::new(status, body));
    (http, raw)
}

struct Driven {
    requests: Vec<HttpRequest>,
    pages: Vec<ScanPage>,
    /// The first error, from the transport layer or from the scan.
    error: Option<Error>,
    scan: WindowScan,
}

impl Driven {
    fn timestamps(&self) -> Vec<u64> {
        self.pages
            .iter()
            .flat_map(|page| page.scrobbles())
            .map(|s| s.timestamp())
            .collect()
    }

    fn markers(&self) -> Vec<usize> {
        self.pages
            .iter()
            .flat_map(|page| page.scrobbles())
            .map(|s| marker_of(s.track().mbid()))
            .collect()
    }

    fn error(&self) -> &Error {
        self.error.as_ref().expect("the scan should have failed")
    }
}

/// Runs a scan against the fake until it ends or an error stops it.
fn drive(dataset: &Dataset, mut scan: WindowScan) -> Driven {
    let mut requests = Vec::new();
    let mut pages = Vec::new();
    let mut error = None;
    while let Some(request) = scan.next_request() {
        assert!(requests.len() < 20_000, "the scan does not terminate");
        let (http, raw) = fetch(dataset, &request);
        requests.push(http);
        match raw.and_then(|raw| scan.accept(raw)) {
            Ok(page) => pages.push(page),
            Err(e) => {
                error = Some(e);
                break;
            }
        }
    }
    Driven {
        requests,
        pages,
        error,
        scan,
    }
}

fn scan_of(window: Window, page_size: u32, extended: bool) -> WindowScan {
    RecentTracks::new(USER)
        .window(window)
        .limit(page_size)
        .extended(extended)
        .scan()
        .unwrap()
}

struct Completed {
    requests: Vec<HttpRequest>,
    pages: Vec<ScanPage>,
    summary: ScanSummary,
}

impl Completed {
    fn markers(&self) -> Vec<usize> {
        self.pages
            .iter()
            .flat_map(|page| page.scrobbles())
            .map(|s| marker_of(s.track().mbid()))
            .collect()
    }
}

/// Runs a scan that must succeed and finish.
#[track_caller]
fn complete(dataset: &Dataset, window: Window, page_size: u32, extended: bool) -> Completed {
    let driven = drive(dataset, scan_of(window, page_size, extended));
    if let Some(error) = &driven.error {
        panic!("the scan failed: {error:?}");
    }
    let summary = driven.scan.finish().unwrap();
    Completed {
        requests: driven.requests,
        pages: driven.pages,
        summary,
    }
}

fn pages_for(total: usize, page_size: usize) -> usize {
    total.div_ceil(page_size).max(1)
}

/// A scan that failed refuses everything afterwards.
#[track_caller]
fn assert_poisoned(mut driven: Driven) {
    assert!(driven.scan.next_request().is_none(), "a request was issued");
    let clean = Dataset::distinct(3);
    let first = RecentTracks::new(USER)
        .scan()
        .unwrap()
        .next_request()
        .unwrap();
    let raw = fetch(&clean, &first).1.unwrap();
    let refused = driven.scan.accept(raw).unwrap_err();
    assert_eq!(refused.kind(), ErrorKind::Inconsistent);
    assert!(driven.scan.finish().is_err(), "finish succeeded");
}

// ---------------------------------------------------------------------------
// Gate 1: constant page size and identical bounds on every request.

#[test]
fn gate1_every_request_has_the_same_limit_and_bounds() {
    let dataset = Dataset::distinct(1001);
    let bounded = window(BASE + 500, BASE + 9_000);
    for page_size in [200_u32, 37, 1] {
        let done = complete(&dataset, bounded, page_size, false);
        let total = dataset.expected(bounded).len();
        assert!(total > 600, "the window should span several pages");
        assert_eq!(done.requests.len(), pages_for(total, page_size as usize));

        let names: BTreeSet<_> = query(&done.requests[0])
            .into_iter()
            .map(|(k, _)| k)
            .collect();
        for (index, request) in done.requests.iter().enumerate() {
            let this: BTreeSet<_> = query(request).into_iter().map(|(k, _)| k).collect();
            assert_eq!(this, names, "request {index} has a different parameter set");
            assert_eq!(
                param(request, "limit").as_deref(),
                Some(&*page_size.to_string())
            );
            assert_eq!(
                param(request, "from").as_deref(),
                Some(&*(BASE + 500).to_string()),
                "request {index}"
            );
            assert_eq!(
                param(request, "to").as_deref(),
                Some(&*(BASE + 9_000).to_string()),
                "request {index}"
            );
            assert_eq!(
                param(request, "page").as_deref(),
                Some(&*(index + 1).to_string())
            );
            assert_eq!(param(request, "user").as_deref(), Some(USER));
        }
        for name in ["method", "api_key", "user", "limit", "page", "from", "to"] {
            assert!(names.contains(name), "`{name}` is missing");
        }
        assert_eq!(done.markers(), dataset.expected(bounded));
    }
}

#[test]
fn gate1_only_the_bounds_that_were_set_are_sent_and_always() {
    let dataset = Dataset::distinct(450);
    for (win, has_from, has_to) in [
        (Window::since(BASE + 1_000), true, false),
        (Window::before(BASE + 3_000), false, true),
        (Window::ALL, false, false),
    ] {
        let done = complete(&dataset, win, 200, false);
        assert!(done.requests.len() >= 2);
        for request in &done.requests {
            assert_eq!(param(request, "from").is_some(), has_from);
            assert_eq!(param(request, "to").is_some(), has_to);
            assert_eq!(param(request, "limit").as_deref(), Some("200"));
        }
        assert_eq!(done.markers(), dataset.expected(win));
    }
}

#[test]
fn gate1_default_page_size_is_200_and_extended_is_constant() {
    let dataset = Dataset::distinct(450);
    let driven = drive(
        &dataset,
        RecentTracks::new(USER).extended(true).scan().unwrap(),
    );
    assert!(driven.error.is_none());
    assert_eq!(driven.requests.len(), 3);
    for request in &driven.requests {
        assert_eq!(param(request, "limit").as_deref(), Some("200"));
        assert_eq!(param(request, "extended").as_deref(), Some("1"));
    }
}

// ---------------------------------------------------------------------------
// Gate 2: exact multiples and off-by-ones.

#[test]
fn gate2_chunk_multiples_yield_every_marker_exactly_once_in_order() {
    for total in [0, 1, 199, 200, 201, 400, 1000, 1001, 1501, 5000] {
        for extended in [false, true] {
            let dataset = Dataset::distinct(total);
            let done = complete(&dataset, Window::ALL, 200, extended);
            let expected: Vec<usize> = (0..total).collect();
            assert_eq!(
                done.markers(),
                expected,
                "total {total}, extended {extended}"
            );
            assert_eq!(done.summary.total(), total as u64);
            assert_eq!(done.summary.pages() as usize, pages_for(total, 200));
            assert_eq!(done.requests.len(), pages_for(total, 200));
            assert_eq!(done.summary.window(), Window::ALL);
        }
    }
}

#[test]
fn gate2_other_page_sizes_have_the_same_arithmetic() {
    for page_size in [1_u32, 2, 50, 100, 199] {
        for total in [
            0_usize,
            1,
            page_size as usize - 1,
            page_size as usize,
            page_size as usize + 1,
            3 * page_size as usize,
        ] {
            let dataset = Dataset::distinct(total);
            let done = complete(&dataset, Window::ALL, page_size, false);
            let expected: Vec<usize> = (0..total).collect();
            assert_eq!(done.markers(), expected, "size {page_size}, total {total}");
            assert_eq!(done.requests.len(), pages_for(total, page_size as usize));
        }
    }
}

#[test]
fn gate2_a_page_of_one_row_uses_the_single_object_shape() {
    let dataset = Dataset::distinct(3);
    let done = complete(&dataset, Window::ALL, 1, true);
    for page in &done.pages {
        let body: Value = serde_json::from_slice(page.raw().body()).unwrap();
        assert!(
            body["recenttracks"]["track"].is_object(),
            "page {}",
            page.number()
        );
    }
    assert_eq!(done.markers(), vec![0, 1, 2]);
}

#[test]
fn gate2_a_server_cap_equal_to_the_page_size_is_fine() {
    let dataset = Dataset::distinct(450).with_cap(100);
    let done = complete(&dataset, Window::ALL, 100, false);
    assert_eq!(done.markers(), (0..450).collect::<Vec<_>>());
}

// ---------------------------------------------------------------------------
// Gate 3: an empty page mid-scan fails, and no bound is ever dropped.

#[test]
fn gate3_a_page_without_rows_mid_scan_fails_and_issues_nothing_more() {
    let dataset = Dataset::distinct(1001).with_faults(Faults {
        empty_page: Some(3),
        ..Faults::default()
    });
    let bounded = window(BASE + 10, BASE + 100_000);
    let driven = drive(&dataset, scan_of(bounded, 200, false));

    let error = driven.error();
    assert_eq!(error.kind(), ErrorKind::Decode);
    assert!(error.to_string().contains("recenttracks.track"), "{error}");
    assert_eq!(
        driven.requests.len(),
        3,
        "a request followed the empty page"
    );
    for request in &driven.requests {
        assert_eq!(
            param(request, "from").as_deref(),
            Some(&*(BASE + 10).to_string())
        );
        assert_eq!(
            param(request, "to").as_deref(),
            Some(&*(BASE + 100_000).to_string())
        );
        assert_eq!(param(request, "limit").as_deref(), Some("200"));
    }
    assert_eq!(driven.pages.len(), 2);
    assert_poisoned(driven);
}

#[test]
fn gate3_a_first_page_without_rows_and_a_nonzero_total_fails() {
    let dataset = Dataset::distinct(300).with_faults(Faults {
        empty_page: Some(1),
        ..Faults::default()
    });
    let driven = drive(
        &dataset,
        scan_of(Window::before(BASE + 100_000), 200, false),
    );
    assert_eq!(driven.error().kind(), ErrorKind::Decode);
    assert_eq!(driven.requests.len(), 1);
    assert_poisoned(driven);
}

#[test]
fn gate3_a_page_with_only_the_now_playing_row_is_short_by_rule_4() {
    // `track` is not empty here, so the shape passes and the count fails.
    let dataset = Dataset::distinct(1001)
        .with_now_playing(false, Pages::All)
        .with_faults(Faults {
            empty_page: Some(3),
            ..Faults::default()
        });
    let driven = drive(&dataset, scan_of(Window::ALL, 200, false));
    let error = driven.error();
    assert_eq!(error.kind(), ErrorKind::Inconsistent);
    assert!(error.to_string().contains("rule 4"), "{error}");
    assert_eq!(driven.requests.len(), 3);
    assert_poisoned(driven);
}

#[test]
fn gate3_a_genuinely_empty_bounded_window_is_one_request_with_both_bounds() {
    let dataset = Dataset::distinct(100);
    let nothing = window(BASE + 50_000, BASE + 60_000);
    let done = complete(&dataset, nothing, 200, false);
    assert_eq!(done.requests.len(), 1);
    assert_eq!(
        param(&done.requests[0], "from").as_deref(),
        Some(&*(BASE + 50_000).to_string())
    );
    assert_eq!(
        param(&done.requests[0], "to").as_deref(),
        Some(&*(BASE + 60_000).to_string())
    );
    assert_eq!(done.summary.total(), 0);
    assert!(done.markers().is_empty());
}

// ---------------------------------------------------------------------------
// Gate 4: same-second multiplicity.

/// 190 distinct seconds, a run of 30 in one second that straddles the
/// boundary between pages 1 and 2, then 100 more distinct seconds.
fn straddling_run() -> Dataset {
    let mut timestamps = Vec::new();
    timestamps.extend((0..190).map(|i| BASE + 10_000 - i));
    timestamps.extend(std::iter::repeat_n(BASE + 5_000, 30));
    timestamps.extend((0..100).map(|i| BASE + 4_000 - i));
    Dataset::from_timestamps(timestamps).identical_rows()
}

#[test]
fn gate4_a_same_second_run_across_a_page_boundary_survives() {
    let dataset = straddling_run();
    let done = complete(&dataset, Window::ALL, 200, false);
    assert_eq!(done.markers(), (0..320).collect::<Vec<_>>());
    assert_eq!(done.summary.total(), 320);

    // The run is rows 190..220: on both sides of the page boundary at 200.
    let page_one = done.pages[0].scrobbles();
    let page_two = done.pages[1].scrobbles();
    assert_eq!(page_one.last().unwrap().timestamp(), BASE + 5_000);
    assert_eq!(page_two.first().unwrap().timestamp(), BASE + 5_000);
}

#[test]
fn gate4_windows_cut_at_the_run_keep_all_or_none_of_it() {
    let dataset = straddling_run();
    let second = BASE + 5_000;

    let only_the_run = complete(&dataset, window(second, second + 1), 7, false);
    assert_eq!(only_the_run.markers(), (190..220).collect::<Vec<_>>());

    // `from` is inclusive: the run and everything older.
    let from_the_run = complete(&dataset, Window::since(second), 200, false);
    assert_eq!(from_the_run.markers(), (0..220).collect::<Vec<_>>());

    // `to` is exclusive: the run is out.
    let before_the_run = complete(&dataset, Window::before(second), 200, false);
    assert_eq!(before_the_run.markers(), (220..320).collect::<Vec<_>>());
}

#[test]
fn gate4_identical_rows_in_one_second_are_all_kept() {
    let second = BASE + 1;
    let dataset = Dataset::from_timestamps(vec![second; 401]).identical_rows();
    let done = complete(&dataset, window(second, second + 1), 200, true);
    assert_eq!(done.markers(), (0..401).collect::<Vec<_>>());
    assert_eq!(done.summary.total(), 401);
    assert_eq!(done.summary.pages(), 3);

    // The rows differ only in the synthetic marker.
    let rows: Vec<_> = done.pages.iter().flat_map(|p| p.scrobbles()).collect();
    let first = rows[0];
    for row in &rows {
        assert_eq!(row.timestamp(), first.timestamp());
        assert_eq!(row.track().name(), first.track().name());
        assert_eq!(row.artist().name(), first.artist().name());
        assert_eq!(row.album().title(), first.album().title());
    }
    let unique: BTreeSet<_> = done.markers().into_iter().collect();
    assert_eq!(unique.len(), 401);
}

#[test]
fn gate4_byte_identical_rows_across_a_page_boundary_are_all_kept() {
    // 195 distinct seconds, ten listens of one song in one second that
    // straddle the boundary between pages 1 and 2, then 100 distinct
    // seconds. The ten share name, artist, album, ids and timestamp, so
    // nothing in a row tells one from another.
    let twin = BASE + 5_000;
    let mut timestamps = Vec::new();
    timestamps.extend((0..195).map(|i| BASE + 10_000 - i));
    timestamps.extend(std::iter::repeat_n(twin, 10));
    timestamps.extend((0..100).map(|i| BASE + 4_000 - i));
    let dataset = Dataset::from_timestamps(timestamps).byte_identical();

    let done = complete(&dataset, Window::ALL, 200, true);
    assert_eq!(done.summary.total(), 305);
    assert_eq!(done.summary.pages(), 2);
    let rows: Vec<_> = done.pages.iter().flat_map(|p| p.scrobbles()).collect();
    assert_eq!(rows.len(), 305);

    let twins: Vec<_> = rows.iter().filter(|r| r.timestamp() == twin).collect();
    assert_eq!(twins.len(), 10, "every listen is kept");
    let on_page = |number: usize| {
        done.pages[number]
            .scrobbles()
            .iter()
            .filter(|r| r.timestamp() == twin)
            .count()
    };
    assert_eq!(
        (on_page(0), on_page(1)),
        (5, 5),
        "the run straddles the boundary"
    );
    for row in &twins {
        assert_eq!(row.json(), twins[0].json());
        assert_eq!(row.json().to_string(), twins[0].json().to_string());
    }
    // Every other row is its own: 195 + 1 + 100 distinct rows in all.
    let distinct: BTreeSet<String> = rows.iter().map(|r| r.json().to_string()).collect();
    assert_eq!(distinct.len(), 296);
}

// ---------------------------------------------------------------------------
// Gate 5: now-playing.

#[test]
fn gate5_now_playing_is_never_counted_and_never_displaces_a_scrobble() {
    for total in [0_usize, 1, 199, 200, 201, 400] {
        for (with_date, on) in [
            (false, Pages::First),
            (true, Pages::First),
            (false, Pages::Last),
            (true, Pages::Last),
            (false, Pages::All),
            (true, Pages::All),
        ] {
            for extended in [false, true] {
                let dataset = Dataset::distinct(total).with_now_playing(with_date, on);
                let label = format!("total {total}, date {with_date}, {on:?}, extended {extended}");
                let done = complete(&dataset, Window::ALL, 200, extended);

                // The oldest scrobble is still there.
                assert_eq!(done.markers(), (0..total).collect::<Vec<_>>(), "{label}");
                assert_eq!(done.summary.total(), total as u64, "{label}");

                let last = done.pages.len();
                for page in &done.pages {
                    let expected = match on {
                        Pages::First => page.number() == 1,
                        Pages::Last => page.number() as usize == last,
                        Pages::All => true,
                    };
                    assert_eq!(page.now_playing().is_some(), expected, "{label}");
                    if let Some(playing) = page.now_playing() {
                        assert_eq!(playing.track().name(), "Now Playing Track");
                        assert_eq!(playing.json()["@attr"]["nowplaying"], "true");
                        assert_eq!(playing.json().get("date").is_some(), with_date);
                    }
                    // A full page of scrobbles, plus the now-playing row on
                    // the wire.
                    assert!(page.scrobbles().len() <= 200);
                    for scrobble in page.scrobbles() {
                        assert_ne!(marker_of(scrobble.track().mbid()), NOW_PLAYING_MARKER);
                    }
                }
            }
        }
    }
}

#[test]
fn gate5_a_now_playing_row_with_a_date_outside_the_window_is_not_checked_against_it() {
    let dataset = Dataset::distinct(450).with_now_playing(true, Pages::All);
    let win = window(BASE + 10, BASE + 5_000);
    let done = complete(&dataset, win, 100, true);
    assert_eq!(done.markers(), dataset.expected(win));
    assert!(done.pages.iter().all(|p| p.now_playing().is_some()));
}

#[test]
fn gate5_loved_comes_from_the_extended_shape_only() {
    let dataset = Dataset::distinct(5).with_now_playing(false, Pages::First);
    let plain = complete(&dataset, Window::ALL, 200, false);
    assert!(
        plain.pages[0]
            .scrobbles()
            .iter()
            .all(|s| s.loved().is_none())
    );
    let extended = complete(&dataset, Window::ALL, 200, true);
    assert!(
        extended.pages[0]
            .scrobbles()
            .iter()
            .all(|s| s.loved().is_some())
    );
    assert_eq!(
        extended.pages[0].now_playing().unwrap().loved(),
        Some(false)
    );
}

// ---------------------------------------------------------------------------
// Gate 6: malformed pages are errors, never an empty or shortened success.

#[test]
fn gate6_a_missing_or_non_numeric_date_is_an_error_naming_the_row() {
    for how in [BadDate::Missing, BadDate::NonNumeric] {
        for (page, row) in [(1_u32, 0_usize), (1, 5), (2, 7)] {
            let dataset = Dataset::distinct(450).with_faults(Faults {
                bad_date: Some((page, row, how)),
                ..Faults::default()
            });
            let driven = drive(&dataset, scan_of(Window::ALL, 200, false));
            let error = driven.error();
            assert_eq!(error.kind(), ErrorKind::Decode, "{how:?} {page}/{row}");
            assert!(
                error
                    .to_string()
                    .contains(&format!("recenttracks.track[{row}].date.uts")),
                "{error}"
            );
            assert_eq!(driven.pages.len() as u32, page - 1, "no shortened success");
            assert_poisoned(driven);
        }
    }
}

#[test]
fn gate6_a_bad_date_on_a_single_row_page_is_still_an_error() {
    let dataset = Dataset::distinct(1).with_faults(Faults {
        bad_date: Some((1, 0, BadDate::Missing)),
        ..Faults::default()
    });
    let driven = drive(&dataset, scan_of(Window::ALL, 200, true));
    assert_eq!(driven.error().kind(), ErrorKind::Decode);
    assert!(driven.pages.is_empty());
    assert_poisoned(driven);
}

#[test]
fn gate6_a_missing_attr_is_an_error() {
    for page in [1_u32, 2] {
        let dataset = Dataset::distinct(300).with_faults(Faults {
            missing_attr_on_page: Some(page),
            ..Faults::default()
        });
        let driven = drive(&dataset, scan_of(Window::ALL, 200, false));
        let error = driven.error();
        assert_eq!(error.kind(), ErrorKind::Decode);
        assert!(error.to_string().contains("recenttracks.@attr"), "{error}");
        assert_eq!(driven.pages.len() as u32, page - 1);
        assert_poisoned(driven);
    }
}

#[test]
fn gate6_a_missing_total_is_an_error() {
    for page in [1_u32, 2] {
        let dataset = Dataset::distinct(300).with_faults(Faults {
            missing_total_on_page: Some(page),
            ..Faults::default()
        });
        let driven = drive(&dataset, scan_of(Window::ALL, 200, false));
        let error = driven.error();
        assert_eq!(error.kind(), ErrorKind::Decode);
        assert!(
            error.to_string().contains("recenttracks.@attr.total"),
            "{error}"
        );
        assert_poisoned(driven);
    }
}

#[test]
fn gate6_an_empty_history_shape_with_no_attr_is_not_an_empty_success() {
    let mut scan = RecentTracks::new(USER).scan().unwrap();
    let raw = answer(&scan, &json!({"recenttracks": {"track": []}}));
    assert_eq!(scan.accept(raw).unwrap_err().kind(), ErrorKind::Decode);
    assert!(scan.finish().is_err());
}

#[test]
fn gate6_a_repeated_member_is_a_decode_error_and_poisons_the_scan() {
    // Two `track` members, the first holding the promised row. Read through a
    // map, the second wins and the row vanishes behind an `Ok` scan.
    let row = hand_row(BASE + 5).to_string();
    let body = format!(
        r#"{{"recenttracks": {{"track": [{row}], "track": [],
            "@attr": {{"user": "{USER}", "page": "1", "perPage": "200",
            "totalPages": "1", "total": "1"}}}}}}"#
    );
    let mut scan = scan_for(USER, Window::ALL, 200);
    let request = scan.next_request().unwrap();
    let raw = protocol::decode(&request, HttpResponse::new(200, body)).unwrap();
    let error = scan.accept(raw).unwrap_err();
    assert_eq!(error.kind(), ErrorKind::Decode);
    assert!(
        error.to_string().contains("repeats a member name"),
        "{error}"
    );
    assert!(scan.next_request().is_none());
    assert!(scan.finish().is_err());
}

// ---------------------------------------------------------------------------
// Gate 7: API errors are errors, and an empty window is not one.

#[test]
fn gate7_error_envelopes_at_http_200_are_api_errors_with_the_code() {
    for (code, constant) in [
        (17, ApiErrorCode::LOGIN_REQUIRED),
        (6, ApiErrorCode::INVALID_PARAMETERS),
    ] {
        for on_page in [None, Some(1_u32), Some(2)] {
            let dataset = Dataset::distinct(450).with_faults(Faults {
                api_error: Some((on_page, code)),
                ..Faults::default()
            });
            let driven = drive(&dataset, scan_of(Window::ALL, 200, false));
            let error = driven.error();
            assert_eq!(
                error.kind(),
                ErrorKind::Api,
                "code {code}, page {on_page:?}"
            );
            assert_eq!(error.api_code(), Some(constant));
            assert_eq!(error.http_status(), Some(200));
            assert_eq!(error.method(), Some("user.getRecentTracks"));
            // The envelope never reached the scan, so it is not poisoned: a
            // transient error can be retried. It cannot be finished.
            assert!(driven.scan.next_request().is_some());
            assert!(driven.scan.finish().is_err());
        }
    }
}

#[test]
fn gate7_an_error_is_distinguishable_from_a_genuinely_empty_window() {
    let hidden = Dataset::distinct(450).with_faults(Faults {
        api_error: Some((None, 17)),
        ..Faults::default()
    });
    let hidden_run = drive(&hidden, scan_of(Window::ALL, 200, false));
    assert_eq!(hidden_run.error().kind(), ErrorKind::Api);

    for empty_total_pages in [0_u64, 1] {
        let mut empty = Dataset::distinct(450);
        empty.empty_total_pages = empty_total_pages;
        let done = complete(&empty, window(BASE + 50_000, BASE + 60_000), 200, false);
        assert_eq!(done.summary.total(), 0);
        assert_eq!(done.summary.pages(), 1);
        assert!(done.markers().is_empty());
    }

    let none_at_all = complete(&Dataset::distinct(0), Window::ALL, 200, false);
    assert_eq!(none_at_all.summary.total(), 0);
}

#[test]
fn gate7_an_unknown_user_is_an_api_error_not_an_empty_history() {
    let dataset = Dataset::distinct(10);
    let scan = RecentTracks::new("nobody").scan().unwrap();
    let request = scan.next_request().unwrap();
    let error = fetch(&dataset, &request).1.unwrap_err();
    assert_eq!(error.kind(), ErrorKind::Api);
    assert_eq!(error.api_code(), Some(ApiErrorCode::INVALID_PARAMETERS));
    assert_eq!(error.http_status(), Some(404));
    assert!(scan.next_request().is_some());
}

#[test]
fn gate7_a_truncated_body_is_a_decode_error_and_retryable() {
    let dataset = Dataset::distinct(450).with_faults(Faults {
        truncate_on_page: Some(2),
        ..Faults::default()
    });
    let driven = drive(&dataset, scan_of(Window::ALL, 200, false));
    assert_eq!(driven.error().kind(), ErrorKind::Decode);
    assert_eq!(driven.pages.len(), 1);
    assert!(driven.scan.finish().is_err());
}

// ---------------------------------------------------------------------------
// Gate 8: every fault switch is caught.

#[derive(Clone)]
struct FaultCase {
    name: &'static str,
    total: usize,
    window: Window,
    page_size: u32,
    cap: u32,
    faults: Faults,
    kind: ErrorKind,
    /// Text the error must contain: the rule that failed, or the field.
    names: &'static str,
    /// Pages accepted before the failing one.
    accepted: usize,
}

fn fault_cases() -> Vec<FaultCase> {
    let base = FaultCase {
        name: "",
        total: 450,
        window: Window::ALL,
        page_size: 200,
        cap: 200,
        faults: Faults::default(),
        kind: ErrorKind::Inconsistent,
        names: "",
        accepted: 0,
    };
    vec![
        FaultCase {
            name: "wrong user on the first page",
            faults: Faults {
                wrong_user_on_page: Some(1),
                ..Faults::default()
            },
            names: "rule 1",
            ..base.clone()
        },
        FaultCase {
            name: "wrong user on a later page",
            faults: Faults {
                wrong_user_on_page: Some(2),
                ..Faults::default()
            },
            names: "rule 1",
            accepted: 1,
            ..base.clone()
        },
        FaultCase {
            name: "a scrobble arrives mid-scan",
            faults: Faults {
                arrive_from_page: Some(2),
                ..Faults::default()
            },
            names: "rule 3",
            accepted: 1,
            ..base.clone()
        },
        FaultCase {
            name: "a scrobble is deleted mid-scan",
            faults: Faults {
                delete_from_page: Some(3),
                ..Faults::default()
            },
            names: "rule 3",
            accepted: 2,
            ..base.clone()
        },
        FaultCase {
            name: "a row dropped from a middle page",
            faults: Faults {
                drop_row_on_page: Some(2),
                ..Faults::default()
            },
            names: "rule 4",
            accepted: 1,
            ..base.clone()
        },
        FaultCase {
            name: "a row dropped from the last page",
            faults: Faults {
                drop_row_on_page: Some(3),
                ..Faults::default()
            },
            names: "rule 4",
            accepted: 2,
            ..base.clone()
        },
        FaultCase {
            name: "a page served twice, with distinct timestamps",
            faults: Faults {
                repeat_page: Some(2),
                ..Faults::default()
            },
            names: "rule 6",
            accepted: 1,
            ..base.clone()
        },
        FaultCase {
            name: "a row older than the window",
            window: Window::since(BASE + 5),
            faults: Faults {
                stray_row_on_page: Some(3),
                ..Faults::default()
            },
            names: "rule 5",
            accepted: 2,
            ..base.clone()
        },
        FaultCase {
            name: "`to` treated as inclusive",
            window: Window::new(BASE + 100, BASE + 2_000).unwrap(),
            faults: Faults {
                inclusive_to: true,
                ..Faults::default()
            },
            names: "rule 5",
            total: 450,
            ..base.clone()
        },
        FaultCase {
            name: "rows in increasing order",
            faults: Faults {
                increasing_order: true,
                ..Faults::default()
            },
            names: "rule 6",
            ..base.clone()
        },
        FaultCase {
            name: "perPage not honoured",
            cap: 100,
            names: "rule 2",
            ..base.clone()
        },
        FaultCase {
            name: "perPage not honoured at a small page size",
            page_size: 50,
            cap: 49,
            names: "rule 2",
            ..base.clone()
        },
        FaultCase {
            name: "a missing date",
            faults: Faults {
                bad_date: Some((2, 3, BadDate::Missing)),
                ..Faults::default()
            },
            kind: ErrorKind::Decode,
            names: "recenttracks.track[3].date.uts",
            accepted: 1,
            ..base.clone()
        },
        FaultCase {
            name: "a non-numeric date",
            faults: Faults {
                bad_date: Some((1, 0, BadDate::NonNumeric)),
                ..Faults::default()
            },
            kind: ErrorKind::Decode,
            names: "recenttracks.track[0].date.uts",
            ..base.clone()
        },
        FaultCase {
            name: "a missing @attr",
            faults: Faults {
                missing_attr_on_page: Some(2),
                ..Faults::default()
            },
            kind: ErrorKind::Decode,
            names: "recenttracks.@attr",
            accepted: 1,
            ..base.clone()
        },
        FaultCase {
            name: "a missing total",
            faults: Faults {
                missing_total_on_page: Some(1),
                ..Faults::default()
            },
            kind: ErrorKind::Decode,
            names: "recenttracks.@attr.total",
            ..base.clone()
        },
        FaultCase {
            name: "an empty page in the middle",
            faults: Faults {
                empty_page: Some(2),
                ..Faults::default()
            },
            kind: ErrorKind::Decode,
            names: "recenttracks.track",
            accepted: 1,
            ..base.clone()
        },
    ]
}

#[test]
fn gate8_every_fault_switch_is_caught_and_poisons_the_scan() {
    for case in fault_cases() {
        assert!(!case.names.is_empty(), "{}: nothing is asserted", case.name);
        let dataset = Dataset::distinct(case.total)
            .with_cap(case.cap)
            .with_faults(case.faults.clone());
        let driven = drive(&dataset, scan_of(case.window, case.page_size, false));

        let error = driven.error();
        assert_eq!(error.kind(), case.kind, "{}: {error:?}", case.name);
        assert!(
            error.to_string().contains(case.names),
            "{}: `{error}` does not name {}",
            case.name,
            case.names
        );
        assert_eq!(
            error.method(),
            Some("user.getRecentTracks"),
            "{}",
            case.name
        );
        assert_eq!(driven.pages.len(), case.accepted, "{}", case.name);
        assert_eq!(
            driven.requests.len(),
            case.accepted + 1,
            "{}: a request followed the failure",
            case.name
        );
        for text in ["someone-else", "Synthetic", "Track "] {
            assert!(
                !error.to_string().contains(text) && !format!("{error:?}").contains(text),
                "{}: response text `{text}` in the error",
                case.name
            );
        }
        assert_poisoned(driven);
    }
}

/// Runs a scan against a faulty service and checks that it stops with the
/// rule at the given page.
#[track_caller]
fn assert_stops(dataset: &Dataset, win: Window, rule: &str, failing_page: usize) {
    let driven = drive(dataset, scan_of(win, 200, false));
    let error = driven.error();
    assert_eq!(error.kind(), ErrorKind::Inconsistent, "{error:?}");
    assert!(
        error.to_string().contains(rule),
        "`{error}` does not name {rule}"
    );
    assert_eq!(driven.requests.len(), failing_page, "requests issued");
    assert_poisoned(driven);
}

#[test]
fn gate8_a_scrobble_arriving_mid_scan_changes_total_and_is_caught() {
    // Without an upper bound the new scrobble is in the window and shifts
    // every later page by one row: a duplicate that a row count would hide.
    let dataset = Dataset::distinct(450).with_faults(Faults {
        arrive_from_page: Some(2),
        ..Faults::default()
    });
    assert_stops(&dataset, Window::ALL, "rule 3", 2);
}

#[test]
fn gate8_a_scrobble_deleted_mid_scan_changes_total_and_is_caught() {
    let dataset = Dataset::distinct(450).with_faults(Faults {
        delete_from_page: Some(2),
        ..Faults::default()
    });
    assert_stops(&dataset, Window::ALL, "rule 3", 2);
}

#[test]
fn gate8_a_fixed_upper_bound_keeps_a_new_scrobble_out_of_the_scan() {
    // The caller fixed `to` in the past, so the arrival is outside the
    // window and the scan completes with every original row.
    let dataset = Dataset::distinct(450).with_faults(Faults {
        arrive_from_page: Some(2),
        ..Faults::default()
    });
    let newest = dataset.entries[0].ts;
    let win = Window::before(newest + 1);
    let done = complete(&dataset, win, 200, false);
    assert_eq!(done.markers(), (0..450).collect::<Vec<_>>());
}

#[test]
fn gate8_a_service_reading_to_as_inclusive_is_caught_by_the_window_rule() {
    // The row at exactly `to` exists, and the faulty service returns it.
    let dataset = Dataset::distinct(450).with_faults(Faults {
        inclusive_to: true,
        ..Faults::default()
    });
    let win = window(BASE + 100, BASE + 2_000);
    assert!(dataset.entries.iter().any(|e| e.ts == BASE + 2_000));
    assert_stops(&dataset, win, "rule 5", 1);
}

#[test]
fn gate8_a_row_older_than_from_is_caught_by_the_window_rule() {
    let dataset = Dataset::distinct(450).with_faults(Faults {
        stray_row_on_page: Some(3),
        ..Faults::default()
    });
    assert_stops(&dataset, Window::since(BASE + 5), "rule 5", 3);
}

#[test]
fn gate8_a_failed_scan_cannot_be_finished_even_if_it_saw_every_row() {
    // The last page is the one that fails: every earlier row was good.
    let dataset = Dataset::distinct(450).with_faults(Faults {
        drop_row_on_page: Some(3),
        ..Faults::default()
    });
    let driven = drive(&dataset, scan_of(Window::ALL, 200, false));
    assert_eq!(driven.pages.len(), 2);
    assert!(driven.scan.clone().finish().is_err());
}

// ---------------------------------------------------------------------------
// What a completed scan does not prove. Each test below completes `Ok` today
// with wrong rows, and asserts exactly which. They pin behaviour the design
// documents as undetectable: if detection ever improves, a test here fails
// and the change is a decision, not an accident.

#[test]
fn residual_balanced_arrival_and_deletion_is_not_detectable() {
    // A scrobble arrives and the oldest one is deleted between page 1 and
    // page 2. `total` is unchanged, every row is in order, and the new row
    // pushes row 199 onto page 2 as well.
    let dataset = Dataset::distinct(450).with_faults(Faults {
        arrive_from_page: Some(2),
        delete_oldest_from_page: Some(2),
        ..Faults::default()
    });
    let done = complete(&dataset, Window::ALL, 200, false);
    let expected: Vec<usize> = (0..=199).chain(199..=398).chain(399..=448).collect();
    assert_eq!(done.markers(), expected);
    assert_eq!(done.summary.total(), 450);
}

#[test]
fn residual_a_fixed_upper_bound_does_not_stop_a_backdated_addition_and_a_deletion() {
    // `to` keeps arrivals at the head out. A listen added with an old
    // timestamp and a deletion inside the window still balance `total`.
    let dataset = Dataset::distinct(450).with_faults(Faults {
        backdate_from_page: Some(2),
        delete_oldest_from_page: Some(2),
        ..Faults::default()
    });
    let win = Window::before(dataset.entries[0].ts + 1);
    let done = complete(&dataset, win, 200, false);
    let expected: Vec<usize> = (0..=199).chain(199..=398).chain(399..=448).collect();
    assert_eq!(done.markers(), expected);
    assert_eq!(done.summary.total(), 450);
}

#[test]
fn residual_reordered_same_second_rows_are_not_detectable() {
    // Thirty rows share a second and straddle the page boundary at 200. From
    // page 2 the service lists them the other way round: rows 190 to 199
    // come twice and rows 210 to 219 never.
    let dataset = straddling_run().with_faults(Faults {
        reverse_ties_from_page: Some(2),
        ..Faults::default()
    });
    let done = complete(&dataset, Window::ALL, 200, false);
    let expected: Vec<usize> = (0..200).chain((190..=209).rev()).chain(220..320).collect();
    assert_eq!(done.markers(), expected);
    assert_eq!(done.summary.total(), 320);
}

#[test]
fn residual_a_page_repeated_with_tied_timestamps_is_not_detectable() {
    // Rule 6 catches a repeated page only because its timestamps differ from
    // the previous page's last (see the fault table). When every listen
    // shares a second, nothing differs: page 2 repeats page 1, rows 200 to
    // 399 never arrive, and the counts still add up.
    let second = BASE + 1;
    let dataset = Dataset::from_timestamps(vec![second; 401])
        .identical_rows()
        .with_faults(Faults {
            repeat_page: Some(2),
            ..Faults::default()
        });
    let done = complete(&dataset, window(second, second + 1), 200, false);
    let expected: Vec<usize> = (0..200).chain(0..200).chain(400..401).collect();
    assert_eq!(done.markers(), expected);
    assert_eq!(done.summary.total(), 401);
}

// ---------------------------------------------------------------------------
// Rules checked on hand-made pages.

/// A scan of `window` for `user` with a fixed page size.
fn scan_for(user: &str, window: Window, page_size: u32) -> WindowScan {
    RecentTracks::new(user)
        .window(window)
        .limit(page_size)
        .scan()
        .unwrap()
}

/// A response carrying `body` to the scan's outstanding request.
fn answer(scan: &WindowScan, body: &Value) -> Raw {
    let request = scan.next_request().unwrap();
    protocol::decode(&request, HttpResponse::new(200, body.to_string())).unwrap()
}

fn hand_row(uts: u64) -> Value {
    json!({
        "artist": {"mbid": "", "#text": "A"},
        "mbid": "", "name": "t", "url": "",
        "album": {"mbid": "", "#text": ""},
        "date": {"uts": uts.to_string(), "#text": "x"}
    })
}

fn hand_page(
    scan: &WindowScan,
    user: &str,
    page: u64,
    per_page: u64,
    total_pages: u64,
    total: u64,
    ts: &[u64],
) -> Raw {
    let rows: Vec<Value> = ts.iter().map(|&t| hand_row(t)).collect();
    answer(
        scan,
        &json!({"recenttracks": {
            "track": rows,
            "@attr": {
                "user": user, "page": page.to_string(), "perPage": per_page.to_string(),
                "totalPages": total_pages.to_string(), "total": total.to_string()
            }
        }}),
    )
}

#[track_caller]
fn assert_rule(result: Result<ScanPage, Error>, rule: &str) {
    let error = result.unwrap_err();
    assert_eq!(error.kind(), ErrorKind::Inconsistent, "{error:?}");
    assert!(
        error.to_string().contains(rule),
        "`{error}` does not name {rule}"
    );
}

#[test]
fn rule1_the_user_is_compared_ignoring_case_and_the_echo_is_reported() {
    let mut scan = scan_for("rj", Window::ALL, 2);
    scan.accept(hand_page(&scan, "RJ", 1, 2, 1, 1, &[5]))
        .unwrap();
    assert_eq!(scan.finish().unwrap().user(), "RJ");

    let mut scan = scan_for("rj", Window::ALL, 2);
    assert_rule(
        scan.accept(hand_page(&scan, "rjx", 1, 2, 1, 1, &[5])),
        "rule 1",
    );
}

#[test]
fn rule2_page_and_per_page_must_be_the_requested_ones() {
    let mut scan = scan_for("rj", Window::ALL, 2);
    assert_rule(
        scan.accept(hand_page(&scan, "rj", 2, 2, 2, 3, &[5, 4])),
        "rule 2",
    );
    let mut scan = scan_for("rj", Window::ALL, 2);
    assert_rule(
        scan.accept(hand_page(&scan, "rj", 1, 3, 1, 1, &[5])),
        "rule 2",
    );
    let mut scan = scan_for("rj", Window::ALL, 2);
    assert_rule(
        scan.accept(hand_page(&scan, "rj", 0, 2, 1, 1, &[5])),
        "rule 2",
    );
}

#[test]
fn rule3_total_pages_must_follow_from_total_and_per_page() {
    let page_size = 2;
    for (total, total_pages) in [
        (5_u64, 2_u64),
        (5, 4),
        (5, 0),
        (1, 0),
        (1, 2),
        (0, 2),
        (4, 3),
    ] {
        let mut scan = scan_for("rj", Window::ALL, page_size);
        let rows: Vec<u64> = (0..total.min(2)).map(|i| 100 - i).collect();
        assert_rule(
            scan.accept(hand_page(&scan, "rj", 1, 2, total_pages, total, &rows)),
            "rule 3",
        );
    }
}

#[test]
fn rule3_an_empty_window_may_report_zero_or_one_page() {
    for total_pages in [0, 1] {
        let mut scan = scan_for("rj", Window::ALL, 200);
        scan.accept(hand_page(&scan, "rj", 1, 200, total_pages, 0, &[]))
            .unwrap();
        assert!(scan.next_request().is_none());
        let summary = scan.finish().unwrap();
        assert_eq!((summary.total(), summary.pages()), (0, 1));
    }
}

#[test]
fn rule3_an_absurd_page_count_is_rejected_rather_than_followed() {
    let mut scan = scan_for("rj", Window::ALL, 200);
    let total = 200 * 5_000_000_000_u64;
    assert_rule(
        scan.accept(hand_page(&scan, "rj", 1, 200, 5_000_000_000, total, &[5])),
        "rule 3",
    );
}

#[test]
fn rule3_total_pages_changing_alone_is_caught() {
    let mut scan = scan_for("rj", Window::ALL, 1);
    scan.accept(hand_page(&scan, "rj", 1, 1, 3, 3, &[9]))
        .unwrap();
    // Self-consistent, but not the same as the first page.
    assert_rule(
        scan.accept(hand_page(&scan, "rj", 2, 1, 4, 4, &[8])),
        "rule 3",
    );
}

#[test]
fn rule4_the_count_must_match_the_totals() {
    let mut scan = scan_for("rj", Window::ALL, 2);
    assert_rule(
        scan.accept(hand_page(&scan, "rj", 1, 2, 2, 3, &[9])),
        "rule 4",
    );
    let mut scan = scan_for("rj", Window::ALL, 2);
    assert_rule(
        scan.accept(hand_page(&scan, "rj", 1, 2, 2, 3, &[9, 8, 7])),
        "rule 4",
    );
    let mut scan = scan_for("rj", Window::ALL, 2);
    assert_rule(
        scan.accept(hand_page(&scan, "rj", 1, 2, 0, 0, &[9])),
        "rule 4",
    );
}

#[test]
fn rule5_only_the_bounds_that_were_set_are_checked() {
    // from only: nothing above is wrong.
    let mut scan = scan_for("rj", Window::since(10), 2);
    scan.accept(hand_page(&scan, "rj", 1, 2, 1, 2, &[u64::MAX, 10]))
        .unwrap();
    let mut scan = scan_for("rj", Window::since(10), 2);
    assert_rule(
        scan.accept(hand_page(&scan, "rj", 1, 2, 1, 1, &[9])),
        "rule 5",
    );

    // to only: zero is fine, `to` itself is not.
    let mut scan = scan_for("rj", Window::before(10), 2);
    scan.accept(hand_page(&scan, "rj", 1, 2, 1, 2, &[9, 0]))
        .unwrap();
    let mut scan = scan_for("rj", Window::before(10), 2);
    assert_rule(
        scan.accept(hand_page(&scan, "rj", 1, 2, 1, 1, &[10])),
        "rule 5",
    );

    // both: from inclusive, to exclusive.
    let both = window(10, 20);
    let mut scan = scan_for("rj", both, 2);
    scan.accept(hand_page(&scan, "rj", 1, 2, 1, 2, &[19, 10]))
        .unwrap();
    for outside in [20, 9] {
        let mut scan = scan_for("rj", both, 2);
        assert_rule(
            scan.accept(hand_page(&scan, "rj", 1, 2, 1, 1, &[outside])),
            "rule 5",
        );
    }
}

#[test]
fn rule6_equal_timestamps_are_fine_and_increases_are_not() {
    let mut scan = scan_for("rj", Window::ALL, 2);
    scan.accept(hand_page(&scan, "rj", 1, 2, 2, 4, &[5, 5]))
        .unwrap();
    scan.accept(hand_page(&scan, "rj", 2, 2, 2, 4, &[5, 5]))
        .unwrap();
    assert_eq!(scan.finish().unwrap().total(), 4);

    let mut scan = scan_for("rj", Window::ALL, 2);
    assert_rule(
        scan.accept(hand_page(&scan, "rj", 1, 2, 1, 2, &[5, 6])),
        "rule 6",
    );

    // Across a page boundary: the first row of page 2 is newer than the last
    // row of page 1.
    let mut scan = scan_for("rj", Window::ALL, 2);
    scan.accept(hand_page(&scan, "rj", 1, 2, 2, 4, &[9, 5]))
        .unwrap();
    assert_rule(
        scan.accept(hand_page(&scan, "rj", 2, 2, 2, 4, &[6, 4])),
        "rule 6",
    );
}

// ---------------------------------------------------------------------------
// Gate 9: property test.

#[derive(Debug, Clone)]
struct Case {
    gaps: Vec<u64>,
    page_size: u32,
    from: Option<u64>,
    to: Option<u64>,
    now_playing: Option<(bool, u8)>,
    extended: bool,
    flip_case: bool,
    /// Rows that share a second are byte-identical.
    identical: bool,
}

fn cases() -> impl Strategy<Value = Case> {
    (
        prop::collection::vec(prop_oneof![Just(0_u64), Just(0), 1_u64..4], 0..=800),
        1_u32..=200,
        prop::option::of(0_u64..4_000),
        prop::option::of(0_u64..4_000),
        prop::option::of((any::<bool>(), 0_u8..3)),
        any::<bool>(),
        any::<bool>(),
        any::<bool>(),
    )
        .prop_map(
            |(gaps, page_size, from, to, now_playing, extended, flip_case, identical)| Case {
                gaps,
                page_size,
                from: from.map(|v| BASE + 95 + v),
                to: to.map(|v| BASE + 95 + v),
                now_playing,
                extended,
                flip_case,
                identical,
            },
        )
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(256))]

    #[test]
    fn gate9_a_scan_yields_exactly_the_in_window_markers_in_order(case in cases()) {
        // Gap 0 puts a row in the same second as the one before it.
        let mut at = BASE + 100;
        let mut timestamps = Vec::with_capacity(case.gaps.len());
        for gap in &case.gaps {
            at += gap;
            timestamps.push(at);
        }
        let mut dataset = Dataset::from_timestamps(timestamps);
        if case.identical {
            dataset = dataset.byte_identical();
        }
        if let Some((with_date, on)) = case.now_playing {
            let on = [Pages::First, Pages::Last, Pages::All][on as usize];
            dataset = dataset.with_now_playing(with_date, on);
        }
        let win = match (case.from, case.to) {
            (Some(from), Some(to)) if from < to => window(from, to),
            (Some(from), _) => Window::since(from),
            (None, Some(to)) => Window::before(to),
            (None, None) => Window::ALL,
        };

        let user = if case.flip_case { dataset.user.to_uppercase() } else { dataset.user.clone() };
        let scan = RecentTracks::new(user)
            .window(win)
            .limit(case.page_size)
            .extended(case.extended)
            .scan()
            .unwrap();
        let driven = drive(&dataset, scan);
        prop_assert!(driven.error.is_none(), "{:?}", driven.error);

        let expected = dataset.expected(win);
        prop_assert_eq!(driven.timestamps(), dataset.expected_timestamps(win));
        if !case.identical {
            prop_assert_eq!(driven.markers(), expected.clone());
        }
        let rows: Vec<_> = driven.pages.iter().flat_map(|p| p.scrobbles()).collect();
        for pair in rows.windows(2) {
            if case.identical && pair[0].timestamp() == pair[1].timestamp() {
                prop_assert_eq!(pair[0].json(), pair[1].json());
            }
        }

        let requests = driven.requests.clone();
        let summary = driven.scan.finish().unwrap();
        prop_assert_eq!(summary.total(), expected.len() as u64);
        prop_assert_eq!(summary.window(), win);
        prop_assert_eq!(summary.pages() as usize, pages_for(expected.len(), case.page_size as usize));
        prop_assert_eq!(requests.len(), summary.pages() as usize);

        let limit = case.page_size.to_string();
        for (index, request) in requests.iter().enumerate() {
            prop_assert_eq!(param(request, "limit"), Some(limit.clone()));
            prop_assert_eq!(param(request, "from"), win.from().map(|v| v.to_string()));
            prop_assert_eq!(param(request, "to"), win.to().map(|v| v.to_string()));
            prop_assert_eq!(param(request, "page"), Some((index + 1).to_string()));
        }
    }
}

// ---------------------------------------------------------------------------
// Gate 10: a retried request neither skips nor duplicates.

#[test]
fn gate10_next_request_is_idempotent_until_accept() {
    let dataset = Dataset::distinct(450);
    let mut scan = scan_of(Window::ALL, 200, false);
    let mut markers = Vec::new();
    let mut accepted = 0;
    while let Some(request) = scan.next_request() {
        let first = protocol::prepare(&credentials(), &request).unwrap();
        // A transport failure on the first attempt: ask again, and again.
        for _ in 0..3 {
            let again = scan.next_request().unwrap();
            let retried = protocol::prepare(&credentials(), &again).unwrap();
            assert_eq!(retried.url(), first.url());
        }
        let page = scan.accept(fetch(&dataset, &request).1.unwrap()).unwrap();
        assert_eq!(page.number(), accepted + 1);
        accepted += 1;
        markers.extend(page.scrobbles().iter().map(|s| marker_of(s.track().mbid())));
    }
    assert_eq!(markers, (0..450).collect::<Vec<_>>());
    assert_eq!(scan.finish().unwrap().pages(), 3);
}

#[test]
fn gate10_a_failed_attempt_can_be_retried_without_losing_the_scan() {
    // Page 2 fails once with a temporary API error, then succeeds.
    let healthy = Dataset::distinct(450);
    let flaky = healthy.clone().with_faults(Faults {
        api_error: Some((Some(2), 16)),
        ..Faults::default()
    });
    let mut scan = scan_of(Window::ALL, 200, false);
    let mut markers = Vec::new();
    let mut tried_page_two = false;
    while let Some(request) = scan.next_request() {
        let source = if tried_page_two { &healthy } else { &flaky };
        let (http, raw) = fetch(source, &request);
        match raw {
            Ok(raw) => {
                let page = scan.accept(raw).unwrap();
                markers.extend(page.scrobbles().iter().map(|s| marker_of(s.track().mbid())));
            }
            Err(error) => {
                assert_eq!(error.api_code(), Some(ApiErrorCode::TEMPORARY_ERROR));
                assert_eq!(param(&http, "page").as_deref(), Some("2"));
                tried_page_two = true;
            }
        }
    }
    assert!(tried_page_two);
    assert_eq!(markers, (0..450).collect::<Vec<_>>());
    assert_eq!(scan.finish().unwrap().total(), 450);
}

// ---------------------------------------------------------------------------
// Gate 11: thread safety, checked at compile time.

#[test]
fn gate11_scan_types_are_send_and_sync() {
    fn assert_send_sync<T: Send + Sync>() {}
    assert_send_sync::<WindowScan>();
    assert_send_sync::<ScanPage>();
    assert_send_sync::<ScanSummary>();
    assert_send_sync::<RecentTracksPage>();
    assert_send_sync::<RecentTracks>();
    assert_send_sync::<Window>();
    assert_send_sync::<Error>();
}

// ---------------------------------------------------------------------------
// The rest of the contract.

#[test]
fn the_exact_response_is_kept_with_each_page() {
    let dataset = Dataset::distinct(450).with_now_playing(true, Pages::First);
    let done = complete(&dataset, Window::ALL, 200, true);
    for (page, request) in done.pages.iter().zip(&done.requests) {
        assert_eq!(page.raw().body(), &respond(&dataset, request).1[..]);
        assert_eq!(page.raw().method(), "user.getRecentTracks");
        assert_eq!(page.attr().page(), u64::from(page.number()));
        assert_eq!(page.page().scrobbles().len(), page.scrobbles().len());
    }
}

#[test]
fn the_summary_reports_the_services_spelling_of_the_user() {
    let dataset = Dataset::distinct(3);
    let done = complete(&dataset, Window::ALL, 200, false);
    assert_eq!(done.summary.user(), "rj_Synthetic");
    assert_ne!(done.summary.user(), USER);
}

#[test]
fn accept_with_nothing_outstanding_is_an_error_and_leaves_a_finished_scan_alone() {
    let dataset = Dataset::distinct(5);
    let mut driven = drive(&dataset, scan_of(Window::ALL, 200, false));
    assert!(driven.error.is_none());
    assert!(driven.scan.next_request().is_none());

    let spare = fetch(
        &dataset,
        &RecentTracks::new(USER)
            .scan()
            .unwrap()
            .next_request()
            .unwrap(),
    )
    .1
    .unwrap();
    let error = driven.scan.accept(spare).unwrap_err();
    assert_eq!(error.kind(), ErrorKind::InvalidRequest);
    assert_eq!(driven.scan.finish().unwrap().total(), 5);
}

#[test]
fn finish_fails_until_every_page_was_accepted() {
    let dataset = Dataset::distinct(450);
    let mut scan = scan_of(Window::ALL, 200, false);
    assert_eq!(
        scan.clone().finish().unwrap_err().kind(),
        ErrorKind::Inconsistent
    );
    for _ in 0..2 {
        let request = scan.next_request().unwrap();
        scan.accept(fetch(&dataset, &request).1.unwrap()).unwrap();
        assert!(scan.clone().finish().is_err());
    }
    let request = scan.next_request().unwrap();
    scan.accept(fetch(&dataset, &request).1.unwrap()).unwrap();
    assert!(scan.finish().is_ok());
}

#[test]
fn a_response_that_does_not_decode_poisons_the_scan() {
    let mut scan = RecentTracks::new(USER).scan().unwrap();
    let broken = answer(&scan, &json!({"recenttracks": 5}));
    assert_eq!(scan.accept(broken).unwrap_err().kind(), ErrorKind::Decode);
    assert!(scan.next_request().is_none());
    assert!(scan.finish().is_err());
}

/// Feeds `scan` the reply to `other`, a request it did not issue, and checks
/// that it is refused as such and the scan is abandoned.
#[track_caller]
fn assert_misdirected(dataset: &Dataset, scan: &RecentTracks, other: &RecentTracks) {
    let mut scan = scan.clone().scan().unwrap();
    assert_ne!(scan.next_request().unwrap(), other.request().unwrap());
    let foreign = fetch(dataset, &other.request().unwrap()).1.unwrap();
    let error = scan.accept(foreign).unwrap_err();
    assert_eq!(error.kind(), ErrorKind::Inconsistent, "{error:?}");
    assert!(
        error.to_string().contains("answers a different request"),
        "{error}"
    );
    assert_eq!(error.method(), Some("user.getRecentTracks"));
    assert!(scan.next_request().is_none(), "the scan was not abandoned");
    assert!(scan.finish().is_err());
}

#[test]
fn a_response_for_another_window_cannot_complete_a_scan() {
    // The reviewer's case: rows A@90, B@80, C@60, D@50. The reply for
    // [70, 100) holds A and B with totals that agree with themselves, and
    // used to finish a scan of [10, 100) with two of its four rows.
    let dataset = Dataset::from_timestamps(vec![BASE + 90, BASE + 80, BASE + 60, BASE + 50]);
    let scan = RecentTracks::new(USER)
        .window(window(BASE + 10, BASE + 100))
        .limit(2);
    let other = scan.clone().window(window(BASE + 70, BASE + 100)).page(1);
    assert_misdirected(&dataset, &scan, &other);
}

#[test]
fn a_response_for_another_bound_cannot_stand_in_for_the_scans() {
    let dataset = Dataset::distinct(50);
    let scan = RecentTracks::new(USER)
        .window(window(BASE + 100, BASE + 400))
        .limit(10);
    for other_window in [
        window(BASE + 101, BASE + 400),
        window(BASE + 100, BASE + 399),
        Window::since(BASE + 100),
        Window::before(BASE + 400),
        Window::ALL,
    ] {
        assert_misdirected(&dataset, &scan, &scan.clone().window(other_window).page(1));
    }
}

#[test]
fn a_response_for_the_right_window_but_another_page_is_refused() {
    let dataset = Dataset::distinct(50);
    let scan = RecentTracks::new(USER).limit(10);
    assert_misdirected(&dataset, &scan, &scan.clone().page(2));
}

#[test]
fn a_response_for_another_user_is_refused_even_if_the_spelling_differs_only_in_case() {
    let dataset = Dataset::distinct(50);
    let scan = RecentTracks::new(USER).limit(10);
    let lower = USER.to_lowercase();
    assert_ne!(lower, USER);
    assert_misdirected(&dataset, &scan, &RecentTracks::new(lower).limit(10).page(1));
}

#[test]
fn a_response_for_another_limit_is_refused() {
    let dataset = Dataset::distinct(50);
    let scan = RecentTracks::new(USER).limit(10);
    assert_misdirected(&dataset, &scan, &scan.clone().limit(11).page(1));
}

#[test]
fn a_response_for_a_different_extended_flag_is_refused() {
    let dataset = Dataset::distinct(50);
    let plain = RecentTracks::new(USER).limit(10);
    assert_misdirected(&dataset, &plain, &plain.clone().extended(true).page(1));
    let extended = plain.clone().extended(true);
    assert_misdirected(&dataset, &extended, &plain.page(1));
}

#[test]
fn a_response_to_a_request_made_as_the_user_is_not_the_scans() {
    let dataset = Dataset::distinct(50);
    let mut scan = RecentTracks::new(USER).limit(10).scan().unwrap();
    let plain = scan.next_request().unwrap();
    let body = respond(
        &dataset,
        &protocol::prepare(&credentials(), &plain).unwrap(),
    )
    .1;
    let as_user = RecentTracks::new(USER)
        .limit(10)
        .page(1)
        .as_user()
        .request()
        .unwrap();
    let raw = protocol::decode(&as_user, HttpResponse::new(200, body)).unwrap();
    let error = scan.accept(raw).unwrap_err();
    assert_eq!(error.kind(), ErrorKind::Inconsistent);
    assert!(scan.next_request().is_none());
}

#[test]
fn a_response_for_another_method_is_refused_as_another_request() {
    let mut scan = RecentTracks::new(USER).scan().unwrap();
    let request = Request::new(&protocol::methods::USER_GET_INFO).param("user", USER);
    let raw = protocol::decode(&request, HttpResponse::new(200, r#"{"user":{}}"#)).unwrap();
    let error = scan.accept(raw).unwrap_err();
    assert_eq!(error.kind(), ErrorKind::Inconsistent);
    assert!(scan.next_request().is_none());
    assert!(scan.finish().is_err());
}

#[test]
fn a_refused_response_does_not_leak_into_the_error() {
    let dataset = Dataset::distinct(5);
    let mut scan = RecentTracks::new(USER).scan().unwrap();
    let other = RecentTracks::new(USER).limit(3).page(1).request().unwrap();
    let error = scan.accept(fetch(&dataset, &other).1.unwrap()).unwrap_err();
    for text in ["Synthetic", "Track ", USER, "rj_Synthetic"] {
        assert!(!error.to_string().contains(text), "`{text}` in {error}");
        assert!(
            !format!("{error:?}").contains(text),
            "`{text}` in {error:?}"
        );
    }
}

#[test]
fn all_requests_of_a_scan_are_equal_apart_from_the_page() {
    let dataset = Dataset::distinct(450);
    let builder = RecentTracks::new(USER)
        .window(window(BASE + 100, BASE + 4_000))
        .limit(37)
        .extended(true);
    let mut scan = builder.clone().scan().unwrap();
    let mut issued = 0_u32;
    while let Some(request) = scan.next_request() {
        issued += 1;
        assert_eq!(request, builder.clone().page(issued).request().unwrap());
        assert_eq!(request.get("page"), Some(&*issued.to_string()));
        for _ in 0..3 {
            assert_eq!(scan.next_request().unwrap(), request);
        }
        scan.accept(fetch(&dataset, &request).1.unwrap()).unwrap();
    }
    assert_eq!(issued, 3_900_u32.div_ceil(10).div_ceil(37));
    assert_eq!(scan.finish().unwrap().pages(), issued);
}

#[test]
fn a_scan_exposes_its_window_and_page_size_and_nothing_to_change_them() {
    let scan = RecentTracks::new(USER)
        .window(window(10, 20))
        .limit(7)
        .scan()
        .unwrap();
    assert_eq!(scan.window(), window(10, 20));
    assert_eq!(scan.page_size(), 7);
    let default = RecentTracks::new(USER).scan().unwrap();
    assert_eq!(default.window(), Window::ALL);
    assert_eq!(default.page_size(), 200);
}

#[test]
fn a_scan_is_started_from_a_builder_that_chose_no_page() {
    for page in [0, 1, 2] {
        let error = RecentTracks::new(USER).page(page).scan().unwrap_err();
        assert_eq!(error.kind(), ErrorKind::InvalidRequest, "page {page}");
        assert!(
            error.to_string().contains("chooses its own pages"),
            "{error}"
        );
    }
}

#[test]
fn a_scan_page_size_is_one_to_two_hundred() {
    for size in [1, 2, 199, 200] {
        let scan = RecentTracks::new(USER).limit(size).scan().unwrap();
        assert_eq!(scan.page_size(), size);
    }
    for size in [0, 201, 1000, u32::MAX] {
        let error = RecentTracks::new(USER).limit(size).scan().unwrap_err();
        assert_eq!(error.kind(), ErrorKind::InvalidRequest, "{size}");
    }
}

#[test]
fn a_scan_carries_the_builders_settings() {
    let scan = RecentTracks::new(USER)
        .as_user()
        .extended(true)
        .scan()
        .unwrap();
    let request = scan.next_request().unwrap();
    assert!(format!("{request:?}").contains("as_user: true"));
    assert_eq!(request.get("extended"), Some("1"));
    assert_eq!(request.get("limit"), Some("200"));
    assert_eq!(request.get("page"), Some("1"));
    assert_eq!(request.get("user"), Some(USER));
    let plain = RecentTracks::new(USER)
        .scan()
        .unwrap()
        .next_request()
        .unwrap();
    assert!(format!("{plain:?}").contains("as_user: false"));
    assert_eq!(plain.get("extended"), None);
}

// ---------------------------------------------------------------------------
// Window and RecentTracks.

#[test]
fn window_new_rejects_an_empty_or_reversed_range() {
    for (from, to) in [(5, 5), (6, 5), (0, 0), (u64::MAX, 0)] {
        let error = Window::new(from, to).unwrap_err();
        assert_eq!(error.kind(), ErrorKind::InvalidRequest, "{from}..{to}");
    }
    let win = Window::new(5, 6).unwrap();
    assert_eq!((win.from(), win.to()), (Some(5), Some(6)));
}

#[test]
fn window_is_half_open() {
    let win = window(10, 20);
    assert!(!win.contains(9));
    assert!(win.contains(10));
    assert!(win.contains(19));
    assert!(!win.contains(20));

    assert!(Window::since(10).contains(10) && Window::since(10).contains(u64::MAX));
    assert!(!Window::since(10).contains(9));
    assert!(Window::before(10).contains(0) && Window::before(10).contains(9));
    assert!(!Window::before(10).contains(10));
    assert!(Window::ALL.contains(0) && Window::ALL.contains(u64::MAX));
    assert_eq!((Window::ALL.from(), Window::ALL.to()), (None, None));
    assert_eq!(Window::since(7).to(), None);
    assert_eq!(Window::before(7).from(), None);
}

fn url_of(builder: &RecentTracks) -> String {
    protocol::prepare(&credentials(), &builder.request().unwrap())
        .unwrap()
        .url()
        .to_owned()
}

#[test]
fn recent_tracks_sends_what_was_set_and_nothing_else() {
    let prefix = "https://ws.audioscrobbler.com/2.0/?method=user.getRecentTracks&api_key=SENTINEL_API_KEY_0001";
    assert_eq!(
        url_of(&RecentTracks::new("rj")),
        format!("{prefix}&user=rj&format=json")
    );
    assert_eq!(
        url_of(
            &RecentTracks::new("rj")
                .window(window(10, 20))
                .limit(50)
                .page(2)
                .extended(true)
        ),
        format!("{prefix}&user=rj&limit=50&page=2&from=10&to=20&extended=1&format=json")
    );
    assert_eq!(
        url_of(&RecentTracks::new("rj").window(Window::since(10))),
        format!("{prefix}&user=rj&from=10&format=json")
    );
    assert_eq!(
        url_of(
            &RecentTracks::new("rj")
                .window(Window::before(20))
                .extended(false)
        ),
        format!("{prefix}&user=rj&to=20&format=json")
    );
}

#[test]
fn recent_tracks_rejects_a_limit_outside_the_documented_range() {
    for limit in [1, 50, 200] {
        assert!(
            RecentTracks::new("rj").limit(limit).request().is_ok(),
            "{limit}"
        );
    }
    for limit in [0, 201, 1000] {
        let error = RecentTracks::new("rj").limit(limit).request().unwrap_err();
        assert_eq!(error.kind(), ErrorKind::InvalidRequest, "{limit}");
    }
    assert_eq!(
        RecentTracks::new("rj")
            .page(0)
            .request()
            .unwrap_err()
            .kind(),
        ErrorKind::InvalidRequest
    );
    assert!(RecentTracks::new("rj").page(1).request().is_ok());
}

#[test]
fn recent_tracks_rejects_a_bound_that_looks_like_milliseconds() {
    const LIMIT: u64 = 100_000_000_000;
    // 2023-11-14 in milliseconds, on either side and in every constructor.
    let millis = 1_700_000_000_000;
    let too_large = [
        Window::since(millis),
        Window::before(millis),
        Window::new(1_700_000_000, millis).unwrap(),
        Window::new(millis, millis + 1).unwrap(),
        Window::since(LIMIT),
        Window::before(u64::MAX),
    ];
    for window in too_large {
        let query = RecentTracks::new("rj").window(window);
        let error = query.request().unwrap_err();
        assert_eq!(error.kind(), ErrorKind::InvalidRequest, "{window:?}");
        assert!(error.to_string().contains("milliseconds"), "{error}");
        assert_eq!(
            query.scan().unwrap_err().kind(),
            ErrorKind::InvalidRequest,
            "{window:?}"
        );
    }
    for window in [
        Window::ALL,
        Window::since(0),
        Window::before(LIMIT - 1),
        Window::new(1_700_000_000, 1_700_086_400).unwrap(),
    ] {
        let query = RecentTracks::new("rj").window(window);
        assert!(query.request().is_ok(), "{window:?}");
        assert!(query.scan().is_ok(), "{window:?}");
    }
}

#[test]
fn recent_tracks_as_user_reaches_the_request() {
    let request = RecentTracks::new("rj").as_user().request().unwrap();
    assert!(format!("{request:?}").contains("as_user: true"));
}

#[test]
fn recent_tracks_response_decodes_through_the_public_model() {
    // The count probe from the design: one row, the window's total.
    let dataset = Dataset::distinct(450);
    let probe = RecentTracks::new(USER)
        .window(window(BASE + 100, BASE + 1_000))
        .limit(1)
        .request()
        .unwrap();
    let raw = fetch(&dataset, &probe).1.unwrap();
    let page = RecentTracksPage::decode(&raw).unwrap();
    assert_eq!(page.attr().total(), 90);
    assert_eq!(page.attr().per_page(), 1);
    assert_eq!(page.scrobbles().len(), 1);
}
