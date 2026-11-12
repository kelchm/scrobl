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
use crate::protocol::{self, HttpResponse, MethodSpec, Raw, Request, methods};

// Fixtures. Provenance for each:
//
// - The `geo-top-*` bodies were recorded by the repository owner from the
//   service's public charts on 2026-11-11 and are kept byte for byte.
//   recorded.
// - Everything else is built here from those shapes. synthetic.

const ARTISTS: &[u8] = include_bytes!("../../../fixtures/recorded/geo-top-artists.json");
const TRACKS: &[u8] = include_bytes!("../../../fixtures/recorded/geo-top-tracks.json");

/// A string that appears in no error and no `Debug` output.
const SECRET: &str = "SYNTHETIC-RESPONSE-TEXT";

fn raw(spec: &'static MethodSpec, body: impl Into<Vec<u8>>) -> Raw {
    let request = Request::new(spec).param("country", "United States");
    protocol::decode(&request, HttpResponse::new(200, body.into())).unwrap()
}

fn artists(body: &Value) -> Result<Page<GeoArtist>, Error> {
    decode::geo_get_top_artists(&raw(&methods::GEO_GET_TOP_ARTISTS, body.to_string()))
}

fn tracks(body: &Value) -> Result<Page<GeoTrack>, Error> {
    decode::geo_get_top_tracks(&raw(&methods::GEO_GET_TOP_TRACKS, body.to_string()))
}

/// A page of `total` rows in all, of which `rows` are on this one.
fn paged(root: &str, key: &str, rows: Value, total: u64) -> Value {
    json!({root: {key: rows, "@attr": {
        "country": "Germany", "page": "1", "perPage": "50",
        "totalPages": total.min(1).to_string(), "total": total.to_string()}}})
}

fn artist_row(name: &str) -> Value {
    json!({"name": name, "listeners": "3", "mbid": "", "url": "https://example.invalid/a",
           "streamable": "0", "@attr": {"rank": "1"}})
}

fn track_row(name: &str) -> Value {
    json!({"name": name, "duration": "180", "listeners": "3", "mbid": "",
           "url": "https://example.invalid/t",
           "streamable": {"#text": "0", "fulltrack": "0"},
           "artist": {"name": "Artist", "mbid": "", "url": "https://example.invalid/a"},
           "@attr": {"rank": "0"}})
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
fn the_recorded_artists_of_a_country_decode() {
    let page = decode::geo_get_top_artists(&raw(&methods::GEO_GET_TOP_ARTISTS, ARTISTS)).unwrap();
    assert_eq!(page.subject(), Some("United States"));
    assert_eq!(
        (
            page.number(),
            page.per_page(),
            page.total_pages(),
            page.total()
        ),
        (1, 3, 746, 2238)
    );
    let rows: Vec<_> = page
        .items()
        .iter()
        .map(|a| {
            (
                a.artist().name(),
                a.listeners(),
                a.artist().mbid().is_some(),
            )
        })
        .collect();
    // The second row has no `mbid` member at all.
    assert_eq!(
        rows,
        [
            ("Radiohead", 69_663, true),
            ("Kanye West", 63_978, false),
            ("Tyler, The Creator", 62_022, true)
        ]
    );
    assert!(page.items()[1].json().get("mbid").is_none());
    assert_eq!(page.items()[0].artist().images().len(), 5);
    // The rank the model does not read is still in the row.
    assert_eq!(page.items()[2].json()["@attr"]["rank"], "3");
}

#[test]
fn the_recorded_tracks_of_a_country_decode() {
    let page = decode::geo_get_top_tracks(&raw(&methods::GEO_GET_TOP_TRACKS, TRACKS)).unwrap();
    assert_eq!(page.subject(), Some("United States"));
    assert_eq!(page.total(), 10_000);
    let rows: Vec<_> = page
        .items()
        .iter()
        .map(|t| {
            (
                t.track().name(),
                t.artist().name(),
                t.duration(),
                t.listeners(),
            )
        })
        .collect();
    assert_eq!(
        rows,
        [
            ("Nicole Kidman", "ADÉLA", Some(219), 19_359),
            ("the cure", "Olivia Rodrigo", Some(296), 18_378),
            ("Earrings", "Malcolm Todd", Some(153), 17_525)
        ]
    );
    assert_eq!(page.items()[0].track().images().len(), 4);
    // The service counts this list's ranks from 0.
    assert_eq!(page.items()[0].json()["@attr"]["rank"], "0");
}

#[test]
fn a_page_may_be_one_object_or_empty_and_needs_no_rank() {
    let mut row = artist_row("Only");
    row.as_object_mut().unwrap().remove("@attr");
    let one = artists(&paged("topartists", "artist", row, 1)).unwrap();
    assert_eq!(one.items()[0].artist().name(), "Only");

    let none = tracks(&paged("tracks", "track", json!([]), 0)).unwrap();
    assert!(none.items().is_empty());
    assert_eq!(none.subject(), Some("Germany"));

    let mut track = track_row("T");
    track["duration"] = json!("0");
    let page = tracks(&paged("tracks", "track", json!([track]), 1)).unwrap();
    assert_eq!(page.items()[0].duration(), None);
}

#[test]
fn a_malformed_page_names_the_field() {
    let mut row = artist_row("A");
    row["listeners"] = json!(SECRET);
    names(
        artists(&paged("topartists", "artist", json!([row]), 1)),
        "topartists.artist[0].listeners",
    );
    let mut row = artist_row("A");
    row.as_object_mut().unwrap().remove("name");
    names(
        artists(&paged("topartists", "artist", json!([row]), 1)),
        "topartists.artist[0].name",
    );

    for (field, path) in [
        ("listeners", "tracks.track[0].listeners"),
        ("duration", "tracks.track[0].duration"),
        ("artist", "tracks.track[0].artist"),
    ] {
        let mut row = track_row("T");
        row[field] = json!(SECRET);
        names(tracks(&paged("tracks", "track", json!([row]), 1)), path);
    }

    let mut page = paged("topartists", "artist", json!([]), 0);
    page["topartists"]["@attr"]
        .as_object_mut()
        .unwrap()
        .remove("country");
    names(artists(&page), "topartists.@attr.country");
    let mut page = paged("tracks", "track", json!([]), 0);
    page["tracks"]["@attr"]["country"] = json!([SECRET]);
    names(tracks(&page), "tracks.@attr.country");
    names(tracks(&json!({"tracks": {"track": []}})), "tracks.@attr");
}

#[test]
fn a_response_to_another_method_is_refused() {
    let raw = raw(&methods::GEO_GET_TOP_TRACKS, TRACKS);
    let error = decode::geo_get_top_artists(&raw).unwrap_err();
    assert_eq!(error.kind(), ErrorKind::Decode);
    assert_eq!(error.method(), Some("geo.getTopTracks"));
}

#[test]
fn debug_leaves_out_the_text_of_the_response() {
    let mut page = paged("topartists", "artist", json!([artist_row(SECRET)]), 1);
    page["topartists"]["@attr"]["country"] = json!(SECRET);
    let artists = artists(&page).unwrap();
    let tracks = tracks(&paged("tracks", "track", json!([track_row(SECRET)]), 1)).unwrap();
    let shown = format!(
        "{artists:?} {:?} {tracks:?} {:?}",
        artists.items()[0],
        tracks.items()[0]
    );
    assert!(!shown.contains(SECRET), "{shown}");
    assert!(shown.contains("Page {"), "{shown}");
}
