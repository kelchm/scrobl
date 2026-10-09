//! Gates for the async client, over a real socket.
//!
//! Every test talks to the fake Last.fm in `support::server`, which listens
//! on `127.0.0.1` and serves the synthetic dataset of `support`. Each gate
//! exists because an audited Last.fm crate gets the case wrong in a way that
//! looks right: configuration that never reaches the wire, a retry that
//! resends a write, a rate limiter that every sleeping caller resets, a
//! redirect that carries the API key somewhere else, an error that prints
//! the URL.
//!
//! The suite stays fast by using millisecond intervals and delays. Waits are
//! asserted as lower bounds only; the one upper bound is a generous hang
//! guard around each test.
//!
//! provenance: synthetic. The fake service generates every response; the
//! credentials are obvious sentinels.

#![cfg(feature = "client")]
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]

mod support;

use std::error::Error as _;
use std::future::Future;
use std::time::Duration;

use proptest::prelude::*;
use scrobl::client::{RecentTracksQuery, Scan, User};
use scrobl::history::{ScanPage, ScanSummary, Window};
use scrobl::model::RecentTracksPage;
use scrobl::protocol::{Request, Requirement, methods};
use scrobl::{
    ApiErrorCode, ApiKey, ApiSecret, Client, ClientBuilder, Delivery, Error, ErrorKind, Response,
    Retry, SessionKey, Writer,
};
use support::server::{Behaviour, FakeLastfm, Recorded, dead_base_url};
use support::*;
use tokio::task::JoinSet;

const KEY: &str = "SENTINEL_API_KEY_0001";
const SECRET: &str = "SENTINEL_API_SECRET_0002";
const SESSION: &str = "SENTINEL_SESSION_KEY_0003";

/// The user the synthetic service knows. Lookups ignore case.
const USER: &str = "rj_Synthetic";

/// A test that takes longer than this is hung, not slow.
const HANG_GUARD: Duration = Duration::from_secs(60);

/// How much earlier than its due time a request may be seen to arrive. The
/// server stamps a connection when it accepts it, which is a little after or
/// before the client's own clock for the same moment.
const SLACK: Duration = Duration::from_millis(15);

async fn bounded<F: Future>(future: F) -> F::Output {
    tokio::time::timeout(HANG_GUARD, future)
        .await
        .expect("the test hung")
}

/// A builder aimed at `base_url`, with pacing off and short delays.
fn builder_for(base_url: &str) -> ClientBuilder {
    Client::builder(ApiKey::new(KEY))
        .secret(ApiSecret::new(SECRET))
        .session(SessionKey::new(SESSION))
        .base_url(base_url)
        .min_interval(Duration::ZERO)
        .retry_delay(Duration::from_millis(1))
        .connect_timeout(Duration::from_secs(5))
        .timeout(Duration::from_secs(10))
}

fn builder(server: &FakeLastfm) -> ClientBuilder {
    builder_for(&server.base_url())
}

fn client(server: &FakeLastfm) -> Client {
    builder(server).build().unwrap()
}

fn writer(server: &FakeLastfm) -> Writer {
    builder(server).build_writer().unwrap()
}

async fn serve(dataset: Dataset, script: impl IntoIterator<Item = Behaviour>) -> FakeLastfm {
    FakeLastfm::start(dataset).await.script(script)
}

/// A plain read.
fn read() -> Request {
    Request::new(&methods::USER_GET_RECENT_TRACKS).param("user", USER)
}

/// A read made as the session's user, so the URL carries `sk`.
fn read_as_user() -> Request {
    read().as_user()
}

/// A write.
fn love() -> Request {
    Request::new(&methods::TRACK_LOVE)
        .param("track", "Synthetic Track")
        .param("artist", "Synthetic Artist")
}

fn dataset() -> Dataset {
    Dataset::distinct(3)
}

fn gaps(requests: &[Recorded]) -> Vec<Duration> {
    let mut arrivals: Vec<_> = requests.iter().map(|r| r.arrived).collect();
    arrivals.sort();
    arrivals.windows(2).map(|w| w[1] - w[0]).collect()
}

#[track_caller]
fn assert_waited(gap: Duration, wanted: Duration, what: &str) {
    assert!(
        gap + SLACK >= wanted,
        "{what}: requests {gap:?} apart, wanted at least {wanted:?}"
    );
}

/// Every text an error can show: its `Display` and `Debug`, and those of
/// every error down its `source` chain.
fn texts(error: &Error) -> Vec<String> {
    let mut out = vec![
        format!("{error}"),
        format!("{error:?}"),
        format!("{error:#?}"),
    ];
    let mut source = error.source();
    while let Some(inner) = source {
        out.extend([
            format!("{inner}"),
            format!("{inner:?}"),
            format!("{inner:#?}"),
        ]);
        source = inner.source();
    }
    out
}

#[track_caller]
fn assert_clean(label: &str, error: &Error) {
    for text in texts(error) {
        for secret in [KEY, SECRET, SESSION, "api_key", "api_sig", "127.0.0.1"] {
            assert!(
                !text.contains(secret),
                "{label}: `{secret}` appears in: {text}"
            );
        }
    }
    assert!(error.method().is_some(), "{label}: no method on {error:?}");
}

fn markers(pages: &[ScanPage]) -> Vec<usize> {
    pages
        .iter()
        .flat_map(|page| page.scrobbles())
        .map(|s| marker_of(s.track().mbid()))
        .collect()
}

async fn read_scan(mut scan: Scan) -> Result<(Vec<ScanPage>, ScanSummary), Error> {
    let mut pages = Vec::new();
    while let Some(page) = scan.next_page().await? {
        pages.push(page);
    }
    Ok((pages, scan.finish()?))
}

/// A window that holds all of `Dataset::distinct(n)`.
fn whole(n: usize) -> Window {
    Window::new(BASE, BASE + 10 * (n as u64 + 1)).unwrap()
}

// ---------------------------------------------------------------------------
// Gate 1: configuration reaches the wire.

#[tokio::test]
async fn gate1_the_configured_user_agent_is_what_the_server_sees() {
    bounded(async {
        let server = serve(dataset(), []).await;
        let client = builder(&server)
            .user_agent("scrobl-tests/9.9 (+https://example.invalid)")
            .build()
            .unwrap();
        client.call(&read()).await.unwrap();
        assert_eq!(
            server.requests()[0].header("user-agent"),
            Some("scrobl-tests/9.9 (+https://example.invalid)")
        );
    })
    .await;
}

#[tokio::test]
async fn gate1_the_default_user_agent_names_the_crate_and_its_version() {
    bounded(async {
        let server = serve(dataset(), []).await;
        client(&server).call(&read()).await.unwrap();
        assert_eq!(
            server.requests()[0].header("user-agent"),
            Some(format!("scrobl/{}", env!("CARGO_PKG_VERSION")).as_str())
        );
    })
    .await;
}

#[tokio::test]
async fn gate1_a_get_carries_everything_in_the_query_and_nothing_in_a_body() {
    bounded(async {
        let server = serve(dataset(), []).await;
        client(&server)
            .call(&read().param("limit", 7))
            .await
            .unwrap();

        let request = &server.requests()[0];
        assert_eq!(request.verb, "GET");
        assert_eq!(request.path, "/2.0/");
        assert!(request.body.is_empty(), "a GET sent a body");
        assert_eq!(request.header("content-length"), None);
        assert_eq!(request.header("content-type"), None);
        assert_eq!(request.query_param("method"), Some("user.getRecentTracks"));
        assert_eq!(request.query_param("api_key"), Some(KEY));
        assert_eq!(request.query_param("user"), Some(USER));
        assert_eq!(request.query_param("limit"), Some("7"));
        assert_eq!(request.query_param("format"), Some("json"));
        assert_eq!(request.query_param("api_sig"), None);
    })
    .await;
}

#[tokio::test]
async fn gate1_a_post_carries_everything_in_a_form_body_and_nothing_in_the_query() {
    bounded(async {
        let server = serve(dataset(), []).await;
        writer(&server).call(&love()).await.unwrap();

        let request = &server.requests()[0];
        assert_eq!(request.verb, "POST");
        assert_eq!(request.path, "/2.0/");
        assert!(request.query.is_empty(), "a POST sent a query");
        assert_eq!(
            request.header("content-type"),
            Some("application/x-www-form-urlencoded")
        );
        assert_eq!(
            request.header("content-length"),
            Some(request.body.len().to_string().as_str())
        );
        assert_eq!(request.form_param("method").as_deref(), Some("track.love"));
        assert_eq!(request.form_param("api_key").as_deref(), Some(KEY));
        assert_eq!(request.form_param("sk").as_deref(), Some(SESSION));
        assert_eq!(
            request.form_param("track").as_deref(),
            Some("Synthetic Track")
        );
        assert_eq!(
            request.form_param("artist").as_deref(),
            Some("Synthetic Artist")
        );
        assert_eq!(request.form_param("format").as_deref(), Some("json"));
        let signature = request.form_param("api_sig").unwrap();
        assert!(signature.len() == 32 && signature.bytes().all(|b| b.is_ascii_hexdigit()));

        // The secret signs. It is never sent.
        let wire = format!("{:?}{:?}", request.headers, request.body);
        assert!(!wire.contains(SECRET));
    })
    .await;
}

#[tokio::test]
async fn gate1_no_cookie_is_sent_even_when_the_server_sets_one() {
    bounded(async {
        let server = serve(
            dataset(),
            [Behaviour::status_with(
                200,
                "{}",
                &[("Set-Cookie", "session=abc; Path=/")],
            )],
        )
        .await;
        let writer = writer(&server);
        writer.call(&read()).await.unwrap();
        writer.call(&read()).await.unwrap();
        writer.call(&love()).await.unwrap();

        let requests = server.requests();
        assert_eq!(requests.len(), 3);
        for request in requests {
            assert_eq!(request.header("cookie"), None);
        }
    })
    .await;
}

