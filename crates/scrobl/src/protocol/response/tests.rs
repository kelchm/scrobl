#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]

use proptest::prelude::*;
use serde::Deserialize;

use super::*;
use crate::protocol::testing::*;
use crate::{ApiErrorCode, Delivery, ErrorKind, Retry};

fn read() -> Request {
    Request::new(&READ).param("user", "rj")
}

fn write() -> Request {
    Request::new(&NOW_PLAYING)
        .param("artist", "a")
        .param("track", "t")
}

fn response(status: u16, body: &[u8]) -> HttpResponse {
    HttpResponse::new(status, Bytes::copy_from_slice(body))
}

fn decoded(status: u16, body: &[u8]) -> Result<Raw, Error> {
    decode(&read(), response(status, body))
}

// Fixtures. Provenance for each:
//
// - NOT_FOUND and LOGIN_REQUIRED: derived from the example error responses in
//   docs_user_getRecentTracks.md (community documentation), with the
//   statuses that page gives (404 and 403).
// - Everything else: synthetic.
const NOT_FOUND: &[u8] = br#"{"error": 6, "message": "User not found"}"#;
const LOGIN_REQUIRED: &[u8] =
    br#"{"error": 17, "message": "Login: User required to be logged in"}"#;
const OK_BODY: &[u8] = br#"{"user":{"name":"rj","playcount":"42"}}"#;
const HTML_500: &[u8] = b"<html><head><title>500 Internal Server Error</title></head>\
<body><h1>Internal Server Error</h1></body></html>";

#[test]
fn an_envelope_with_an_integer_code_is_an_api_error() {
    let error = decoded(404, NOT_FOUND).unwrap_err();
    assert_eq!(error.kind(), ErrorKind::Api);
    assert_eq!(error.api_code(), Some(ApiErrorCode::INVALID_PARAMETERS));
    assert_eq!(error.api_message(), Some("User not found"));
    assert_eq!(error.http_status(), Some(404));
    assert_eq!(error.method(), Some("user.getInfo"));
    assert_eq!(error.body(), Some(NOT_FOUND));

    let error = decoded(403, LOGIN_REQUIRED).unwrap_err();
    assert_eq!(error.api_code(), Some(ApiErrorCode::LOGIN_REQUIRED));
    assert_eq!(error.http_status(), Some(403));
}

