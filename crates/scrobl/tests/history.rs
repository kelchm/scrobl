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
    WindowScan::new(USER, window)
        .page_size(page_size)
        .unwrap()
        .extended(extended)
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
    let first = WindowScan::new(USER, Window::ALL).next_request().unwrap();
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
    let driven = drive(&dataset, WindowScan::new(USER, Window::ALL).extended(true));
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
fn gate3_an_empty_page_mid_scan_fails_and_issues_nothing_more() {
    let dataset = Dataset::distinct(1001).with_faults(Faults {
        empty_page: Some(3),
        ..Faults::default()
    });
    let bounded = window(BASE + 10, BASE + 100_000);
    let driven = drive(&dataset, scan_of(bounded, 200, false));

    let error = driven.error();
    assert_eq!(error.kind(), ErrorKind::Inconsistent);
    assert!(error.to_string().contains("rule 4"), "{error}");
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
fn gate3_an_empty_first_page_with_a_nonzero_total_fails() {
    let dataset = Dataset::distinct(300).with_faults(Faults {
        empty_page: Some(1),
        ..Faults::default()
    });
    let driven = drive(
        &dataset,
        scan_of(Window::before(BASE + 100_000), 200, false),
    );
    assert_eq!(driven.error().kind(), ErrorKind::Inconsistent);
    assert_eq!(driven.requests.len(), 1);
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
    let raw = raw_of(&json!({"recenttracks": {"track": []}}));
    let mut scan = WindowScan::new(USER, Window::ALL);
    assert_eq!(scan.accept(raw).unwrap_err().kind(), ErrorKind::Decode);
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
    let scan = WindowScan::new("nobody", Window::ALL);
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
    rule: &'static str,
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
        rule: "",
        accepted: 0,
    };
    vec![
        FaultCase {
            name: "wrong user on the first page",
            faults: Faults {
                wrong_user_on_page: Some(1),
                ..Faults::default()
            },
            rule: "rule 1",
            ..base.clone()
        },
        FaultCase {
            name: "wrong user on a later page",
            faults: Faults {
                wrong_user_on_page: Some(2),
                ..Faults::default()
            },
            rule: "rule 1",
            accepted: 1,
            ..base.clone()
        },
        FaultCase {
            name: "a scrobble arrives mid-scan",
            faults: Faults {
                arrive_from_page: Some(2),
                ..Faults::default()
            },
            rule: "rule 3",
            accepted: 1,
            ..base.clone()
        },
        FaultCase {
            name: "a scrobble is deleted mid-scan",
            faults: Faults {
                delete_from_page: Some(3),
                ..Faults::default()
            },
            rule: "rule 3",
            accepted: 2,
            ..base.clone()
        },
        FaultCase {
            name: "a row dropped from a middle page",
            faults: Faults {
                drop_row_on_page: Some(2),
                ..Faults::default()
            },
            rule: "rule 4",
            accepted: 1,
            ..base.clone()
        },
        FaultCase {
            name: "a row dropped from the last page",
            faults: Faults {
                drop_row_on_page: Some(3),
                ..Faults::default()
            },
            rule: "rule 4",
            accepted: 2,
            ..base.clone()
        },
        FaultCase {
            name: "a page served twice",
            faults: Faults {
                repeat_page: Some(2),
                ..Faults::default()
            },
            rule: "rule 6",
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
            rule: "rule 5",
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
            rule: "rule 5",
            total: 450,
            ..base.clone()
        },
        FaultCase {
            name: "rows in increasing order",
            faults: Faults {
                increasing_order: true,
                ..Faults::default()
            },
            rule: "rule 6",
            ..base.clone()
        },
        FaultCase {
            name: "perPage not honoured",
            cap: 100,
            rule: "rule 2",
            ..base.clone()
        },
        FaultCase {
            name: "perPage not honoured at a small page size",
            page_size: 50,
            cap: 49,
            rule: "rule 2",
            ..base.clone()
        },
        FaultCase {
            name: "a missing date",
            faults: Faults {
                bad_date: Some((2, 3, BadDate::Missing)),
                ..Faults::default()
            },
            kind: ErrorKind::Decode,
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
            ..base.clone()
        },
        FaultCase {
            name: "a missing @attr",
            faults: Faults {
                missing_attr_on_page: Some(2),
                ..Faults::default()
            },
            kind: ErrorKind::Decode,
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
            ..base.clone()
        },
        FaultCase {
            name: "an empty page in the middle",
            faults: Faults {
                empty_page: Some(2),
                ..Faults::default()
            },
            rule: "rule 4",
            accepted: 1,
            ..base.clone()
        },
    ]
}

