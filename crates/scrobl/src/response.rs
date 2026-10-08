//! [`Response`]: a typed value together with the exact response it came from.

use std::fmt;
use std::ops::Deref;

use crate::protocol::Raw;

/// A typed view of a response, together with the response itself.
///
/// Every typed call of the client returns one, such as
/// `client.user("rj").recent_tracks().send()`. The value is decoded from the
/// response and never replaces it: the exact status, headers and body bytes
/// stay reachable through [`raw`](Self::raw), so a caller can archive what
/// Last.fm sent, or read a field the typed view does not model.
///
/// A `Response<T>` dereferences to `T`, so the typed accessors are called
/// on it directly.
///
/// ```
/// use scrobl::Response;
/// use scrobl::model::RecentTracksPage;
///
/// fn inspect(page: Response<RecentTracksPage>) {
///     // `page` derefs to the typed `RecentTracksPage`.
///     let rows = page.scrobbles().len();
///
///     // The exact bytes Last.fm sent.
///     let bytes = page.raw().body().len();
///
///     // Or take the two apart.
///     let (typed, raw) = page.into_parts();
///     # let _ = (rows, bytes, typed, raw);
/// }
/// ```
///
/// `Debug` shows the value's `Debug` and leaves the body out, as [`Raw`]
/// does.
#[derive(Clone)]
pub struct Response<T> {
    value: T,
    raw: Raw,
}

impl<T> Response<T> {
    /// Pairs a value with the response it was decoded from. The client does
    /// this for every typed call; it is public for callers who run
    /// [`protocol::decode`](crate::protocol::decode) themselves.
    pub fn new(value: T, raw: Raw) -> Self {
        Self { value, raw }
    }

    /// The response exactly as received.
    pub fn raw(&self) -> &Raw {
        &self.raw
    }

    /// Splits the response into the typed value and the exact response.
    pub fn into_parts(self) -> (T, Raw) {
        (self.value, self.raw)
    }
}

impl<T> Deref for Response<T> {
    type Target = T;

    fn deref(&self) -> &T {
        &self.value
    }
}

impl<T: fmt::Debug> fmt::Debug for Response<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Response")
            .field("value", &self.value)
            .field("raw", &self.raw)
            .finish()
    }
}
