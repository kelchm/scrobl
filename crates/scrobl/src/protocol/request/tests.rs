#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]

use std::collections::BTreeMap;

use md5::{Digest, Md5};
use proptest::prelude::*;

use super::*;
use crate::protocol::Raw;
use crate::protocol::testing::*;
use crate::{Delivery, ErrorKind};

/// Decoded name/value pairs of a form-encoded string, in wire order.
fn pairs(encoded: &str) -> Vec<(String, String)> {
    form_urlencoded::parse(encoded.as_bytes())
        .map(|(n, v)| (n.into_owned(), v.into_owned()))
        .collect()
}

fn query_of(http: &HttpRequest) -> Vec<(String, String)> {
    pairs(http.url().split_once('?').expect("a query").1)
}

fn body_of(http: &HttpRequest) -> Vec<(String, String)> {
    pairs(std::str::from_utf8(http.body().expect("a body")).unwrap())
}

fn get<'a>(params: &'a [(String, String)], name: &str) -> Option<&'a str> {
    params
        .iter()
        .find(|(n, _)| n == name)
        .map(|(_, v)| v.as_str())
}

/// The signature computed again, without `sign`: sort by name, join, hash.
/// Independent of the code under test except for the `md5` crate; the
/// digests in `sign`'s own tests come from the system `md5` tool.
fn independent_signature(params: &[(String, String)], secret: &str) -> String {
    let sorted: BTreeMap<&str, &str> = params
        .iter()
        .filter(|(n, _)| !["format", "callback", "api_sig"].contains(&n.as_str()))
        .map(|(n, v)| (n.as_str(), v.as_str()))
        .collect();
    let mut text = String::new();
    for (name, value) in sorted {
        text.push_str(name);
        text.push_str(value);
    }
    text.push_str(secret);
    Md5::digest(text.as_bytes())
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}

fn invalid(result: Result<HttpRequest, Error>) -> Error {
    let error = result.expect_err("the request should be rejected");
    assert_eq!(error.kind(), crate::ErrorKind::InvalidRequest);
    error
}

// Compile-time: the plain-data types can move between threads.
const _: () = {
    const fn send_sync<T: Send + Sync>() {}
    send_sync::<Error>();
    send_sync::<HttpRequest>();
    send_sync::<Raw>();
    send_sync::<Credentials>();
    send_sync::<Request>();
    send_sync::<crate::protocol::HttpResponse>();
};

#[test]
fn get_puts_everything_in_the_query() {
    let request = Request::new(&READ).param("user", "rj").param("limit", 200);
    let http = prepare(&credentials(), &request).unwrap();

    assert_eq!(http.verb(), Verb::Get);
    assert!(http.body().is_none());
    assert!(
        http.url()
            .starts_with("https://ws.audioscrobbler.com/2.0/?")
    );
    assert_eq!(
        query_of(&http),
        [
            ("method", "user.getInfo"),
            ("api_key", SENTINEL_API_KEY),
            ("user", "rj"),
            ("limit", "200"),
            ("format", "json"),
        ]
        .map(|(n, v)| (n.to_owned(), v.to_owned()))
    );
}

#[test]
fn a_key_only_method_is_not_signed() {
    let request = Request::new(&READ).param("user", "rj");
    let http = prepare(&credentials(), &request).unwrap();
    assert!(get(&query_of(&http), "api_sig").is_none());
    assert!(get(&query_of(&http), "sk").is_none());
}

#[test]
fn a_key_only_method_needs_no_secret() {
    let credentials = Credentials::new(ApiKey::new("k"));
    let request = Request::new(&READ).param("user", "rj");
    assert!(prepare(&credentials, &request).is_ok());
}

