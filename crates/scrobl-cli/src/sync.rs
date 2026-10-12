//! Reading the history the database does not hold yet, and reading the
//! newest part of it again.
//!
//! A sync has a *cutoff*, fixed when it starts: the present, by Last.fm's
//! clock as well as this machine's, which is what [`server_time`] is for.
//! It makes the database cover `[0, cutoff)`.
//!
//! The history up to [`Options::settle`] before the cutoff is *sealed*: it
//! is read once, a window at a time from its newest end backwards, each
//! window one validated scan stored in one transaction. A sync that stops,
//! for any reason, has lost at most the window it was reading, and the next
//! sync carries on from there. Windows are sized by time,
//! [`Options::span`] at most, and placed by where the scrobbles are: before
//! each one the sync asks Last.fm for the newest scrobble left in the span,
//! so years of silence cost one request, not one per month.
//!
//! The history newer than that is the *tail*. A scrobble is dated when its
//! track started and sent when it ended, or days later from a device that
//! was offline, so the tail is still filling in. Every sync reads all of it
//! again and replaces the copy in the database in one transaction: the part
//! that has grown old enough is sealed, after this one more reading, and
//! the rest stays the tail. The sealed edge moves a whole UTC day at a
//! time, so that a sync every few minutes does not leave a window behind
//! each time.
//!
//! At the end the sync asks Last.fm how many scrobbles lie before the
//! sealed edge and puts that next to what the database holds. The two differ
//! when the number of scrobbles in a period changed after it was sealed.
//! This version reports that; it does not read a sealed period twice. It is
//! one count for the whole sealed history, so a scrobble removed and another
//! added, in the same period or in two, leave it as it was.
//!
//! [`loved`] reads the tracks the user has loved, which is a list of its
//! own and not part of the history, and replaces the copy in the database.

use std::collections::HashSet;

use scrobl::history::{ScanPage, ScanSummary, Window};
use scrobl::model::LovedTracksPage;
use scrobl::{Client, ErrorKind, Response};

use crate::Error;
use crate::store::{Store, Tail, TailChange, TailWindow};
use crate::time;

/// How many times the tail is read when Last.fm's answers do not agree
/// with each other, which is what a scrobble arriving during the read
/// looks like.
const TAIL_ATTEMPTS: u32 = 3;

/// How a sync divides the history.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub struct Options {
    /// The longest stretch of listening one window holds, in seconds. Zero
    /// is taken as one.
    pub span: u64,
    /// The rows asked for on every page of a scan, 1 to 200.
    pub page_size: u32,
    /// The rows asked for on every page of loved tracks, 1 to 1000.
    pub loved_page_size: u32,
    /// How long a scrobble's time has to be past before the window it is
    /// in is sealed, in seconds. Everything newer is read again by every
    /// sync. Zero seals everything a sync reads, at once.
    pub settle: u64,
}

impl Default for Options {
    /// Windows of thirty days, pages of 200 scrobbles, the most the service
    /// gives, pages of 1000 loved tracks, and fourteen days before a window
    /// is sealed.
    fn default() -> Self {
        Self {
            span: 30 * 86_400,
            page_size: 200,
            loved_page_size: 1000,
            settle: 14 * 86_400,
        }
    }
}

/// One window, as it was stored.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Stored {
    /// The inclusive lower bound.
    pub from: u64,
    /// The exclusive upper bound.
    pub to: u64,
    /// The scrobbles in it.
    pub scrobbles: u64,
    /// Whether it is final. A window that is not is read again next time.
    pub sealed: bool,
}

/// What a sync did.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Report {
    /// The database now covers `[0, cutoff)`.
    pub cutoff: u64,
    /// The history before this is sealed; from here to the cutoff is the
    /// tail.
    pub sealed_to: u64,
    /// How many more scrobbles the database holds than before the sync.
    pub added: u64,
    /// Scrobbles in the database before the cutoff.
    pub stored: u64,
    /// Scrobbles in the database before the sealed edge.
    pub sealed: u64,
    /// Scrobbles Last.fm reported before the sealed edge, asked after the
    /// last window was stored.
    pub reported: u64,
    /// Scrobbles the tail held at the last sync that Last.fm no longer
    /// lists. The reading that had them is kept in the database.
    pub lost: u64,
}

impl Report {
    /// Whether the database holds as many sealed scrobbles as Last.fm
    /// reports for the same time. Equal counts do not prove equal rows.
    pub fn matches(&self) -> bool {
        self.sealed == self.reported
    }
}