#[tokio::test]
async fn gate1_the_client_never_asks_for_an_encoded_response() {
    // If another crate in the application turns on a `reqwest` compression
    // feature, `reqwest` would send `Accept-Encoding` and decode the answer,
    // and `Raw::body` would no longer be what the service sent. The client
    // turns every decoder off, so no header goes and nothing is decoded.
    bounded(async {
        let server = serve(
            dataset(),
            [Behaviour::status_with(
                200,
                "{}",
                &[("Content-Encoding", "gzip")],
            )],
        )
        .await;
        let writer = writer(&server);
        // A body that claims to be gzip and is not: it comes back untouched.
        let raw = writer.call(&read()).await.unwrap();
        assert!(raw.body() == b"{}", "the body was decoded");
        assert_eq!(server.request_count(), 1, "the first answer was refused");
        writer.call(&read()).await.unwrap();
        writer.call(&love()).await.unwrap();

        let requests = server.requests();
        assert_eq!(requests.len(), 3);
        for request in requests {
            assert_eq!(request.header("accept-encoding"), None);
        }
    })
    .await;
}

#[tokio::test]
async fn gate1_a_session_client_signs_for_its_own_session_and_shares_the_pacing_clock() {
    bounded(async {
        let interval = Duration::from_millis(80);
        let server = serve(dataset(), []).await;
        let client = builder(&server).min_interval(interval).build().unwrap();
        let other = client.with_session(SessionKey::new("SENTINEL_SESSION_KEY_OTHER"));

        client.call(&read_as_user()).await.unwrap();
        other.call(&read_as_user()).await.unwrap();
        client.call(&read_as_user()).await.unwrap();

        let requests = server.requests();
        assert_eq!(requests[0].query_param("sk"), Some(SESSION));
        assert_eq!(
            requests[1].query_param("sk"),
            Some("SENTINEL_SESSION_KEY_OTHER")
        );
        assert_eq!(requests[2].query_param("sk"), Some(SESSION));
        for gap in gaps(&requests) {
            assert_waited(gap, interval, "calls through different sessions");
        }
    })
    .await;
}

#[tokio::test]
async fn gate1_a_request_that_cannot_be_built_sends_nothing() {
    bounded(async {
        let server = serve(dataset(), []).await;
        let writer = Client::builder(ApiKey::new(KEY))
            .base_url(server.base_url())
            .build_writer()
            .unwrap();

        // A missing required parameter, a missing session and a missing
        // secret.
        let read = writer
            .call(&Request::new(&methods::USER_GET_RECENT_TRACKS))
            .await
            .unwrap_err();
        assert_eq!(read.kind(), ErrorKind::InvalidRequest);
        assert_eq!(read.delivery(), None);

        let write = writer.call(&love()).await.unwrap_err();
        assert_eq!(write.kind(), ErrorKind::InvalidRequest);
        assert_eq!(write.delivery(), Some(Delivery::NotSent));

        let bad = writer
            .user(USER)
            .recent_tracks()
            .limit(201)
            .send()
            .await
            .unwrap_err();
        assert_eq!(bad.kind(), ErrorKind::InvalidRequest);

        assert_eq!(server.connections(), 0);
    })
    .await;
}

// ---------------------------------------------------------------------------
// Gate 2: timeouts are enforced.

#[tokio::test]
async fn gate2_a_server_that_never_answers_times_out() {
    bounded(async {
        let timeout = Duration::from_millis(150);
        let server = serve(dataset(), [Behaviour::StallBeforeResponse]).await;
        let client = builder(&server)
            .timeout(timeout)
            .read_attempts(1)
            .build()
            .unwrap();

        let started = tokio::time::Instant::now();
        let error = client.call(&read()).await.unwrap_err();
        let took = started.elapsed();

        assert_eq!(error.kind(), ErrorKind::Timeout);
        assert!(took >= timeout, "gave up after {took:?}");
        assert!(took < Duration::from_secs(20), "took {took:?}");
        assert_eq!(error.retry(), Retry::Later);
        assert_clean("stalled", &error);
    })
    .await;
}

#[tokio::test]
async fn gate2_a_body_that_stalls_midway_times_out_because_the_budget_covers_the_body() {
    bounded(async {
        let timeout = Duration::from_millis(150);
        let server = serve(dataset(), [Behaviour::StallMidBody]).await;
        let client = builder(&server)
            .timeout(timeout)
            .read_attempts(1)
            .build()
            .unwrap();

        let started = tokio::time::Instant::now();
        let error = client.call(&read()).await.unwrap_err();
        let took = started.elapsed();

        assert_eq!(error.kind(), ErrorKind::Timeout);
        assert!(took >= timeout, "gave up after {took:?}");
        assert!(took < Duration::from_secs(20), "took {took:?}");
        assert_clean("stalled body", &error);
    })
    .await;
}

/// Starts `request`, lets the server see it, stops polling the call while
/// the answer arrives in full and the timeout passes, then polls it again.
async fn call_observed_late(request: Request) -> (Result<scrobl::protocol::Raw, Error>, Duration) {
    let timeout = Duration::from_millis(100);
    let server = serve(
        dataset(),
        [Behaviour::Delayed {
            by: Duration::from_millis(150),
            body: b"{}".to_vec(),
        }],
    )
    .await;
    let writer = builder(&server)
        .timeout(timeout)
        .read_attempts(1)
        .build_writer()
        .unwrap();

    let started = tokio::time::Instant::now();
    let mut call = Box::pin(writer.call(&request));
    tokio::select! {
        _ = &mut call => panic!("the call finished before the server answered"),
        () = server.wait_for_requests(1) => {}
    }
    // Nothing polls the call now. The answer arrives at 150 ms, in full,
    // after the 100 ms timeout, and the call is polled again at 250 ms.
    tokio::time::sleep(Duration::from_millis(250)).await;
    let result = call.await;
    (result, started.elapsed())
}

#[tokio::test]
async fn gate2_decision_a_read_whose_answer_arrived_in_full_is_returned_though_polled_late() {
    // The timeout is the longest the client waits for an attempt. It does not
    // discard an answer that has already arrived and decoded when the future
    // is next polled.
    bounded(async {
        let (result, took) = call_observed_late(read()).await;
        assert!(took >= Duration::from_millis(250), "{took:?}");
        let raw = result.expect("a complete answer was thrown away");
        assert_eq!(raw.body(), b"{}");
    })
    .await;
}

#[tokio::test]
async fn gate2_decision_a_write_whose_answer_arrived_in_full_is_not_turned_into_unknown() {
    // Throwing the answer away would turn a known outcome into
    // `Delivery::Unknown`, which is strictly worse.
    bounded(async {
        let (result, took) = call_observed_late(love()).await;
        assert!(took >= Duration::from_millis(250), "{took:?}");
        let raw = result.expect("a complete answer was thrown away");
        assert_eq!(raw.body(), b"{}");
    })
    .await;
}

/// A listener that accepts TCP connections and then says nothing, so a TLS
/// handshake with it never completes. Returns its `https` root and keeps the
/// accepted sockets open until dropped.
async fn mute_tls_root() -> (String, tokio::task::JoinHandle<()>) {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let root = format!("https://{}/2.0/", listener.local_addr().unwrap());
    let holder = tokio::spawn(async move {
        let mut held = Vec::new();
        while let Ok((socket, _)) = listener.accept().await {
            held.push(socket);
        }
    });
    (root, holder)
}

#[tokio::test]
async fn gate2_a_handshake_that_never_completes_times_out_at_the_connect_timeout() {
    // Observed: the connect timeout covers the TLS handshake. The error is
    // `Timeout`, as for any deadline, and for a write `Delivery::Unknown`,
    // as the design says of every timeout: the error does not say where the
    // deadline fell, and here nothing was in fact sent.
    bounded(async {
        let (root, holder) = mute_tls_root().await;
        let connect_timeout = Duration::from_millis(300);
        let total = Duration::from_secs(20);
        let writer = builder_for(&root)
            .connect_timeout(connect_timeout)
            .timeout(total)
            .read_attempts(1)
            .build_writer()
            .unwrap();

        for (label, request, delivery) in [
            ("read", read(), None),
            ("write", love(), Some(Delivery::Unknown)),
        ] {
            let started = tokio::time::Instant::now();
            let error = writer.call(&request).await.unwrap_err();
            let took = started.elapsed();

            assert_eq!(error.kind(), ErrorKind::Timeout, "{label}");
            assert_eq!(error.delivery(), delivery, "{label}");
            assert!(took >= connect_timeout, "{label}: gave up after {took:?}");
            // Far below the total timeout, which is what would have fired
            // had the connect timeout not applied to the handshake.
            assert!(took < total / 2, "{label}: took {took:?}");
            assert_clean(label, &error);
        }
        holder.abort();
    })
    .await;
}

#[tokio::test]
async fn gate2_a_dead_port_is_a_transport_failure_that_certainly_sent_nothing() {
    bounded(async {
        let writer = builder_for(&dead_base_url()).build_writer().unwrap();

        let write = writer.call(&love()).await.unwrap_err();
        assert_eq!(write.kind(), ErrorKind::Transport);
        assert_eq!(write.delivery(), Some(Delivery::NotSent));
        assert_eq!(write.http_status(), None);
        assert_clean("dead port, write", &write);

        let read = writer.call(&read()).await.unwrap_err();
        assert_eq!(read.kind(), ErrorKind::Transport);
        assert_eq!(read.delivery(), None);
        assert_eq!(read.retry(), Retry::Later);
        assert_clean("dead port, read", &read);
    })
    .await;
}

#[tokio::test]
async fn gate2_the_timeout_is_per_attempt_not_per_call() {
    bounded(async {
        let timeout = Duration::from_millis(150);
        let server = serve(
            dataset(),
            [
                Behaviour::StallBeforeResponse,
                Behaviour::StallBeforeResponse,
            ],
        )
        .await;
        let client = builder(&server).timeout(timeout).build().unwrap();

        let started = tokio::time::Instant::now();
        client.call(&read()).await.unwrap();
        assert!(started.elapsed() >= 2 * timeout);
        assert_eq!(server.request_count(), 3);
    })
    .await;
}

// ---------------------------------------------------------------------------
// Gate 3: the body cap is enforced, and the client stops reading.

