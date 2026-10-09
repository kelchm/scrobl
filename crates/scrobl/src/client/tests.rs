#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]

use std::time::Duration;

use reqwest::header::{HeaderMap, HeaderValue, RETRY_AFTER};
use tokio::net::TcpListener;

use super::pacing::Pacer;
use super::{MAX_WAIT, parse_retry_after, push_chunk, retry_wait};
use crate::error::ErrorKind;
use crate::secret::ApiKey;
use crate::{Client, Error};

const SENTINEL: &str = "SENTINEL_API_KEY_0001";

fn builder() -> super::ClientBuilder {
    Client::builder(ApiKey::new(SENTINEL))
}

fn config_error(builder: super::ClientBuilder) -> Error {
    let error = builder.build().unwrap_err();
    assert_eq!(error.kind(), ErrorKind::Config);
    assert_eq!(error.delivery(), None);
    for text in [format!("{error}"), format!("{error:?}")] {
        assert!(!text.contains(SENTINEL), "{text}");
    }
    error
}

#[test]
fn defaults_are_the_documented_ones() {
    let client = builder().build().unwrap();
    let settings = &client.shared.settings;
    assert_eq!(
        settings.user_agent,
        format!("scrobl/{}", env!("CARGO_PKG_VERSION"))
    );
    assert_eq!(settings.connect_timeout, Duration::from_secs(10));
    assert_eq!(settings.timeout, Duration::from_secs(30));
    assert_eq!(settings.max_response_bytes, 8 * 1024 * 1024);
    assert_eq!(settings.min_interval, Duration::from_secs(1));
    assert_eq!(settings.read_attempts, 3);
    assert_eq!(settings.retry_delay, Duration::from_secs(1));
    assert!(client.shared.root.is_none());
    assert!(!settings.no_proxy);
}

#[test]
fn the_builder_accepts_no_proxy_and_remembers_it() {
    let client = builder().no_proxy().build().unwrap();
    assert!(client.shared.settings.no_proxy);
    // Alongside a root, which already ignores the environment.
    builder()
        .no_proxy()
        .base_url("http://127.0.0.1:9/")
        .build()
        .unwrap();
}

#[test]
fn a_bad_user_agent_is_a_config_error() {
    for bad in ["", "   ", "line\nbreak", "tab\there", "caf\u{e9}", "\u{7f}"] {
        let error = config_error(builder().user_agent(bad));
        assert!(error.to_string().contains("user agent"), "{error}");
    }
    builder()
        .user_agent("my-app/1.0 (+https://example.org)")
        .build()
        .unwrap();
}

#[test]
fn a_zero_setting_that_cannot_work_is_a_config_error() {
    config_error(builder().connect_timeout(Duration::ZERO));
    config_error(builder().timeout(Duration::ZERO));
    config_error(builder().max_response_bytes(0));
    config_error(builder().read_attempts(0));
    config_error(builder().min_interval(Duration::MAX));
    config_error(builder().min_interval(Duration::from_secs(25 * 60 * 60)));
    // These zeros mean something.
    builder()
        .min_interval(Duration::ZERO)
        .retry_delay(Duration::ZERO)
        .read_attempts(1)
        .build()
        .unwrap();
}

#[test]
fn a_day_is_the_longest_pacing_interval() {
    builder()
        .min_interval(Duration::from_secs(24 * 60 * 60))
        .build()
        .unwrap();
}

#[test]
fn a_bad_base_url_is_a_config_error() {
    config_error(builder().base_url("not a url"));
}