#[test]
fn gate8_every_fault_switch_is_caught_and_poisons_the_scan() {
    for case in fault_cases() {
        let dataset = Dataset::distinct(case.total)
            .with_cap(case.cap)
            .with_faults(case.faults.clone());
        let driven = drive(&dataset, scan_of(case.window, case.page_size, false));

        let error = driven.error();
        assert_eq!(error.kind(), case.kind, "{}: {error:?}", case.name);
        assert!(
            error.to_string().contains(case.rule),
            "{}: `{error}` does not name {}",
            case.name,
            case.rule
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
// Rules checked on hand-made pages.

fn raw_of(body: &Value) -> Raw {
    let request = Request::new(&protocol::methods::USER_GET_RECENT_TRACKS).param("user", "rj");
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
    user: &str,
    page: u64,
    per_page: u64,
    total_pages: u64,
    total: u64,
    ts: &[u64],
) -> Raw {
    let rows: Vec<Value> = ts.iter().map(|&t| hand_row(t)).collect();
    raw_of(&json!({"recenttracks": {
        "track": rows,
        "@attr": {
            "user": user, "page": page.to_string(), "perPage": per_page.to_string(),
            "totalPages": total_pages.to_string(), "total": total.to_string()
        }
    }}))
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
    let mut scan = WindowScan::new("rj", Window::ALL).page_size(2).unwrap();
    scan.accept(hand_page("RJ", 1, 2, 1, 1, &[5])).unwrap();
    assert_eq!(scan.finish().unwrap().user(), "RJ");

    let mut scan = WindowScan::new("rj", Window::ALL).page_size(2).unwrap();
    assert_rule(scan.accept(hand_page("rjx", 1, 2, 1, 1, &[5])), "rule 1");
}

#[test]
fn rule2_page_and_per_page_must_be_the_requested_ones() {
    let mut scan = WindowScan::new("rj", Window::ALL).page_size(2).unwrap();
    assert_rule(scan.accept(hand_page("rj", 2, 2, 2, 3, &[5, 4])), "rule 2");
    let mut scan = WindowScan::new("rj", Window::ALL).page_size(2).unwrap();
    assert_rule(scan.accept(hand_page("rj", 1, 3, 1, 1, &[5])), "rule 2");
    let mut scan = WindowScan::new("rj", Window::ALL).page_size(2).unwrap();
    assert_rule(scan.accept(hand_page("rj", 0, 2, 1, 1, &[5])), "rule 2");
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
        let mut scan = WindowScan::new("rj", Window::ALL)
            .page_size(page_size)
            .unwrap();
        let rows: Vec<u64> = (0..total.min(2)).map(|i| 100 - i).collect();
        assert_rule(
            scan.accept(hand_page("rj", 1, 2, total_pages, total, &rows)),
            "rule 3",
        );
    }
}

#[test]
fn rule3_an_empty_window_may_report_zero_or_one_page() {
    for total_pages in [0, 1] {
        let mut scan = WindowScan::new("rj", Window::ALL);
        scan.accept(hand_page("rj", 1, 200, total_pages, 0, &[]))
            .unwrap();
        assert!(scan.next_request().is_none());
        let summary = scan.finish().unwrap();
        assert_eq!((summary.total(), summary.pages()), (0, 1));
    }
}

#[test]
fn rule3_an_absurd_page_count_is_rejected_rather_than_followed() {
    let mut scan = WindowScan::new("rj", Window::ALL);
    let total = 200 * 5_000_000_000_u64;
    assert_rule(
        scan.accept(hand_page("rj", 1, 200, 5_000_000_000, total, &[])),
        "rule 3",
    );
}

#[test]
fn rule3_total_pages_changing_alone_is_caught() {
    let mut scan = WindowScan::new("rj", Window::ALL).page_size(1).unwrap();
    scan.accept(hand_page("rj", 1, 1, 3, 3, &[9])).unwrap();
    // Self-consistent, but not the same as the first page.
    assert_rule(scan.accept(hand_page("rj", 2, 1, 4, 4, &[8])), "rule 3");
}

#[test]
fn rule4_the_count_must_match_the_totals() {
    let mut scan = WindowScan::new("rj", Window::ALL).page_size(2).unwrap();
    assert_rule(scan.accept(hand_page("rj", 1, 2, 2, 3, &[9])), "rule 4");
    let mut scan = WindowScan::new("rj", Window::ALL).page_size(2).unwrap();
    assert_rule(
        scan.accept(hand_page("rj", 1, 2, 2, 3, &[9, 8, 7])),
        "rule 4",
    );
    let mut scan = WindowScan::new("rj", Window::ALL).page_size(2).unwrap();
    assert_rule(scan.accept(hand_page("rj", 1, 2, 0, 0, &[9])), "rule 4");
}

#[test]
fn rule5_only_the_bounds_that_were_set_are_checked() {
    // from only: nothing above is wrong.
    let mut scan = WindowScan::new("rj", Window::since(10))
        .page_size(2)
        .unwrap();
    scan.accept(hand_page("rj", 1, 2, 1, 2, &[u64::MAX, 10]))
        .unwrap();
    let mut scan = WindowScan::new("rj", Window::since(10))
        .page_size(2)
        .unwrap();
    assert_rule(scan.accept(hand_page("rj", 1, 2, 1, 1, &[9])), "rule 5");

    // to only: zero is fine, `to` itself is not.
    let mut scan = WindowScan::new("rj", Window::before(10))
        .page_size(2)
        .unwrap();
    scan.accept(hand_page("rj", 1, 2, 1, 2, &[9, 0])).unwrap();
    let mut scan = WindowScan::new("rj", Window::before(10))
        .page_size(2)
        .unwrap();
    assert_rule(scan.accept(hand_page("rj", 1, 2, 1, 1, &[10])), "rule 5");

    // both: from inclusive, to exclusive.
    let both = window(10, 20);
    let mut scan = WindowScan::new("rj", both).page_size(2).unwrap();
    scan.accept(hand_page("rj", 1, 2, 1, 2, &[19, 10])).unwrap();
    for outside in [20, 9] {
        let mut scan = WindowScan::new("rj", both).page_size(2).unwrap();
        assert_rule(
            scan.accept(hand_page("rj", 1, 2, 1, 1, &[outside])),
            "rule 5",
        );
    }
}

#[test]
fn rule6_equal_timestamps_are_fine_and_increases_are_not() {
    let mut scan = WindowScan::new("rj", Window::ALL).page_size(2).unwrap();
    scan.accept(hand_page("rj", 1, 2, 2, 4, &[5, 5])).unwrap();
    scan.accept(hand_page("rj", 2, 2, 2, 4, &[5, 5])).unwrap();
    assert_eq!(scan.finish().unwrap().total(), 4);

    let mut scan = WindowScan::new("rj", Window::ALL).page_size(2).unwrap();
    assert_rule(scan.accept(hand_page("rj", 1, 2, 1, 2, &[5, 6])), "rule 6");

    // Across a page boundary: the first row of page 2 is newer than the last
    // row of page 1.
    let mut scan = WindowScan::new("rj", Window::ALL).page_size(2).unwrap();
    scan.accept(hand_page("rj", 1, 2, 2, 4, &[9, 5])).unwrap();
    assert_rule(scan.accept(hand_page("rj", 2, 2, 2, 4, &[6, 4])), "rule 6");
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
}

fn cases() -> impl Strategy<Value = Case> {
    (
        prop::collection::vec(prop_oneof![Just(0_u64), Just(0), 1_u64..4], 0..=1200),
        1_u32..=200,
        prop::option::of(0_u64..4_000),
        prop::option::of(0_u64..4_000),
        prop::option::of((any::<bool>(), 0_u8..3)),
        any::<bool>(),
        any::<bool>(),
    )
        .prop_map(
            |(gaps, page_size, from, to, now_playing, extended, flip_case)| Case {
                gaps,
                page_size,
                from: from.map(|v| BASE + 95 + v),
                to: to.map(|v| BASE + 95 + v),
                now_playing,
                extended,
                flip_case,
            },
        )
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(64))]

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
        let scan = WindowScan::new(user, win)
            .page_size(case.page_size)
            .unwrap()
            .extended(case.extended);
        let driven = drive(&dataset, scan);
        prop_assert!(driven.error.is_none(), "{:?}", driven.error);

        let expected = dataset.expected(win);
        prop_assert_eq!(driven.markers(), expected.clone());

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
        &WindowScan::new(USER, Window::ALL).next_request().unwrap(),
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
    let mut scan = WindowScan::new(USER, Window::ALL);
    let wrong_method = {
        let request = Request::new(&protocol::methods::USER_GET_INFO).param("user", "rj");
        protocol::decode(&request, HttpResponse::new(200, r#"{"user":{}}"#)).unwrap()
    };
    assert_eq!(
        scan.accept(wrong_method).unwrap_err().kind(),
        ErrorKind::Decode
    );
    assert!(scan.next_request().is_none());
    assert!(scan.finish().is_err());
}

#[test]
fn settings_cannot_change_once_a_page_was_accepted() {
    let dataset = Dataset::distinct(450);
    let mut scan = scan_of(Window::ALL, 200, false);
    let request = scan.next_request().unwrap();
    scan.accept(fetch(&dataset, &request).1.unwrap()).unwrap();

    let scan = scan.extended(true).as_user();
    let next = protocol::prepare(&credentials(), &scan.next_request().unwrap()).unwrap();
    assert_eq!(param(&next, "extended"), None);
    assert_eq!(param(&next, "limit").as_deref(), Some("200"));
    assert_eq!(
        scan.page_size(100).unwrap_err().kind(),
        ErrorKind::InvalidRequest
    );
}

#[test]
fn page_size_is_one_to_two_hundred() {
    for size in [1, 2, 199, 200] {
        assert!(
            WindowScan::new(USER, Window::ALL).page_size(size).is_ok(),
            "{size}"
        );
    }
    for size in [0, 201, 1000, u32::MAX] {
        let error = WindowScan::new(USER, Window::ALL)
            .page_size(size)
            .unwrap_err();
        assert_eq!(error.kind(), ErrorKind::InvalidRequest, "{size}");
    }
}

#[test]
fn as_user_is_passed_to_the_request() {
    let scan = WindowScan::new(USER, Window::ALL).as_user();
    let request = scan.next_request().unwrap();
    assert!(format!("{request:?}").contains("as_user: true"));
    let plain = WindowScan::new(USER, Window::ALL).next_request().unwrap();
    assert!(format!("{plain:?}").contains("as_user: false"));
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
