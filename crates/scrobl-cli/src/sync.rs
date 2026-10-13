//! Making the database agree with the history at Last.fm.
//!
//! A sync works on *spans* of time, `[from, to)`, and has one way of
//! dealing with any of them: ask Last.fm how many scrobbles the span holds,
//! and compare that with the database. If the two agree, the span is left
//! alone. If they differ and Last.fm counts more than [`Options::chunk`]
//! scrobbles in the span, it is halved and each half is dealt with the same
//! way. Otherwise the span is read, as one validated scan, and the
//! database's copy of it is replaced by what the scan read, in one
//! transaction.
//!
//! That one rule is the first import, the daily run and the repair of a
//! change to old history. Nothing it stores is final and nothing records how
//! far it got, so a sync may stop anywhere: the next one passes over what
//! was stored, wherever it agrees, and reads the rest.
//!
//! Equal counts do not prove equal rows, and the newest history is where
//! that matters: a scrobble is dated when its track started and sent when it
//! ended, or days later from a device that was offline. So the last
//! [`Options::recent`] seconds are read every time, whatever the counts say,
//! and [`Options::full`] reads the whole history that way. A change that
//! leaves a count as it was, in history that is not recent, waits for that.
//!
//! [`loved`] reads the tracks the user has loved, which is a list of its
//! own and not part of the history, and replaces the copy in the database.

use scrobl::history::{ScanPage, ScanSummary, Window};
use scrobl::{Client, ErrorKind};

use crate::Error;
use crate::store::{Change, Store};
use crate::time;

/// How many times a span is read before a sync gives up on it.
const ATTEMPTS: u32 = 3;

/// The rows asked for on every page of loved tracks, the most the service
/// was seen to give.
const LOVED_PAGE: u32 = 1000;

/// How a sync goes about the history. The tool runs on the defaults; the
/// tests shrink them, so that a few dozen scrobbles are a history worth
/// halving.
#[derive(Debug, Clone, Copy)]
pub struct Options {
    /// How much of the newest history every sync reads again, in seconds.
    pub recent: u64,
    /// A span in which Last.fm counts more scrobbles than this is halved,
    /// unless it is a single second.
    pub chunk: u64,
    /// The rows asked for on every page of a scan, 1 to 200.
    pub page_size: u32,
    /// Read the whole history again, whatever the counts say.
    pub full: bool,
}

impl Default for Options {
    /// Fourteen days, because several clients report that Last.fm ignores a
    /// scrobble dated more than about two weeks back; 5000 scrobbles to a
    /// scan; and pages of 200, the most the service gives.
    fn default() -> Self {
        Self {
            recent: 14 * 86_400,
            chunk: 5_000,
            page_size: 200,
            full: false,
        }
    }
}

/// One span, as it was read and stored.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Stored {
    /// The inclusive lower bound.
    pub from: u64,
    /// The exclusive upper bound.
    pub to: u64,
    pub scrobbles: u64,
    /// How many of them the database did not hold before.
    pub added: u64,
    /// Scrobbles the database held for the span that Last.fm no longer
    /// lists. They were moved to `removed`.
    pub removed: u64,
}

/// Makes `store` agree with `user`'s history before `now`, and returns what
/// that added to the file and removed from it. `stored` is called for each
/// span once it is committed, and the spans stored before an error stay
/// stored. A new database is tied to `user` first.
pub async fn sync(
    client: &Client,
    store: &mut Store,
    user: &str,
    now: u64,
    options: Options,
    mut stored: impl FnMut(Stored),
) -> Result<Change, Error> {
    store.bind_user(user)?;
    let recent_from = now.saturating_sub(options.recent);
    let (mut added, mut removed) = (0, 0);

    // The last span pushed is the next one dealt with, so the recent part
    // goes first and the rest follows from its newest end backwards: the
    // newest history is in the file whatever becomes of the rest.
    let mut spans = vec![(0, recent_from, options.full), (recent_from, now, true)];
    while let Some((from, to, always)) = spans.pop() {
        if from >= to {
            continue;
        }
        let theirs = count(client, user, from, to).await?;
        if !always && theirs == store.count(from, to)? {
            continue;
        }
        if theirs > options.chunk && to - from > 1 {
            let middle = from + (to - from) / 2;
            spans.push((from, middle, always));
            spans.push((middle, to, always));
            continue;
        }
        let (summary, pages) = read(client, user, from, to, options.page_size).await?;
        let change = store.replace(&summary, &pages, now)?;
        added += change.added;
        removed += change.removed;
        stored(Stored {
            from,
            to,
            scrobbles: summary.total(),
            added: change.added,
            removed: change.removed,
        });
    }

    store.note_history_read(now)?;
    Ok(Change { added, removed })
}