const CAP: usize = 64 * 1024;
const FLOOD: usize = 128 * 1024 * 1024;

#[tokio::test]
async fn gate3_an_honest_content_length_over_the_cap_is_refused_before_the_body() {
    bounded(async {
        let server = serve(
            dataset(),
            [Behaviour::OversizeBody {
                total: FLOOD,
                chunked: false,
            }],
        )
        .await;
        let client = builder(&server).max_response_bytes(CAP).build().unwrap();

        let error = client.call(&read()).await.unwrap_err();
        assert_eq!(error.kind(), ErrorKind::BodyTooLarge);
        assert_eq!(error.retry(), Retry::No);
        assert_clean("announced oversize", &error);

        // Not retried, and the server was not drained.
        assert_eq!(server.request_count(), 1);
        // The flood ends when the client hangs up, so what was written by
        // then is all there will be.
        server.wait_for_closed(1).await;
        assert!(
            server.bytes_written() < (FLOOD / 4) as u64,
            "the client let the server write {} bytes",
            server.bytes_written()
        );
    })
    .await;
}

#[tokio::test]
async fn gate3_a_chunked_body_over_the_cap_is_cut_off_as_it_passes_the_cap() {
    bounded(async {
        let server = serve(
            dataset(),
            [Behaviour::OversizeBody {
                total: FLOOD,
                chunked: true,
            }],
        )
        .await;
        let client = builder(&server).max_response_bytes(CAP).build().unwrap();

        let error = client.call(&read()).await.unwrap_err();
        assert_eq!(error.kind(), ErrorKind::BodyTooLarge);
        assert_clean("chunked oversize", &error);

        assert_eq!(server.request_count(), 1);
        // The flood ends when the client hangs up, so what was written by
        // then is all there will be.
        server.wait_for_closed(1).await;
        assert!(
            server.bytes_written() < (FLOOD / 4) as u64,
            "the client let the server write {} bytes",
            server.bytes_written()
        );
    })
    .await;
}

