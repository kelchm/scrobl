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
// - The `recorded/tag-*` bodies were recorded by the repository owner from
//   the service's public lists on 2026-11-11 and are kept byte for byte.
//   recorded.
// - `derived/tag-info.json` follows a recording of the same day, with the
//   description users of the service wrote replaced by invented text.
//   derived.
// - `derived/tag-weekly-chart-list.json` is the first two and the last two
//   of the 1134 weeks of a recording of the same day. derived.
// - Everything else is built here from those shapes. synthetic.

const INFO: &[u8] = include_bytes!("../../../fixtures/derived/tag-info.json");
const SIMILAR: &[u8] = include_bytes!("../../../fixtures/recorded/tag-similar.json");
const ALBUMS: &[u8] = include_bytes!("../../../fixtures/recorded/tag-top-albums.json");
const UNKNOWN: &[u8] = include_bytes!("../../../fixtures/recorded/tag-top-albums-unknown.json");
const ARTISTS: &[u8] = include_bytes!("../../../fixtures/recorded/tag-top-artists.json");
const TRACKS: &[u8] = include_bytes!("../../../fixtures/recorded/tag-top-tracks.json");
const ALL: &[u8] = include_bytes!("../../../fixtures/recorded/tag-all-tags.json");
const WEEKS: &[u8] = include_bytes!("../../../fixtures/derived/tag-weekly-chart-list.json");

/// A string that appears in no error and no `Debug` output.
const SECRET: &str = "SYNTHETIC-RESPONSE-TEXT";

fn raw(spec: &'static MethodSpec, body: impl Into<Vec<u8>>) -> Raw {
    let request = Request::new(spec).param("tag", "indie");
    protocol::decode(&request, HttpResponse::new(200, body.into())).unwrap()
}

fn info(body: &Value) -> Result<TagInfo, Error> {
    TagInfo::decode(&raw(&methods::TAG_GET_INFO, body.to_string()))
}

fn similar(body: &Value) -> Result<SimilarTags, Error> {
    SimilarTags::decode(&raw(&methods::TAG_GET_SIMILAR, body.to_string()))
}

fn albums(body: &Value) -> Result<TagTopAlbumsPage, Error> {
    TagTopAlbumsPage::decode(&raw(&methods::TAG_GET_TOP_ALBUMS, body.to_string()))
}

fn artists(body: &Value) -> Result<TagTopArtistsPage, Error> {
    TagTopArtistsPage::decode(&raw(&methods::TAG_GET_TOP_ARTISTS, body.to_string()))
}

fn tracks(body: &Value) -> Result<TagTopTracksPage, Error> {
    TagTopTracksPage::decode(&raw(&methods::TAG_GET_TOP_TRACKS, body.to_string()))
}

fn all(body: &Value) -> Result<AllTags, Error> {
    AllTags::decode(&raw(&methods::TAG_GET_TOP_TAGS, body.to_string()))
}

fn weeks(body: &Value) -> Result<TagWeeklyChartList, Error> {
    TagWeeklyChartList::decode(&raw(&methods::TAG_GET_WEEKLY_CHART_LIST, body.to_string()))
}

/// A page of `total` rows in all, of which `rows` are on this one.
fn paged(root: &str, key: &str, rows: Value, total: u64) -> Value {
    json!({root: {key: rows, "@attr": {
        "tag": "indie", "page": "1", "perPage": "50",
        "totalPages": total.min(1).to_string(), "total": total.to_string()}}})
}

fn album_row(name: &str) -> Value {
    json!({"name": name, "mbid": "", "url": "https://example.invalid/b",
           "artist": {"name": "Artist", "mbid": "", "url": "https://example.invalid/a"},
           "@attr": {"rank": "1"}})
}

fn artist_row(name: &str) -> Value {
    json!({"name": name, "mbid": "", "url": "https://example.invalid/a",
           "streamable": "0", "@attr": {"rank": "1"}})
}

