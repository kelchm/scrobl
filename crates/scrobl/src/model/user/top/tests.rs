#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]

use serde_json::{Value, json};

use super::*;
use crate::ErrorKind;
use crate::protocol::{self, HttpResponse, MethodSpec, Request};

// Fixtures. Provenance for each:
//
// - The `kelchm-top-*` bodies were recorded by the repository owner from
//   their own account on 2026-11-10 and are kept byte for byte. recorded.
// - Everything else is built here from those shapes. synthetic.

const ARTISTS: &[u8] = include_bytes!("../../../../fixtures/recorded/kelchm-top-artists.json");
const BEYOND: &[u8] =
    include_bytes!("../../../../fixtures/recorded/kelchm-top-artists-beyond.json");
const ALBUMS: &[u8] = include_bytes!("../../../../fixtures/recorded/kelchm-top-albums.json");
const TRACKS: &[u8] = include_bytes!("../../../../fixtures/recorded/kelchm-top-tracks.json");
const TAGS: &[u8] = include_bytes!("../../../../fixtures/recorded/kelchm-top-tags.json");

/// A string that appears in no error and no `Debug` output.
const SECRET: &str = "SYNTHETIC-RESPONSE-TEXT";

fn raw(spec: &'static MethodSpec, body: impl Into<Vec<u8>>) -> Raw {
    let request = Request::new(spec).param("user", "rj");
    protocol::decode(&request, HttpResponse::new(200, body.into())).unwrap()
}

fn artists(body: &Value) -> Result<TopArtistsPage, Error> {
    TopArtistsPage::decode(&raw(&methods::USER_GET_TOP_ARTISTS, body.to_string()))
}

fn albums(body: &Value) -> Result<TopAlbumsPage, Error> {
    TopAlbumsPage::decode(&raw(&methods::USER_GET_TOP_ALBUMS, body.to_string()))
}

fn tracks(body: &Value) -> Result<TopTracksPage, Error> {
    TopTracksPage::decode(&raw(&methods::USER_GET_TOP_TRACKS, body.to_string()))
}

fn tags(body: &Value) -> Result<TopTags, Error> {
    TopTags::decode(&raw(&methods::USER_GET_TOP_TAGS, body.to_string()))
}

/// A page of `total` rows in all, of which `rows` are on this one.
fn paged(root: &str, key: &str, rows: Value, total: u64) -> Value {
    json!({root: {key: rows, "@attr": {
        "user": "rj", "page": "1", "perPage": "50",
        "totalPages": total.min(1).to_string(), "total": total.to_string()}}})
}

fn artist_row(rank: &str, name: &str) -> Value {
    json!({"streamable": "0", "mbid": "", "url": "https://example.invalid/a",
           "playcount": "7", "@attr": {"rank": rank}, "name": name})
}

fn album_row(rank: &str, name: &str) -> Value {
    json!({"artist": {"url": "https://example.invalid/a", "name": "Artist", "mbid": ""},
           "mbid": "", "url": "https://example.invalid/b",
           "playcount": "7", "@attr": {"rank": rank}, "name": name})
}

fn track_row(rank: &str, name: &str) -> Value {
    json!({"streamable": {"fulltrack": "0", "#text": "0"}, "mbid": "", "name": name,
           "artist": {"url": "https://example.invalid/a", "name": "Artist", "mbid": ""},
           "url": "https://example.invalid/t", "duration": "180",
           "@attr": {"rank": rank}, "playcount": "7"})
}

fn tag_row(name: &str) -> Value {
    json!({"name": name, "count": "2", "url": "https://example.invalid/tag"})
}