#[test]
fn post_puts_everything_in_the_body() {
    let request = Request::new(&SCROBBLE)
        .indexed("artist", 0, "Björk")
        .indexed("track", 0, "Jóga")
        .indexed("timestamp", 0, 1_700_000_000_u64);
    let http = prepare(&credentials(), &request).unwrap();

    assert_eq!(http.verb(), Verb::Post);
    assert_eq!(http.url(), "https://ws.audioscrobbler.com/2.0/");
    assert_eq!(
        HttpRequest::CONTENT_TYPE,
        "application/x-www-form-urlencoded"
    );

    let body = body_of(&http);
    let names: Vec<_> = body.iter().map(|(n, _)| n.as_str()).collect();
    assert_eq!(
        names,
        [
            "method",
            "api_key",
            "artist[0]",
            "track[0]",
            "timestamp[0]",
            "sk",
            "api_sig",
            "format"
        ]
    );
    assert_eq!(get(&body, "method"), Some("track.scrobble"));
    assert_eq!(get(&body, "artist[0]"), Some("Björk"));
    assert_eq!(get(&body, "sk"), Some(SENTINEL_SESSION_KEY));
    assert_eq!(get(&body, "format"), Some("json"));
}

#[test]
fn the_body_is_ascii_form_encoding() {
    let request = Request::new(&NOW_PLAYING)
        .param("artist", "Björk")
        .param("track", "a b");
    let http = prepare(&credentials(), &request).unwrap();
    let body = std::str::from_utf8(http.body().unwrap()).unwrap();
    assert!(body.is_ascii());
    assert!(body.contains("artist=Bj%C3%B6rk"));
    assert!(body.contains("track=a+b"));
}

#[test]
fn format_json_is_always_present() {
    let cases = [
        Request::new(&READ).param("user", "rj"),
        Request::new(&SIGNED_GET).param("token", "t"),
        Request::new(&NOW_PLAYING)
            .param("artist", "a")
            .param("track", "t"),
    ];
    for request in cases {
        let http = prepare(&credentials(), &request).unwrap();
        let params = match http.verb() {
            Verb::Get => query_of(&http),
            Verb::Post => body_of(&http),
        };
        assert_eq!(get(&params, "format"), Some("json"), "{request:?}");
    }
}

#[test]
fn a_signed_get_carries_the_signature_without_a_session() {
    let request = Request::new(&SIGNED_GET).param("token", SENTINEL_TOKEN);
    let http = prepare(&credentials(), &request).unwrap();
    let query = query_of(&http);
    assert!(get(&query, "api_sig").is_some());
    assert!(get(&query, "sk").is_none());
}

#[test]
fn the_secret_is_never_sent() {
    let request = Request::new(&NOW_PLAYING)
        .param("artist", "a")
        .param("track", "t");
    let http = prepare(&credentials(), &request).unwrap();
    let wire = std::str::from_utf8(http.body().unwrap()).unwrap();
    assert!(!wire.contains(SENTINEL_API_SECRET));
}

#[test]
fn caller_order_is_preserved() {
    let request = Request::new(&READ)
        .param("zebra", "1")
        .param("alpha", "2")
        .param("user", "rj");
    let http = prepare(&credentials(), &request).unwrap();
    let names: Vec<_> = query_of(&http).into_iter().map(|(n, _)| n).collect();
    assert_eq!(
        names,
        ["method", "api_key", "zebra", "alpha", "user", "format"]
    );
}

#[test]
fn values_have_their_wire_forms() {
    let owned = String::from("r");
    let request = Request::new(&READ)
        .param("user", "rj")
        .param("a_string", String::from("s"))
        .param("a_ref", &owned)
        .param("t", true)
        .param("f", false)
        .param("i8", i8::MIN)
        .param("u64", u64::MAX)
        .param("i128", i128::MIN)
        .param("usize", 7_usize);
    let http = prepare(&credentials(), &request).unwrap();
    let query = query_of(&http);
    assert_eq!(get(&query, "a_string"), Some("s"));
    assert_eq!(get(&query, "a_ref"), Some("r"));
    assert_eq!(get(&query, "t"), Some("1"));
    assert_eq!(get(&query, "f"), Some("0"));
    assert_eq!(get(&query, "i8"), Some("-128"));
    assert_eq!(get(&query, "u64"), Some("18446744073709551615"));
    assert_eq!(get(&query, "i128"), Some(i128::MIN.to_string().as_str()));
    assert_eq!(get(&query, "usize"), Some("7"));
}

