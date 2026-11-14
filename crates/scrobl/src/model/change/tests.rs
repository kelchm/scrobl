#![allow(clippy::unwrap_used)]

use super::*;
use crate::protocol::{self, HttpResponse, Request};
use crate::{Delivery, ErrorKind};

const EMPTY: &[u8] = include_bytes!("../../../fixtures/derived/acknowledged.json");

fn raw(spec: &'static MethodSpec, body: &[u8]) -> Raw {
    protocol::decode(&Request::new(spec), HttpResponse::new(200, body.to_vec())).unwrap()
}

#[test]
fn an_empty_object_is_success() {
    track_love(&raw(&methods::TRACK_LOVE, EMPTY)).unwrap();
    album_remove_tag(&raw(&methods::ALBUM_REMOVE_TAG, EMPTY)).unwrap();
}

#[test]
fn anything_but_an_object_is_unknown_delivery() {
    for body in [&b"[]"[..], b"\"ok\"", b"null"] {
        let error = track_unlove(&raw(&methods::TRACK_UNLOVE, body)).unwrap_err();
        assert_eq!(error.kind(), ErrorKind::Decode);
        assert_eq!(error.delivery(), Some(Delivery::Unknown));
    }
}

#[test]
fn a_reply_for_another_method_is_refused() {
    let error = track_love(&raw(&methods::TRACK_UNLOVE, EMPTY)).unwrap_err();
    assert_eq!(error.kind(), ErrorKind::Decode);
}