fn track_row(name: &str) -> Value {
    json!({"name": name, "duration": "180", "mbid": "", "url": "https://example.invalid/t",
           "streamable": {"#text": "0", "fulltrack": "0"},
           "artist": {"name": "Artist", "mbid": "", "url": "https://example.invalid/a"},
           "@attr": {"rank": "1"}})
}

fn tag_of(name: &str) -> Value {
    json!({"tag": {"name": name, "total": 7, "reach": 3,
                   "wiki": {"summary": "Summary.", "content": "Content."}}})
}

fn all_of(tag: Value) -> Value {
    json!({"toptags": {"@attr": {"offset": 0, "num_res": 50, "total": 9}, "tag": tag}})
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
fn the_derived_tag_decodes() {
    let tag = TagInfo::decode(&raw(&methods::TAG_GET_INFO, INFO)).unwrap();
    assert_eq!(tag.name(), "indie");
    assert_eq!((tag.total(), tag.reach()), (2_068_156, 260_909));
    let summary = tag.summary().unwrap();
    assert!(summary.starts_with("An invented description"));
    assert!(summary.ends_with("Read more on Last.fm</a>."));
    assert!(tag.content().unwrap().contains("second paragraph"));
    // The counts are numbers on the wire, where a page's are strings.
    assert_eq!(tag.json()["total"], 2_068_156);
}

#[test]
fn a_tag_may_lack_a_description_and_its_counts_may_be_strings() {
    let mut body = tag_of("t");
    body["tag"]["wiki"]["content"] = json!("");
    body["tag"]["total"] = json!("7");
    let tag = info(&body).unwrap();
    assert_eq!(tag.total(), 7);
    assert_eq!(tag.summary(), Some("Summary."));
    assert_eq!(tag.content(), None);

    body["tag"].as_object_mut().unwrap().remove("wiki");
    let tag = info(&body).unwrap();
    assert_eq!((tag.summary(), tag.content()), (None, None));
}

#[test]
fn a_malformed_tag_names_the_field() {
    for (field, path) in [
        ("name", "tag.name"),
        ("total", "tag.total"),
        ("reach", "tag.reach"),
    ] {
        let mut body = tag_of("t");
        body["tag"][field] = json!([SECRET]);
        names(info(&body), path);
        let mut body = tag_of("t");
        body["tag"].as_object_mut().unwrap().remove(field);
        names(info(&body), path);
    }
    let mut body = tag_of("t");
    body["tag"]["wiki"] = json!(SECRET);
    names(info(&body), "tag.wiki");
    let mut body = tag_of("t");
    body["tag"]["wiki"]["summary"] = json!(5);
    names(info(&body), "tag.wiki.summary");
    // What the service sends when no tag is named: an empty object.
    names(info(&json!({})), "tag");
}

#[test]
fn the_recorded_similar_tags_are_none() {
    let list = SimilarTags::decode(&raw(&methods::TAG_GET_SIMILAR, SIMILAR)).unwrap();
    assert_eq!(list.tag(), "n/a");
    assert!(list.tags().is_empty());
}

#[test]
fn similar_tags_decode_as_the_official_sample_lays_them_out() {
    // provenance: synthetic, after the sample on the method's official page.
    let body = json!({"similartags": {
        "tag": [
            {"name": "italo disco", "url": "https://example.invalid/tag/italo", "streamable": "1"},
            {"name": "funk", "url": ""}
        ],
        "@attr": {"tag": "disco"}
    }});
    let list = similar(&body).unwrap();
    assert_eq!(list.tag(), "disco");
    let rows: Vec<_> = list.tags().iter().map(|t| (t.name(), t.url())).collect();
    assert_eq!(
        rows,
        [
            ("italo disco", Some("https://example.invalid/tag/italo")),
            ("funk", None)
        ]
    );
    assert_eq!(list.tags()[0].json()["streamable"], "1");

    let one =
        similar(&json!({"similartags": {"tag": {"name": "funk"}, "@attr": {"tag": "disco"}}}));
    assert_eq!(one.unwrap().tags().len(), 1);

    names(
        similar(&json!({"similartags": {"tag": [{"url": SECRET}], "@attr": {"tag": "disco"}}})),
        "similartags.tag[0].name",
    );
    names(
        similar(&json!({"similartags": {"tag": []}})),
        "similartags.@attr",
    );
    names(
        similar(&json!({"similartags": {"tag": [], "@attr": {}}})),
        "similartags.@attr.tag",
    );
}

#[test]
fn the_recorded_albums_of_a_tag_decode() {
    let page = TagTopAlbumsPage::decode(&raw(&methods::TAG_GET_TOP_ALBUMS, ALBUMS)).unwrap();
    assert_eq!(page.tag(), "indie");
    let attr = page.attr();
    assert_eq!(
        (
            attr.page(),
            attr.per_page(),
            attr.total_pages(),
            attr.total()
        ),
        (1, 3, 7067, 21_199)
    );
    let rows: Vec<_> = page
        .albums()
        .iter()
        .map(|a| {
            (
                a.album().title(),
                a.artist().name(),
                a.album().mbid().is_some(),
            )
        })
        .collect();
    // The second row has no `mbid` member at all.
    assert_eq!(
        rows,
        [
            (Some("Le Tigre"), "Le Tigre", true),
            (Some("Inside In / Inside Out"), "The Kooks", false),
            (Some("Beacon"), "Two Door Cinema Club", true)
        ]
    );
    assert_eq!(
        page.albums()[1].album().url(),
        Some("https://www.last.fm/music/The+Kooks/Inside+In+%2F+Inside+Out")
    );
    assert_eq!(page.albums()[0].album().images().len(), 4);
    // The rank the model does not read is still in the row.
    assert_eq!(page.albums()[2].json()["@attr"]["rank"], "3");
}

#[test]
fn the_recorded_page_of_a_tag_nobody_used_is_empty() {
    let page = TagTopAlbumsPage::decode(&raw(&methods::TAG_GET_TOP_ALBUMS, UNKNOWN)).unwrap();
    assert_eq!(page.tag(), "zzqqxx-no-such-tag-scrobl");
    assert!(page.albums().is_empty());
    assert_eq!((page.attr().total(), page.attr().total_pages()), (0, 0));
}

#[test]
fn the_recorded_artists_and_tracks_of_a_tag_decode() {
    let page = TagTopArtistsPage::decode(&raw(&methods::TAG_GET_TOP_ARTISTS, ARTISTS)).unwrap();
    assert_eq!((page.tag(), page.attr().total()), ("indie", 82_722));
    let names: Vec<_> = page.artists().iter().map(|a| a.artist().name()).collect();
    assert_eq!(
        names,
        ["The Neighbourhood", "Arctic Monkeys", "Lana Del Rey"]
    );
    assert_eq!(
        page.artists()[0].artist().mbid(),
        Some("53b06622-ec51-4924-a554-58b0affde6b5")
    );
    assert_eq!(page.artists()[0].artist().images().len(), 5);
    assert_eq!(page.artists()[0].json()["streamable"], "0");

    let page = TagTopTracksPage::decode(&raw(&methods::TAG_GET_TOP_TRACKS, TRACKS)).unwrap();
    assert_eq!((page.tag(), page.attr().total()), ("indie", 1296));
    let rows: Vec<_> = page
        .tracks()
        .iter()
        .map(|t| (t.track().name(), t.artist().name(), t.duration()))
        .collect();
    assert_eq!(
        rows,
        [
            ("Sweater Weather", "The Neighbourhood", Some(240)),
            ("505", "Arctic Monkeys", Some(253)),
            (
                "Why'd You Only Call Me When You're High?",
                "Arctic Monkeys",
                Some(146)
            )
        ]
    );
    assert_eq!(page.tracks()[0].track().images().len(), 4);
    // What the model does not name is still in the row.
    let row = page.tracks()[2].json();
    assert_eq!(row["streamable"]["fulltrack"], "0");
    assert_eq!(row["@attr"]["rank"], "3");
}

#[test]
fn a_page_of_a_tag_may_be_one_object_and_names_what_is_malformed() {
    let one = albums(&paged("albums", "album", album_row("Only"), 1)).unwrap();
    assert_eq!(one.albums()[0].album().title(), Some("Only"));
    let one = artists(&paged("topartists", "artist", artist_row("Only"), 1)).unwrap();
    assert_eq!(one.artists()[0].artist().name(), "Only");
    let mut track = track_row("Only");
    track["duration"] = json!("0");
    let one = tracks(&paged("tracks", "track", track, 1)).unwrap();
    assert_eq!(one.tracks()[0].duration(), None);

    let mut row = album_row("A");
    row["artist"] = json!(SECRET);
    names(
        albums(&paged("albums", "album", json!([row]), 1)),
        "albums.album[0].artist",
    );
    let mut row = artist_row("A");
    row["name"] = json!(5);
    names(
        artists(&paged("topartists", "artist", json!([row]), 1)),
        "topartists.artist[0].name",
    );
    let mut row = track_row("T");
    row["duration"] = json!(SECRET);
    names(
        tracks(&paged("tracks", "track", json!([row]), 1)),
        "tracks.track[0].duration",
    );
    let mut page = paged("albums", "album", json!([]), 0);
    page["albums"]["@attr"]
        .as_object_mut()
        .unwrap()
        .remove("tag");
    names(albums(&page), "albums.@attr.tag");
    names(
        tracks(&paged("tracks", "track", json!([]), 4)),
        "tracks.track",
    );
}

#[test]
fn the_recorded_list_of_every_tag_decodes() {
    let list = AllTags::decode(&raw(&methods::TAG_GET_TOP_TAGS, ALL)).unwrap();
    assert_eq!((list.offset(), list.total()), (0, 2_896_879));
    let rows: Vec<_> = list
        .tags()
        .iter()
        .map(|t| (t.name(), t.count(), t.reach()))
        .collect();
    assert_eq!(
        rows,
        [
            ("rock", 4_077_917, 403_467),
            ("electronic", 2_511_372, 262_938),
            ("seen live", 2_200_630, 82_570)
        ]
    );
    assert_eq!(list.tags()[0].json()["count"], 4_077_917);
}

#[test]
fn a_malformed_list_of_every_tag_names_the_field() {
    let row = json!({"name": "t", "count": 7, "reach": 3});
    assert_eq!(all(&all_of(row.clone())).unwrap().tags().len(), 1);

    for (field, path) in [
        ("name", "toptags.tag[0].name"),
        ("count", "toptags.tag[0].count"),
        ("reach", "toptags.tag[0].reach"),
    ] {
        let mut bad = row.clone();
        bad[field] = json!([SECRET]);
        names(all(&all_of(json!([bad]))), path);
    }
    for field in ["offset", "total"] {
        let mut body = all_of(json!([]));
        body["toptags"]["@attr"][field] = json!(SECRET);
        names(all(&body), &format!("toptags.@attr.{field}"));
    }
    names(all(&json!({"toptags": {"tag": []}})), "toptags.@attr");
}

#[test]
fn a_list_of_every_tag_is_empty_only_where_its_attr_allows() {
    // `all_of` puts the offset at 0 of 9 tags, so rows are owed.
    for missing in [json!([]), json!("")] {
        names(all(&all_of(missing)), "toptags.tag");
    }
    let mut renamed = all_of(json!([]));
    let list = renamed["toptags"].as_object_mut().unwrap();
    let rows = list.remove("tag").unwrap();
    list.insert("tags".to_owned(), rows);
    names(all(&renamed), "toptags.tag");

    // Past the last tag, or with none asked for, there is nothing to owe.
    for (member, value) in [
        ("offset", json!(9)),
        ("offset", json!(50)),
        ("num_res", json!(0)),
    ] {
        let mut body = all_of(json!([]));
        body["toptags"]["@attr"][member] = value;
        assert!(all(&body).unwrap().tags().is_empty(), "{member}");
    }
    // The number asked for is not required, but a malformed one is named.
    let mut body = all_of(json!([{"name": "t", "count": 7, "reach": 3}]));
    body["toptags"]["@attr"]
        .as_object_mut()
        .unwrap()
        .remove("num_res");
    assert_eq!(all(&body).unwrap().tags().len(), 1);
    body["toptags"]["@attr"]["num_res"] = json!(SECRET);
    names(all(&body), "toptags.@attr.num_res");
}

#[test]
fn the_derived_list_of_weeks_decodes() {
    let list =
        TagWeeklyChartList::decode(&raw(&methods::TAG_GET_WEEKLY_CHART_LIST, WEEKS)).unwrap();
    assert_eq!(list.tag(), "indie");
    let spans: Vec<_> = list.ranges().iter().map(|w| (w.from(), w.to())).collect();
    assert_eq!(
        spans,
        [
            (1_108_296_000, 1_108_900_800),
            (1_108_900_800, 1_109_505_600),
            (1_792_929_600, 1_793_534_400),
            (1_793_534_400, 1_794_139_200)
        ]
    );
    assert_eq!(list.ranges()[0].json()["#text"], "");

    names(
        weeks(
            &json!({"weeklychartlist": {"chart": [{"from": SECRET, "to": "2"}],
                                          "@attr": {"tag": "indie"}}}),
        ),
        "weeklychartlist.chart[0].from",
    );
    names(
        weeks(&json!({"weeklychartlist": {"chart": [], "@attr": {"user": "rj"}}})),
        "weeklychartlist.@attr.tag",
    );
}

#[test]
fn a_response_to_another_method_is_refused() {
    // A user's list of weeks has the same root as a tag's.
    let raw = raw(&methods::USER_GET_WEEKLY_CHART_LIST, WEEKS);
    let error = TagWeeklyChartList::decode(&raw).unwrap_err();
    assert_eq!(error.kind(), ErrorKind::Decode);
    assert_eq!(error.method(), Some("user.getWeeklyChartList"));

    let raw = self::raw(&methods::TAG_GET_TOP_ARTISTS, ARTISTS);
    assert_eq!(
        TagTopTracksPage::decode(&raw).unwrap_err().kind(),
        ErrorKind::Decode
    );
    assert_eq!(TagInfo::decode(&raw).unwrap_err().kind(), ErrorKind::Decode);
    assert_eq!(AllTags::decode(&raw).unwrap_err().kind(), ErrorKind::Decode);
    assert_eq!(
        SimilarTags::decode(&raw).unwrap_err().kind(),
        ErrorKind::Decode
    );
}

#[test]
fn debug_leaves_out_the_text_of_the_response() {
    let mut body = tag_of(SECRET);
    body["tag"]["wiki"]["summary"] = json!(SECRET);
    let info = info(&body).unwrap();
    let similar = similar(&json!({"similartags": {
        "tag": [{"name": SECRET, "url": SECRET}], "@attr": {"tag": SECRET}}}))
    .unwrap();
    let mut page = paged("albums", "album", json!([album_row(SECRET)]), 1);
    page["albums"]["@attr"]["tag"] = json!(SECRET);
    let albums = albums(&page).unwrap();
    let artists = artists(&paged(
        "topartists",
        "artist",
        json!([artist_row(SECRET)]),
        1,
    ))
    .unwrap();
    let tracks = tracks(&paged("tracks", "track", json!([track_row(SECRET)]), 1)).unwrap();
    let all = all(&all_of(json!([{"name": SECRET, "count": 7, "reach": 3}]))).unwrap();
    let weeks = weeks(&json!({"weeklychartlist": {
        "chart": [{"#text": SECRET, "from": "1", "to": "2"}], "@attr": {"tag": SECRET}}}))
    .unwrap();
    let shown = format!(
        "{info:?} {similar:?} {:?} {albums:?} {:?} {artists:?} {:?} {tracks:?} {:?} {all:?} {:?} {weeks:?}",
        similar.tags()[0],
        albums.albums()[0],
        artists.artists()[0],
        tracks.tracks()[0],
        all.tags()[0],
    );
    assert!(!shown.contains(SECRET), "{shown}");
    assert!(shown.contains("TagTopAlbumsPage"), "{shown}");
}
