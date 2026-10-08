//! The async client: executes the requests the I/O-free core builds.
//!
//! This module exists with the `client` feature, which is on by default.
//!
//! [`Client`] is one concrete client over `reqwest` with rustls. It sends
//! over HTTPS only, follows no redirect, keeps no cookie, and turns off
//! `reqwest`'s own retries, so that every request that leaves is one this
//! crate chose to send. Responses are requested and kept unencoded: the client
//! sends no `Accept-Encoding` and decodes no compression, whatever other
//! crates in the program have switched on in `reqwest`, so [`Raw::body`] is
//! exactly the bytes the service sent. Use [`Client::user`] for typed history access and
//! [`Client::call`] for any of the 57 methods as a raw [`Raw`] response.
//!
//! ```no_run
//! use scrobl::history::Window;
//! use scrobl::{ApiKey, Client};
//!
//! # async fn backup() -> Result<(), scrobl::Error> {
//! let client = Client::builder(ApiKey::new("your-api-key"))
//!     .user_agent("example-backup/0.1 (+https://example.org)")
//!     .build()?;
//!
//! // One closed window, read completely or not at all.
//! let window = Window::new(1_700_000_000, 1_700_086_400)?;
//! let mut scan = client
//!     .user("rj")
//!     .recent_tracks()
//!     .window(window)
//!     .extended(true)
//!     .scan()?;
//! while let Some(page) = scan.next_page().await? {
//!     let exact_bytes = page.raw().body();
//!     for scrobble in page.scrobbles() {
//!         // `scrobble.timestamp()`, `scrobble.track().name()`, ...
//! #       let _ = (scrobble, exact_bytes);
//!     }
//! }
//! let summary = scan.finish()?; // fails unless every rule of the scan held
//! println!("{} scrobbles in {} pages", summary.total(), summary.pages());
//! # Ok(())
//! # }
//! ```
//!
//! One page, for example to learn how many scrobbles a window holds:
//!
//! ```no_run
//! # use scrobl::history::Window;
//! # async fn count(client: scrobl::Client, window: Window) -> Result<(), scrobl::Error> {
//! let page = client
//!     .user("rj")
//!     .recent_tracks()
//!     .window(window)
//!     .limit(1)
//!     .send()
//!     .await?;
//! println!("{} scrobbles", page.attr().total());
//! # Ok(())
//! # }
//! ```
//!
//! # Runtime
//!
//! The client runs on whatever Tokio runtime polls its futures, including a
//! current-thread runtime, which is how a synchronous program uses it. The
//! crate spawns no task of its own, starts no thread and owns no runtime,
//! so dropping a client or a future leaves nothing of ours running. The
//! machinery underneath does what any `reqwest` client does: its connection
//! tasks run on the calling runtime and end with the connection, and
//! resolving a host name uses that runtime's blocking pool. A client can be
//! built outside a runtime and used inside one. The runtime needs both its I/O driver and its time driver enabled
//! (`enable_all`), as `#[tokio::main]`, `#[tokio::test]` and `Runtime::new`
//! do. On a runtime built without them the first call panics inside Tokio
//! ("timers are disabled" or "IO is disabled"); the client does not hide that.
//!
//! A `Client` is `Clone + Send + Sync + 'static`, and every future it
//! returns is `Send`, so it can sit in shared application state and be used
//! from spawned tasks. A clone is an `Arc` bump; clones share one
//! connection pool and one pacing clock.
//!
//! # Pacing
//!
//! Last.fm asks applications to make no more than about one request a
//! second. The client keeps at least [`min_interval`] (one second by
//! default) between the starts of two requests, across all its clones, and a
//! retry is a request like any other.
//!
//! A start is an *admission*: the moment a caller is let through to the
//! transport, measured by this process's clock when it happens. Callers are
//! admitted one at a time, in the order they asked, and each admission waits
//! out whatever remains of the interval since the previous one. The spacing
//! therefore holds however late a task is polled: a runtime that stalls for
//! several intervals delays the callers queued behind the stall, it does not
//! release them together. Nothing is reserved ahead, so a caller whose future
//! is dropped, whether it is queued or already waiting its turn, leaves no
//! slot spent and no debt: the next caller waits only for what remains of the
//! interval since the last real admission.
//!
//! What this does not promise is the spacing of arrivals at the server.
//! Opening a connection (the lookup, the TCP and TLS handshakes) takes
//! different times for different requests, so requests admitted an interval
//! apart can arrive slightly closer or farther apart. Set the interval to
//! zero to turn pacing off; it is then not consulted at all. An interval over
//! 24 hours is refused at [`build`](ClientBuilder::build).
//!
//! # Retries
//!
//! A read is attempted up to [`read_attempts`] times (three by default,
//! counting the first). It is repeated only after a failure whose
//! [`Error::retry`] is [`Retry::Later`] or [`Retry::AfterBackoff`]: a
//! transport failure or timeout, HTTP 5xx, API codes 11 and 16, and then
//! HTTP 429 and API code 29. Nothing else is repeated: a bad parameter, a
//! suspended key, a body that is too large or one that cannot be decoded
//! will fail the same way again.
//!
//! Before retry number `n` the client waits [`retry_delay`] times 2^(n-1)
//! (five times as long after a rate-limit answer), on top of pacing, and at
//! least as long as the response's `Retry-After` header asked when it gave
//! a number of seconds. No single wait is longer than five minutes.
//!
//! **A write is never retried by the client**, whatever went wrong. A
//! write that failed without a readable answer may have happened, and
//! sending it again could scrobble twice. [`Error::delivery`] tells the
//! caller what happened: [`Delivery::NotSent`], [`Delivery::Rejected`] or
//! [`Delivery::Unknown`]. Deciding what to do with an unknown outcome is the
//! application's job, because only it knows what it can check.
//!
//! # Timeouts and the body cap
//!
//! [`connect_timeout`] (10 seconds) limits opening a connection, the TLS
//! handshake included. [`timeout`] (30 seconds) is the longest the client
//! waits for one attempt, from the start to the last body byte, so a server
//! that sends headers and then trickles or stalls is cut off. Either gives
//! [`ErrorKind::Timeout`](crate::ErrorKind).
//!
//! The timeout is a limit on waiting, not on when a future is next polled. A
//! response that has already arrived in full, and decodes, by the time the
//! future is polled is returned even if the deadline has passed in the
//! meantime: for a write, discarding it would turn a known outcome into
//! [`Delivery::Unknown`], which is strictly worse. Only an attempt still
//! waiting for the service when it is polled is cut off.
//!
//! The timeout is per attempt. A call that retries has that much for each
//! attempt, and the waits between them are outside it: so are the pacing
//! wait before an attempt and the [`retry_delay`] after one. To bound a whole
//! operation, put a deadline around it:
//!
//! ```no_run
//! use std::time::Duration;
//!
//! use scrobl::history::Window;
//!
//! # async fn deadline(client: scrobl::Client, window: Window) -> Result<(), scrobl::Error> {
//! let mut scan = client.user("rj").recent_tracks().window(window).scan()?;
//! // Five minutes for the whole window, however many pages and retries.
//! let read = tokio::time::timeout(Duration::from_secs(300), async {
//!     while let Some(page) = scan.next_page().await? {
//! #       let _ = page;
//!         // Store the page.
//!     }
//!     Ok::<_, scrobl::Error>(())
//! })
//! .await;
//! match read {
//!     Ok(result) => result?,
//!     // The deadline cancelled `next_page`. The scan has not moved: the
//!     // same page is still outstanding, so calling `next_page` again would
//!     // continue it, and nothing has been skipped or repeated.
//!     Err(_elapsed) => println!("out of time; resume later"),
//! }
//! # Ok(())
//! # }
//! ```
//!
//! A body is read up to [`max_response_bytes`] (8 MiB). A `Content-Length`
//! over the cap fails before any body is read; an unannounced or chunked
//! body fails as soon as it passes the cap. The cap bounds the length of the
//! body the client keeps, and the buffer it grows for it is never asked to
//! be larger than the cap. On top of that the transport holds the chunk it is
//! handing over, an amount bounded by its own read buffer and not by the cap.
//! Either failure is [`ErrorKind::BodyTooLarge`](crate::ErrorKind).
//!
//! # Cancellation
//!
//! Dropping a future abandons the attempt in flight and closes its
//! connection. Nothing shared is left half-updated, and a caller that was
//! waiting for its turn to be paced leaves nothing spent behind it. Dropping
//! the future of a **write** means the outcome is unknown: the request may
//! have been sent and acted on. Do not cancel a write unless you can find out
//! afterwards.
//!
//! # Errors
//!
//! Every error carries the Last.fm method, and none holds a URL or a
//! credential, in its `Display`, its `Debug` or anywhere along its
//! [`source`](std::error::Error::source) chain: a `GET` URL contains the API
//! key. A connection that could not be opened is
//! [`ErrorKind::Transport`](crate::ErrorKind) and, for a write,
//! [`Delivery::NotSent`]. Failure after the request may have left is a
//! transport failure too, with [`Delivery::Unknown`] for a write.
//!
//! HTTP 200 can carry an error: the client decodes the error envelope at
//! every status, so a login-required answer ([`ApiErrorCode::LOGIN_REQUIRED`])
//! is an error and never an empty history.
//!
//! # Proxies
//!
//! Like `reqwest`, the client honours the `HTTPS_PROXY`, `ALL_PROXY` and
//! `NO_PROXY` environment variables. Through an HTTP proxy the proxy sees a
//! `CONNECT` naming the host and port and nothing else: the request, its
//! query and the key are inside TLS. [`no_proxy`] opts out and always
//! connects directly.
//!
//! [`min_interval`]: ClientBuilder::min_interval
//! [`read_attempts`]: ClientBuilder::read_attempts
//! [`retry_delay`]: ClientBuilder::retry_delay
//! [`connect_timeout`]: ClientBuilder::connect_timeout
//! [`timeout`]: ClientBuilder::timeout
//! [`max_response_bytes`]: ClientBuilder::max_response_bytes
//! [`no_proxy`]: ClientBuilder::no_proxy
//! [`Raw::body`]: crate::protocol::Raw::body
//! [`Raw`]: crate::protocol::Raw
//! [`Retry`]: crate::Retry
//! [`Retry::Later`]: crate::Retry::Later
//! [`Retry::AfterBackoff`]: crate::Retry::AfterBackoff
//! [`Delivery`]: crate::Delivery
//! [`Delivery::NotSent`]: crate::Delivery::NotSent
//! [`Delivery::Rejected`]: crate::Delivery::Rejected
//! [`Delivery::Unknown`]: crate::Delivery::Unknown
//! [`ApiErrorCode::LOGIN_REQUIRED`]: crate::ApiErrorCode::LOGIN_REQUIRED
//! [`Error`]: crate::Error
//! [`Error::retry`]: crate::Error::retry
//! [`Error::delivery`]: crate::Error::delivery