#[test]
fn indexed_names_are_percent_encoded_and_decode_back() {
    let request = Request::new(&SCROBBLE)
        .indexed("artist", 10, "a")
        .indexed("track", 10, "t")
        .indexed("timestamp", 10, 1);
    let http = prepare(&credentials(), &request).unwrap();
    let body = std::str::from_utf8(http.body().unwrap()).unwrap();
    assert!(body.contains("artist%5B10%5D=a"));
    assert_eq!(get(&body_of(&http), "artist[10]"), Some("a"));
}

#[test]
fn reserved_names_are_rejected() {
    for reserved in ["method", "api_key", "api_sig", "sk", "format", "callback"] {
        let request = Request::new(&READ)
            .param("user", "rj")
            .param(reserved, "SENTINEL_VALUE");
        let error = invalid(prepare(&credentials(), &request));
        assert!(error.to_string().contains(reserved), "{error}");
        assert!(!error.to_string().contains("SENTINEL_VALUE"));
        assert_eq!(error.method(), Some("user.getInfo"));
    }
}

#[test]
fn reserved_names_are_rejected_when_indexed() {
    let request = Request::new(&READ)
        .param("user", "rj")
        .indexed("sk", 0, "x");
    invalid(prepare(&credentials(), &request));
}

#[test]
fn an_empty_name_is_rejected() {
    let request = Request::new(&READ).param("user", "rj").param("", "x");
    invalid(prepare(&credentials(), &request));
}

#[test]
fn a_repeated_name_is_rejected() {
    let request = Request::new(&READ)
        .param("user", "rj")
        .param("limit", 1)
        .param("limit", 2);
    let error = invalid(prepare(&credentials(), &request));
    assert!(error.to_string().contains("limit"), "{error}");

    let batch = Request::new(&SCROBBLE)
        .indexed("artist", 0, "a")
        .indexed("artist", 0, "b");
    invalid(prepare(&credentials(), &batch));
}

#[test]
fn distinct_indexes_are_not_repeats() {
    let request = Request::new(&SCROBBLE)
        .indexed("artist", 0, "a")
        .indexed("artist", 1, "b")
        .indexed("track", 0, "t")
        .indexed("timestamp", 0, 1);
    assert!(prepare(&credentials(), &request).is_ok());
}

#[test]
fn a_missing_secret_is_rejected() {
    let credentials = Credentials::new(ApiKey::new("k")).with_session(SessionKey::new("s"));
    let request = Request::new(&SIGNED_GET).param("token", "t");
    let error = invalid(prepare(&credentials, &request));
    assert!(error.to_string().contains("secret"), "{error}");
}

#[test]
fn a_missing_session_is_rejected() {
    let credentials = Credentials::new(ApiKey::new("k")).with_secret(ApiSecret::new("s"));
    let request = Request::new(&NOW_PLAYING)
        .param("artist", "a")
        .param("track", "t");
    let error = invalid(prepare(&credentials, &request));
    assert!(error.to_string().contains("session"), "{error}");
}

#[test]
fn a_missing_secret_is_reported_before_a_missing_session() {
    let credentials = Credentials::new(ApiKey::new("k"));
    let request = Request::new(&NOW_PLAYING)
        .param("artist", "a")
        .param("track", "t");
    let error = invalid(prepare(&credentials, &request));
    assert!(error.to_string().contains("secret"), "{error}");
}

#[test]
fn a_session_alone_does_not_stand_in_for_the_secret() {
    let credentials = Credentials::new(ApiKey::new("k")).with_session(SessionKey::new("s"));
    let request = Request::new(&NOW_PLAYING)
        .param("artist", "a")
        .param("track", "t");
    invalid(prepare(&credentials, &request));
}

#[test]
fn a_missing_required_parameter_is_rejected() {
    let error = invalid(prepare(&credentials(), &Request::new(&READ)));
    assert!(error.to_string().contains("user"), "{error}");

    let request = Request::new(&NOW_PLAYING).param("artist", "a");
    let error = invalid(prepare(&credentials(), &request));
    assert!(error.to_string().contains("track"), "{error}");
}

