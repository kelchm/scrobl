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

fn artists(body: &Value) -> Result<Page<UserArtist>, Error> {
    decode::user_get_top_artists(&raw(&methods::USER_GET_TOP_ARTISTS, body.to_string()))
}

fn albums(body: &Value) -> Result<Page<UserAlbum>, Error> {
    decode::user_get_top_albums(&raw(&methods::USER_GET_TOP_ALBUMS, body.to_string()))
}

fn tracks(body: &Value) -> Result<Page<UserTrack>, Error> {
    decode::user_get_top_tracks(&raw(&methods::USER_GET_TOP_TRACKS, body.to_string()))
}

fn tags(body: &Value) -> Result<List<UserTag>, Error> {
    decode::user_get_top_tags(&raw(&methods::USER_GET_TOP_TAGS, body.to_string()))
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
    let page = decode::user_get_top_artists(&raw(&methods::USER_GET_TOP_ARTISTS, ARTISTS)).unwrap();

    assert_eq!(page.subject(), Some("kelchm"));
    assert_eq!(
        (
            page.number(),
            page.per_page(),
            page.total_pages(),
            page.total()
        ),
        (1, 3, 3369, 10_105)
    );

    let rows: Vec<_> = page
        .ranked()
        .map(|(place, a)| (place, a.playcount(), a.artist().name()))
        .collect();
    assert_eq!(
        rows,
        [
            (1, 5584, "Portugal. The Man"),
            (2, 4146, "Bright Eyes"),
            (3, 3726, "Elliott Smith")
        ]
    );
    let first = &page.items()[0];
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
    let page = decode::user_get_top_artists(&raw(&methods::USER_GET_TOP_ARTISTS, BEYOND)).unwrap();
    assert!(page.items().is_empty());
    assert_eq!(
        (page.number(), page.total_pages()),
        (9999, 11),
        "the list is `[]` on a page past the last"
    );
    assert_eq!(page.total(), 10_105);
}

#[test]
fn the_recorded_top_albums_decode() {
    let page = decode::user_get_top_albums(&raw(&methods::USER_GET_TOP_ALBUMS, ALBUMS)).unwrap();
    assert_eq!((page.total_pages(), page.total()), (7258, 21_774));

    let rows: Vec<_> = page
        .ranked()
        .map(|(place, a)| {
            (
                place,
                a.playcount(),
                a.album().name(),
                a.album().artist().unwrap().name(),
            )
        })
        .collect();
    assert_eq!(
        rows,
        [
            (
                1,
                1675,
                "The Social Network",
                "Trent Reznor and Atticus Ross"
            ),
            (2, 888, "The Satanic Satanist", "Portugal. The Man"),
            (3, 770, "The Glass Passenger", "Jack's Mannequin")
        ]
    );
    let first = &page.items()[0];
    assert_eq!(
        first.album().mbid(),
        Some("121682be-5d99-40b9-8ea0-4b20a7fe4a38")
    );
    assert_eq!(
        first.album().url(),
        Some("https://www.last.fm/music/Trent+Reznor+and+Atticus+Ross/The+Social+Network")
    );
    // The service sent an empty artist `mbid`; it is present in the row.
    assert_eq!(first.album().artist().unwrap().mbid(), None);
    assert_eq!(first.json()["artist"]["mbid"], "");
    assert_eq!(
        page.items()[1].album().artist().unwrap().mbid(),
        Some("3599a39e-4e10-4cb5-90d4-c8a015ebc73b")
    );
}

