//! The error model: [`Error`], [`ErrorKind`], [`ApiErrorCode`] and the two
//! pieces of advice, [`Retry`] and [`Delivery`].

use std::error::Error as StdError;
use std::fmt;

use crate::protocol::MethodSpec;

/// The most response body an [`Error`] keeps.
const MAX_BODY: usize = 64 * 1024;
/// The most of a Last.fm error message an [`Error`] keeps.
const MAX_MESSAGE: usize = 1024;
/// The most of a diagnostic an [`Error`] keeps.
const MAX_DETAIL: usize = 256;

/// The one write method that must never be repeated, because a later
/// "now playing" is simply a newer one.
const UPDATE_NOW_PLAYING: &str = "track.updateNowPlaying";
const GET_SESSION: &str = "auth.getSession";

/// Cuts `s` to at most `max` bytes without splitting a character.
pub(crate) fn truncate_utf8(s: &str, max: usize) -> &str {
    if s.len() <= max {
        return s;
    }
    let mut end = max;
    while !s.is_char_boundary(end) {
        end -= 1;
    }
    s.get(..end).unwrap_or_default()
}

/// What went wrong, in broad terms.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum ErrorKind {
    /// Last.fm returned an error envelope, at any HTTP status.
    Api,
    /// A non-2xx status with no error envelope.
    Http,
    /// The body was not the expected JSON, or a field was malformed.
    Decode,
    /// A history page broke a scan rule.
    Inconsistent,
    /// Connection, TLS or I/O failure.
    Transport,
    /// A deadline passed.
    Timeout,
    /// The response exceeded the body cap.
    BodyTooLarge,
    /// The request could not be built. Nothing was sent.
    InvalidRequest,
    /// The client could not be built.
    Config,
    /// The method changes the account and the client or credentials are
    /// read-only. Nothing was signed or sent.
    ReadOnly,
}

/// What a caller can usefully do about an error. This is advice and depends
/// on the method as well as the failure.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum Retry {
    /// The same call will fail the same way, or repeating it is not safe.
    No,
    /// A transient failure. Try again shortly.
    Later,
    /// The service is rate limiting. Try again after a longer wait.
    AfterBackoff,
    /// The session key is no longer valid. Authenticate again first.
    AfterReauthentication,
}

/// What a failed write did.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum Delivery {
    /// The request certainly never left the process.
    NotSent,
    /// The service answered with an error envelope and did not act on it.
    Rejected,
    /// The request was sent, or may have been, and there is no readable
    /// answer. The write may or may not have happened.
    Unknown,
}

/// A Last.fm error code.
///
/// Codes this library does not know are preserved, so a new code is a
/// different value rather than a failure.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ApiErrorCode(u32);

impl ApiErrorCode {
    /// 2: the service is invalid or unavailable.
    pub const INVALID_SERVICE: Self = Self(2);
    /// 3: the method does not exist.
    pub const INVALID_METHOD: Self = Self(3);
    /// 4: authentication failed, as when a user revoked the application.
    pub const AUTHENTICATION_FAILED: Self = Self(4);
    /// 5: the `format` parameter is not supported.
    pub const INVALID_FORMAT: Self = Self(5);
    /// 6: a parameter is missing or invalid. Also the answer to a lookup that
    /// finds nothing.
    pub const INVALID_PARAMETERS: Self = Self(6);
    /// 7: the resource cannot give what was asked of it.
    pub const INVALID_RESOURCE: Self = Self(7);
    /// 8: something failed that Last.fm cannot describe.
    pub const OPERATION_FAILED: Self = Self(8);
    /// 9: the session key is invalid. The user must authenticate again.
    pub const INVALID_SESSION_KEY: Self = Self(9);
    /// 10: the API key is invalid.
    pub const INVALID_API_KEY: Self = Self(10);
    /// 11: the service is temporarily offline.
    pub const SERVICE_OFFLINE: Self = Self(11);
    /// 13: the request signature is wrong.
    pub const INVALID_SIGNATURE: Self = Self(13);
    /// 14: the authentication token has not been authorized by the user yet.
    pub const UNAUTHORIZED_TOKEN: Self = Self(14);
    /// 16: a temporary error. The same request will probably succeed.
    pub const TEMPORARY_ERROR: Self = Self(16);
    /// 17: login required. The profile is not public, or the user changed
    /// their privacy settings. Not the same as an empty history.
    pub const LOGIN_REQUIRED: Self = Self(17);
    /// 26: the API key has been suspended.
    pub const SUSPENDED_KEY: Self = Self(26);
    /// 29: the rate limit was exceeded.
    pub const RATE_LIMIT_EXCEEDED: Self = Self(29);

