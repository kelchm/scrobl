//! [`Rows`]: every row of a paged list, read page after page.

use std::fmt;
use std::vec::IntoIter;

use super::query::Ask;
use crate::error::Error;
use crate::model::Page;
use crate::protocol::Raw;

/// Every row of a list the service pages, in the service's order. Made by
/// `all()` on a paged query, such as
/// [`UserTopArtistsQuery::all`](super::UserTopArtistsQuery::all).
///
/// It sends nothing until the first [`next`](Self::next), and then asks
/// for a page only when the rows of the last one have been given out. Each
/// request goes through [`Client::call`](crate::Client::call), so it is
/// paced and retried like any read.
///
/// ```no_run
/// # async fn demo(client: scrobl::Client) -> Result<(), scrobl::Error> {
/// let mut rows = client.user("rj").top_artists().limit(200).all();
/// while let Some(row) = rows.next().await? {
///     println!("{} ({})", row.artist().name(), row.playcount());
/// }
/// # Ok(())
/// # }
/// ```
///
/// # What it does not check
///
/// Nothing ties one page to the next. If the list changes while it is
/// read, a row can come twice or be missed, and nothing here says so. To
/// read a history, where that matters, use
/// [`scan`](super::UserRecentTracksQuery::scan), which checks every page
/// against the others.
///
/// It is not a `Stream`, so that the library needs no crate for the trait:
/// `next` in a `while let` loop is the whole interface.
pub struct Rows<T> {
    ask: Ask,
    decode: fn(&Raw) -> Result<Page<T>, Error>,
    /// The page to ask for when `ready` runs out. `None` once the last
    /// page has been read.
    next_page: Option<u32>,
    /// The rows of the last page read that have not been given out.
    ready: IntoIter<T>,
    total: Option<u64>,
}

impl<T> Rows<T> {
    pub(super) fn new(ask: Ask, decode: fn(&Raw) -> Result<Page<T>, Error>) -> Self {
        let first = ask.page.unwrap_or(1);
        Self {
            ask,
            decode,
            next_page: Some(first),
            ready: Vec::new().into_iter(),
            total: None,
        }
    }

    /// The next row, or `None` once the last page has been given out.
    ///
    /// # Errors
    ///
    /// Any error of the query's `send`. An error leaves the rows where
    /// they were: calling `next` again asks for the same page again, and no
    /// row is skipped or repeated because of it.
    ///
    /// # Cancel safety
    ///
    /// Dropping the future is safe: no row is lost, and the next call asks
    /// for the same page.
    pub async fn next(&mut self) -> Result<Option<T>, Error> {
        loop {
            if let Some(row) = self.ready.next() {
                return Ok(Some(row));
            }
            let Some(number) = self.next_page else {
                return Ok(None);
            };
            self.ask.page = Some(number);
            let page = self.ask.send(self.decode).await?.into_value();
            self.total = Some(page.total());
            // Counted from the page asked for, not from the one the reply
            // says it is: some methods answer a page past the end with the
            // first page again, and following that would never end.
            self.next_page = (number < page.total_pages()).then(|| number + 1);
            self.ready = page.into_items().into_iter();
        }
    }

    /// How many rows the service says the whole list has, by the last page
    /// read. `None` before the first.
    pub fn total(&self) -> Option<u64> {
        self.total
    }
}

/// Shows where the read stands and no row.
impl<T> fmt::Debug for Rows<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Rows")
            .field("next_page", &self.next_page)
            .field("ready", &self.ready.len())
            .field("total", &self.total)
            .finish_non_exhaustive()
    }
}
