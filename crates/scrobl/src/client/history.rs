//! Typed history access: a user's recent tracks, one page or a whole window.

use super::Client;
use crate::error::Error;
use crate::history::{RecentTracks, ScanPage, ScanSummary, Window, WindowScan};
use crate::model::RecentTracksPage;
use crate::response::Response;

/// One user's history, to read with typed methods. Made by [`Client::user`].
///
/// The handle owns a clone of the client, so it can be moved into a task.
#[derive(Debug, Clone)]
pub struct User {
    client: Client,
    name: String,
}

impl User {
    pub(super) fn new(client: Client, name: String) -> Self {
        Self { client, name }
    }

    /// The user name as given.
    pub fn name(&self) -> &str {
        &self.name
    }

    /// Starts a `user.getRecentTracks` query for this user.
    pub fn recent_tracks(&self) -> RecentTracksQuery {
        RecentTracksQuery {
            client: self.client.clone(),
            user: self.name.clone(),
            window: None,
            limit: None,
            page: None,
            extended: false,
            as_user: false,
        }
    }
}

/// A `user.getRecentTracks` query. Made by [`User::recent_tracks`].
///
/// [`send`](Self::send) reads one page. [`scan`](Self::scan) reads a whole
/// window, page after page, and checks every page.
///
/// ```no_run
/// # async fn demo(client: scrobl::Client) -> Result<(), scrobl::Error> {
/// let page = client.user("rj").recent_tracks().limit(50).send().await?;
/// for scrobble in page.scrobbles() {
///     println!("{}", scrobble.track().name());
/// }
/// # Ok(())
/// # }
/// ```
#[derive(Debug, Clone)]
#[must_use = "a query does nothing until it is sent or scanned"]
pub struct RecentTracksQuery {
    client: Client,
    user: String,
    window: Option<Window>,
    limit: Option<u32>,
    page: Option<u32>,
    extended: bool,
    as_user: bool,
}

impl RecentTracksQuery {
    /// Restricts the query to a window: `from` inclusive, `to` exclusive.
    /// Applies to a scan too; a scan without one reads
    /// [`Window::ALL`].
    pub fn window(mut self, window: Window) -> Self {
        self.window = Some(window);
        self
    }

    /// The page size, 1 to 200. Checked by [`send`](Self::send) and
    /// [`scan`](Self::scan). A scan uses it as the page size of every
    /// request, 200 when unset.
    pub fn limit(mut self, limit: u32) -> Self {
        self.limit = Some(limit);
        self
    }

    /// The page to read, counting from 1. Checked by [`send`](Self::send).
    /// A scan chooses its own pages, so [`scan`](Self::scan) refuses a query
    /// that set one.
    pub fn page(mut self, page: u32) -> Self {
        self.page = Some(page);
        self
    }

    /// Asks for the extended row shape: artist details and the loved flag.
    /// Applies to a scan too.
    pub fn extended(mut self, extended: bool) -> Self {
        self.extended = extended;
        self
    }

    /// Makes the call as the session's user, which is how a hidden history
    /// is read. Needs a client with a secret and a session. Applies to a
    /// scan too. See [`Request::as_user`](crate::protocol::Request::as_user).
    pub fn as_user(mut self) -> Self {
        self.as_user = true;
        self
    }

    /// Reads one page.
    ///
    /// The page is decoded but not checked against any other page; use
    /// [`scan`](Self::scan) to read a window completely. The exact response
    /// is on the result, next to the typed page.
    ///
    /// # Errors
    ///
    /// [`ErrorKind::InvalidRequest`](crate::ErrorKind) for a `limit` outside
    /// 1 to 200 or a `page` of 0, before anything is sent; any error of
    /// [`Client::call`]; and [`ErrorKind::Decode`](crate::ErrorKind) when the
    /// page is not the expected shape.
    pub async fn send(self) -> Result<Response<RecentTracksPage>, Error> {
        let (client, query) = self.into_core();
        let raw = client.call(&query.request()?).await?;
        let page = RecentTracksPage::decode(&raw)?;
        Ok(Response::new(page, raw))
    }

