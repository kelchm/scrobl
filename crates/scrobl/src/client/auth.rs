//! Signing a user in: the three ways to get a session key.

use super::Client;
use crate::error::{ApiErrorCode, Error};
use crate::model::{Session, decode};
use crate::protocol::{MethodSpec, Request, methods};
use crate::response::Response;
use crate::secret::{AuthToken, Password};

/// Where the user approves a sign-in.
const APPROVAL: &str = "https://www.last.fm/api/auth/";

/// Signing a user in. Made by [`Client::auth`].
///
/// All three flows end in a [`Session`], whose key makes a client that acts
/// for the user: [`Client::with_session`], or [`Writer::with_session`](super::Writer::with_session)
/// to write. The library builds the addresses and makes the exchanges; the
/// application opens the browser, receives the web callback, and stores the
/// key, which does not expire.
///
/// Every call here is signed, so the client must have been built with an
/// [`ApiSecret`](crate::ApiSecret). None of them changes the account, so a
/// [`Client`] makes them.
///
/// **Desktop.** Ask for a token, send the user to
/// [`approval_url`](Self::approval_url), and exchange the token once they
/// have approved it:
///
/// ```no_run
/// use std::time::Duration;
///
/// # async fn demo(client: scrobl::Client) -> Result<(), scrobl::Error> {
/// let auth = client.auth();
/// let token = auth.token().send().await?.into_value();
/// println!("Approve the sign-in at {}", auth.approval_url(&token));
/// let session = loop {
///     match auth.session(&token).send().await? {
///         Some(session) => break session.into_value(),
///         None => tokio::time::sleep(Duration::from_secs(5)).await,
///     }
/// };
/// let client = client.with_session(session.into_key());
/// # let _ = client;
/// # Ok(())
/// # }
/// ```
///
/// **Web.** Send the user to [`web_approval_url`](Self::web_approval_url).
/// Last.fm sends them back to the callback with `token` in the query, and
/// [`session`](Self::session) exchanges it.
///
/// **Mobile.** [`mobile_session`](Self::mobile_session) exchanges the user's
/// name and password directly.
#[derive(Debug, Clone)]
pub struct AuthHandle {
    client: Client,
}

impl AuthHandle {
    pub(super) fn new(client: Client) -> Self {
        Self { client }
    }

    /// A token for the user to approve, for the desktop flow. It lasts an
    /// hour.
    #[doc(alias = "auth.getToken")]
    pub fn token(&self) -> AuthTokenQuery {
        AuthTokenQuery {
            client: self.client.clone(),
        }
    }

    /// The address where the user approves `token`, to open in their
    /// browser. It holds the API key and the token.
    pub fn approval_url(&self, token: &AuthToken) -> String {
        self.address(&[("token", token.expose())])
    }

    /// The address where the user approves a sign-in to a web application.
    /// Last.fm then sends them to `callback` with `token` added to its
    /// query, or to the callback registered with the API account when
    /// `callback` is `None`. It holds the API key.
    pub fn web_approval_url(&self, callback: Option<&str>) -> String {
        match callback {
            Some(callback) => self.address(&[("cb", callback)]),
            None => self.address(&[]),
        }
    }

    /// Exchanges a token the user has approved for their session. Spends
    /// the token.
    #[doc(alias = "auth.getSession")]
    pub fn session(&self, token: &AuthToken) -> AuthSessionQuery {
        AuthSessionQuery {
            client: self.client.clone(),
            token: token.clone(),
        }
    }

    /// Exchanges a user's name and password for their session, for an
    /// application that asks for them itself. The password is sent over
    /// HTTPS in a request body, as the service requires.
    #[doc(alias = "auth.getMobileSession")]
    pub fn mobile_session(
        &self,
        username: impl Into<String>,
        password: Password,
    ) -> AuthMobileSessionQuery {
        AuthMobileSessionQuery {
            client: self.client.clone(),
            username: username.into(),
            password,
        }
    }

    fn address(&self, more: &[(&str, &str)]) -> String {
        let key = self.client.credentials.api_key().expose();
        let query = form_urlencoded::Serializer::new(String::new())
            .append_pair("api_key", key)
            .extend_pairs(more)
            .finish();
        format!("{APPROVAL}?{query}")
    }
}

/// A token for the user to approve. Made by [`AuthHandle::token`].
#[derive(Debug, Clone)]
#[must_use = "a query does nothing until it is sent"]
pub struct AuthTokenQuery {
    client: Client,
}

