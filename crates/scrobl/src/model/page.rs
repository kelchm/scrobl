//! The two shapes a list comes in: a [`Page`] of a list the service pages,
//! and a [`List`] it sends whole.

use std::fmt;
use std::ops::Deref;

/// Which page of a list, of how many, and who or what the list is about.
/// What a [`Page`] and a page of the history share.
#[derive(Clone)]
pub(super) struct Paging {
    pub(super) number: u32,
    pub(super) per_page: u32,
    pub(super) total_pages: u32,
    pub(super) total: u64,
    pub(super) subject: Option<String>,
}

impl Paging {
    /// The page after this one, unless this is the last or lies beyond it.
    pub(super) fn next_page(&self) -> Option<u32> {
        (self.number < self.total_pages).then(|| self.number + 1)
    }

    /// The place in the whole list of this page's first row, from 1.
    pub(super) fn first_place(&self) -> u64 {
        u64::from(self.number.saturating_sub(1)) * u64::from(self.per_page) + 1
    }
}

impl fmt::Debug for Paging {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Paging")
            .field("number", &self.number)
            .field("per_page", &self.per_page)
            .field("total_pages", &self.total_pages)
            .field("total", &self.total)
            .finish_non_exhaustive()
    }
}

/// One page of a list that the service pages.
///
/// A page dereferences to a slice of its rows, so `page.len()`, `page[0]`
/// and `for row in page.iter()` work, as does `for row in &page`.
///
/// ```
/// use scrobl::model::decode;
/// use scrobl::protocol::{self, HttpResponse, Request, methods};
///
/// let body = r##"{"topartists": {
///     "artist": [
///         {"name": "A", "playcount": "9", "@attr": {"rank": "3"}},
///         {"name": "B", "playcount": "7", "@attr": {"rank": "4"}}
///     ],
///     "@attr": {"user": "rj", "page": "2", "perPage": "2", "totalPages": "5", "total": "10"}
/// }}"##;
/// let request = Request::new(&methods::USER_GET_TOP_ARTISTS).param("user", "rj");
/// let raw = protocol::decode(&request, HttpResponse::new(200, body))?;
///
/// let page = decode::user_get_top_artists(&raw)?;
/// assert_eq!((page.number(), page.per_page(), page.total()), (2, 2, 10));
/// assert_eq!(page.subject(), Some("rj"));
/// assert_eq!(page.next_page(), Some(3));
/// let places: Vec<_> = page.ranked().map(|(place, row)| (place, row.artist().name())).collect();
/// assert_eq!(places, [(3, "A"), (4, "B")]);
/// # Ok::<(), scrobl::Error>(())
/// ```
#[derive(Clone)]
pub struct Page<T> {
    items: Vec<T>,
    paging: Paging,
}

impl<T> Page<T> {
    pub(super) fn new(items: Vec<T>, paging: Paging) -> Self {
        Self { items, paging }
    }

    /// The rows, in the order the service sent them.
    pub fn items(&self) -> &[T] {
        &self.items
    }

    /// The rows, to keep.
    pub fn into_items(self) -> Vec<T> {
        self.items
    }

    /// Each row with its place in the whole list, from 1.
    ///
    /// The place is counted from where the page stands in the list: the
    /// first row of page 3 at 50 to a page is 101. It does not come from
    /// the `rank` the service sends with some rows, which on some methods
    /// starts again on every page.
    pub fn ranked(&self) -> impl Iterator<Item = (u64, &T)> {
        (self.paging.first_place()..).zip(&self.items)
    }

    /// Which page this is, from 1.
    pub fn number(&self) -> u32 {
        self.paging.number
    }

    /// The page size the service applied.
    pub fn per_page(&self) -> u32 {
        self.paging.per_page
    }

    /// The number of pages the service says there are.
    pub fn total_pages(&self) -> u32 {
        self.paging.total_pages
    }

    /// The number of rows the service says the whole list has.
    pub fn total(&self) -> u64 {
        self.paging.total
    }

    /// The number of the page after this one. `None` on the last page and
    /// on any page beyond it.
    pub fn next_page(&self) -> Option<u32> {
        self.paging.next_page()
    }

    /// Who or what the list is about, as the service spells it back: the
    /// user, the tag or the country. `None` for a list about nothing in
    /// particular, such as a chart of the whole service.
    pub fn subject(&self) -> Option<&str> {
        self.paging.subject.as_deref()
    }
}

impl<T> Deref for Page<T> {
    type Target = [T];

    fn deref(&self) -> &[T] {
        &self.items
    }
}

impl<T> IntoIterator for Page<T> {
    type Item = T;
    type IntoIter = std::vec::IntoIter<T>;

    fn into_iter(self) -> Self::IntoIter {
        self.items.into_iter()
    }
}

impl<'a, T> IntoIterator for &'a Page<T> {
    type Item = &'a T;
    type IntoIter = std::slice::Iter<'a, T>;

    fn into_iter(self) -> Self::IntoIter {
        self.items.iter()
    }
}

impl<T> fmt::Debug for Page<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Page")
            .field("number", &self.paging.number)
            .field("per_page", &self.paging.per_page)
            .field("total_pages", &self.paging.total_pages)
            .field("total", &self.paging.total)
            .field("items", &self.items.len())
            .finish_non_exhaustive()
    }
}

/// A whole list, of a kind the service does not page.
///
/// A list dereferences to a slice of its rows, as a [`Page`] does.
#[derive(Clone)]
pub struct List<T> {
    items: Vec<T>,
    subject: Option<String>,
    /// How many rows of the service's list come before the first here.
    skipped: u64,
}

impl<T> List<T> {
    pub(super) fn new(items: Vec<T>, subject: Option<String>) -> Self {
        Self {
            items,
            subject,
            skipped: 0,
        }
    }

    /// The same list, as one that starts `skipped` rows into the service's
    /// own: the one method that takes an offset says how far it went.
    pub(super) fn after(mut self, skipped: u64) -> Self {
        self.skipped = skipped;
        self
    }

    /// The rows, in the order the service sent them.
    pub fn items(&self) -> &[T] {
        &self.items
    }

    /// The rows, to keep.
    pub fn into_items(self) -> Vec<T> {
        self.items
    }

    /// Each row with its place in the service's list, from 1. A list read
    /// from part of the way down, which only
    /// [`tag.getTopTags`](super::decode::tag_get_top_tags) can be, counts
    /// from where it starts.
    pub fn ranked(&self) -> impl Iterator<Item = (u64, &T)> {
        (self.skipped.saturating_add(1)..).zip(&self.items)
    }

    /// Who or what the list is about, as the service spells it back: the
    /// user or the tag. `None` for a list about nothing in particular.
    pub fn subject(&self) -> Option<&str> {
        self.subject.as_deref()
    }
}

impl<T> Deref for List<T> {
    type Target = [T];

    fn deref(&self) -> &[T] {
        &self.items
    }
}

impl<T> IntoIterator for List<T> {
    type Item = T;
    type IntoIter = std::vec::IntoIter<T>;

    fn into_iter(self) -> Self::IntoIter {
        self.items.into_iter()
    }
}

impl<'a, T> IntoIterator for &'a List<T> {
    type Item = &'a T;
    type IntoIter = std::slice::Iter<'a, T>;

    fn into_iter(self) -> Self::IntoIter {
        self.items.iter()
    }
}

impl<T> fmt::Debug for List<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("List")
            .field("items", &self.items.len())
            .finish_non_exhaustive()
    }
}