mod builder;
mod cause;
mod history;
mod pacing;

use std::fmt;
use std::sync::Arc;
use std::time::Duration;

use reqwest::header::{CONTENT_TYPE, DATE, HeaderMap, RETRY_AFTER};
use tokio::time::sleep;

pub use builder::ClientBuilder;
pub use history::{RecentTracksQuery, Scan, User};

use crate::error::{Error, Retry};
use crate::protocol::{
    self, Credentials, HttpRequest, HttpResponse, Raw, Request, Verb, prepare_with_root,
};
use crate::secret::{ApiKey, SessionKey};
use builder::Settings;
use cause::{Cause, Redactor};
use pacing::Pacer;

/// The longest the client waits before a retry, however it was worked out.
const MAX_WAIT: Duration = Duration::from_secs(300);

/// A rate-limit answer waits this many times as long as other retries.
const BACKOFF_FACTOR: u32 = 5;

/// The most body space reserved before any body byte has arrived.
const FIRST_RESERVE: usize = 64 * 1024;

/// An async client for the Last.fm API.
///
/// Build one with [`Client::builder`], keep it for the life of the program
/// and clone it freely: a clone shares the connection pool and the pacing
/// clock. See the [module documentation](self) for the runtime it needs and
/// how it paces, retries, times out and caps bodies.
///
/// `Debug` shows the settings and never a credential.
#[derive(Clone)]
pub struct Client {
    shared: Arc<Shared>,
    credentials: Arc<Credentials>,
}

