//! Acceptance against the live Last.fm service. Nothing here writes.
//!
//! Nothing here runs unless asked for: the tests are ignored, and each fails
//! at once without `SCROBL_LIVE_API_KEY` and `SCROBL_LIVE_USER`. They are
//! never run in CI.
//!
//! ```sh
//! SCROBL_LIVE_API_KEY=... SCROBL_LIVE_USER=... \
//!     cargo test --test live -- --ignored --nocapture
//! ```
//!
//! Ten tests send about 180 key-only requests between them, a second
//! apart: no signature, no session, no write. With
//! `SCROBL_LIVE_CAPTURE_DIR` set, the exact body of every response is
//! written there, for the owner to turn into fixtures by hand. The bodies
//! are that user's listening history; the tests commit nothing.
//!
//! One more, [`the_desktop_sign_in_against_the_live_service`], also needs
//! `SCROBL_LIVE_API_SECRET` and the owner at a browser. It prints an address
//! for the owner to approve, waits up to five minutes, and creates a session
//! on that account, which it uses for one read. The session key is never
//! printed or written; the owner can revoke it on the website.

#![cfg(feature = "client")]
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing,
    clippy::print_stdout
)]

use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

use scrobl::client::Period;
use scrobl::history::Window;
use scrobl::protocol::Raw;
use scrobl::{ApiErrorCode, ApiKey, Client, ErrorKind};

const USER_AGENT: &str = "scrobl-live-acceptance/0 (+https://github.com/kelchm/scrobl)";

fn env(name: &str) -> String {
    std::env::var(name).unwrap_or_else(|_| panic!("set {name} to run the live acceptance test"))
}

fn client(key: &str) -> Client {
    Client::builder(ApiKey::new(key))
        .user_agent(USER_AGENT)
        .build()
        .unwrap()
}

/// Writes the exact response body to the capture directory, if one is set.
fn capture(name: &str, raw: &Raw) {
    let Ok(dir) = std::env::var("SCROBL_LIVE_CAPTURE_DIR") else {
        return;
    };
    let dir = PathBuf::from(dir);
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join(format!("{name}.json")), raw.body()).unwrap();
}

/// Reads a whole window and returns its total, having checked every page.
async fn scan(client: &Client, user: &str, window: Window, limit: u32, name: &str) -> u64 {
    let mut scan = client
        .user(user)
        .recent_tracks()
        .window(window)
        .limit(limit)
        .extended(true)
        .scan()
        .unwrap();
    let mut rows = 0;
    while let Some(page) = scan.next_page().await.unwrap() {
        capture(&format!("{name}-page-{}", page.number()), page.raw());
        rows += page.scrobbles().len() as u64;
    }
    let summary = scan.finish().unwrap();
    assert_eq!(summary.total(), rows, "{name}: rows seen");
    println!("{name}: {rows} scrobbles in {} pages", summary.pages());
    rows
}

async fn total(client: &Client, user: &str, window: Window, name: &str) -> u64 {
    let page = client
        .user(user)
        .recent_tracks()
        .window(window)
        .limit(1)
        .send()
        .await
        .unwrap();
    capture(name, page.raw());
    page.total()
}

#[tokio::test]
#[ignore = "calls Last.fm; needs SCROBL_LIVE_API_KEY and SCROBL_LIVE_USER"]
async fn recent_tracks_against_the_live_service() {
    let key = env("SCROBL_LIVE_API_KEY");
    let user = env("SCROBL_LIVE_USER");
    let client = client(&key);

    // The week that ends at the user's latest scrobble, or an hour ago if
    // that is earlier: a window in the past cannot gain a scrobble while it
    // is read.
    let latest = client
        .user(&user)
        .recent_tracks()
        .limit(1)
        .send()
        .await
        .unwrap();
    capture("latest", latest.raw());
    println!("history: {} scrobbles", latest.total());
    let latest = latest
        .scrobbles()
        .first()
        .expect("the user has no scrobbles")
        .timestamp();
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_secs();
    let to = (latest + 1).min(now - 3600);
    let week = Window::new(to - 7 * 86_400, to).unwrap();

    // One page decodes, and the service echoes what was asked.
    let first = client
        .user(&user)
        .recent_tracks()
        .window(week)
        .limit(5)
        .extended(true)
        .send()
        .await
        .unwrap();
    capture("week-first-page-extended", first.raw());
    assert!(
        first
            .subject()
            .is_some_and(|it| it.eq_ignore_ascii_case(&user))
    );
    assert_eq!(first.per_page(), 5);
    let newest = first
        .scrobbles()
        .first()
        .expect("the window before the latest scrobble is empty")
        .timestamp();
    assert!(week.contains(newest));

    // The whole week at the largest page size and at a small one, which
    // takes several pages: the scan's
    // rules hold on real pages, and the two readings agree.
    let large = scan(&client, &user, week, 200, "week-200").await;
    let small = scan(&client, &user, week, 7, "week-7").await;
    assert_eq!(large, small, "the same window at two page sizes");

    // The bounds are `[from, to)`: `from` is inclusive and `to` exclusive.
    let at = total(
        &client,
        &user,
        Window::new(newest, newest + 1).unwrap(),
        "bound-at",
    )
    .await;
    assert!(
        at >= 1,
        "`from` is inclusive: [t, t+1) holds the scrobble at t"
    );
    let through = total(
        &client,
        &user,
        Window::new(newest, to).unwrap(),
        "bound-from",
    )
    .await;
    let after = total(
        &client,
        &user,
        Window::new(newest + 1, to + 1).unwrap(),
        "bound-after",
    )
    .await;
    assert_eq!(through, at + after, "[t, to) is [t, t+1) plus [t+1, to)");
    let before = total(
        &client,
        &user,
        Window::new(to - 7 * 86_400, newest).unwrap(),
        "bound-before",
    )
    .await;
    assert_eq!(
        before + through,
        large,
        "`to` is exclusive: [from, t) leaves out t"
    );

    // An error arrives as an envelope, whatever the HTTP status.
    let error = client
        .user("scrobl-no-such-user-0a1b2c3d4e5f")
        .recent_tracks()
        .limit(1)
        .send()
        .await
        .unwrap_err();
    println!("unknown user: {error} (http {:?})", error.http_status());
    assert_eq!(error.kind(), ErrorKind::Api);
    assert_eq!(error.api_code(), Some(ApiErrorCode::INVALID_PARAMETERS));

    let error = self::client("00000000000000000000000000000000")
        .user(&user)
        .recent_tracks()
        .limit(1)
        .send()
        .await
        .unwrap_err();
    println!("bad key: {error} (http {:?})", error.http_status());
    assert_eq!(error.kind(), ErrorKind::Api);
    assert_eq!(error.api_code(), Some(ApiErrorCode::INVALID_API_KEY));
}

