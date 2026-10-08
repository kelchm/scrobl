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
use super::{MAX_WAIT, parse_retry_after, retry_wait};
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
    // These zeros mean something.
    builder()
        .min_interval(Duration::ZERO)
        .retry_delay(Duration::ZERO)
        .read_attempts(1)
        .build()
        .unwrap();
}

#[test]
fn a_bad_base_url_is_a_config_error() {
    config_error(builder().base_url("not a url"));
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

#[tokio::test]
async fn pacing_slots_are_one_interval_apart_and_never_reset_by_waiting() {
    let interval = Duration::from_secs(10);
    let pacer = Pacer::new(interval);
    let first = pacer.reserve();
    let second = pacer.reserve();
    let third = pacer.reserve();
    assert_eq!(second - first, interval);
    assert_eq!(third - second, interval);
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
