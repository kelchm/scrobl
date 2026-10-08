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
// - Row and page shapes follow the example responses in
//   docs_user_getRecentTracks.md (community documentation): the plain and
//   extended artist, `@attr.nowplaying` as the string "true", string-typed
//   `@attr` values. The values are invented. derived.
// - Everything else is built here from those shapes. synthetic.

/// A string that appears in no error and no `Debug` output.
const SECRET: &str = "SYNTHETIC-RESPONSE-TEXT";

fn attr() -> Value {
    json!({"user": "rj", "page": "1", "perPage": "50", "totalPages": "1", "total": "2"})
}

fn plain_row(uts: &str, name: &str) -> Value {
    json!({
        "artist": {"mbid": "", "#text": "Artist"},
        "streamable": "0",
        "image": [{"size": "small", "#text": "https://example.invalid/s.jpg"}],
        "mbid": "",
        "album": {"mbid": "", "#text": "Album"},
        "name": name,
        "url": "https://example.invalid/track",
        "date": {"uts": uts, "#text": "01 Jan 2023, 00:00"}
    })
}

fn extended_row(uts: &str, loved: &str) -> Value {
    json!({
        "artist": {
            "url": "https://example.invalid/artist",
            "name": "Artist",
            "mbid": "11111111-1111-1111-1111-111111111111",
            "image": []
        },
        "loved": loved,
        "mbid": "22222222-2222-2222-2222-222222222222",
        "name": "Track",
        "album": {"mbid": "", "#text": ""},
        "url": "https://example.invalid/track",
        "date": {"uts": uts, "#text": "01 Jan 2023, 00:00"}
    })
}

fn now_playing_row() -> Value {
    json!({
        "@attr": {"nowplaying": "true"},
        "artist": {"mbid": "", "#text": "Playing Artist"},
        "name": "Playing",
        "mbid": "",
        "album": {"mbid": "", "#text": ""},
        "url": "https://example.invalid/playing"
    })
}

fn page_with(track: Option<Value>, attr: Value) -> Value {
    let mut recent = json!({"@attr": attr});
    if let Some(track) = track {
        recent["track"] = track;
    }
    json!({"recenttracks": recent})
}

fn raw_of(body: &Value) -> Raw {
    let request = Request::new(&methods::USER_GET_RECENT_TRACKS).param("user", "rj");
    protocol::decode(&request, HttpResponse::new(200, body.to_string())).unwrap()
}

fn decode(body: &Value) -> Result<RecentTracksPage, Error> {
    RecentTracksPage::decode(&raw_of(body))
}

fn rows(rows: Vec<Value>) -> Value {
    page_with(Some(Value::Array(rows)), attr())
}

#[track_caller]
fn decode_error(body: &Value) -> Error {
    let error = decode(body).unwrap_err();
    assert_eq!(error.kind(), ErrorKind::Decode, "{error}");
    assert_eq!(error.method(), Some("user.getRecentTracks"));
    error
}

#[track_caller]
fn assert_names(error: &Error, path: &str) {
    let text = error.to_string();
    assert!(text.contains(path), "`{text}` does not name `{path}`");
}

#[test]
fn plain_page_with_now_playing() {
    let page = decode(&rows(vec![
        now_playing_row(),
        plain_row("1700000000", "One"),
        plain_row("1699999999", "Two"),
    ]))
    .unwrap();

    assert_eq!(page.attr().user(), "rj");
    assert_eq!(page.attr().page(), 1);
    assert_eq!(page.attr().per_page(), 50);
    assert_eq!(page.attr().total_pages(), 1);
    assert_eq!(page.attr().total(), 2);

    let scrobbles = page.scrobbles();
    assert_eq!(scrobbles.len(), 2);
    assert_eq!(scrobbles[0].timestamp(), 1_700_000_000);
    assert_eq!(scrobbles[0].track().name(), "One");
    assert_eq!(scrobbles[0].track().mbid(), None);
    assert_eq!(
        scrobbles[0].track().url(),
        Some("https://example.invalid/track")
    );
    assert_eq!(scrobbles[0].artist().name(), "Artist");
    assert_eq!(scrobbles[0].artist().mbid(), None);
    assert_eq!(scrobbles[0].artist().url(), None);
    assert_eq!(scrobbles[0].album().title(), Some("Album"));
    assert_eq!(scrobbles[0].album().mbid(), None);
    assert_eq!(scrobbles[0].loved(), None);
    assert_eq!(scrobbles[1].timestamp(), 1_699_999_999);

    let playing = page.now_playing().unwrap();
    assert_eq!(playing.track().name(), "Playing");
    assert_eq!(playing.artist().name(), "Playing Artist");
    assert_eq!(playing.album().title(), None);
}

