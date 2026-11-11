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
// - The four `kelchm-weekly-*` bodies were recorded by the repository owner
//   from their own account on 2026-11-10 and are kept byte for byte.
//   recorded.
// - `weekly-chart-list.json` is the first two and the last two of the 1134
//   weeks of a list recorded the same day, with nothing else changed.
//   derived.
// - Everything else is built here from those shapes. synthetic.

const LIST: &[u8] = include_bytes!("../../../../fixtures/derived/weekly-chart-list.json");
const ARTISTS: &[u8] =
    include_bytes!("../../../../fixtures/recorded/kelchm-weekly-artist-chart.json");
const ALBUMS: &[u8] =
    include_bytes!("../../../../fixtures/recorded/kelchm-weekly-album-chart.json");
const TRACKS: &[u8] =
    include_bytes!("../../../../fixtures/recorded/kelchm-weekly-track-chart.json");
const EMPTY: &[u8] =
    include_bytes!("../../../../fixtures/recorded/kelchm-weekly-artist-chart-empty.json");

/// A string that appears in no error and no `Debug` output.
const SECRET: &str = "SYNTHETIC-RESPONSE-TEXT";

fn raw(spec: &'static MethodSpec, body: impl Into<Vec<u8>>) -> Raw {
    let request = Request::new(spec).param("user", "rj");
    protocol::decode(&request, HttpResponse::new(200, body.into())).unwrap()
}

fn list(body: &Value) -> Result<WeeklyChartList, Error> {
    WeeklyChartList::decode(&raw(&methods::USER_GET_WEEKLY_CHART_LIST, body.to_string()))
}

fn artists(body: &Value) -> Result<WeeklyArtistChart, Error> {
    WeeklyArtistChart::decode(&raw(
        &methods::USER_GET_WEEKLY_ARTIST_CHART,
        body.to_string(),
    ))
}

fn artist_chart(artist: Value) -> Value {
    json!({"weeklyartistchart": {"artist": artist,
           "@attr": {"from": "100", "user": "rj", "to": "200"}}})
}

fn artist_row(rank: &str, name: &str) -> Value {
    json!({"mbid": "", "url": "https://example.invalid/a", "name": name,
           "@attr": {"rank": rank}, "playcount": "7"})
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
fn the_chart_list_decodes() {
    let list = WeeklyChartList::decode(&raw(&methods::USER_GET_WEEKLY_CHART_LIST, LIST)).unwrap();
    assert_eq!(list.user(), "kelchm");
    let weeks: Vec<_> = list.ranges().iter().map(|w| (w.from(), w.to())).collect();
    assert_eq!(
        weeks,
        [
            (1_108_296_000, 1_108_900_800),
            (1_108_900_800, 1_109_505_600),
            (1_792_929_600, 1_793_534_400),
            (1_793_534_400, 1_794_139_200)
        ]
    );
    assert_eq!(list.ranges()[0].json()["#text"], "");
}

#[test]
fn the_recorded_artist_chart_decodes() {
    let chart =
        WeeklyArtistChart::decode(&raw(&methods::USER_GET_WEEKLY_ARTIST_CHART, ARTISTS)).unwrap();
    assert_eq!(
        (chart.user(), chart.from(), chart.to()),
        ("kelchm", 1_793_534_400, 1_794_139_200)
    );
    let rows: Vec<_> = chart
        .artists()
        .iter()
        .map(|a| (a.rank(), a.playcount(), a.artist().name()))
        .collect();
    assert_eq!(
        rows,
        [
            (1, 21, "Trent Reznor and Atticus Ross"),
            (2, 5, "Noah Kahan"),
            (3, 4, "Medium Build")
        ]
    );
    assert_eq!(chart.artists()[0].artist().mbid(), None);
    assert_eq!(
        chart.artists()[1].artist().url(),
        Some("https://www.last.fm/music/Noah+Kahan")
    );
}

#[test]
fn the_recorded_album_chart_decodes() {
    let chart =
        WeeklyAlbumChart::decode(&raw(&methods::USER_GET_WEEKLY_ALBUM_CHART, ALBUMS)).unwrap();
    assert_eq!((chart.from(), chart.to()), (1_793_534_400, 1_794_139_200));
    let rows: Vec<_> = chart
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
                19,
                Some("The Social Network"),
                "Trent Reznor and Atticus Ross"
            ),
            (2, 4, Some("Putting the Days to Bed"), "The Long Winters"),
            (3, 3, Some("takeaways"), "Medium Build")
        ]
    );
    // The artist is a name and an id here: no `url`, and an empty `mbid`
    // reads as none.
    let first = &chart.albums()[0];
    assert_eq!((first.artist().mbid(), first.artist().url()), (None, None));
    assert_eq!(
        chart.albums()[1].artist().mbid(),
        Some("966bd819-2ba1-4e1b-9d35-72385937e8ed")
    );
    assert_eq!(
        first.album().url(),
        Some("https://www.last.fm/music/Trent+Reznor+and+Atticus+Ross/The+Social+Network")
    );
}