/// Ranks count up by one from `first`, and no row was played more than the
/// row before it.
/// The place the service gives a row of a user's list, which the typed API
/// leaves to a page's own count.
fn service_rank(row: &serde_json::Value) -> u64 {
    row["@attr"]["rank"]
        .as_str()
        .and_then(|rank| rank.parse().ok())
        .expect("a rank on the row")
}

fn assert_ranked(name: &str, first: u64, rows: &[(u64, u64)]) {
    for (index, (rank, _)) in rows.iter().enumerate() {
        assert_eq!(*rank, first + index as u64, "{name}: rank of row {index}");
    }
    assert!(
        rows.windows(2).all(|pair| pair[0].1 >= pair[1].1),
        "{name}: most played first"
    );
}

#[tokio::test]
#[ignore = "calls Last.fm; needs SCROBL_LIVE_API_KEY and SCROBL_LIVE_USER"]
async fn the_lists_of_a_user_against_the_live_service() {
    let key = env("SCROBL_LIVE_API_KEY");
    let name = env("SCROBL_LIVE_USER");
    let user = client(&key).user(&name);

    // Top artists: the service echoes what was asked, ranks run on from one
    // page to the next, and a shorter period holds no more than the whole.
    let artists = user.top_artists().limit(5).send().await.unwrap();
    capture("top-artists", artists.raw());
    assert!(
        artists
            .subject()
            .is_some_and(|it| it.eq_ignore_ascii_case(&name))
    );
    assert_eq!((artists.number(), artists.per_page()), (1, 5));
    let ranked = |page: &scrobl::model::Page<scrobl::model::UserArtist>| -> Vec<(u64, u64)> {
        page.items()
            .iter()
            .map(|a| (service_rank(a.json()), a.playcount()))
            .collect()
    };
    assert_ranked("top artists", 1, &ranked(&artists));
    let second = user.top_artists().limit(5).page(2).send().await.unwrap();
    capture("top-artists-page-2", second.raw());
    assert_ranked("top artists, page 2", 6, &ranked(&second));
    let week = user
        .top_artists()
        .period(Period::SevenDays)
        .limit(5)
        .send()
        .await
        .unwrap();
    capture("top-artists-7day", week.raw());
    assert!(week.total() <= artists.total());
    println!(
        "top artists: {} overall, {} in seven days",
        artists.total(),
        week.total()
    );

    // A page far past the last one is empty, not an error.
    let beyond = user
        .top_artists()
        .limit(1000)
        .page(9999)
        .send()
        .await
        .unwrap();
    capture("top-artists-beyond", beyond.raw());
    assert!(beyond.items().is_empty());

    // A page of a thousand of each list decodes: every row has what the
    // model requires.
    let artists = user.top_artists().limit(1000).send().await.unwrap();
    capture("top-artists-1000", artists.raw());
    assert_ranked("a thousand artists", 1, &ranked(&artists));
    println!(
        "top artists: a page of {} at `limit=1000`, `perPage` {}",
        artists.items().len(),
        artists.per_page()
    );

    let albums = user.top_albums().limit(1000).send().await.unwrap();
    capture("top-albums-1000", albums.raw());
    let rows: Vec<_> = albums
        .items()
        .iter()
        .map(|a| (service_rank(a.json()), a.playcount()))
        .collect();
    assert_ranked("top albums", 1, &rows);
    println!(
        "top albums: {} in all, {} on the page, {} untitled, {} without an mbid",
        albums.total(),
        rows.len(),
        albums
            .items()
            .iter()
            .filter(|a| a.album().name().is_empty())
            .count(),
        albums
            .items()
            .iter()
            .filter(|a| a.album().mbid().is_none())
            .count()
    );

    let tracks = user.top_tracks().limit(1000).send().await.unwrap();
    capture("top-tracks-1000", tracks.raw());
    let rows: Vec<_> = tracks
        .items()
        .iter()
        .map(|t| (service_rank(t.json()), t.playcount()))
        .collect();
    assert_ranked("top tracks", 1, &rows);
    println!(
        "top tracks: {} in all, {} on the page, {} of unknown length",
        tracks.total(),
        rows.len(),
        tracks
            .items()
            .iter()
            .filter(|t| t.track().duration().is_none())
            .count()
    );

    let tags = user.top_tags().send().await.unwrap();
    capture("top-tags", tags.raw());
    assert!(
        tags.subject()
            .is_some_and(|it| it.eq_ignore_ascii_case(&name))
    );
    let limited = user.top_tags().limit(1).send().await.unwrap();
    capture("top-tags-1", limited.raw());
    assert!(limited.items().len() <= 1);
    println!("top tags: {}", tags.items().len());

    // The library is the same artists as the top list of all time, and one
    // row to a page comes as a single object.
    let library = user.library_artists().limit(1000).send().await.unwrap();
    capture("library-artists-1000", library.raw());
    assert_eq!(library.total(), artists.total());
    assert!(
        library
            .items()
            .windows(2)
            .all(|pair| pair[0].playcount() >= pair[1].playcount()),
        "library: most played first"
    );
    let one = user.library_artists().limit(1).send().await.unwrap();
    capture("library-artists-1", one.raw());
    assert_eq!(one.items().len(), 1);
    assert_eq!(
        one.items()[0].artist().name(),
        library.items()[0].artist().name()
    );
    println!(
        "library: {} artists, {} on the page",
        library.total(),
        library.items().len()
    );
}

