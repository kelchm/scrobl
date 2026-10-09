//! [`ClientBuilder`]: configuration, checked once at [`build`](ClientBuilder::build).

use std::fmt;
use std::net::IpAddr;
use std::sync::Arc;
use std::time::Duration;

use super::pacing::{MAX_INTERVAL, Pacer};
use super::{Client, Shared};
use crate::error::Error;
use crate::protocol::Credentials;
use crate::secret::{ApiKey, ApiSecret, SessionKey};

const DEFAULT_CONNECT_TIMEOUT: Duration = Duration::from_secs(10);
const DEFAULT_TIMEOUT: Duration = Duration::from_secs(30);
const DEFAULT_MAX_RESPONSE_BYTES: usize = 8 * 1024 * 1024;
const DEFAULT_MIN_INTERVAL: Duration = Duration::from_secs(1);
const DEFAULT_READ_ATTEMPTS: u32 = 3;
const DEFAULT_RETRY_DELAY: Duration = Duration::from_secs(1);

/// What a client does, apart from its credentials.
#[derive(Debug, Clone)]
pub(super) struct Settings {
    pub(super) user_agent: String,
    pub(super) connect_timeout: Duration,
    pub(super) timeout: Duration,
    pub(super) max_response_bytes: usize,
    pub(super) min_interval: Duration,
    pub(super) read_attempts: u32,
    pub(super) retry_delay: Duration,
    pub(super) no_proxy: bool,
}

/// Configures and builds a [`Client`].
///
/// Start with [`Client::builder`]. Every setting has a default, so
/// `Client::builder(api_key).build()` is a working client. Settings are
/// checked together by [`build`](Self::build), which fails with
/// [`ErrorKind::Config`](crate::ErrorKind) and never puts a credential in
/// the error.
///
/// `Debug` shows the settings and hides every credential.
#[derive(Clone)]
#[must_use = "a builder does nothing until `build` is called"]
pub struct ClientBuilder {
    credentials: Credentials,
    settings: Settings,
    base_url: Option<String>,
}

impl fmt::Debug for ClientBuilder {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        // Only whether a root is set: it is not part of the supported API,
        // and a test root can carry anything.
        f.debug_struct("ClientBuilder")
            .field("credentials", &self.credentials)
            .field("settings", &self.settings)
            .field("base_url", &self.base_url.is_some())
            .finish()
    }
}

impl ClientBuilder {
    pub(super) fn new(api_key: ApiKey) -> Self {
        Self {
            credentials: Credentials::new(api_key),
            settings: Settings {
                user_agent: format!("scrobl/{}", env!("CARGO_PKG_VERSION")),
                connect_timeout: DEFAULT_CONNECT_TIMEOUT,
                timeout: DEFAULT_TIMEOUT,
                max_response_bytes: DEFAULT_MAX_RESPONSE_BYTES,
                min_interval: DEFAULT_MIN_INTERVAL,
                read_attempts: DEFAULT_READ_ATTEMPTS,
                retry_delay: DEFAULT_RETRY_DELAY,
                no_proxy: false,
            },
            base_url: None,
        }
    }

    /// Adds the API secret, which methods that are signed need. It signs
    /// requests and is never sent.
    pub fn secret(mut self, secret: ApiSecret) -> Self {
        self.credentials = self.credentials.with_secret(secret);
        self
    }

    /// Adds a user's session key, which methods that act for a user need.
    /// [`Client::with_session`] makes a client for another session later.
    pub fn session(mut self, session: SessionKey) -> Self {
        self.credentials = self.credentials.with_session(session);
        self
    }

    /// Sets the `User-Agent` header. The default is `scrobl/<version>`.
    ///
    /// Last.fm asks applications to identify themselves, so name yours and
    /// give a way to reach you, as in `"my-app/1.0 (+https://example.org)"`.
    /// An empty value, or one with a character that cannot appear in a
    /// header (anything outside printable ASCII), fails at
    /// [`build`](Self::build).
    pub fn user_agent(mut self, user_agent: impl Into<String>) -> Self {
        self.settings.user_agent = user_agent.into();
        self
    }

