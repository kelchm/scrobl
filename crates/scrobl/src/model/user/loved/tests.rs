#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]

use serde_json::{Value, json};

use super::*;
use crate::ErrorKind;
use crate::protocol::{self, HttpResponse, Request};

// Fixtures. Provenance for each:
//
// - `fixtures/derived/loved-tracks-page.json` follows a response recorded by
//   the repository owner from their own account on 2026-10-10 with
//   `limit=2`: member names and order, nesting, value types, the escaped
//   slashes and the four image sizes. Every value is invented. derived.
// - Everything else is built here from that shape. synthetic.

const PAGE: &[u8] = include_bytes!("../../../../fixtures/derived/loved-tracks-page.json");
/// Two loved tracks, recorded by the repository owner from their own
/// account on 2026-11-10 and kept byte for byte. recorded.
const RECORDED: &[u8] = include_bytes!("../../../../fixtures/recorded/kelchm-loved-tracks.json");

/// A string that appears in no error and no `Debug` output.
const SECRET: &str = "SYNTHETIC-RESPONSE-TEXT";

fn request() -> Request {
    Request::new(&methods::USER_GET_LOVED_TRACKS).param("user", "rj")
}

fn raw(body: impl Into<Vec<u8>>) -> Raw {
    protocol::decode(&request(), HttpResponse::new(200, body.into())).unwrap()
}

fn decode(body: &Value) -> Result<LovedTracksPage, crate::Error> {
    LovedTracksPage::decode(&raw(body.to_string()))
}

fn attr(total: u64, total_pages: u64) -> Value {
    json!({"user": "rj", "page": "1", "perPage": "50",
           "totalPages": total_pages.to_string(), "total": total.to_string()})
}

fn row(uts: &str, name: &str) -> Value {
    json!({
        "artist": {"url": "https://example.invalid/a", "name": "Artist", "mbid": ""},
        "date": {"uts": uts, "#text": "01 Jan 2023, 00:00"},
        "mbid": "",
        "url": "https://example.invalid/t",
        "name": name,
        "streamable": {"fulltrack": "0", "#text": "0"}
    })
}