#[tokio::test]
#[ignore = "calls Last.fm; needs SCROBL_LIVE_API_KEY and SCROBL_LIVE_USER"]
async fn what_a_user_played_in_a_week_against_the_live_service() {
    let key = env("SCROBL_LIVE_API_KEY");
    let name = env("SCROBL_LIVE_USER");
    let user = client(&key).user(&name);

    // The weeks follow one another without a gap.
    let weeks = user.weeks().send().await.unwrap();
    capture("weekly-chart-list", weeks.raw());
    assert!(
        weeks
            .subject()
            .is_some_and(|it| it.eq_ignore_ascii_case(&name))
    );
    assert!(
        weeks.windows(2).all(|pair| pair[0].to() == pair[1].from()),
        "each week ends where the next begins"
    );
    let last = *weeks.last().expect("the list has no weeks");
    println!("chart list: {} weeks, the last {last:?}", weeks.len());

    // Each chart of the last listed week is the user's and is ranked.
    let artists = user.top_artists_in(last).send().await.unwrap();
    capture("weekly-artist-chart", artists.raw());
    assert!(
        artists
            .subject()
            .is_some_and(|it| it.eq_ignore_ascii_case(&name))
    );
    let rows: Vec<_> = artists
        .iter()
        .map(|a| (service_rank(a.json()), a.playcount()))
        .collect();
    assert_ranked("weekly artists", 1, &rows);

    let albums = user.top_albums_in(last).send().await.unwrap();
    capture("weekly-album-chart", albums.raw());
    let album_rows: Vec<_> = albums
        .iter()
        .map(|a| (service_rank(a.json()), a.playcount()))
        .collect();
    assert_ranked("weekly albums", 1, &album_rows);

    let tracks = user.top_tracks_in(last).send().await.unwrap();
    capture("weekly-track-chart", tracks.raw());
    let track_rows: Vec<_> = tracks
        .iter()
        .map(|t| (service_rank(t.json()), t.playcount()))
        .collect();
    assert_ranked("weekly tracks", 1, &track_rows);
    // The plays of a week are the same by artist and by track, unless the
    // track chart was cut off at its thousand rows.
    let plays = |rows: &[(u64, u64)]| rows.iter().map(|row| row.1).sum::<u64>();
    println!(
        "last week: {} artists ({} plays), {} albums ({} plays), {} tracks ({} plays)",
        rows.len(),
        plays(&rows),
        album_rows.len(),
        plays(&album_rows),
        track_rows.len(),
        plays(&track_rows)
    );
    if track_rows.len() < 1000 {
        assert_eq!(plays(&rows), plays(&track_rows), "artists and tracks");
    }

    // A limit, which the method's page does not document, is applied: the
    // rows are the first of the whole chart.
    let few = user.top_artists_in(last).limit(3).send().await.unwrap();
    capture("weekly-artist-chart-limit", few.raw());
    assert_eq!(few.len(), rows.len().min(3));
    for (short, whole) in few.iter().zip(artists.iter()) {
        assert_eq!(short.artist().name(), whole.artist().name());
    }

    // The first listed week.
    let first = *weeks.first().expect("the list has no weeks");
    let oldest = user.top_tracks_in(first).send().await.unwrap();
    capture("weekly-track-chart-first", oldest.raw());
    println!("first listed week: {} tracks", oldest.len());
}

#[tokio::test]
#[ignore = "calls Last.fm; needs SCROBL_LIVE_API_KEY and SCROBL_LIVE_USER"]
async fn the_profile_friends_tags_and_loved_tracks_of_a_user_against_the_live_service() {
    let key = env("SCROBL_LIVE_API_KEY");
    let name = env("SCROBL_LIVE_USER");
    let user = client(&key).user(&name);

    // The profile, and how its counts sit beside the totals of the lists.
    // They are printed, not compared: a scrobble between two requests moves
    // one and not the other.
    let info = user.info().send().await.unwrap();
    capture("user-info", info.raw());
    assert!(info.name().eq_ignore_ascii_case(&name));
    assert!(info.registered_at() > 0);
    let artists = user.top_artists().limit(1).send().await.unwrap();
    capture("top-artists-1", artists.raw());
    let albums = user.top_albums().limit(1).send().await.unwrap();
    capture("top-albums-1", albums.raw());
    let tracks = user.top_tracks().limit(1).send().await.unwrap();
    capture("top-tracks-1", tracks.raw());
    println!(
        "profile: {} scrobbles; artists {:?} (top list {}), albums {:?} ({}), tracks {:?} ({})",
        info.playcount().unwrap_or_default(),
        info.artist_count(),
        artists.total(),
        info.album_count(),
        albums.total(),
        info.track_count(),
        tracks.total()
    );

    // Friends: a page decodes, and a page past the last is an error.
    let friends = user.friends().limit(2).send().await.unwrap();
    capture("friends", friends.raw());
    assert!(
        friends
            .subject()
            .is_some_and(|it| it.eq_ignore_ascii_case(&name))
    );
    assert_eq!(friends.per_page(), 2);
    assert!(friends.items().len() <= 2);
    println!(
        "friends: {} in all; on the page, playcounts {:?}, counts of artists {:?}",
        friends.total(),
        friends
            .items()
            .iter()
            .map(|u| u.playcount())
            .collect::<Vec<_>>(),
        friends
            .items()
            .iter()
            .map(|u| u.artist_count())
            .collect::<Vec<_>>()
    );
    let past = friends.total_pages() + 1;
    let error = user.friends().limit(2).page(past).send().await.unwrap_err();
    println!(
        "friends, a page past the last: {error} (http {:?})",
        error.http_status()
    );
    assert_eq!(error.kind(), ErrorKind::Api);
    assert_eq!(error.api_code(), Some(ApiErrorCode::INVALID_PARAMETERS));

    // Loved tracks, newest first.
    let loved = user.loved_tracks().limit(2).send().await.unwrap();
    capture("loved-tracks", loved.raw());
    assert!(
        loved
            .subject()
            .is_some_and(|it| it.eq_ignore_ascii_case(&name))
    );
    assert!(
        loved
            .items()
            .windows(2)
            .all(|pair| pair[0].loved_at() >= pair[1].loved_at()),
        "loved tracks: newest first"
    );
    println!("loved tracks: {}", loved.total());

    // Every loved track, page after page: as many rows as the service
    // counts, and newest first across the pages.
    let mut rows = user.loved_tracks().limit(100).all();
    let mut times = Vec::new();
    while let Some(row) = rows.next().await.unwrap() {
        times.push(row.loved_at());
    }
    assert_eq!(Some(times.len() as u64), rows.total());
    assert!(times.windows(2).all(|pair| pair[0] >= pair[1]));
    println!("all loved tracks: {} rows", times.len());

    // What the user gave each of their three most used tags.
    let tags = user.top_tags().limit(3).send().await.unwrap();
    capture("top-tags-3", tags.raw());
    for (index, tag) in tags.items().iter().enumerate() {
        let artists = user
            .tagged_artists(tag.tag().name())
            .limit(2)
            .send()
            .await
            .unwrap();
        capture(&format!("tagged-artists-{index}"), artists.raw());
        let albums = user
            .tagged_albums(tag.tag().name())
            .limit(2)
            .send()
            .await
            .unwrap();
        capture(&format!("tagged-albums-{index}"), albums.raw());
        let tracks = user
            .tagged_tracks(tag.tag().name())
            .limit(2)
            .send()
            .await
            .unwrap();
        capture(&format!("tagged-tracks-{index}"), tracks.raw());
        assert!(
            artists
                .subject()
                .is_some_and(|it| it.eq_ignore_ascii_case(&name))
        );
        assert_eq!(artists.per_page(), 2);
        println!(
            "a tag used {} times: {} artists, {} albums, {} tracks",
            tag.count(),
            artists.total(),
            albums.total(),
            tracks.total()
        );
    }
}

