//! What every typed read is made of: the request so far, the way it is
//! sent, and the macro that writes a query type around the two.

use super::{Client, Period};
use crate::error::Error;
use crate::history::Window;
use crate::protocol::{MethodSpec, Raw, Request};
use crate::response::Response;

/// A typed read being put together: the method, who or what it is about,
/// and the options set so far.
#[derive(Debug, Clone)]
pub(super) struct Ask {
    client: Client,
    spec: &'static MethodSpec,
    /// What the handle set: the user, the tag, the country.
    about: Vec<(&'static str, String)>,
    /// The largest `limit` the client lets through, and what it says of a
    /// larger one. Most methods have none: the service refuses for itself.
    most: Option<(u32, &'static str)>,
    /// The span of time a chart is asked for.
    window: Option<Window>,
    pub(super) lang: Option<String>,
    pub(super) period: Option<Period>,
    pub(super) limit: Option<u32>,
    pub(super) page: Option<u32>,
}

impl Ask {
    pub(super) fn new(client: &Client, spec: &'static MethodSpec) -> Self {
        Self {
            client: client.clone(),
            spec,
            about: Vec::new(),
            most: None,
            window: None,
            lang: None,
            period: None,
            limit: None,
            page: None,
        }
    }

    /// Adds a parameter that says who or what the call is about.
    pub(super) fn about(mut self, name: &'static str, value: impl Into<String>) -> Self {
        self.about.push((name, value.into()));
        self
    }

    /// Refuses a `limit` over `most` before anything is sent, saying
    /// `problem`.
    pub(super) fn at_most(mut self, most: u32, problem: &'static str) -> Self {
        self.most = Some((most, problem));
        self
    }

    /// Asks for `window`, which must have both its ends.
    pub(super) fn within(mut self, window: Window) -> Self {
        self.window = Some(window);
        self
    }

    fn request(&self) -> Result<Request, Error> {
        let refuse = |problem| Err(Error::invalid_request(self.spec, problem));
        // The service refuses a 0 too, but its answer is error 6 with the
        // reason in a message this library never prints.
        match (self.limit, self.most) {
            (Some(0), None) => return refuse("`limit` starts at 1"),
            (Some(limit), Some((most, problem))) if limit == 0 || limit > most => {
                return refuse(problem);
            }
            _ => {}
        }
        if self.page == Some(0) {
            return refuse("`page` starts at 1");
        }
        let mut request = Request::new(self.spec);
        for (name, value) in &self.about {
            request = request.param(name, value);
        }
        if let Some(lang) = &self.lang {
            request = request.param("lang", lang);
        }
        if let Some(period) = self.period {
            request = request.param("period", period.as_str());
        }
        if let Some(limit) = self.limit {
            request = request.param("limit", limit);
        }
        if let Some(page) = self.page {
            request = request.param("page", page);
        }
        if let Some(window) = self.window {
            // With one end or none the service charts a span of its own
            // choosing, which is not what was asked.
            let (Some(from), Some(to)) = (window.from(), window.to()) else {
                return refuse("the window needs a start and an end");
            };
            request = request.param("from", from).param("to", to);
        }
        Ok(request)
    }

    /// Sends the request and decodes the answer.
    pub(super) async fn send<T>(
        &self,
        decode: fn(&Raw) -> Result<T, Error>,
    ) -> Result<Response<T>, Error> {
        let raw = self.client.call(&self.request()?).await?;
        let value = decode(&raw)?;
        Ok(Response::new(value, raw))
    }
}

/// Writes a query type: a struct around an [`Ask`], a setter for each
/// option named, and `send`.
///
/// ```ignore
/// query! {
///     /// What the query reads.
///     UserTopArtistsQuery => Page<UserArtist> = decode::user_get_top_artists;
///     period, limit = "Sets `limit`, the page size, from 1 to 1000.", page
/// }
/// ```
macro_rules! query {
    (
        $(#[$doc:meta])*
        $name:ident => $out:ty = $decode:path;
        $($option:ident $(= $about:literal)?),*
    ) => {
        $(#[$doc])*
        #[derive(Debug, Clone)]
        #[must_use = "a query does nothing until it is sent"]
        pub struct $name {
            ask: $crate::client::query::Ask,
        }

        impl $name {
            pub(super) fn new(ask: $crate::client::query::Ask) -> Self {
                Self { ask }
            }

            $($crate::client::query::query!(@option $option $(= $about)?);)*

            /// Makes the call. The exact response is on the result, next
            /// to the typed value.
            ///
            /// # Errors
            ///
            /// [`ErrorKind::InvalidRequest`](crate::ErrorKind), before
            /// anything is sent, for an option out of range, such as a
            /// `limit` or a `page` of 0. Any error of
            /// [`Client::call`](crate::Client::call), which is where a
            /// value the service refuses arrives, as
            /// [`INVALID_PARAMETERS`](crate::ApiErrorCode::INVALID_PARAMETERS).
            /// [`ErrorKind::Decode`](crate::ErrorKind) when the reply is
            /// not the expected shape.
            pub async fn send(
                self,
            ) -> Result<$crate::Response<$out>, $crate::Error> {
                self.ask.send($decode).await
            }
        }
    };
    (@option limit = $about:literal) => {
        #[doc = $about]
        pub fn limit(mut self, limit: u32) -> Self {
            self.ask.limit = Some(limit);
            self
        }
    };
    (@option page) => {
        $crate::client::query::query!(@option page = "Sets `page`, from 1.");
    };
    (@option page = $about:literal) => {
        #[doc = $about]
        pub fn page(mut self, page: u32) -> Self {
            self.ask.page = Some(page);
            self
        }
    };
    (@option period) => {
        /// Sets `period`, the span of time to count plays in. The service
        /// counts all of the user's history when none is sent.
        pub fn period(mut self, period: $crate::client::Period) -> Self {
            self.ask.period = Some(period);
            self
        }
    };
    (@option lang) => {
        /// Sets `lang`, the language to give the description in, as an ISO
        /// 639 code of two letters such as `de`.
        pub fn lang(mut self, lang: impl Into<String>) -> Self {
            self.ask.lang = Some(lang.into());
            self
        }
    };
}

pub(super) use query;