fn tag_list(tag: Value) -> Value {
    json!({"toptags": {"tag": tag, "@attr": {"user": "rj"}}})
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
fn the_recorded_top_artists_decode() {
    let page = TopArtistsPage::decode(&raw(&methods::USER_GET_TOP_ARTISTS, ARTISTS)).unwrap();

    let attr = page.attr();
    assert_eq!(attr.user(), "kelchm");
    assert_eq!(
        (
            attr.page(),
            attr.per_page(),
            attr.total_pages(),
            attr.total()
        ),
        (1, 3, 3369, 10_105)
    );

    let rows: Vec<_> = page
        .artists()
        .iter()
        .map(|a| (a.rank(), a.playcount(), a.artist().name()))
        .collect();
    assert_eq!(
        rows,
        [
            (1, 5584, "Portugal. The Man"),
            (2, 4146, "Bright Eyes"),
            (3, 3726, "Elliott Smith")
        ]
    );
    let first = &page.artists()[0];
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
    assert_eq!(first.json()["image"][0]["size"], "small");
}

#[test]
fn the_recorded_page_beyond_the_last_is_empty() {
    let page = TopArtistsPage::decode(&raw(&methods::USER_GET_TOP_ARTISTS, BEYOND)).unwrap();
    assert!(page.artists().is_empty());
    assert_eq!(
        (page.attr().page(), page.attr().total_pages()),
        (9999, 11),
        "the list is `[]` on a page past the last"
    );
    assert_eq!(page.attr().total(), 10_105);
}

#[test]
fn the_recorded_top_albums_decode() {
    let page = TopAlbumsPage::decode(&raw(&methods::USER_GET_TOP_ALBUMS, ALBUMS)).unwrap();
    assert_eq!(
        (page.attr().total_pages(), page.attr().total()),
        (7258, 21_774)
    );

    let rows: Vec<_> = page
        .albums()
        .iter()
        .map(|a| {
            (
                a.rank(),
                a.playcount(),
                a.album().title(),
                a.artist().name(),
            )
        })
        .collect();
    assert_eq!(
        rows,
        [
            (
                1,
                1675,
                Some("The Social Network"),
                "Trent Reznor and Atticus Ross"
            ),
            (2, 888, Some("The Satanic Satanist"), "Portugal. The Man"),
            (3, 770, Some("The Glass Passenger"), "Jack's Mannequin")
        ]
    );
    let first = &page.albums()[0];
    assert_eq!(
        first.album().mbid(),
        Some("121682be-5d99-40b9-8ea0-4b20a7fe4a38")
    );
    assert_eq!(
        first.album().url(),
        Some("https://www.last.fm/music/Trent+Reznor+and+Atticus+Ross/The+Social+Network")
    );
    // The service sent an empty artist `mbid`; it is present in the row.
    assert_eq!(first.artist().mbid(), None);
    assert_eq!(first.json()["artist"]["mbid"], "");
    assert_eq!(
        page.albums()[1].artist().mbid(),
        Some("3599a39e-4e10-4cb5-90d4-c8a015ebc73b")
    );
}

#[test]
fn the_recorded_top_tracks_decode() {
    let page = TopTracksPage::decode(&raw(&methods::USER_GET_TOP_TRACKS, TRACKS)).unwrap();
    assert_eq!(
        (page.attr().total_pages(), page.attr().total()),
        (15_687, 47_059)
    );

    let rows: Vec<_> = page
        .tracks()
        .iter()
        .map(|t| {
            (
                t.rank(),
                t.playcount(),
                t.duration(),
                t.track().name(),
                t.artist().name(),
            )
        })
        .collect();
    assert_eq!(
        rows,
        [
            (1, 163, Some(197), "Swim", "Fickle Friends"),
            (2, 161, Some(256), "Four Winds", "Bright Eyes"),
            (3, 157, Some(209), "Greek Tragedy", "The Wombats")
        ]
    );
    let first = &page.tracks()[0];
    assert_eq!(
        first.track().mbid(),
        Some("0b34b2a4-d282-4fba-a1d8-f9970a0852c3")
    );
    assert_eq!(
        first.track().url(),
        Some("https://www.last.fm/music/Fickle+Friends/_/Swim")
    );
    assert_eq!(
        first.artist().url(),
        Some("https://www.last.fm/music/Fickle+Friends")
    );
    assert_eq!(first.json()["streamable"]["fulltrack"], "0");
}

#[test]
fn the_recorded_top_tags_decode() {
    let top = TopTags::decode(&raw(&methods::USER_GET_TOP_TAGS, TAGS)).unwrap();
    assert_eq!(top.user(), "kelchm");
    let rows: Vec<_> = top
        .tags()
        .iter()
        .map(|t| (t.name(), t.count(), t.url()))
        .collect();
    assert_eq!(
        rows,
        [
            ("indie", 1, Some("https://www.last.fm/tag/indie")),
            (
                "interesting",
                1,
                Some("https://www.last.fm/tag/interesting")
            ),
            ("to buy", 1, Some("https://www.last.fm/tag/to+buy"))
        ]
    );
    assert_eq!(top.tags()[2].json()["name"], "to buy");
}

#[test]
fn a_single_row_may_come_as_an_object() {
    let page = artists(&paged("topartists", "artist", artist_row("1", "Only"), 1)).unwrap();
    assert_eq!(page.artists()[0].artist().name(), "Only");

    let page = albums(&paged("topalbums", "album", album_row("1", "Only"), 1)).unwrap();
    assert_eq!(page.albums()[0].album().title(), Some("Only"));

    let page = tracks(&paged("toptracks", "track", track_row("1", "Only"), 1)).unwrap();
    assert_eq!(page.tracks()[0].track().name(), "Only");

    let top = tags(&tag_list(tag_row("only"))).unwrap();
    assert_eq!(top.tags()[0].name(), "only");
}

#[test]
fn an_empty_list_decodes_in_each_of_its_forms() {
    for rows in [Some(json!([])), Some(json!("")), None] {
        let mut body = paged("toptracks", "track", json!([]), 0);
        let mut list = tag_list(json!([]));
        match rows {
            Some(rows) => {
                body["toptracks"]["track"] = rows.clone();
                list["toptags"]["tag"] = rows;
            }
            None => {
                body["toptracks"].as_object_mut().unwrap().remove("track");
                list["toptags"].as_object_mut().unwrap().remove("tag");
            }
        }
        assert!(tracks(&body).unwrap().tracks().is_empty());
        assert!(tags(&list).unwrap().tags().is_empty());
    }
}

#[test]
fn rows_that_are_promised_must_be_there() {
    names(
        artists(&paged("topartists", "artist", json!([]), 3)),
        "topartists.artist",
    );
    names(
        albums(&paged("topalbums", "album", json!(SECRET), 3)),
        "topalbums.album",
    );
    names(
        tracks(&paged("toptracks", "track", Value::Null, 3)),
        "toptracks.track",
    );
    names(tags(&tag_list(json!(SECRET))), "toptags.tag");
}

#[test]
fn a_malformed_list_names_the_field() {
    names(artists(&json!({"other": SECRET})), "topartists");
    names(
        albums(&json!({"topalbums": {"album": []}})),
        "topalbums.@attr",
    );
    names(tags(&json!({"toptags": {"tag": []}})), "toptags.@attr");
    names(
        tags(&json!({"toptags": {"tag": [], "@attr": {"user": 7}}})),
        "toptags.@attr.user",
    );

    let mut body = paged("toptracks", "track", json!([track_row("1", "A")]), 1);
    body["toptracks"]["@attr"]["total"] = json!(SECRET);
    names(tracks(&body), "toptracks.@attr.total");
}

#[test]
fn a_malformed_row_names_the_field() {
    let with = |row: Value, change: &dyn Fn(&mut Value)| {
        let mut second = row;
        change(&mut second);
        second
    };
    let artist_page = |second: Value| {
        artists(&paged(
            "topartists",
            "artist",
            json!([artist_row("1", "A"), second]),
            2,
        ))
    };
    let album_page = |second: Value| {
        albums(&paged(
            "topalbums",
            "album",
            json!([album_row("1", "A"), second]),
            2,
        ))
    };
    let track_page = |second: Value| {
        tracks(&paged(
            "toptracks",
            "track",
            json!([track_row("1", "A"), second]),
            2,
        ))
    };

    names(artist_page(json!(SECRET)), "topartists.artist[1]");
    names(
        artist_page(artist_row(SECRET, "B")),
        "topartists.artist[1].@attr.rank",
    );
    names(
        artist_page(with(artist_row("2", "B"), &|row| {
            row.as_object_mut().unwrap().remove("@attr");
        })),
        "topartists.artist[1].@attr.rank",
    );
    names(
        artist_page(with(artist_row("2", "B"), &|row| {
            row["playcount"] = json!(SECRET);
        })),
        "topartists.artist[1].playcount",
    );
    names(
        artist_page(with(artist_row("2", "B"), &|row| row["name"] = json!(7))),
        "topartists.artist[1].name",
    );
    names(
        artist_page(with(artist_row("2", "B"), &|row| row["mbid"] = json!(7))),
        "topartists.artist[1].mbid",
    );

    names(
        album_page(with(album_row("2", "B"), &|row| {
            row["artist"] = json!(SECRET);
        })),
        "topalbums.album[1].artist",
    );
    names(
        album_page(with(album_row("2", "B"), &|row| {
            row.as_object_mut().unwrap().remove("name");
        })),
        "topalbums.album[1].name",
    );
    names(
        album_page(with(album_row("2", "B"), &|row| row["url"] = json!(7))),
        "topalbums.album[1].url",
    );

    names(
        track_page(with(track_row("2", "B"), &|row| {
            row["duration"] = json!(SECRET);
        })),
        "toptracks.track[1].duration",
    );
    names(
        track_page(with(track_row("2", "B"), &|row| {
            row["artist"]["mbid"] = json!(7);
        })),
        "toptracks.track[1].artist.mbid",
    );

    names(
        tags(&tag_list(json!([tag_row("a"), SECRET]))),
        "toptags.tag[1]",
    );
    names(
        tags(&tag_list(
            json!([tag_row("a"), {"name": "b", "count": SECRET}]),
        )),
        "toptags.tag[1].count",
    );
    names(
        tags(&tag_list(json!([tag_row("a"), {"count": "1"}]))),
        "toptags.tag[1].name",
    );
}

#[test]
fn what_a_row_may_leave_out_reads_as_none() {
    let mut row = track_row("1", "A");
    row.as_object_mut().unwrap().remove("duration");
    let page = tracks(&paged("toptracks", "track", json!([row]), 1)).unwrap();
    assert_eq!(page.tracks()[0].duration(), None);
    assert_eq!(page.tracks()[0].track().mbid(), None);

    let page = albums(&paged("topalbums", "album", json!([album_row("1", "")]), 1)).unwrap();
    assert_eq!(page.albums()[0].album().title(), None);

    let top = tags(&tag_list(json!([{"name": "a", "count": 3}]))).unwrap();
    assert_eq!((top.tags()[0].count(), top.tags()[0].url()), (3, None));
}

#[test]
fn a_repeated_member_is_an_error() {
    let body = r#"{"toptags": {"tag": [], "tag": [], "@attr": {"user": "rj"}}}"#;
    let error = TopTags::decode(&raw(&methods::USER_GET_TOP_TAGS, body)).unwrap_err();
    assert_eq!(error.kind(), ErrorKind::Decode);
}

#[test]
fn a_response_to_another_method_is_refused() {
    let other = raw(&methods::USER_GET_RECENT_TRACKS, ARTISTS);
    for error in [
        TopArtistsPage::decode(&other).map(drop).unwrap_err(),
        TopAlbumsPage::decode(&other).map(drop).unwrap_err(),
        TopTracksPage::decode(&other).map(drop).unwrap_err(),
        TopTags::decode(&other).map(drop).unwrap_err(),
    ] {
        assert_eq!(error.kind(), ErrorKind::Decode);
        assert!(error.to_string().contains("different method"));
    }
}

#[test]
fn debug_leaves_out_the_text_of_the_response() {
    let artists = artists(&paged(
        "topartists",
        "artist",
        json!([artist_row("1", SECRET)]),
        1,
    ))
    .unwrap();
    let albums = albums(&paged(
        "topalbums",
        "album",
        json!([album_row("1", SECRET)]),
        1,
    ))
    .unwrap();
    let tracks = tracks(&paged(
        "toptracks",
        "track",
        json!([track_row("1", SECRET)]),
        1,
    ))
    .unwrap();
    let tags = tags(&tag_list(json!([tag_row(SECRET)]))).unwrap();

    let shown = format!(
        "{artists:?} {:?} {albums:?} {:?} {tracks:?} {:?} {tags:?} {:?}",
        artists.artists(),
        albums.albums(),
        tracks.tracks(),
        tags.tags()
    );
    assert!(!shown.contains(SECRET), "{shown}");
    assert!(!shown.contains("rj"), "{shown}");
    assert!(shown.contains("playcount: 7"), "{shown}");
    assert!(shown.contains("duration: Some(180)"), "{shown}");
}