#[tokio::test]
#[ignore = "calls Last.fm; needs SCROBL_LIVE_API_KEY"]
async fn the_charts_of_the_service_and_of_a_country_against_the_live_service() {
    let key = env("SCROBL_LIVE_API_KEY");
    let client = client(&key);
    let chart = client.charts();

    // The chart of artists: the service echoes what was asked, a second
    // page holds other artists, a page of 1000 decodes, and a page beyond
    // the last is empty and not an error.
    let artists = chart.top_artists().limit(5).send().await.unwrap();
    capture("chart-artists", artists.raw());
    assert_eq!((artists.number(), artists.per_page()), (1, 5));
    assert_eq!(artists.items().len(), 5);
    let second = chart.top_artists().limit(5).page(2).send().await.unwrap();
    capture("chart-artists-page-2", second.raw());
    assert_eq!(second.number(), 2);
    assert_ne!(second.items()[0].name(), artists.items()[0].name());
    let full = chart.top_artists().limit(1000).send().await.unwrap();
    capture("chart-artists-1000", full.raw());
    assert_eq!(full.items().len(), 1000);
    let beyond = chart
        .top_artists()
        .limit(50)
        .page(999_999)
        .send()
        .await
        .unwrap();
    capture("chart-artists-beyond", beyond.raw());
    assert!(beyond.items().is_empty());
    println!(
        "chart artists: {} listed, first {} plays",
        artists.total(),
        artists.items()[0].playcount().unwrap_or_default()
    );

    let tracks = chart.top_tracks().limit(5).send().await.unwrap();
    capture("chart-tracks", tracks.raw());
    assert_eq!(tracks.items().len(), 5);
    let full = chart.top_tracks().limit(1000).send().await.unwrap();
    capture("chart-tracks-1000", full.raw());
    assert_eq!(full.items().len(), 1000);
    let unknown = full
        .items()
        .iter()
        .filter(|t| t.duration().is_none())
        .count();
    println!("chart tracks: {unknown} of 1000 without a length");

    // The chart of tags. Whether it pages is printed and not asserted: on
    // 2026-11-11 page 2 held the rows of page 1.
    let tags = chart.top_tags().limit(5).send().await.unwrap();
    capture("chart-tags", tags.raw());
    assert_eq!(tags.items().len(), 5);
    println!("chart tags: `{}` first", tags.items()[0].name());
    let full = chart.top_tags().limit(1000).send().await.unwrap();
    capture("chart-tags-1000", full.raw());
    assert_eq!(full.items().len(), 1000);

    // A country: the service spells the name back its own way, and refuses
    // a code and a name it does not know.
    let germany = client.country("germany");
    let artists = germany.top_artists().limit(5).send().await.unwrap();
    capture("geo-artists", artists.raw());
    assert_eq!(artists.subject(), Some("Germany"));
    assert_eq!(artists.items().len(), 5);
    let full = germany.top_artists().limit(1000).send().await.unwrap();
    capture("geo-artists-1000", full.raw());
    assert_eq!(full.items().len(), 1000);
    let tracks = germany.top_tracks().limit(5).send().await.unwrap();
    capture("geo-tracks", tracks.raw());
    assert_eq!(tracks.subject(), Some("Germany"));
    assert_eq!(tracks.items().len(), 5);
    let full = germany.top_tracks().limit(1000).send().await.unwrap();
    capture("geo-tracks-1000", full.raw());
    assert_eq!(full.items().len(), 1000);
    println!(
        "Germany: {} artists, {} tracks listed",
        artists.total(),
        tracks.total()
    );

    for country in ["DE", "Atlantis"] {
        let error = client
            .country(country)
            .top_artists()
            .limit(5)
            .send()
            .await
            .unwrap_err();
        println!(
            "country `{country}`: {error} (http {:?})",
            error.http_status()
        );
        assert_eq!(error.api_code(), Some(ApiErrorCode::INVALID_PARAMETERS));
    }
}

