#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]

use serde_json::{Value, json};

use super::*;
use crate::ErrorKind;
use crate::model::Image;
use crate::model::decode;
use crate::protocol::{self, HttpResponse, MethodSpec, Raw, Request, methods};

// Fixtures. Provenance for each:
//
// - The `chart-top-*` bodies were recorded by the repository owner from the
//   service's public charts on 2026-11-11 and are kept byte for byte.
//   recorded.
// - Everything else is built here from those shapes. synthetic.

const ARTISTS: &[u8] = include_bytes!("../../../fixtures/recorded/chart-top-artists.json");
const BEYOND: &[u8] = include_bytes!("../../../fixtures/recorded/chart-top-artists-beyond.json");
const TRACKS: &[u8] = include_bytes!("../../../fixtures/recorded/chart-top-tracks.json");
const TAGS: &[u8] = include_bytes!("../../../fixtures/recorded/chart-top-tags.json");

/// A string that appears in no error and no `Debug` output.
const SECRET: &str = "SYNTHETIC-RESPONSE-TEXT";

fn raw(spec: &'static MethodSpec, body: impl Into<Vec<u8>>) -> Raw {
    let request = Request::new(spec).param("limit", 3);
    protocol::decode(&request, HttpResponse::new(200, body.into())).unwrap()
}

fn artists(body: &Value) -> Result<Page<Artist>, Error> {
    decode::chart_get_top_artists(&raw(&methods::CHART_GET_TOP_ARTISTS, body.to_string()))
}

fn tracks(body: &Value) -> Result<Page<Track>, Error> {
    decode::chart_get_top_tracks(&raw(&methods::CHART_GET_TOP_TRACKS, body.to_string()))
}

fn tags(body: &Value) -> Result<List<Tag>, Error> {
    decode::chart_get_top_tags(&raw(&methods::CHART_GET_TOP_TAGS, body.to_string()))
}

/// A page of `total` rows in all, of which `rows` are on this one.
fn paged(root: &str, key: &str, rows: Value, total: u64) -> Value {
    json!({root: {key: rows, "@attr": {
        "page": "1", "perPage": "50",
        "totalPages": total.min(1).to_string(), "total": total.to_string()}}})
}

fn artist_row(name: &str) -> Value {
    json!({"name": name, "playcount": "7", "listeners": "3", "mbid": "",
           "url": "https://example.invalid/a", "streamable": "0"})
}

fn track_row(name: &str) -> Value {
    json!({"name": name, "duration": "180", "playcount": "7", "listeners": "3",
           "mbid": "", "url": "https://example.invalid/t",
           "streamable": {"#text": "0", "fulltrack": "0"},
           "artist": {"name": "Artist", "mbid": "", "url": "https://example.invalid/a"}})
}