#[test]
fn a_required_indexed_parameter_needs_at_least_one_index() {
    let none = Request::new(&SCROBBLE)
        .indexed("artist", 0, "a")
        .indexed("track", 0, "t");
    let error = invalid(prepare(&credentials(), &none));
    assert!(error.to_string().contains("timestamp"), "{error}");

    // An unindexed name does not stand in for an indexed one.
    let bare = Request::new(&SCROBBLE)
        .param("artist", "a")
        .indexed("track", 0, "t")
        .indexed("timestamp", 0, 1);
    invalid(prepare(&credentials(), &bare));

    // A high index alone is enough.
    let high = Request::new(&SCROBBLE)
        .indexed("artist", 49, "a")
        .indexed("track", 49, "t")
        .indexed("timestamp", 49, 1);
    assert!(prepare(&credentials(), &high).is_ok());
}

#[test]
fn a_name_that_only_starts_like_an_indexed_one_does_not_count() {
    let request = Request::new(&SCROBBLE)
        .indexed("artistic", 0, "a")
        .indexed("track", 0, "t")
        .indexed("timestamp", 0, 1);
    invalid(prepare(&credentials(), &request));
}

#[test]
fn conditional_optional_and_unlisted_parameters_are_not_checked() {
    // `mbid` is conditional and `limit` optional: neither is needed.
    assert!(prepare(&credentials(), &Request::new(&READ).param("user", "rj")).is_ok());
    // A parameter the spec does not list is passed through.
    let request = Request::new(&READ)
        .param("user", "rj")
        .param("something_new", "x");
    let http = prepare(&credentials(), &request).unwrap();
    assert_eq!(get(&query_of(&http), "something_new"), Some("x"));
}

#[test]
fn invalid_requests_report_what_was_not_sent() {
    let read = invalid(prepare(&credentials(), &Request::new(&READ)));
    assert_eq!(read.delivery(), None);

    let write = invalid(prepare(&credentials(), &Request::new(&NOW_PLAYING)));
    assert_eq!(write.delivery(), Some(Delivery::NotSent));
}

#[test]
fn awkward_values_round_trip() {
    let values = [
        "a b",
        "a&b=c",
        "plus+sign",
        "100%",
        "#hash",
        "?query",
        "slash/path",
        "Björk",
        "日本語",
        "\u{1F3B5}",
        "line\nbreak",
        "",
        "  padded  ",
        "[bracket]",
    ];
    for value in values {
        let request = Request::new(&READ).param("user", value);
        let http = prepare(&credentials(), &request).unwrap();
        assert_eq!(get(&query_of(&http), "user"), Some(value), "GET {value:?}");

        let request = Request::new(&NOW_PLAYING)
            .param("artist", value)
            .param("track", value);
        let http = prepare(&credentials(), &request).unwrap();
        assert_eq!(
            get(&body_of(&http), "artist"),
            Some(value),
            "POST {value:?}"
        );
    }
}

#[test]
fn nothing_but_the_query_follows_the_question_mark() {
    let request = Request::new(&READ).param("user", "a#b?c");
    let http = prepare(&credentials(), &request).unwrap();
    // A raw `#` or a second `?` would change what the server sees.
    assert_eq!(http.url().matches('?').count(), 1);
    assert!(!http.url().contains('#'));
}

#[test]
fn the_signature_matches_an_independent_computation() {
    let cases = [
        Request::new(&SIGNED_GET).param("token", "tok en&=+#é"),
        Request::new(&NOW_PLAYING)
            .param("artist", "Björk")
            .param("track", "Jóga \u{1F3B5}")
            .param("duration", 300),
        Request::new(&SCROBBLE)
            .indexed("artist", 1, "A")
            .indexed("artist", 2, "B")
            .indexed("artist", 10, "C")
            .indexed("track", 1, "t")
            .indexed("timestamp", 1, 1),
    ];
    for request in cases {
        let http = prepare(&credentials(), &request).unwrap();
        let params = match http.verb() {
            Verb::Get => query_of(&http),
            Verb::Post => body_of(&http),
        };
        let sent = get(&params, "api_sig").unwrap().to_owned();
        assert_eq!(sent, independent_signature(&params, SENTINEL_API_SECRET));
    }
}

