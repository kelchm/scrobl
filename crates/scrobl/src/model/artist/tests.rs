#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing,
    clippy::float_cmp
)]

use serde_json::{Value, json};

use super::*;
use crate::ErrorKind;
use crate::model::decode;
use crate::protocol::{self, HttpResponse, MethodSpec, Raw, Request, methods};

// Fixtures. Provenance for each:
//
// - The `artist-*` bodies were recorded by the repository owner from the
//   service's public pages about one artist on 2026-11-11 and are kept byte
//   for byte. recorded.
// - Everything else is built here from those shapes. synthetic.

const SIMILAR: &[u8] = include_bytes!("../../../fixtures/recorded/artist-similar.json");
const ALBUMS: &[u8] = include_bytes!("../../../fixtures/recorded/artist-top-albums.json");
const TRACKS: &[u8] = include_bytes!("../../../fixtures/recorded/artist-top-tracks.json");
const TAGS: &[u8] = include_bytes!("../../../fixtures/recorded/artist-top-tags.json");
const CORRECTION: &[u8] = include_bytes!("../../../fixtures/recorded/artist-correction.json");
const CORRECTION_SAME: &[u8] =
    include_bytes!("../../../fixtures/recorded/artist-correction-same.json");

/// A string that appears in no error and no `Debug` output.
const SECRET: &str = "SYNTHETIC-RESPONSE-TEXT";

fn raw(spec: &'static MethodSpec, body: impl Into<Vec<u8>>) -> Raw {
    let request = Request::new(spec).param("artist", "radiohead");
    protocol::decode(&request, HttpResponse::new(200, body.into())).unwrap()
}

fn similar(rows: Value) -> Result<List<SimilarArtist>, Error> {
    let body = json!({"similarartists": {"artist": rows, "@attr": {"artist": "A"}}});
    decode::artist_get_similar(&raw(&methods::ARTIST_GET_SIMILAR, body.to_string()))
}

fn tags(rows: Value) -> Result<List<TopTag>, Error> {
    let body = json!({"toptags": {"tag": rows, "@attr": {"artist": "A"}}});
    decode::artist_get_top_tags(&raw(&methods::ARTIST_GET_TOP_TAGS, body.to_string()))
}

fn correction(body: &Value) -> Result<Artist, Error> {
    decode::artist_get_correction(&raw(&methods::ARTIST_GET_CORRECTION, body.to_string()))
}

/// A page of `total` rows in all, of which `rows` are on this one.
fn paged(root: &str, key: &str, rows: Value, total: u64) -> Value {
    json!({root: {key: rows, "@attr": {
        "artist": "A", "page": "1", "perPage": "50",
        "totalPages": total.min(1).to_string(), "total": total.to_string()}}})
}

fn albums(rows: Value, total: u64) -> Result<Page<Album>, Error> {
    let body = paged("topalbums", "album", rows, total);
    decode::artist_get_top_albums(&raw(&methods::ARTIST_GET_TOP_ALBUMS, body.to_string()))
}

fn tracks(rows: Value, total: u64) -> Result<Page<Track>, Error> {
    let body = paged("toptracks", "track", rows, total);
    decode::artist_get_top_tracks(&raw(&methods::ARTIST_GET_TOP_TRACKS, body.to_string()))
}

fn album_row(name: &str) -> Value {
    json!({"name": name, "playcount": 7, "url": "https://example.invalid/b",
           "artist": {"name": "A", "mbid": "", "url": "https://example.invalid/a"},
           "@attr": {"rank": "1"}})
}