#[test]
fn extended_page_reads_artist_name_url_and_loved() {
    let page = decode(&rows(vec![
        extended_row("1700000001", "1"),
        extended_row("1700000000", "0"),
    ]))
    .unwrap();

    let first = &page.scrobbles()[0];
    assert_eq!(first.artist().name(), "Artist");
    assert_eq!(
        first.artist().mbid(),
        Some("11111111-1111-1111-1111-111111111111")
    );
    assert_eq!(first.artist().url(), Some("https://example.invalid/artist"));
    assert_eq!(first.loved(), Some(true));
    assert_eq!(first.album().title(), None);
    assert_eq!(page.scrobbles()[1].loved(), Some(false));
}

#[test]
fn json_is_the_row_as_decoded() {
    let row = plain_row("1700000000", "One");
    let page = decode(&rows(vec![row.clone()])).unwrap();
    assert_eq!(page.scrobbles()[0].json(), &row);
}

#[test]
fn json_tells_missing_from_empty() {
    let mut empty = plain_row("1700000000", "Empty");
    empty["mbid"] = json!("");
    let mut missing = plain_row("1700000000", "Missing");
    missing.as_object_mut().unwrap().remove("mbid");

    let page = decode(&rows(vec![empty, missing])).unwrap();
    let [empty, missing] = page.scrobbles() else {
        panic!("two rows");
    };
    assert_eq!(empty.track().mbid(), None);
    assert_eq!(missing.track().mbid(), None);
    assert_eq!(empty.json().get("mbid"), Some(&json!("")));
    assert_eq!(missing.json().get("mbid"), None);
}

#[test]
fn unknown_fields_stay_in_json() {
    let mut row = plain_row("1700000000", "One");
    row["future_field"] = json!({"a": [1, 2, 3]});
    let page = decode(&rows(vec![row])).unwrap();
    assert_eq!(
        page.scrobbles()[0].json()["future_field"],
        json!({"a": [1, 2, 3]})
    );
}

#[test]
fn now_playing_is_never_a_scrobble_even_with_a_date() {
    let mut playing = now_playing_row();
    playing["date"] = json!({"uts": "1700000500", "#text": "now"});
    let page = decode(&rows(vec![playing, plain_row("1700000000", "One")])).unwrap();

    assert_eq!(page.scrobbles().len(), 1);
    assert_eq!(page.scrobbles()[0].track().name(), "One");
    let playing = page.now_playing().unwrap();
    assert_eq!(playing.track().name(), "Playing");
    assert_eq!(playing.json()["date"]["uts"], "1700000500");
}

#[test]
fn now_playing_with_a_malformed_date_is_still_now_playing() {
    let mut playing = now_playing_row();
    playing["date"] = json!({"uts": "soon"});
    let page = decode(&rows(vec![playing])).unwrap();
    assert!(page.now_playing().is_some());
    assert!(page.scrobbles().is_empty());
}

#[test]
fn now_playing_flag_spellings() {
    for (flag, expect_now_playing) in [
        (json!("true"), true),
        (json!("1"), true),
        (json!(1), true),
        (json!("false"), false),
        (json!("0"), false),
        (json!(0), false),
    ] {
        let mut row = plain_row("1700000000", "One");
        row["@attr"] = json!({"nowplaying": flag});
        let page = decode(&rows(vec![row])).unwrap();
        assert_eq!(
            page.now_playing().is_some(),
            expect_now_playing,
            "flag {flag}"
        );
        assert_eq!(page.scrobbles().len(), usize::from(!expect_now_playing));
    }
}

#[test]
fn row_attr_without_the_flag_is_an_ordinary_row() {
    let mut row = plain_row("1700000000", "One");
    row["@attr"] = json!({"rank": "1"});
    let page = decode(&rows(vec![row])).unwrap();
    assert_eq!(page.scrobbles().len(), 1);
    assert!(page.now_playing().is_none());
}

#[test]
fn unrecognised_now_playing_flag_is_an_error() {
    for flag in [json!("yes"), json!(true), json!(""), json!(2), Value::Null] {
        let mut row = plain_row("1700000000", "One");
        row["@attr"] = json!({"nowplaying": flag});
        let error = decode_error(&rows(vec![row]));
        assert_names(&error, "recenttracks.track[0].@attr.nowplaying");
    }
}

#[test]
fn two_now_playing_rows_are_an_error() {
    let error = decode_error(&rows(vec![now_playing_row(), now_playing_row()]));
    assert_names(&error, "recenttracks.track");
}

#[test]
fn track_as_a_single_object() {
    let page = decode(&page_with(Some(plain_row("1700000000", "Only")), attr())).unwrap();
    assert_eq!(page.scrobbles().len(), 1);
    assert_eq!(page.scrobbles()[0].track().name(), "Only");

    let page = decode(&page_with(Some(now_playing_row()), attr())).unwrap();
    assert!(page.scrobbles().is_empty());
    assert!(page.now_playing().is_some());
}