fn tag_row(name: &str) -> Value {
    json!({"name": name, "url": "https://example.invalid/tag", "reach": "3",
           "taggings": "7", "streamable": "1", "wiki": {}})
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
fn the_recorded_chart_of_artists_decodes() {
    let page =
        decode::chart_get_top_artists(&raw(&methods::CHART_GET_TOP_ARTISTS, ARTISTS)).unwrap();
    assert_eq!(
        (
            page.number(),
            page.per_page(),
            page.total_pages(),
            page.total()
        ),
        (1, 3, 3334, 10_000)
    );
    let rows: Vec<_> = page
        .items()
        .iter()
        .map(|a| (a.name(), a.playcount().unwrap(), a.listeners().unwrap()))
        .collect();
    assert_eq!(
        rows,
        [
            ("Olivia Rodrigo", 722_263_496, 3_431_916),
            ("Radiohead", 1_435_125_453, 8_512_078),
            ("Ariana Grande", 1_242_703_717, 4_752_771)
        ]
    );
    let first = &page.items()[0];
    assert_eq!(first.mbid(), Some("6925db17-f35e-42f3-a4eb-84ee6bf5d4b0"));
    assert_eq!(
        first.url(),
        Some("https://www.last.fm/music/Olivia+Rodrigo")
    );
    assert_eq!(first.images().len(), 5);
    // What the model does not name is still in the row.
    assert_eq!(first.json()["streamable"], "0");
}

#[test]
fn the_recorded_page_beyond_the_last_is_empty() {
    let page =
        decode::chart_get_top_artists(&raw(&methods::CHART_GET_TOP_ARTISTS, BEYOND)).unwrap();
    assert!(page.items().is_empty());
    assert_eq!((page.number(), page.total_pages()), (999_999, 200));
}

#[test]
fn the_recorded_chart_of_tracks_decodes() {
    let page = decode::chart_get_top_tracks(&raw(&methods::CHART_GET_TOP_TRACKS, TRACKS)).unwrap();
    assert_eq!(page.total(), 10_000);
    let rows: Vec<_> = page
        .items()
        .iter()
        .map(|t| {
            (
                t.name(),
                t.artist().name(),
                t.duration().map(|length| length.as_secs()),
                t.playcount().unwrap(),
                t.listeners().unwrap(),
            )
        })
        .collect();
    assert_eq!(
        rows,
        [
            ("Nicole Kidman", "ADÉLA", Some(219), 3_953_122, 313_352),
            ("Ain't In LA", "ADÉLA", Some(186), 4_862_273, 320_939),
            ("the cure", "Olivia Rodrigo", Some(296), 16_463_151, 920_111)
        ]
    );
    let first = &page.items()[0];
    assert_eq!(first.mbid(), Some("7c959127-f934-4ea8-ae27-99b2533cbf9e"));
    assert_eq!(
        first.artist().url(),
        Some("https://www.last.fm/music/AD%C3%89LA")
    );
    assert_eq!(first.images().len(), 4);
    assert_eq!(first.json()["streamable"]["fulltrack"], "0");
}

#[test]
fn the_recorded_chart_of_tags_decodes() {
    let page = decode::chart_get_top_tags(&raw(&methods::CHART_GET_TOP_TAGS, TAGS)).unwrap();
    // The reply has paging attributes, and the list does not read them.
    assert_eq!(page.len(), 3);
    assert_eq!(page.subject(), None);
    let rows: Vec<_> = page
        .items()
        .iter()
        .map(|t| (t.name(), t.reach().unwrap(), t.taggings().unwrap()))
        .collect();
    assert_eq!(
        rows,
        [
            ("rock", 403_467, 4_077_917),
            ("electronic", 262_938, 2_511_372),
            ("seen live", 82_570, 2_200_630)
        ]
    );
    assert_eq!(
        page.items()[2].url(),
        Some("https://www.last.fm/tag/seen+live")
    );
    assert_eq!(page.items()[0].json()["streamable"], "1");
}

#[test]
fn a_page_may_be_one_object_or_empty() {
    let one = artists(&paged("artists", "artist", artist_row("Only"), 1)).unwrap();
    assert_eq!(one.items().len(), 1);
    assert_eq!(one.items()[0].name(), "Only");

    for empty in [json!([]), json!("")] {
        let page = tracks(&paged("tracks", "track", empty, 0)).unwrap();
        assert!(page.items().is_empty());
    }
    // Rows that `@attr` promises and the page lacks are an error.
    names(
        tracks(&paged("tracks", "track", json!([]), 5)),
        "tracks.track",
    );
    // The list of tags is not held to its paging attributes, which the
    // service does not keep to.
    assert!(
        tags(&paged("tags", "tag", json!([]), 5))
            .unwrap()
            .is_empty()
    );
}

#[test]
fn what_a_row_may_leave_out_reads_as_none() {
    let mut artist = artist_row("A");
    artist.as_object_mut().unwrap().remove("mbid");
    let page = artists(&paged("artists", "artist", json!([artist]), 1)).unwrap();
    assert_eq!(page.items()[0].mbid(), None);

    for duration in [Some(json!("0")), Some(json!(0)), None] {
        let mut track = track_row("T");
        match duration {
            Some(value) => track["duration"] = value,
            None => {
                track.as_object_mut().unwrap().remove("duration");
            }
        }
        let page = tracks(&paged("tracks", "track", json!([track]), 1)).unwrap();
        assert_eq!(page.items()[0].duration(), None);
    }

    let mut tag = tag_row("t");
    tag["url"] = json!("");
    let page = tags(&paged("tags", "tag", json!([tag]), 1)).unwrap();
    assert_eq!(page.items()[0].url(), None);
}

#[test]
fn a_malformed_page_names_the_field() {
    for (field, path) in [
        ("playcount", "artists.artist[0].playcount"),
        ("listeners", "artists.artist[0].listeners"),
        ("name", "artists.artist[0].name"),
    ] {
        let mut row = artist_row("A");
        row[field] = json!([SECRET]);
        names(artists(&paged("artists", "artist", json!([row]), 1)), path);
        let mut row = artist_row("A");
        row.as_object_mut().unwrap().remove(field);
        names(artists(&paged("artists", "artist", json!([row]), 1)), path);
    }

    for (field, path) in [
        ("playcount", "tracks.track[0].playcount"),
        ("listeners", "tracks.track[0].listeners"),
        ("duration", "tracks.track[0].duration"),
        ("artist", "tracks.track[0].artist"),
    ] {
        let mut row = track_row("T");
        row[field] = json!(SECRET);
        names(tracks(&paged("tracks", "track", json!([row]), 1)), path);
    }

    for (field, path) in [
        ("reach", "tags.tag[0].reach"),
        ("taggings", "tags.tag[0].taggings"),
        ("name", "tags.tag[0].name"),
    ] {
        let mut row = tag_row("t");
        row[field] = json!({"x": SECRET});
        names(tags(&paged("tags", "tag", json!([row]), 1)), path);
    }

    names(
        artists(&paged("artists", "artist", json!([SECRET]), 1)),
        "artists.artist[0]",
    );
    names(
        artists(&json!({"artists": {"artist": []}})),
        "artists.@attr",
    );
    let mut page = paged("artists", "artist", json!([]), 0);
    page["artists"]["@attr"]
        .as_object_mut()
        .unwrap()
        .remove("total");
    names(artists(&page), "artists.@attr.total");
    names(artists(&json!({"topartists": {}})), "artists");
}

#[test]
fn a_response_to_another_method_is_refused() {
    let raw = raw(&methods::CHART_GET_TOP_TRACKS, TRACKS);
    let error = decode::chart_get_top_artists(&raw).unwrap_err();
    assert_eq!(error.kind(), ErrorKind::Decode);
    assert_eq!(error.method(), Some("chart.getTopTracks"));
}

#[test]
fn debug_leaves_out_the_text_of_the_response() {
    let artists = artists(&paged("artists", "artist", json!([artist_row(SECRET)]), 1)).unwrap();
    let tracks = tracks(&paged("tracks", "track", json!([track_row(SECRET)]), 1)).unwrap();
    let tags = tags(&paged("tags", "tag", json!([tag_row(SECRET)]), 1)).unwrap();
    let shown = format!(
        "{artists:?} {:?} {tracks:?} {:?} {tags:?} {:?}",
        artists.items()[0],
        tracks.items()[0],
        tags.items()[0]
    );
    assert!(!shown.contains(SECRET), "{shown}");
    assert!(shown.contains("Page {"), "{shown}");
}

#[test]
fn the_things_on_a_chart_say_the_counts_for_everyone_and_keep_their_json() {
    let artists =
        decode::chart_get_top_artists(&raw(&methods::CHART_GET_TOP_ARTISTS, ARTISTS)).unwrap();
    let artist: &Artist = artists[0].as_ref();
    assert_eq!(
        (artist.listeners(), artist.playcount()),
        (Some(3_431_916), Some(722_263_496))
    );
    assert_eq!(artist.json()["name"], "Olivia Rodrigo");

    let tracks =
        decode::chart_get_top_tracks(&raw(&methods::CHART_GET_TOP_TRACKS, TRACKS)).unwrap();
    let track: &Track = tracks[0].as_ref();
    assert_eq!(track.listeners(), Some(313_352));
    assert_eq!(track.duration(), Some(std::time::Duration::from_secs(219)));
    // The artist a track names has no counts of its own in this reply, and
    // the track names no album.
    assert_eq!(
        (track.artist().listeners(), track.artist().playcount()),
        (None, None)
    );
    assert_eq!(track.artist().json()["name"], "ADÉLA");
    assert!(track.album().is_none());

    let tags = decode::chart_get_top_tags(&raw(&methods::CHART_GET_TOP_TAGS, TAGS)).unwrap();
    let tag: &Tag = tags[0].as_ref();
    assert_eq!(
        (tag.taggings(), tag.reach()),
        (Some(4_077_917), Some(403_467))
    );
    assert_eq!(tag.json()["taggings"], "4077917");
}

#[test]
fn an_image_is_asked_for_by_size_or_by_its_place() {
    let page =
        decode::chart_get_top_artists(&raw(&methods::CHART_GET_TOP_ARTISTS, ARTISTS)).unwrap();
    let images = page[0].images();
    assert_eq!(images.len(), 5);
    assert_eq!(images.smallest().and_then(Image::size), Some("small"));
    assert_eq!(images.largest().and_then(Image::size), Some("mega"));
    assert_eq!(
        images.get("large").map(Image::url),
        Some("https://lastfm-img.freetls.fastly.net/i/u/174s/2a96cbd8b46e442fc41c2b86b821562f.png")
    );
    assert!(images.get("huge").is_none());
    let sizes: Vec<_> = images.into_iter().filter_map(Image::size).collect();
    assert_eq!(sizes, ["small", "medium", "large", "extralarge", "mega"]);

    // An artist without an image has none to give.
    let bare = artists(&paged("artists", "artist", json!([artist_row("A")]), 1)).unwrap();
    assert!(bare[0].images().is_empty());
    assert!(bare[0].images().largest().is_none());
    assert!(bare[0].images().smallest().is_none());
    assert!(format!("{:?}", bare[0].images()).contains("Images(0)"));
}