fn track_row(name: &str) -> Value {
    json!({"name": name, "playcount": "9", "listeners": "3",
           "url": "https://example.invalid/t", "streamable": "0",
           "artist": {"name": "A", "mbid": "", "url": "https://example.invalid/a"},
           "@attr": {"rank": "1"}})
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
fn the_recorded_similar_artists_decode() {
    let list = decode::artist_get_similar(&raw(&methods::ARTIST_GET_SIMILAR, SIMILAR)).unwrap();
    assert_eq!(list.subject(), Some("Radiohead"));
    let rows: Vec<_> = list
        .ranked()
        .map(|(place, row)| (place, row.artist().name(), row.similarity()))
        .collect();
    assert_eq!(
        rows,
        [
            (1, "Thom Yorke", 1.0),
            (2, "Atoms for Peace", 0.531_659),
            (3, "Muse", 0.316_509)
        ]
    );
    let first = list[0].clone();
    assert!(first.artist().mbid().is_some());
    assert_eq!(first.artist().images().len(), 6);
    // The likeness is the row's own: the artist has no counts here.
    assert_eq!(
        (first.artist().listeners(), first.artist().playcount()),
        (None, None)
    );
    assert_eq!(first.json()["match"], "1");
    let artist: &Artist = first.as_ref();
    assert_eq!(artist.name(), "Thom Yorke");
    assert_eq!(first.into_artist().name(), "Thom Yorke");
}

#[test]
fn the_recorded_top_albums_decode() {
    let page =
        decode::artist_get_top_albums(&raw(&methods::ARTIST_GET_TOP_ALBUMS, ALBUMS)).unwrap();
    assert_eq!(page.subject(), Some("Radiohead"));
    assert_eq!(
        (page.number(), page.per_page(), page.total()),
        (1, 3, 230_757)
    );
    let rows: Vec<_> = page
        .iter()
        .map(|album| {
            (
                album.name(),
                album.playcount(),
                album.listeners(),
                album.artist().map(Artist::name),
            )
        })
        .collect();
    // The service's order is not by play count.
    assert_eq!(
        rows,
        [
            ("OK Computer", Some(267_796_974), None, Some("Radiohead")),
            ("Pablo Honey", Some(97_569_961), None, Some("Radiohead")),
            ("In Rainbows", Some(258_888_472), None, Some("Radiohead"))
        ]
    );
    assert_eq!(page[0].images().len(), 4);
}

#[test]
fn the_recorded_top_tracks_decode() {
    let page =
        decode::artist_get_top_tracks(&raw(&methods::ARTIST_GET_TOP_TRACKS, TRACKS)).unwrap();
    assert_eq!(page.subject(), Some("Radiohead"));
    assert_eq!(page.total(), 462_958);
    let rows: Vec<_> = page
        .iter()
        .map(|track| (track.name(), track.playcount(), track.listeners()))
        .collect();
    assert_eq!(
        rows,
        [
            ("Creep", Some(64_205_671), Some(4_267_814)),
            ("No Surprises", Some(57_814_442), Some(3_497_214)),
            ("Karma Police", Some(44_494_634), Some(3_392_504))
        ]
    );
    // A row has its artist and no length and no album.
    let first = &page[0];
    assert_eq!(first.artist().name(), "Radiohead");
    assert_eq!((first.duration(), first.album().is_none()), (None, true));
}

#[test]
fn the_recorded_top_tags_decode() {
    let list = decode::artist_get_top_tags(&raw(&methods::ARTIST_GET_TOP_TAGS, TAGS)).unwrap();
    assert_eq!(list.subject(), Some("Radiohead"));
    assert_eq!(list.len(), 10);
    let first: Vec<_> = list
        .iter()
        .take(3)
        .map(|row| (row.tag().name(), row.count()))
        .collect();
    assert_eq!(
        first,
        [("rock", 100), ("alternative", 54), ("alternative rock", 34)]
    );
    let row = list[0].clone();
    assert_eq!(row.tag().url(), Some("https://www.last.fm/tag/rock"));
    // The count is a share of the most given tag, not the tag's uses.
    assert_eq!((row.tag().taggings(), row.tag().reach()), (None, None));
    assert_eq!(row.json()["count"], 100);
    let tag: &Tag = row.as_ref();
    assert_eq!(tag.name(), "rock");
    assert_eq!(row.into_tag().name(), "rock");
}

#[test]
fn the_recorded_corrections_decode() {
    let corrected =
        decode::artist_get_correction(&raw(&methods::ARTIST_GET_CORRECTION, CORRECTION)).unwrap();
    assert_eq!(corrected.name(), "Guns N' Roses");
    assert_eq!(
        (corrected.mbid(), corrected.url()),
        (None, Some("https://www.last.fm/music/Guns+N%27+Roses"))
    );
    // A name that is already right comes back as itself.
    let same =
        decode::artist_get_correction(&raw(&methods::ARTIST_GET_CORRECTION, CORRECTION_SAME))
            .unwrap();
    assert_eq!(same.name(), "Radiohead");
    assert!(same.mbid().is_some());
}

#[test]
fn rows_may_be_one_object_or_none_at_all() {
    let one = similar(json!({"name": "B", "match": 0.5})).unwrap();
    assert_eq!((one[0].artist().name(), one[0].similarity()), ("B", 0.5));
    assert!(similar(json!([])).unwrap().is_empty());
    assert!(tags(json!("")).unwrap().is_empty());
    assert_eq!(
        tags(json!({"name": "t", "count": "7"})).unwrap()[0].count(),
        7
    );
    assert!(albums(json!([]), 0).unwrap().is_empty());
    assert_eq!(tracks(track_row("T"), 1).unwrap()[0].name(), "T");
    // A row needs no rank and no id.
    let mut row = album_row("B");
    row.as_object_mut().unwrap().remove("@attr");
    assert_eq!(albums(json!([row]), 1).unwrap()[0].mbid(), None);
}

#[test]
fn a_malformed_list_names_the_field() {
    for unlike in [json!(SECRET), json!("-1"), json!("1e3"), json!([1])] {
        names(
            similar(json!([{"name": "B", "match": unlike}])),
            "similarartists.artist[0].match",
        );
    }
    names(
        similar(json!([{"name": "B"}])),
        "similarartists.artist[0].match",
    );
    names(
        similar(json!([{"match": "1"}])),
        "similarartists.artist[0].name",
    );
    names(similar(json!(SECRET)), "similarartists.artist");
    names(tags(json!([{"name": "t"}])), "toptags.tag[0].count");
    names(
        tags(json!([{"name": "t", "count": SECRET}])),
        "toptags.tag[0].count",
    );
    names(tags(json!([{"count": 1}])), "toptags.tag[0].name");

    let mut row = album_row("B");
    row["playcount"] = json!(SECRET);
    names(albums(json!([row]), 1), "topalbums.album[0].playcount");
    let mut row = album_row("B");
    row["artist"] = json!(SECRET);
    names(albums(json!([row]), 1), "topalbums.album[0].artist");
    for member in ["playcount", "listeners", "name", "artist"] {
        let mut row = track_row("T");
        row.as_object_mut().unwrap().remove(member);
        names(
            tracks(json!([row]), 1),
            &format!("toptracks.track[0].{member}"),
        );
    }
    // Rows that the page's numbers promise must be there.
    names(albums(json!([]), 3), "topalbums.album");

    let no_subject = json!({"toptags": {"tag": [], "@attr": {}}});
    names(
        decode::artist_get_top_tags(&raw(&methods::ARTIST_GET_TOP_TAGS, no_subject.to_string())),
        "toptags.@attr.artist",
    );
}

#[test]
fn a_malformed_correction_names_the_field() {
    names(correction(&json!({"other": SECRET})), "corrections");
    names(
        correction(&json!({"corrections": {"correction": SECRET}})),
        "corrections.correction",
    );
    names(
        correction(&json!({"corrections": {}})),
        "corrections.correction",
    );
    names(
        correction(&json!({"corrections": {"correction": {"artist": [SECRET]}}})),
        "corrections.correction.artist",
    );
    names(
        correction(&json!({"corrections": {"correction": {"artist": {"url": SECRET}}}})),
        "corrections.correction.artist.name",
    );
}

#[test]
fn a_response_to_another_method_is_refused() {
    let other = raw(&methods::ARTIST_GET_TOP_TRACKS, TRACKS);
    for error in [
        decode::artist_get_similar(&other).map(drop).unwrap_err(),
        decode::artist_get_top_albums(&other).map(drop).unwrap_err(),
        decode::artist_get_top_tags(&other).map(drop).unwrap_err(),
        decode::artist_get_correction(&other).map(drop).unwrap_err(),
        decode::artist_get_top_tracks(&raw(&methods::ARTIST_GET_SIMILAR, SIMILAR))
            .map(drop)
            .unwrap_err(),
    ] {
        assert_eq!(error.kind(), ErrorKind::Decode);
        assert!(error.to_string().contains("different method"));
    }
}

#[test]
fn debug_leaves_out_the_text_of_the_response() {
    let similar = similar(json!([{"name": SECRET, "match": "0.5", "url": SECRET}])).unwrap();
    let tags = tags(json!([{"name": SECRET, "count": 7, "url": SECRET}])).unwrap();
    let corrected = correction(&json!({"corrections": {"correction": {
        "artist": {"name": SECRET, "url": SECRET}}}}))
    .unwrap();
    let shown = format!(
        "{similar:?} {:?} {tags:?} {:?} {corrected:?}",
        similar[0], tags[0]
    );
    assert!(!shown.contains(SECRET), "{shown}");
    assert!(
        shown.contains("SimilarArtist { similarity: 0.5, .. }"),
        "{shown}"
    );
    assert!(shown.contains("TopTag { count: 7, .. }"), "{shown}");
}