#[test]
fn empty_result_shapes() {
    for track in [None, Some(json!([])), Some(json!(""))] {
        let page = decode(&page_with(track.clone(), attr())).unwrap();
        assert!(page.scrobbles().is_empty(), "track {track:?}");
        assert!(page.now_playing().is_none());
    }
}

#[test]
fn other_track_shapes_are_errors() {
    for track in [json!(null), json!("x"), json!(" "), json!(0), json!(true)] {
        let error = decode_error(&page_with(Some(track.clone()), attr()));
        assert_names(&error, "recenttracks.track");
    }
}

#[test]
fn a_row_without_a_valid_date_is_an_error_naming_the_row() {
    let bad_dates = [
        ("missing date", None),
        ("empty date", Some(json!({}))),
        ("missing uts", Some(json!({"#text": "x"}))),
        ("non-numeric", Some(json!({"uts": "soon"}))),
        ("empty", Some(json!({"uts": ""}))),
        ("float", Some(json!({"uts": 1.5}))),
        ("negative", Some(json!({"uts": -1}))),
        ("signed", Some(json!({"uts": "+5"}))),
        ("null", Some(json!({"uts": null}))),
        ("bool", Some(json!({"uts": true}))),
        ("date is a string", Some(json!("1700000000"))),
    ];
    for (label, date) in bad_dates {
        let mut bad = plain_row("1700000000", "Bad");
        match date {
            Some(date) => bad["date"] = date,
            None => {
                bad.as_object_mut().unwrap().remove("date");
            }
        }
        let error = decode_error(&rows(vec![
            plain_row("1700000002", "A"),
            plain_row("1700000001", "B"),
            bad,
        ]));
        assert_names(&error, "recenttracks.track[2].date.uts");
        assert!(!label.is_empty());
    }
}

#[test]
fn numeric_and_string_timestamps_both_decode() {
    let mut numeric = plain_row("0", "N");
    numeric["date"] = json!({"uts": 1_700_000_000_u64});
    let page = decode(&rows(vec![numeric, plain_row("1699999999", "S")])).unwrap();
    assert_eq!(page.scrobbles()[0].timestamp(), 1_700_000_000);
    assert_eq!(page.scrobbles()[1].timestamp(), 1_699_999_999);
}

#[test]
fn page_attr_members_are_all_required() {
    for key in ["user", "page", "perPage", "totalPages", "total"] {
        let mut missing = attr();
        missing.as_object_mut().unwrap().remove(key);
        let error = decode_error(&page_with(Some(json!([])), missing));
        assert_names(&error, &format!("recenttracks.@attr.{key}"));

        let mut malformed = attr();
        malformed[key] = if key == "user" {
            json!(5)
        } else {
            json!("many")
        };
        let error = decode_error(&page_with(Some(json!([])), malformed));
        assert_names(&error, &format!("recenttracks.@attr.{key}"));
    }
}

#[test]
fn page_attr_counts_accept_numbers_and_digit_strings_only() {
    let mut numeric = attr();
    numeric["total"] = json!(7);
    assert_eq!(
        decode(&page_with(Some(json!([])), numeric))
            .unwrap()
            .attr()
            .total(),
        7
    );
    for bad in [json!(1.5), json!("-1"), json!(""), json!(null), json!(true)] {
        let mut malformed = attr();
        malformed["total"] = bad;
        let error = decode_error(&page_with(Some(json!([])), malformed));
        assert_names(&error, "recenttracks.@attr.total");
    }
}

#[test]
fn missing_attr_or_root_is_an_error() {
    let error = decode_error(&json!({"recenttracks": {"track": []}}));
    assert_names(&error, "recenttracks.@attr");
    let error = decode_error(&json!({"recenttracks": {"track": [], "@attr": "x"}}));
    assert_names(&error, "recenttracks.@attr");
    let error = decode_error(&json!({"other": {}}));
    assert_names(&error, "recenttracks");
    let error = decode_error(&json!({"recenttracks": []}));
    assert_names(&error, "recenttracks");
    let error = decode_error(&json!([]));
    assert_eq!(error.kind(), ErrorKind::Decode);
}

