//! Signing a user in.

use super::Client;
use crate::error::{ApiErrorCode, Error};
use crate::model::{Session, decode};
use crate::protocol::{MethodSpec, Request, methods};
use crate::response::Response;
use crate::secret::{AuthToken, Password};

const APPROVAL: &str = "https://www.last.fm/api/auth/";

/// Signing a user in. Made by [`Client::auth`]; the client needs an
/// [`ApiSecret`](crate::ApiSecret).
///
/// ```no_run
/// # async fn demo(client: scrobl::Client) -> Result<(), scrobl::Error> {
/// let auth = client.auth();
/// let token = auth.token().send().await?.into_value();
/// println!("Approve the sign-in at {}", auth.approval_url(&token));
/// let session = loop {
///     match auth.session(&token).send().await? {
///         Some(session) => break session.into_value(),
///         None => tokio::time::sleep(std::time::Duration::from_secs(5)).await,
///     }
/// };
/// let client = client.with_session(session.into_key());
/// # let _ = client;
/// # Ok(())
/// # }
/// ```
#[derive(Debug, Clone)]
pub struct AuthHandle {
    client: Client,
}

impl AuthHandle {
    pub(super) fn new(client: Client) -> Self {
        Self { client }
    }

    /// A token for the user to approve. It lasts an hour.
    #[doc(alias = "auth.getToken")]
    pub fn token(&self) -> AuthTokenQuery {
        AuthTokenQuery {
            client: self.client.clone(),
        }
    }

    /// The address where the user approves `token`.
    pub fn approval_url(&self, token: &AuthToken) -> String {
        self.address(&[("token", token.expose())])
    }

    /// The address where the user approves a web sign-in. Last.fm returns
    /// them to `callback`, or to the one registered for the API key, with
    /// `token` in the query.
    pub fn web_approval_url(&self, callback: Option<&str>) -> String {
        match callback {
            Some(callback) => self.address(&[("cb", callback)]),
            None => self.address(&[]),
        }
    }

    /// Exchanges an approved token for the user's session.
    #[doc(alias = "auth.getSession")]
    pub fn session(&self, token: &AuthToken) -> AuthSessionQuery {
        AuthSessionQuery {
            client: self.client.clone(),
            token: token.clone(),
        }
    }

    /// Exchanges a user's name and password for their session.
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

/// Made by [`AuthHandle::token`].
#[derive(Debug, Clone)]
#[must_use = "a query does nothing until it is sent"]
pub struct AuthTokenQuery {
    client: Client,
}

impl AuthTokenQuery {
    /// Makes the call.
    ///
    /// # Errors
    ///
    /// Any error of [`Client::call`], and
    /// [`ErrorKind::Decode`](crate::ErrorKind) for a reply with no token.
    pub async fn send(self) -> Result<Response<AuthToken>, Error> {
        let raw = self
            .client
            .call(&Request::new(&methods::AUTH_GET_TOKEN))
            .await?;
        let token = decode::auth_get_token(&raw)?;
        Ok(Response::new(token, raw))
    }
}

/// Made by [`AuthHandle::session`].
#[derive(Debug, Clone)]
#[must_use = "a query does nothing until it is sent"]
pub struct AuthSessionQuery {
    client: Client,
    token: AuthToken,
}

impl AuthSessionQuery {
    /// Makes the call. `None` while the user has not approved the token;
    /// the same call can be made again.
    ///
    /// # Errors
    ///
    /// [`ErrorKind::InvalidRequest`](crate::ErrorKind) for an empty token.
    /// Any error of [`Client::call`]: a spent or unknown token is
    /// [`AUTHENTICATION_FAILED`](ApiErrorCode::AUTHENTICATION_FAILED), one
    /// over an hour old [`TOKEN_EXPIRED`](ApiErrorCode::TOKEN_EXPIRED).
    pub async fn send(self) -> Result<Option<Response<Session>>, Error> {
        let spec = &methods::AUTH_GET_SESSION;
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

/// Made by [`AuthHandle::mobile_session`].
#[derive(Debug, Clone)]
#[must_use = "a query does nothing until it is sent"]
pub struct AuthMobileSessionQuery {
    client: Client,
    username: String,
    password: Password,
}

impl AuthMobileSessionQuery {
    /// Makes the call, as a POST.
    ///
    /// # Errors
    ///
    /// [`ErrorKind::InvalidRequest`](crate::ErrorKind) for an empty name or
    /// password. Any error of [`Client::call`]: a wrong name or password is
    /// [`AUTHENTICATION_FAILED`](ApiErrorCode::AUTHENTICATION_FAILED).
    pub async fn send(self) -> Result<Response<Session>, Error> {
        let spec = &methods::AUTH_GET_MOBILE_SESSION;
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

fn refuse_blank(spec: &'static MethodSpec, blank: bool, problem: &str) -> Result<(), Error> {
    if blank {
        Err(Error::invalid_request(spec, problem))
    } else {
        Ok(())
    }
}