impl AuthTokenQuery {
    /// Makes the call. The exact response is on the result, next to the
    /// token.
    ///
    /// # Errors
    ///
    /// [`ErrorKind::InvalidRequest`](crate::ErrorKind), before anything is
    /// sent, when the client has no API secret. Any error of
    /// [`Client::call`]. [`ErrorKind::Decode`](crate::ErrorKind) when the
    /// reply holds no token.
    pub async fn send(self) -> Result<Response<AuthToken>, Error> {
        let raw = self
            .client
            .call(&Request::new(&methods::AUTH_GET_TOKEN))
            .await?;
        let token = decode::auth_get_token(&raw)?;
        Ok(Response::new(token, raw))
    }
}

/// The exchange of a token for a session. Made by [`AuthHandle::session`].
///
/// `Debug` does not show the token.
#[derive(Debug, Clone)]
#[must_use = "a query does nothing until it is sent"]
pub struct AuthSessionQuery {
    client: Client,
    token: AuthToken,
}

impl AuthSessionQuery {
    /// Makes the call. `None` means the user has not approved the token
    /// yet, which is an ordinary answer: the token is not spent, and the
    /// same call can be made again.
    ///
    /// A failure is never attempted again once the request may have reached
    /// the service, because an exchange that went through spends the token.
    ///
    /// # Errors
    ///
    /// [`ErrorKind::InvalidRequest`](crate::ErrorKind), before anything is
    /// sent, when the token is empty or the client has no API secret. Any
    /// error of [`Client::call`], including
    /// [`AUTHENTICATION_FAILED`](ApiErrorCode::AUTHENTICATION_FAILED), with
    /// HTTP 403, for a token already spent or one the service never issued,
    /// and [`TOKEN_EXPIRED`](ApiErrorCode::TOKEN_EXPIRED) for one over an
    /// hour old. [`ErrorKind::Decode`](crate::ErrorKind)
    /// when the reply is not a session.
    pub async fn send(self) -> Result<Option<Response<Session>>, Error> {
        let spec = &methods::AUTH_GET_SESSION;
        // A web callback that arrived without a token gives an empty one.
        refuse_blank(spec, self.token.is_blank(), "the token is empty")?;
        let request = Request::new(spec).param("token", self.token.expose());
        match self.client.call(&request).await {
            Ok(raw) => {
                let session = decode::auth_get_session(&raw)?;
                Ok(Some(Response::new(session, raw)))
            }
            Err(error) if error.api_code() == Some(ApiErrorCode::UNAUTHORIZED_TOKEN) => Ok(None),
            Err(error) => Err(error),
        }
    }
}

/// The exchange of a user's name and password for a session. Made by
/// [`AuthHandle::mobile_session`].
///
/// `Debug` does not show the password.
#[derive(Debug, Clone)]
#[must_use = "a query does nothing until it is sent"]
pub struct AuthMobileSessionQuery {
    client: Client,
    username: String,
    password: Password,
}

impl AuthMobileSessionQuery {
    /// Makes the call. The exact response is on the result, next to the
    /// session.
    ///
    /// # Errors
    ///
    /// [`ErrorKind::InvalidRequest`](crate::ErrorKind), before anything is
    /// sent, when the name or the password is empty or the client has no
    /// API secret. Any error of [`Client::call`], including
    /// [`AUTHENTICATION_FAILED`](ApiErrorCode::AUTHENTICATION_FAILED) for a
    /// wrong name or password. [`ErrorKind::Decode`](crate::ErrorKind) when
    /// the reply is not a session.
    pub async fn send(self) -> Result<Response<Session>, Error> {
        let spec = &methods::AUTH_GET_MOBILE_SESSION;
        // An empty field of a sign-in form.
        refuse_blank(
            spec,
            self.username.trim().is_empty(),
            "the username is empty",
        )?;
        refuse_blank(
            spec,
            self.password.expose().is_empty(),
            "the password is empty",
        )?;
        let request = Request::new(spec)
            .param("username", &self.username)
            .param("password", self.password.expose());
        let raw = self.client.call(&request).await?;
        let session = decode::auth_get_mobile_session(&raw)?;
        Ok(Response::new(session, raw))
    }
}

/// Refuses the call before anything is sent when `blank`. The service would
/// refuse it too, with a message this library never prints.
fn refuse_blank(spec: &'static MethodSpec, blank: bool, problem: &str) -> Result<(), Error> {
    if blank {
        Err(Error::invalid_request(spec, problem))
    } else {
        Ok(())
    }
}