#[tokio::test]
#[ignore = "calls Last.fm; needs SCROBL_LIVE_API_KEY"]
async fn tags_against_the_live_service() {
    let key = env("SCROBL_LIVE_API_KEY");
    let client = client(&key);
    let tag = client.tag("indie");

    // What the service says about a tag, in two languages, and about one
    // nobody has used, which is not an error.
    let info = tag.info().send().await.unwrap();
    capture("tag-info", info.raw());
    assert_eq!(info.tag().name(), "indie");
    assert!(info.taggings() >= info.reach());
    assert!(info.summary().is_some());
    let german = tag.info().lang("de").send().await.unwrap();
    capture("tag-info-de", german.raw());
    assert_eq!(german.taggings(), info.taggings());
    println!(
        "indie: used {} times by {}; the German description {} the English one",
        info.taggings(),
        info.reach(),
        if german.content() == info.content() {
            "is"
        } else {
            "is not"
        }
    );
    let unused = client.tag("zzqqxx-no-such-tag-scrobl");
    let nothing = unused.info().send().await.unwrap();
    capture("tag-info-unused", nothing.raw());
    assert_eq!((nothing.taggings(), nothing.reach()), (0, 0));

    // Similar tags. What comes back is printed and not asserted: on
    // 2026-11-11 the service sent no rows for any tag.
    let similar = tag.similar().send().await.unwrap();
    capture("tag-similar", similar.raw());
    println!(
        "similar to indie: {} tags, for `{}`",
        similar.items().len(),
        similar.subject().unwrap_or_default()
    );

    // The three lists: the service spells the tag back, a page of 1000
    // decodes, a page beyond the last is empty, and so is every page of a
    // tag nobody has used.
    let albums = tag.top_albums().limit(5).send().await.unwrap();
    capture("tag-albums", albums.raw());
    assert_eq!(albums.subject(), Some("indie"));
    assert_eq!(
        (albums.number(), albums.per_page(), albums.items().len()),
        (1, 5, 5)
    );
    let full = tag.top_albums().limit(1000).send().await.unwrap();
    capture("tag-albums-1000", full.raw());
    assert_eq!(full.items().len(), 1000);
    let beyond = tag
        .top_albums()
        .limit(50)
        .page(999_999)
        .send()
        .await
        .unwrap();
    capture("tag-albums-beyond", beyond.raw());
    assert!(beyond.items().is_empty());
    let none = unused.top_albums().limit(5).send().await.unwrap();
    capture("tag-albums-unused", none.raw());
    assert!(none.items().is_empty());
    assert_eq!(none.total(), 0);

    let artists = tag.top_artists().limit(5).send().await.unwrap();
    capture("tag-artists", artists.raw());
    assert_eq!(artists.items().len(), 5);
    let full = tag.top_artists().limit(1000).send().await.unwrap();
    capture("tag-artists-1000", full.raw());
    assert_eq!(full.items().len(), 1000);

    let tracks = tag.top_tracks().limit(5).send().await.unwrap();
    capture("tag-tracks", tracks.raw());
    assert_eq!(tracks.items().len(), 5);
    let full = tag.top_tracks().limit(1000).send().await.unwrap();
    capture("tag-tracks-1000", full.raw());
    assert_eq!(full.items().len(), 1000);
    println!(
        "indie: {} albums, {} artists, {} tracks",
        albums.total(),
        artists.total(),
        tracks.total()
    );

    // The weeks: each ends where the next begins.
    let weeks = tag.weeks().send().await.unwrap();
    capture("tag-weeks", weeks.raw());
    assert_eq!(weeks.subject(), Some("indie"));
    assert!(weeks.windows(2).all(|pair| pair[0].to() == pair[1].from()));
    println!("indie: {} weeks", weeks.len());

    // Every tag: 50 when nothing is asked, and the two parameters the
    // method's page does not list move through the list.
    let all = client.charts().tags().send().await.unwrap();
    capture("all-tags", all.raw());
    assert_eq!(all.items().len(), 50);
    let next = client
        .charts()
        .tags()
        .limit(3)
        .offset(2)
        .send()
        .await
        .unwrap();
    capture("all-tags-offset", next.raw());
    assert_eq!(next.items().len(), 3);
    assert_eq!(next.items()[0].name(), all.items()[2].name());
    assert!(
        all.items()
            .windows(2)
            .all(|pair| pair[0].taggings() >= pair[1].taggings())
    );
    println!("all tags: `{}` first", all.items()[0].name());
}

#[tokio::test]
#[ignore = "calls Last.fm; needs SCROBL_LIVE_API_KEY"]
async fn the_lists_of_an_artist_against_the_live_service() {
    let key = env("SCROBL_LIVE_API_KEY");
    let client = client(&key);
    let artist = client.artist("radiohead");

    // Similar artists: most alike first, the name spelt back the service's
    // way, and never more than 250 whatever is asked.
    let similar = artist.similar().limit(5).send().await.unwrap();
    capture("artist-similar", similar.raw());
    assert_eq!(similar.subject(), Some("Radiohead"));
    assert_eq!(similar.len(), 5);
    assert!(
        similar
            .windows(2)
            .all(|pair| pair[0].similarity() >= pair[1].similarity())
    );
    assert!(similar.iter().all(|row| row.similarity() <= 1.0));
    let most = artist.similar().limit(1000).send().await.unwrap();
    capture("artist-similar-1000", most.raw());
    assert_eq!(most.len(), 250);
    println!(
        "similar: {} at most, from {} down to {}",
        most.len(),
        most[0].similarity(),
        most[249].similarity()
    );

    // Albums: a page of 1000 decodes, a second page holds other albums, and
    // a page beyond the last is the first page again.
    let albums = artist.top_albums().limit(5).send().await.unwrap();
    capture("artist-albums", albums.raw());
    assert_eq!(albums.subject(), Some("Radiohead"));
    assert_eq!((albums.number(), albums.len()), (1, 5));
    assert!(albums.iter().all(|album| album.playcount().is_some()));
    let second = artist.top_albums().limit(5).page(2).send().await.unwrap();
    assert_eq!(second.number(), 2);
    assert_ne!(second[0].name(), albums[0].name());
    let full = artist.top_albums().limit(1000).send().await.unwrap();
    capture("artist-albums-1000", full.raw());
    assert_eq!(full.len(), 1000);
    let beyond = artist
        .top_albums()
        .limit(5)
        .page(999_999)
        .send()
        .await
        .unwrap();
    capture("artist-albums-beyond", beyond.raw());
    println!(
        "albums: {} in all; a page beyond the last came back as page {} with {} rows",
        albums.total(),
        beyond.number(),
        beyond.len()
    );

    // Tracks: each with both counts, and in order of listeners, not plays.
    let tracks = artist.top_tracks().limit(5).send().await.unwrap();
    capture("artist-tracks", tracks.raw());
    assert_eq!(tracks.subject(), Some("Radiohead"));
    assert!(tracks.iter().all(|track| track.playcount().is_some()));
    let full = artist.top_tracks().limit(1000).send().await.unwrap();
    capture("artist-tracks-1000", full.raw());
    assert_eq!(full.len(), 1000);
    assert!(
        full.windows(2)
            .all(|pair| pair[0].listeners() >= pair[1].listeners()),
        "tracks: most listeners first"
    );
    println!("tracks: {} in all", tracks.total());

    // Every row of a paged list, page after page.
    let mut rows = artist.top_tracks().limit(3).page(1).all();
    let mut seen = Vec::new();
    while seen.len() < 7 {
        seen.push(rows.next().await.unwrap().expect("a track"));
    }
    assert_eq!(seen[0].name(), tracks[0].name());
    assert_eq!(seen[4].name(), tracks[4].name());

    // Tags: the first is given most, at 100.
    let tags = artist.top_tags().send().await.unwrap();
    capture("artist-top-tags", tags.raw());
    assert_eq!(tags.subject(), Some("Radiohead"));
    assert_eq!(tags[0].count(), 100);
    assert!(
        tags.windows(2)
            .all(|pair| pair[0].count() >= pair[1].count())
    );
    println!("tags: {}, `{}` first", tags.len(), tags[0].tag().name());

    // By id, the same artist.
    let by_id = client.artist_mbid("a74b1b7f-71a5-4011-9441-d0b5e4122711");
    let same = by_id.top_tags().send().await.unwrap();
    assert_eq!(same.subject(), Some("Radiohead"));

    // A misspelt name: corrected when asked, and filed under the right one.
    let misspelt = client.artist("Guns and Roses");
    let corrected = misspelt
        .clone()
        .autocorrect()
        .top_tags()
        .send()
        .await
        .unwrap();
    capture("artist-top-tags-autocorrect", corrected.raw());
    assert_eq!(corrected.subject(), Some("Guns N' Roses"));
    let filed = misspelt.correction().send().await.unwrap();
    capture("artist-correction", filed.raw());
    assert_eq!(filed.name(), "Guns N' Roses");
    let right = artist.correction().send().await.unwrap();
    assert_eq!(right.name(), "Radiohead");

    // An artist the service does not know is an error, not an empty answer.
    let unknown = client.artist("zzqqxx-no-such-artist-scrobl");
    for error in [
        unknown.similar().send().await.map(drop).unwrap_err(),
        unknown.top_albums().send().await.map(drop).unwrap_err(),
        unknown.top_tracks().send().await.map(drop).unwrap_err(),
        unknown.top_tags().send().await.map(drop).unwrap_err(),
        unknown.correction().send().await.map(drop).unwrap_err(),
    ] {
        assert_eq!(error.api_code(), Some(ApiErrorCode::INVALID_PARAMETERS));
    }
    println!("unknown artist: error 6 on all five");
}