#[test]
fn a_base_url_must_be_loopback_and_plain() {
    // Anything that could carry the key somewhere else is refused, and the
    // message never repeats what was given.
    let hostile = [
        "https://ws.audioscrobbler.com/2.0/",
        "http://example.com/2.0/",
        "http://127.0.0.1.example.com/2.0/",
        "http://localhost.example.com/2.0/",
        "http://notlocalhost/2.0/",
        "http://localhost:9/2.0/",
        "http://LOCALHOST/2.0/",
        "http://10.0.0.1/2.0/",
        "http://0.0.0.0/2.0/",
        "http://[::ffff:127.0.0.1]/2.0/",
        "http://[::2]/2.0/",
        "ftp://127.0.0.1/2.0/",
        "file:///tmp/2.0/",
        "http://user:pw@127.0.0.1:9/2.0/",
        "http://user@127.0.0.1:9/2.0/",
        "http://127.0.0.1:9/2.0/?token=SENTINEL_QUERY",
        "http://127.0.0.1:9/2.0/#SENTINEL_FRAGMENT",
        "http://127.0.0.1:9/2.0/?",
        "http://127.0.0.1:9/2.0/#",
    ];
    for url in hostile {
        let error = config_error(builder().base_url(url));
        for text in [format!("{error}"), format!("{error:?}")] {
            for piece in [
                "audioscrobbler",
                "example.com",
                "10.0.0.1",
                "127.0.0.1",
                "SENTINEL_QUERY",
                "SENTINEL_FRAGMENT",
                "user",
                "pw",
            ] {
                assert!(!text.contains(piece), "{url}: `{piece}` in {text}");
            }
        }
    }
}

#[test]
fn a_loopback_base_url_is_accepted() {
    for url in [
        "http://127.0.0.1:9/2.0/",
        "http://127.8.9.10:9/",
        "http://[::1]:9/2.0/",
        "https://127.0.0.1:9/2.0/",
        "https://[::1]/",
    ] {
        let client = builder().base_url(url).build();
        assert!(client.is_ok(), "{url}: {:?}", client.err());
    }
}

#[test]
fn debug_says_whether_a_base_url_is_set_and_never_shows_it() {
    let url = "http://127.0.0.1:54321/SENTINEL_PATH/";
    let with = builder().base_url(url);
    let without = builder();
    let client = with.clone().build().unwrap();
    for shown in [
        format!("{with:?}"),
        format!("{with:#?}"),
        format!("{client:?}"),
        format!("{client:#?}"),
    ] {
        for piece in ["SENTINEL_PATH", "54321", "127.0.0.1"] {
            assert!(!shown.contains(piece), "`{piece}` in {shown}");
        }
        assert!(shown.contains("base_url: true"), "{shown}");
    }
    assert!(format!("{without:?}").contains("base_url: false"));
    assert!(format!("{:?}", builder().build().unwrap()).contains("base_url: false"));
}

#[tokio::test]
async fn the_real_service_is_reached_over_https_only() {
    // Without a base URL the underlying client refuses a plain-HTTP URL
    // before it connects.
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}/", listener.local_addr().unwrap());
    let client = builder().build().unwrap();

    let error = client.shared.http.get(&url).send().await.unwrap_err();
    assert!(error.is_builder(), "{error:?}");

    let accepted = tokio::time::timeout(Duration::from_millis(100), listener.accept()).await;
    assert!(accepted.is_err(), "the client connected over plain HTTP");
}

/// Admits `callers` waiters in turn and returns when each was admitted.
async fn admissions(pacer: &std::sync::Arc<Pacer>, callers: usize) -> Vec<tokio::time::Instant> {
    let mut waiters = tokio::task::JoinSet::new();
    for _ in 0..callers {
        let pacer = std::sync::Arc::clone(pacer);
        waiters.spawn(async move { pacer.wait().await });
    }
    let mut at = Vec::new();
    while let Some(admitted) = waiters.join_next().await {
        at.push(admitted.unwrap());
    }
    at.sort();
    at
}

#[tokio::test]
async fn pacing_admissions_are_one_interval_apart() {
    let interval = Duration::from_millis(40);
    let pacer = std::sync::Arc::new(Pacer::new(interval));
    let at = admissions(&pacer, 5).await;
    for pair in at.windows(2) {
        assert!(pair[1] - pair[0] >= interval, "{:?}", pair[1] - pair[0]);
    }
}