    /// Wraps a code as Last.fm sent it.
    pub const fn new(code: u32) -> Self {
        Self(code)
    }

    /// The number Last.fm sent.
    pub const fn get(self) -> u32 {
        self.0
    }
}

/// An error from building a request, talking to Last.fm or reading its answer.
///
/// Callers branch on [`kind`](Self::kind), on the Last.fm code from
/// [`api_code`](Self::api_code), and on the two pieces of advice the library
/// can give: [`retry`](Self::retry) says whether trying the same call again
/// is reasonable, and [`delivery`](Self::delivery) says what a failed write
/// did.
///
/// # What an error contains
///
/// `Display` and `Debug` are safe to log. They name the kind, the Last.fm
/// method, the HTTP status and the API code, and nothing taken from the
/// network: no URL, no credential, no response text. That includes the
/// `message` of a Last.fm error envelope. The message and the body are still
/// reachable, on purpose, through [`api_message`](Self::api_message) and
/// [`body`](Self::body); the caller who asks for them decides where they go.
///
/// The body is capped at 64 KiB and the message at 1 KiB, the message cut on
/// a UTF-8 character boundary.
pub struct Error {
    inner: Box<Inner>,
}

struct Inner {
    kind: ErrorKind,
    spec: Option<&'static MethodSpec>,
    http_status: Option<u16>,
    api_code: Option<ApiErrorCode>,
    api_message: Option<Box<str>>,
    body: Option<Box<[u8]>>,
    /// A short description written by this library. Never network text.
    detail: Option<Box<str>>,
    /// For transport failures: whether the request may have reached the
    /// server.
    possibly_sent: bool,
    source: Option<Box<dyn StdError + Send + Sync>>,
}

impl Error {
    fn from_kind(kind: ErrorKind) -> Self {
        Self {
            inner: Box::new(Inner {
                kind,
                spec: None,
                http_status: None,
                api_code: None,
                api_message: None,
                body: None,
                detail: None,
                possibly_sent: false,
                source: None,
            }),
        }
    }

    fn with_detail(mut self, detail: &str) -> Self {
        self.inner.detail = Some(truncate_utf8(detail, MAX_DETAIL).into());
        self
    }

    /// The broad category of the failure.
    pub fn kind(&self) -> ErrorKind {
        self.inner.kind
    }

    /// The Last.fm error code, for [`ErrorKind::Api`].
    pub fn api_code(&self) -> Option<ApiErrorCode> {
        self.inner.api_code
    }

    /// The message Last.fm sent with the error, cut to 1 KiB. Free text from
    /// the network: do not assume it is safe to display.
    pub fn api_message(&self) -> Option<&str> {
        self.inner.api_message.as_deref()
    }

    /// The HTTP status, when a response arrived.
    pub fn http_status(&self) -> Option<u16> {
        self.inner.http_status
    }