/// Makes `store` cover `user`'s history before `cutoff`, reading the tail
/// again, then compares the counts. `stored` is called for each window
/// once it is committed. A new database is tied to `user` first.
///
/// # Errors
///
/// [`Error::OtherUser`] when the database holds another user's history;
/// [`Error::Lastfm`] when a call fails after the client's retries or a page
/// breaks a rule of the scan; [`Error::Service`] when an answer cannot be
/// acted on, or the cutoff is earlier than what the database covers;
/// [`Error::Database`] and [`Error::Sqlite`] from the store. The windows
/// stored before the error stay stored, and the tail stays as it was unless
/// its new reading was stored whole.
pub async fn sync(
    client: &Client,
    store: &mut Store,
    user: &str,
    cutoff: u64,
    options: Options,
    mut stored: impl FnMut(Stored),
) -> Result<Report, Error> {
    if cutoff == 0 {
        return Err(Error::Service("the cutoff is the epoch; check the clock"));
    }
    store.bind_user(user)?;
    if cutoff < store.covered_to()? {
        return Err(Error::Service(
            "the database already covers a later time than it is now; check the clock",
        ));
    }
    let before = store.scrobbles_before(cutoff)?;

    // The sealed edge: a whole day, and never earlier than what is sealed
    // already or than where the tail began.
    let tail = store.tail()?;
    let settled = match options.settle {
        0 => cutoff,
        settle => cutoff.saturating_sub(settle) / 86_400 * 86_400,
    };
    let floor = match &tail {
        Some(tail) => tail.from,
        None => store.sealed_to()?,
    };
    let sealed_to = settled.max(floor);

    // The tail first, so that the newest history is there whatever happens
    // to the rest.
    let mut lost = 0;
    let mut replace = async |store: &mut Store, was: Option<&Tail>, from, seal_to, to| {
        let change = replace_tail(
            client,
            store,
            user,
            was,
            (from, seal_to, to),
            options,
            &mut stored,
        )
        .await?;
        lost += change.lost;
        Ok::<(), Error>(())
    };
    match &tail {
        // The old tail reaches past the edge: one reading replaces it, the
        // older part sealed and the newer part the tail from now on.
        Some(tail) if tail.to > sealed_to => {
            replace(store, Some(tail), tail.from, sealed_to, cutoff).await?;
        }
        // The whole of the old tail is old enough: it is read once more
        // and sealed, and the new tail is read on its own.
        Some(tail) => {
            replace(store, Some(tail), tail.from, tail.to, tail.to).await?;
            replace(store, None, sealed_to, sealed_to, cutoff).await?;
        }
        None => replace(store, None, sealed_to, sealed_to, cutoff).await?,
    }

    for (gap_from, gap_to) in store.gaps(sealed_to)? {
        let mut to = gap_to;
        while to > gap_from {
            let from = window_start(client, user, gap_from, to, options.span.max(1)).await?;
            let (summary, pages) = read(client, user, from, to, options.page_size).await?;
            store.commit_window(&summary, &pages, time::now())?;
            stored(Stored {
                from,
                to,
                scrobbles: summary.total(),
                sealed: true,
            });
            to = from;
        }
    }

    let reported = match sealed_to {
        0 => 0,
        sealed_to => probe(client, user, 0, sealed_to).await?.0,
    };
    let now = store.scrobbles_before(cutoff)?;
    Ok(Report {
        cutoff,
        sealed_to,
        added: now.saturating_sub(before),
        stored: now,
        sealed: store.scrobbles_before(sealed_to)?,
        reported,
        lost,
    })
}

/// Reads `[from, to)` afresh and puts it in place of the tail `was`: the
/// part before `seal_to` sealed, the rest as the new tail. Nothing is
/// asked or stored when the span is empty and there is no tail to replace.
///
/// A scrobble that arrives while the span is read makes Last.fm's answers
/// disagree, and the scan refuses them. That is no fault here, where the
/// span reaches the present, so the reading is tried again, three times in
/// all, and the tail in the database stays as it was until one succeeds.
async fn replace_tail(
    client: &Client,
    store: &mut Store,
    user: &str,
    was: Option<&Tail>,
    (from, seal_to, to): (u64, u64, u64),
    options: Options,
    stored: &mut impl FnMut(Stored),
) -> Result<TailChange, Error> {
    if from >= to && was.is_none() {
        return Ok(TailChange::default());
    }
    let mut attempt = 1;
    let (sealed, open) = loop {
        let reading = async {
            let sealed = read_span(client, user, from, seal_to, options).await?;
            let open = read_span(client, user, seal_to, to, options).await?;
            Ok::<_, Error>((sealed, open))
        };
        match reading.await {
            Err(Error::Lastfm(error))
                if error.kind() == ErrorKind::Inconsistent && attempt < TAIL_ATTEMPTS =>
            {
                attempt += 1;
            }
            reading => break reading?,
        }
    };
    let windows: Vec<TailWindow<'_>> = sealed
        .iter()
        .map(|window| (window, true))
        .chain(open.iter().map(|window| (window, false)))
        .map(|((summary, pages), sealed)| TailWindow {
            summary,
            pages,
            sealed,
        })
        .collect();
    let change = store.replace_tail(was, &windows, time::now())?;
    for window in &windows {
        let span = window.summary.window();
        stored(Stored {
            from: span.from().unwrap_or(0),
            to: span.to().unwrap_or(to),
            scrobbles: window.summary.total(),
            sealed: window.sealed,
        });
    }
    Ok(change)
}

