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
use crate::protocol::{self, HttpResponse, MethodSpec, Request};

// Fixtures. Provenance for each:
//
// - `kelchm-user-info.json` was recorded by the repository owner from their
//   own account on 2026-11-10 and is kept byte for byte. recorded.
// - `friends-page.json` follows a `user.getFriends` response recorded the
//   same day: member names and order, nesting, value types and the escaped
//   slashes. Every value that says who a friend is is invented. derived.
// - Everything else is built here from those shapes. synthetic.

const INFO: &[u8] = include_bytes!("../../../../fixtures/recorded/kelchm-user-info.json");
const FRIENDS: &[u8] = include_bytes!("../../../../fixtures/derived/friends-page.json");

/// A string that appears in no error and no `Debug` output.
const SECRET: &str = "SYNTHETIC-RESPONSE-TEXT";

fn raw(spec: &'static MethodSpec, body: impl Into<Vec<u8>>) -> Raw {
    let request = Request::new(spec).param("user", "rj");
    protocol::decode(&request, HttpResponse::new(200, body.into())).unwrap()
}

fn info(user: &Value) -> Result<User, Error> {
    decode::user_get_info(&raw(
        &methods::USER_GET_INFO,
        json!({"user": user}).to_string(),
    ))
}

fn friends(body: &Value) -> Result<Page<User>, Error> {
    decode::user_get_friends(&raw(&methods::USER_GET_FRIENDS, body.to_string()))
}

fn profile(name: &str) -> Value {
    json!({"name": name, "url": "https://example.invalid/user/a", "country": "None",
           "playlists": "0", "playcount": "0", "image": [],
           "registered": {"unixtime": "1500000000", "#text": "2017-07-14 02:40"},
           "realname": "", "subscriber": "0", "bootstrap": "0", "type": "user"})
}