#[test]
fn pacing_admissions_stay_apart_however_late_the_waiters_are_polled() {
    let interval = Duration::from_millis(50);
    let pacer = std::sync::Arc::new(Pacer::new(interval));
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    let at = runtime.block_on(async {
        let mut waiters = tokio::task::JoinSet::new();
        for _ in 0..4 {
            let pacer = std::sync::Arc::clone(&pacer);
            waiters.spawn(async move { pacer.wait().await });
        }
        // The first is admitted; the rest queue. Then every one of their
        // times comes and goes while nothing is polled.
        tokio::task::yield_now().await;
        std::thread::sleep(6 * interval);
        let mut at = Vec::new();
        while let Some(admitted) = waiters.join_next().await {
            at.push(admitted.unwrap());
        }
        at.sort();
        at
    });
    for pair in at.windows(2) {
        assert!(pair[1] - pair[0] >= interval, "{:?}", pair[1] - pair[0]);
    }
}

#[tokio::test]
async fn pacing_a_waiter_that_is_dropped_leaves_no_debt() {
    let interval = Duration::from_millis(100);
    let pacer = std::sync::Arc::new(Pacer::new(interval));
    let first = pacer.wait().await;

    let mut waiters = Vec::new();
    for _ in 0..40 {
        let pacer = std::sync::Arc::clone(&pacer);
        waiters.push(tokio::spawn(async move { pacer.wait().await }));
    }
    // Every waiter runs up to the pacer before this task runs again.
    tokio::task::yield_now().await;
    tokio::task::yield_now().await;
    let last = waiters.pop().unwrap();
    for waiter in waiters {
        waiter.abort();
    }

    let admitted = last.await.unwrap();
    assert!(admitted - first >= interval);
    assert!(
        admitted - first < 20 * interval,
        "{:?}: the dropped waiters left their slots spent",
        admitted - first
    );
}

#[tokio::test]
async fn pacing_a_dropped_waiter_does_not_reset_the_wait_of_the_next() {
    let interval = Duration::from_millis(120);
    let pacer = Pacer::new(interval);
    let first = pacer.wait().await;

    // Gives up part-way through its wait.
    let gave_up = tokio::time::timeout(Duration::from_millis(40), pacer.wait()).await;
    assert!(gave_up.is_err());
    let second = pacer.wait().await;
    // Still measured from the last admission, not from the abandoned wait.
    assert!(second - first >= interval);
}

#[tokio::test]
async fn pacing_an_enormous_interval_cannot_overflow() {
    for interval in [Duration::MAX, Duration::from_secs(u64::MAX / 4)] {
        let pacer = Pacer::new(interval);
        // The first call is admitted at once; the next waits, without
        // panicking, for as long as it is polled.
        pacer.wait().await;
        let second = tokio::time::timeout(Duration::from_millis(30), pacer.wait()).await;
        assert!(second.is_err());
    }
}

#[tokio::test]
async fn pacing_off_never_waits() {
    let pacer = Pacer::new(Duration::ZERO);
    tokio::time::timeout(Duration::from_secs(5), async {
        for _ in 0..1_000 {
            pacer.wait().await;
        }
    })
    .await
    .unwrap();
}

#[test]
fn retry_waits_double_and_rate_limits_wait_five_times_as_long() {
    let base = Duration::from_secs(2);
    assert_eq!(retry_wait(base, 1, 1, None), Duration::from_secs(2));
    assert_eq!(retry_wait(base, 2, 1, None), Duration::from_secs(4));
    assert_eq!(retry_wait(base, 3, 1, None), Duration::from_secs(8));
    assert_eq!(retry_wait(base, 1, 5, None), Duration::from_secs(10));
    assert_eq!(retry_wait(base, 2, 5, None), Duration::from_secs(20));
}

#[test]
fn a_retry_after_longer_than_the_backoff_wins_and_a_shorter_one_does_not() {
    let base = Duration::from_secs(1);
    assert_eq!(
        retry_wait(base, 1, 1, Some(Duration::from_secs(30))),
        Duration::from_secs(30)
    );
    assert_eq!(
        retry_wait(base, 3, 1, Some(Duration::from_secs(2))),
        Duration::from_secs(4)
    );
}