#[tokio::test]
#[ignore = "calls Last.fm; needs SCROBL_LIVE_API_KEY and SCROBL_LIVE_USER"]
async fn what_the_service_says_of_an_artist_and_an_album_against_the_live_service() {
    let key = env("SCROBL_LIVE_API_KEY");
    let name = env("SCROBL_LIVE_USER");
    let client = client(&key);

    // An artist: both counts, a few similar artists and tags, and a
    // description. The counts on the artist are the reply's own.
    let artist = client.artist("radiohead");
    let info = artist.info().send().await.unwrap();
    capture("artist-info", info.raw());
    assert_eq!(info.artist().name(), "Radiohead");
    assert_eq!(info.artist().listeners(), Some(info.listeners()));
    assert!(info.listeners() > 0 && info.playcount() > info.listeners());
    assert_eq!(info.user_playcount(), None);
    assert!(!info.similar().is_empty() && !info.tags().is_empty());
    assert!(info.summary().is_some() && info.published().is_some());
    println!(
        "artist: {} listeners, {} similar, {} tags, on tour {}",
        info.listeners(),
        info.similar().len(),
        info.tags().len(),
        info.on_tour()
    );

    // With a user named, that user's own plays; by id, the same artist; in
    // German, another description.
    let mine = artist.info().for_user(&name).send().await.unwrap();
    capture("artist-info-user", mine.raw());
    println!("artist: {:?} plays by the user", mine.user_playcount());
    assert!(mine.user_playcount().is_some());
    // An artist the user has never played, on 2026-11-12, has a count of
    // 0 and not none.
    let never = client
        .artist("Crazy Frog")
        .info()
        .for_user(&name)
        .send()
        .await
        .unwrap();
    capture("artist-info-user-never", never.raw());
    println!(
        "an artist the user may never have played: {:?}",
        never.user_playcount()
    );
    let by_id = client
        .artist_mbid("a74b1b7f-71a5-4011-9441-d0b5e4122711")
        .info()
        .lang("de")
        .send()
        .await
        .unwrap();
    assert_eq!(by_id.artist().name(), "Radiohead");
    assert_ne!(by_id.summary(), info.summary());
    let corrected = client
        .artist("Guns and Roses")
        .autocorrect()
        .info()
        .send()
        .await
        .unwrap();
    assert_eq!(corrected.artist().name(), "Guns N' Roses");

    // The tags of a user: none is an empty list, and no such user an error.
    let tags = artist.tags_of(&name).send().await.unwrap();
    capture("artist-tags", tags.raw());
    assert_eq!(tags.subject(), Some("Radiohead"));
    println!("artist: {} tags by the user", tags.len());
    let error = artist
        .tags_of("zzqqxx-no-such-user-scrobl")
        .send()
        .await
        .unwrap_err();
    assert_eq!(error.api_code(), Some(ApiErrorCode::INVALID_PARAMETERS));

    // An album: its artist is a name alone, its tracks are in order and
    // each has its artist.
    let album = client.album("radiohead", "ok computer");
    let info = album.info().for_user(&name).send().await.unwrap();
    capture("album-info", info.raw());
    assert_eq!(info.album().name(), "OK Computer");
    let by = info.album().artist().expect("the album's artist");
    assert_eq!((by.name(), by.url()), ("Radiohead", None));
    assert!(info.tracks().len() >= 12);
    assert!(
        info.tracks()
            .iter()
            .all(|track| track.artist().name() == "Radiohead")
    );
    assert!(info.user_playcount().is_some());
    println!(
        "album: {} listeners, {} tracks, {} tags, {:?} plays by the user",
        info.listeners(),
        info.tracks().len(),
        info.tags().len(),
        info.user_playcount()
    );
    let small = client
        .album("Nolan Taylor", "68")
        .info()
        .send()
        .await
        .unwrap();
    capture("album-info-small", small.raw());
    println!(
        "a small album: {} tracks, {} tags, a description: {}",
        small.tracks().len(),
        small.tags().len(),
        small.summary().is_some()
    );
    let by_id = client
        .album_mbid("233b7916-ae58-3278-9907-f3b591390651")
        .info()
        .send()
        .await
        .unwrap();
    capture("album-info-mbid", by_id.raw());
    println!("an album by id: `{}`", by_id.album().name());

    let top = album.top_tags().send().await.unwrap();
    capture("album-top-tags", top.raw());
    assert_eq!(top.subject(), Some("OK Computer"));
    assert_eq!(top[0].count(), 100);
    let tags = album.tags_of(&name).send().await.unwrap();
    capture("album-tags", tags.raw());
    assert_eq!(tags.subject(), Some("OK Computer"));

    // An album the service does not know is an error on all three.
    let unknown = client.album("radiohead", "zzqqxx-no-such-album-scrobl");
    for error in [
        unknown.info().send().await.map(drop).unwrap_err(),
        unknown.top_tags().send().await.map(drop).unwrap_err(),
        unknown.tags_of(&name).send().await.map(drop).unwrap_err(),
    ] {
        assert_eq!(error.api_code(), Some(ApiErrorCode::INVALID_PARAMETERS));
    }
    println!("unknown album: error 6 on all three");
}