fn page(user: Value, total: u64) -> Value {
    json!({"friends": {"@attr": {
        "user": "rj", "totalPages": total.min(1).to_string(), "page": "1",
        "total": total.to_string(), "perPage": "50"}, "user": user}})
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
fn the_recorded_profile_decodes() {
    let user = decode::user_get_info(&raw(&methods::USER_GET_INFO, INFO)).unwrap();
    assert_eq!(user.name(), "kelchm");
    assert_eq!(user.realname(), Some("Matt Kelch"));
    assert_eq!(user.url(), Some("https://www.last.fm/user/kelchm"));
    assert_eq!(user.country(), Some("United States"));
    assert_eq!(user.playcount(), Some(242_328));
    assert_eq!(
        (user.artist_count(), user.album_count(), user.track_count()),
        (Some(10_105), Some(21_774), Some(47_059))
    );
    assert_eq!(user.registered_at(), 1_179_864_814);
    assert_eq!(user.subscriber(), Some(true));
    // What the model does not name is still in the profile.
    assert_eq!(user.json()["type"], "subscriber");
    assert_eq!(user.json()["registered"]["#text"], 1_179_864_814_u64);
    assert_eq!(user.json()["image"][3]["size"], "extralarge");

    let sizes: Vec<_> = user.images().iter().map(|image| image.size()).collect();
    assert_eq!(sizes, ["small", "medium", "large", "extralarge"].map(Some));
    assert_eq!(
        user.images()[3].url(),
        "https://lastfm-img.freetls.fastly.net/i/u/300x300/b1216fe4b5dfaec4f86ecf2075f54df4.png"
    );
}

#[test]
fn a_profile_without_an_image_has_none_and_a_malformed_one_names_the_field() {
    // Every address in the derived page is empty.
    let page = decode::user_get_friends(&raw(&methods::USER_GET_FRIENDS, FRIENDS)).unwrap();
    assert!(page.items().iter().all(|user| user.images().is_empty()));
    assert_eq!(page.items()[0].json()["image"][0]["size"], "small");

    let mut user = profile("rj");
    user["image"] = json!([{"size": "small", "#text": 5}]);
    names(info(&user), "user.image[0].#text");
}

#[test]
fn the_derived_friends_page_decodes() {
    let page = decode::user_get_friends(&raw(&methods::USER_GET_FRIENDS, FRIENDS)).unwrap();
    assert_eq!(page.subject(), Some("rj_Synthetic"));
    assert_eq!(
        (
            page.number(),
            page.per_page(),
            page.total_pages(),
            page.total()
        ),
        (1, 3, 8, 24)
    );

    let rows: Vec<_> = page
        .items()
        .iter()
        .map(|u| (u.name(), u.realname(), u.country(), u.registered_at()))
        .collect();
    assert_eq!(
        rows,
        [
            ("first_Synthetic", None, None, 1_500_000_000),
            (
                "second_Synthetic",
                Some("Second Listener"),
                None,
                1_400_000_000
            ),
            (
                "third_Synthetic",
                Some("Third Listener"),
                Some("Iceland"),
                1_100_000_000
            )
        ]
    );
    // A friend's row has no counts but the playcount, which the service
    // sent as 0, and `registered.#text` is a date here, not a number.
    let first = &page.items()[0];
    // The service sends zeros for a friend's counts, and they are not read.
    assert_eq!(
        (first.playcount(), first.artist_count(), first.subscriber()),
        (None, None, None)
    );
    assert_eq!(first.json()["playcount"], "0");
    assert_eq!(first.json()["country"], "None");
    assert_eq!(first.json()["registered"]["#text"], "2017-07-14 02:40");
}

#[test]
fn a_page_of_friends_may_be_one_object_or_empty() {
    let page_of_one = friends(&page(profile("only"), 1)).unwrap();
    assert_eq!(page_of_one.items()[0].name(), "only");

    for user in [Some(json!([])), Some(json!("")), None] {
        let mut body = page(json!([]), 0);
        match user {
            Some(user) => body["friends"]["user"] = user,
            None => {
                body["friends"].as_object_mut().unwrap().remove("user");
            }
        }
        assert!(friends(&body).unwrap().items().is_empty());
    }
    names(friends(&page(json!([]), 3)), "friends.user");
}

#[test]
fn a_malformed_profile_names_the_field() {
    let with = |change: &dyn Fn(&mut Value)| {
        let mut user = profile("a");
        change(&mut user);
        user
    };
    names(
        decode::user_get_info(&raw(
            &methods::USER_GET_INFO,
            json!({"other": SECRET}).to_string(),
        )),
        "`user`",
    );
    names(info(&with(&|u| u["name"] = json!(7))), "user.name");
    names(
        info(&with(&|u| u["playcount"] = json!(SECRET))),
        "user.playcount",
    );
    names(
        info(&with(&|u| {
            u.as_object_mut().unwrap().remove("registered");
        })),
        "user.registered.unixtime",
    );
    names(
        info(&with(&|u| u["registered"]["unixtime"] = json!(SECRET))),
        "user.registered.unixtime",
    );
    names(
        info(&with(&|u| u["subscriber"] = json!("yes"))),
        "user.subscriber",
    );
    names(
        info(&with(&|u| u["artist_count"] = json!(SECRET))),
        "user.artist_count",
    );
    names(info(&with(&|u| u["country"] = json!(7))), "user.country");
    names(info(&with(&|u| u["realname"] = json!(7))), "user.realname");

    names(
        friends(&page(json!([profile("a"), SECRET]), 2)),
        "friends.user[1]",
    );
    names(
        friends(&page(json!([profile("a"), {"name": "b"}]), 2)),
        "friends.user[1].registered.unixtime",
    );
    names(friends(&json!({"friends": {"user": []}})), "friends.@attr");
}

#[test]
fn what_a_profile_may_leave_out_reads_as_none() {
    let user = info(&json!({"name": "a", "playcount": 3,
                            "registered": {"unixtime": 1_500_000_000}}))
    .unwrap();
    assert_eq!(user.playcount(), Some(3));
    assert_eq!(
        (
            user.realname(),
            user.url(),
            user.country(),
            user.subscriber()
        ),
        (None, None, None, None)
    );
    assert_eq!(
        (user.artist_count(), user.album_count(), user.track_count()),
        (None, None, None)
    );
}

#[test]
fn a_response_to_another_method_is_refused() {
    let as_friends = raw(&methods::USER_GET_FRIENDS, INFO);
    let error = decode::user_get_info(&as_friends).unwrap_err();
    assert_eq!(error.kind(), ErrorKind::Decode);
    assert!(error.to_string().contains("different method"));

    let as_info = raw(&methods::USER_GET_INFO, FRIENDS);
    let error = decode::user_get_friends(&as_info).unwrap_err();
    assert!(error.to_string().contains("different method"));
}

#[test]
fn debug_leaves_out_the_text_of_the_response() {
    let mut named = profile(SECRET);
    named["realname"] = json!(SECRET);
    named["country"] = json!(SECRET);
    let user = info(&named).unwrap();
    let page = friends(&page(json!([named]), 1)).unwrap();
    let shown = format!("{user:?} {page:?} {:?}", page.items());
    assert!(!shown.contains(SECRET), "{shown}");
    assert!(shown.contains("registered_at: 1500000000"), "{shown}");
}

#[test]
fn a_user_is_a_user_wherever_it_comes_from() {
    fn name(user: &impl AsRef<User>) -> &str {
        user.as_ref().name()
    }
    let user = decode::user_get_info(&raw(&methods::USER_GET_INFO, INFO)).unwrap();
    assert_eq!(name(&user), "kelchm");
    let page = decode::user_get_friends(&raw(&methods::USER_GET_FRIENDS, FRIENDS)).unwrap();
    assert_eq!(name(&page[0]), page[0].name());
}