/// A JSON body of exactly `len` bytes.
fn json_of_len(len: usize) -> Vec<u8> {
    let mut body = br#"{"pad":""#.to_vec();
    body.resize(len - 2, b'x');
    body.extend_from_slice(br#""}"#);
    assert_eq!(body.len(), len);
    body
}

#[tokio::test]
async fn gate3_a_body_of_exactly_the_cap_is_accepted_and_one_more_byte_is_not() {
    bounded(async {
        let server = serve(
            dataset(),
            [
                Behaviour::status(200, json_of_len(CAP)),
                Behaviour::status(200, json_of_len(CAP + 1)),
            ],
        )
        .await;
        let client = builder(&server).max_response_bytes(CAP).build().unwrap();

        let raw = client.call(&read()).await.unwrap();
        assert_eq!(raw.body().len(), CAP);
        let error = client.call(&read()).await.unwrap_err();
        assert_eq!(error.kind(), ErrorKind::BodyTooLarge);
    })
    .await;
}

// ---------------------------------------------------------------------------
// Gate 4: redirects are never followed.

#[tokio::test]
async fn gate4_a_redirect_is_an_http_error_and_the_target_gets_nothing() {
    bounded(async {
        let target = serve(dataset(), []).await;
        let location = format!("{}?stolen=1", target.base_url());
        let server = serve(dataset(), [])
            .await
            .then(Behaviour::redirect(&location));
        let writer = writer(&server);

        for (label, request) in [("read", read_as_user()), ("write", love())] {
            let error = writer.call(&request).await.unwrap_err();
            assert_eq!(error.kind(), ErrorKind::Http, "{label}");
            assert_eq!(error.http_status(), Some(302), "{label}");
            assert_eq!(error.retry(), Retry::No, "{label}");
            assert_clean(label, &error);
        }
        // Neither was retried: one request each.
        assert_eq!(server.request_count(), 2);
        assert_eq!(target.connections(), 0, "the redirect was followed");
        assert_eq!(target.request_count(), 0);
    })
    .await;
}

#[tokio::test]
async fn gate4_301_303_307_and_308_are_not_followed_either() {
    bounded(async {
        let target = serve(dataset(), []).await;
        let location = target.base_url();
        let statuses = [301, 303, 307, 308];
        let server = serve(
            dataset(),
            statuses
                .iter()
                // One for a read and one for a write.
                .flat_map(|status| {
                    let redirect = Behaviour::status_with(*status, "", &[("Location", &location)]);
                    [redirect.clone(), redirect]
                })
                .collect::<Vec<_>>(),
        )
        .await;
        let writer = writer(&server);

        for status in statuses {
            for (label, request) in [("read", read_as_user()), ("write", love())] {
                let error = writer.call(&request).await.unwrap_err();
                assert_eq!(error.kind(), ErrorKind::Http, "{status} {label}");
                assert_eq!(error.http_status(), Some(status), "{status} {label}");
                assert_eq!(error.retry(), Retry::No, "{status} {label}");
                assert_clean(label, &error);
            }
        }
        assert_eq!(server.request_count(), 8, "a redirect was retried");
        assert_eq!(target.connections(), 0, "a redirect was followed");
    })
    .await;
}

// ---------------------------------------------------------------------------
// Gate 5: the retry budget.

#[tokio::test]
async fn gate5_two_failures_then_success_succeeds_on_the_third_attempt() {
    bounded(async {
        let server = serve(dataset(), [Behaviour::html(500), Behaviour::html(500)]).await;
        let client = client(&server);

        let raw = client.call(&read()).await.unwrap();
        assert_eq!(raw.status(), 200);
        assert_eq!(server.request_count(), 3);
        // The same request each time.
        let requests = server.requests();
        assert_eq!(requests[0].params(), requests[1].params());
        assert_eq!(requests[1].params(), requests[2].params());
    })
    .await;
}

#[tokio::test]
async fn gate5_the_budget_counts_attempts_in_total_not_retries_on_top() {
    bounded(async {
        let server = serve(dataset(), [Behaviour::html(500), Behaviour::html(500)]).await;
        let two = builder(&server).read_attempts(2).build().unwrap();
        let error = two.call(&read()).await.unwrap_err();
        assert_eq!(error.kind(), ErrorKind::Http);
        assert_eq!(error.http_status(), Some(500));
        assert_eq!(server.request_count(), 2);

        // Always failing, with the default budget of three.
        let server = serve(dataset(), []).await.then(Behaviour::html(503));
        let error = client(&server).call(&read()).await.unwrap_err();
        assert_eq!(error.http_status(), Some(503));
        assert_eq!(server.request_count(), 3);

        // One attempt turns retries off.
        let server = serve(dataset(), []).await.then(Behaviour::html(500));
        let one = builder(&server).read_attempts(1).build().unwrap();
        one.call(&read()).await.unwrap_err();
        assert_eq!(server.request_count(), 1);
    })
    .await;
}

#[tokio::test]
async fn gate5_transient_api_errors_at_http_200_are_retried() {
    bounded(async {
        for code in [11, 16] {
            let server = serve(dataset(), [Behaviour::api_error(code)]).await;
            client(&server).call(&read()).await.unwrap();
            assert_eq!(server.request_count(), 2, "code {code}");
        }
    })
    .await;
}

#[tokio::test]
async fn gate5_errors_that_would_fail_the_same_way_again_are_not_retried() {
    bounded(async {
        for code in [6, 17, 26, 10, 9, 4] {
            let server = serve(dataset(), []).await.then(Behaviour::api_error(code));
            let error = client(&server).call(&read()).await.unwrap_err();
            assert_eq!(error.kind(), ErrorKind::Api, "code {code}");
            assert_eq!(error.api_code(), Some(ApiErrorCode::new(code)));
            assert_eq!(server.request_count(), 1, "code {code} was retried");
        }
        // HTTP statuses that are not transient.
        for status in [400, 403, 404] {
            let server = serve(dataset(), []).await.then(Behaviour::html(status));
            let error = client(&server).call(&read()).await.unwrap_err();
            assert_eq!(error.http_status(), Some(status));
            assert_eq!(server.request_count(), 1, "status {status} was retried");
        }
        // A 200 that is not JSON.
        let server = serve(dataset(), [Behaviour::status(200, "<html>")]).await;
        let error = client(&server).call(&read()).await.unwrap_err();
        assert_eq!(error.kind(), ErrorKind::Decode);
        assert_eq!(server.request_count(), 1);
    })
    .await;
}

#[tokio::test]
async fn gate5_transport_failures_of_a_read_are_retried() {
    bounded(async {
        for (label, behaviour) in [
            ("truncated body", Behaviour::TruncatedBody),
            ("hang-up", Behaviour::Close),
        ] {
            let server = serve(dataset(), [behaviour]).await;
            client(&server).call(&read()).await.unwrap();
            assert_eq!(server.request_count(), 2, "{label}");
        }
        let server = serve(dataset(), [Behaviour::StallBeforeResponse]).await;
        let client = builder(&server)
            .timeout(Duration::from_millis(100))
            .build()
            .unwrap();
        client.call(&read()).await.unwrap();
        assert_eq!(server.request_count(), 2, "timeout");
    })
    .await;
}

#[tokio::test]
async fn gate5_get_session_is_sent_once_when_it_may_have_arrived() {
    // The token is spent by the first request that arrives.
    let session = || Request::new(&methods::AUTH_GET_SESSION).param("token", "t");
    bounded(async {
        for (label, behaviour) in [
            ("truncated body", Behaviour::TruncatedBody),
            ("hang-up", Behaviour::Close),
        ] {
            let server = serve(dataset(), [behaviour]).await;
            let error = client(&server).call(&session()).await.unwrap_err();
            assert_eq!(error.kind(), ErrorKind::Transport, "{label}");
            assert_eq!(server.request_count(), 1, "{label}");
        }
        let server = serve(dataset(), [Behaviour::StallBeforeResponse]).await;
        let client = builder(&server)
            .timeout(Duration::from_millis(100))
            .build()
            .unwrap();
        let error = client.call(&session()).await.unwrap_err();
        assert_eq!(error.kind(), ErrorKind::Timeout);
        assert_eq!(server.request_count(), 1, "timeout");

        // An answer that says "try later" means the token was not spent.
        let server = serve(dataset(), [Behaviour::html(500)]).await;
        let _ = self::client(&server).call(&session()).await;
        assert_eq!(server.request_count(), 2, "HTTP 500");
    })
    .await;
}

#[tokio::test]
async fn gate5_waits_double_and_a_rate_limit_waits_five_times_as_long() {
    bounded(async {
        let delay = Duration::from_millis(40);

        let server = serve(dataset(), [Behaviour::html(500), Behaviour::html(500)]).await;
        builder(&server)
            .retry_delay(delay)
            .build()
            .unwrap()
            .call(&read())
            .await
            .unwrap();
        let gaps = gaps(&server.requests());
        assert_waited(gaps[0], delay, "first retry");
        assert_waited(gaps[1], 2 * delay, "second retry");

        for rate_limit in [Behaviour::status(429, ""), Behaviour::api_error(29)] {
            let server = serve(dataset(), [rate_limit]).await;
            builder(&server)
                .retry_delay(delay)
                .build()
                .unwrap()
                .call(&read())
                .await
                .unwrap();
            assert_waited(gaps_of(&server)[0], 5 * delay, "rate limit");
        }
    })
    .await;
}

fn gaps_of(server: &FakeLastfm) -> Vec<Duration> {
    gaps(&server.requests())
}

#[tokio::test]
async fn gate5_retry_after_is_a_floor_for_the_wait() {
    bounded(async {
        let server = serve(
            dataset(),
            [Behaviour::status_with(429, "", &[("Retry-After", "1")])],
        )
        .await;
        client(&server).call(&read()).await.unwrap();
        assert_eq!(server.request_count(), 2);
        assert_waited(
            gaps_of(&server)[0],
            Duration::from_secs(1),
            "Retry-After: 1",
        );
    })
    .await;
}

// ---------------------------------------------------------------------------
// Gate 6: a write is never retried.

#[tokio::test]
async fn gate6_a_write_is_sent_once_whatever_goes_wrong() {
    bounded(async {
        let cases: [(&str, Behaviour, ErrorKind); 5] = [
            ("500", Behaviour::html(500), ErrorKind::Http),
            ("429", Behaviour::status(429, ""), ErrorKind::Http),
            (
                "truncated body",
                Behaviour::TruncatedBody,
                ErrorKind::Transport,
            ),
            ("hang-up", Behaviour::Close, ErrorKind::Transport),
            ("stall", Behaviour::StallBeforeResponse, ErrorKind::Timeout),
        ];
        for (label, behaviour, kind) in cases {
            let server = serve(dataset(), [behaviour]).await;
            let writer = builder(&server)
                .timeout(Duration::from_millis(150))
                .build_writer()
                .unwrap();

            let error = writer.call(&love()).await.unwrap_err();
            assert_eq!(error.kind(), kind, "{label}");
            assert_eq!(error.delivery(), Some(Delivery::Unknown), "{label}");
            assert_eq!(error.retry(), Retry::No, "{label}");
            assert_eq!(error.method(), Some("track.love"), "{label}");
            assert_clean(label, &error);
            // A retry would happen inside the call, so it has returned.
            assert_eq!(server.request_count(), 1, "{label}: the write was resent");
        }
    })
    .await;
}

#[tokio::test]
async fn gate6_an_error_envelope_is_a_rejection_and_is_not_retried_either() {
    bounded(async {
        // Code 16 is advice to retry; the client still does not.
        for (code, retry) in [
            (6, Retry::No),
            (16, Retry::Later),
            (29, Retry::AfterBackoff),
            (9, Retry::AfterReauthentication),
        ] {
            let server = serve(dataset(), [Behaviour::api_error(code)]).await;
            let error = writer(&server).call(&love()).await.unwrap_err();
            assert_eq!(error.kind(), ErrorKind::Api, "code {code}");
            assert_eq!(error.delivery(), Some(Delivery::Rejected), "code {code}");
            assert_eq!(error.retry(), retry, "code {code}");
            assert_eq!(
                server.request_count(),
                1,
                "code {code}: the write was resent"
            );
        }
    })
    .await;
}

#[tokio::test]
async fn gate6_a_write_that_certainly_sent_nothing_is_still_not_retried() {
    bounded(async {
        // The error advises `Retry::Later`, but the client does not act on
        // advice for a write. A retry would first wait out the 30 s delay,
        // which the 10 s limit below would catch.
        let writer = builder_for(&dead_base_url())
            .retry_delay(Duration::from_secs(30))
            .build_writer()
            .unwrap();
        let result = tokio::time::timeout(Duration::from_secs(10), writer.call(&love())).await;
        let error = result.expect("the write was retried").unwrap_err();
        assert_eq!(error.kind(), ErrorKind::Transport);
        assert_eq!(error.delivery(), Some(Delivery::NotSent));
        assert_eq!(error.retry(), Retry::Later);
    })
    .await;
}

#[tokio::test]
async fn gate6_a_write_answered_normally_succeeds_once() {
    bounded(async {
        let server = serve(dataset(), []).await;
        let raw = writer(&server).call(&love()).await.unwrap();
        assert_eq!(raw.body(), b"{}");
        assert_eq!(server.request_count(), 1);
    })
    .await;
}

// ---------------------------------------------------------------------------
// Gate 7: every attempt is paced.

#[tokio::test]
async fn gate7_every_attempt_takes_a_slot_the_retries_included() {
    bounded(async {
        let interval = Duration::from_millis(80);
        let server = serve(dataset(), [Behaviour::html(500), Behaviour::html(500)]).await;
        let client = builder(&server).min_interval(interval).build().unwrap();

        client.call(&read()).await.unwrap();

        let requests = server.requests();
        assert_eq!(requests.len(), 3);
        for gap in gaps(&requests) {
            assert_waited(gap, interval, "consecutive attempts");
        }
    })
    .await;
}

async fn ten_concurrent_calls_start_one_interval_apart() {
    let interval = Duration::from_millis(40);
    let server = serve(dataset(), []).await;
    let client = builder(&server).min_interval(interval).build().unwrap();

    let mut calls = JoinSet::new();
    for _ in 0..10 {
        let client = client.clone();
        calls.spawn(async move { client.call(&read()).await });
    }
    while let Some(done) = calls.join_next().await {
        done.unwrap().unwrap();
    }

    let gaps = gaps_of(&server);
    assert_eq!(gaps.len(), 9);
    for gap in gaps {
        assert_waited(gap, interval, "concurrent calls");
    }
}

#[tokio::test]
async fn gate7_ten_concurrent_calls_from_clones_queue_instead_of_bursting() {
    bounded(ten_concurrent_calls_start_one_interval_apart()).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn gate7_ten_concurrent_calls_queue_on_a_multi_thread_runtime_too() {
    bounded(ten_concurrent_calls_start_one_interval_apart()).await;
}

/// A fake service on a runtime of its own, so that stalling the client's
/// runtime stalls neither the server nor the arrival times it records.
fn serve_apart(dataset: Dataset) -> (tokio::runtime::Runtime, FakeLastfm) {
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(1)
        .enable_all()
        .build()
        .unwrap();
    let server = runtime.block_on(serve(dataset, []));
    (runtime, server)
}

#[test]
fn gate7_a_stalled_runtime_does_not_turn_overdue_slots_into_a_burst() {
    let interval = Duration::from_millis(100);
    let (_server_runtime, server) = serve_apart(dataset());
    let client = builder(&server).min_interval(interval).build().unwrap();

    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    runtime.block_on(bounded(async {
        let mut calls = JoinSet::new();
        for _ in 0..5 {
            let client = client.clone();
            calls.spawn(async move { client.call(&read()).await });
        }
        // The first call goes out, the server sees it, and the other four are
        // queued behind it: all five ran to the pacer before any connection
        // could progress.
        server.wait_for_requests(1).await;
        // Several intervals pass while nothing on this runtime is polled.
        std::thread::sleep(5 * interval);
        while let Some(done) = calls.join_next().await {
            done.unwrap().unwrap();
        }
    }));

    let gaps = gaps_of(&server);
    assert_eq!(gaps.len(), 4);
    for gap in gaps {
        assert_waited(gap, interval, "calls queued across a stalled runtime");
    }
}

#[tokio::test]
async fn gate7_callers_dropped_while_queued_leave_no_debt_behind() {
    bounded(async {
        let interval = Duration::from_millis(100);
        let queued: u32 = 50;
        let server = serve(dataset(), []).await;
        let client = builder(&server).min_interval(interval).build().unwrap();

        client.call(&read()).await.unwrap();
        let mut calls = Vec::new();
        for _ in 0..queued {
            let client = client.clone();
            calls.push(tokio::spawn(async move { client.call(&read()).await }));
        }
        // Let every one of them run up to the pacer: the first is asleep on
        // its wait and the rest are queued behind it.
        tokio::task::yield_now().await;
        tokio::task::yield_now().await;
        let last = calls.pop().unwrap();
        for call in calls {
            call.abort();
        }
        last.await.unwrap().unwrap();

        let requests = server.requests();
        assert_eq!(requests.len(), 2, "a dropped call was sent");
        let waited = requests[1].arrived - requests[0].arrived;
        assert_waited(waited, interval, "the survivor");
        // With the slots of the dropped calls still spent this would be
        // fifty intervals. The bound is far above what is expected.
        assert!(
            waited < (queued / 2) * interval,
            "the survivor waited {waited:?}: dropped calls left debt"
        );
    })
    .await;
}

// ---------------------------------------------------------------------------
// Gate 8: HTTP 200 carrying an error is an error.

#[tokio::test]
async fn gate8_login_required_and_a_suspended_key_are_errors_not_empty_histories() {
    bounded(async {
        for (code, name) in [
            (17, ApiErrorCode::LOGIN_REQUIRED),
            (26, ApiErrorCode::SUSPENDED_KEY),
        ] {
            let faults = Faults {
                api_error: Some((None, code)),
                ..Faults::default()
            };
            let server = serve(Dataset::distinct(5).with_faults(faults), []).await;
            let client = client(&server);

            let error = client.user(USER).recent_tracks().send().await.unwrap_err();
            assert_eq!(error.kind(), ErrorKind::Api, "code {code}");
            assert_eq!(error.api_code(), Some(name));
            assert_eq!(error.http_status(), Some(200));
            assert_eq!(error.method(), Some("user.getRecentTracks"));

            let scan_error = client
                .user(USER)
                .recent_tracks()
                .window(whole(5))
                .scan()
                .unwrap()
                .next_page()
                .await
                .unwrap_err();
            assert_eq!(scan_error.api_code(), Some(name));
        }
    })
    .await;
}

#[tokio::test]
async fn gate8_a_genuinely_empty_window_is_a_success_with_nothing_in_it() {
    bounded(async {
        let server = serve(Dataset::distinct(5), []).await;
        let client = client(&server);

        let page = client
            .user(USER)
            .recent_tracks()
            .window(Window::new(BASE + 1_000_000, BASE + 2_000_000).unwrap())
            .limit(1)
            .send()
            .await
            .unwrap();
        assert_eq!(page.attr().total(), 0);
        assert!(page.scrobbles().is_empty());

        let (pages, summary) = read_scan(
            client
                .user(USER)
                .recent_tracks()
                .window(Window::new(BASE + 1_000_000, BASE + 2_000_000).unwrap())
                .scan()
                .unwrap(),
        )
        .await
        .unwrap();
        assert_eq!(summary.total(), 0);
        assert_eq!(pages.len(), 1);
    })
    .await;
}

// ---------------------------------------------------------------------------
// Gate 9: history over the socket.

#[tokio::test]
async fn gate9_a_scan_yields_every_marker_once_in_order_for_each_size() {
    bounded(async {
        for n in [0_usize, 1, 200, 201, 1001] {
            let dataset = Dataset::distinct(n).with_now_playing(true, Pages::All);
            let window = whole(n);
            let expected = dataset.expected(window);
            assert_eq!(expected.len(), n);
            let server = serve(dataset, []).await;

            let (pages, summary) = read_scan(
                client(&server)
                    .user(USER)
                    .recent_tracks()
                    .window(window)
                    .scan()
                    .unwrap(),
            )
            .await
            .unwrap();

            assert_eq!(markers(&pages), expected, "n = {n}");
            assert_eq!(summary.total(), n as u64);
            let expected_pages = n.div_ceil(200).max(1);
            assert_eq!(pages.len(), expected_pages, "n = {n}");
            assert_eq!(summary.pages() as usize, expected_pages);
            for page in &pages {
                assert!(
                    page.now_playing().is_some(),
                    "n = {n}, page {}",
                    page.number()
                );
            }

            let requests = server.requests();
            assert_eq!(requests.len(), expected_pages, "n = {n}");
            for (index, request) in requests.iter().enumerate() {
                assert_eq!(request.query_param("limit"), Some("200"), "n = {n}");
                assert_eq!(
                    request.query_param("from"),
                    Some(BASE.to_string().as_str()),
                    "n = {n}"
                );
                assert_eq!(
                    request.query_param("to"),
                    Some((BASE + 10 * (n as u64 + 1)).to_string().as_str()),
                    "n = {n}"
                );
                assert_eq!(
                    request.query_param("page"),
                    Some((index + 1).to_string().as_str()),
                    "n = {n}"
                );
                assert_eq!(request.query_param("user"), Some(USER));
            }
        }
    })
    .await;
}

#[tokio::test]
async fn gate9_the_page_size_and_the_extended_flag_hold_for_every_request() {
    bounded(async {
        let dataset = Dataset::distinct(95);
        let window = whole(95);
        let expected = dataset.expected(window);
        let server = serve(dataset, []).await;

        let scan = client(&server)
            .user(USER)
            .recent_tracks()
            .window(window)
            .extended(true)
            .limit(30)
            .scan()
            .unwrap();
        let (pages, summary) = read_scan(scan).await.unwrap();

        assert_eq!(markers(&pages), expected);
        assert_eq!(summary.pages(), 4);
        assert!(pages[0].scrobbles().iter().all(|s| s.loved().is_some()));
        for request in server.requests() {
            assert_eq!(request.query_param("limit"), Some("30"));
            assert_eq!(request.query_param("extended"), Some("1"));
        }
    })
    .await;
}

#[tokio::test]
async fn gate9_a_total_that_changes_mid_scan_fails_it_and_nothing_more_is_sent() {
    bounded(async {
        let faults = Faults {
            arrive_from_page: Some(2),
            ..Faults::default()
        };
        let server = serve(Dataset::distinct(450).with_faults(faults), []).await;
        // No upper bound, so the scrobble that arrives is inside the window.
        let mut scan = client(&server)
            .user(USER)
            .recent_tracks()
            .window(Window::since(BASE))
            .scan()
            .unwrap();

        scan.next_page().await.unwrap().unwrap();
        let error = scan.next_page().await.unwrap_err();
        assert_eq!(error.kind(), ErrorKind::Inconsistent);
        assert_eq!(server.request_count(), 2);

        // The scan is dead: it says so and sends nothing.
        for _ in 0..2 {
            let again = scan.next_page().await.unwrap_err();
            assert_eq!(again.kind(), ErrorKind::Inconsistent);
        }
        assert_eq!(server.request_count(), 2);
        assert_eq!(scan.finish().unwrap_err().kind(), ErrorKind::Inconsistent);
    })
    .await;
}

#[tokio::test]
async fn gate9_a_transient_failure_mid_scan_is_retried_and_the_scan_is_still_exact() {
    bounded(async {
        let dataset = Dataset::distinct(450);
        let window = whole(450);
        let expected = dataset.expected(window);
        let server = serve(dataset, [Behaviour::Normal, Behaviour::html(500)]).await;

        let (pages, summary) = read_scan(
            client(&server)
                .user(USER)
                .recent_tracks()
                .window(window)
                .scan()
                .unwrap(),
        )
        .await
        .unwrap();

        assert_eq!(markers(&pages), expected);
        assert_eq!(summary.total(), 450);
        let requests = server.requests();
        assert_eq!(requests.len(), 4, "three pages and one retry");
        let pages_asked: Vec<_> = requests
            .iter()
            .map(|r| r.query_param("page").unwrap().to_owned())
            .collect();
        assert_eq!(pages_asked, ["1", "2", "2", "3"]);
        assert_eq!(requests[1].params(), requests[2].params());
    })
    .await;
}

#[tokio::test]
async fn gate9_a_failure_that_is_not_retried_leaves_the_page_outstanding() {
    bounded(async {
        let dataset = Dataset::distinct(450);
        let window = whole(450);
        let expected = dataset.expected(window);
        let server = serve(dataset, [Behaviour::Normal, Behaviour::api_error(17)]).await;
        let mut scan = client(&server)
            .user(USER)
            .recent_tracks()
            .window(window)
            .scan()
            .unwrap();

        let mut pages = vec![scan.next_page().await.unwrap().unwrap()];
        let error = scan.next_page().await.unwrap_err();
        assert_eq!(error.api_code(), Some(ApiErrorCode::LOGIN_REQUIRED));
        assert_eq!(server.request_count(), 2, "code 17 must not be retried");

        // The server has recovered. The same page is asked for again.
        while let Some(page) = scan.next_page().await.unwrap() {
            pages.push(page);
        }
        let summary = scan.finish().unwrap();

        assert_eq!(markers(&pages), expected);
        assert_eq!(summary.total(), 450);
        let requests = server.requests();
        assert_eq!(requests[1].params(), requests[2].params());
        assert_eq!(requests[2].query_param("page"), Some("2"));
        assert_eq!(requests.len(), 4);
    })
    .await;
}

#[tokio::test]
async fn gate9_scan_refuses_a_preset_page_before_anything_is_sent() {
    bounded(async {
        let server = serve(dataset(), []).await;
        let user = client(&server).user(USER);

        for query in [
            user.recent_tracks().page(2),
            user.recent_tracks().page(0),
            user.recent_tracks().window(whole(3)).limit(10).page(1),
        ] {
            let error = query.scan().unwrap_err();
            assert_eq!(error.kind(), ErrorKind::InvalidRequest);
            assert_eq!(error.method(), Some("user.getRecentTracks"));
        }
        assert_eq!(server.connections(), 0);
    })
    .await;
}

#[tokio::test]
async fn gate9_scan_refuses_a_limit_outside_1_to_200_before_anything_is_sent() {
    bounded(async {
        let server = serve(dataset(), []).await;
        let user = client(&server).user(USER);

        for limit in [0, 201, u32::MAX] {
            let error = user.recent_tracks().limit(limit).scan().unwrap_err();
            assert_eq!(error.kind(), ErrorKind::InvalidRequest, "limit {limit}");
        }
        for limit in [1, 200] {
            user.recent_tracks().limit(limit).scan().unwrap();
        }
        assert_eq!(server.connections(), 0);
    })
    .await;
}

#[tokio::test]
async fn gate9_a_scan_reports_the_window_and_page_size_it_was_made_with() {
    bounded(async {
        let server = serve(dataset(), []).await;
        let user = client(&server).user(USER);

        let scan = user.recent_tracks().scan().unwrap();
        assert_eq!(scan.window(), Window::ALL);
        assert_eq!(scan.page_size(), 200);

        let scan = user
            .recent_tracks()
            .window(whole(3))
            .limit(7)
            .scan()
            .unwrap();
        assert_eq!(scan.window(), whole(3));
        assert_eq!(scan.page_size(), 7);
    })
    .await;
}

#[tokio::test]
async fn gate9_the_configured_limit_is_the_page_size_of_every_recorded_request() {
    bounded(async {
        let dataset = Dataset::distinct(95);
        let expected = dataset.expected(Window::ALL);
        let server = serve(dataset, []).await;

        let scan = client(&server)
            .user(USER)
            .recent_tracks()
            .limit(40)
            .scan()
            .unwrap();
        let (pages, summary) = read_scan(scan).await.unwrap();

        assert_eq!(markers(&pages), expected);
        assert_eq!(summary.pages(), 3);
        let requests = server.requests();
        assert_eq!(requests.len(), 3);
        for (index, request) in requests.iter().enumerate() {
            assert_eq!(request.query_param("limit"), Some("40"));
            assert_eq!(
                request.query_param("page"),
                Some((index + 1).to_string().as_str())
            );
        }

        // Unset, the page size is the largest, 200.
        let server = serve(Dataset::distinct(5), []).await;
        read_scan(client(&server).user(USER).recent_tracks().scan().unwrap())
            .await
            .unwrap();
        assert_eq!(server.requests()[0].query_param("limit"), Some("200"));
    })
    .await;
}

#[tokio::test]
async fn gate9_a_response_to_another_page_fails_the_scan_and_it_never_reads_as_finished() {
    bounded(async {
        let dataset = Dataset::distinct(250);
        // The body of page 1, served in answer to the request for page 2.
        let page_one = respond_to(
            &dataset,
            &[
                ("method".to_owned(), "user.getRecentTracks".to_owned()),
                ("api_key".to_owned(), KEY.to_owned()),
                ("user".to_owned(), USER.to_owned()),
                ("limit".to_owned(), "100".to_owned()),
                ("page".to_owned(), "1".to_owned()),
            ],
        )
        .1;
        let server = serve(
            dataset,
            [Behaviour::Normal, Behaviour::status(200, page_one)],
        )
        .await;
        let mut scan = client(&server)
            .user(USER)
            .recent_tracks()
            .limit(100)
            .scan()
            .unwrap();

        scan.next_page().await.unwrap().unwrap();
        let error = scan.next_page().await.unwrap_err();
        assert_eq!(error.kind(), ErrorKind::Inconsistent);

        // A dead scan is an error every time, never `Ok(None)`.
        for _ in 0..3 {
            let again = scan.next_page().await;
            assert_eq!(again.unwrap_err().kind(), ErrorKind::Inconsistent);
        }
        assert_eq!(server.request_count(), 2);
        assert_eq!(scan.finish().unwrap_err().kind(), ErrorKind::Inconsistent);
    })
    .await;
}

#[tokio::test]
async fn gate9_a_page_carries_the_exact_bytes_the_server_sent() {
    bounded(async {
        let dataset = Dataset::distinct(5);
        let server = serve(dataset.clone(), []).await;
        let page = client(&server)
            .user(USER)
            .recent_tracks()
            .limit(3)
            .send()
            .await
            .unwrap();

        let request = &server.requests()[0];
        let (status, body) = respond_to(&dataset, &request.params());
        assert_eq!(status, 200);
        assert_eq!(page.raw().body(), body.as_slice());
        assert_eq!(page.raw().status(), 200);
        assert_eq!(page.raw().method(), "user.getRecentTracks");
        assert_eq!(page.scrobbles().len(), 3);
        assert_eq!(page.attr().total(), 5);

        let (typed, raw): (RecentTracksPage, _) = page.into_parts();
        assert_eq!(typed.scrobbles().len(), 3);
        assert_eq!(raw.body(), body.as_slice());
    })
    .await;
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(8))]

    #[test]
    fn gate9_a_scan_over_the_socket_matches_the_dataset(
        n in 0_usize..=260,
        page_size in 1_u32..=200,
        skip in 0_u64..=30,
        now_playing in any::<bool>(),
    ) {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        runtime.block_on(bounded(async {
            let mut dataset = Dataset::distinct(n);
            if now_playing {
                dataset = dataset.with_now_playing(true, Pages::All);
            }
            let from = BASE + 10 * skip.min(n as u64);
            let window = Window::new(from, BASE + 10 * (n as u64 + 1)).unwrap();
            let expected = dataset.expected(window);
            let server = serve(dataset, []).await;

            let scan = client(&server)
                .user(USER)
                .recent_tracks()
                .window(window)
                .limit(page_size)
                .scan()
                .unwrap();
            let (pages, summary) = read_scan(scan).await.unwrap();

            assert_eq!(markers(&pages), expected);
            assert_eq!(summary.total(), expected.len() as u64);
            let expected_pages = expected.len().div_ceil(page_size as usize).max(1);
            assert_eq!(pages.len(), expected_pages);
            let requests = server.requests();
            assert_eq!(requests.len(), expected_pages);
            for (index, request) in requests.iter().enumerate() {
                assert_eq!(
                    request.query_param("limit"),
                    Some(page_size.to_string().as_str())
                );
                assert_eq!(
                    request.query_param("page"),
                    Some((index + 1).to_string().as_str())
                );
            }
        }));
    }
}