/// `[from, to)` as the library takes it. A lower bound of 0 is sent as no
/// lower bound, which means the same and is the plainer request.
fn window(from: u64, to: u64) -> Result<Window, scrobl::Error> {
    if from == 0 {
        Ok(Window::before(to))
    } else {
        Window::new(from, to)
    }
}

/// One request: how many scrobbles Last.fm counts in `[from, to)`.
///
/// The count only decides where a sync looks, so nothing about it is
/// checked. What is stored comes from a scan, which is checked in full.
async fn count(client: &Client, user: &str, from: u64, to: u64) -> Result<u64, Error> {
    let page = client
        .user(user)
        .recent_tracks()
        .window(window(from, to)?)
        .limit(1)
        .send()
        .await?;
    Ok(page.attr().total())
}

/// Reads `[from, to)` completely, as one scan.
///
/// A scrobble that arrives in the span while it is read can make the pages
/// disagree with each other, and then the scan refuses them. That is no
/// fault, least of all where the span reaches the present, so the span is
/// read again, [`ATTEMPTS`] times in all.
async fn read(
    client: &Client,
    user: &str,
    from: u64,
    to: u64,
    page_size: u32,
) -> Result<(ScanSummary, Vec<ScanPage>), Error> {
    let mut attempt = 1;
    loop {
        match scan(client, user, from, to, page_size).await {
            Err(error) if error.kind() == ErrorKind::Inconsistent && attempt < ATTEMPTS => {
                attempt += 1;
            }
            reading => return Ok(reading?),
        }
    }
}

/// One scan of `[from, to)`. The pages are held in memory until the scan has
/// finished, because until then they are provisional.
///
/// The pages are asked for in the plain form. With `extended=1` Last.fm
/// sends every artist's `mbid` empty, and what it adds in exchange is the
/// loved flag, which is not a fact about one scrobble.
async fn scan(
    client: &Client,
    user: &str,
    from: u64,
    to: u64,
    page_size: u32,
) -> Result<(ScanSummary, Vec<ScanPage>), scrobl::Error> {
    let mut scan = client
        .user(user)
        .recent_tracks()
        .window(window(from, to)?)
        .limit(page_size)
        .scan()?;
    let mut pages = Vec::new();
    while let Some(page) = scan.next_page().await? {
        pages.push(page);
    }
    Ok((scan.finish()?, pages))
}

/// Reads every track `user` has loved, a page at a time, and replaces the
/// database's copy with them. Returns how many there are.
///
/// Nothing ties one page to the next, so a track loved or unloved during the
/// read can leave a row listed twice or missing, until the next sync reads
/// the list afresh. The list ends where its rows add up to the total, or at
/// a page with no rows.
pub async fn loved(client: &Client, store: &mut Store, user: &str) -> Result<u64, Error> {
    let mut pages = Vec::new();
    let mut read = 0;
    loop {
        let page = client
            .user(user)
            .loved_tracks()
            .limit(LOVED_PAGE)
            .page(pages.len() as u32 + 1)
            .send()
            .await?;
        let (rows, total) = (page.tracks().len() as u64, page.attr().total());
        read += rows;
        pages.push(page);
        if rows == 0 || read >= total {
            break;
        }
    }
    store.replace_loved(&pages, time::now())?;
    Ok(read)
}