    /// The time allowed to open a connection, TLS included. The default is
    /// 10 seconds. Must not be zero.
    pub fn connect_timeout(mut self, timeout: Duration) -> Self {
        self.settings.connect_timeout = timeout;
        self
    }

    /// The longest the client waits for one attempt, from the start to the
    /// last byte of the body. The default is 30 seconds. Must not be zero.
    ///
    /// A call that retries has this much for each attempt, not for the whole
    /// call; pacing waits and retry delays are outside it. It limits waiting,
    /// not polling: a response that has already arrived in full when the
    /// future is next polled is returned even if the deadline has passed. See
    /// the [module documentation](super) for how to bound a whole scan.
    pub fn timeout(mut self, timeout: Duration) -> Self {
        self.settings.timeout = timeout;
        self
    }

    /// The most body bytes a response may have. The default is 8 MiB. Must
    /// not be zero.
    ///
    /// A response over the cap is [`ErrorKind::BodyTooLarge`](crate::ErrorKind),
    /// and the client stops reading as soon as it knows. The cap bounds the
    /// length of the body the client keeps; the transport's own buffer for the
    /// chunk in hand is on top of it.
    pub fn max_response_bytes(mut self, bytes: usize) -> Self {
        self.settings.max_response_bytes = bytes;
        self
    }

    /// The least time between the starts of two requests, across this client
    /// and every clone of it. The default is one second, this library's
    /// conservative choice and not an allowance Last.fm documents. Separate
    /// clients and processes do not share the pacing. Zero turns pacing off;
    /// more than 24 hours fails at [`build`](Self::build).
    ///
    /// A start is the moment a request is let through to the transport,
    /// measured in this process; connection setup can still make requests
    /// arrive at the server slightly closer together. See the
    /// [module documentation](super).
    pub fn min_interval(mut self, interval: Duration) -> Self {
        self.settings.min_interval = interval;
        self
    }

    /// The most attempts a read makes, the first included. The default is 3;
    /// 1 turns retries off, and 0 fails at [`build`](Self::build). A write
    /// is always attempted once.
    pub fn read_attempts(mut self, attempts: u32) -> Self {
        self.settings.read_attempts = attempts;
        self
    }

    /// The base wait before a retry. The default is one second.
    ///
    /// The wait doubles with each retry, and is five times as long after a
    /// rate-limit answer. It comes on top of pacing.
    pub fn retry_delay(mut self, delay: Duration) -> Self {
        self.settings.retry_delay = delay;
        self
    }

    /// Ignores the proxy environment variables (`HTTPS_PROXY`, `ALL_PROXY`
    /// and the rest), which a client honours by default, and connects
    /// directly.
    ///
    /// Through an HTTP proxy the client sends the proxy a `CONNECT` naming
    /// the host and port, and the request itself, its query and the API key
    /// stay inside TLS. Use this to refuse a proxy anyway.
    pub fn no_proxy(mut self) -> Self {
        self.settings.no_proxy = true;
        self
    }

    /// Talks to a local test server instead of Last.fm, over plain HTTP if
    /// it says so, and ignores proxy environment variables. Not part of the
    /// supported API.
    ///
    /// Only a literal loopback root is accepted (`127.0.0.0/8` or `::1`,
    /// with no user information, query or fragment), so the hook
    /// cannot switch off HTTPS or send a key to another host. Anything else
    /// fails at [`build`](Self::build).
    #[doc(hidden)]
    pub fn base_url(mut self, base_url: impl Into<String>) -> Self {
        self.base_url = Some(base_url.into());
        self
    }