#[test]
fn the_recorded_track_chart_decodes() {
    let chart =
        WeeklyTrackChart::decode(&raw(&methods::USER_GET_WEEKLY_TRACK_CHART, TRACKS)).unwrap();
    assert_eq!(chart.user(), "kelchm");
    let rows: Vec<_> = chart
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
            (1, 4, None, "Fire Island, AK", "The Long Winters"),
            (2, 3, None, "White Male Privilege", "Medium Build"),
            (3, 3, None, "Fossils", "Thao")
        ]
    );
    assert_eq!(
        chart.tracks()[0].track().mbid(),
        Some("785e81ce-d95a-3499-979a-67272042e89b")
    );
    assert_eq!(chart.tracks()[2].artist().mbid(), None);
}

#[test]
fn the_recorded_week_without_a_scrobble_is_empty() {
    let chart =
        WeeklyArtistChart::decode(&raw(&methods::USER_GET_WEEKLY_ARTIST_CHART, EMPTY)).unwrap();
    assert!(chart.artists().is_empty());
    assert_eq!((chart.from(), chart.to()), (1_108_296_000, 1_108_900_800));
}

#[test]
fn rows_may_be_one_object_or_none_at_all() {
    let chart = artists(&artist_chart(artist_row("1", "Only"))).unwrap();
    assert_eq!(chart.artists()[0].artist().name(), "Only");

    for artist in [Some(json!([])), Some(json!("")), None] {
        let mut body = artist_chart(json!([]));
        let mut weeks = json!({"weeklychartlist": {"chart": [], "@attr": {"user": "rj"}}});
        match artist {
            Some(rows) => {
                body["weeklyartistchart"]["artist"] = rows.clone();
                weeks["weeklychartlist"]["chart"] = rows;
            }
            None => {
                body["weeklyartistchart"]
                    .as_object_mut()
                    .unwrap()
                    .remove("artist");
                weeks["weeklychartlist"]
                    .as_object_mut()
                    .unwrap()
                    .remove("chart");
            }
        }
        assert!(artists(&body).unwrap().artists().is_empty());
        assert!(list(&weeks).unwrap().ranges().is_empty());
    }
}

#[test]
fn a_malformed_chart_names_the_field() {
    names(artists(&json!({"other": SECRET})), "weeklyartistchart");
    names(
        artists(&json!({"weeklyartistchart": {"artist": []}})),
        "weeklyartistchart.@attr",
    );
    for (member, field) in [
        ("user", "weeklyartistchart.@attr.user"),
        ("from", "weeklyartistchart.@attr.from"),
        ("to", "weeklyartistchart.@attr.to"),
    ] {
        let mut body = artist_chart(json!([]));
        body["weeklyartistchart"]["@attr"][member] = json!([SECRET]);
        names(artists(&body), field);
    }
    names(
        artists(&artist_chart(json!(SECRET))),
        "weeklyartistchart.artist",
    );
    names(
        artists(&artist_chart(json!([artist_row("1", "A"), SECRET]))),
        "weeklyartistchart.artist[1]",
    );
    names(
        artists(&artist_chart(json!([
            artist_row("1", "A"),
            artist_row(SECRET, "B")
        ]))),
        "weeklyartistchart.artist[1].@attr.rank",
    );
}

#[test]
fn a_malformed_chart_list_names_the_field() {
    names(list(&json!({"other": SECRET})), "weeklychartlist");
    names(
        list(&json!({"weeklychartlist": {"chart": []}})),
        "weeklychartlist.@attr",
    );
    names(
        list(&json!({"weeklychartlist": {"chart": [], "@attr": {"user": 7}}})),
        "weeklychartlist.@attr.user",
    );
    let weeks = |second: Value| {
        list(&json!({"weeklychartlist": {
            "chart": [{"#text": "", "from": "1", "to": "2"}, second],
            "@attr": {"user": "rj"}}}))
    };
    names(weeks(json!(SECRET)), "weeklychartlist.chart[1]");
    names(
        weeks(json!({"from": SECRET, "to": "3"})),
        "weeklychartlist.chart[1].from",
    );
    names(weeks(json!({"from": "2"})), "weeklychartlist.chart[1].to");
}

#[test]
fn a_response_to_another_method_is_refused() {
    let other = raw(&methods::USER_GET_TOP_ARTISTS, ARTISTS);
    for error in [
        WeeklyChartList::decode(&other).map(drop).unwrap_err(),
        WeeklyArtistChart::decode(&other).map(drop).unwrap_err(),
        WeeklyAlbumChart::decode(&other).map(drop).unwrap_err(),
        WeeklyTrackChart::decode(&other).map(drop).unwrap_err(),
    ] {
        assert_eq!(error.kind(), ErrorKind::Decode);
        assert!(error.to_string().contains("different method"));
    }
}

#[test]
fn debug_leaves_out_the_text_of_the_response() {
    let chart = artists(&artist_chart(json!([artist_row("1", SECRET)]))).unwrap();
    let weeks = list(&json!({"weeklychartlist": {
        "chart": [{"#text": SECRET, "from": "1", "to": "2"}],
        "@attr": {"user": SECRET}}}))
    .unwrap();
    let shown = format!(
        "{chart:?} {:?} {weeks:?} {:?}",
        chart.artists(),
        weeks.ranges()
    );
    assert!(!shown.contains(SECRET), "{shown}");
    assert!(!shown.contains("rj"), "{shown}");
    assert!(shown.contains("from: 100"), "{shown}");
    assert!(
        shown.contains("WeeklyChartRange { from: 1, to: 2, .. }"),
        "{shown}"
    );
}