#[test]
fn the_wire_signature_does_not_depend_on_the_caller_order() {
    let a = Request::new(&NOW_PLAYING)
        .param("artist", "a")
        .param("track", "t");
    let b = Request::new(&NOW_PLAYING)
        .param("track", "t")
        .param("artist", "a");
    let sig = |r: &Request| {
        let http = prepare(&credentials(), r).unwrap();
        get(&body_of(&http), "api_sig").unwrap().to_owned()
    };
    assert_eq!(sig(&a), sig(&b));
}

#[test]
fn a_different_root_is_used_for_get_and_post() {
    let root = "http://127.0.0.1:9/2.0/";
    let get = prepare_with_root(
        root,
        &credentials(),
        &Request::new(&READ).param("user", "u"),
    )
    .unwrap();
    assert!(get.url().starts_with("http://127.0.0.1:9/2.0/?method="));
    let post = prepare_with_root(
        root,
        &credentials(),
        &Request::new(&NOW_PLAYING)
            .param("artist", "a")
            .param("track", "t"),
    )
    .unwrap();
    assert_eq!(post.url(), root);
}

// Redaction. Sentinels are obviously fake and appear nowhere in real data.

#[test]
fn credentials_debug_hides_every_secret() {
    let text = format!("{:?} {:#?}", credentials(), credentials());
    assert_no_sentinel(&text);
    assert!(text.contains("<redacted>"));
}

#[test]
fn request_debug_hides_sensitive_parameters_and_shows_the_rest() {
    let request = Request::new(&SIGNED_GET)
        .param("token", SENTINEL_TOKEN)
        .param("password", SENTINEL_PASSWORD)
        .param("artist", "KITANO REM");
    let text = format!("{request:?} {request:#?}");
    assert_no_sentinel(&text);
    assert!(text.contains("auth.getSession"));
    assert!(text.contains("KITANO REM"));
}

#[test]
fn http_request_debug_hides_secrets_in_the_url() {
    let request = Request::new(&SIGNED_GET)
        .param("token", SENTINEL_TOKEN)
        .param("password", SENTINEL_PASSWORD)
        .param("artist", "KITANO REM");
    let http = prepare(&credentials(), &request).unwrap();
    // The accessor is exact, so the test is meaningful.
    assert!(http.url().contains(SENTINEL_API_KEY));
    assert!(http.url().contains(SENTINEL_TOKEN));

    let text = format!("{http:?} {http:#?}");
    assert_no_sentinel(&text);
    assert!(text.contains("artist=KITANO+REM"), "{text}");
    assert!(text.contains("method=auth.getSession"), "{text}");
    assert!(text.contains("format=json"), "{text}");
}

#[test]
fn http_request_debug_hides_secrets_in_the_body() {
    let request = Request::new(&NOW_PLAYING)
        .param("artist", "KITANO REM")
        .param("track", "RAINSICK")
        .param("token", SENTINEL_TOKEN)
        .param("password", SENTINEL_PASSWORD);
    let http = prepare(&credentials(), &request).unwrap();
    assert!(
        std::str::from_utf8(http.body().unwrap())
            .unwrap()
            .contains(SENTINEL_SESSION_KEY)
    );

    let text = format!("{http:?} {http:#?}");
    assert_no_sentinel(&text);
    assert!(text.contains("artist=KITANO+REM"), "{text}");
    assert!(text.contains("track=RAINSICK"), "{text}");
    assert!(text.contains("method=track.updateNowPlaying"), "{text}");
}

#[test]
fn every_sensitive_name_is_redacted() {
    let request = Request::new(&NOW_PLAYING)
        .param("artist", "a")
        .param("track", "t")
        .param("token", "x")
        .param("password", "y");
    let http = prepare(&credentials(), &request).unwrap();
    let text = format!("{http:?}");
    for name in ["api_key", "api_sig", "sk", "token", "password"] {
        assert!(
            text.contains(&format!("{name}=<redacted>")),
            "{name}: {text}"
        );
    }
}