#[test]
fn malformed_row_fields_are_errors_naming_the_field() {
    type Break = fn(&mut Value);
    let cases: [(&str, Break); 10] = [
        ("recenttracks.track[0].name", |r| {
            r.as_object_mut().unwrap().remove("name");
        }),
        ("recenttracks.track[0].name", |r| r["name"] = json!(5)),
        ("recenttracks.track[0].mbid", |r| r["mbid"] = json!(5)),
        ("recenttracks.track[0].url", |r| r["url"] = json!(null)),
        ("recenttracks.track[0].artist", |r| {
            r.as_object_mut().unwrap().remove("artist");
        }),
        ("recenttracks.track[0].artist", |r| r["artist"] = json!("A")),
        ("recenttracks.track[0].artist.name", |r| {
            r["artist"] = json!({"mbid": ""});
        }),
        ("recenttracks.track[0].album", |r| r["album"] = json!("A")),
        ("recenttracks.track[0].album.mbid", |r| {
            r["album"]["mbid"] = json!(1);
        }),
        ("recenttracks.track[0].loved", |r| r["loved"] = json!("yes")),
    ];
    for (path, break_row) in cases {
        let mut row = plain_row("1700000000", "One");
        break_row(&mut row);
        let error = decode_error(&rows(vec![row]));
        assert_names(&error, path);
    }
}

#[test]
fn a_row_that_is_not_an_object_is_an_error() {
    let error = decode_error(&rows(vec![plain_row("1700000000", "One"), json!("x")]));
    assert_names(&error, "recenttracks.track[1]");
    let error = decode_error(&rows(vec![json!(null)]));
    assert_names(&error, "recenttracks.track[0]");
}

#[test]
fn a_row_without_an_album_has_no_album() {
    let mut row = plain_row("1700000000", "One");
    row.as_object_mut().unwrap().remove("album");
    let page = decode(&rows(vec![row])).unwrap();
    assert_eq!(page.scrobbles()[0].album().title(), None);
    assert_eq!(page.scrobbles()[0].album().mbid(), None);
}

#[test]
fn a_response_for_another_method_is_an_error() {
    let request = Request::new(&methods::USER_GET_INFO).param("user", "rj");
    let raw = protocol::decode(&request, HttpResponse::new(200, rows(vec![]).to_string())).unwrap();
    let error = RecentTracksPage::decode(&raw).unwrap_err();
    assert_eq!(error.kind(), ErrorKind::Decode);
}

#[test]
fn a_body_that_is_not_json_is_a_decode_error() {
    let request = Request::new(&methods::USER_GET_RECENT_TRACKS).param("user", "rj");
    // protocol::decode already rejects a non-JSON 2xx, so a Raw always parses.
    let error =
        protocol::decode(&request, HttpResponse::new(200, "{\"recenttracks\": {")).unwrap_err();
    assert_eq!(error.kind(), ErrorKind::Decode);
}

#[test]
fn errors_and_debug_never_contain_response_text() {
    let mut row = plain_row("1700000000", SECRET);
    row["artist"] = json!({"#text": SECRET, "mbid": SECRET});
    row["album"] = json!({"#text": SECRET, "mbid": SECRET});
    row["url"] = json!(SECRET);

    let page = decode(&page_with(
        Some(json!([row.clone()])),
        json!({"user": SECRET, "page": "1", "perPage": "1", "totalPages": "1", "total": "1"}),
    ))
    .unwrap();
    let rendered = format!(
        "{page:?} {:?} {:?} {:?} {:?} {:?}",
        page.attr(),
        page.scrobbles()[0],
        page.scrobbles()[0].track(),
        page.scrobbles()[0].artist(),
        page.scrobbles()[0].album(),
    );
    assert!(!rendered.contains(SECRET), "{rendered}");

    let mut playing = now_playing_row();
    playing["name"] = json!(SECRET);
    let page = decode(&rows(vec![playing])).unwrap();
    assert!(!format!("{:?}", page.now_playing().unwrap()).contains(SECRET));

    // Malformed values that carry the text.
    row["date"] = json!({"uts": SECRET});
    let mut broken = [row.clone()];
    broken[0]["mbid"] = json!({"x": SECRET});
    for body in [
        rows(vec![row]),
        rows(broken.to_vec()),
        page_with(Some(json!(SECRET)), attr()),
        page_with(
            Some(json!([])),
            json!({"user": "rj", "page": SECRET, "perPage": "1", "totalPages": "1", "total": "1"}),
        ),
    ] {
        let error = decode_error(&body);
        assert!(!error.to_string().contains(SECRET), "{error}");
        assert!(!format!("{error:?}").contains(SECRET), "{error:?}");
    }
}

#[test]
fn model_types_are_send_sync_and_clone() {
    fn assert_traits<T: Send + Sync + Clone + std::fmt::Debug>() {}
    assert_traits::<RecentTracksPage>();
    assert_traits::<PageAttr>();
    assert_traits::<Scrobble>();
    assert_traits::<NowPlaying>();
    assert_traits::<Track>();
    assert_traits::<Artist>();
    assert_traits::<Album>();
}
