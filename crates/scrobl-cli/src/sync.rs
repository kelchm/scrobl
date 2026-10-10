//! Reading the history the database does not hold yet.
//!
//! A sync has a *cutoff*, fixed when it starts, and makes the database cover
//! `[0, cutoff)`. It asks the [`Store`] which spans are not covered and
//! reads each one from its newest end backwards, a window at a time. Every
//! window is one validated scan, stored in one transaction once the scan
//! has finished. A sync that stops, for any reason, has lost at most the
//! window it was reading, and the next sync carries on from there.
//!
//! Windows are sized by time, [`Options::span`] at most, and placed by where
//! the scrobbles are: before each one the sync asks Last.fm for the newest
//! scrobble left in the span, so years of silence cost one request, not one
//! per month.
//!
//! At the end the sync asks Last.fm how many scrobbles lie before the
//! cutoff and puts that next to what the database holds. The two differ when
//! scrobbles were added to or removed from a period after it was read. This
//! version reports that; it does not read a period twice.

use scrobl::Client;
use scrobl::history::{ScanPage, ScanSummary, Window};

use crate::Error;
use crate::store::Store;
use crate::time;

/// How a sync divides the history.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub struct Options {
    /// The longest stretch of listening one window holds, in seconds.
    pub span: u64,
    /// The rows asked for on every page of a scan, 1 to 200.
    pub page_size: u32,
}

impl Default for Options {
    /// Windows of thirty days and pages of 200, the most the service gives.
    fn default() -> Self {
        Self {
            span: 30 * 86_400,
            page_size: 200,
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
}

/// What a sync did.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Report {
    /// The database now covers `[0, cutoff)`.
    pub cutoff: u64,
    /// Scrobbles this sync stored.
    pub added: u64,
    /// Scrobbles in the database before the cutoff.
    pub stored: u64,
    /// Scrobbles Last.fm reported before the cutoff, asked after the last
    /// window was stored.
    pub reported: u64,
}

impl Report {
    /// Whether the database holds as many scrobbles as Last.fm reports. Equal
    /// counts do not prove equal rows.
    pub fn matches(&self) -> bool {
        self.stored == self.reported
    }
}

/// Makes `store` cover `user`'s history before `cutoff`, then compares the
/// counts. `stored` is called after each window is committed.
///
/// # Errors
///
/// [`Error::Lastfm`] when a call fails after the client's retries or a page
/// breaks a rule of the scan; [`Error::Service`] when an answer cannot be
/// acted on; [`Error::Database`] and [`Error::Sqlite`] from the store. The
/// windows stored before the error stay stored.
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
    let mut added = 0;
    for (gap_from, gap_to) in store.gaps(cutoff)? {
        let mut to = gap_to;
        while to > gap_from {
            let from = window_start(client, user, gap_from, to, options.span).await?;
            let (summary, pages) = read(client, user, from, to, options.page_size).await?;
            store.commit_window(&summary, &pages, time::now())?;
            added += summary.total();
            stored(Stored {
                from,
                to,
                scrobbles: summary.total(),
            });
            to = from;
        }
    }

    let reported = probe(client, user, 0, cutoff).await?.0;
    Ok(Report {
        cutoff,
        added,
        stored: store.scrobbles_before(cutoff)?,
        reported,
    })
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
    let newest = newest.ok_or(Error::Service(
        "Last.fm counted scrobbles in a period but returned none",
    ))?;
    if newest < gap_from || newest >= to {
        return Err(Error::Service(
            "Last.fm returned a scrobble from outside the period asked for",
        ));
    }
    Ok(gap_from.max((newest + 1).saturating_sub(span)))
}

/// One request: how many scrobbles `[from, to)` holds, and the timestamp of
/// the newest.
async fn probe(
    client: &Client,
    user: &str,
    from: u64,
    to: u64,
) -> Result<(u64, Option<u64>), Error> {
    let page = client
        .user(user)
        .recent_tracks()
        .window(window(from, to)?)
        .limit(1)
        .send()
        .await?;
    let newest = page
        .scrobbles()
        .first()
        .map(|scrobble| scrobble.timestamp());
    Ok((page.attr().total(), newest))
}

/// Reads `[from, to)` completely. The pages are held in memory until the
/// scan has finished, because until then they are provisional.
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
        .extended(true)
        .scan()?;
    let mut pages = Vec::new();
    while let Some(page) = scan.next_page().await? {
        pages.push(page);
    }
    Ok((scan.finish()?, pages))
}