// ---------------------------------------------------------------------------
// Gate 10: no secret escapes.

#[tokio::test]
async fn gate10_no_error_class_shows_a_credential_or_a_url() {
    bounded(async {
        let mut errors: Vec<(&str, Error)> = Vec::new();

        // Dead port.
        let dead = builder_for(&dead_base_url()).build_writer().unwrap();
        errors.push((
            "dead port, read",
            dead.call(&read_as_user()).await.unwrap_err(),
        ));
        errors.push(("dead port, write", dead.call(&love()).await.unwrap_err()));

        // Everything else the server can do wrong.
        let target = serve(dataset(), []).await;
        let location = target.base_url();
        let cases: Vec<(&str, Behaviour)> = vec![
            ("stall", Behaviour::StallBeforeResponse),
            ("stall mid-body", Behaviour::StallMidBody),
            ("500", Behaviour::html(500)),
            ("truncated body", Behaviour::TruncatedBody),
            ("hang-up", Behaviour::Close),
            ("api error", Behaviour::api_error(9)),
            ("api error, rate limit", Behaviour::api_error(29)),
            ("not json", Behaviour::status(200, "<html>")),
            (
                "redirect",
                Behaviour::status_with(302, "", &[("Location", &location)]),
            ),
            (
                "too large (announced)",
                Behaviour::OversizeBody {
                    total: 4096,
                    chunked: false,
                },
            ),
            (
                "too large (chunked)",
                Behaviour::OversizeBody {
                    total: 4096,
                    chunked: true,
                },
            ),
        ];
        for (label, behaviour) in cases {
            let server = serve(dataset(), []).await.then(behaviour);
            let writer = builder(&server)
                .timeout(Duration::from_millis(100))
                .max_response_bytes(1024)
                .read_attempts(2)
                .build_writer()
                .unwrap();
            errors.push((label, writer.call(&read_as_user()).await.unwrap_err()));
            if label != "too large (announced)" && label != "too large (chunked)" {
                errors.push((label, writer.call(&love()).await.unwrap_err()));
            }
        }

        let kinds: std::collections::BTreeSet<_> = errors
            .iter()
            .map(|(_, e)| format!("{:?}", e.kind()))
            .collect();
        for expected in [
            "Transport",
            "Timeout",
            "Http",
            "Api",
            "Decode",
            "BodyTooLarge",
        ] {
            assert!(
                kinds.contains(expected),
                "no {expected} error was provoked: {kinds:?}"
            );
        }
        for (label, error) in &errors {
            assert_clean(label, error);
        }
    })
    .await;
}

