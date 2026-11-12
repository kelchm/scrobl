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
// - The three `*-search*` bodies were recorded by the repository owner
//   from the service's public search on 2026-11-11 and are kept byte for
//   byte. recorded.
// - Everything else is built here from those shapes. synthetic.

const ARTISTS: &[u8] = include_bytes!("../../../fixtures/recorded/artist-search.json");
const ALBUMS: &[u8] = include_bytes!("../../../fixtures/recorded/album-search.json");
const NOTHING: &[u8] = include_bytes!("../../../fixtures/recorded/artist-search-nothing.json");
// `track-search.json` was recorded the same way on 2026-11-12. recorded.
const TRACKS: &[u8] = include_bytes!("../../../fixtures/recorded/track-search.json");

/// A string that appears in no error and no `Debug` output.
const SECRET: &str = "SYNTHETIC-RESPONSE-TEXT";

fn raw(spec: &'static MethodSpec, body: impl Into<Vec<u8>>) -> Raw {
    let request = Request::new(spec).param("artist", "radiohead");
    protocol::decode(&request, HttpResponse::new(200, body.into())).unwrap()
}

/// Page `page` of `total` matches, `size` to a page, holding `rows`.
fn results(matches: &str, key: &str, rows: Value, page: u64, size: u64, total: u64) -> Value {
    json!({"results": {
        "opensearch:Query": {"#text": "", "role": "request", "searchTerms": "q",
                             "startPage": page.to_string()},
        "opensearch:totalResults": total.to_string(),
        "opensearch:startIndex": ((page - 1) * size).to_string(),
        "opensearch:itemsPerPage": size.to_string(),
        matches: {key: rows},
        "@attr": {"for": "q"}}})
}

fn artists(body: &Value) -> Result<Page<Artist>, Error> {
    decode::artist_search(&raw(&methods::ARTIST_SEARCH, body.to_string()))
}

fn albums(body: &Value) -> Result<Page<Album>, Error> {
    decode::album_search(&raw(&methods::ALBUM_SEARCH, body.to_string()))
}

