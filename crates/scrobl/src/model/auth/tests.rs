#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]

use serde_json::{Value, json};

use super::*;
use crate::ErrorKind;
use crate::model::decode;
use crate::protocol::{self, HttpResponse, Request};

// Fixtures. Provenance for each:
//
// - `derived/auth-token.json` and `derived/auth-session.json` carry the
//   sample values of the official pages of `auth.getToken` and
//   `auth.getSession`, which show XML, in the JSON shape the service gives
//   other replies. derived.
// - Everything else is built here. synthetic.

const TOKEN: &[u8] = include_bytes!("../../../fixtures/derived/auth-token.json");
const SESSION: &[u8] = include_bytes!("../../../fixtures/derived/auth-session.json");

/// The session key of the fixture, which no error or `Debug` may show.
const KEY: &str = "d580d57f32848f5dcf574d1ce18d78b2";

fn raw(spec: &'static MethodSpec, body: impl Into<Vec<u8>>) -> Raw {
    protocol::decode(&Request::new(spec), HttpResponse::new(200, body.into())).unwrap()
}

fn session(body: &Value) -> Result<Session, Error> {
    decode::auth_get_session(&raw(&methods::AUTH_GET_SESSION, body.to_string()))
}

#[test]
fn a_token_is_read() {
    let token = decode::auth_get_token(&raw(&methods::AUTH_GET_TOKEN, TOKEN)).unwrap();
    assert_eq!(token.expose(), "cf45fe5a3e3cebe168480a086d7fe481");
}

#[test]
fn a_token_that_is_missing_empty_or_not_a_string_is_an_error() {
    for body in [
        json!({}),
        json!({"token": ""}),
        json!({"token": 12}),
        json!({"token": {"#text": "x"}}),
    ] {
        let error =
            decode::auth_get_token(&raw(&methods::AUTH_GET_TOKEN, body.to_string())).unwrap_err();
        assert_eq!(error.kind(), ErrorKind::Decode, "{body}");
        assert!(error.to_string().contains("token"), "{error}");
        assert_eq!(error.method(), Some("auth.getToken"));
    }
    let error = decode::auth_get_token(&raw(&methods::AUTH_GET_TOKEN, "[]")).unwrap_err();
    assert_eq!(error.kind(), ErrorKind::Decode);
}

#[test]
fn a_session_is_read_from_either_method() {
    let desktop = decode::auth_get_session(&raw(&methods::AUTH_GET_SESSION, SESSION)).unwrap();
    let mobile =
        decode::auth_get_mobile_session(&raw(&methods::AUTH_GET_MOBILE_SESSION, SESSION)).unwrap();
    for session in [desktop, mobile] {
        assert_eq!(session.name(), "MyLastFMUsername");
        assert_eq!(session.key().expose(), KEY);
        assert!(!session.subscriber());
        assert_eq!(session.clone().into_key().expose(), KEY);
    }
    let paying = session(&json!({"session": {"name": "rj", "key": "k", "subscriber": "1"}}));
    assert!(paying.unwrap().subscriber());
}

#[test]
fn the_debug_of_a_session_does_not_show_the_key() {
    let session = decode::auth_get_session(&raw(&methods::AUTH_GET_SESSION, SESSION)).unwrap();
    let debug = format!("{session:?}");
    assert!(!debug.contains(KEY), "{debug}");
    assert!(debug.contains("MyLastFMUsername"), "{debug}");
}

#[test]
fn a_session_missing_what_it_needs_is_an_error_that_does_not_show_the_key() {
    let cases = [
        (json!({}), "session"),
        (
            json!({"session": {"key": KEY, "subscriber": 0}}),
            "session.name",
        ),
        (
            json!({"session": {"name": "", "key": KEY, "subscriber": 0}}),
            "session.name",
        ),
        (
            json!({"session": {"name": "rj", "subscriber": 0}}),
            "session.key",
        ),
        (
            json!({"session": {"name": "rj", "key": "", "subscriber": 0}}),
            "session.key",
        ),
        (
            json!({"session": {"name": "rj", "key": KEY}}),
            "session.subscriber",
        ),
        (
            json!({"session": {"name": "rj", "key": KEY, "subscriber": 2}}),
            "session.subscriber",
        ),
    ];
    for (body, field) in cases {
        let error = session(&body).unwrap_err();
        assert_eq!(error.kind(), ErrorKind::Decode, "{body}");
        let shown = format!("{error} {error:?}");
        assert!(shown.contains(field), "{body}: {shown}");
        assert!(!shown.contains(KEY), "{body}: {shown}");
    }
}

#[test]
fn a_reply_to_another_method_is_an_error() {
    let token = raw(&methods::AUTH_GET_TOKEN, TOKEN);
    assert_eq!(
        decode::auth_get_session(&token).unwrap_err().kind(),
        ErrorKind::Decode
    );
    let session = raw(&methods::AUTH_GET_SESSION, SESSION);
    assert_eq!(
        decode::auth_get_token(&session).unwrap_err().kind(),
        ErrorKind::Decode
    );
    assert_eq!(
        decode::auth_get_mobile_session(&session)
            .unwrap_err()
            .kind(),
        ErrorKind::Decode
    );
}
