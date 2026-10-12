//! Making the database agree with the history at Last.fm.
//!
//! A sync works on *spans* of time, `[from, to)`, and has one way of
//! dealing with any of them: ask Last.fm how many scrobbles the span holds,
//! and compare that with the database. If the two agree, the span is left
//! alone. If they differ and the span holds more than [`Options::chunk`]
//! scrobbles, it is halved and each half is dealt with the same way.
//! Otherwise the span is read, as one validated scan, and the database's
//! copy of it is replaced by what the scan read, in one transaction.
//!
//! That one rule is the first import, the daily run and the repair of a
//! change to old history. A new database agrees with Last.fm only where the
//! history is empty, so all of it is read. A sync that stops, for any
//! reason, has lost at most the span it was reading, and the next sync finds
//! that what was stored agrees and reads the rest. A scrobble that turns up
//! in a year long past changes that year's count, and the halving leads to
//! it.
//!
//! Equal counts do not prove equal rows, and the newest history is where
//! that matters: a scrobble is dated when its track started and sent when it
//! ended, or days later from a device that was offline. So the last
//! [`Options::recent`] seconds are read every time, whatever the counts say,
//! and [`Options::full`] reads the whole history that way.
//!
//! Nothing a sync stores is final and nothing records how far it got, so a
//! clock that is wrong or a second sync on the same file can do no lasting
//! harm: whatever one sync left wrong, the next one finds by its count or
//! reads again because it is recent.
//!
//! [`loved`] reads the tracks the user has loved, which is a list of its
//! own and not part of the history, and replaces the copy in the database.

use scrobl::history::{ScanPage, ScanSummary, Window};
use scrobl::model::LovedTracksPage;
use scrobl::{Client, ErrorKind, Response};

use crate::Error;
use crate::store::Store;
use crate::time;

/// How many times a span is read when Last.fm's pages do not agree with
/// each other, which is what a scrobble arriving during the read looks
/// like.
const ATTEMPTS: u32 = 3;

/// How a sync goes about the history.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub struct Options {
    /// How much of the newest history every sync reads again, in seconds.
    pub recent: u64,
    /// The most scrobbles read as one scan. A span that holds more is
    /// halved, unless it is a single second.
    pub chunk: u64,
    /// The rows asked for on every page of a scan, 1 to 200.
    pub page_size: u32,
    /// The rows asked for on every page of loved tracks, 1 to 1000.
    pub loved_page_size: u32,
    /// Read the whole history again, whatever the counts say.
    pub full: bool,
}

