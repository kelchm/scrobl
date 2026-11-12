#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]

use std::time::Duration;

use serde_json::{Value, json};

use super::*;
use crate::ErrorKind;
use crate::model::decode;
use crate::protocol::{self, HttpResponse, MethodSpec, Raw, Request, methods};

// Fixtures. Provenance for each:
//
// - `derived/album-info.json` follows a response recorded on 2026-11-11,
//   with the owner's own play count: every member and value is kept but
//   the two texts that users of the service wrote, which are invented.
//   derived.
// - `album-info-small.json` and `album-top-tags.json` were recorded by the
//   repository owner the same day from what the service shows anyone, and
//   `kelchm-album-tags-empty.json` is the owner's own tags of an album,
//   which are none. All three are kept byte for byte. recorded.
// - `derived/album-tags.json` has the shape of that last reply with rows
//   added as `artist.getTags` sends them, every value invented. derived.
// - Everything else is built here from those shapes. synthetic.

const INFO: &[u8] = include_bytes!("../../../fixtures/derived/album-info.json");
const SMALL: &[u8] = include_bytes!("../../../fixtures/recorded/album-info-small.json");
const TOP_TAGS: &[u8] = include_bytes!("../../../fixtures/recorded/album-top-tags.json");
const USER_TAGS: &[u8] = include_bytes!("../../../fixtures/derived/album-tags.json");
const NO_USER_TAGS: &[u8] =
    include_bytes!("../../../fixtures/recorded/kelchm-album-tags-empty.json");

/// A string that appears in no error and no `Debug` output.
const SECRET: &str = "SYNTHETIC-RESPONSE-TEXT";

fn raw(spec: &'static MethodSpec, body: impl Into<Vec<u8>>) -> Raw {
    let request = Request::new(spec)
        .param("artist", "radiohead")
        .param("album", "ok computer");
    protocol::decode(&request, HttpResponse::new(200, body.into())).unwrap()
}

fn info(album: &Value) -> Result<AlbumInfo, Error> {
    let body = json!({ "album": album });
    decode::album_get_info(&raw(&methods::ALBUM_GET_INFO, body.to_string()))
}

/// The least an `album.getInfo` reply can hold.
fn bare_album() -> Value {
    json!({"name": "B", "artist": "A", "listeners": "3", "playcount": "9"})
}