/// What every clone of a client shares.
struct Shared {
    http: reqwest::Client,
    /// `None` is the real service.
    root: Option<String>,
    settings: Settings,
    pacer: Pacer,
}

impl Client {
    /// Starts configuring a client that signs in with `api_key`.
    pub fn builder(api_key: ApiKey) -> ClientBuilder {
        ClientBuilder::new(api_key)
    }

    /// A client that acts for the user of `session`.
    ///
    /// Cheap: it shares this client's connection pool and pacing clock, so
    /// one program serving several users still makes one request a second.
    /// This client is unchanged.
    #[must_use]
    pub fn with_session(&self, session: SessionKey) -> Client {
        Client {
            shared: Arc::clone(&self.shared),
            credentials: Arc::new((*self.credentials).clone().with_session(session)),
        }
    }

    /// Makes a call to any Last.fm method and returns the exact response.
    ///
    /// Build the [`Request`] with [`protocol::methods`]; the client adds
    /// `method`, `api_key`, `format`, and `sk` and `api_sig` where the
    /// method needs them. The caller needs no credential of its own.
    ///
    /// One attempt waits for its turn to be paced, sends the request, reads the body up
    /// to the cap and decodes it with [`protocol::decode`]. A read that
    /// fails in a way that is worth repeating is attempted again, up to the
    /// configured number of attempts. A write is attempted once. See the
    /// [module documentation](self) for the details.
    ///
    /// ```no_run
    /// use scrobl::protocol::{Request, methods};
    ///
    /// # async fn demo(client: scrobl::Client) -> Result<(), scrobl::Error> {
    /// let request = Request::new(&methods::USER_GET_INFO).param("user", "rj");
    /// let raw = client.call(&request).await?;
    /// println!("{}", String::from_utf8_lossy(raw.body()));
    /// # Ok(())
    /// # }
    /// ```
    ///
    /// # Errors
    ///
    /// [`ErrorKind::InvalidRequest`](crate::ErrorKind) when the request
    /// cannot be built (nothing is sent). Otherwise the error of the last
    /// attempt: [`Transport`](crate::ErrorKind), [`Timeout`](crate::ErrorKind),
    /// [`BodyTooLarge`](crate::ErrorKind), [`Http`](crate::ErrorKind),
    /// [`Api`](crate::ErrorKind) or [`Decode`](crate::ErrorKind). For a
    /// write, [`Error::delivery`] says whether it can have happened.
    ///
    /// # Cancel safety
    ///
    /// Dropping the future abandons the attempt and leaves the client
    /// untouched. **For a write, a dropped future means the delivery is
    /// unknown**: the request may already have been sent and acted on.
    pub async fn call(&self, request: &Request) -> Result<Raw, Error> {
        let shared = &self.shared;
        let http = match &shared.root {
            None => protocol::prepare(&self.credentials, request),
            Some(root) => prepare_with_root(root, &self.credentials, request),
        }?;

        // The client never repeats a write.
        let attempts = if request.spec().write {
            1
        } else {
            shared.settings.read_attempts
        };
        let mut attempt = 1;
        loop {
            shared.pacer.wait().await;
            let outcome = self.attempt(request, &http).await;
            let error = match outcome.result {
                Ok(raw) => return Ok(raw),
                Err(error) => error,
            };
            if attempt >= attempts {
                return Err(error);
            }
            let factor = match error.retry() {
                Retry::Later => 1,
                Retry::AfterBackoff => BACKOFF_FACTOR,
                _ => return Err(error),
            };
            let wait = retry_wait(
                shared.settings.retry_delay,
                attempt,
                factor,
                outcome.retry_after,
            );
            sleep(wait).await;
            attempt += 1;
        }
    }

