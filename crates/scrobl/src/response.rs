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
/// on it directly. A method of `Response` itself wins over one of `T` with
/// the same name; [`value`](Self::value) and [`into_value`](Self::into_value)
/// reach the typed value without going through `Deref` when that matters.
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
/// `Debug` shows the value's `Debug` and leaves the body and the header
/// values out, as [`Raw`] does.
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

    /// The typed value, explicitly.
    ///
    /// The same value `Deref` reaches, for when a method of `T` is named like
    /// one of `Response` (`raw`, `value`, `into_parts`) and would otherwise be
    /// shadowed: `response.value().raw()` calls `T::raw`.
    pub fn value(&self) -> &T {
        &self.value
    }

    /// Takes the typed value and drops the response it came from. Use
    /// [`into_parts`](Self::into_parts) to keep both.
    pub fn into_value(self) -> T {
        self.value
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

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]

    use super::*;
    use crate::protocol::{HttpResponse, Request, decode, methods};

    /// A value with methods named like the ones on `Response`.
    struct Shadowing;

    impl Shadowing {
        fn raw(&self) -> &'static str {
            "the value's own `raw`"
        }

        fn value(&self) -> &'static str {
            "the value's own `value`"
        }
    }

    fn response() -> Response<Shadowing> {
        let request = Request::new(&methods::USER_GET_INFO).param("user", "rj");
        let raw = decode(&request, HttpResponse::new(200, "{}")).unwrap();
        Response::new(Shadowing, raw)
    }

    #[test]
    fn a_method_of_the_response_shadows_the_values_but_value_reaches_it() {
        let response = response();
        // `Response::raw`, not `Shadowing::raw`.
        assert_eq!(response.raw().status(), 200);
        assert_eq!(response.value().raw(), "the value's own `raw`");
        assert_eq!(response.value().value(), "the value's own `value`");
        assert_eq!(response.into_value().raw(), "the value's own `raw`");
    }
}