#[test]
fn the_recorded_top_tracks_decode() {
    let page = decode::user_get_top_tracks(&raw(&methods::USER_GET_TOP_TRACKS, TRACKS)).unwrap();
    assert_eq!((page.total_pages(), page.total()), (15_687, 47_059));

    let rows: Vec<_> = page
        .ranked()
        .map(|(place, t)| {
            (
                place,
                t.playcount(),
                t.track().duration().map(|length| length.as_secs()),
                t.track().name(),
                t.track().artist().name(),
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
    let first = &page.items()[0];
    assert_eq!(
        first.track().mbid(),
        Some("0b34b2a4-d282-4fba-a1d8-f9970a0852c3")
    );
    assert_eq!(
        first.track().url(),
        Some("https://www.last.fm/music/Fickle+Friends/_/Swim")
    );
    assert_eq!(
        first.track().artist().url(),
        Some("https://www.last.fm/music/Fickle+Friends")
    );
    assert_eq!(first.json()["streamable"]["fulltrack"], "0");
}

#[test]
fn the_recorded_top_tags_decode() {
    let top = decode::user_get_top_tags(&raw(&methods::USER_GET_TOP_TAGS, TAGS)).unwrap();
    assert_eq!(top.subject(), Some("kelchm"));
    let rows: Vec<_> = top
        .items()
        .iter()
        .map(|t| (t.tag().name(), t.count(), t.tag().url()))
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
    assert_eq!(top.items()[2].json()["name"], "to buy");
}

#[test]
fn a_single_row_may_come_as_an_object() {
    let page = artists(&paged("topartists", "artist", artist_row("1", "Only"), 1)).unwrap();
    assert_eq!(page.items()[0].artist().name(), "Only");

    let page = albums(&paged("topalbums", "album", album_row("1", "Only"), 1)).unwrap();
    assert_eq!(page.items()[0].album().name(), "Only");

    let page = tracks(&paged("toptracks", "track", track_row("1", "Only"), 1)).unwrap();
    assert_eq!(page.items()[0].track().name(), "Only");

    let top = tags(&tag_list(tag_row("only"))).unwrap();
    assert_eq!(top.items()[0].tag().name(), "only");
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
        assert!(tracks(&body).unwrap().items().is_empty());
        assert!(tags(&list).unwrap().items().is_empty());
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
    // The rank the service sends is not read: a row's place is the page's
    // to give. So a row decodes whatever its rank says, and without one.
    assert_eq!(artist_page(artist_row(SECRET, "B")).unwrap().len(), 2);
    let unranked = artist_page(with(artist_row("2", "B"), &|row| {
        row.as_object_mut().unwrap().remove("@attr");
    }))
    .unwrap();
    assert_eq!(unranked.ranked().last().map(|(place, _)| place), Some(2));
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
    assert_eq!(page.items()[0].track().duration(), None);
    assert_eq!(page.items()[0].track().mbid(), None);

    // A length the service does not know comes as 0.
    let mut unknown = track_row("1", "A");
    unknown["duration"] = json!("0");
    let page = tracks(&paged("toptracks", "track", json!([unknown]), 1)).unwrap();
    assert_eq!(page.items()[0].track().duration(), None);
    assert_eq!(page.items()[0].json()["duration"], "0");

    let page = albums(&paged("topalbums", "album", json!([album_row("1", "")]), 1)).unwrap();
    assert_eq!(page.items()[0].album().name(), "");

    let top = tags(&tag_list(json!([{"name": "a", "count": 3}]))).unwrap();
    assert_eq!(
        (top.items()[0].count(), top.items()[0].tag().url()),
        (3, None)
    );
}

#[test]
fn a_repeated_member_is_an_error() {
    let body = r#"{"toptags": {"tag": [], "tag": [], "@attr": {"user": "rj"}}}"#;
    let error = decode::user_get_top_tags(&raw(&methods::USER_GET_TOP_TAGS, body)).unwrap_err();
    assert_eq!(error.kind(), ErrorKind::Decode);
}

#[test]
fn a_response_to_another_method_is_refused() {
    let other = raw(&methods::USER_GET_RECENT_TRACKS, ARTISTS);
    for error in [
        decode::user_get_top_artists(&other).map(drop).unwrap_err(),
        decode::user_get_top_albums(&other).map(drop).unwrap_err(),
        decode::user_get_top_tracks(&other).map(drop).unwrap_err(),
        decode::user_get_top_tags(&other).map(drop).unwrap_err(),
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
        artists.items(),
        albums.items(),
        tracks.items(),
        tags.items()
    );
    assert!(!shown.contains(SECRET), "{shown}");
    assert!(!shown.contains("rj"), "{shown}");
    assert!(shown.contains("playcount: 7"), "{shown}");
    assert!(shown.contains("UserTrack"), "{shown}");
    // The length is the track's, and the track says it.
    assert!(
        format!("{:?}", tracks.items()[0].track()).contains("duration: Some(180s)"),
        "{shown}"
    );
}

/// What the service calls each of `images`.
fn sizes(images: &[Image]) -> Vec<Option<&str>> {
    images.iter().map(Image::size).collect()
}

/// A page whose one artist has `image` under that name, or no such member.
fn artist_with(image: Option<Value>) -> Result<Page<UserArtist>, Error> {
    let mut row = artist_row("1", "A");
    if let Some(image) = image {
        row["image"] = image;
    }
    artists(&paged("topartists", "artist", json!([row]), 1))
}

#[test]
fn the_recorded_rows_carry_their_images() {
    let artists =
        decode::user_get_top_artists(&raw(&methods::USER_GET_TOP_ARTISTS, ARTISTS)).unwrap();
    let images = artists.items()[0].artist().images();
    assert_eq!(
        sizes(images),
        ["small", "medium", "large", "extralarge", "mega"].map(Some)
    );
    assert_eq!(
        images[0].url(),
        "https://lastfm-img.freetls.fastly.net/i/u/34s/2a96cbd8b46e442fc41c2b86b821562f.png"
    );

    let albums = decode::user_get_top_albums(&raw(&methods::USER_GET_TOP_ALBUMS, ALBUMS)).unwrap();
    assert_eq!(albums.subject(), Some("kelchm"));
    let images = albums.items()[0].album().images();
    assert_eq!(
        sizes(images),
        ["small", "medium", "large", "extralarge"].map(Some)
    );
    assert_eq!(
        images[3].url(),
        "https://lastfm-img.freetls.fastly.net/i/u/300x300/26b6daf514da76b731dbfb02ffe23325.jpg"
    );
    // The artist of an album is a name, an id and a page: no image.
    assert!(
        albums.items()[0]
            .album()
            .artist()
            .unwrap()
            .images()
            .is_empty()
    );

    let tracks = decode::user_get_top_tracks(&raw(&methods::USER_GET_TOP_TRACKS, TRACKS)).unwrap();
    assert_eq!(tracks.subject(), Some("kelchm"));
    assert_eq!(tracks.items()[0].track().images().len(), 4);
    assert!(tracks.items()[0].track().artist().images().is_empty());
}

#[test]
fn images_may_be_a_list_one_object_or_none() {
    for none in [None, Some(json!("")), Some(json!([]))] {
        let page = artist_with(none).unwrap();
        assert!(page.items()[0].artist().images().is_empty());
    }

    let one = json!({"size": "large", "#text": "https://example.invalid/l.png"});
    for image in [json!([one.clone()]), one] {
        let page = artist_with(Some(image)).unwrap();
        let [only] = &page.items()[0].artist().images()[..] else {
            panic!("one image expected");
        };
        assert_eq!(only.size(), Some("large"));
        assert_eq!(only.url(), "https://example.invalid/l.png");
    }
}

#[test]
fn an_image_without_an_address_is_left_out() {
    let page = artist_with(Some(json!([
        {"size": "small", "#text": ""},
        {"size": "medium"},
        {"size": "", "#text": "https://example.invalid/x.png"},
        {"#text": "https://example.invalid/y.png"}
    ])))
    .unwrap();
    let row = &page.items()[0];
    let images = row.artist().images();
    assert_eq!(sizes(images), [None, None]);
    assert_eq!(
        (images[0].url(), images[1].url()),
        (
            "https://example.invalid/x.png",
            "https://example.invalid/y.png"
        )
    );
    // The row still holds all four.
    assert_eq!(row.json()["image"].as_array().unwrap().len(), 4);
}

#[test]
fn a_malformed_image_names_the_field() {
    for (image, field) in [
        (json!(5), "topartists.artist[0].image"),
        (json!(SECRET), "topartists.artist[0].image"),
        (json!([SECRET]), "topartists.artist[0].image[0]"),
        (
            json!([{"size": "small", "#text": 5}]),
            "topartists.artist[0].image[0].#text",
        ),
        (
            json!([{"size": [SECRET], "#text": ""}]),
            "topartists.artist[0].image[0].size",
        ),
    ] {
        names(artist_with(Some(image)), field);
    }

    // The same holds for an album, for a track and for the artist a row names.
    let mut album = album_row("1", "A");
    album["image"] = json!(5);
    names(
        albums(&paged("topalbums", "album", json!([album]), 1)),
        "topalbums.album[0].image",
    );
    let mut track = track_row("1", "T");
    track["image"] = json!([5]);
    names(
        tracks(&paged("toptracks", "track", json!([track]), 1)),
        "toptracks.track[0].image[0]",
    );
    let mut track = track_row("1", "T");
    track["artist"]["image"] = json!(SECRET);
    names(
        tracks(&paged("toptracks", "track", json!([track]), 1)),
        "toptracks.track[0].artist.image",
    );
}

#[test]
fn debug_of_an_image_leaves_out_its_text() {
    let page = artist_with(Some(json!([{"size": SECRET, "#text": SECRET}]))).unwrap();
    let image = &page.items()[0].artist().images()[0];
    assert_eq!(image.url(), SECRET);
    assert!(!format!("{image:?} {page:?}").contains(SECRET));
}

/// A page of artists `number` of `total_pages`, two to a page.
fn numbered(number: u32, total_pages: u32, rows: &[&str]) -> Page<UserArtist> {
    let rows: Vec<Value> = rows.iter().map(|name| artist_row("1", name)).collect();
    artists(&json!({"topartists": {"artist": rows, "@attr": {
        "user": "rj", "page": number.to_string(), "perPage": "2",
        "totalPages": total_pages.to_string(), "total": (total_pages * 2).to_string()}}}))
    .unwrap()
}

#[test]
fn a_page_knows_its_place_and_the_place_of_each_row() {
    let page = numbered(3, 5, &["E", "F"]);
    assert_eq!(
        (
            page.number(),
            page.per_page(),
            page.total_pages(),
            page.total()
        ),
        (3, 2, 5, 10)
    );
    assert_eq!(page.subject(), Some("rj"));
    assert_eq!(page.next_page(), Some(4));
    // The place is counted from the page's own, whatever rank a row carries.
    let places: Vec<_> = page
        .ranked()
        .map(|(place, row)| (place, row.artist().name()))
        .collect();
    assert_eq!(places, [(5, "E"), (6, "F")]);

    // The last page has no next, and neither has a page beyond it.
    assert_eq!(numbered(5, 5, &["I", "J"]).next_page(), None);
    assert_eq!(numbered(9, 5, &[]).next_page(), None);
    assert_eq!(numbered(1, 1, &["A", "B"]).ranked().next().unwrap().0, 1);
}

#[test]
fn a_page_is_read_as_a_slice_of_its_rows_or_taken_apart() {
    let page = numbered(1, 1, &["A", "B"]);
    assert_eq!((page.len(), page.items().len()), (2, 2));
    assert_eq!(page[1].artist().name(), "B");
    let borrowed: Vec<_> = (&page).into_iter().map(|row| row.artist().name()).collect();
    assert_eq!(borrowed, ["A", "B"]);
    let owned: Vec<UserArtist> = page.clone().into_iter().collect();
    assert_eq!(owned.len(), 2);
    assert_eq!(page.into_items()[0].artist().name(), "A");
}

#[test]
fn a_page_number_that_no_request_could_name_does_not_decode() {
    let mut body = paged("topartists", "artist", json!([]), 0);
    body["topartists"]["@attr"]["page"] = json!("5000000000");
    names(artists(&body), "topartists.@attr.page");
    let mut body = paged("topartists", "artist", json!([]), 0);
    body["topartists"]["@attr"]["perPage"] = json!(5_000_000_000_u64);
    names(artists(&body), "topartists.@attr.perPage");
}

#[test]
fn a_list_is_read_as_a_slice_of_its_rows_or_taken_apart() {
    let list = tags(&tag_list(json!([tag_row("a"), tag_row("b")]))).unwrap();
    assert_eq!(list.subject(), Some("rj"));
    assert_eq!((list.len(), list.items().len()), (2, 2));
    let places: Vec<_> = list
        .ranked()
        .map(|(place, tag)| (place, tag.tag().name()))
        .collect();
    assert_eq!(places, [(1, "a"), (2, "b")]);
    let borrowed: Vec<_> = (&list).into_iter().map(|row| row.tag().name()).collect();
    assert_eq!(borrowed, ["a", "b"]);
    assert_eq!(list.clone().into_iter().count(), 2);
    assert_eq!(list.into_items()[1].tag().name(), "b");
    assert!(format!("{:?}", tags(&tag_list(json!([tag_row(SECRET)]))).unwrap()).contains("List {"));
}

#[test]
fn a_row_gives_up_the_thing_it_wraps() {
    let artists =
        decode::user_get_top_artists(&raw(&methods::USER_GET_TOP_ARTISTS, ARTISTS)).unwrap();
    let row = artists[0].clone();
    let artist: &Artist = row.as_ref();
    assert_eq!(artist.name(), "Portugal. The Man");
    // The user's plays are the row's. The artist's own are not in this reply.
    assert_eq!((row.playcount(), artist.playcount()), (5584, None));
    assert_eq!(row.tagcount(), None);
    assert_eq!(row.json()["playcount"], "5584");
    assert_eq!(row.into_artist().name(), "Portugal. The Man");

    let albums = decode::user_get_top_albums(&raw(&methods::USER_GET_TOP_ALBUMS, ALBUMS)).unwrap();
    let row = albums[0].clone();
    let album: &Album = row.as_ref();
    assert_eq!(album.name(), "The Social Network");
    assert_eq!((album.listeners(), album.playcount()), (None, None));
    assert_eq!(row.json()["playcount"], "1675");
    assert_eq!(row.into_album().name(), "The Social Network");

    let tracks = decode::user_get_top_tracks(&raw(&methods::USER_GET_TOP_TRACKS, TRACKS)).unwrap();
    let row = tracks[0].clone();
    let track: &Track = row.as_ref();
    assert_eq!((track.name(), track.playcount()), ("Swim", None));
    assert_eq!(row.into_track().artist().name(), "Fickle Friends");

    let tags = decode::user_get_top_tags(&raw(&methods::USER_GET_TOP_TAGS, TAGS)).unwrap();
    let row = tags[0].clone();
    let tag: &Tag = row.as_ref();
    assert_eq!((tag.taggings(), tag.reach()), (None, None));
    assert_eq!(row.json()["name"], tag.name());
    assert_eq!(row.clone().into_tag().name(), row.tag().name());
}
