//! [`ClientBuilder`]: configuration, checked once at [`build`](ClientBuilder::build).

use std::sync::Arc;
use std::time::Duration;

use super::pacing::Pacer;
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
#[derive(Debug, Clone)]
#[must_use = "a builder does nothing until `build` is called"]
pub struct ClientBuilder {
    credentials: Credentials,
    settings: Settings,
    base_url: Option<String>,
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

    /// The time allowed for one attempt, from the start to the last byte of
    /// the body. The default is 30 seconds. Must not be zero.
    ///
    /// A call that retries has this much for each attempt, not for the whole
    /// call.
    pub fn timeout(mut self, timeout: Duration) -> Self {
        self.settings.timeout = timeout;
        self
    }

    /// The most body bytes a response may have. The default is 8 MiB. Must
    /// not be zero.
    ///
    /// A response over the cap is [`ErrorKind::BodyTooLarge`](crate::ErrorKind),
    /// and the client stops reading as soon as it knows.
    pub fn max_response_bytes(mut self, bytes: usize) -> Self {
        self.settings.max_response_bytes = bytes;
        self
    }

    /// The least time between the starts of two requests, across this client
    /// and every clone of it. The default is one second, which is what
    /// Last.fm asks of an application. Zero turns pacing off.
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

    /// Talks to a different root, such as a local test server, over plain
    /// HTTP if it says so, and ignores proxy environment variables. Not part
    /// of the supported API.
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
    /// (an empty or unusable user agent, a zero timeout, a zero body cap or
    /// zero attempts) or when the HTTP client cannot be set up.
    pub fn build(self) -> Result<Client, Error> {
        let Self {
            credentials,
            settings,
            base_url,
        } = self;
        check(&settings)?;

        let mut http = reqwest::Client::builder()
            .user_agent(settings.user_agent.as_str())
            .connect_timeout(settings.connect_timeout)
            // A redirect would resend the query, which holds the API key, to
            // wherever the response points. Last.fm does not redirect.
            .redirect(reqwest::redirect::Policy::none())
            // The built-in policy resends requests on protocol NACKs. This
            // crate decides what is safe to repeat.
            .retry(reqwest::retry::never());
        http = match &base_url {
            None => http.https_only(true),
            Some(url) => {
                reqwest::Url::parse(url)
                    .map_err(|_| Error::config("the base URL is not a valid URL"))?;
                http.no_proxy()
            }
        };
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
    if settings.read_attempts == 0 {
        return Err(Error::config("the read attempts must be at least 1"));
    }
    Ok(())
}