#[test]
fn an_envelope_with_a_string_code_is_an_api_error() {
    let error = decoded(200, br#"{"error":"6","message":"User not found"}"#).unwrap_err();
    assert_eq!(error.kind(), ErrorKind::Api);
    assert_eq!(error.api_code(), Some(ApiErrorCode::INVALID_PARAMETERS));
}

#[test]
fn an_envelope_is_found_at_every_status() {
    for status in [200, 201, 204, 301, 400, 401, 403, 404, 429, 500, 502, 503] {
        let error = decoded(status, NOT_FOUND).unwrap_err();
        assert_eq!(error.kind(), ErrorKind::Api, "status {status}");
        assert_eq!(error.http_status(), Some(status));
        assert_eq!(error.api_code(), Some(ApiErrorCode::new(6)));
    }
}

#[test]
fn http_200_carrying_an_error_is_an_error() {
    let error = decoded(200, br#"{"message":"Rate Limit Exceeded","error":29}"#).unwrap_err();
    assert_eq!(error.api_code(), Some(ApiErrorCode::RATE_LIMIT_EXCEEDED));
}

#[test]
fn an_envelope_without_a_message_is_still_an_error() {
    let error = decoded(500, br#"{"error": 16}"#).unwrap_err();
    assert_eq!(error.kind(), ErrorKind::Api);
    assert_eq!(error.api_message(), None);
}

#[test]
fn an_envelope_with_a_non_string_message_keeps_the_code() {
    let error = decoded(200, br#"{"error": 8, "message": {"nested": true}}"#).unwrap_err();
    assert_eq!(error.api_code(), Some(ApiErrorCode::OPERATION_FAILED));
    assert_eq!(error.api_message(), None);
}

#[test]
fn unknown_codes_are_preserved() {
    let error = decoded(200, br#"{"error": 9999, "message": "New"}"#).unwrap_err();
    assert_eq!(error.api_code().map(ApiErrorCode::get), Some(9999));
    assert_eq!(error.kind(), ErrorKind::Api);
}

#[test]
fn error_members_that_are_not_codes_are_not_envelopes() {
    let not_codes: [&[u8]; 11] = [
        br#"{"error": "boom"}"#,
        br#"{"error": ""}"#,
        br#"{"error": "+6"}"#,
        br#"{"error": " 6"}"#,
        br#"{"error": "6.0"}"#,
        br#"{"error": -1}"#,
        br#"{"error": 6.5}"#,
        br#"{"error": 4294967296}"#,
        br#"{"error": true}"#,
        br#"{"error": null}"#,
        br#"{"error": {"code": 6}}"#,
    ];
    for body in not_codes {
        // A 2xx without an envelope is a successful payload...
        assert!(
            decoded(200, body).is_ok(),
            "{}",
            String::from_utf8_lossy(body)
        );
        // ...and a non-2xx without one is an HTTP error.
        let error = decoded(500, body).unwrap_err();
        assert_eq!(error.kind(), ErrorKind::Http);
    }
}

#[test]
fn a_nested_error_key_is_not_an_envelope() {
    let bodies: [&[u8]; 4] = [
        br#"{"track":{"name":"error","error":6,"message":"x"}}"#,
        br#"{"results":[{"error":6},{"error":"7"}]}"#,
        br#"[{"error":6,"message":"x"}]"#,
        br#"{"recenttracks":{"@attr":{"error":"6"},"track":[]}}"#,
    ];
    for body in bodies {
        let raw = decoded(200, body).unwrap();
        assert_eq!(raw.body(), body);
    }
}

#[test]
fn only_a_top_level_object_can_be_an_envelope() {
    // Arrays, scalars and strings are JSON, but not envelopes.
    for body in [&b"[6,\"x\"]"[..], b"6", b"\"error\"", b"null", b"true"] {
        assert!(decoded(200, body).is_ok());
        assert_eq!(decoded(502, body).unwrap_err().kind(), ErrorKind::Http);
    }
}

#[test]
fn the_error_key_is_case_sensitive() {
    assert!(decoded(200, br#"{"Error": 6, "ERROR": 6}"#).is_ok());
}

#[test]
fn a_non_2xx_without_an_envelope_is_an_http_error() {
    for status in [100, 301, 400, 404, 429, 500, 503] {
        let error = decoded(status, HTML_500).unwrap_err();
        assert_eq!(error.kind(), ErrorKind::Http, "status {status}");
        assert_eq!(error.http_status(), Some(status));
        assert_eq!(error.api_code(), None);
        assert_eq!(error.body(), Some(HTML_500));
    }
}

#[test]
fn a_non_2xx_with_valid_json_but_no_envelope_is_an_http_error() {
    let error = decoded(404, br#"{"message":"nope"}"#).unwrap_err();
    assert_eq!(error.kind(), ErrorKind::Http);
}

#[test]
fn an_empty_non_2xx_body_is_an_http_error() {
    assert_eq!(decoded(502, b"").unwrap_err().kind(), ErrorKind::Http);
}

#[test]
fn an_empty_2xx_body_is_a_decode_error() {
    let error = decoded(200, b"").unwrap_err();
    assert_eq!(error.kind(), ErrorKind::Decode);
    assert_eq!(error.http_status(), Some(200));
    assert_eq!(error.method(), Some("user.getInfo"));
    assert_eq!(error.body(), Some(&b""[..]));
}

#[test]
fn truncated_json_is_a_decode_error() {
    for body in [
        &br#"{"user":{"name":"rj""#[..],
        br#"{"error": 6, "message": "User no"#,
        br#"{"error": 6"#,
        b"{",
        b"[1,2",
    ] {
        let error = decoded(200, body).unwrap_err();
        assert_eq!(error.kind(), ErrorKind::Decode, "{body:?}");
    }
}

#[test]
fn invalid_utf8_is_a_decode_error() {
    for body in [
        &b"{\"user\":\"\xff\xfe\"}"[..],
        b"\xff",
        b"{\"error\": 6, \"message\": \"\xc3\x28\"}",
    ] {
        let error = decoded(200, body).unwrap_err();
        assert_eq!(error.kind(), ErrorKind::Decode, "{body:?}");
    }
}

#[test]
fn a_byte_order_mark_or_trailing_text_is_a_decode_error() {
    for body in [
        &b"\xef\xbb\xbf{}"[..],
        b"{} trailing",
        b"{}{}",
        b"<html></html>",
        b"OK",
    ] {
        let error = decoded(200, body).unwrap_err();
        assert_eq!(error.kind(), ErrorKind::Decode, "{body:?}");
    }
}

#[test]
fn very_deep_json_does_not_overflow_the_stack() {
    let deep = format!("{}{}", "[".repeat(100_000), "]".repeat(100_000));
    // It is valid JSON, so it is a success; typed decoding has its own depth
    // limit and reports it as an error rather than overflowing.
    let raw = decoded(200, deep.as_bytes()).unwrap();
    assert_eq!(
        raw.json::<serde_json::Value>().unwrap_err().kind(),
        ErrorKind::Decode
    );
    assert_eq!(
        decoded(500, deep.as_bytes()).unwrap_err().kind(),
        ErrorKind::Http
    );

    let under_error = format!(
        r#"{{"error": {}{}, "message": {}"x"{}}}"#,
        "[".repeat(100_000),
        "]".repeat(100_000),
        "[".repeat(100_000),
        "]".repeat(100_000)
    );
    assert!(decoded(200, under_error.as_bytes()).is_ok());
}

#[test]
fn surrounding_whitespace_is_fine() {
    assert!(decoded(200, b"  \n{\"a\":1}\r\n ").is_ok());
    let error = decoded(200, b"\n {\"error\":6}\n").unwrap_err();
    assert_eq!(error.kind(), ErrorKind::Api);
}

#[test]
fn success_keeps_the_exact_bytes() {
    // Odd spacing, key order and escapes survive untouched.
    let body = b"{ \"b\" : 1,\n\"a\":\"caf\\u00e9\" ,\"user\":{\"name\":\"rj\"} }\n";
    let raw = decoded(200, body).unwrap();
    assert_eq!(raw.body(), &body[..]);
    assert_eq!(raw.status(), 200);
    assert_eq!(raw.method(), "user.getInfo");
}

#[test]
fn any_2xx_status_is_success() {
    for status in [200, 201, 202, 204, 299] {
        assert_eq!(decoded(status, OK_BODY).unwrap().status(), status);
    }
    for status in [199, 300, 304] {
        assert!(decoded(status, OK_BODY).is_err());
    }
}

#[test]
fn headers_are_limited_to_an_allow_list() {
    let http = HttpResponse::new(200, OK_BODY)
        .with_header("Content-Type", "application/json")
        .with_header("RETRY-AFTER", "30")
        .with_header("date", "Thu, 08 Oct 2026 10:00:00 GMT")
        .with_header("Set-Cookie", "session=SENTINEL_SESSION_KEY_0001")
        .with_header("Authorization", "Bearer SENTINEL_TOKEN_0001")
        .with_header("X-Anything", "x");
    let raw = decode(&read(), http).unwrap();

    assert_eq!(raw.header("content-type"), Some("application/json"));
    assert_eq!(raw.header("Content-Type"), Some("application/json"));
    assert_eq!(raw.header("retry-after"), Some("30"));
    assert_eq!(raw.header("DATE"), Some("Thu, 08 Oct 2026 10:00:00 GMT"));
    assert_eq!(raw.header("set-cookie"), None);
    assert_eq!(raw.header("authorization"), None);
    assert_eq!(raw.header("x-anything"), None);
    assert_no_sentinel(&format!("{raw:?}"));
}

#[test]
fn a_repeated_header_keeps_the_last_value() {
    let http = HttpResponse::new(200, OK_BODY)
        .with_header("retry-after", "1")
        .with_header("Retry-After", "2");
    assert_eq!(
        decode(&read(), http).unwrap().header("retry-after"),
        Some("2")
    );
}

#[test]
fn a_long_header_value_is_cut_on_a_character_boundary() {
    let long = "é".repeat(1000);
    let http = HttpResponse::new(200, OK_BODY).with_header("content-type", &long);
    let value = decode(&read(), http).unwrap();
    let value = value.header("content-type").unwrap();
    assert_eq!(value.len(), 256);
    assert!(value.chars().all(|c| c == 'é'));
}

#[test]
fn raw_and_response_debug_leave_out_the_body() {
    let body = format!(r#"{{"session":{{"key":"{SENTINEL_SESSION_KEY}"}}}}"#);
    let http = HttpResponse::new(200, body.clone());
    assert_no_sentinel(&format!("{http:?}"));
    let raw = decode(&read(), http).unwrap();
    assert_no_sentinel(&format!("{raw:?} {raw:#?}"));
    assert!(format!("{raw:?}").contains("body_len"));
}

#[derive(Debug, Deserialize)]
struct UserReply {
    user: User,
}

#[derive(Debug, Deserialize)]
struct User {
    name: String,
    #[allow(dead_code)]
    playcount: String,
}

#[test]
fn json_deserializes_the_body() {
    let reply: UserReply = decoded(200, OK_BODY).unwrap().json().unwrap();
    assert_eq!(reply.user.name, "rj");
}

#[test]
fn json_can_keep_the_last_of_a_repeated_member_and_json_strict_refuses_it() {
    let raw = decoded(
        200,
        br#"{"user":{"name":"SENTINEL_TOKEN_0001","name":"rj","playcount":"1"}}"#,
    )
    .unwrap();
    // A map or a `Value` keeps the last; a derived struct happens to refuse.
    let loose: Value = raw.json().unwrap();
    assert_eq!(loose["user"]["name"], "rj");
    assert!(raw.json::<UserReply>().is_err());

    let error = raw.json_strict().unwrap_err();
    assert_eq!(error.kind(), ErrorKind::Decode);
    assert_eq!(error.method(), Some("user.getInfo"));
    assert_eq!(error.body(), Some(raw.body()));
    assert_no_sentinel(&format!("{error} {error:?}"));
    assert!(
        error.to_string().contains("repeats a member name"),
        "{error}"
    );

    let value = decoded(200, OK_BODY).unwrap().json_strict().unwrap();
    assert_eq!(value["user"]["name"], "rj");
}

#[test]
fn json_failures_are_decode_errors_that_do_not_quote_the_body() {
    let raw = decoded(
        200,
        br#"{"user":{"name":"SENTINEL_TOKEN_0001","playcount":[1]}}"#,
    )
    .unwrap();
    let error = raw.json::<UserReply>().unwrap_err();
    assert_eq!(error.kind(), ErrorKind::Decode);
    assert_eq!(error.http_status(), Some(200));
    assert_eq!(error.method(), Some("user.getInfo"));
    assert_eq!(error.body(), Some(raw.body()));
    assert_no_sentinel(&format!("{error} {error:?}"));
    assert!(error.to_string().contains("line 1 column"), "{error}");

    // Wrong string where a number is wanted: serde would quote it.
    let raw = decoded(200, br#"{"n":"SENTINEL_TOKEN_0001"}"#).unwrap();
    let error = raw
        .json::<std::collections::HashMap<String, u32>>()
        .unwrap_err();
    assert_no_sentinel(&format!("{error} {error:?}"));
}

#[test]
fn json_reports_a_missing_field_without_naming_it() {
    let raw = decoded(200, br#"{"user":{"name":"rj"}}"#).unwrap();
    let error = raw.json::<UserReply>().unwrap_err();
    assert_eq!(error.kind(), ErrorKind::Decode);
    assert!(!error.to_string().contains("playcount"), "{error}");
    assert!(error.to_string().contains("line 1 column"), "{error}");
}

#[test]
fn a_callers_own_missing_field_text_cannot_carry_the_body() {
    struct Echo;
    impl<'de> Deserialize<'de> for Echo {
        fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
            let value = String::deserialize(deserializer)?;
            Err(serde::de::Error::custom(format!("missing field `{value}`")))
        }
    }

    let body = format!(r#""{SENTINEL_TOKEN}""#);
    let raw = decoded(200, body.as_bytes()).unwrap();
    let Err(error) = raw.json::<Echo>() else {
        unreachable!()
    };
    assert_eq!(error.kind(), ErrorKind::Decode);
    assert_no_sentinel(&format!("{error} {error:?} {error:#?}"));
}

#[test]
fn a_repeated_error_member_is_a_decode_error_at_every_status() {
    let bodies: [&[u8]; 3] = [
        br#"{"error":6,"error":null,"message":"failed"}"#,
        br#"{"error":6,"error":16}"#,
        br#"{"error":null,"error":6}"#,
    ];
    for body in bodies {
        for status in [200, 500] {
            let error = decoded(status, body).unwrap_err();
            assert_eq!(error.kind(), ErrorKind::Decode, "{status} {body:?}");
            assert_eq!(error.api_code(), None);
            assert_eq!(error.http_status(), Some(status));
            assert_eq!(error.body(), Some(body));
            assert_eq!(error.delivery(), None);

            let error = decode(&write(), response(status, body)).unwrap_err();
            assert_eq!(error.kind(), ErrorKind::Decode, "{status} {body:?}");
            assert_eq!(error.delivery(), Some(Delivery::Unknown));
            assert_eq!(error.retry(), Retry::No);
        }
    }
}

#[test]
fn a_repeated_message_keeps_the_first_string() {
    let error = decoded(200, br#"{"error":6,"message":"first","message":"second"}"#).unwrap_err();
    assert_eq!(error.api_message(), Some("first"));

    let error = decoded(200, br#"{"error":6,"message":1,"message":"second"}"#).unwrap_err();
    assert_eq!(error.api_message(), Some("second"));
}

#[test]
fn display_and_debug_leave_out_the_message_and_body() {
    let body = format!(r#"{{"error": 6, "message": "echo {SENTINEL_TOKEN}"}}"#);
    let error = decode(&read(), HttpResponse::new(400, body.clone())).unwrap_err();

    // Display and Debug are for logs: neither carries text from the network.
    assert_no_sentinel(&format!("{error} {error:?} {error:#?}"));
    assert!(!error.to_string().contains("echo"));
    // The caller who asks for the message or the body gets them.
    assert!(error.api_message().unwrap().contains(SENTINEL_TOKEN));
    assert_eq!(error.body(), Some(body.as_bytes()));
}

#[test]
fn display_is_one_line_with_code_method_and_status() {
    let error = decoded(404, NOT_FOUND).unwrap_err();
    assert_eq!(
        error.to_string(),
        "Last.fm returned error 6 for user.getInfo (HTTP 404)"
    );
    let error = decoded(502, HTML_500).unwrap_err();
    assert_eq!(
        error.to_string(),
        "unexpected HTTP status for user.getInfo (HTTP 502)"
    );
    let error = decoded(200, b"").unwrap_err();
    assert_eq!(
        error.to_string(),
        "could not decode the response for user.getInfo (HTTP 200): the body is not valid JSON"
    );
}

#[test]
fn the_error_body_is_capped_at_64_kib() {
    let html = vec![b'x'; 200 * 1024];
    let error = decoded(500, &html).unwrap_err();
    assert_eq!(error.body().unwrap().len(), 64 * 1024);

    let mut envelope = br#"{"error": 6, "message": ""#.to_vec();
    envelope.extend(std::iter::repeat_n(b'm', 200 * 1024));
    envelope.extend_from_slice(br#""}"#);
    let error = decoded(400, &envelope).unwrap_err();
    assert_eq!(error.kind(), ErrorKind::Api);
    assert_eq!(error.body().unwrap().len(), 64 * 1024);
    assert_eq!(error.body().unwrap(), &envelope[..64 * 1024]);
    assert_eq!(error.api_message().unwrap().len(), 1024);

    let error = decoded(200, &vec![b'x'; 70_000]).unwrap_err();
    assert_eq!(error.kind(), ErrorKind::Decode);
    assert_eq!(error.body().unwrap().len(), 64 * 1024);
}

#[test]
fn a_body_at_the_cap_is_kept_whole() {
    let html = vec![b'x'; 64 * 1024];
    assert_eq!(
        decoded(500, &html).unwrap_err().body().unwrap().len(),
        64 * 1024
    );
}

#[test]
fn a_long_message_is_cut_on_a_character_boundary() {
    // Each '€' is three bytes: 1024 is not a multiple of three.
    let message = "€".repeat(2000);
    let body = format!(r#"{{"error": 8, "message": "{message}"}}"#);
    let error = decode(&read(), HttpResponse::new(500, body)).unwrap_err();
    let kept = error.api_message().unwrap();
    assert_eq!(kept.len(), 1023);
    assert!(kept.chars().all(|c| c == '€'));
}

#[test]
fn a_write_error_reports_its_delivery() {
    let rejected = decode(&write(), response(200, NOT_FOUND)).unwrap_err();
    assert_eq!(rejected.delivery(), Some(Delivery::Rejected));

    let unknown_http = decode(&write(), response(502, HTML_500)).unwrap_err();
    assert_eq!(unknown_http.delivery(), Some(Delivery::Unknown));

    let unknown_decode = decode(&write(), response(200, b"")).unwrap_err();
    assert_eq!(unknown_decode.delivery(), Some(Delivery::Unknown));

    let read_error = decoded(200, b"").unwrap_err();
    assert_eq!(read_error.delivery(), None);
}

proptest! {
    // synthetic
    #[test]
    fn arbitrary_bytes_never_panic(
        status in any::<u16>(),
        body in proptest::collection::vec(any::<u8>(), 0..512),
    ) {
        match decoded(status, &body) {
            Ok(raw) => {
                prop_assert!((200..300).contains(&status));
                prop_assert_eq!(raw.body(), &body[..]);
            }
            Err(error) => {
                prop_assert!(error.body().is_some_and(|b| b.len() <= 64 * 1024));
                let _ = format!("{error} {error:?}");
            }
        }
    }

    // synthetic: bytes that look like JSON are the likeliest to find a
    // panic in the envelope probe.
    #[test]
    fn json_like_text_never_panics(
        status in any::<u16>(),
        body in "[\\[\\]{}\":,0-9a-z\\\\ .+eE-]{0,80}",
    ) {
        let _ = decoded(status, body.as_bytes());
    }

    // synthetic
    #[test]
    fn any_u32_code_in_an_envelope_is_preserved(
        status in 100_u16..600,
        code in any::<u32>(),
        as_string in any::<bool>(),
        message in "\\PC{0,40}",
    ) {
        let code_json = if as_string { format!("\"{code}\"") } else { code.to_string() };
        let body = serde_json::json!({ "message": message }).to_string();
        let body = format!("{{\"error\":{code_json},{}", &body[1..]);
        let error = decoded(status, body.as_bytes()).unwrap_err();
        prop_assert_eq!(error.kind(), ErrorKind::Api);
        prop_assert_eq!(error.api_code().map(ApiErrorCode::get), Some(code));
        prop_assert_eq!(error.api_message(), Some(message.as_str()));
        prop_assert_eq!(error.http_status(), Some(status));
    }
}

#[test]
fn a_raw_carries_the_request_it_answers() {
    let request = Request::new(&SIGNED_GET).param("token", SENTINEL_TOKEN);
    let raw = decode(&request, response(200, br#"{"ok": true}"#)).unwrap();
    assert_eq!(raw.request(), &request);
    assert_eq!(raw.request().get("token"), Some(SENTINEL_TOKEN));
    assert_eq!(raw.method(), request.method());
    // The request is kept for comparison, not for display.
    assert!(!format!("{raw:?}").contains(SENTINEL_TOKEN));
    assert!(!format!("{:?}", raw.clone()).contains(SENTINEL_TOKEN));
}
