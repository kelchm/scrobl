//! Reading a user's scrobble history: [`Window`], the [`RecentTracks`]
//! request and [`WindowScan`], a validated read of one time window.
//!
//! Everything here is I/O-free. A scan hands out requests and checks the
//! responses it is given; the caller, or the async client, moves the bytes.

use crate::error::Error;
use crate::model::{NowPlaying, PageAttr, RecentTracksPage, Scrobble};
use crate::protocol::{MethodSpec, Raw, Request, methods};

/// The largest `limit` the documentation allows for `user.getRecentTracks`.
const MAX_LIMIT: u32 = 200;

/// The first window bound refused as not being seconds: the year 5138. The
/// present in milliseconds is more than ten times this.
const MAX_TIMESTAMP: u64 = 100_000_000_000;

const SPEC: &MethodSpec = &methods::USER_GET_RECENT_TRACKS;

/// A half-open range of Unix seconds: `from` is inclusive and `to` is
/// exclusive. Either bound can be absent.
///
/// The service documents neither bound's inclusivity. The rule was seen on
/// the live service on 2026-10-08 (the `live` test repeats the check), and
/// [`WindowScan`] checks it on every page.
///
/// A `to` of 0 is an empty window. The service may read `to=0` as "no
/// bound"; a scan would then see rows outside the window and fail.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Window {
    from: Option<u64>,
    to: Option<u64>,
}

impl Window {
    /// No bounds: the whole history, up to whenever the scan runs.
    pub const ALL: Self = Self {
        from: None,
        to: None,
    };

    /// The window `[from, to)`.
    ///
    /// # Errors
    ///
    /// [`ErrorKind::InvalidRequest`](crate::ErrorKind) when `from >= to`,
    /// which would be empty.
    pub fn new(from: u64, to: u64) -> Result<Self, Error> {
        if from >= to {
            return Err(Error::invalid_request(
                SPEC,
                "the window is empty: `from` must be less than `to`",
            ));
        }
        Ok(Self {
            from: Some(from),
            to: Some(to),
        })
    }

    /// Everything at or after `from`, with no upper bound.
    ///
    /// A scan without an upper bound sees scrobbles that arrive while it
    /// runs. That usually changes `total` and fails rule 3, but not always:
    /// see [`WindowScan`], "What a completed scan does not prove". Callers
    /// who want a more stable read fix `to` first.
    pub const fn since(from: u64) -> Self {
        Self {
            from: Some(from),
            to: None,
        }
    }

    /// Everything before `to`, with no lower bound.
    pub const fn before(to: u64) -> Self {
        Self {
            from: None,
            to: Some(to),
        }
    }

    /// The inclusive lower bound.
    pub const fn from(&self) -> Option<u64> {
        self.from
    }

    /// The exclusive upper bound.
    pub const fn to(&self) -> Option<u64> {
        self.to
    }

    /// Whether `timestamp` lies in `[from, to)`.
    pub fn contains(&self, timestamp: u64) -> bool {
        self.from.is_none_or(|from| timestamp >= from) && self.to.is_none_or(|to| timestamp < to)
    }
}

/// One `user.getRecentTracks` request, built without I/O.
///
/// Bounds, page and limit are sent exactly as set. Nothing is sent that was
/// not set, except `user`. [`scan`](Self::scan) turns the query into a
/// [`WindowScan`] that reads a whole window.
///
/// ```
/// use scrobl::history::{RecentTracks, Window};
///
/// let request = RecentTracks::new("rj")
///     .window(Window::new(1_700_000_000, 1_700_086_400)?)
///     .limit(1)
///     .request()?;
/// assert_eq!(request.method(), "user.getRecentTracks");
/// # Ok::<(), scrobl::Error>(())
/// ```
#[derive(Debug, Clone)]
pub struct RecentTracks {
    user: String,
    window: Window,
    limit: Option<u32>,
    page: Option<u32>,
    extended: bool,
    as_user: bool,
}

impl RecentTracks {
    /// A request for `user`'s most recent tracks.
    pub fn new(user: impl Into<String>) -> Self {
        Self {
            user: user.into(),
            window: Window::ALL,
            limit: None,
            page: None,
            extended: false,
            as_user: false,
        }
    }

    /// Restricts the request to a window. The bounds are sent as `from` and
    /// `to`, and only when set.
    #[must_use]
    pub fn window(mut self, window: Window) -> Self {
        self.window = window;
        self
    }

