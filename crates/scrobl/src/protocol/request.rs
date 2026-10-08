//! Building the HTTP request for a method call: [`prepare`] turns a
//! [`Request`] and [`Credentials`] into an [`HttpRequest`], plain data that
//! any HTTP client can send.

use std::collections::HashSet;
use std::fmt;

use bytes::Bytes;

use super::{Auth, MethodSpec, Requirement, Verb, sign::sign};
use crate::error::{Error, truncate_utf8};
use crate::secret::{ApiKey, ApiSecret, SessionKey};

/// Where every call goes.
const ROOT: &str = "https://ws.audioscrobbler.com/2.0/";

/// Names the library sets itself.
const RESERVED: [&str; 6] = ["method", "api_key", "api_sig", "sk", "format", "callback"];

/// Parameters whose values are credentials, or become them.
const SENSITIVE: [&str; 5] = ["api_key", "api_sig", "sk", "token", "password"];

/// What is printed in place of a sensitive value.
const REDACTED: &str = "<redacted>";

/// The most of a parameter name an error message repeats.
const MAX_NAME_IN_MESSAGE: usize = 64;

/// The credentials a call is made with.
///
/// An API key is always needed. The secret is needed for methods that are
/// signed, and the session key for methods that act for a user.
#[derive(Debug, Clone)]
pub struct Credentials {
    api_key: ApiKey,
    secret: Option<ApiSecret>,
    session: Option<SessionKey>,
}

impl Credentials {
    /// Credentials with an API key only, enough for plain reads.
    pub fn new(api_key: ApiKey) -> Self {
        Self {
            api_key,
            secret: None,
            session: None,
        }
    }

    /// Adds the API secret, so signed methods can be called.
    #[must_use]
    pub fn with_secret(mut self, secret: ApiSecret) -> Self {
        self.secret = Some(secret);
        self
    }

    /// Adds a user's session key, so methods that act for the user can be
    /// called.
    #[must_use]
    pub fn with_session(mut self, session: SessionKey) -> Self {
        self.session = Some(session);
        self
    }
}

/// A parameter value. Strings are sent as they are, integers in decimal and
/// booleans as `1` or `0`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParamValue(String);

impl From<&str> for ParamValue {
    fn from(value: &str) -> Self {
        Self(value.to_owned())
    }
}

impl From<String> for ParamValue {
    fn from(value: String) -> Self {
        Self(value)
    }
}

impl From<&String> for ParamValue {
    fn from(value: &String) -> Self {
        Self(value.clone())
    }
}

impl From<bool> for ParamValue {
    fn from(value: bool) -> Self {
        Self(String::from(if value { "1" } else { "0" }))
    }
}

macro_rules! param_value_from_int {
    ($($int:ty),*) => {
        $(impl From<$int> for ParamValue {
            fn from(value: $int) -> Self {
                Self(value.to_string())
            }
        })*
    };
}

param_value_from_int!(
    i8, i16, i32, i64, i128, isize, u8, u16, u32, u64, u128, usize
);

/// One call to a Last.fm method: the method and the parameters the caller
/// chose. The library adds `method`, `api_key`, `format`, `sk` and `api_sig`
/// itself.
#[derive(Clone)]
pub struct Request {
    spec: &'static MethodSpec,
    params: Vec<(String, String)>,
    as_user: bool,
}

impl Request {
    /// A call to `spec` with no parameters yet.
    pub fn new(spec: &'static MethodSpec) -> Self {
        Self {
            spec,
            params: Vec::new(),
            as_user: false,
        }
    }

    /// Sends the session key and a signature even though the method does not
    /// require them, so a read is made as the session's user.
    ///
    /// A user who hides their listening history can still read it this way,
    /// and a few methods default `user` to the session's user. The official
    /// method pages describe this mode without documenting it fully, so
    /// which reads accept a session is not verified. Methods that already
    /// require a session are unaffected.
    #[must_use]
    pub fn as_user(mut self) -> Self {
        self.as_user = true;
        self
    }

    /// Sets a parameter. Parameters are sent in the order they are set.
    ///
    /// Setting a name twice, or a reserved name, is reported by
    /// [`prepare`], not here.
    #[must_use]
    pub fn param(mut self, name: &str, value: impl Into<ParamValue>) -> Self {
        self.params.push((name.to_owned(), value.into().0));
        self
    }