/// Reads `[from, to)` completely, as windows from its newest end backwards,
/// and keeps them in memory.
async fn read_span(
    client: &Client,
    user: &str,
    from: u64,
    to: u64,
    options: Options,
) -> Result<Vec<(ScanSummary, Vec<ScanPage>)>, Error> {
    let mut windows = Vec::new();
    let mut end = to;
    while end > from {
        let start = window_start(client, user, from, end, options.span.max(1)).await?;
        windows.push(read(client, user, start, end, options.page_size).await?);
        end = start;
    }
    Ok(windows)
}

/// The time by Last.fm's clock, in Unix seconds: one request, for the
/// `Date` header of its answer.
///
/// A cutoff is safe only if it lies in the past, because a window that ends
/// after now is recorded as read while scrobbles can still arrive in it.
/// This machine's clock cannot vouch for that on its own, so the service's
/// is a second opinion. It is no more than that: the header says when the
/// answer was made, by whichever server made it.
///
/// # Errors
///
/// [`Error::Lastfm`] when the call fails; [`Error::Service`] when the
/// answer carries no `Date` header that can be read. There is no falling
/// back on this machine's clock, which is the one in doubt.
pub async fn server_time(client: &Client, user: &str) -> Result<u64, Error> {
    let page = client.user(user).recent_tracks().limit(1).send().await?;
    page.raw()
        .header("date")
        .and_then(time::http_date)
        .ok_or(Error::Service(
            "Last.fm's answer did not say what time it is, so the sync cannot tell how far it is \
             safe to read",
        ))
}

/// `[from, to)` as the library takes it. A lower bound of 0 is sent as no
/// lower bound, which means the same and is the plainer request.
fn window(from: u64, to: u64) -> Result<Window, Error> {
    if from == 0 {
        Ok(Window::before(to))
    } else {
        Ok(Window::new(from, to)?)
    }
}

/// Where the window that ends at `to` starts: far enough back to hold `span`
/// seconds of listening, and no further than `gap_from`. An empty remainder
/// is one window.
async fn window_start(
    client: &Client,
    user: &str,
    gap_from: u64,
    to: u64,
    span: u64,
) -> Result<u64, Error> {
    if to - gap_from <= span {
        return Ok(gap_from);
    }
    let (total, newest) = probe(client, user, gap_from, to).await?;
    if total == 0 {
        return Ok(gap_from);
    }
    // Scrobbles were counted but none came back, as could happen if the
    // service spent the one row asked for on what is playing now. Any lower
    // bound is a correct one, so take the plain one.
    let Some(newest) = newest else {
        return Ok(gap_from.max(to - span));
    };
    Ok(gap_from.max((newest + 1).saturating_sub(span)))
}

/// One request: how many scrobbles `[from, to)` holds, and the timestamp of
/// the newest.
///
/// The count decides how a sync ends, so the answer has to be the answer to
/// this question: the user, the page and the page size asked for, and
/// nothing in it that contradicts the rest, such as a scrobble next to a
/// count of none or one from outside the period. What is playing now is not
/// a scrobble and is not counted.
async fn probe(
    client: &Client,
    user: &str,
    from: u64,
    to: u64,
) -> Result<(u64, Option<u64>), Error> {
    let window = window(from, to)?;
    let page = client
        .user(user)
        .recent_tracks()
        .window(window)
        .limit(1)
        .send()
        .await?;
    let attr = page.attr();
    if !attr.user().eq_ignore_ascii_case(user) || attr.page() != 1 || attr.per_page() != 1 {
        return Err(Error::Service(
            "Last.fm answered a count with a page that was not the one asked for",
        ));
    }
    let scrobbles = page.scrobbles();
    if scrobbles
        .iter()
        .any(|scrobble| !window.contains(scrobble.timestamp()))
    {
        return Err(Error::Service(
            "Last.fm returned a scrobble from outside the period asked for",
        ));
    }
    if attr.total() == 0 && !scrobbles.is_empty() {
        return Err(Error::Service(
            "Last.fm counted no scrobbles in a period and returned one from it",
        ));
    }
    let newest = page
        .scrobbles()
        .first()
        .map(|scrobble| scrobble.timestamp());
    Ok((attr.total(), newest))
}