fn track_row(name: &str) -> Value {
    json!({"name": name, "duration": 61, "@attr": {"rank": 1},
           "artist": {"name": "A", "url": "https://example.invalid/a"}})
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
fn the_derived_info_of_an_album_decodes() {
    let info = decode::album_get_info(&raw(&methods::ALBUM_GET_INFO, INFO)).unwrap();
    let album = info.album();
    assert_eq!(album.name(), "OK Computer");
    assert!(album.mbid().is_some());
    assert_eq!(album.images().len(), 6);
    // The reply names the artist with a string: a name and nothing else.
    let artist = album.artist().unwrap();
    assert_eq!(
        (artist.name(), artist.mbid(), artist.url()),
        ("Radiohead", None, None)
    );
    assert_eq!(artist.json(), &json!("Radiohead"));
    assert_eq!(
        (info.listeners(), info.playcount(), info.user_playcount()),
        (4_954_748, 267_796_974, Some(5))
    );
    assert_eq!(
        (album.listeners(), album.playcount()),
        (Some(4_954_748), Some(267_796_974))
    );

    assert_eq!(info.tracks().len(), 12);
    let first = &info.tracks()[0];
    assert_eq!(
        (first.name(), first.duration(), first.artist().name()),
        ("Airbag", Some(Duration::from_secs(284)), "Radiohead")
    );
    assert!(first.artist().mbid().is_some());
    assert_eq!(info.tracks()[1].name(), "Paranoid Android");
    let tags: Vec<_> = info.tags().iter().map(Tag::name).collect();
    assert_eq!(
        tags,
        [
            "alternative",
            "alternative rock",
            "rock",
            "radiohead",
            "indie"
        ]
    );
    assert_eq!(info.published(), Some("11 Nov 2022, 15:31"));
    assert!(info.summary().unwrap().starts_with("Invented text"));
    assert!(info.content().is_some());
    assert_eq!(info.json()["userplaycount"], 5);
    let album: &Album = info.as_ref();
    assert_eq!(album.name(), "OK Computer");
    assert_eq!(info.into_album().name(), "OK Computer");
}

#[test]
fn the_recorded_info_of_a_small_album_decodes() {
    // No description, no id, `tags` an empty string, and nobody asked for.
    let info = decode::album_get_info(&raw(&methods::ALBUM_GET_INFO, SMALL)).unwrap();
    assert_eq!(info.album().name(), "68");
    assert_eq!(info.album().mbid(), None);
    assert_eq!((info.listeners(), info.playcount()), (11_251, 61_117));
    assert_eq!(info.user_playcount(), None);
    assert!(info.tags().is_empty());
    assert_eq!(
        (info.summary(), info.content(), info.published()),
        (None, None, None)
    );
    let tracks: Vec<_> = info
        .tracks()
        .iter()
        .map(|track| (track.name(), track.duration().map(|d| d.as_secs())))
        .collect();
    assert_eq!(
        tracks,
        [("68", Some(207)), ("68 (Behind The Song)", Some(74))]
    );
}

#[test]
fn tracks_and_tags_may_be_one_object_or_none_at_all() {
    let info = self::info(&bare_album()).unwrap();
    assert!(info.tracks().is_empty() && info.tags().is_empty());

    let mut album = bare_album();
    album["tracks"] = json!({"track": track_row("Only")});
    album["tags"] = json!({"tag": []});
    album["userplaycount"] = json!(0);
    let info = self::info(&album).unwrap();
    assert_eq!(info.tracks()[0].name(), "Only");
    assert_eq!(info.user_playcount(), Some(0));
    // A length of 0 is one the service does not know.
    let mut unknown = track_row("T");
    unknown["duration"] = json!(0);
    album["tracks"] = json!({"track": [unknown]});
    assert_eq!(self::info(&album).unwrap().tracks()[0].duration(), None);
}

#[test]
fn a_malformed_info_names_the_field() {
    names(
        decode::album_get_info(&raw(&methods::ALBUM_GET_INFO, r#"{"other": 1}"#)),
        "album",
    );
    for member in ["name", "artist", "listeners", "playcount"] {
        let mut album = bare_album();
        album.as_object_mut().unwrap().remove(member);
        names(info(&album), &format!("album.{member}"));
        let mut album = bare_album();
        album[member] = json!([SECRET]);
        names(info(&album), &format!("album.{member}"));
    }
    let mut album = bare_album();
    album["artist"] = json!("");
    names(info(&album), "album.artist");
    let mut album = bare_album();
    album["userplaycount"] = json!(SECRET);
    names(info(&album), "album.userplaycount");

    let mut album = bare_album();
    album["tracks"] = json!({"track": [track_row("A"), {"name": "B"}]});
    names(info(&album), "album.tracks.track[1].artist");
    let mut long = track_row("A");
    long["duration"] = json!(SECRET);
    album["tracks"] = json!({"track": [long]});
    names(info(&album), "album.tracks.track[0].duration");
    album["tracks"] = json!(7);
    names(info(&album), "album.tracks");
    let mut album = bare_album();
    album["tags"] = json!({"tag": [{"url": SECRET}]});
    names(info(&album), "album.tags.tag[0].name");
    let mut album = bare_album();
    album["wiki"] = json!([SECRET]);
    names(info(&album), "album.wiki");
}

#[test]
fn the_recorded_top_tags_of_an_album_decode() {
    let tags = decode::album_get_top_tags(&raw(&methods::ALBUM_GET_TOP_TAGS, TOP_TAGS)).unwrap();
    assert_eq!(tags.subject(), Some("OK Computer"));
    assert_eq!(tags.len(), 10);
    assert_eq!(
        (tags[0].tag().name(), tags[0].count()),
        ("alternative rock", 100)
    );
    assert_eq!((tags[1].tag().name(), tags[1].count()), ("alternative", 66));
}

#[test]
fn the_tags_of_a_user_decode_and_none_is_an_empty_list() {
    let tags = decode::album_get_tags(&raw(&methods::ALBUM_GET_TAGS, USER_TAGS)).unwrap();
    assert_eq!(tags.subject(), Some("Synthetic Album"));
    let names_of: Vec<_> = tags.iter().map(Tag::name).collect();
    assert_eq!(names_of, ["synthetic tag", "another"]);

    let none = decode::album_get_tags(&raw(&methods::ALBUM_GET_TAGS, NO_USER_TAGS)).unwrap();
    assert!(none.is_empty());
    assert_eq!(none.subject(), Some("OK Computer"));

    // The album is what the list is about, so its name must be there.
    let body = json!({"tags": {"tag": [], "@attr": {"artist": "A"}}});
    names(
        decode::album_get_tags(&raw(&methods::ALBUM_GET_TAGS, body.to_string())),
        "tags.@attr.album",
    );
    let body = json!({"toptags": {"tag": [{"name": "t"}], "@attr": {"album": "B"}}});
    names(
        decode::album_get_top_tags(&raw(&methods::ALBUM_GET_TOP_TAGS, body.to_string())),
        "toptags.tag[0].count",
    );
}

#[test]
fn a_response_to_another_method_is_refused() {
    let other = raw(&methods::ALBUM_GET_TOP_TAGS, TOP_TAGS);
    for error in [
        decode::album_get_info(&other).map(drop).unwrap_err(),
        decode::album_get_tags(&other).map(drop).unwrap_err(),
        decode::album_get_top_tags(&raw(&methods::ALBUM_GET_TAGS, USER_TAGS))
            .map(drop)
            .unwrap_err(),
    ] {
        assert_eq!(error.kind(), ErrorKind::Decode);
        assert!(error.to_string().contains("different method"));
    }
}

#[test]
fn debug_leaves_out_the_text_of_the_response() {
    let mut album = bare_album();
    album["name"] = json!(SECRET);
    album["artist"] = json!(SECRET);
    album["tracks"] = json!({"track": [track_row(SECRET)]});
    album["tags"] = json!({"tag": [{"name": SECRET}]});
    album["wiki"] = json!({"summary": SECRET, "content": SECRET, "published": SECRET});
    let info = info(&album).unwrap();
    let shown = format!(
        "{info:?} {:?} {:?} {:?}",
        info.album(),
        info.tracks(),
        info.tags()
    );
    assert!(!shown.contains(SECRET), "{shown}");
    assert!(shown.contains("tracks: 1"), "{shown}");
}
