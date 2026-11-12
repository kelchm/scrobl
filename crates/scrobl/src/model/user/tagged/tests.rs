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
// - The three `kelchm-tagged-*` bodies were recorded by the repository
//   owner from their own account on 2026-11-10 and are kept byte for byte.
//   The owner has tagged no album, so the album body is an empty page.
//   recorded.
// - `tagged-albums-page.json` follows a response recorded the same day for
//   another user's public tag: member names and order, nesting, value
//   types and the counts. Every value that says whose it is or what was
//   tagged is invented. derived.
// - Everything else is built here from those shapes. synthetic.

const ARTISTS: &[u8] = include_bytes!("../../../../fixtures/recorded/kelchm-tagged-artists.json");
const TRACKS: &[u8] = include_bytes!("../../../../fixtures/recorded/kelchm-tagged-tracks.json");
const NO_ALBUMS: &[u8] =
    include_bytes!("../../../../fixtures/recorded/kelchm-tagged-albums-empty.json");
const ALBUMS: &[u8] = include_bytes!("../../../../fixtures/derived/tagged-albums-page.json");

/// A string that appears in no error and no `Debug` output.
const SECRET: &str = "SYNTHETIC-RESPONSE-TEXT";

fn raw(kind: &str, body: impl Into<Vec<u8>>) -> Raw {
    let request = Request::new(&methods::USER_GET_PERSONAL_TAGS)
        .param("user", "rj")
        .param("tag", "rock")
        .param("taggingtype", kind);
    protocol::decode(&request, HttpResponse::new(200, body.into())).unwrap()
}

fn artists(body: &Value) -> Result<Page<TaggedArtist>, Error> {
    decode::user_get_personal_tags_artists(&raw("artist", body.to_string()))
}

fn tracks(body: &Value) -> Result<Page<TaggedTrack>, Error> {
    decode::user_get_personal_tags_tracks(&raw("track", body.to_string()))
}

fn taggings(kind: &str, key: &str, rows: Value, total: u64) -> Value {
    json!({"taggings": {kind: {key: rows}, "@attr": {
        "user": "rj", "tag": "rock", "page": "1", "perPage": "50",
        "totalPages": total.min(1).to_string(), "total": total.to_string()}}})
}

fn artist_row(name: &str) -> Value {
    json!({"name": name, "mbid": "", "url": "https://example.invalid/a", "streamable": "0"})
}