#[test]
fn errors_from_prepare_carry_no_credentials() {
    let missing_session = Credentials::new(ApiKey::new(SENTINEL_API_KEY))
        .with_secret(ApiSecret::new(SENTINEL_API_SECRET));
    let request = Request::new(&NOW_PLAYING)
        .param("artist", "a")
        .param("track", "t");
    let error = invalid(prepare(&missing_session, &request));
    assert_no_sentinel(&format!("{error} {error:?} {error:#?}"));

    let request = Request::new(&READ)
        .param("user", "rj")
        .param("sk", SENTINEL_SESSION_KEY);
    let error = invalid(prepare(&credentials(), &request));
    assert_no_sentinel(&format!("{error} {error:?} {error:#?}"));
}

#[test]
fn error_kind_is_invalid_request_for_every_rejection() {
    // A guard that `invalid` checks what it says.
    let ok = prepare(&credentials(), &Request::new(&READ).param("user", "rj"));
    assert!(ok.is_ok());
    let error = prepare(&credentials(), &Request::new(&READ)).unwrap_err();
    assert_eq!(error.kind(), ErrorKind::InvalidRequest);
}

proptest! {
    // synthetic
    #[test]
    fn arbitrary_parameters_round_trip_and_sign_correctly(
        params in proptest::collection::btree_map("[a-z][a-z0-9_]{0,10}", "\\PC{0,24}", 0..8),
        post in any::<bool>(),
    ) {
        let reserved = ["method", "api_key", "api_sig", "sk", "format", "callback"];
        let mut request = if post {
            Request::new(&NOW_PLAYING).param("artist", "a").param("track", "t")
        } else {
            Request::new(&READ).param("user", "rj")
        };
        let mut sent = BTreeMap::new();
        for (name, value) in &params {
            if reserved.contains(&name.as_str())
                || ["artist", "track", "user"].contains(&name.as_str())
            {
                continue;
            }
            request = request.param(name, value.as_str());
            sent.insert(name.clone(), value.clone());
        }
        let http = prepare(&credentials(), &request).unwrap();
        let wire = if post { body_of(&http) } else { query_of(&http) };

        for (name, value) in &sent {
            prop_assert_eq!(get(&wire, name), Some(value.as_str()));
        }
        // Library parameters: method, api_key, format, and for the POST
        // sk and api_sig, plus the two fixed caller parameters.
        let fixed = if post { 5 + 2 } else { 3 + 1 };
        prop_assert_eq!(wire.len(), sent.len() + fixed);
        if post {
            prop_assert_eq!(
                get(&wire, "api_sig").unwrap(),
                independent_signature(&wire, SENTINEL_API_SECRET)
            );
        }
        prop_assert!(http.url().is_ascii());
        let debug = format!("{http:?}");
        for sentinel in SENTINELS {
            prop_assert!(!debug.contains(sentinel));
        }
    }
}

// A read made as the session's user. synthetic

#[test]
fn a_read_as_user_carries_the_session_and_a_signature() {
    let request = Request::new(&READ).param("user", "rj").as_user();
    let http = prepare(&credentials(), &request).unwrap();
    assert_eq!(http.verb(), Verb::Get);
    let params = query_of(&http);
    assert_eq!(get(&params, "sk"), Some(SENTINEL_SESSION_KEY));
    assert_eq!(
        get(&params, "api_sig"),
        Some(independent_signature(&params, SENTINEL_API_SECRET).as_str())
    );
    assert_no_sentinel(&format!("{http:?}"));
}

#[test]
fn a_read_as_user_needs_the_secret_and_the_session() {
    let request = Request::new(&READ).param("user", "rj").as_user();
    let key_only = Credentials::new(ApiKey::new(SENTINEL_API_KEY));
    assert_eq!(
        invalid(prepare(&key_only, &request)).kind(),
        ErrorKind::InvalidRequest
    );
    let no_session = key_only.with_secret(ApiSecret::new(SENTINEL_API_SECRET));
    assert_eq!(
        invalid(prepare(&no_session, &request)).kind(),
        ErrorKind::InvalidRequest
    );
}

#[test]
fn a_plain_read_ignores_a_session_it_was_not_asked_to_use() {
    let request = Request::new(&READ).param("user", "rj");
    let params = query_of(&prepare(&credentials(), &request).unwrap());
    assert_eq!(get(&params, "sk"), None);
    assert_eq!(get(&params, "api_sig"), None);
}