    /// Sets `limit`, the page size. Checked by [`request`](Self::request):
    /// it must be 1 to 200.
    #[must_use]
    pub fn limit(mut self, limit: u32) -> Self {
        self.limit = Some(limit);
        self
    }

    /// Sets `page`, counting from 1. Checked by [`request`](Self::request).
    #[must_use]
    pub fn page(mut self, page: u32) -> Self {
        self.page = Some(page);
        self
    }

    /// Asks for the extended row shape (`extended=1`): artist details and
    /// the loved flag.
    #[must_use]
    pub fn extended(mut self, extended: bool) -> Self {
        self.extended = extended;
        self
    }

    /// Makes the call as the session's user. See [`Request::as_user`].
    #[must_use]
    pub fn as_user(mut self) -> Self {
        self.as_user = true;
        self
    }

    /// Builds the request.
    ///
    /// # Errors
    ///
    /// [`ErrorKind::InvalidRequest`](crate::ErrorKind) when `limit` is
    /// outside 1 to 200, the documented range, `page` is 0, or a window
    /// bound is 100,000,000,000 or more, which is past the year 5000 as
    /// seconds and so almost certainly milliseconds. Nothing is clamped.
    pub fn request(&self) -> Result<Request, Error> {
        self.check()?;
        if self.page == Some(0) {
            return Err(Error::invalid_request(SPEC, "`page` starts at 1"));
        }
        Ok(self.build())
    }

    /// Starts a [`WindowScan`] of this query: the same user, window,
    /// `extended` and [`as_user`](Self::as_user), with `limit` as the page
    /// size, 200 when unset. The scan cannot be changed afterwards.
    ///
    /// # Errors
    ///
    /// [`ErrorKind::InvalidRequest`](crate::ErrorKind) when `limit` is
    /// outside 1 to 200, when a window bound is too large to be seconds
    /// (see [`request`](Self::request)), or when `page` was set: a scan
    /// chooses its own pages.
    pub fn scan(mut self) -> Result<WindowScan, Error> {
        if self.page.is_some() {
            return Err(Error::invalid_request(
                SPEC,
                "a scan chooses its own pages: do not set `page`",
            ));
        }
        self.check()?;
        self.limit.get_or_insert(MAX_LIMIT);
        self.page = Some(1);
        Ok(WindowScan::start(self))
    }

    fn check(&self) -> Result<(), Error> {
        // Milliseconds are the usual mistake, and the service answers them
        // with an empty page that looks like success.
        if [self.window.from, self.window.to]
            .into_iter()
            .flatten()
            .any(|bound| bound >= MAX_TIMESTAMP)
        {
            return Err(Error::invalid_request(
                SPEC,
                "a window bound is too large to be seconds since the epoch: is it milliseconds?",
            ));
        }
        if self
            .limit
            .is_some_and(|limit| !(1..=MAX_LIMIT).contains(&limit))
        {
            return Err(Error::invalid_request(
                SPEC,
                "`limit` must be between 1 and 200",
            ));
        }
        Ok(())
    }

    /// The request for a query already known to be valid.
    fn build(&self) -> Request {
        let mut request = Request::new(SPEC).param("user", &self.user);
        if let Some(limit) = self.limit {
            request = request.param("limit", limit);
        }
        if let Some(page) = self.page {
            request = request.param("page", page);
        }
        if let Some(from) = self.window.from {
            request = request.param("from", from);
        }
        if let Some(to) = self.window.to {
            request = request.param("to", to);
        }
        if self.extended {
            request = request.param("extended", true);
        }
        if self.as_user {
            request = request.as_user();
        }
        request
    }
}

/// Where a scan stands.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum State {
    /// A request is outstanding.
    Reading,
    /// Every page was accepted.
    Done,
    /// A page broke a rule. Nothing more is issued or accepted.
    Failed,
}

/// What the first page promised, and every later page must repeat.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Totals {
    total: u64,
    total_pages: u64,
}

