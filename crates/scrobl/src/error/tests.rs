#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]

use std::collections::HashSet;
use std::io;

use proptest::prelude::*;

use super::*;
use crate::protocol::testing::*;

// Compile-time: `Error` can cross threads and be a `std::error::Error`.
const _: () = {
    const fn send_sync_static<T: Send + Sync + 'static + std::error::Error>() {}
    send_sync_static::<Error>();
    const fn copy_eq_hash<T: Copy + Eq + std::hash::Hash + std::fmt::Debug>() {}
    copy_eq_hash::<ErrorKind>();
    copy_eq_hash::<Retry>();
    copy_eq_hash::<Delivery>();
    copy_eq_hash::<ApiErrorCode>();
};

#[test]
fn the_error_stays_small() {
    assert_eq!(size_of::<Error>(), size_of::<usize>());
    assert_eq!(size_of::<Result<(), Error>>(), size_of::<usize>());
}

fn api(spec: &'static MethodSpec, status: u16, code: u32) -> Error {
    Error::api(spec, status, ApiErrorCode::new(code), Some("m"), b"{}")
}

/// One error and what it should advise for a read, a write, and
/// `track.updateNowPlaying`.
struct Row {
    label: &'static str,
    make: fn(&'static MethodSpec) -> Error,
    read: Retry,
    write: Retry,
    now_playing: Retry,
    /// For a write.
    delivery: Delivery,
}

use Delivery::{NotSent, Rejected, Unknown};
use Retry::{AfterBackoff, AfterReauthentication, Later, No};

// Rules: the Scrobbling 2.0 guide (scrobbling.txt) for codes 9, 11 and 16,
// docs_codes.md for 29 and HTTP 429, docs/design.md for the rest.
const ROWS: &[Row] = &[
    Row {
        label: "api 11",
        make: |s| api(s, 200, 11),
        read: Later,
        write: Later,
        now_playing: No,
        delivery: Rejected,
    },
    Row {
        label: "api 16",
        make: |s| api(s, 500, 16),
        read: Later,
        write: Later,
        now_playing: No,
        delivery: Rejected,
    },
    Row {
        label: "api 29",
        make: |s| api(s, 429, 29),
        read: AfterBackoff,
        write: AfterBackoff,
        now_playing: No,
        delivery: Rejected,
    },
    Row {
        label: "api 9",
        make: |s| api(s, 403, 9),
        read: AfterReauthentication,
        write: AfterReauthentication,
        now_playing: No,
        delivery: Rejected,
    },
    Row {
        label: "api 6",
        make: |s| api(s, 400, 6),
        read: No,
        write: No,
        now_playing: No,
        delivery: Rejected,
    },
    Row {
        label: "api 17",
        make: |s| api(s, 403, 17),
        read: No,
        write: No,
        now_playing: No,
        delivery: Rejected,
    },
    Row {
        label: "api 26",
        make: |s| api(s, 403, 26),
        read: No,
        write: No,
        now_playing: No,
        delivery: Rejected,
    },
    Row {
        label: "api 13",
        make: |s| api(s, 403, 13),
        read: No,
        write: No,
        now_playing: No,
        delivery: Rejected,
    },
    Row {
        label: "api 14",
        make: |s| api(s, 403, 14),
        read: No,
        write: No,
        now_playing: No,
        delivery: Rejected,
    },
    Row {
        label: "api 9999",
        make: |s| api(s, 200, 9999),
        read: No,
        write: No,
        now_playing: No,
        delivery: Rejected,
    },
    Row {
        label: "http 500",
        make: |s| Error::http(s, 500, b"<html>"),
        read: Later,
        write: No,
        now_playing: No,
        delivery: Unknown,
    },
    Row {
        label: "http 503",
        make: |s| Error::http(s, 503, b""),
        read: Later,
        write: No,
        now_playing: No,
        delivery: Unknown,
    },
    Row {
        label: "http 599",
        make: |s| Error::http(s, 599, b""),
        read: Later,
        write: No,
        now_playing: No,
        delivery: Unknown,
    },
    Row {
        label: "http 429",
        make: |s| Error::http(s, 429, b""),
        read: AfterBackoff,
        write: No,
        now_playing: No,
        delivery: Unknown,
    },
    Row {
        label: "http 400",
        make: |s| Error::http(s, 400, b""),
        read: No,
        write: No,
        now_playing: No,
        delivery: Unknown,
    },
    Row {
        label: "http 404",
        make: |s| Error::http(s, 404, b""),
        read: No,
        write: No,
        now_playing: No,
        delivery: Unknown,
    },
    Row {
        label: "http 301",
        make: |s| Error::http(s, 301, b""),
        read: No,
        write: No,
        now_playing: No,
        delivery: Unknown,
    },
    Row {
        label: "decode",
        make: |s| Error::decode("x").with_method(s).with_response(200, b""),
        read: No,
        write: No,
        now_playing: No,
        delivery: Unknown,
    },
    Row {
        label: "decode field",
        make: |s| Error::decode_field("a.b", "x").with_method(s),
        read: No,
        write: No,
        now_playing: No,
        delivery: Unknown,
    },
    Row {
        label: "inconsistent",
        make: |s| Error::inconsistent("rule").with_method(s),
        read: No,
        write: No,
        now_playing: No,
        delivery: Unknown,
    },
    Row {
        label: "transport, not sent",
        make: |s| Error::transport(false, io::Error::other("refused")).with_method(s),
        read: Later,
        write: Later,
        now_playing: No,
        delivery: NotSent,
    },
    Row {
        label: "transport, maybe sent",
        make: |s| Error::transport(true, io::Error::other("reset")).with_method(s),
        read: Later,
        write: No,
        now_playing: No,
        delivery: Unknown,
    },
    Row {
        label: "timeout",
        make: |s| Error::timeout().with_method(s),
        read: Later,
        write: No,
        now_playing: No,
        delivery: Unknown,
    },
    Row {
        label: "body too large",
        make: |s| Error::body_too_large(10).with_method(s),
        read: No,
        write: No,
        now_playing: No,
        delivery: Unknown,
    },
    Row {
        label: "invalid request",
        make: |s| Error::invalid_request(s, "x"),
        read: No,
        write: No,
        now_playing: No,
        delivery: NotSent,
    },
    Row {
        label: "read-only",
        make: Error::read_only,
        read: No,
        write: No,
        now_playing: No,
        delivery: NotSent,
    },
    Row {
        label: "config",
        make: |s| Error::config("x").with_method(s),
        read: No,
        write: No,
        now_playing: No,
        delivery: NotSent,
    },
];

#[test]
fn retry_for_a_read() {
    for row in ROWS {
        let error = (row.make)(&READ);
        assert_eq!(error.retry(), row.read, "{}", row.label);
    }
}

#[test]
fn retry_for_a_write() {
    for row in ROWS {
        let error = (row.make)(&SCROBBLE);
        assert_eq!(error.retry(), row.write, "{}", row.label);
        let error = (row.make)(&LOVE);
        assert_eq!(error.retry(), row.write, "{}", row.label);
    }
}

#[test]
fn update_now_playing_is_never_retried() {
    for row in ROWS {
        let error = (row.make)(&NOW_PLAYING);
        assert_eq!(error.retry(), row.now_playing, "{}", row.label);
        assert_eq!(error.retry(), Retry::No, "{}", row.label);
    }
}

#[test]
fn get_session_is_not_retried_once_it_may_have_been_received() {
    let spec = &crate::protocol::methods::AUTH_GET_SESSION;
    let mut refused = Vec::new();
    for row in ROWS {
        let error = (row.make)(spec);
        let possibly_received = match error.kind() {
            ErrorKind::Timeout => true,
            ErrorKind::Transport => row.delivery == Unknown,
            _ => false,
        };
        if possibly_received {
            assert_eq!(error.retry(), No, "{}", row.label);
            assert_eq!(row.read, Later, "{}: a read would be retried", row.label);
            refused.push(row.label);
        } else {
            assert_eq!(error.retry(), row.read, "{}", row.label);
        }
        // Another signed read keeps the ordinary advice.
        let token = &crate::protocol::methods::AUTH_GET_TOKEN;
        assert_eq!((row.make)(token).retry(), row.read, "{}", row.label);
    }
    assert_eq!(refused.len(), 2, "{refused:?}");
}

#[test]
fn delivery_is_none_for_a_read() {
    for row in ROWS {
        assert_eq!((row.make)(&READ).delivery(), None, "{}", row.label);
        assert_eq!((row.make)(&SIGNED_GET).delivery(), None, "{}", row.label);
    }
}

#[test]
fn delivery_for_a_write() {
    for row in ROWS {
        for spec in [&SCROBBLE, &NOW_PLAYING, &LOVE] {
            let error = (row.make)(spec);
            assert_eq!(error.delivery(), Some(row.delivery), "{}", row.label);
        }
    }
}

#[test]
fn retry_is_never_advised_when_delivery_is_unknown() {
    for row in ROWS {
        for spec in [&SCROBBLE, &NOW_PLAYING, &LOVE] {
            let error = (row.make)(spec);
            if error.delivery() == Some(Delivery::Unknown) {
                assert_eq!(error.retry(), Retry::No, "{}", row.label);
            }
        }
    }
}

#[test]
fn an_error_with_no_method_has_no_delivery() {
    let config = Error::config("bad");
    assert_eq!(config.delivery(), None);
    assert_eq!(config.retry(), Retry::No);
    assert_eq!(config.method(), None);

    let transport = Error::transport(false, io::Error::other("x"));
    assert_eq!(transport.delivery(), None);
}

#[test]
fn accessors_report_what_was_attached() {
    let error = Error::api(
        &READ,
        404,
        ApiErrorCode::INVALID_PARAMETERS,
        Some("User not found"),
        b"body",
    );
    assert_eq!(error.kind(), ErrorKind::Api);
    assert_eq!(error.api_code(), Some(ApiErrorCode::INVALID_PARAMETERS));
    assert_eq!(error.api_message(), Some("User not found"));
    assert_eq!(error.http_status(), Some(404));
    assert_eq!(error.method(), Some("user.getInfo"));
    assert_eq!(error.body(), Some(&b"body"[..]));

    let timeout = Error::timeout();
    assert_eq!(timeout.api_code(), None);
    assert_eq!(timeout.api_message(), None);
    assert_eq!(timeout.http_status(), None);
    assert_eq!(timeout.body(), None);
}

#[test]
fn the_body_is_capped_at_64_kib() {
    let big = vec![7_u8; 100_000];
    let error = Error::http(&READ, 500, &big);
    assert_eq!(error.body().unwrap(), &big[..64 * 1024]);
    let error = Error::api(&READ, 200, ApiErrorCode::new(1), None, &big);
    assert_eq!(error.body().unwrap().len(), 64 * 1024);
}

#[test]
fn api_codes_have_their_documented_numbers() {
    // Numbers from docs_codes.md and the per-method error lists; 14 from
    // docs/design.md.
    let expected = [
        (ApiErrorCode::INVALID_SERVICE, 2),
        (ApiErrorCode::INVALID_METHOD, 3),
        (ApiErrorCode::AUTHENTICATION_FAILED, 4),
        (ApiErrorCode::INVALID_FORMAT, 5),
        (ApiErrorCode::INVALID_PARAMETERS, 6),
        (ApiErrorCode::INVALID_RESOURCE, 7),
        (ApiErrorCode::OPERATION_FAILED, 8),
        (ApiErrorCode::INVALID_SESSION_KEY, 9),
        (ApiErrorCode::INVALID_API_KEY, 10),
        (ApiErrorCode::SERVICE_OFFLINE, 11),
        (ApiErrorCode::INVALID_SIGNATURE, 13),
        (ApiErrorCode::UNAUTHORIZED_TOKEN, 14),
        (ApiErrorCode::TEMPORARY_ERROR, 16),
        (ApiErrorCode::LOGIN_REQUIRED, 17),
        (ApiErrorCode::SUSPENDED_KEY, 26),
        (ApiErrorCode::RATE_LIMIT_EXCEEDED, 29),
    ];
    let mut seen = HashSet::new();
    for (code, number) in expected {
        assert_eq!(code.get(), number);
        assert_eq!(code, ApiErrorCode::new(number));
        assert!(seen.insert(number));
    }
}

#[test]
fn constants_work_in_patterns_and_unknown_codes_survive() {
    let error = Error::api(&READ, 403, ApiErrorCode::new(17), None, b"");
    assert!(matches!(
        error.api_code(),
        Some(ApiErrorCode::LOGIN_REQUIRED)
    ));
    let unknown = ApiErrorCode::new(4242);
    assert_eq!(unknown.get(), 4242);
    assert_ne!(unknown, ApiErrorCode::LOGIN_REQUIRED);
}

#[test]
fn display_is_one_line_and_names_the_kind() {
    for row in ROWS {
        let text = (row.make)(&SCROBBLE).to_string();
        assert!(!text.is_empty(), "{}", row.label);
        assert!(!text.contains('\n'), "{}: {text}", row.label);
        assert!(text.len() < 200, "{}: {text}", row.label);
        assert!(!text.contains("http"), "{}: {text}", row.label);
    }
    assert_eq!(
        Error::timeout().with_method(&READ).to_string(),
        "the request timed out for user.getInfo"
    );
    assert_eq!(
        Error::read_only(&LOVE).to_string(),
        "refused a write for track.love: writes are not allowed; use a `Writer`, or `Credentials::allow_writes`"
    );
    assert_eq!(
        Error::body_too_large(8_388_608).to_string(),
        "the response body was too large: limit is 8388608 bytes"
    );
}

#[test]
fn display_and_debug_never_contain_secrets_or_body_text() {
    let body = format!("echo {SENTINEL_API_KEY} {SENTINEL_SESSION_KEY}");
    let url_like = format!("https://ws.audioscrobbler.com/2.0/?api_key={SENTINEL_API_KEY}");
    let errors = [
        Error::api(
            &READ,
            400,
            ApiErrorCode::new(6),
            Some(&body),
            body.as_bytes(),
        ),
        Error::http(&READ, 502, body.as_bytes()),
        Error::timeout().with_method(&READ),
        Error::config("no user agent").with_method(&READ),
        // The source's own text is the caller's responsibility, but the
        // error's `Display` and `Debug` do not repeat it.
        Error::transport(true, io::Error::other(url_like)).with_method(&READ),
    ];
    for error in errors {
        assert_no_sentinel(&format!("{error} {error:?} {error:#?}"));
    }
}

#[test]
fn only_a_transport_error_has_a_source() {
    let transport = Error::transport(false, io::Error::other("refused"));
    let source = std::error::Error::source(&transport).expect("a source");
    assert_eq!(source.to_string(), "refused");

    for error in [
        Error::timeout(),
        Error::http(&READ, 500, b""),
        Error::decode("x"),
        Error::config("x"),
        Error::inconsistent("x"),
        Error::body_too_large(1),
    ] {
        assert!(std::error::Error::source(&error).is_none());
    }
}

#[test]
fn diagnostics_are_bounded() {
    let long = "é".repeat(1000);
    let error = Error::inconsistent(&long);
    let text = error.to_string();
    // The prefix, then at most 256 bytes of the rule.
    assert!(text.len() < 256 + 64, "{}", text.len());
    assert!(text.ends_with('é'));

    let error = Error::invalid_request(&READ, &long);
    assert!(error.to_string().len() < 256 + 64);
}

#[test]
fn a_field_path_is_part_of_a_decode_error() {
    let error = Error::decode_field("recenttracks.@attr.total", "expected a count");
    assert_eq!(error.kind(), ErrorKind::Decode);
    assert!(error.to_string().contains("`recenttracks.@attr.total`"));
}

#[test]
fn truncation_examples() {
    assert_eq!(truncate_utf8("hello", 10), "hello");
    assert_eq!(truncate_utf8("hello", 5), "hello");
    assert_eq!(truncate_utf8("hello", 3), "hel");
    assert_eq!(truncate_utf8("hello", 0), "");
    assert_eq!(truncate_utf8("", 0), "");
    // 'é' is two bytes, '€' three, '🎵' four.
    assert_eq!(truncate_utf8("aé", 2), "a");
    assert_eq!(truncate_utf8("aé", 3), "aé");
    assert_eq!(truncate_utf8("€", 2), "");
    assert_eq!(truncate_utf8("a€", 3), "a");
    assert_eq!(truncate_utf8("🎵🎵", 7), "🎵");
    assert_eq!(truncate_utf8("🎵", 3), "");
    assert_eq!(truncate_utf8("e\u{301}x", 2), "e");
}

proptest! {
    // synthetic
    #[test]
    fn truncation_is_a_valid_maximal_prefix(s in "\\PC{0,64}|[a-zé€🎵\u{301}]{0,40}", max in 0_usize..160) {
        let cut = truncate_utf8(&s, max);
        // A &str is valid UTF-8 by construction; it must also be a prefix
        // no longer than `max`.
        prop_assert!(s.starts_with(cut));
        prop_assert!(cut.len() <= max);
        if cut.len() < s.len() {
            // Maximal: the next character would not have fit.
            let next = s[cut.len()..].chars().next().unwrap();
            prop_assert!(cut.len() + next.len_utf8() > max);
        } else {
            prop_assert_eq!(cut, s.as_str());
        }
    }

    // synthetic: the same, through the paths that use it.
    #[test]
    fn long_messages_and_details_never_panic(s in "\\PC{0,3000}") {
        let error = Error::api(&READ, 400, ApiErrorCode::new(6), Some(&s), s.as_bytes());
        prop_assert!(error.api_message().unwrap().len() <= 1024);
        let error = Error::inconsistent(&s);
        let _ = error.to_string();
    }
}
