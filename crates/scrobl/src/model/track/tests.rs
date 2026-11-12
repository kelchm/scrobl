#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing,
    clippy::float_cmp
)]

use std::time::Duration;

use serde_json::{Value, json};

use super::*;
use crate::ErrorKind;
use crate::model::decode;
use crate::protocol::{self, HttpResponse, MethodSpec, Raw, Request, methods};

// Fixtures. Provenance for each:
//
// - `derived/track-info.json` follows a response recorded on 2026-11-12,
//   with the owner's own play count: every member and value is kept but
//   the two texts that users of the service wrote, which are invented.
//   derived.
// - `kelchm-track-info.json` is a track nobody has described, asked for
//   with the owner's name, and `kelchm-track-tags.json` and
//   `kelchm-track-tags-empty.json` are the owner's own tags of two tracks.
//   `track-correction.json`, `track-similar.json` and `track-top-tags.json`
//   are what the service shows anyone. All were recorded by the repository
//   owner on 2026-11-12 and are kept byte for byte. recorded.
// - Everything else is built here from those shapes. synthetic.

const INFO: &[u8] = include_bytes!("../../../fixtures/derived/track-info.json");
const OWN_INFO: &[u8] = include_bytes!("../../../fixtures/recorded/kelchm-track-info.json");
const CORRECTION: &[u8] = include_bytes!("../../../fixtures/recorded/track-correction.json");
const SIMILAR: &[u8] = include_bytes!("../../../fixtures/recorded/track-similar.json");
const TOP_TAGS: &[u8] = include_bytes!("../../../fixtures/recorded/track-top-tags.json");
const USER_TAGS: &[u8] = include_bytes!("../../../fixtures/recorded/kelchm-track-tags.json");
const NO_USER_TAGS: &[u8] =
    include_bytes!("../../../fixtures/recorded/kelchm-track-tags-empty.json");

/// A string that appears in no error and no `Debug` output.
const SECRET: &str = "SYNTHETIC-RESPONSE-TEXT";

fn raw(spec: &'static MethodSpec, body: impl Into<Vec<u8>>) -> Raw {
    let request = Request::new(spec)
        .param("artist", "radiohead")
        .param("track", "creep");
    protocol::decode(&request, HttpResponse::new(200, body.into())).unwrap()
}

fn info(track: &Value) -> Result<TrackInfo, Error> {
    let body = json!({ "track": track });
    decode::track_get_info(&raw(&methods::TRACK_GET_INFO, body.to_string()))
}

/// The least a `track.getInfo` reply can hold.
fn bare_track() -> Value {
    json!({"name": "T", "listeners": "3", "playcount": "9", "artist": {"name": "A"}})
}

fn similar(rows: Value) -> Result<List<SimilarTrack>, Error> {
    let body = json!({"similartracks": {"track": rows,
                                         "@attr": {"artist": "A", "track": "T"}}});
    decode::track_get_similar(&raw(&methods::TRACK_GET_SIMILAR, body.to_string()))
}

fn similar_row(name: &str) -> Value {
    json!({"name": name, "playcount": 7, "match": 0.5, "duration": 61,
           "artist": {"name": "A"}})
}

fn correction(body: &Value) -> Result<Track, Error> {
    decode::track_get_correction(&raw(&methods::TRACK_GET_CORRECTION, body.to_string()))
}

/// The message of a decode error, which must name `field` and repeat no
/// response text.
fn names<T>(result: Result<T, Error>, field: &str) {
    let Err(error) = result else {
        panic!("{field}: decoded");
    };
    assert_eq!(error.kind(), ErrorKind::Decode);
    let shown = format!("{error} {error:?}");
    assert!(shown.contains(field), "{shown} does not name {field}");
    assert!(!shown.contains(SECRET), "{shown}");
}