    /// Sets one element of a batch parameter, sent as `name[index]`.
    #[must_use]
    pub fn indexed(mut self, name: &str, index: usize, value: impl Into<ParamValue>) -> Self {
        self.params
            .push((format!("{name}[{index}]"), value.into().0));
        self
    }

    /// The Last.fm method name.
    pub fn method(&self) -> &'static str {
        self.spec.name
    }

    pub(crate) fn spec(&self) -> &'static MethodSpec {
        self.spec
    }
}

impl fmt::Debug for Request {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        struct Params<'a>(&'a [(String, String)]);
        impl fmt::Debug for Params<'_> {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.debug_map()
                    .entries(self.0.iter().map(|(name, value)| {
                        let value = if is_sensitive(name) { REDACTED } else { value };
                        (name, value)
                    }))
                    .finish()
            }
        }
        f.debug_struct("Request")
            .field("method", &self.spec.name)
            .field("params", &Params(&self.params))
            .field("as_user", &self.as_user)
            .finish()
    }
}

/// A request ready to send, as plain data.
///
/// Its `Debug` output hides the values of `api_key`, `api_sig`, `sk`,
/// `token` and `password`. [`url`](Self::url) and [`body`](Self::body) are
/// exact and contain them, so do not log those.
#[derive(Clone)]
pub struct HttpRequest {
    verb: Verb,
    url: String,
    body: Option<Bytes>,
}

impl HttpRequest {
    /// The `Content-Type` to send with a `POST` body.
    pub const CONTENT_TYPE: &'static str = "application/x-www-form-urlencoded";

    /// The HTTP verb.
    pub fn verb(&self) -> Verb {
        self.verb
    }

    /// The full URL. For a `GET` it includes the query string.
    pub fn url(&self) -> &str {
        &self.url
    }

    /// The form-encoded body of a `POST`, and `None` for a `GET`. Send it
    /// with [`CONTENT_TYPE`](Self::CONTENT_TYPE).
    pub fn body(&self) -> Option<&[u8]> {
        self.body.as_deref()
    }
}

impl fmt::Debug for HttpRequest {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let (path, query) = match self.url.split_once('?') {
            Some((path, query)) => (path, Some(query)),
            None => (self.url.as_str(), None),
        };
        let url = match query {
            Some(query) => format!("{path}?{}", redact_form(query.as_bytes())),
            None => path.to_owned(),
        };
        f.debug_struct("HttpRequest")
            .field("verb", &self.verb)
            .field("url", &url)
            .field("body", &self.body.as_deref().map(redact_form))
            .finish()
    }
}

fn is_sensitive(name: &str) -> bool {
    SENSITIVE.contains(&name)
}

/// A form-encoded string with sensitive values replaced.
fn redact_form(encoded: &[u8]) -> String {
    let mut out = String::with_capacity(encoded.len());
    for (name, value) in form_urlencoded::parse(encoded) {
        if !out.is_empty() {
            out.push('&');
        }
        out.extend(form_urlencoded::byte_serialize(name.as_bytes()));
        out.push('=');
        if is_sensitive(&name) {
            out.push_str(REDACTED);
        } else {
            out.extend(form_urlencoded::byte_serialize(value.as_bytes()));
        }
    }
    out
}

/// Builds the HTTP request for `request`.
///
/// Adds `method`, `api_key` and `format=json`, then `sk` and `api_sig` when
/// the method's [`Auth`] needs them or the request is made
/// [`as_user`](Request::as_user). A `GET` carries everything in the query
/// string; a `POST` carries everything, `method` included, in a form-encoded
/// body. Values are percent-encoded as UTF-8.
///
/// Parameters the method does not list are allowed. Optional and
/// conditional parameters are not checked.
///
/// # Errors
///
/// Fails with [`ErrorKind::InvalidRequest`](crate::ErrorKind) and sends
/// nothing when a credential the method needs is missing, when a caller
/// parameter has a reserved name (`method`, `api_key`, `api_sig`, `sk`,
/// `format`, `callback`), when a name is set twice, or when a parameter the
/// method requires is absent. For a batch parameter one index is enough. The
/// message names the parameter, never a value.
///
/// ```
/// use scrobl::ApiKey;
/// use scrobl::protocol::{self, Credentials, Request, Verb, methods};
///
/// let credentials = Credentials::new(ApiKey::new("key"));
/// let request = Request::new(&methods::USER_GET_RECENT_TRACKS)
///     .param("user", "rj")
///     .param("limit", 200);
/// let http = protocol::prepare(&credentials, &request)?;
/// assert_eq!(http.verb(), Verb::Get);
/// assert_eq!(
///     http.url(),
///     "https://ws.audioscrobbler.com/2.0/?method=user.getRecentTracks&api_key=key&user=rj&limit=200&format=json"
/// );
/// # Ok::<(), scrobl::Error>(())
/// ```
pub fn prepare(credentials: &Credentials, request: &Request) -> Result<HttpRequest, Error> {
    prepare_with_root(ROOT, credentials, request)
}