    /// The Last.fm method the call was for, such as `user.getRecentTracks`.
    pub fn method(&self) -> Option<&'static str> {
        self.inner.spec.map(|spec| spec.name())
    }

    /// The response body, capped at 64 KiB. Raw bytes from the network, and
    /// for some methods they include secrets such as a session key.
    pub fn body(&self) -> Option<&[u8]> {
        self.inner.body.as_deref()
    }

    /// Whether repeating the same call is reasonable.
    ///
    /// For a read: transient failures (API codes 11 and 16, HTTP 5xx,
    /// transport failures and timeouts) are [`Retry::Later`]; code 29 and
    /// HTTP 429 are [`Retry::AfterBackoff`]; code 9 is
    /// [`Retry::AfterReauthentication`]. Everything else is [`Retry::No`].
    ///
    /// For a write the API code rules are the same. A transport failure
    /// that certainly sent nothing is [`Retry::Later`]. Anything whose
    /// [`delivery`](Self::delivery) is [`Delivery::Unknown`] is
    /// [`Retry::No`], and `track.updateNowPlaying` is always [`Retry::No`].
    ///
    /// `auth.getSession` spends its token, so a timeout or a transport
    /// failure after the request may have left is [`Retry::No`] for it too:
    /// a repeat could only fail, and would hide what happened to the first.
    pub fn retry(&self) -> Retry {
        let inner = &*self.inner;
        let write = inner.spec.is_some_and(|spec| spec.write());
        if inner
            .spec
            .is_some_and(|spec| spec.name() == UPDATE_NOW_PLAYING)
        {
            return Retry::No;
        }
        if write && self.delivery() == Some(Delivery::Unknown) {
            return Retry::No;
        }
        let possibly_received = match inner.kind {
            ErrorKind::Timeout => true,
            ErrorKind::Transport => inner.possibly_sent,
            _ => false,
        };
        if possibly_received && inner.spec.is_some_and(|spec| spec.name() == GET_SESSION) {
            return Retry::No;
        }
        match inner.kind {
            ErrorKind::Api => match inner.api_code {
                Some(ApiErrorCode::SERVICE_OFFLINE | ApiErrorCode::TEMPORARY_ERROR) => Retry::Later,
                Some(ApiErrorCode::RATE_LIMIT_EXCEEDED) => Retry::AfterBackoff,
                Some(ApiErrorCode::INVALID_SESSION_KEY) => Retry::AfterReauthentication,
                _ => Retry::No,
            },
            ErrorKind::Http => match inner.http_status {
                Some(429) => Retry::AfterBackoff,
                Some(500..=599) => Retry::Later,
                _ => Retry::No,
            },
            ErrorKind::Transport | ErrorKind::Timeout => Retry::Later,
            _ => Retry::No,
        }
    }

    /// What a failed write did. `None` for a read, and for an error that is
    /// not about a method call, such as a configuration error.
    ///
    /// [`Delivery::NotSent`] for an invalid request, a write refused as
    /// read-only or a transport failure that certainly sent nothing;
    /// [`Delivery::Rejected`] for an error
    /// envelope; [`Delivery::Unknown`] for everything else, including a
    /// timeout and any failure after the request may have left.
    pub fn delivery(&self) -> Option<Delivery> {
        let inner = &*self.inner;
        if !inner.spec?.write() {
            return None;
        }
        Some(match inner.kind {
            ErrorKind::InvalidRequest | ErrorKind::Config | ErrorKind::ReadOnly => {
                Delivery::NotSent
            }
            ErrorKind::Transport if !inner.possibly_sent => Delivery::NotSent,
            ErrorKind::Api => Delivery::Rejected,
            _ => Delivery::Unknown,
        })
    }
}

// Constructors for the code in this crate.
impl Error {
    /// Attaches the method the call was for.
    pub(crate) fn with_method(mut self, spec: &'static MethodSpec) -> Self {
        self.inner.spec = Some(spec);
        self
    }

    /// Attaches the HTTP status and, capped, the body.
    pub(crate) fn with_response(mut self, status: u16, body: &[u8]) -> Self {
        self.inner.http_status = Some(status);
        self.inner.body = Some(body.get(..MAX_BODY).unwrap_or(body).into());
        self
    }

    /// Last.fm answered with an error envelope.
    pub(crate) fn api(
        spec: &'static MethodSpec,
        status: u16,
        code: ApiErrorCode,
        message: Option<&str>,
        body: &[u8],
    ) -> Self {
        let mut error = Self::from_kind(ErrorKind::Api)
            .with_method(spec)
            .with_response(status, body);
        error.inner.api_code = Some(code);
        error.inner.api_message = message.map(|m| truncate_utf8(m, MAX_MESSAGE).into());
        error
    }

    /// A non-2xx status with no error envelope.
    pub(crate) fn http(spec: &'static MethodSpec, status: u16, body: &[u8]) -> Self {
        Self::from_kind(ErrorKind::Http)
            .with_method(spec)
            .with_response(status, body)
    }

    /// The body was not the JSON the caller expected. `problem` must not
    /// contain text from the response.
    pub(crate) fn decode(problem: &str) -> Self {
        Self::from_kind(ErrorKind::Decode).with_detail(problem)
    }

    /// A field was missing or malformed. `path` names it, for example
    /// `recenttracks.@attr.total`; `problem` must not contain response text.
    pub(crate) fn decode_field(path: &str, problem: &str) -> Self {
        Self::decode(&format!("field `{path}`: {problem}"))
    }