#[tokio::test]
async fn gate10_debug_of_a_client_and_its_parts_shows_no_credential() {
    bounded(async {
        let server = serve(dataset(), []).await;
        let builder = builder(&server);
        let client = builder.clone().build().unwrap();
        let with_other = client.with_session(SessionKey::new("SENTINEL_SESSION_KEY_OTHER"));
        let user = client.user(USER);
        let query = user.recent_tracks().as_user();
        let scan = query.clone().scan().unwrap();

        let shown = [
            format!("{builder:?}"),
            format!("{builder:#?}"),
            format!("{client:?}"),
            format!("{client:#?}"),
            format!("{with_other:?}"),
            format!("{user:?}"),
            format!("{query:?}"),
            format!("{scan:?}"),
        ];
        for text in shown {
            for secret in [KEY, SECRET, SESSION, "SENTINEL_SESSION_KEY_OTHER"] {
                assert!(!text.contains(secret), "`{secret}` appears in: {text}");
            }
        }
        assert!(format!("{client:?}").contains("redacted"));
    })
    .await;
}

#[tokio::test]
async fn gate10_debug_never_shows_a_header_value_the_network_sent() {
    bounded(async {
        // The body of a real first page of a scan, served again under
        // hostile headers.
        let plain = serve(Dataset::distinct(3), []).await;
        let body = client(&plain)
            .user(USER)
            .recent_tracks()
            .window(whole(3))
            .scan()
            .unwrap()
            .next_page()
            .await
            .unwrap()
            .unwrap()
            .raw()
            .body()
            .to_vec();
        let sentinels = [
            ("Content-Type", "application/json; SENTINEL_CONTENT_TYPE"),
            ("Retry-After", "SENTINEL_RETRY_AFTER"),
            ("Date", "SENTINEL_DATE"),
            ("Cache-Control", "max-age=SENTINEL_CACHE_CONTROL"),
            ("Expires", "SENTINEL_EXPIRES"),
            ("ETag", "SENTINEL_ETAG"),
            ("Last-Modified", "SENTINEL_LAST_MODIFIED"),
            ("Age", "SENTINEL_AGE"),
        ];
        let hostile = serve(Dataset::distinct(3), [])
            .await
            .then(Behaviour::status_with(200, body, &sentinels));
        let client = client(&hostile);

        let raw = client.call(&read()).await.unwrap();
        // The values are kept, and available through `header`.
        assert_eq!(raw.header("retry-after"), Some("SENTINEL_RETRY_AFTER"));
        assert_eq!(raw.header("date"), Some("SENTINEL_DATE"));
        assert_eq!(raw.header("etag"), Some("SENTINEL_ETAG"));
        assert_eq!(raw.header("age"), Some("SENTINEL_AGE"));

        let page = client.user(USER).recent_tracks().send().await.unwrap();
        let mut scan = client
            .user(USER)
            .recent_tracks()
            .window(whole(3))
            .scan()
            .unwrap();
        let scan_page = scan.next_page().await.unwrap().unwrap();
        assert_eq!(scan_page.raw().header("date"), Some("SENTINEL_DATE"));
        let http = scrobl::protocol::HttpResponse::new(200, "{}")
            .with_header("Content-Type", "SENTINEL_CONTENT_TYPE")
            .with_header("Retry-After", "SENTINEL_RETRY_AFTER")
            .with_header("Date", "SENTINEL_DATE")
            .with_header("Cache-Control", "SENTINEL_CACHE_CONTROL")
            .with_header("ETag", "SENTINEL_ETAG");

        let shown = [
            ("Raw", format!("{raw:?}\n{raw:#?}")),
            ("Response", format!("{page:?}\n{page:#?}")),
            (
                "Response<()>",
                format!("{:?}", Response::new((), raw.clone())),
            ),
            ("HttpResponse", format!("{http:?}\n{http:#?}")),
            ("ScanPage", format!("{scan_page:?}\n{scan_page:#?}")),
        ];
        for (what, text) in shown {
            assert!(!text.contains("SENTINEL"), "{what} shows a value: {text}");
        }
        // The header names are still visible, so a log says what was kept.
        assert!(format!("{raw:?}").contains("content-type"));
        assert!(format!("{http:?}").contains("retry-after"));
        assert!(format!("{raw:?}").contains("cache-control"));
        assert!(format!("{http:?}").contains("etag"));
    })
    .await;
}