fn artist_row(name: &str) -> Value {
    json!({"name": name, "listeners": "3", "mbid": "", "url": "https://example.invalid/a"})
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
fn the_recorded_search_for_artists_decodes() {
    let page = decode::artist_search(&raw(&methods::ARTIST_SEARCH, ARTISTS)).unwrap();
    assert_eq!(page.subject(), Some("radiohead"));
    assert_eq!((page.number(), page.per_page(), page.total()), (1, 3, 9275));
    // The number of pages is worked out: 9275 matches, three to a page.
    assert_eq!((page.total_pages(), page.next_page()), (3092, Some(2)));
    let rows: Vec<_> = page
        .ranked()
        .map(|(place, artist)| {
            (
                place,
                artist.name(),
                artist.listeners(),
                artist.mbid().is_some(),
            )
        })
        .collect();
    assert_eq!(
        rows,
        [
            (1, "Radiohead", Some(8_512_078), true),
            (2, "radıohead", Some(117), false),
            (3, "RADİOHEAD", Some(36), false)
        ]
    );
    assert_eq!(page[0].playcount(), None);
    assert_eq!(page[0].images().len(), 4);
}

#[test]
fn the_recorded_search_for_albums_decodes() {
    let page = decode::album_search(&raw(&methods::ALBUM_SEARCH, ALBUMS)).unwrap();
    assert_eq!(page.subject(), Some("ok computer"));
    assert_eq!((page.total(), page.total_pages()), (4768, 1590));
    let rows: Vec<_> = page
        .iter()
        .map(|album| (album.name(), album.artist().map(Artist::name)))
        .collect();
    assert_eq!(
        rows,
        [
            ("OK Computer", Some("Radiohead")),
            ("OK Computer", Some("レディオヘッド")),
            ("Ok Computer", Some("Lemaitre"))
        ]
    );
    // A match names its artist with a string, and has no counts.
    let first = &page[0];
    assert_eq!(first.artist().unwrap().url(), None);
    assert_eq!((first.listeners(), first.playcount()), (None, None));
    assert!(first.mbid().is_some() && first.url().is_some());
}

#[test]
fn the_recorded_search_for_tracks_decodes() {
    let page = decode::track_search(&raw(&methods::TRACK_SEARCH, TRACKS)).unwrap();
    assert_eq!(page.subject(), Some("creep"));
    assert_eq!((page.per_page(), page.total()), (3, 97_919));
    let rows: Vec<_> = page
        .iter()
        .map(|track| (track.name(), track.artist().name(), track.listeners()))
        .collect();
    assert_eq!(
        rows,
        [
            ("Creep", "Radiohead", Some(4_267_814)),
            ("Creep", "Frost Children", Some(69_729)),
            ("Creep", "TLC", Some(553_465))
        ]
    );
    // A match names its artist with a string, and has no plays, no length
    // and no album.
    let first = &page[0];
    assert_eq!(first.artist().url(), None);
    assert_eq!(
        (first.playcount(), first.duration(), first.album().is_none()),
        (None, None, true)
    );

    let row = |artist: Value| {
        let body = results(
            "trackmatches",
            "track",
            json!([{"name": "T", "listeners": "3", "artist": artist}]),
            1,
            3,
            1,
        );
        decode::track_search(&raw(&methods::TRACK_SEARCH, body.to_string()))
    };
    assert_eq!(row(json!("A")).unwrap()[0].artist().name(), "A");
    names(
        row(json!({"name": SECRET})),
        "results.trackmatches.track[0].artist",
    );
    let body = results(
        "trackmatches",
        "track",
        json!([{"name": "T", "artist": "A"}]),
        1,
        3,
        1,
    );
    names(
        decode::track_search(&raw(&methods::TRACK_SEARCH, body.to_string())),
        "results.trackmatches.track[0].listeners",
    );
}

#[test]
fn the_recorded_search_that_finds_nothing_is_an_empty_page() {
    let page = decode::artist_search(&raw(&methods::ARTIST_SEARCH, NOTHING)).unwrap();
    assert!(page.is_empty());
    assert_eq!((page.total(), page.total_pages()), (0, 0));
    assert_eq!(page.next_page(), None);
}

#[test]
fn the_paging_is_read_from_the_three_numbers() {
    let last = artists(&results(
        "artistmatches",
        "artist",
        artist_row("A"),
        5,
        2,
        9,
    ))
    .unwrap();
    assert_eq!((last.number(), last.total_pages()), (5, 5));
    assert_eq!(last.next_page(), None);
    assert_eq!(last.ranked().next().map(|(place, _)| place), Some(9));
    // An exact fit has no page left over.
    let even = artists(&results("artistmatches", "artist", json!([]), 6, 2, 10)).unwrap();
    assert_eq!(even.total_pages(), 5);
    // More pages than fit a count of pages do not overflow it.
    let vast = artists(&results(
        "artistmatches",
        "artist",
        artist_row("A"),
        1,
        1,
        u64::MAX,
    ))
    .unwrap();
    assert_eq!(vast.total_pages(), u32::MAX);
    // A page size of 0 leaves no way to say how many pages there are.
    let mut none_to_a_page = results("artistmatches", "artist", json!([]), 1, 1, 9);
    none_to_a_page["results"]["opensearch:itemsPerPage"] = json!("0");
    names(artists(&none_to_a_page), "results.opensearch:itemsPerPage");
}

#[test]
fn a_malformed_search_names_the_field() {
    names(artists(&json!({"other": SECRET})), "results");
    for (member, field) in [
        ("opensearch:Query", "results.opensearch:Query"),
        ("opensearch:itemsPerPage", "results.opensearch:itemsPerPage"),
        ("opensearch:totalResults", "results.opensearch:totalResults"),
        ("artistmatches", "results.artistmatches"),
        ("@attr", "results.@attr"),
    ] {
        let mut body = results("artistmatches", "artist", json!([artist_row("A")]), 1, 3, 1);
        body["results"].as_object_mut().unwrap().remove(member);
        names(artists(&body), field);
        let mut body = results("artistmatches", "artist", json!([artist_row("A")]), 1, 3, 1);
        body["results"][member] = json!([SECRET]);
        names(artists(&body), field);
    }
    let mut body = results("artistmatches", "artist", json!([]), 1, 3, 0);
    body["results"]["opensearch:Query"]["startPage"] = json!(SECRET);
    names(artists(&body), "results.opensearch:Query.startPage");
    let mut body = results("artistmatches", "artist", json!([]), 1, 3, 0);
    body["results"]["@attr"] = json!({});
    names(artists(&body), "results.@attr.for");

    // Rows the numbers promise must be there, and each needs what it needs.
    names(
        artists(&results("artistmatches", "artist", json!([]), 1, 3, 7)),
        "results.artistmatches.artist",
    );
    names(
        artists(&results(
            "artistmatches",
            "artist",
            json!([{"name": "A"}]),
            1,
            3,
            1,
        )),
        "results.artistmatches.artist[0].listeners",
    );
    for artist in [json!(""), json!({"name": SECRET}), json!(7)] {
        names(
            albums(&results(
                "albummatches",
                "album",
                json!([{"name": "B", "artist": artist}]),
                1,
                3,
                1,
            )),
            "results.albummatches.album[0].artist",
        );
    }
    names(
        albums(&results(
            "albummatches",
            "album",
            json!([{"artist": "A"}]),
            1,
            3,
            1,
        )),
        "results.albummatches.album[0].name",
    );
}

#[test]
fn a_response_to_another_method_is_refused() {
    for error in [
        decode::artist_search(&raw(&methods::ALBUM_SEARCH, ALBUMS))
            .map(drop)
            .unwrap_err(),
        decode::album_search(&raw(&methods::ARTIST_SEARCH, ARTISTS))
            .map(drop)
            .unwrap_err(),
    ] {
        assert_eq!(error.kind(), ErrorKind::Decode);
        assert!(error.to_string().contains("different method"));
    }
}

#[test]
fn debug_leaves_out_the_text_of_the_response() {
    let mut body = results(
        "albummatches",
        "album",
        json!([{"name": SECRET, "artist": SECRET}]),
        1,
        3,
        1,
    );
    body["results"]["@attr"]["for"] = json!(SECRET);
    let page = albums(&body).unwrap();
    let shown = format!("{page:?} {:?} {:?}", page[0], page[0].artist());
    assert!(!shown.contains(SECRET), "{shown}");
    assert!(shown.contains("Page {"), "{shown}");
}