    /// Builds the client.
    ///
    /// Needs no runtime, so a client can be made at start-up and used later.
    ///
    /// # Errors
    ///
    /// [`ErrorKind::Config`](crate::ErrorKind) when a setting is not valid
    /// (an empty API key, secret or session key, an empty or unusable user
    /// agent, a zero timeout, a zero body cap, zero attempts or a pacing
    /// interval over 24 hours) or when the HTTP client cannot be set up.
    pub fn build(self) -> Result<Client, Error> {
        let Self {
            credentials,
            settings,
            base_url,
        } = self;
        check(&settings)?;
        check_credentials(&credentials)?;

        let mut http = reqwest::Client::builder()
            .user_agent(settings.user_agent.as_str())
            .connect_timeout(settings.connect_timeout)
            // A redirect would resend the query, which holds the API key, to
            // wherever the response points. Last.fm does not redirect.
            .redirect(reqwest::redirect::Policy::none())
            // The built-in policy resends requests on protocol NACKs. This
            // crate decides what is safe to repeat.
            .retry(reqwest::retry::never())
            // The body is exactly what the service sent. Cargo unifies
            // features, so another crate in the application can switch these
            // decoders on; the methods exist without the features.
            .no_gzip()
            .no_brotli()
            .no_deflate()
            .no_zstd();
        http = match &base_url {
            None => http.https_only(true),
            Some(url) => {
                check_base_url(url)?;
                http.no_proxy()
            }
        };
        if settings.no_proxy {
            http = http.no_proxy();
        }
        let http = http.build().map_err(|e| {
            Error::config(&format!(
                "could not set up the HTTP client: {}",
                e.without_url()
            ))
        })?;

        Ok(Client {
            shared: Arc::new(Shared {
                http,
                root: base_url,
                pacer: Pacer::new(settings.min_interval),
                settings,
            }),
            credentials: Arc::new(credentials),
        })
    }
}

/// A root is only for tests, so it must be this machine: a literal loopback
/// address, not a name a resolver could point elsewhere. The messages say what is wrong, never what was
/// given.
fn check_base_url(url: &str) -> Result<(), Error> {
    let url =
        reqwest::Url::parse(url).map_err(|_| Error::config("the base URL is not a valid URL"))?;
    if !matches!(url.scheme(), "http" | "https") {
        return Err(Error::config("the base URL must be http or https"));
    }
    if !url.username().is_empty()
        || url.password().is_some()
        || url.query().is_some()
        || url.fragment().is_some()
    {
        return Err(Error::config(
            "the base URL must not carry credentials, a query or a fragment",
        ));
    }
    // The parser has already made a numeric host canonical, and writes an
    // IPv6 one in brackets.
    let loopback = url.host_str().is_some_and(|host| {
        host.trim_start_matches('[')
            .trim_end_matches(']')
            .parse::<IpAddr>()
            .is_ok_and(|address| address.is_loopback())
    });
    if !loopback {
        return Err(Error::config(
            "the base URL must be a literal loopback address",
        ));
    }
    Ok(())
}

fn check_credentials(credentials: &Credentials) -> Result<(), Error> {
    match credentials.blank() {
        Some(which) => Err(Error::config(&format!("the {which} is empty"))),
        None => Ok(()),
    }
}

fn check(settings: &Settings) -> Result<(), Error> {
    let agent = settings.user_agent.as_str();
    if agent.trim().is_empty() {
        return Err(Error::config("the user agent is empty"));
    }
    if !agent.bytes().all(|b| (0x20..=0x7e).contains(&b)) {
        return Err(Error::config(
            "the user agent has a character that cannot appear in a header",
        ));
    }
    if settings.connect_timeout.is_zero() {
        return Err(Error::config("the connect timeout is zero"));
    }
    if settings.timeout.is_zero() {
        return Err(Error::config("the timeout is zero"));
    }
    if settings.max_response_bytes == 0 {
        return Err(Error::config("the response body cap is zero"));
    }
    if settings.min_interval > MAX_INTERVAL {
        return Err(Error::config("the pacing interval is longer than 24 hours"));
    }
    if settings.read_attempts == 0 {
        return Err(Error::config("the read attempts must be at least 1"));
    }
    Ok(())
}