    /// Starts reading the whole window, page after page.
    ///
    /// The scan takes the window ([`Window::ALL`] when unset), the page size
    /// ([`limit`](Self::limit), 200 when unset), [`extended`](Self::extended)
    /// and [`as_user`](Self::as_user) of this query, and cannot be changed
    /// afterwards. Nothing is sent until the first [`Scan::next_page`].
    ///
    /// # Errors
    ///
    /// [`ErrorKind::InvalidRequest`](crate::ErrorKind), before anything is
    /// sent, for a `limit` outside 1 to 200 or when [`page`](Self::page) was
    /// set: a scan chooses its own pages.
    pub fn scan(self) -> Result<Scan, Error> {
        let (client, query) = self.into_core();
        Ok(Scan {
            client,
            scan: query.scan()?,
            failed: false,
        })
    }

    fn into_core(self) -> (Client, RecentTracks) {
        let mut query = RecentTracks::new(self.user)
            .extended(self.extended)
            .window(self.window.unwrap_or(Window::ALL));
        if let Some(limit) = self.limit {
            query = query.limit(limit);
        }
        if let Some(page) = self.page {
            query = query.page(page);
        }
        if self.as_user {
            query = query.as_user();
        }
        (self.client, query)
    }
}

/// A validated read of one window of history. Made by
/// [`RecentTracksQuery::scan`].
///
/// This is [`WindowScan`] driven by a [`Client`]: each
/// [`next_page`](Self::next_page) sends the outstanding request through
/// [`Client::call`], so it is paced and retried like any read, and hands the
/// response to the scan, which checks every rule of the design before it
/// yields the page. Read [`WindowScan`] for the rules.
///
/// ```no_run
/// # use scrobl::history::Window;
/// # async fn demo(client: scrobl::Client, window: Window) -> Result<(), scrobl::Error> {
/// let mut scan = client
///     .user("rj")
///     .recent_tracks()
///     .window(window)
///     .limit(100)
///     .scan()?;
/// while let Some(page) = scan.next_page().await? {
///     // Store `page.raw().body()` before asking for the next page.
/// #   let _ = page;
/// }
/// let summary = scan.finish()?;
/// # let _ = summary;
/// # Ok(())
/// # }
/// ```
///
/// # Errors during a scan
///
/// There are two kinds of failure, and they differ in what happens next.
///
/// - **The transport or the API failed.** [`next_page`](Self::next_page)
///   returns the error from [`Client::call`] after its retries, and the same
///   page stays outstanding. Calling `next_page` again sends that request
///   again, with the same bounds and the same page size, so a scan can ride
///   out an outage without skipping or repeating a page. A login-required
///   answer (code 17) is this kind: it is an error, never an empty window.
/// - **A page broke a rule.** The scan is dead. Its error is
///   [`ErrorKind::Inconsistent`](crate::ErrorKind) (or
///   [`Decode`](crate::ErrorKind) if the page could not be read), every later
///   `next_page` and [`finish`](Self::finish) fails, and nothing more is
///   sent. Start a new scan to try again.
///
/// Dropping the future of `next_page` is safe: the scan has not moved, and
/// the next call sends the same request.
#[derive(Debug)]
pub struct Scan {
    client: Client,
    scan: WindowScan,
    /// A page broke a rule. The core then offers no request, which would
    /// read as a finished scan, so the client remembers it.
    failed: bool,
}

impl Scan {
    /// The window being read.
    pub fn window(&self) -> Window {
        self.scan.window()
    }

    /// The number of rows asked for on every page, 1 to 200.
    pub fn page_size(&self) -> u32 {
        self.scan.page_size()
    }

    /// Reads the next page, or returns `None` once every page has been read.
    ///
    /// # Errors
    ///
    /// The error of [`Client::call`] when the transport or the API failed,
    /// leaving the same page outstanding; see
    /// [the type documentation](Scan#errors-during-a-scan). An error from
    /// the checks on the page ends the scan.
    pub async fn next_page(&mut self) -> Result<Option<ScanPage>, Error> {
        if self.failed {
            return Err(
                Error::inconsistent("the scan was abandoned after an earlier failure")
                    .with_method(&crate::protocol::methods::USER_GET_RECENT_TRACKS),
            );
        }
        let Some(request) = self.scan.next_request() else {
            return Ok(None);
        };
        let raw = self.client.call(&request).await?;
        match self.scan.accept(raw) {
            Ok(page) => Ok(Some(page)),
            Err(error) => {
                self.failed = true;
                Err(error)
            }
        }
    }

    /// Ends the scan and reports what it read.
    ///
    /// # Errors
    ///
    /// [`ErrorKind::Inconsistent`](crate::ErrorKind) unless every page was
    /// read and the scrobbles seen are the `total` the service reported, and
    /// always after a page broke a rule.
    pub fn finish(self) -> Result<ScanSummary, Error> {
        self.scan.finish()
    }
}