    /// A handle for `name`'s history, to read it with typed methods.
    ///
    /// The handle is a cheap value that holds a clone of this client.
    pub fn user(&self, name: impl Into<String>) -> User {
        User::new(self.clone(), name.into())
    }

    /// One attempt: send, read the body, decode. It waits no longer than the
    /// configured timeout for the service. A response that is complete when
    /// the future is next polled is returned even if the deadline has passed,
    /// because `timeout` polls the exchange before its timer.
    async fn attempt(&self, request: &Request, http: &HttpRequest) -> Attempt {
        let mut retry_after = None;
        let exchange = self.exchange(request, http, &mut retry_after);
        let result = match tokio::time::timeout(self.shared.settings.timeout, exchange).await {
            Ok(result) => result,
            Err(_) => Err(Error::timeout()),
        };
        Attempt {
            result: result.map_err(|error| error.with_method(request.spec())),
            retry_after,
        }
    }

    /// What a message about `http` must not repeat.
    fn redactor(&self, http: &HttpRequest) -> Redactor {
        Redactor::for_request(&self.credentials, http)
    }

    async fn exchange(
        &self,
        request: &Request,
        http: &HttpRequest,
        retry_after: &mut Option<Duration>,
    ) -> Result<Raw, Error> {
        let limit = self.shared.settings.max_response_bytes;
        let too_large = || Error::body_too_large(limit);

        let builder = match http.verb() {
            Verb::Get => self.shared.http.get(http.url()),
            Verb::Post => self
                .shared
                .http
                .post(http.url())
                .header(CONTENT_TYPE, HttpRequest::CONTENT_TYPE)
                .body(http.body().unwrap_or_default().to_vec()),
        };
        let mut response = builder
            .send()
            .await
            .map_err(|e| send_error(e, &self.redactor(http)))?;

        let status = response.status().as_u16();
        let headers = response.headers().clone();
        *retry_after = parse_retry_after(&headers);

        let announced = response.content_length();
        if announced.is_some_and(|len| len > limit as u64) {
            return Err(too_large());
        }
        let reserve = announced.map_or(0, |len| usize::try_from(len).unwrap_or(limit));
        let mut body = Vec::with_capacity(reserve.min(FIRST_RESERVE));
        while let Some(chunk) = response
            .chunk()
            .await
            .map_err(|e| body_error(e, &self.redactor(http)))?
        {
            if !push_chunk(&mut body, &chunk, limit) {
                return Err(too_large());
            }
        }

        let mut reply = HttpResponse::new(status, body);
        for name in [CONTENT_TYPE, RETRY_AFTER, DATE] {
            if let Some(value) = headers.get(&name).and_then(|v| v.to_str().ok()) {
                reply = reply.with_header(name.as_str(), value);
            }
        }
        protocol::decode(request, reply)
    }
}

