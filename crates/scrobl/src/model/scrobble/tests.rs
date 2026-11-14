#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]

use serde_json::{Value, json};

use super::*;
use crate::protocol::{self, HttpResponse, Request};
use crate::{Delivery, ErrorKind};

// provenance: derived from the shape on the official pages (see
// fixtures/README.md); the rest synthetic.

const ONE: &[u8] = include_bytes!("../../../fixtures/derived/scrobble-one.json");
const BATCH: &[u8] = include_bytes!("../../../fixtures/derived/scrobble-batch.json");
const NOW: &[u8] = include_bytes!("../../../fixtures/derived/now-playing.json");
const REORDERED: &[u8] = include_bytes!("../../../fixtures/derived/scrobble-reordered.json");

fn sent(plays: &[(&str, &str, u64)]) -> Request {
    let mut request = Request::new(&methods::TRACK_SCROBBLE);
    for (i, (artist, track, t)) in plays.iter().enumerate() {
        request = request
            .indexed("artist", i, *artist)
            .indexed("track", i, *track)
            .indexed("timestamp", i, *t);
    }
    request
}

fn named(plays: &[(&str, &str, u64)], body: impl Into<Vec<u8>>) -> Result<ScrobbleReport, Error> {
    let raw = protocol::decode(&sent(plays), HttpResponse::new(200, body.into())).unwrap();
    track_scrobble(&raw)
}

/// Plays of artist `A`, track `T`.
fn scrobble(timestamps: &[u64], body: impl Into<Vec<u8>>) -> Result<ScrobbleReport, Error> {
    let plays: Vec<_> = timestamps.iter().map(|t| ("A", "T", *t)).collect();
    named(&plays, body)
}

fn row(timestamp: u64, code: u32) -> Value {
    track_row("T", timestamp, code)
}

fn track_row(track: &str, timestamp: u64, code: u32) -> Value {
    json!({
        "artist": {"corrected": "0", "#text": "A"},
        "track": {"corrected": "0", "#text": track},
        "ignoredMessage": {"code": code.to_string(), "#text": ""},
        "timestamp": timestamp.to_string()
    })
}

fn reply(rows: Value, accepted: u64, ignored: u64) -> Vec<u8> {
    json!({"scrobbles": {"scrobble": rows, "@attr": {"accepted": accepted, "ignored": ignored}}})
        .to_string()
        .into_bytes()
}

#[test]
fn one_accepted_scrobble_is_read() {
    let report = named(&[("Cher", "Believe", 1_287_140_447)], ONE).unwrap();
    assert_eq!((report.accepted(), report.ignored()), (1, 0));
    let outcome = &report.outcomes()[0];
    assert!(outcome.is_accepted());
    assert_eq!(outcome.artist().name(), "Cher");
    assert!(!outcome.artist().corrected());
    assert_eq!(outcome.album().unwrap().name(), "Believe");
    assert_eq!(outcome.timestamp(), Some(1_287_140_447));
}

#[test]
fn a_partly_ignored_batch_is_read_with_its_corrections() {
    let report = named(
        &[
            ("Guns and Roses", "Paradise City", 1_287_140_447),
            ("Cher", "Believe", 1_000_000_000),
        ],
        BATCH,
    )
    .unwrap();
    assert_eq!((report.accepted(), report.ignored()), (1, 1));
    let [first, second] = report.outcomes() else {
        panic!("two outcomes")
    };
    assert!(first.artist().corrected());
    assert_eq!(first.artist().name(), "Guns N' Roses");
    assert!(first.album().is_none());
    let ignored = second.ignored().unwrap();
    assert_eq!(ignored.code(), IgnoredCode::TOO_OLD);
    assert_eq!(ignored.message(), "Timestamp failed filter");
}

#[test]
fn both_spellings_of_the_ignored_message_are_read() {
    let mut row = row(5, 2);
    let message = row
        .as_object_mut()
        .unwrap()
        .remove("ignoredMessage")
        .unwrap();
    row.as_object_mut()
        .unwrap()
        .insert("ignoredmessage".into(), message);
    let report = scrobble(&[5], reply(row, 0, 1)).unwrap();
    assert_eq!(
        report.outcomes()[0].ignored().unwrap().code(),
        IgnoredCode::TRACK
    );
}