/// A validated read of one window of `user.getRecentTracks`.
///
/// The scan is an I/O-free state machine. The caller loops: ask for the
/// [`next_request`](Self::next_request), execute it by any means, and give
/// the response to [`accept`](Self::accept). When no request is left,
/// [`finish`](Self::finish) reports the totals or fails.
///
/// A scan is made by [`RecentTracks::scan`] and cannot be changed afterwards:
/// it has no setters, so every request it issues is the same except for its
/// `page`. It pages by page number only, under the window the caller gave and
/// a page size that never changes. It never uses a timestamp as a cursor,
/// never drops a duplicate and never reorders a row.
///
/// A response must answer the outstanding request. [`accept`](Self::accept)
/// compares [`Raw::request`] with it first, and a response to any other
/// request is refused without being decoded. Every page must then satisfy
/// these rules, and a failure is [`ErrorKind::Inconsistent`](crate::ErrorKind)
/// naming the rule:
///
/// 1. `@attr.user` is the requested user, ignoring case.
/// 2. `@attr.page` is the requested page and `@attr.perPage` the requested
///    limit.
/// 3. `@attr.total` and `@attr.totalPages` are the same on every page, and
///    `totalPages` is `ceil(total / perPage)`. When `total` is 0, one page
///    is read and `totalPages` may be 0 or 1.
/// 4. The page holds exactly the number of scrobbles the totals imply:
///    `perPage` on every page but the last, the remainder on the last. A
///    now-playing row is not a scrobble and is not counted.
/// 5. Every scrobble lies in the window, `from` inclusive and `to`
///    exclusive. Only the bounds that were set are checked.
/// 6. Timestamps never increase, within a page or from the last scrobble of
///    one page to the first of the next.
/// 7. At [`finish`](Self::finish), the scrobbles seen are `total`.
///
/// A failure of any kind, including a response that does not decode,
/// abandons the scan: `next_request` returns `None`, and `accept` and
/// `finish` fail.
///
/// # What a completed scan does not prove
///
/// Success means every page satisfied the rules against the service's own
/// totals at the time that page was read. It does not mean the rows are what
/// the window held at any one moment. In particular:
///
/// - A scrobble arriving and another being deleted between two page reads
///   leaves `total` unchanged and can shift a row across a page boundary,
///   giving one duplicate and one omission. A fixed `to` keeps arrivals at
///   the head out of the window, but not backdated additions or deletions
///   inside it.
/// - If the service changes the order of same-second rows between page
///   reads, a row can repeat and another go missing with no rule broken.
/// - A page repeated under a new page number is caught by rule 6 only if its
///   timestamps differ from the previous page's last. Listens can be
///   identical, so equal rows cannot be rejected.
/// - A coherent wrong answer cannot be detected from the response alone:
///   another user's rows under the right `@attr.user`, or rows left out with
///   the totals reduced to match.
/// - Fixed bounds are not snapshot isolation.
///
/// An application that needs more reads the window again and compares the two
/// reads. The library never deduplicates.
///
/// ```
/// use scrobl::history::{RecentTracks, Window};
/// use scrobl::protocol::{self, HttpResponse};
///
/// fn page(number: u32, rows: &str) -> String {
///     format!(
///         r##"{{"recenttracks": {{"track": [{rows}], "@attr": {{"user": "rj",
///             "page": "{number}", "perPage": "2", "totalPages": "2", "total": "3"}}}}}}"##
///     )
/// }
/// let row = |uts: u64| {
///     format!(
///         r##"{{"date": {{"uts": "{uts}"}}, "name": "t", "mbid": "",
///             "artist": {{"#text": "a", "mbid": ""}}, "album": {{"#text": "", "mbid": ""}}}}"##
///     )
/// };
/// let bodies = [
///     page(1, &format!("{}, {}", row(1_700_000_030), row(1_700_000_020))),
///     page(2, &row(1_700_000_020)),
/// ];
///
/// let mut scan = RecentTracks::new("rj")
///     .window(Window::new(1_700_000_000, 1_700_000_100)?)
///     .limit(2)
///     .scan()?;
/// let mut seen = 0;
/// for body in bodies {
///     let request = scan.next_request().expect("a page is outstanding");
///     // Execute `request` with any HTTP client. Here the reply is inline.
///     let raw = protocol::decode(&request, HttpResponse::new(200, body))?;
///     seen += scan.accept(raw)?.scrobbles().len();
/// }
/// assert!(scan.next_request().is_none());
/// let summary = scan.finish()?;
/// assert_eq!((seen, summary.total(), summary.pages()), (3, 3, 2));
/// # Ok::<(), scrobl::Error>(())
/// ```
#[derive(Debug, Clone)]
pub struct WindowScan {
    query: RecentTracks,
    state: State,
    /// The page the outstanding request is for.
    next_page: u32,
    pages_accepted: u32,
    totals: Option<Totals>,
    /// The user as the service spelled it on the first page.
    echoed_user: Option<String>,
    last_timestamp: Option<u64>,
    scrobbles_seen: u64,
}

