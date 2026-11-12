//! The checked read of a window of a user's history, driven by the client.

use super::Client;
use crate::error::Error;
use crate::history::{ScanSummary, Window, WindowScan};
use crate::model::HistoryPage;
use crate::protocol::methods;
use crate::response::Response;

/// A validated read of one window of history. Made by
/// [`UserRecentTracksQuery::scan`](super::UserRecentTracksQuery::scan).
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
    pub(super) fn new(client: Client, scan: WindowScan) -> Self {
        Self {
            client,
            scan,
            failed: false,
        }
    }

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
    pub async fn next_page(&mut self) -> Result<Option<Response<HistoryPage>>, Error> {
        if self.failed {
            return Err(
                Error::inconsistent("the scan was abandoned after an earlier failure")
                    .with_method(&methods::USER_GET_RECENT_TRACKS),
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