/// [`prepare`] against a different root URL, for tests that talk to a local
/// server. Not part of the supported API.
#[doc(hidden)]
pub fn prepare_with_root(
    root: &str,
    credentials: &Credentials,
    request: &Request,
) -> Result<HttpRequest, Error> {
    let spec = request.spec;
    let invalid = |problem: &str| Error::invalid_request(spec, problem);

    let auth = if request.as_user {
        Auth::Session
    } else {
        spec.auth
    };
    let secret = match auth {
        Auth::ApiKey => None,
        Auth::Signed | Auth::Session => Some(
            credentials
                .secret
                .as_ref()
                .ok_or_else(|| invalid("the call is signed and needs an API secret"))?,
        ),
    };
    let session = match auth {
        Auth::Session => Some(
            credentials
                .session
                .as_ref()
                .ok_or_else(|| invalid("the call needs a session key"))?,
        ),
        Auth::ApiKey | Auth::Signed => None,
    };

    check_params(request, &invalid)?;

    let mut params: Vec<(&str, &str)> = Vec::with_capacity(request.params.len() + 5);
    params.push(("method", spec.name));
    params.push(("api_key", credentials.api_key.expose()));
    params.extend(request.params.iter().map(|(n, v)| (n.as_str(), v.as_str())));
    if let Some(session) = session {
        params.push(("sk", session.expose()));
    }
    let signature;
    if let Some(secret) = secret {
        signature = sign(params.iter().copied(), secret);
        params.push(("api_sig", &signature));
    }
    params.push(("format", "json"));

    let encoded = form_urlencoded::Serializer::new(String::new())
        .extend_pairs(params)
        .finish();

    Ok(match spec.verb {
        Verb::Get => HttpRequest {
            verb: Verb::Get,
            url: format!("{root}?{encoded}"),
            body: None,
        },
        Verb::Post => HttpRequest {
            verb: Verb::Post,
            url: root.to_owned(),
            body: Some(Bytes::from(encoded)),
        },
    })
}

/// Checks the caller's parameters against the reserved names, each other and
/// the method's required parameters.
fn check_params(request: &Request, invalid: &dyn Fn(&str) -> Error) -> Result<(), Error> {
    let name_in = |name: &str| truncate_utf8(name, MAX_NAME_IN_MESSAGE).to_owned();

    let mut seen = HashSet::new();
    for (name, _) in &request.params {
        // `callback[0]` is as reserved as `callback`.
        let base = name.split('[').next().unwrap_or(name);
        if name.is_empty() {
            return Err(invalid("a parameter name is empty"));
        }
        if RESERVED.contains(&base) {
            return Err(invalid(&format!(
                "`{}` is set by the library and cannot be passed",
                name_in(base)
            )));
        }
        if !seen.insert(name.as_str()) {
            return Err(invalid(&format!(
                "parameter `{}` is set more than once",
                name_in(name)
            )));
        }
    }

    for spec in request.spec.params {
        if spec.requirement != Requirement::Required {
            continue;
        }
        let present = if spec.indexed {
            request.params.iter().any(|(name, _)| {
                name.strip_prefix(spec.name)
                    .is_some_and(|rest| rest.starts_with('[') && rest.ends_with(']'))
            })
        } else {
            seen.contains(spec.name)
        };
        if !present {
            return Err(invalid(&format!(
                "required parameter `{}` is missing",
                spec.name
            )));
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests;