impl WindowScan {
    /// A scan of `query`, already validated by [`RecentTracks::scan`], with
    /// its page set to 1.
    fn start(query: RecentTracks) -> Self {
        Self {
            query,
            state: State::Reading,
            next_page: 1,
            pages_accepted: 0,
            totals: None,
            echoed_user: None,
            last_timestamp: None,
            scrobbles_seen: 0,
        }
    }

    /// The window being read.
    pub fn window(&self) -> Window {
        self.query.window
    }

    /// The number of rows asked for on every page, 1 to 200.
    pub fn page_size(&self) -> u32 {
        self.query.limit.unwrap_or(MAX_LIMIT)
    }

    /// The request for the next page, or `None` once the scan is complete or
    /// has failed.
    ///
    /// Until the page is [`accept`](Self::accept)ed this returns the same
    /// request, so a transport failure can be retried without skipping or
    /// repeating a page.
    pub fn next_request(&self) -> Option<Request> {
        (self.state == State::Reading).then(|| self.query.build())
    }

    /// Validates the response to the outstanding request and yields the
    /// page.
    ///
    /// # Errors
    ///
    /// [`ErrorKind::Inconsistent`](crate::ErrorKind) when the response
    /// answers a different request than the outstanding one, or breaks a
    /// rule of the scan, and [`ErrorKind::Decode`](crate::ErrorKind) when the
    /// page does not decode. Either abandons the scan. Calling this with no
    /// request outstanding is also an error and leaves a finished scan as it
    /// was.
    pub fn accept(&mut self, raw: Raw) -> Result<ScanPage, Error> {
        match self.state {
            State::Failed => return Err(abandoned()),
            State::Done => {
                return Err(Error::invalid_request(
                    SPEC,
                    "the scan is complete: no request is outstanding",
                ));
            }
            State::Reading => {}
        }
        match self.validate(&raw) {
            Ok((page, progress)) => {
                self.commit(&page, progress);
                Ok(ScanPage {
                    raw,
                    number: progress.page,
                    page,
                })
            }
            Err(error) => {
                self.state = State::Failed;
                Err(error
                    .with_method(SPEC)
                    .with_response(raw.status(), raw.body()))
            }
        }
    }

    /// Ends the scan and reports what it read.
    ///
    /// # Errors
    ///
    /// [`ErrorKind::Inconsistent`](crate::ErrorKind) unless every page was
    /// accepted and the scrobbles seen are `total`, and always after a
    /// failure.
    pub fn finish(self) -> Result<ScanSummary, Error> {
        match self.state {
            State::Failed => return Err(abandoned()),
            State::Reading => {
                return Err(Error::inconsistent(
                    "the scan is incomplete: a page was not accepted",
                ));
            }
            State::Done => {}
        }
        let (Some(totals), Some(user)) = (self.totals, self.echoed_user) else {
            return Err(Error::inconsistent("the scan read no page"));
        };
        if self.scrobbles_seen != totals.total {
            return Err(Error::inconsistent(
                "rule 7: the scrobbles seen are not `total`",
            ));
        }
        Ok(ScanSummary {
            user,
            window: self.query.window,
            total: totals.total,
            pages: self.pages_accepted,
        })
    }

