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
// - The two `kelchm-library-artists*` bodies were recorded by the
//   repository owner from their own account on 2026-11-10 and are kept
//   byte for byte. recorded.
// - Everything else is built here from those shapes. synthetic.

/// `limit=3`: `artist` is an array.
const THREE: &[u8] = include_bytes!("../../../fixtures/recorded/kelchm-library-artists.json");
/// `limit=1`: `artist` is a single object.
const ONE: &[u8] = include_bytes!("../../../fixtures/recorded/kelchm-library-artists-one.json");

/// A string that appears in no error and no `Debug` output.
const SECRET: &str = "SYNTHETIC-RESPONSE-TEXT";

fn raw(body: impl Into<Vec<u8>>) -> Raw {
    let request = Request::new(&methods::LIBRARY_GET_ARTISTS).param("user", "rj");
    protocol::decode(&request, HttpResponse::new(200, body.into())).unwrap()
}

fn decode(body: &Value) -> Result<Page<LibraryArtist>, Error> {
    decode::library_get_artists(&raw(body.to_string()))
}

fn row(name: &str) -> Value {
    json!({"tagcount": "2", "mbid": "", "url": "https://example.invalid/a",
           "playcount": "7", "name": name, "streamable": "0"})
}

fn page(artist: Value, total: u64) -> Value {
    json!({"artists": {"artist": artist, "@attr": {
        "user": "rj", "page": "1", "perPage": "50",
        "totalPages": total.min(1).to_string(), "total": total.to_string()}}})
}

/// The message of a decode error, which must name `field` and repeat no
/// response text.
fn decode_error(body: &Value, field: &str) {
    let error = decode(body).unwrap_err();
    assert_eq!(error.kind(), ErrorKind::Decode);
    let shown = format!("{error} {error:?}");
    assert!(shown.contains(field), "{shown} does not name {field}");
    assert!(!shown.contains(SECRET), "{shown}");
}

#[test]
fn the_recorded_page_decodes() {
    let page = decode::library_get_artists(&raw(THREE)).unwrap();

    assert_eq!(page.subject(), Some("kelchm"));
    assert_eq!(
        (
            page.number(),
            page.per_page(),
            page.total_pages(),
            page.total()
        ),
        (1, 3, 3369, 10_105)
    );

    let rows: Vec<_> = page
        .items()
        .iter()
        .map(|a| (a.artist().name(), a.playcount(), a.tagcount()))
        .collect();
    assert_eq!(
        rows,
        [
            ("Portugal. The Man", 5584, Some(0)),
            ("Bright Eyes", 4146, Some(0)),
            ("Elliott Smith", 3726, Some(0))
        ]
    );
    let first = &page.items()[0];
    assert_eq!(
        first.artist().mbid(),
        Some("3599a39e-4e10-4cb5-90d4-c8a015ebc73b")
    );
    assert_eq!(
        first.artist().url(),
        Some("https://www.last.fm/music/Portugal.+The+Man")
    );
    // What the model does not name is still in the row.
    assert_eq!(first.json()["streamable"], "0");
    assert_eq!(first.artist().images().len(), 5);
    assert_eq!(first.artist().images()[4].size(), Some("mega"));
}

#[test]
fn the_recorded_page_of_one_is_a_single_object_and_decodes() {
    let raw = raw(ONE);
    assert!(
        raw.json::<Value>().unwrap()["artists"]["artist"].is_object(),
        "the service sent one row as an object, not an array of one"
    );
    let page = decode::library_get_artists(&raw).unwrap();
    assert_eq!((page.per_page(), page.total()), (1, 10_105));
    let [only] = page.items() else {
        panic!("one artist expected");
    };
    assert_eq!(only.artist().name(), "Portugal. The Man");
    assert_eq!(only.playcount(), 5584);
}

#[test]
fn an_empty_list_decodes_in_each_of_its_forms() {
    for artist in [Some(json!([])), Some(json!("")), None] {
        let mut body = page(json!([]), 0);
        match artist {
            Some(artist) => body["artists"]["artist"] = artist,
            None => {
                body["artists"].as_object_mut().unwrap().remove("artist");
            }
        }
        assert!(decode(&body).unwrap().items().is_empty());
    }
}

#[test]
fn rows_that_are_promised_must_be_there() {
    decode_error(&page(json!([]), 3), "artists.artist");
    decode_error(&page(json!(SECRET), 3), "artists.artist");
}

#[test]
fn a_malformed_page_names_the_field() {
    decode_error(&json!({"other": SECRET}), "artists");
    decode_error(&json!({"artists": {"artist": []}}), "artists.@attr");
    decode_error(&page(json!([row("A"), SECRET]), 2), "artists.artist[1]");

    let mut unnamed = row("B");
    unnamed["name"] = json!(7);
    decode_error(
        &page(json!([row("A"), unnamed]), 2),
        "artists.artist[1].name",
    );

    let mut uncounted = row("B");
    uncounted["playcount"] = json!(SECRET);
    decode_error(
        &page(json!([row("A"), uncounted]), 2),
        "artists.artist[1].playcount",
    );

    let mut odd_tags = row("B");
    odd_tags["tagcount"] = json!(SECRET);
    decode_error(
        &page(json!([row("A"), odd_tags]), 2),
        "artists.artist[1].tagcount",
    );
}

#[test]
fn a_row_without_a_tagcount_has_none() {
    let mut bare = row("A");
    bare.as_object_mut().unwrap().remove("tagcount");
    let page = decode(&page(json!([bare]), 1)).unwrap();
    assert_eq!(page.items()[0].tagcount(), None);
    assert_eq!(page.items()[0].artist().mbid(), None);
}

#[test]
fn a_response_to_another_method_is_refused() {
    let request = Request::new(&methods::USER_GET_TOP_ARTISTS).param("user", "rj");
    let raw = protocol::decode(&request, HttpResponse::new(200, THREE)).unwrap();
    let error = decode::library_get_artists(&raw).unwrap_err();
    assert_eq!(error.kind(), ErrorKind::Decode);
    assert!(error.to_string().contains("different method"));
}

#[test]
fn debug_leaves_out_the_text_of_the_response() {
    let page = decode(&page(json!([row(SECRET)]), 1)).unwrap();
    let shown = format!("{page:?} {:?}", page.items());
    assert!(!shown.contains(SECRET), "{shown}");
    assert!(shown.contains("playcount: 7"), "{shown}");
}