    /// A response broke a consistency rule. `rule` must not contain
    /// response text.
    pub(crate) fn inconsistent(rule: &str) -> Self {
        Self::from_kind(ErrorKind::Inconsistent).with_detail(rule)
    }

    /// A connection, TLS or I/O failure. `possibly_sent` says whether the
    /// request may have reached the server. The source must not expose the
    /// URL: strip it before boxing.
    #[cfg_attr(not(feature = "client"), allow(dead_code))] // Raised by the client.
    pub(crate) fn transport(
        possibly_sent: bool,
        source: impl Into<Box<dyn StdError + Send + Sync>>,
    ) -> Self {
        let mut error = Self::from_kind(ErrorKind::Transport);
        error.inner.possibly_sent = possibly_sent;
        error.inner.source = Some(source.into());
        error
    }

    /// A deadline passed.
    #[cfg_attr(not(feature = "client"), allow(dead_code))] // Raised by the client.
    pub(crate) fn timeout() -> Self {
        Self::from_kind(ErrorKind::Timeout)
    }

    /// The response exceeded `limit` bytes.
    #[cfg_attr(not(feature = "client"), allow(dead_code))] // Raised by the client.
    pub(crate) fn body_too_large(limit: usize) -> Self {
        Self::from_kind(ErrorKind::BodyTooLarge).with_detail(&format!("limit is {limit} bytes"))
    }

    /// The request could not be built. `problem` may name a parameter but
    /// never contains a value.
    pub(crate) fn invalid_request(spec: &'static MethodSpec, problem: &str) -> Self {
        Self::from_kind(ErrorKind::InvalidRequest)
            .with_method(spec)
            .with_detail(problem)
    }

    /// A write was asked of credentials that do not allow writes.
    pub(crate) fn read_only(spec: &'static MethodSpec) -> Self {
        Self::from_kind(ErrorKind::ReadOnly)
            .with_method(spec)
            .with_detail("writes are not allowed; use a `Writer`, or `Credentials::allow_writes`")
    }

    /// The client could not be built.
    #[cfg_attr(not(feature = "client"), allow(dead_code))] // Raised by the client.
    pub(crate) fn config(problem: &str) -> Self {
        Self::from_kind(ErrorKind::Config).with_detail(problem)
    }
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let inner = &*self.inner;
        let method = inner.spec.map_or("", |spec| spec.name());
        let status = inner.http_status;
        match inner.kind {
            ErrorKind::Api => {
                f.write_str("Last.fm returned error")?;
                if let Some(code) = inner.api_code {
                    write!(f, " {}", code.get())?;
                }
            }
            ErrorKind::Http => f.write_str("unexpected HTTP status")?,
            ErrorKind::Decode => f.write_str("could not decode the response")?,
            ErrorKind::Inconsistent => f.write_str("inconsistent response")?,
            ErrorKind::Transport => f.write_str("transport failure")?,
            ErrorKind::Timeout => f.write_str("the request timed out")?,
            ErrorKind::BodyTooLarge => f.write_str("the response body was too large")?,
            ErrorKind::InvalidRequest => f.write_str("invalid request")?,
            ErrorKind::Config => f.write_str("invalid configuration")?,
            ErrorKind::ReadOnly => f.write_str("refused a write")?,
        }
        if !method.is_empty() {
            write!(f, " for {method}")?;
        }
        if let Some(status) = status {
            write!(f, " (HTTP {status})")?;
        }
        if let Some(detail) = &inner.detail {
            write!(f, ": {detail}")?;
        }
        Ok(())
    }
}

impl fmt::Debug for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let inner = &*self.inner;
        let mut out = f.debug_struct("Error");
        out.field("kind", &inner.kind);
        if let Some(spec) = inner.spec {
            out.field("method", &spec.name());
        }
        if let Some(status) = inner.http_status {
            out.field("http_status", &status);
        }
        if let Some(code) = inner.api_code {
            out.field("api_code", &code.get());
        }
        if let Some(detail) = &inner.detail {
            out.field("detail", detail);
        }
        if let Some(body) = &inner.body {
            out.field("body_len", &body.len());
        }
        out.finish_non_exhaustive()
    }
}

impl StdError for Error {
    fn source(&self) -> Option<&(dyn StdError + 'static)> {
        self.inner
            .source
            .as_ref()
            .map(|source| &**source as &(dyn StdError + 'static))
    }
}

#[cfg(test)]
mod tests;