#[tokio::test]
#[ignore = "calls Last.fm; needs SCROBL_LIVE_API_KEY"]
async fn the_searches_against_the_live_service() {
    let key = env("SCROBL_LIVE_API_KEY");
    let search = client(&key).search();

    // Artists: the query comes back, the pages follow one another, and a
    // page of 1000 decodes.
    let artists = search.artists("radiohead").limit(5).send().await.unwrap();
    capture("search-artists", artists.raw());
    assert_eq!(artists.subject(), Some("radiohead"));
    assert_eq!(
        (artists.number(), artists.per_page(), artists.len()),
        (1, 5, 5)
    );
    assert_eq!(artists[0].name(), "Radiohead");
    assert!(artists.iter().all(|artist| artist.listeners().is_some()));
    let second = search
        .artists("radiohead")
        .limit(5)
        .page(2)
        .send()
        .await
        .unwrap();
    assert_eq!(second.number(), 2);
    assert_ne!(second[0].name(), artists[0].name());
    let full = search.artists("radio").limit(1000).send().await.unwrap();
    capture("search-artists-1000", full.raw());
    assert_eq!(full.len(), 1000);
    let unset = search.artists("radiohead").send().await.unwrap();
    println!(
        "artists: {} match, {} pages of {}; {} to a page when no limit is set",
        artists.total(),
        artists.total_pages(),
        artists.per_page(),
        unset.per_page()
    );

    // Every match, page after page.
    let mut rows = search.artists("radiohead").limit(3).all();
    let mut seen = Vec::new();
    while seen.len() < 7 {
        seen.push(rows.next().await.unwrap().expect("a match"));
    }
    assert_eq!(seen[0].name(), artists[0].name());
    assert_eq!(seen[4].name(), artists[4].name());

    // Albums: each names its artist.
    let albums = search.albums("ok computer").limit(5).send().await.unwrap();
    capture("search-albums", albums.raw());
    assert_eq!(albums.subject(), Some("ok computer"));
    assert!(albums.iter().all(|album| album.artist().is_some()));
    let full = search.albums("love").limit(1000).send().await.unwrap();
    capture("search-albums-1000", full.raw());
    assert_eq!(full.len(), 1000);
    println!("albums: {} match", albums.total());

    // Nothing found is an empty page, and a page or a limit beyond 10000
    // is an error.
    let none = search
        .albums("zzqqxxjjkkvvwwyy")
        .limit(5)
        .send()
        .await
        .unwrap();
    assert!(none.is_empty());
    assert_eq!(none.total(), 0);
    for error in [
        search
            .artists("radiohead")
            .page(10_001)
            .send()
            .await
            .map(drop)
            .unwrap_err(),
        search
            .albums("love")
            .limit(10_001)
            .send()
            .await
            .map(drop)
            .unwrap_err(),
    ] {
        assert_eq!(error.api_code(), Some(ApiErrorCode::INVALID_PARAMETERS));
    }
    println!("beyond 10000: error 6");
}

