//! Signing in: the replies to `auth.getToken`, `auth.getSession` and
//! `auth.getMobileSession`.

use std::fmt;

use serde_json::Value;

use super::entity;
use super::shared::{self, missing_or};
use crate::error::Error;
use crate::protocol::{MethodSpec, Raw, methods};
use crate::secret::{AuthToken, SessionKey};

/// A signed-in user: the reply to `auth.getSession` and
/// `auth.getMobileSession`.
///
/// The key is a [`SessionKey`], so `Debug` does not show it. Unlike other
/// models, this one does not keep its JSON, which holds the key in plain
/// text; the exact reply is still on the [`Response`](crate::Response).
///
/// ```
/// use scrobl::model::decode;
/// use scrobl::protocol::{self, HttpResponse, Request, methods};
///
/// let body = r#"{"session": {"name": "rj", "key": "d580d57f32848f5dcf574d1ce18d78b2", "subscriber": 0}}"#;
/// let request = Request::new(&methods::AUTH_GET_SESSION).param("token", "a-token");
/// let raw = protocol::decode(&request, HttpResponse::new(200, body))?;
///
/// let session = decode::auth_get_session(&raw)?;
/// assert_eq!(session.name(), "rj");
/// assert_eq!(session.key().expose(), "d580d57f32848f5dcf574d1ce18d78b2");
/// assert!(!session.subscriber());
/// assert!(!format!("{session:?}").contains("d580d57f"));
/// # Ok::<(), scrobl::Error>(())
/// ```
#[derive(Clone)]
pub struct Session {
    name: String,
    key: SessionKey,
    subscriber: bool,
}

impl Session {
    /// The name of the user who signed in, as the service spells it.
    pub fn name(&self) -> &str {
        &self.name
    }

    /// The session key, which a client acting for the user is made with:
    /// `Client::with_session`. It does not expire; the user can revoke it
    /// on the website.
    pub fn key(&self) -> &SessionKey {
        &self.key
    }

    /// The session key, to keep.
    pub fn into_key(self) -> SessionKey {
        self.key
    }

    /// Whether the user pays for a Last.fm subscription.
    pub fn subscriber(&self) -> bool {
        self.subscriber
    }
}

impl fmt::Debug for Session {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Session")
            .field("name", &self.name)
            .field("key", &self.key)
            .field("subscriber", &self.subscriber)
            .finish()
    }
}

/// Decodes an `auth.getToken` response: a token for the user to approve.
///
/// ```
/// use scrobl::model::decode;
/// use scrobl::protocol::{self, HttpResponse, Request, methods};
///
/// let body = r#"{"token": "cf45fe5a3e3cebe168480a086d7fe481"}"#;
/// let request = Request::new(&methods::AUTH_GET_TOKEN);
/// let raw = protocol::decode(&request, HttpResponse::new(200, body))?;
///
/// let token = decode::auth_get_token(&raw)?;
/// assert_eq!(token.expose(), "cf45fe5a3e3cebe168480a086d7fe481");
/// # Ok::<(), scrobl::Error>(())
/// ```
///
/// # Errors
///
/// [`ErrorKind::Decode`](crate::ErrorKind) when the response is for another
/// method, or `token` is missing, empty or not a string.
pub fn auth_get_token(raw: &Raw) -> Result<AuthToken, Error> {
    let spec = &methods::AUTH_GET_TOKEN;
    if raw.method() != spec.name() {
        return Err(answered(
            raw,
            Error::decode("the response is for a different method"),
        ));
    }
    let parsed = match raw.json_strict()? {
        Value::Object(body) => match body.get("token") {
            Some(Value::String(token)) if !token.is_empty() => Ok(AuthToken::new(token.clone())),
            _ => Err(missing_or("token", "a non-empty string")),
        },
        _ => Err(Error::decode("the body is not a JSON object")),
    };
    parsed.map_err(|e| answered(raw, e))
}

/// Decodes an `auth.getSession` response: the session of the user who
/// approved the token.
///
/// A token the user has not approved yet is answered with an error envelope,
/// which [`protocol::decode`](crate::protocol::decode) has already turned
/// into an error with the code
/// [`UNAUTHORIZED_TOKEN`](crate::ApiErrorCode::UNAUTHORIZED_TOKEN).
///
/// # Errors
///
/// [`ErrorKind::Decode`](crate::ErrorKind) when the response is for another
/// method, or `session` is not an object with a non-empty `name` and `key`
/// and a `subscriber` of 0 or 1.
pub fn auth_get_session(raw: &Raw) -> Result<Session, Error> {
    session(raw, &methods::AUTH_GET_SESSION)
}

/// Decodes an `auth.getMobileSession` response: the session of the user
/// whose name and password were sent.
///
/// # Errors
///
/// As [`auth_get_session`].
pub fn auth_get_mobile_session(raw: &Raw) -> Result<Session, Error> {
    session(raw, &methods::AUTH_GET_MOBILE_SESSION)
}

fn session(raw: &Raw, spec: &'static MethodSpec) -> Result<Session, Error> {
    shared::decode(raw, spec, "session", |session| {
        let name = shared::required_str(&session, "session", "name")?;
        let key = shared::required_str(&session, "session", "key")?;
        if name.is_empty() {
            return Err(missing_or("session.name", "a non-empty string"));
        }
        if key.is_empty() {
            return Err(missing_or("session.key", "a non-empty string"));
        }
        let subscriber = entity::optional_flag(&session, "session", "subscriber")?
            .ok_or_else(|| missing_or("session.subscriber", "a 0/1 flag"))?;
        Ok(Session {
            name: name.to_owned(),
            key: SessionKey::new(key),
            subscriber,
        })
    })
}

/// `error`, with the method and the response it came from.
fn answered(raw: &Raw, error: Error) -> Error {
    error
        .with_method(raw.spec())
        .with_response(raw.status(), raw.body())
}

#[cfg(test)]
mod tests;