#[tokio::test]
async fn the_headers_a_cache_goes_by_are_kept_and_capped_and_others_dropped() {
    bounded(async {
        let long = "x".repeat(1000);
        let headers = [
            ("Cache-Control", "max-age=60"),
            ("Expires", "Thu, 08 Oct 2026 10:01:00 GMT"),
            ("ETag", "\"abc\""),
            ("Last-Modified", "Thu, 08 Oct 2026 10:00:00 GMT"),
            ("Age", "5"),
            ("Date", long.as_str()),
            ("Set-Cookie", "session=abc"),
            ("X-Anything", "x"),
        ];
        let server = serve(Dataset::distinct(3), [])
            .await
            .then(Behaviour::status_with(200, "{}", &headers));
        let raw = client(&server).call(&read()).await.unwrap();

        assert_eq!(raw.header("cache-control"), Some("max-age=60"));
        assert_eq!(raw.header("Expires"), Some("Thu, 08 Oct 2026 10:01:00 GMT"));
        assert_eq!(raw.header("etag"), Some("\"abc\""));
        assert_eq!(
            raw.header("last-modified"),
            Some("Thu, 08 Oct 2026 10:00:00 GMT")
        );
        assert_eq!(raw.header("age"), Some("5"));
        assert_eq!(raw.header("date").map(str::len), Some(256));
        assert_eq!(raw.header("set-cookie"), None);
        assert_eq!(raw.header("x-anything"), None);
    })
    .await;
}

#[tokio::test]
async fn gate10_a_build_error_shows_no_credential() {
    let error = Client::builder(ApiKey::new(KEY))
        .secret(ApiSecret::new(SECRET))
        .session(SessionKey::new(SESSION))
        .user_agent("")
        .build()
        .unwrap_err();
    assert_eq!(error.kind(), ErrorKind::Config);
    for text in texts(&error) {
        for secret in [KEY, SECRET, SESSION] {
            assert!(!text.contains(secret), "{text}");
        }
    }
}

// ---------------------------------------------------------------------------
// Gate 11: cancellation.

/// Polls `call` until the server has seen `n` requests, then drops it: the
/// caller gave up while the request was in flight.
async fn abandon_once_sent<F: Future>(server: &FakeLastfm, n: usize, call: F) {
    tokio::select! {
        _ = call => panic!("the call finished before it was abandoned"),
        () = server.wait_for_requests(n) => {}
    }
}

#[tokio::test]
async fn gate11_a_dropped_call_leaves_the_client_and_its_clones_working_and_paced() {
    bounded(async {
        let interval = Duration::from_millis(80);
        let server = serve(dataset(), [Behaviour::StallBeforeResponse]).await;
        let client = builder(&server).min_interval(interval).build().unwrap();
        let clone = client.clone();

        abandon_once_sent(&server, 1, client.call(&read())).await;

        client.call(&read()).await.unwrap();
        clone.call(&read()).await.unwrap();

        let requests = server.requests();
        assert_eq!(requests.len(), 3);
        for gap in gaps(&requests) {
            assert_waited(gap, interval, "after a cancellation");
        }
    })
    .await;
}

#[tokio::test]
async fn gate11_a_dropped_write_is_not_resent_and_the_client_goes_on() {
    bounded(async {
        let server = serve(dataset(), [Behaviour::StallBeforeResponse]).await;
        let writer = writer(&server);

        abandon_once_sent(&server, 1, writer.call(&love())).await;
        writer.call(&read()).await.unwrap();

        let requests = server.requests();
        assert_eq!(requests.len(), 2, "the dropped write was resent");
        assert_eq!(requests[0].verb, "POST");
    })
    .await;
}

#[tokio::test]
async fn gate11_a_dropped_next_page_leaves_the_same_page_outstanding() {
    bounded(async {
        let dataset = Dataset::distinct(250);
        let window = whole(250);
        let expected = dataset.expected(window);
        let server = serve(dataset, [Behaviour::StallBeforeResponse]).await;
        let mut scan = client(&server)
            .user(USER)
            .recent_tracks()
            .window(window)
            .scan()
            .unwrap();

        abandon_once_sent(&server, 1, scan.next_page()).await;

        let mut pages = Vec::new();
        while let Some(page) = scan.next_page().await.unwrap() {
            pages.push(page);
        }
        scan.finish().unwrap();
        assert_eq!(markers(&pages), expected);

        let requests = server.requests();
        assert_eq!(requests[0].params(), requests[1].params());
        assert_eq!(requests[1].query_param("page"), Some("1"));
    })
    .await;
}

// ---------------------------------------------------------------------------
// Gate 12: Tauri-shaped use compiles.

fn assert_send<T: Send>(_: &T) {}
fn assert_shareable<T: Clone + Send + Sync + 'static>() {}
fn assert_owned<T: Send + 'static>() {}
fn assert_sync<T: Sync>() {}