#[test]
fn a_reply_that_does_not_account_for_every_play_is_unknown_delivery() {
    let cases = [
        // One row for two plays.
        (vec![1, 2], reply(row(1, 0), 1, 1)),
        // Counts that do not add up to the batch.
        (vec![1, 2], reply(json!([row(1, 0), row(2, 0)]), 1, 0)),
        // Counts that disagree with the rows.
        (vec![1, 2], reply(json!([row(1, 0), row(2, 0)]), 1, 1)),
        // A timestamp not sent.
        (vec![1, 2], reply(json!([row(1, 0), row(3, 0)]), 2, 0)),
        // One play answered twice.
        (vec![1, 2], reply(json!([row(1, 0), row(1, 0)]), 2, 0)),
    ];
    for (timestamps, body) in cases {
        let error = scrobble(&timestamps, body).unwrap_err();
        assert_eq!(error.kind(), ErrorKind::Inconsistent, "{error}");
        assert_eq!(error.delivery(), Some(Delivery::Unknown));
        assert!(error.body().is_some());
    }
    let error = scrobble(&[1], br#"{"scrobbles": {}}"#.to_vec()).unwrap_err();
    assert_eq!(error.kind(), ErrorKind::Decode);
    assert_eq!(error.delivery(), Some(Delivery::Unknown));
}

#[test]
fn a_reply_listing_accepted_plays_first_is_put_in_the_order_sent() {
    let plays = [1_000_000_000, 1_700_000_000, 1_000_000_100].map(|t| ("Cher", "Believe", t));
    let report = named(&plays, REORDERED).unwrap();
    let outcomes = report.outcomes();
    let stamps: Vec<_> = outcomes.iter().map(Outcome::timestamp).collect();
    assert_eq!(
        stamps,
        [
            Some(1_000_000_000),
            Some(1_700_000_000),
            Some(1_000_000_100)
        ]
    );
    assert!(outcomes[1].is_accepted());
    assert_eq!(outcomes[1].album().unwrap().name(), "Believe");
    for old in [&outcomes[0], &outcomes[2]] {
        assert_eq!(old.ignored().unwrap().code(), IgnoredCode::ARTIST);
        assert!(old.album().is_none());
    }
}

#[test]
fn outcomes_are_matched_by_names_as_well_as_timestamps() {
    let plays = [
        ("A", "Blocked", 100),
        ("A", "Middle", 200),
        ("A", "Good", 100),
    ];
    let body = reply(
        json!([
            track_row("Middle", 200, 0),
            track_row("Good", 100, 0),
            track_row("Blocked", 100, 2)
        ]),
        2,
        1,
    );
    let report = named(&plays, body).unwrap();
    let names: Vec<_> = report
        .outcomes()
        .iter()
        .map(|o| (o.track().name(), o.is_accepted()))
        .collect();
    assert_eq!(
        names,
        [("Blocked", false), ("Middle", true), ("Good", true)]
    );
}

#[test]
fn an_outcome_that_fits_no_play_or_several_is_unknown_delivery() {
    let corrected = json!({
        "artist": {"corrected": "0", "#text": "A"},
        "track": {"corrected": "1", "#text": "X"},
        "ignoredMessage": {"code": "0", "#text": ""},
        "timestamp": "100"
    });
    let cases = [
        // One play answered twice, another not at all.
        (
            vec![("A", "A", 100), ("A", "C", 200), ("A", "B", 100)],
            reply(
                json!([
                    track_row("C", 200, 0),
                    track_row("A", 100, 0),
                    track_row("A", 100, 0)
                ]),
                3,
                0,
            ),
        ),
        // Names that match no play at that time.
        (
            vec![("A", "A", 100), ("A", "B", 200)],
            reply(
                json!([track_row("B", 200, 0), track_row("B", 100, 0)]),
                2,
                0,
            ),
        ),
        // A corrected name that could be either play.
        (
            vec![("A", "P", 100), ("A", "Q", 100)],
            reply(json!([corrected.clone(), corrected]), 2, 0),
        ),
    ];
    for (plays, body) in cases {
        let error = named(&plays, body).unwrap_err();
        assert_eq!(error.kind(), ErrorKind::Inconsistent, "{error}");
        assert_eq!(error.delivery(), Some(Delivery::Unknown));
    }
}

#[test]
fn identical_plays_take_their_outcomes_in_the_reply_order() {
    let report = scrobble(&[100, 100], reply(json!([row(100, 0), row(100, 5)]), 1, 1)).unwrap();
    let accepted: Vec<_> = report.outcomes().iter().map(Outcome::is_accepted).collect();
    assert_eq!(accepted, [true, false]);
}

#[test]
fn now_playing_is_read() {
    let request = Request::new(&methods::TRACK_UPDATE_NOW_PLAYING);
    let raw = protocol::decode(&request, HttpResponse::new(200, NOW)).unwrap();
    let outcome = track_update_now_playing(&raw).unwrap();
    assert!(outcome.is_accepted());
    assert_eq!(outcome.track().name(), "Believe");
    assert_eq!(outcome.timestamp(), None);
    assert!(outcome.album_artist().is_none());
}

#[test]
fn debug_shows_no_name_and_no_message() {
    let report = named(
        &[
            ("Guns and Roses", "Paradise City", 1_287_140_447),
            ("Cher", "Believe", 1_000_000_000),
        ],
        BATCH,
    )
    .unwrap();
    let debug = format!("{report:?} {:?}", report.outcomes());
    for text in ["Guns", "Cher", "Believe", "Timestamp failed"] {
        assert!(!debug.contains(text), "{debug}");
    }
}

#[test]
fn a_malformed_row_is_an_error_naming_the_field() {
    let mut cases = Vec::new();
    for (key, value, field) in [
        ("artist", json!(null), "artist"),
        ("artist", json!("A"), "artist"),
        (
            "track",
            json!({"corrected": "x", "#text": "T"}),
            "track.corrected",
        ),
        ("ignoredMessage", json!("0"), "ignoredMessage"),
        (
            "ignoredMessage",
            json!({"code": 4_294_967_296_u64}),
            "ignoredMessage.code",
        ),
        ("timestamp", json!("soon"), "timestamp"),
    ] {
        let mut row = row(1, 0);
        let map = row.as_object_mut().unwrap();
        if value.is_null() {
            map.remove(key);
        } else {
            map.insert(key.into(), value);
        }
        cases.push((row, field));
    }
    let mut no_message = row(1, 0);
    no_message.as_object_mut().unwrap().remove("ignoredMessage");
    cases.push((no_message, "ignoredMessage"));
    for (row, field) in cases {
        let error = scrobble(&[1], reply(row, 1, 0)).unwrap_err();
        assert_eq!(error.kind(), ErrorKind::Decode, "{field}");
        assert!(error.to_string().contains(field), "{field}: {error}");
    }
}

fn request_with(params: &[(&str, &str)]) -> Request {
    params
        .iter()
        .fold(Request::new(&methods::TRACK_SCROBBLE), |r, (n, v)| {
            r.param(n, *v)
        })
}

#[test]
fn a_request_with_a_gap_or_a_bad_timestamp_is_never_accounted_for() {
    let gap = request_with(&[
        ("artist[0]", "a"),
        ("track[0]", "t"),
        ("timestamp[0]", "1"),
        ("artist[2]", "a"),
        ("track[2]", "t"),
        ("timestamp[2]", "2"),
    ]);
    let bad = request_with(&[
        ("artist[0]", "a"),
        ("track[0]", "t"),
        ("timestamp[0]", "soon"),
    ]);
    for request in [gap, bad] {
        let body = reply(row(1, 0), 1, 0);
        let raw = protocol::decode(&request, HttpResponse::new(200, body)).unwrap();
        let error = track_scrobble(&raw).unwrap_err();
        assert_eq!(error.kind(), ErrorKind::Inconsistent, "{error}");
        assert_eq!(error.delivery(), Some(Delivery::Unknown));
    }
}

#[test]
fn counts_out_of_range_or_given_as_strings_are_compared_not_added() {
    let huge = json!({"scrobbles": {"scrobble": row(1, 0),
        "@attr": {"accepted": u64::MAX, "ignored": 1}}});
    let error = scrobble(&[1], huge.to_string()).unwrap_err();
    assert_eq!(error.kind(), ErrorKind::Inconsistent);
    let strings = json!({"scrobbles": {"scrobble": row(1, 0),
        "@attr": {"accepted": "1", "ignored": "0"}}});
    assert_eq!(scrobble(&[1], strings.to_string()).unwrap().accepted(), 1);
}

#[test]
fn both_spellings_at_once_are_an_error() {
    let mut row = row(1, 0);
    row.as_object_mut()
        .unwrap()
        .insert("ignoredmessage".into(), json!({"code": "3", "#text": ""}));
    let error = scrobble(&[1], reply(row, 1, 0)).unwrap_err();
    assert_eq!(error.kind(), ErrorKind::Decode);
    assert_eq!(error.delivery(), Some(Delivery::Unknown));
}

#[test]
fn an_unknown_ignored_code_is_kept() {
    let report = scrobble(&[1], reply(row(1, 9), 0, 1)).unwrap();
    let ignored = report.outcomes()[0].ignored().unwrap();
    assert_eq!(ignored.code().get(), 9);
    assert_eq!(format!("{ignored:?}"), "Ignored(IgnoredCode(9))");
}