impl fmt::Debug for Client {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Client")
            .field("credentials", &self.credentials)
            .field("settings", &self.shared.settings)
            .field("base_url", &self.shared.root.is_some())
            .finish_non_exhaustive()
    }
}

/// What one attempt came to.
struct Attempt {
    result: Result<Raw, Error>,
    /// The wait the response asked for, whatever the result.
    retry_after: Option<Duration>,
}

/// Appends `chunk` to `body` unless that would make it longer than `limit`,
/// and says whether it did.
///
/// The capacity is grown by doubling but never asked past `limit`, so a body
/// that ends near the cap is not held in a buffer that doubled beyond it.
fn push_chunk(body: &mut Vec<u8>, chunk: &[u8], limit: usize) -> bool {
    if chunk.len() > limit.saturating_sub(body.len()) {
        return false;
    }
    let needed = body.len() + chunk.len();
    if needed > body.capacity() {
        let target = body.capacity().saturating_mul(2).max(needed).min(limit);
        body.reserve_exact(target - body.len());
    }
    body.extend_from_slice(chunk);
    true
}

/// A failure to send, or to receive the headers.
///
/// The URL is dropped from the error first: it holds the API key. The cause
/// kept is a snapshot of the messages, scrubbed of the credentials and of the
/// request, because the `Debug` of some of the errors underneath prints the
/// address connected to, and a message may repeat what a peer sent.
fn send_error(error: reqwest::Error, redactor: &Redactor) -> Error {
    let error = error.without_url();
    if error.is_timeout() {
        Error::timeout()
    } else {
        // Anything but a failure to connect may have left the machine.
        let possibly_sent = !(error.is_connect() || error.is_builder());
        Error::transport(possibly_sent, Cause::of(&error, redactor))
    }
}

/// A failure while the body was coming in. The request was sent.
fn body_error(error: reqwest::Error, redactor: &Redactor) -> Error {
    let error = error.without_url();
    if error.is_timeout() {
        Error::timeout()
    } else {
        Error::transport(true, Cause::of(&error, redactor))
    }
}

/// How long to wait before retry number `attempt` (counting from 1):
/// `base * 2^(attempt - 1) * factor`, or what the response asked for if
/// that is longer, and never more than [`MAX_WAIT`].
fn retry_wait(base: Duration, attempt: u32, factor: u32, asked: Option<Duration>) -> Duration {
    // Double the `Duration` itself, which saturates, rather than a count that
    // would plateau short of the cap for a small base. Stops at the cap, or at
    // zero, where more doubling changes nothing.
    let mut backoff = base;
    for _ in 1..attempt {
        if backoff.is_zero() || backoff >= MAX_WAIT {
            break;
        }
        backoff = backoff.saturating_mul(2);
    }
    backoff
        .saturating_mul(factor)
        .max(asked.unwrap_or_default())
        .min(MAX_WAIT)
}

/// `Retry-After` as a number of seconds, which is the form Last.fm and most
/// rate limiters use. A date is ignored.
fn parse_retry_after(headers: &HeaderMap) -> Option<Duration> {
    let seconds = headers
        .get(RETRY_AFTER)?
        .to_str()
        .ok()?
        .trim()
        .parse::<u64>()
        .ok()?;
    Some(Duration::from_secs(seconds).min(MAX_WAIT))
}

#[cfg(test)]
mod tests;