#[test]
fn the_derived_info_of_a_track_decodes() {
    let info = decode::track_get_info(&raw(&methods::TRACK_GET_INFO, INFO)).unwrap();
    let track = info.track();
    assert_eq!(track.name(), "Creep");
    assert!(track.mbid().is_some());
    // This reply gives the length in milliseconds.
    assert_eq!(track.duration(), Some(Duration::from_secs(237)));
    assert_eq!(track.artist().name(), "Radiohead");
    assert!(track.artist().mbid().is_some());

    // The album is named by a `title`, with a link, images and its
    // artist's name.
    let album = track.album().unwrap();
    assert_eq!(album.name(), "Pablo Honey");
    assert_eq!(
        album.url(),
        Some("https://www.last.fm/music/Radiohead/Pablo+Honey")
    );
    assert_eq!(album.images().len(), 4);
    assert_eq!(
        album.artist().map(|artist| artist.name()),
        Some("Radiohead")
    );

    assert_eq!(
        (info.listeners(), info.playcount()),
        (4_267_814, 64_205_671)
    );
    assert_eq!(
        (track.listeners(), track.playcount()),
        (Some(4_267_814), Some(64_205_671))
    );
    assert_eq!(
        (info.user_playcount(), info.user_loved()),
        (Some(10), Some(false))
    );
    let tags: Vec<_> = info.tags().iter().map(Tag::name).collect();
    assert_eq!(
        tags,
        [
            "alternative",
            "alternative rock",
            "rock",
            "indie",
            "radiohead"
        ]
    );
    assert_eq!(info.published(), Some("18 Jul 2008, 15:22"));
    assert!(info.summary().unwrap().starts_with("Invented text"));
    assert!(info.content().is_some());
    assert_eq!(info.json()["userloved"], "0");
    let track: &Track = info.as_ref();
    assert_eq!(track.name(), "Creep");
    assert_eq!(info.into_track().name(), "Creep");
}

#[test]
fn the_recorded_info_of_a_track_nobody_described_decodes() {
    let info = decode::track_get_info(&raw(&methods::TRACK_GET_INFO, OWN_INFO)).unwrap();
    assert_eq!(info.track().name(), "Bedroom in Hell");
    assert_eq!(info.track().duration(), Some(Duration::from_secs(338)));
    assert_eq!(
        info.track().album().map(|album| album.name()),
        Some("Summercamp Nightmare")
    );
    assert_eq!(
        (info.user_playcount(), info.user_loved()),
        (Some(1), Some(false))
    );
    assert!(info.tags().is_empty());
    assert_eq!(
        (info.summary(), info.content(), info.published()),
        (None, None, None)
    );
}

#[test]
fn an_info_with_nothing_optional_decodes() {
    let info = self::info(&bare_track()).unwrap();
    assert_eq!((info.listeners(), info.playcount()), (3, 9));
    assert_eq!((info.user_playcount(), info.user_loved()), (None, None));
    assert!(info.track().album().is_none() && info.tags().is_empty());
    assert_eq!(info.track().duration(), None);

    // A length of 0 is one the service does not know, an album without a
    // title is none, and a loved track says so.
    let mut track = bare_track();
    track["duration"] = json!("0");
    track["album"] = json!({"artist": "A", "url": "https://example.invalid/b"});
    track["userloved"] = json!("1");
    track["userplaycount"] = json!(0);
    track["toptags"] = json!({"tag": {"name": "only"}});
    let info = self::info(&track).unwrap();
    assert_eq!(info.track().duration(), None);
    assert!(info.track().album().is_none());
    assert_eq!(
        (info.user_playcount(), info.user_loved()),
        (Some(0), Some(true))
    );
    assert_eq!(info.tags()[0].name(), "only");
    track["duration"] = json!(1500);
    assert_eq!(
        self::info(&track).unwrap().track().duration(),
        Some(Duration::from_millis(1500))
    );
}