impl Default for Options {
    /// The last fourteen days read every time, scans of at most 5000
    /// scrobbles, pages of 200 scrobbles, the most the service gives, pages
    /// of 1000 loved tracks, and the rest of the history read only where the
    /// counts differ.
    ///
    /// Fourteen days because several clients report that Last.fm ignores a
    /// scrobble dated more than about two weeks back. Last.fm documents no
    /// number, and nothing depends on this one: a scrobble that arrives
    /// later still changes a count.
    fn default() -> Self {
        Self {
            recent: 14 * 86_400,
            chunk: 5_000,
            page_size: 200,
            loved_page_size: 1000,
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
    /// The scrobbles in it.
    pub scrobbles: u64,
    /// How many of them the database did not hold before.
    pub added: u64,
    /// Scrobbles the database held for the span that Last.fm no longer
    /// lists. They were moved to `removed`.
    pub removed: u64,
}

/// What a sync did.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Report {
    /// The history was read up to here.
    pub to: u64,
    /// Where the recent part began: from here on everything was read,
    /// before it only the spans whose counts differed.
    pub recent_from: u64,
    /// Scrobbles this sync stored that the database did not hold.
    pub added: u64,
    /// Scrobbles the database held that Last.fm no longer lists. They were
    /// moved to `removed`.
    pub removed: u64,
    /// Scrobbles in the database before `to`.
    pub stored: u64,
    /// Scrobbles in the database before `recent_from`.
    pub older: u64,
    /// Scrobbles Last.fm counted before `recent_from`, asked after the last
    /// span was stored.
    pub reported: u64,
}

impl Report {
    /// Whether the database holds as many scrobbles as Last.fm counts,
    /// before the recent part. The recent part is left out because it is
    /// still filling in. Equal counts do not prove equal rows.
    pub fn matches(&self) -> bool {
        self.older == self.reported
    }
}

/// Makes `store` agree with `user`'s history before `now`, then compares
/// the counts once more. `stored` is called for each span once it is
/// committed. A new database is tied to `user` first.
///
/// # Errors
///
/// [`Error::OtherUser`] when the database holds another user's history;
/// [`Error::Lastfm`] when a call fails after the client's retries or a span
/// does not hold still for three readings; [`Error::Service`] when a count
/// is answered for another user; [`Error::Database`] and [`Error::Sqlite`]
/// from the store. The spans stored before the error stay stored.
pub async fn sync(
    client: &Client,
    store: &mut Store,
    user: &str,
    now: u64,
    options: Options,
    mut stored: impl FnMut(Stored),
) -> Result<Report, Error> {
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

    let reported = match recent_from {
        0 => 0,
        recent_from => count(client, user, 0, recent_from).await?,
    };
    store.note_history_read(now)?;
    Ok(Report {
        to: now,
        recent_from,
        added,
        removed,
        stored: store.count(0, now)?,
        older: store.count(0, recent_from)?,
        reported,
    })
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
/// The count only decides where a sync looks. What is stored comes from a
/// scan, which is checked in full, so all that is asked of this answer is
/// that it is about the right user.
async fn count(client: &Client, user: &str, from: u64, to: u64) -> Result<u64, Error> {
    let page = client
        .user(user)
        .recent_tracks()
        .window(window(from, to)?)
        .limit(1)
        .send()
        .await?;
    if !page.attr().user().eq_ignore_ascii_case(user) {
        return Err(Error::Service("Last.fm answered a count for another user"));
    }
    Ok(page.attr().total())
}

/// Reads `[from, to)` completely, as one scan.
///
/// A scrobble that arrives in the span while it is read makes the pages
/// disagree with each other, and the scan refuses them. That is no fault,
/// least of all where the span reaches the present, so the span is read
/// again, three times in all. Only a disturbance that shows is retried: a
/// reading that passes every check is still not a picture of one moment.
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

/// Reads every track `user` has loved and replaces the database's copy
/// with it. Returns how many there are.
///
/// The list is read newest first, a page at a time, and nothing ties one
/// page to the next: a track loved or unloved between two requests moves
/// every later row. So the read is kept only if every page reports the same
/// total, every page but the last is full, and the pages hold exactly that
/// many tracks. The end of the list is where the rows add up to the total,
/// whatever `totalPages` says. If any of that fails nothing is stored, and
/// the copy from the last sync stays as it was.
///
/// That catches a list that grew or shrank during the read. It does not
/// catch one track unloved and another loved between two pages, which
/// leaves the total as it was and one row missing or listed twice. The next
/// sync reads the list afresh, so such a row is wrong for a day. Only a
/// list that fits one page is read at a single moment.
///
/// A new database is tied to `user` first.
///
/// # Errors
///
/// [`Error::OtherUser`] when the database holds another user's loved
/// tracks; [`Error::Lastfm`] when a call fails; [`Error::Service`] when a
/// page is another user's or the list changed while it was read;
/// [`Error::Sqlite`] from the store.
pub async fn loved(
    client: &Client,
    store: &mut Store,
    user: &str,
    page_size: u32,
) -> Result<u64, Error> {
    store.bind_user(user)?;
    let mut pages: Vec<Response<LovedTracksPage>> = Vec::new();
    let mut read = 0;
    loop {
        let page = client
            .user(user)
            .loved_tracks()
            .limit(page_size)
            .page(pages.len() as u32 + 1)
            .send()
            .await?;
        if !page.attr().user().eq_ignore_ascii_case(user) {
            return Err(Error::Service(
                "Last.fm answered with another user's loved tracks",
            ));
        }
        let total = page.attr().total();
        let moved = pages
            .first()
            .is_some_and(|first| first.attr().total() != total);
        let full = page.tracks().len() as u64 == u64::from(page_size);
        read += page.tracks().len() as u64;
        pages.push(page);
        // Every page before the end is full, so each request brings the end
        // nearer and a short page is a list that shrank.
        if moved || read > total || (read < total && !full) {
            return Err(Error::Service(
                "the loved tracks changed while they were being read; the next sync reads them \
                 again",
            ));
        }
        if read == total {
            break;
        }
    }
    store.replace_loved(&pages, time::now())?;
    Ok(read)
}