fn page(track: Value, total: u64) -> Value {
    json!({"lovedtracks": {"track": track, "@attr": attr(total, total.min(1))}})
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
fn the_derived_page_decodes() {
    let raw = raw(PAGE);
    let page = LovedTracksPage::decode(&raw).unwrap();

    let attr = page.attr();
    assert_eq!(attr.user(), "rj_Synthetic");
    assert_eq!(
        (
            attr.page(),
            attr.per_page(),
            attr.total_pages(),
            attr.total()
        ),
        (1, 2, 130, 259)
    );

    let [first, second] = page.tracks() else {
        panic!("two tracks expected");
    };
    assert_eq!(first.loved_at(), 1_700_000_200);
    assert_eq!(first.track().name(), "First Song");
    assert_eq!(first.track().mbid(), None);
    assert_eq!(
        first.track().url(),
        Some("https://example.invalid/music/First+Artist/_/First+Song")
    );
    assert_eq!(first.artist().name(), "First Artist");
    assert_eq!(
        first.artist().mbid(),
        Some("00000000-0000-4000-8000-0000000000a1")
    );
    assert_eq!(second.loved_at(), 1_700_000_100);
    assert_eq!(second.artist().mbid(), None);
    assert_eq!(
        second.track().mbid(),
        Some("00000000-0000-4000-8000-0000000000b2")
    );
    // What the model does not name is still in the row.
    assert_eq!(first.json()["streamable"]["fulltrack"], "0");
    assert_eq!(first.json()["mbid"], "");
}

#[test]
fn the_recorded_page_decodes() {
    let page = LovedTracksPage::decode(&raw(RECORDED)).unwrap();
    let attr = page.attr();
    assert_eq!(attr.user(), "kelchm");
    assert_eq!(
        (
            attr.page(),
            attr.per_page(),
            attr.total_pages(),
            attr.total()
        ),
        (1, 2, 130, 259)
    );
    let rows: Vec<_> = page
        .tracks()
        .iter()
        .map(|t| (t.loved_at(), t.track().name(), t.artist().name()))
        .collect();
    assert_eq!(
        rows,
        [
            (1_717_821_545, "68", "Nolan Taylor"),
            (1_715_748_724, "& (Audiotree Live Version)", "Pinegrove")
        ]
    );
    let first = &page.tracks()[0];
    assert_eq!(first.track().mbid(), None);
    assert_eq!(
        first.artist().mbid(),
        Some("5be29c0c-1a6f-481d-bab0-8c805e63ba68")
    );
    assert_eq!(
        first.track().url(),
        Some("https://www.last.fm/music/Nolan+Taylor/_/68")
    );
    assert_eq!(first.track().images().len(), 4);
    assert!(first.artist().images().is_empty());
}

#[test]
fn a_single_row_may_come_as_an_object() {
    let page = decode(&page(row("1700000000", "Only"), 1)).unwrap();
    assert_eq!(page.tracks().len(), 1);
    assert_eq!(page.tracks()[0].track().name(), "Only");
}

#[test]
fn an_empty_list_decodes_in_each_of_its_forms() {
    for track in [Some(json!([])), Some(json!("")), None] {
        let mut body = page(json!([]), 0);
        match track {
            Some(track) => body["lovedtracks"]["track"] = track,
            None => {
                body["lovedtracks"].as_object_mut().unwrap().remove("track");
            }
        }
        assert!(decode(&body).unwrap().tracks().is_empty());
    }
}

#[test]
fn a_page_beyond_the_last_may_be_empty() {
    let mut body = page(json!([]), 3);
    body["lovedtracks"]["@attr"]["page"] = json!("2");
    assert!(decode(&body).unwrap().tracks().is_empty());
}

#[test]
fn rows_that_are_promised_must_be_there() {
    decode_error(&page(json!([]), 3), "lovedtracks.track");
    decode_error(&page(json!(SECRET), 3), "lovedtracks.track");
    decode_error(&page(Value::Null, 3), "lovedtracks.track");
}

#[test]
fn a_malformed_page_names_the_field() {
    decode_error(&json!({"other": SECRET}), "lovedtracks");
    decode_error(&json!({"lovedtracks": {"track": []}}), "lovedtracks.@attr");

    let mut body = page(json!([row("1700000000", "A")]), 1);
    body["lovedtracks"]["@attr"]["total"] = json!(SECRET);
    decode_error(&body, "lovedtracks.@attr.total");

    let mut body = page(json!([row("1700000000", "A")]), 1);
    body["lovedtracks"]["@attr"]["user"] = json!(7);
    decode_error(&body, "lovedtracks.@attr.user");
}

#[test]
fn a_malformed_row_names_the_field() {
    let rows = |second: Value| page(json!([row("1700000000", "A"), second]), 2);

    decode_error(&rows(json!(SECRET)), "lovedtracks.track[1]");
    decode_error(&rows(row(SECRET, "B")), "lovedtracks.track[1].date.uts");

    let mut undated = row("1700000000", "B");
    undated.as_object_mut().unwrap().remove("date");
    decode_error(&rows(undated), "lovedtracks.track[1].date.uts");

    let mut unnamed = row("1700000000", "B");
    unnamed["name"] = json!(7);
    decode_error(&rows(unnamed), "lovedtracks.track[1].name");

    let mut no_artist = row("1700000000", "B");
    no_artist["artist"] = json!(SECRET);
    decode_error(&rows(no_artist), "lovedtracks.track[1].artist");

    let mut odd_mbid = row("1700000000", "B");
    odd_mbid["artist"]["mbid"] = json!(7);
    decode_error(&rows(odd_mbid), "lovedtracks.track[1].artist.mbid");
}

#[test]
fn a_row_with_nothing_in_it_names_its_first_field() {
    // The track, then the artist, then the date: the order of the checks is
    // the order of the errors.
    decode_error(&page(json!([{}]), 1), "lovedtracks.track[0].name");
    decode_error(
        &page(json!([{"name": "A"}]), 1),
        "lovedtracks.track[0].artist",
    );
}

#[test]
fn a_repeated_member_is_an_error() {
    let body = r#"{"lovedtracks": {"track": [], "track": [],
        "@attr": {"user": "rj", "page": "1", "perPage": "50",
                  "totalPages": "0", "total": "0"}}}"#;
    let error = LovedTracksPage::decode(&raw(body)).unwrap_err();
    assert_eq!(error.kind(), ErrorKind::Decode);
}

#[test]
fn a_response_to_another_method_is_refused() {
    let request = Request::new(&methods::USER_GET_RECENT_TRACKS).param("user", "rj");
    let raw = protocol::decode(&request, HttpResponse::new(200, PAGE)).unwrap();
    let error = LovedTracksPage::decode(&raw).unwrap_err();
    assert_eq!(error.kind(), ErrorKind::Decode);
    assert!(error.to_string().contains("different method"));
}

#[test]
fn debug_leaves_out_the_text_of_the_response() {
    let page = decode(&page(json!([row("1700000000", SECRET)]), 1)).unwrap();
    let shown = format!("{page:?} {:?} {:#?}", page.tracks()[0], page.tracks());
    assert!(!shown.contains(SECRET), "{shown}");
    assert!(shown.contains("1700000000"));
}