#[tokio::test]
#[ignore = "calls Last.fm; needs SCROBL_LIVE_API_KEY and SCROBL_LIVE_USER"]
async fn a_track_against_the_live_service() {
    let key = env("SCROBL_LIVE_API_KEY");
    let name = env("SCROBL_LIVE_USER");
    let client = client(&key);
    let track = client.track("radiohead", "creep");

    // What the service says: both counts, a length, an album, a few tags,
    // and with a user named that user's plays and whether they love it.
    let info = track.info().send().await.unwrap();
    capture("track-info", info.raw());
    assert_eq!(info.track().name(), "Creep");
    assert_eq!(info.track().artist().name(), "Radiohead");
    assert_eq!(info.track().listeners(), Some(info.listeners()));
    let length = info.track().duration().expect("a length");
    assert!((60..1200).contains(&length.as_secs()), "{length:?}");
    assert!(info.track().album().is_some());
    assert_eq!((info.user_playcount(), info.user_loved()), (None, None));
    let mine = track.info().for_user(&name).send().await.unwrap();
    capture("track-info-user", mine.raw());
    assert!(mine.user_playcount().is_some() && mine.user_loved().is_some());
    println!(
        "track: {} listeners, {length:?}, {} tags; the user: {:?} plays, loved {:?}",
        info.listeners(),
        info.tags().len(),
        mine.user_playcount(),
        mine.user_loved()
    );
    let by_id = client
        .track_mbid("012e70cd-4ef6-37bf-8ebf-02d318fe5151")
        .info()
        .send()
        .await
        .unwrap();
    assert_eq!(by_id.track().name(), "Creep");
    let corrected = client
        .track("Guns and Roses", "Paradise City")
        .autocorrect()
        .info()
        .send()
        .await
        .unwrap();
    assert_eq!(corrected.track().artist().name(), "Guns N' Roses");

    // The names the service files a track under.
    let filed = client
        .track("Guns and Roses", "Paradise City")
        .correction()
        .send()
        .await
        .unwrap();
    capture("track-correction", filed.raw());
    assert_eq!(filed.artist().name(), "Guns N' Roses");
    let same = track.correction().send().await.unwrap();
    assert_eq!((same.name(), same.artist().name()), ("Creep", "Radiohead"));

    // Similar tracks: most alike first, and no more than the service has.
    let similar = track.similar().limit(5).send().await.unwrap();
    capture("track-similar", similar.raw());
    assert_eq!(similar.subject(), Some("Creep"));
    assert_eq!(similar.len(), 5);
    assert!(
        similar
            .windows(2)
            .all(|pair| pair[0].similarity() >= pair[1].similarity())
    );
    assert!(similar.iter().all(|row| row.track().playcount().is_some()));
    let most = track.similar().limit(1000).send().await.unwrap();
    capture("track-similar-1000", most.raw());
    let unset = track.similar().send().await.unwrap();
    println!(
        "similar: {} when 1000 are asked for, {} when no limit is set",
        most.len(),
        unset.len()
    );

    // Tags: the most given first, and one user's own.
    let top = track.top_tags().send().await.unwrap();
    capture("track-top-tags", top.raw());
    assert_eq!(top.subject(), Some("Creep"));
    assert_eq!(top[0].count(), 100);
    let tags = track.tags_of(&name).send().await.unwrap();
    capture("track-tags", tags.raw());
    assert_eq!(tags.subject(), Some("Creep"));
    println!("tags: {} given most, {} by the user", top.len(), tags.len());

    // A track the service does not know is an error on all five.
    let unknown = client.track("radiohead", "zzqqxx-no-such-track-scrobl");
    for error in [
        unknown.info().send().await.map(drop).unwrap_err(),
        unknown.correction().send().await.map(drop).unwrap_err(),
        unknown.similar().send().await.map(drop).unwrap_err(),
        unknown.top_tags().send().await.map(drop).unwrap_err(),
        unknown.tags_of(&name).send().await.map(drop).unwrap_err(),
    ] {
        assert_eq!(error.api_code(), Some(ApiErrorCode::INVALID_PARAMETERS));
    }
    println!("unknown track: error 6 on all five");

    // The search: narrowed by an artist, a page of 1000, every match page
    // after page, and where the pages stop.
    let search = client.search();
    let found = search.tracks("creep").limit(5).send().await.unwrap();
    capture("search-tracks", found.raw());
    assert_eq!(found.subject(), Some("creep"));
    assert_eq!((found.number(), found.len()), (1, 5));
    assert!(found.iter().all(|track| track.listeners().is_some()));
    let narrowed = search
        .tracks("creep")
        .artist("radiohead")
        .limit(5)
        .send()
        .await
        .unwrap();
    assert!(narrowed.total() < found.total());
    assert_eq!(narrowed[0].artist().name(), "Radiohead");
    let full = search.tracks("love").limit(1000).send().await.unwrap();
    capture("search-tracks-1000", full.raw());
    assert_eq!(full.len(), 1000);
    let unset = search.tracks("creep").send().await.unwrap();
    let mut rows = search.tracks("creep").limit(3).all();
    let mut seen = Vec::new();
    while seen.len() < 7 {
        seen.push(rows.next().await.unwrap().expect("a match"));
    }
    assert_eq!(seen[4].name(), found[4].name());
    let last = search.tracks("creep").limit(1).page(1000).send().await;
    let beyond = search.tracks("creep").limit(1).page(1001).send().await;
    let large = search.tracks("creep").limit(1001).send().await;
    println!(
        "search: {} match, {} with the artist; {} to a page when no limit is set; \
         page 1000 ok: {}, page 1001 ok: {}, a limit of 1001 ok: {}",
        found.total(),
        narrowed.total(),
        unset.per_page(),
        last.is_ok(),
        beyond.is_ok(),
        large.is_ok()
    );
    assert_eq!(unset.per_page(), 30);
    assert!(last.is_ok());
    assert_eq!(
        beyond.unwrap_err().api_code(),
        Some(ApiErrorCode::INVALID_PARAMETERS)
    );
    assert!(large.is_err());
}

#[tokio::test]
#[ignore = "calls Last.fm and creates a session; needs SCROBL_LIVE_API_KEY, SCROBL_LIVE_API_SECRET, SCROBL_LIVE_USER and the owner at a browser"]
async fn the_desktop_sign_in_against_the_live_service() {
    let user = env("SCROBL_LIVE_USER");
    let client = Client::builder(ApiKey::new(env("SCROBL_LIVE_API_KEY")))
        .secret(scrobl::ApiSecret::new(env("SCROBL_LIVE_API_SECRET")))
        .user_agent(USER_AGENT)
        .build()
        .unwrap();
    let auth = client.auth();

    // A token, then the exchange before the owner has approved it.
    let token = auth.token().send().await.unwrap();
    let shape = String::from_utf8_lossy(token.raw().body()).replace(token.expose(), "<token>");
    println!(
        "token reply, token replaced: {shape} (HTTP {})",
        token.raw().status()
    );
    let token = token.into_value();
    assert!(auth.session(&token).send().await.unwrap().is_none());

    // A token the service never issued.
    let never = scrobl::AuthToken::new("00000000000000000000000000000000");
    let error = auth.session(&never).send().await.unwrap_err();
    println!(
        "a token never issued: {:?}, HTTP {:?}",
        error.api_code(),
        error.http_status()
    );
    assert_eq!(error.kind(), ErrorKind::Api);

    println!("\nApprove the sign-in at:\n{}\n", auth.approval_url(&token));
    let started = std::time::Instant::now();
    let session = loop {
        if let Some(session) = auth.session(&token).send().await.unwrap() {
            break session;
        }
        assert!(
            started.elapsed() < std::time::Duration::from_secs(300),
            "not approved within five minutes"
        );
        tokio::time::sleep(std::time::Duration::from_secs(3)).await;
    };
    let shape =
        String::from_utf8_lossy(session.raw().body()).replace(session.key().expose(), "<key>");
    println!(
        "session reply, key replaced: {shape} (HTTP {})",
        session.raw().status()
    );
    assert!(session.name().eq_ignore_ascii_case(&user));
    assert!(!format!("{session:?}").contains(session.key().expose()));

    // The token is spent.
    let error = auth.session(&token).send().await.unwrap_err();
    println!(
        "the spent token again: {:?}, HTTP {:?}",
        error.api_code(),
        error.http_status()
    );
    assert_eq!(error.kind(), ErrorKind::Api);

    // The key works: one read made as the user.
    let as_user = client.with_session(session.into_value().into_key());
    let request =
        scrobl::protocol::Request::new(&scrobl::protocol::methods::USER_GET_RECENT_TRACKS)
            .param("user", user.as_str())
            .param("limit", 1)
            .as_user();
    let raw = as_user.call(&request).await.unwrap();
    println!("a read as the user: HTTP {}", raw.status());
}