#[test]
fn no_wait_passes_five_minutes_and_huge_inputs_do_not_overflow() {
    let base = Duration::from_secs(1);
    assert_eq!(retry_wait(base, 40, 5, None), MAX_WAIT);
    assert_eq!(retry_wait(base, u32::MAX, u32::MAX, None), MAX_WAIT);
    assert_eq!(
        retry_wait(base, 1, 1, Some(Duration::from_secs(86_400))),
        MAX_WAIT
    );
    assert_eq!(
        retry_wait(Duration::MAX, 5, 5, None),
        MAX_WAIT,
        "saturates instead of panicking"
    );
}

#[test]
fn the_backoff_keeps_doubling_up_to_the_cap_whatever_the_base() {
    for base in [
        Duration::from_nanos(1),
        Duration::from_millis(1),
        Duration::from_secs(1),
    ] {
        let mut expected = base;
        for attempt in 1..=80 {
            assert_eq!(
                retry_wait(base, attempt, 1, None),
                expected.min(MAX_WAIT),
                "base {base:?}, attempt {attempt}"
            );
            assert_eq!(
                retry_wait(base, attempt, 5, None),
                expected.saturating_mul(5).min(MAX_WAIT),
                "base {base:?}, attempt {attempt}, rate limited"
            );
            expected = expected.saturating_mul(2);
        }
        assert_eq!(retry_wait(base, 80, 1, None), MAX_WAIT, "{base:?}");
    }
    assert_eq!(
        retry_wait(Duration::ZERO, u32::MAX, 5, None),
        Duration::ZERO
    );
}

#[test]
fn the_body_buffer_is_never_asked_to_grow_past_the_cap() {
    // Caps that are not powers of two, where doubling would overshoot, and
    // chunks of every awkward size.
    for limit in [1, 7, 1_000, 65_537, 100_000, 8 * 1024 * 1024 + 1] {
        for size in [1, 3, 997, 16_384] {
            let chunk = vec![b'x'; size];
            let mut body = Vec::with_capacity(limit.min(64));
            while push_chunk(&mut body, &chunk, limit) {
                assert!(body.len() <= limit);
                // The allocator may round a request up, but the client never
                // asks for more than the cap.
                assert!(
                    body.capacity() <= limit.max(64),
                    "limit {limit}, chunks of {size}: capacity {}",
                    body.capacity()
                );
            }
            // The first chunk that does not fit is refused and changes
            // nothing.
            let before = body.clone();
            assert!(!push_chunk(&mut body, &chunk, limit));
            assert_eq!(body, before);
            assert!(limit - body.len() < size);
        }
    }
}

#[test]
fn a_body_of_exactly_the_cap_fits_and_one_more_byte_does_not() {
    let mut body = Vec::new();
    assert!(push_chunk(&mut body, &[0; 40], 100));
    assert!(push_chunk(&mut body, &[0; 60], 100));
    assert_eq!(body.len(), 100);
    assert!(!push_chunk(&mut body, &[0], 100));
    assert!(push_chunk(&mut body, &[], 100));
    assert_eq!(body.len(), 100);
}

#[test]
fn retry_after_is_read_as_seconds_and_capped() {
    let header = |value: &str| {
        let mut headers = HeaderMap::new();
        headers.insert(RETRY_AFTER, HeaderValue::from_str(value).unwrap());
        parse_retry_after(&headers)
    };
    assert_eq!(header("5"), Some(Duration::from_secs(5)));
    assert_eq!(header(" 7 "), Some(Duration::from_secs(7)));
    assert_eq!(header("0"), Some(Duration::ZERO));
    assert_eq!(header("99999"), Some(MAX_WAIT));
    assert_eq!(header("18446744073709551615"), Some(MAX_WAIT));
    for ignored in ["", "-1", "1.5", "soon", "Wed, 21 Oct 2026 07:28:00 GMT"] {
        assert_eq!(header(ignored), None, "{ignored:?}");
    }
    assert_eq!(parse_retry_after(&HeaderMap::new()), None);
}