#[test]
fn gate12_the_client_is_clone_send_sync_and_static_and_every_future_is_send() {
    assert_shareable::<Client>();
    assert_shareable::<Writer>();
    assert_shareable::<User>();
    assert_shareable::<RecentTracksQuery>();
    assert_shareable::<ClientBuilder>();
    assert_owned::<Scan>();
    assert_shareable::<Response<RecentTracksPage>>();
    assert_owned::<Error>();
    assert_sync::<Error>();

    #[allow(dead_code)]
    fn futures(client: &Client, request: &Request, scan: &mut Scan) {
        assert_send(&client.call(request));
        assert_send(&client.user("u").recent_tracks().send());
        assert_send(&scan.next_page());
        #[allow(dead_code)]
        fn writes(writer: &Writer, request: &Request) {
            assert_send(&writer.call(request));
            assert_send(&writer.user("u").recent_tracks().send());
        }
        // And owned by a task, as a Tauri command holds them.
        assert_send(&async move {
            let client = client.clone();
            client.user("u").recent_tracks().send().await
        });
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn gate12_a_scan_moves_into_a_spawned_task_and_a_client_is_shared_between_tasks() {
    bounded(async {
        let dataset = Dataset::distinct(450);
        let window = whole(450);
        let expected = dataset.expected(window);
        let server = serve(dataset, []).await;
        let client = client(&server);

        let scan = client
            .user(USER)
            .recent_tracks()
            .window(window)
            .scan()
            .unwrap();
        let scanning = tokio::spawn(read_scan(scan));
        let single = {
            let client = client.clone();
            tokio::spawn(async move { client.user(USER).recent_tracks().limit(5).send().await })
        };

        let (pages, summary) = scanning.await.unwrap().unwrap();
        assert_eq!(markers(&pages), expected);
        assert_eq!(summary.total(), 450);
        assert_eq!(single.await.unwrap().unwrap().scrobbles().len(), 5);
    })
    .await;
}

// ---------------------------------------------------------------------------
// Gate 13: a bare current-thread runtime is enough.

#[test]
fn gate13_a_client_made_outside_a_runtime_runs_on_a_current_thread_runtime() {
    // The server has its own runtime, so the client's runtime holds nothing
    // but what the client itself starts.
    let server_runtime = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(1)
        .enable_all()
        .build()
        .unwrap();
    let dataset = Dataset::distinct(450);
    let window = whole(450);
    let expected = dataset.expected(window);
    let server = server_runtime.block_on(serve(dataset, []));

    // No runtime is entered on this thread.
    assert!(tokio::runtime::Handle::try_current().is_err());
    let client = builder(&server).build().unwrap();

    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    runtime.block_on(bounded(async {
        let handle = tokio::runtime::Handle::current();
        assert_eq!(
            handle.runtime_flavor(),
            tokio::runtime::RuntimeFlavor::CurrentThread
        );

        client.call(&read()).await.unwrap();
        let (pages, summary) = read_scan(
            client
                .user(USER)
                .recent_tracks()
                .window(window)
                .scan()
                .unwrap(),
        )
        .await
        .unwrap();
        assert_eq!(markers(&pages), expected);
        assert_eq!(summary.total(), 450);

        // Nothing the client started outlives its connections.
        let metrics = handle.metrics();
        for _ in 0..200 {
            if metrics.num_alive_tasks() == 0 {
                return;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        panic!("{} tasks are still alive", metrics.num_alive_tasks());
    }));
}

#[test]
fn gate13_a_client_outlives_the_runtime_it_was_first_used_on() {
    let server_runtime = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(1)
        .enable_all()
        .build()
        .unwrap();
    let server = server_runtime.block_on(serve(dataset(), []));
    let client = builder(&server).build().unwrap();

    for _ in 0..3 {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        runtime.block_on(bounded(async {
            client.call(&read()).await.unwrap();
        }));
    }
    assert_eq!(server.request_count(), 3);
}

// ---------------------------------------------------------------------------
// Gate 14: pooled connections.
//
// The server here keeps connections open and serves several requests on
// each, so the paths that reuse a connection run, not only the ones that open
// a new one. Connection counts are recorded so reuse is asserted, not assumed.

/// Lets the connection task of the client notice that a body has ended and
/// hand its connection back to the pool, before the next call looks for one.
async fn let_the_pool_settle() {
    for _ in 0..10 {
        tokio::task::yield_now().await;
    }
}

#[tokio::test]
async fn gate14_a_read_then_a_read_reuses_the_connection() {
    bounded(async {
        let server = serve(dataset(), []).await.keep_alive();
        let client = client(&server);

        let first = client.call(&read()).await.unwrap();
        let_the_pool_settle().await;
        let second = client.call(&read()).await.unwrap();
        assert_eq!(first.body(), second.body());

        assert_eq!(server.request_count(), 2);
        assert_eq!(server.connections(), 1, "the second call did not reuse it");
        let requests = server.requests();
        assert_eq!((requests[0].number, requests[0].index), (0, 0));
        assert_eq!((requests[1].number, requests[1].index), (0, 1));
    })
    .await;
}

#[tokio::test]
async fn gate14_a_write_after_the_server_closed_an_idle_connection_arrives_exactly_once() {
    bounded(async {
        let server = serve(dataset(), []).await.keep_alive();
        let writer = writer(&server);

        writer.call(&read()).await.unwrap();
        let_the_pool_settle().await;
        server.hang_up_idle();
        // There is no signal for "the client has seen the hang-up": it is a
        // socket event in the client's own runtime. This is the one wait.
        tokio::time::sleep(Duration::from_millis(100)).await;

        let raw = writer.call(&love()).await.unwrap();
        assert_eq!(raw.body(), b"{}");

        let requests = server.requests();
        assert_eq!(requests.len(), 2, "the write was not sent exactly once");
        assert_eq!(requests[1].verb, "POST");
        assert_eq!(
            requests[1].number, 1,
            "the write used the closed connection"
        );
        assert_eq!(server.connections(), 2);
    })
    .await;
}

#[tokio::test]
async fn gate14_the_call_after_a_body_too_large_gets_a_clean_response() {
    bounded(async {
        let server = serve(
            dataset(),
            [Behaviour::OversizeBody {
                total: FLOOD,
                chunked: false,
            }],
        )
        .await
        .keep_alive();
        let client = builder(&server).max_response_bytes(CAP).build().unwrap();

        let error = client.call(&read()).await.unwrap_err();
        assert_eq!(error.kind(), ErrorKind::BodyTooLarge);
        let_the_pool_settle().await;

        // The unread rest of the first body must not be mistaken for the
        // start of this answer.
        let raw = client.call(&read()).await.unwrap();
        let request = &server.requests()[1];
        let (_, expected) = respond_to(&dataset(), &request.params());
        assert_eq!(raw.body(), expected.as_slice());
        assert_eq!(server.request_count(), 2);
        assert_eq!(request.number, 1, "the abandoned connection was reused");
    })
    .await;
}

#[tokio::test]
async fn gate14_a_chunked_body_too_large_on_a_kept_alive_connection_is_the_same() {
    bounded(async {
        let server = serve(
            dataset(),
            [Behaviour::OversizeBody {
                total: FLOOD,
                chunked: true,
            }],
        )
        .await
        .keep_alive();
        let client = builder(&server).max_response_bytes(CAP).build().unwrap();

        let error = client.call(&read()).await.unwrap_err();
        assert_eq!(error.kind(), ErrorKind::BodyTooLarge);
        let_the_pool_settle().await;

        let raw = client.call(&read()).await.unwrap();
        let (_, expected) = respond_to(&dataset(), &server.requests()[1].params());
        assert_eq!(raw.body(), expected.as_slice());
        assert_eq!(server.requests()[1].number, 1);
    })
    .await;
}

// ---------------------------------------------------------------------------
// Gate 15: a client never writes, and a writer is the only thing that does.

/// The ten methods that change the account, each with what it requires.
fn every_write() -> Vec<Request> {
    let writes: Vec<_> = methods::ALL
        .iter()
        .filter(|spec| spec.write())
        .map(|spec| {
            spec.params()
                .iter()
                .filter(|param| param.requirement() == Requirement::Required)
                .fold(Request::new(spec), |request, param| {
                    if param.indexed() {
                        request.indexed(param.name(), 0, "1700000000")
                    } else {
                        request.param(param.name(), "Synthetic")
                    }
                })
        })
        .collect();
    assert_eq!(writes.len(), 10);
    writes
}

/// Asks `client` for every write, complete and bare, plainly and as the
/// user, and checks each refusal.
async fn assert_refuses_every_write(label: &str, client: &Client) {
    for write in every_write() {
        let bare = Request::new(methods::by_name(write.method()).unwrap());
        for request in [write.clone(), write.clone().as_user(), bare] {
            let error = client.call(&request).await.unwrap_err();
            let method = write.method();
            assert_eq!(error.kind(), ErrorKind::ReadOnly, "{label}: {method}");
            assert_eq!(error.method(), Some(method), "{label}");
            assert_eq!(
                error.delivery(),
                Some(Delivery::NotSent),
                "{label}: {method}"
            );
            assert_eq!(error.retry(), Retry::No, "{label}: {method}");
            assert_clean(label, &error);
        }
    }
}

#[tokio::test]
async fn gate15_a_client_refuses_every_write_and_nothing_reaches_the_socket() {
    bounded(async {
        let server = serve(dataset(), []).await;
        // An hour between requests: a refusal that waited for its turn to
        // be paced would hang the test.
        let paced = || builder(&server).min_interval(Duration::from_secs(3600));
        let other = || SessionKey::new("SENTINEL_SESSION_KEY_OTHER");

        // A client holding everything a write needs except permission.
        let client = paced().build().unwrap();
        assert_refuses_every_write("a client", &client).await;
        assert_refuses_every_write("a clone", &client.clone()).await;
        assert_refuses_every_write("another session", &client.with_session(other())).await;

        // A client holding less is refused the same way, not for what it lacks.
        let key_only = Client::builder(ApiKey::new(KEY))
            .base_url(server.base_url())
            .build()
            .unwrap();
        assert_refuses_every_write("key only", &key_only).await;

        // Every way a writer can be seen as a client.
        let writer = paced().build_writer().unwrap();
        assert_refuses_every_write("a writer as a client", &writer).await;
        assert_refuses_every_write("cloned out of a writer", &Client::clone(&writer)).await;
        let session_on_the_client = Client::with_session(&writer, other());
        assert_refuses_every_write("a session on a writer's client", &session_on_the_client).await;

        assert_eq!(server.connections(), 0);
        assert_eq!(server.request_count(), 0);
    })
    .await;
}

#[tokio::test]
async fn gate15_a_refused_write_spends_no_pacing_and_leaves_reads_working() {
    bounded(async {
        let server = serve(dataset(), []).await;
        let interval = Duration::from_millis(200);
        let client = builder(&server).min_interval(interval).build().unwrap();

        client.call(&read()).await.unwrap();
        let started = tokio::time::Instant::now();
        client.call(&love()).await.unwrap_err();
        assert!(started.elapsed() < interval, "the refusal was paced");
        // Reads as the user, and the authentication methods, are not writes.
        let raw = client.call(&read_as_user()).await.unwrap();
        assert!(!raw.body().is_empty());
        let token = Request::new(&methods::AUTH_GET_TOKEN);
        let _ = client.call(&token).await;

        let requests = server.requests();
        let sent: Vec<_> = requests
            .iter()
            .map(|r| r.param("method").unwrap())
            .collect();
        assert_eq!(
            sent,
            [
                "user.getRecentTracks",
                "user.getRecentTracks",
                "auth.getToken"
            ]
        );
        assert_eq!(requests[1].query_param("sk"), Some(SESSION));
    })
    .await;
}

#[tokio::test]
async fn gate15_a_writer_sends_each_write_once_with_the_session_it_was_given() {
    bounded(async {
        let server = serve(dataset(), []).await;
        let writer = writer(&server);
        let other = "SENTINEL_SESSION_KEY_OTHER";

        let writes = every_write();
        for write in &writes {
            // The fake service answers only some methods; what matters here
            // is what arrived.
            let _ = writer.call(write).await;
            let _ = writer
                .with_session(SessionKey::new(other))
                .call(write)
                .await;
        }

        let requests = server.requests();
        assert_eq!(requests.len(), 20);
        for (pair, write) in requests.chunks(2).zip(&writes) {
            for (request, session) in pair.iter().zip([SESSION, other]) {
                assert_eq!(request.verb, "POST");
                assert_eq!(
                    request.form_param("method").as_deref(),
                    Some(write.method())
                );
                assert_eq!(request.form_param("sk").as_deref(), Some(session));
                assert!(request.form_param("api_sig").is_some());
            }
        }
    })
    .await;
}

#[tokio::test]
async fn gate15_a_writer_reads_through_its_client_and_shares_its_pacing() {
    bounded(async {
        let server = serve(dataset(), []).await;
        let interval = Duration::from_millis(200);
        let writer = builder(&server)
            .min_interval(interval)
            .build_writer()
            .unwrap();

        // A typed read by deref, a raw write, a raw read through the client.
        let page = writer.user(USER).recent_tracks().send().await.unwrap();
        assert_eq!(page.scrobbles().len(), 3);
        writer.call(&love()).await.unwrap();
        Client::call(&writer, &read()).await.unwrap();

        let requests = server.requests();
        assert_eq!(requests.len(), 3);
        for pair in requests.windows(2) {
            let gap = pair[1].arrived.duration_since(pair[0].arrived);
            assert!(gap + SLACK >= interval, "arrived {gap:?} apart");
        }
    })
    .await;
}

#[test]
fn gate15_a_writer_shows_no_credential() {
    let writer = builder_for("http://127.0.0.1:9").build_writer().unwrap();
    let text = format!("{writer:?} {writer:#?}");
    assert!(text.contains("Writer"), "{text}");
    for secret in [KEY, SECRET, SESSION] {
        assert!(!text.contains(secret), "{text}");
    }
}