fn track_row(name: &str) -> Value {
    json!({"name": name, "duration": "FIXME", "mbid": "", "url": "https://example.invalid/t",
           "streamable": {"#text": "0", "fulltrack": "0"},
           "artist": {"name": "Artist", "mbid": "", "url": "https://example.invalid/a"}})
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
fn the_recorded_tagged_artists_decode() {
    let page = decode::user_get_personal_tags_artists(&raw("artist", ARTISTS)).unwrap();
    assert_eq!(page.subject(), Some("kelchm"));
    assert_eq!(
        (
            page.number(),
            page.per_page(),
            page.total_pages(),
            page.total()
        ),
        (1, 3, 1, 1)
    );
    let [only] = page.items() else {
        panic!("one artist expected");
    };
    assert_eq!(only.artist().name(), "Panic at the Disco");
    assert_eq!(only.artist().mbid(), None);
    assert_eq!(
        only.artist().url(),
        Some("https://www.last.fm/music/Panic+at+the+Disco")
    );
    assert_eq!(only.json()["streamable"], "0");
    let sizes: Vec<_> = only.artist().images().iter().map(|i| i.size()).collect();
    assert_eq!(
        sizes,
        ["small", "medium", "large", "extralarge", "mega"].map(Some)
    );
}

#[test]
fn the_recorded_tagged_tracks_decode_whatever_their_duration_says() {
    let page = decode::user_get_personal_tags_tracks(&raw("track", TRACKS)).unwrap();
    assert_eq!((page.subject(), page.total()), (Some("kelchm"), 1));
    let [only] = page.items() else {
        panic!("one track expected");
    };
    assert_eq!(only.track().name(), "Bedroom in Hell");
    assert_eq!(only.artist().name(), "3");
    assert_eq!(only.artist().url(), Some("https://www.last.fm/music/3"));
    // The service sent this word where a number of seconds belongs.
    assert_eq!(only.json()["duration"], "FIXME");
    assert_eq!(only.track().images().len(), 4);
}

#[test]
fn the_recorded_page_without_an_album_is_empty() {
    let page = decode::user_get_personal_tags_albums(&raw("album", NO_ALBUMS)).unwrap();
    assert!(page.items().is_empty());
    assert_eq!((page.subject(), page.total()), (Some("kelchm"), 0));
}

#[test]
fn the_derived_tagged_albums_decode() {
    let page = decode::user_get_personal_tags_albums(&raw("album", ALBUMS)).unwrap();
    assert_eq!(page.subject(), Some("rj_Synthetic"));
    assert_eq!(
        (page.per_page(), page.total_pages(), page.total()),
        (2, 2, 3)
    );
    let rows: Vec<_> = page
        .items()
        .iter()
        .map(|a| (a.album().title(), a.album().mbid(), a.artist().name()))
        .collect();
    assert_eq!(
        rows,
        [
            (Some("First Album"), None, "Tagged Artist"),
            (Some("Second Album"), None, "Tagged Artist")
        ]
    );
    assert_eq!(
        page.items()[0].album().url(),
        Some("https://example.invalid/music/Tagged+Artist/Album+1")
    );
    // What the model does not name is still in the row.
    assert_eq!(page.items()[0].json()["image"][0]["size"], "small");
}

#[test]
fn rows_may_be_one_object_or_none_at_all() {
    let page = artists(&taggings("artists", "artist", artist_row("Only"), 1)).unwrap();
    assert_eq!(page.items()[0].artist().name(), "Only");

    for rows in [Some(json!([])), Some(json!("")), None] {
        let mut body = taggings("tracks", "track", json!([]), 0);
        match rows {
            Some(rows) => body["taggings"]["tracks"]["track"] = rows,
            None => {
                body["taggings"]["tracks"]
                    .as_object_mut()
                    .unwrap()
                    .remove("track");
            }
        }
        assert!(tracks(&body).unwrap().items().is_empty());
    }
    names(
        tracks(&taggings("tracks", "track", json!([]), 3)),
        "taggings.tracks.track",
    );
}

#[test]
fn a_reply_of_another_kind_is_refused() {
    // The same method, asked for tracks: there are no `artists` in it.
    names(
        decode::user_get_personal_tags_artists(&raw("artist", TRACKS)),
        "taggings.artists",
    );
    names(
        decode::user_get_personal_tags_albums(&raw("album", ARTISTS)),
        "taggings.albums",
    );

    let request = Request::new(&methods::USER_GET_TOP_TAGS).param("user", "rj");
    let other = protocol::decode(&request, HttpResponse::new(200, ARTISTS)).unwrap();
    for error in [
        decode::user_get_personal_tags_artists(&other)
            .map(drop)
            .unwrap_err(),
        decode::user_get_personal_tags_albums(&other)
            .map(drop)
            .unwrap_err(),
        decode::user_get_personal_tags_tracks(&other)
            .map(drop)
            .unwrap_err(),
    ] {
        assert_eq!(error.kind(), ErrorKind::Decode);
        assert!(error.to_string().contains("different method"));
    }
}

#[test]
fn a_malformed_page_names_the_field() {
    names(artists(&json!({"other": SECRET})), "taggings");
    names(
        artists(&json!({"taggings": {"artists": {"artist": []}}})),
        "taggings.@attr",
    );
    let mut untagged = taggings("artists", "artist", json!([]), 0);
    untagged["taggings"]["@attr"]["tag"] = json!(7);
    names(artists(&untagged), "taggings.@attr.tag");
    let mut uncounted = taggings("artists", "artist", json!([]), 0);
    uncounted["taggings"]["@attr"]["total"] = json!(SECRET);
    names(artists(&uncounted), "taggings.@attr.total");

    names(
        artists(&taggings("artists", "artist", json!(SECRET), 1)),
        "taggings.artists.artist",
    );
    names(
        artists(&taggings(
            "artists",
            "artist",
            json!([artist_row("A"), SECRET]),
            2,
        )),
        "taggings.artists.artist[1]",
    );
    names(
        artists(&taggings(
            "artists",
            "artist",
            json!([artist_row("A"), {"mbid": ""}]),
            2,
        )),
        "taggings.artists.artist[1].name",
    );
    let mut no_artist = track_row("B");
    no_artist["artist"] = json!(SECRET);
    names(
        tracks(&taggings(
            "tracks",
            "track",
            json!([track_row("A"), no_artist]),
            2,
        )),
        "taggings.tracks.track[1].artist",
    );
}

#[test]
fn debug_leaves_out_the_text_of_the_response() {
    let mut body = taggings("tracks", "track", json!([track_row(SECRET)]), 1);
    body["taggings"]["@attr"]["tag"] = json!(SECRET);
    let page = tracks(&body).unwrap();
    let artists = artists(&taggings(
        "artists",
        "artist",
        json!([artist_row(SECRET)]),
        1,
    ))
    .unwrap();
    let albums = decode::user_get_personal_tags_albums(&raw("album", ALBUMS)).unwrap();
    let shown = format!(
        "{page:?} {:?} {artists:?} {:?} {albums:?} {:?}",
        page.items(),
        artists.items(),
        albums.items()
    );
    assert!(!shown.contains(SECRET), "{shown}");
    assert!(!shown.contains("First Album"), "{shown}");
    assert!(shown.contains("Page {"), "{shown}");
}