    /// Checks `raw` against the rules without changing the scan.
    fn validate(&self, raw: &Raw) -> Result<(RecentTracksPage, Progress), Error> {
        if raw.request() != &self.query.build() {
            return Err(Error::inconsistent(
                "the response answers a different request",
            ));
        }
        let page = RecentTracksPage::decode(raw)?;
        let attr = page.attr();
        let limit = u64::from(self.page_size());
        let number = self.next_page;

        // 1
        if attr.user().to_lowercase() != self.query.user.to_lowercase() {
            return Err(Error::inconsistent(
                "rule 1: the page is for a different user",
            ));
        }
        // 2
        if attr.page() != u64::from(number) {
            return Err(Error::inconsistent(
                "rule 2: `page` is not the requested page",
            ));
        }
        if attr.per_page() != limit {
            return Err(Error::inconsistent(
                "rule 2: `perPage` is not the requested limit",
            ));
        }
        // 3
        let totals = Totals {
            total: attr.total(),
            total_pages: attr.total_pages(),
        };
        if self.totals.is_some_and(|first| first != totals) {
            return Err(Error::inconsistent(
                "rule 3: `total` or `totalPages` changed during the scan",
            ));
        }
        let pages_agree = if totals.total == 0 {
            totals.total_pages <= 1
        } else {
            totals.total_pages == totals.total.div_ceil(limit)
        };
        if !pages_agree {
            return Err(Error::inconsistent(
                "rule 3: `totalPages` is not `ceil(total / perPage)`",
            ));
        }
        // A scan of an empty window is one page.
        let last_page = u32::try_from(totals.total_pages.max(1))
            .map_err(|_| Error::inconsistent("rule 3: `totalPages` is out of range"))?;

        // 4
        let count = page.scrobbles().len() as u64;
        let expected = if number < last_page {
            limit
        } else {
            totals
                .total
                .saturating_sub(limit.saturating_mul(u64::from(last_page) - 1))
        };
        if count != expected {
            return Err(Error::inconsistent(
                "rule 4: the page does not hold the scrobbles the totals imply",
            ));
        }
        // 5
        let window = self.query.window;
        if !page
            .scrobbles()
            .iter()
            .all(|s| window.contains(s.timestamp()))
        {
            return Err(Error::inconsistent(
                "rule 5: a scrobble lies outside the window",
            ));
        }
        // 6
        let mut previous = self.last_timestamp;
        for scrobble in page.scrobbles() {
            if previous.is_some_and(|before| scrobble.timestamp() > before) {
                return Err(Error::inconsistent("rule 6: timestamps increase"));
            }
            previous = Some(scrobble.timestamp());
        }

        Ok((
            page,
            Progress {
                page: number,
                last_page,
                totals,
                count,
                last_timestamp: previous,
            },
        ))
    }

    /// Records a page that passed.
    fn commit(&mut self, page: &RecentTracksPage, progress: Progress) {
        if self.echoed_user.is_none() {
            self.echoed_user = Some(page.attr().user().to_owned());
        }
        self.totals = Some(progress.totals);
        self.last_timestamp = progress.last_timestamp;
        self.scrobbles_seen += progress.count;
        self.pages_accepted += 1;
        if progress.page >= progress.last_page {
            self.state = State::Done;
        } else {
            self.next_page = progress.page + 1;
            self.query.page = Some(self.next_page);
        }
    }
}

/// What a page that passed adds to the scan.
#[derive(Debug, Clone, Copy)]
struct Progress {
    page: u32,
    last_page: u32,
    totals: Totals,
    count: u64,
    last_timestamp: Option<u64>,
}

fn abandoned() -> Error {
    Error::inconsistent("the scan was abandoned after an earlier failure")
}

/// A page that passed every rule of the scan.
///
/// `Debug` leaves out the response.
#[derive(Clone)]
pub struct ScanPage {
    raw: Raw,
    number: u32,
    page: RecentTracksPage,
}

impl ScanPage {
    /// The response exactly as received.
    pub fn raw(&self) -> &Raw {
        &self.raw
    }

    /// The page number, from 1.
    pub fn number(&self) -> u32 {
        self.number
    }

    /// The decoded page.
    pub fn page(&self) -> &RecentTracksPage {
        &self.page
    }

    /// The historical rows in response order, without the now-playing row.
    pub fn scrobbles(&self) -> &[Scrobble] {
        self.page.scrobbles()
    }

    /// The now-playing row, if the page had one.
    pub fn now_playing(&self) -> Option<&NowPlaying> {
        self.page.now_playing()
    }

    /// The paging attributes.
    pub fn attr(&self) -> &PageAttr {
        self.page.attr()
    }
}

impl std::fmt::Debug for ScanPage {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ScanPage")
            .field("number", &self.number)
            .field("page", &self.page)
            .finish()
    }
}

/// What a completed scan read.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScanSummary {
    user: String,
    window: Window,
    total: u64,
    pages: u32,
}

impl ScanSummary {
    /// The user as the service spelled it.
    pub fn user(&self) -> &str {
        &self.user
    }

    /// The window that was read.
    pub fn window(&self) -> Window {
        self.window
    }

    /// The `total` every page reported, which is also the number of
    /// scrobbles read. It is the service's own count when each page was read,
    /// not a proof that the rows are the window's contents; see
    /// [`WindowScan`].
    pub fn total(&self) -> u64 {
        self.total
    }

    /// The number of pages read. An empty window is one page.
    pub fn pages(&self) -> u32 {
        self.pages
    }
}
