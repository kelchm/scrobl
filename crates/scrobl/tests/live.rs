//! Acceptance against the live Last.fm service. Read-only.
//!
//! Nothing here runs unless asked for: the test is ignored, and it fails at
//! once without `SCROBL_LIVE_API_KEY` and `SCROBL_LIVE_USER`. It is never
//! run in CI.
//!
//! ```sh
//! SCROBL_LIVE_API_KEY=... SCROBL_LIVE_USER=... \
//!     cargo test --test live -- --ignored --nocapture
//! ```
//!
//! The five tests send about eighty key-only requests between them, a
//! second apart: no signature, no session, no write. With
//! `SCROBL_LIVE_CAPTURE_DIR` set, the exact body of every response is
//! written there, for the owner to turn into fixtures by hand. The bodies
//! are that user's listening history; the tests commit nothing.

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
    page.attr().total()
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
    println!("history: {} scrobbles", latest.attr().total());
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
    assert!(first.user().eq_ignore_ascii_case(&user));
    assert_eq!(first.attr().per_page(), 5);
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
    assert!(artists.user().eq_ignore_ascii_case(&name));
    assert_eq!((artists.attr().page(), artists.attr().per_page()), (1, 5));
    let ranked = |page: &scrobl::model::TopArtistsPage| -> Vec<(u64, u64)> {
        page.artists()
            .iter()
            .map(|a| (a.rank(), a.playcount()))
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
    assert!(week.attr().total() <= artists.attr().total());
    println!(
        "top artists: {} overall, {} in seven days",
        artists.attr().total(),
        week.attr().total()
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
    assert!(beyond.artists().is_empty());

    // A page of a thousand of each list decodes: every row has what the
    // model requires.
    let artists = user.top_artists().limit(1000).send().await.unwrap();
    capture("top-artists-1000", artists.raw());
    assert_ranked("a thousand artists", 1, &ranked(&artists));
    println!(
        "top artists: a page of {} at `limit=1000`, `perPage` {}",
        artists.artists().len(),
        artists.attr().per_page()
    );

    let albums = user.top_albums().limit(1000).send().await.unwrap();
    capture("top-albums-1000", albums.raw());
    let rows: Vec<_> = albums
        .albums()
        .iter()
        .map(|a| (a.rank(), a.playcount()))
        .collect();
    assert_ranked("top albums", 1, &rows);
    println!(
        "top albums: {} in all, {} on the page, {} untitled, {} without an mbid",
        albums.attr().total(),
        rows.len(),
        albums
            .albums()
            .iter()
            .filter(|a| a.album().title().is_none())
            .count(),
        albums
            .albums()
            .iter()
            .filter(|a| a.album().mbid().is_none())
            .count()
    );

    let tracks = user.top_tracks().limit(1000).send().await.unwrap();
    capture("top-tracks-1000", tracks.raw());
    let rows: Vec<_> = tracks
        .tracks()
        .iter()
        .map(|t| (t.rank(), t.playcount()))
        .collect();
    assert_ranked("top tracks", 1, &rows);
    println!(
        "top tracks: {} in all, {} on the page, {} of unknown length",
        tracks.attr().total(),
        rows.len(),
        tracks
            .tracks()
            .iter()
            .filter(|t| t.duration().is_none())
            .count()
    );

    let tags = user.top_tags().send().await.unwrap();
    capture("top-tags", tags.raw());
    assert!(tags.user().eq_ignore_ascii_case(&name));
    let limited = user.top_tags().limit(1).send().await.unwrap();
    capture("top-tags-1", limited.raw());
    assert!(limited.tags().len() <= 1);
    println!("top tags: {}", tags.tags().len());

    // The library is the same artists as the top list of all time, and one
    // row to a page comes as a single object.
    let library = user.library_artists().limit(1000).send().await.unwrap();
    capture("library-artists-1000", library.raw());
    assert_eq!(library.attr().total(), artists.attr().total());
    assert!(
        library
            .artists()
            .windows(2)
            .all(|pair| pair[0].playcount() >= pair[1].playcount()),
        "library: most played first"
    );
    let one = user.library_artists().limit(1).send().await.unwrap();
    capture("library-artists-1", one.raw());
    assert_eq!(one.artists().len(), 1);
    assert_eq!(
        one.artists()[0].artist().name(),
        library.artists()[0].artist().name()
    );
    println!(
        "library: {} artists, {} on the page",
        library.attr().total(),
        library.artists().len()
    );
}

#[tokio::test]
#[ignore = "calls Last.fm; needs SCROBL_LIVE_API_KEY and SCROBL_LIVE_USER"]
async fn the_weekly_charts_of_a_user_against_the_live_service() {
    let key = env("SCROBL_LIVE_API_KEY");
    let name = env("SCROBL_LIVE_USER");
    let user = client(&key).user(&name);

    // The weeks follow one another without a gap.
    let weeks = user.weekly_chart_list().send().await.unwrap();
    capture("weekly-chart-list", weeks.raw());
    assert!(weeks.user().eq_ignore_ascii_case(&name));
    assert!(
        weeks
            .ranges()
            .windows(2)
            .all(|pair| pair[0].to() == pair[1].from()),
        "each week ends where the next begins"
    );
    let last = weeks.ranges().last().expect("the list has no weeks");
    let (from, to) = (last.from(), last.to());
    println!(
        "chart list: {} weeks, the last {from} to {to}",
        weeks.ranges().len()
    );

    // Each chart of the last listed week echoes the week and is ranked.
    let artists = user
        .weekly_artist_chart()
        .range(from, to)
        .send()
        .await
        .unwrap();
    capture("weekly-artist-chart", artists.raw());
    assert!(artists.user().eq_ignore_ascii_case(&name));
    assert_eq!((artists.from(), artists.to()), (from, to));
    let rows: Vec<_> = artists
        .artists()
        .iter()
        .map(|a| (a.rank(), a.playcount()))
        .collect();
    assert_ranked("weekly artists", 1, &rows);

    let albums = user
        .weekly_album_chart()
        .range(from, to)
        .send()
        .await
        .unwrap();
    capture("weekly-album-chart", albums.raw());
    assert_eq!((albums.from(), albums.to()), (from, to));
    let album_rows: Vec<_> = albums
        .albums()
        .iter()
        .map(|a| (a.rank(), a.playcount()))
        .collect();
    assert_ranked("weekly albums", 1, &album_rows);

    let tracks = user
        .weekly_track_chart()
        .range(from, to)
        .send()
        .await
        .unwrap();
    capture("weekly-track-chart", tracks.raw());
    assert_eq!((tracks.from(), tracks.to()), (from, to));
    let track_rows: Vec<_> = tracks
        .tracks()
        .iter()
        .map(|t| (t.rank(), t.playcount()))
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

    // Without a range the service chooses the span itself.
    let chosen = user.weekly_artist_chart().send().await.unwrap();
    capture("weekly-artist-chart-default", chosen.raw());
    println!(
        "no range: {} to {}, {} artists, a listed week: {}",
        chosen.from(),
        chosen.to(),
        chosen.artists().len(),
        weeks
            .ranges()
            .iter()
            .any(|w| (w.from(), w.to()) == (chosen.from(), chosen.to()))
    );

    // The first listed week.
    let first = weeks.ranges().first().expect("the list has no weeks");
    let oldest = user
        .weekly_track_chart()
        .range(first.from(), first.to())
        .send()
        .await
        .unwrap();
    capture("weekly-track-chart-first", oldest.raw());
    assert_eq!((oldest.from(), oldest.to()), (first.from(), first.to()));
    println!("first listed week: {} tracks", oldest.tracks().len());
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
    assert!(info.registered() > 0);
    let artists = user.top_artists().limit(1).send().await.unwrap();
    capture("top-artists-1", artists.raw());
    let albums = user.top_albums().limit(1).send().await.unwrap();
    capture("top-albums-1", albums.raw());
    let tracks = user.top_tracks().limit(1).send().await.unwrap();
    capture("top-tracks-1", tracks.raw());
    println!(
        "profile: {} scrobbles; artists {:?} (top list {}), albums {:?} ({}), tracks {:?} ({})",
        info.playcount(),
        info.artist_count(),
        artists.attr().total(),
        info.album_count(),
        albums.attr().total(),
        info.track_count(),
        tracks.attr().total()
    );

    // Friends: a page decodes, and a page past the last is an error.
    let friends = user.friends().limit(2).send().await.unwrap();
    capture("friends", friends.raw());
    assert!(friends.user().eq_ignore_ascii_case(&name));
    assert_eq!(friends.attr().per_page(), 2);
    assert!(friends.users().len() <= 2);
    println!(
        "friends: {} in all; on the page, playcounts {:?}, counts of artists {:?}",
        friends.attr().total(),
        friends
            .users()
            .iter()
            .map(|u| u.playcount())
            .collect::<Vec<_>>(),
        friends
            .users()
            .iter()
            .map(|u| u.artist_count())
            .collect::<Vec<_>>()
    );
    let past = u32::try_from(friends.attr().total_pages()).unwrap() + 1;
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
    assert!(loved.user().eq_ignore_ascii_case(&name));
    assert!(
        loved
            .tracks()
            .windows(2)
            .all(|pair| pair[0].loved_at() >= pair[1].loved_at()),
        "loved tracks: newest first"
    );
    println!("loved tracks: {}", loved.attr().total());

    // What the user gave each of their three most used tags.
    let tags = user.top_tags().limit(3).send().await.unwrap();
    capture("top-tags-3", tags.raw());
    for (index, tag) in tags.tags().iter().enumerate() {
        let artists = user
            .tagged_artists(tag.name())
            .limit(2)
            .send()
            .await
            .unwrap();
        capture(&format!("tagged-artists-{index}"), artists.raw());
        let albums = user
            .tagged_albums(tag.name())
            .limit(2)
            .send()
            .await
            .unwrap();
        capture(&format!("tagged-albums-{index}"), albums.raw());
        let tracks = user
            .tagged_tracks(tag.name())
            .limit(2)
            .send()
            .await
            .unwrap();
        capture(&format!("tagged-tracks-{index}"), tracks.raw());
        assert!(artists.tag().eq_ignore_ascii_case(tag.name()));
        assert!(artists.user().eq_ignore_ascii_case(&name));
        assert_eq!(artists.attr().per_page(), 2);
        println!(
            "a tag used {} times: {} artists, {} albums, {} tracks",
            tag.count(),
            artists.attr().total(),
            albums.attr().total(),
            tracks.attr().total()
        );
    }
}

#[tokio::test]
#[ignore = "calls Last.fm; needs SCROBL_LIVE_API_KEY"]
async fn the_charts_of_the_service_and_of_a_country_against_the_live_service() {
    let key = env("SCROBL_LIVE_API_KEY");
    let client = client(&key);
    let chart = client.chart();

    // The chart of artists: the service echoes what was asked, a second
    // page holds other artists, a page of 1000 decodes, and a page beyond
    // the last is empty and not an error.
    let artists = chart.top_artists().limit(5).send().await.unwrap();
    capture("chart-artists", artists.raw());
    assert_eq!((artists.attr().page(), artists.attr().per_page()), (1, 5));
    assert_eq!(artists.artists().len(), 5);
    let second = chart.top_artists().limit(5).page(2).send().await.unwrap();
    capture("chart-artists-page-2", second.raw());
    assert_eq!(second.attr().page(), 2);
    assert_ne!(
        second.artists()[0].artist().name(),
        artists.artists()[0].artist().name()
    );
    let full = chart.top_artists().limit(1000).send().await.unwrap();
    capture("chart-artists-1000", full.raw());
    assert_eq!(full.artists().len(), 1000);
    let beyond = chart
        .top_artists()
        .limit(50)
        .page(999_999)
        .send()
        .await
        .unwrap();
    capture("chart-artists-beyond", beyond.raw());
    assert!(beyond.artists().is_empty());
    println!(
        "chart artists: {} listed, first {} plays",
        artists.attr().total(),
        artists.artists()[0].playcount()
    );

    let tracks = chart.top_tracks().limit(5).send().await.unwrap();
    capture("chart-tracks", tracks.raw());
    assert_eq!(tracks.tracks().len(), 5);
    let full = chart.top_tracks().limit(1000).send().await.unwrap();
    capture("chart-tracks-1000", full.raw());
    assert_eq!(full.tracks().len(), 1000);
    let unknown = full
        .tracks()
        .iter()
        .filter(|t| t.duration().is_none())
        .count();
    println!("chart tracks: {unknown} of 1000 without a length");

    // The chart of tags. Whether it pages is printed and not asserted: on
    // 2026-11-11 page 2 held the rows of page 1.
    let tags = chart.top_tags().limit(5).send().await.unwrap();
    capture("chart-tags", tags.raw());
    assert_eq!(tags.tags().len(), 5);
    let second = chart.top_tags().limit(5).page(2).send().await.unwrap();
    capture("chart-tags-page-2", second.raw());
    println!(
        "chart tags: {} in all; page 2 starts with `{}`, page 1 with `{}`",
        tags.attr().total(),
        second.tags()[0].name(),
        tags.tags()[0].name()
    );
    let full = chart.top_tags().limit(1000).send().await.unwrap();
    capture("chart-tags-1000", full.raw());
    assert_eq!(full.tags().len(), 1000);

    // A country: the service spells the name back its own way, and refuses
    // a code and a name it does not know.
    let germany = client.geo("germany");
    let artists = germany.top_artists().limit(5).send().await.unwrap();
    capture("geo-artists", artists.raw());
    assert_eq!(artists.country(), "Germany");
    assert_eq!(artists.artists().len(), 5);
    let full = germany.top_artists().limit(1000).send().await.unwrap();
    capture("geo-artists-1000", full.raw());
    assert_eq!(full.artists().len(), 1000);
    let tracks = germany.top_tracks().limit(5).send().await.unwrap();
    capture("geo-tracks", tracks.raw());
    assert_eq!(tracks.country(), "Germany");
    assert_eq!(tracks.tracks().len(), 5);
    let full = germany.top_tracks().limit(1000).send().await.unwrap();
    capture("geo-tracks-1000", full.raw());
    assert_eq!(full.tracks().len(), 1000);
    println!(
        "Germany: {} artists, {} tracks listed",
        artists.attr().total(),
        tracks.attr().total()
    );

    for country in ["DE", "Atlantis"] {
        let error = client
            .geo(country)
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