#[test]
fn a_malformed_info_names_the_field() {
    names(
        decode::track_get_info(&raw(&methods::TRACK_GET_INFO, r#"{"other": 1}"#)),
        "track",
    );
    for member in ["name", "listeners", "playcount", "artist"] {
        let mut track = bare_track();
        track.as_object_mut().unwrap().remove(member);
        names(info(&track), &format!("track.{member}"));
        let mut track = bare_track();
        track[member] = json!([SECRET]);
        names(info(&track), &format!("track.{member}"));
    }
    for (member, value, field) in [
        ("duration", json!(SECRET), "track.duration"),
        ("userplaycount", json!(SECRET), "track.userplaycount"),
        ("userloved", json!("2"), "track.userloved"),
        ("album", json!(SECRET), "track.album"),
        ("album", json!({"title": [SECRET]}), "track.album.title"),
        (
            "album",
            json!({"title": "B", "artist": {"name": SECRET}}),
            "track.album.artist",
        ),
        ("toptags", json!([SECRET]), "track.toptags"),
        (
            "toptags",
            json!({"tag": [{"url": SECRET}]}),
            "track.toptags.tag[0].name",
        ),
        ("wiki", json!(SECRET), "track.wiki"),
    ] {
        let mut track = bare_track();
        track[member] = value;
        names(info(&track), field);
    }
}

#[test]
fn the_recorded_correction_decodes() {
    let track =
        decode::track_get_correction(&raw(&methods::TRACK_GET_CORRECTION, CORRECTION)).unwrap();
    assert_eq!(track.name(), "Paradise City");
    assert_eq!(track.artist().name(), "Guns N' Roses");
    assert!(track.mbid().is_some());
    assert_eq!(
        (track.duration(), track.listeners(), track.album().is_none()),
        (None, None, true)
    );

    names(correction(&json!({"other": SECRET})), "corrections");
    names(
        correction(&json!({"corrections": {"correction": [SECRET]}})),
        "corrections.correction",
    );
    names(
        correction(&json!({"corrections": {"correction": {"track": SECRET}}})),
        "corrections.correction.track",
    );
    names(
        correction(&json!({"corrections": {"correction": {"track": {"name": "T"}}}})),
        "corrections.correction.track.artist",
    );
    names(
        correction(&json!({"corrections": {"correction": {
            "track": {"artist": {"name": "A"}}}}})),
        "corrections.correction.track.name",
    );
}

#[test]
fn the_recorded_similar_tracks_decode() {
    let list = decode::track_get_similar(&raw(&methods::TRACK_GET_SIMILAR, SIMILAR)).unwrap();
    assert_eq!(list.subject(), Some("Creep"));
    let rows: Vec<_> = list
        .iter()
        .map(|row| {
            (
                row.track().name(),
                row.track().artist().name(),
                row.similarity(),
                row.track().playcount(),
                row.track().duration().map(|length| length.as_secs()),
            )
        })
        .collect();
    // This reply sends the likeness and the counts as JSON numbers, and
    // the length in seconds.
    assert_eq!(
        rows,
        [
            (
                "No Surprises",
                "Radiohead",
                1.0,
                Some(57_814_442),
                Some(269)
            ),
            (
                "Let Down",
                "Radiohead",
                0.880_215,
                Some(44_223_042),
                Some(299)
            ),
            (
                "Everlong",
                "Foo Fighters",
                0.513_825,
                Some(49_152_390),
                Some(334)
            )
        ]
    );
    let first = list[0].clone();
    assert_eq!(first.track().listeners(), None);
    assert_eq!(first.track().images().len(), 6);
    assert_eq!(first.json()["match"], 1.0);
    let track: &Track = first.as_ref();
    assert_eq!(track.name(), "No Surprises");
    assert_eq!(first.into_track().name(), "No Surprises");
}

#[test]
fn similar_tracks_may_be_one_none_or_malformed() {
    assert!(similar(json!([])).unwrap().is_empty());
    let one = similar(similar_row("Only")).unwrap();
    assert_eq!(one[0].track().name(), "Only");
    let mut unknown = similar_row("T");
    unknown["duration"] = json!(0);
    unknown["match"] = json!("0.25");
    let list = similar(json!([unknown])).unwrap();
    assert_eq!(
        (list[0].track().duration(), list[0].similarity()),
        (None, 0.25)
    );

    for (member, field) in [
        ("match", "similartracks.track[0].match"),
        ("playcount", "similartracks.track[0].playcount"),
        ("artist", "similartracks.track[0].artist"),
        ("name", "similartracks.track[0].name"),
    ] {
        let mut row = similar_row("T");
        row.as_object_mut().unwrap().remove(member);
        names(similar(json!([row])), field);
        let mut row = similar_row("T");
        row[member] = json!([SECRET]);
        names(similar(json!([row])), field);
    }
    let body = json!({"similartracks": {"track": [], "@attr": {"artist": "A"}}});
    names(
        decode::track_get_similar(&raw(&methods::TRACK_GET_SIMILAR, body.to_string())),
        "similartracks.@attr.track",
    );
}

#[test]
fn the_recorded_tags_of_a_track_decode() {
    let top = decode::track_get_top_tags(&raw(&methods::TRACK_GET_TOP_TAGS, TOP_TAGS)).unwrap();
    assert_eq!(top.subject(), Some("Creep"));
    assert_eq!(top.len(), 10);
    assert_eq!((top[0].tag().name(), top[0].count()), ("alternative", 100));

    // The owner's own tag of one track, and of another none.
    let own = decode::track_get_tags(&raw(&methods::TRACK_GET_TAGS, USER_TAGS)).unwrap();
    assert_eq!(own.subject(), Some("Bedroom in Hell"));
    assert_eq!(own.len(), 1);
    assert_eq!(own[0].name(), "interesting");
    let none = decode::track_get_tags(&raw(&methods::TRACK_GET_TAGS, NO_USER_TAGS)).unwrap();
    assert!(none.is_empty());
    assert_eq!(none.subject(), Some("Creep"));

    let body = json!({"tags": {"tag": [{"url": SECRET}],
                                "@attr": {"artist": "A", "track": "T"}}});
    names(
        decode::track_get_tags(&raw(&methods::TRACK_GET_TAGS, body.to_string())),
        "tags.tag[0].name",
    );
    let body = json!({"toptags": {"tag": [], "@attr": {"artist": "A"}}});
    names(
        decode::track_get_top_tags(&raw(&methods::TRACK_GET_TOP_TAGS, body.to_string())),
        "toptags.@attr.track",
    );
}

#[test]
fn a_response_to_another_method_is_refused() {
    let other = raw(&methods::TRACK_GET_TOP_TAGS, TOP_TAGS);
    for error in [
        decode::track_get_info(&other).map(drop).unwrap_err(),
        decode::track_get_correction(&other).map(drop).unwrap_err(),
        decode::track_get_similar(&other).map(drop).unwrap_err(),
        decode::track_get_tags(&other).map(drop).unwrap_err(),
        decode::track_get_top_tags(&raw(&methods::TRACK_GET_TAGS, USER_TAGS))
            .map(drop)
            .unwrap_err(),
    ] {
        assert_eq!(error.kind(), ErrorKind::Decode);
        assert!(error.to_string().contains("different method"));
    }
}

#[test]
fn debug_leaves_out_the_text_of_the_response() {
    let mut track = bare_track();
    track["name"] = json!(SECRET);
    track["artist"] = json!({"name": SECRET});
    track["album"] = json!({"title": SECRET, "artist": SECRET});
    track["toptags"] = json!({"tag": [{"name": SECRET}]});
    track["wiki"] = json!({"summary": SECRET, "content": SECRET, "published": SECRET});
    let info = info(&track).unwrap();
    let mut row = similar_row(SECRET);
    row["artist"] = json!({"name": SECRET});
    let similar = similar(json!([row])).unwrap();
    let shown = format!(
        "{info:?} {:?} {:?} {similar:?} {:?}",
        info.track(),
        info.track().album(),
        similar[0]
    );
    assert!(!shown.contains(SECRET), "{shown}");
    assert!(
        shown.contains("SimilarTrack { similarity: 0.5, .. }"),
        "{shown}"
    );
    assert!(shown.contains("user_loved: None"), "{shown}");
}