/// Reads `[from, to)` completely. The pages are held in memory until the
/// scan has finished, because until then they are provisional.
///
/// The pages are asked for in the plain form. With `extended=1` Last.fm
/// sends every artist's `mbid` empty, and what it adds in exchange is the
/// loved flag, which is not a fact about one scrobble.
async fn read(
    client: &Client,
    user: &str,
    from: u64,
    to: u64,
    page_size: u32,
) -> Result<(ScanSummary, Vec<ScanPage>), Error> {
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
/// every later row. So every page must report the same total, every page
/// but the last must be full, the pages must hold exactly that many tracks,
/// and no track may be listed twice. A list of more than one page has its
/// first page read again at the end, which must be as it was: a track that
/// is loved goes to the top of the list. If any of that fails nothing is
/// stored, and the copy from the last sync stays as it was.
///
/// That catches a list that grew, shrank, or lost one track and gained
/// another during the read. It is not proof. Tracks loved in the same
/// second have no order of their own, and enough changes inside one second
/// can put the first page back as it was. Only a list that fits one page is
/// read at a single moment.
///
/// A new database is tied to `user` first.
///
/// # Errors
///
/// [`Error::OtherUser`] when the database holds another user's loved
/// tracks; [`Error::Lastfm`] when a call fails; [`Error::Service`] when a
/// page is not the one asked for or the list changed while it was read;
/// [`Error::Database`] and [`Error::Sqlite`] from the store.
pub async fn loved(
    client: &Client,
    store: &mut Store,
    user: &str,
    page_size: u32,
) -> Result<u64, Error> {
    store.bind_user(user)?;
    let mut pages: Vec<Response<LovedTracksPage>> = Vec::new();
    loop {
        let page = loved_page(client, user, page_size, pages.len() as u32 + 1).await?;
        let total = page.attr().total();
        let changed = pages
            .first()
            .is_some_and(|first| first.attr().total() != total);
        let full = page.tracks().len() as u64 == u64::from(page_size);
        pages.push(page);
        let read = pages.iter().map(|page| page.tracks().len()).sum::<usize>() as u64;
        // The end is where the rows add up to the total, whatever
        // `totalPages` says. Every page before it must be full, so each
        // request brings the end nearer and a short page is a list that
        // shrank.
        let done = read >= total;
        if changed || read > total || (!done && !full) || (done && repeats(&pages)) {
            return Err(CHANGED);
        }
        if !done {
            continue;
        }
        if let [first, _, ..] = pages.as_slice() {
            let again = loved_page(client, user, page_size, 1).await?;
            if again.attr().total() != total || !rows(&again).eq(rows(first)) {
                return Err(CHANGED);
            }
        }
        store.replace_loved(&pages, time::now())?;
        return Ok(total);
    }
}

/// The loved tracks did not hold still for the read.
const CHANGED: Error = Error::Service(
    "the loved tracks changed while they were being read; the next sync reads them again",
);

/// One page of the loved tracks, checked to be the page asked for.
async fn loved_page(
    client: &Client,
    user: &str,
    page_size: u32,
    number: u32,
) -> Result<Response<LovedTracksPage>, Error> {
    let page = client
        .user(user)
        .loved_tracks()
        .limit(page_size)
        .page(number)
        .send()
        .await?;
    let attr = page.attr();
    if !attr.user().eq_ignore_ascii_case(user)
        || attr.page() != u64::from(number)
        || attr.per_page() != u64::from(page_size)
    {
        return Err(Error::Service(
            "Last.fm answered with a page of loved tracks that was not the one asked for",
        ));
    }
    Ok(page)
}

/// What a page lists, in its order: when, whose and which.
fn rows(page: &LovedTracksPage) -> impl Iterator<Item = (u64, &str, &str)> {
    page.tracks().iter().map(|loved| {
        (
            loved.loved_at(),
            loved.artist().name(),
            loved.track().name(),
        )
    })
}

/// Whether a track is listed twice: the same artist and name, whenever each
/// was loved. A track loved while the list was being read pushes a row from
/// one page onto the next, where it is read again, and a track unloved and
/// loved again comes back with a new time. The totals cannot show either
/// when another track was unloved meanwhile.
fn repeats(pages: &[Response<LovedTracksPage>]) -> bool {
    let mut seen = HashSet::new();
    pages
        .iter()
        .flat_map(|page| page.tracks())
        .any(|loved| !seen.insert((loved.artist().name(), loved.track().name())))
}
