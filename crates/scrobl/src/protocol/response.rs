//! Reading the response to a method call: [`decode`] turns an
//! [`HttpResponse`] into a [`Raw`], or into the error it carries.

use std::fmt;

use bytes::Bytes;
use serde::Deserialize;
use serde::de::{DeserializeOwned, IgnoredAny, MapAccess, SeqAccess, Visitor};
use serde_json::Value;
use serde_json::error::Category;

use super::Request;
use crate::de::StrictError;
use crate::error::{ApiErrorCode, Error, truncate_utf8};
use crate::protocol::MethodSpec;

/// The response headers that are kept. Others are dropped.
const KEPT_HEADERS: [&str; 3] = ["content-type", "retry-after", "date"];

/// The longest header value that is kept.
const MAX_HEADER_VALUE: usize = 256;

/// A response as a transport received it: status, body and a few headers.
#[derive(Clone)]
pub struct HttpResponse {
    status: u16,
    body: Bytes,
    headers: Vec<(&'static str, String)>,
}

impl HttpResponse {
    /// A response with a status and the exact body bytes.
    ///
    /// ```
    /// use scrobl::protocol::HttpResponse;
    ///
    /// let response = HttpResponse::new(200, r#"{"ok":true}"#)
    ///     .with_header("Content-Type", "application/json")
    ///     .with_header("Set-Cookie", "dropped");
    /// assert_eq!(
    ///     format!("{response:?}"),
    ///     r#"HttpResponse { status: 200, body_len: 11, headers: [("content-type", "application/json")] }"#
    /// );
    /// ```
    pub fn new(status: u16, body: impl Into<Bytes>) -> Self {
        Self {
            status,
            body: body.into(),
            headers: Vec::new(),
        }
    }

    /// Records a header. Only `content-type`, `retry-after` and `date` are
    /// kept, matched without regard to case; any other header is ignored. A
    /// value longer than 256 bytes is cut.
    #[must_use]
    pub fn with_header(mut self, name: &str, value: &str) -> Self {
        let Some(kept) = KEPT_HEADERS
            .iter()
            .find(|kept| kept.eq_ignore_ascii_case(name))
        else {
            return self;
        };
        let value = truncate_utf8(value, MAX_HEADER_VALUE).to_owned();
        self.headers.retain(|(existing, _)| existing != kept);
        self.headers.push((kept, value));
        self
    }
}

impl fmt::Debug for HttpResponse {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("HttpResponse")
            .field("status", &self.status)
            .field("body_len", &self.body.len())
            .field("headers", &self.headers)
            .finish()
    }
}

/// A successful response: the status, a few headers, the exact body bytes
/// and the [`Request`] it answers.
///
/// Typed views are decoded from it and never replace it. Its `Debug` output
/// leaves the body and the request's parameters out, because some responses
/// carry a session key.
#[derive(Clone)]
pub struct Raw {
    request: Request,
    status: u16,
    headers: Vec<(&'static str, String)>,
    body: Bytes,
}

impl Raw {
    /// The HTTP status, always 2xx.
    pub fn status(&self) -> u16 {
        self.status
    }

    /// The body, byte for byte as received.
    pub fn body(&self) -> &[u8] {
        &self.body
    }

    /// A header value, matched without regard to case. Only `content-type`,
    /// `retry-after` and `date` are available.
    pub fn header(&self, name: &str) -> Option<&str> {
        self.headers
            .iter()
            .find(|(kept, _)| kept.eq_ignore_ascii_case(name))
            .map(|(_, value)| value.as_str())
    }

    /// The Last.fm method the response is for.
    pub fn method(&self) -> &'static str {
        self.request.method()
    }

    /// The request this response answers, as it was given to [`decode`].
    /// A consumer that pairs responses with requests compares it to the
    /// one it sent. Its `Debug` hides credential values, which `Raw`'s own
    /// does not print at all.
    pub fn request(&self) -> &Request {
        &self.request
    }

    pub(crate) fn spec(&self) -> &'static MethodSpec {
        self.request.spec()
    }

    /// Deserializes the body.
    ///
    /// Fails with [`ErrorKind::Decode`](crate::ErrorKind). The error says
    /// where the JSON went wrong but never repeats text from the body.
    pub fn json<T: DeserializeOwned>(&self) -> Result<T, Error> {
        serde_json::from_slice(&self.body).map_err(|e| {
            Error::decode(&describe(&e))
                .with_method(self.spec())
                .with_response(self.status, &self.body)
        })
    }
}

impl Raw {
    /// The body as a [`Value`], failing on an object that repeats a member
    /// name at any depth. [`json`](Self::json) would keep the last of the two
    /// and say nothing. Neither error repeats text from the body.
    pub(crate) fn json_value_strict(&self) -> Result<Value, Error> {
        crate::de::strict_value(&self.body).map_err(|e| {
            let problem = match e {
                StrictError::Duplicate { line, column } => {
                    format!("an object repeats a member name, at line {line} column {column}")
                }
                StrictError::Json(e) => describe(&e),
            };
            Error::decode(&problem)
                .with_method(self.spec())
                .with_response(self.status, &self.body)
        })
    }
}

impl fmt::Debug for Raw {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Raw")
            .field("method", &self.method())
            .field("status", &self.status)
            .field("headers", &self.headers)
            .field("body_len", &self.body.len())
            .finish()
    }
}

/// What went wrong with some JSON, without any of the JSON's own text.
fn describe(e: &serde_json::Error) -> String {
    let place = format!("line {} column {}", e.line(), e.column());
    match e.classify() {
        Category::Eof => format!("the JSON ends early, at {place}"),
        Category::Syntax | Category::Io => format!("the body is not valid JSON, at {place}"),
        // The text of a data error can quote the body, and a caller's own
        // `Deserialize` impl can put anything in it.
        Category::Data => format!("the JSON does not have the expected shape, at {place}"),
    }
}

/// Reads the response to `request`. A [`Raw`] keeps a copy of `request`, so
/// that whoever holds the response can check what it answers.
///
/// Never panics, whatever the bytes.
///
/// # Errors
///
/// Last.fm reports failures in a JSON envelope, `{"error": 6, "message":
/// ".."}`, and does not tie it to the HTTP status: an error can arrive with
/// status 200, and a 404 can carry a perfectly good answer to the question
/// "no such user". So the envelope is looked for first, at every status.
///
/// - An envelope gives [`ErrorKind::Api`](crate::ErrorKind) with the code,
///   message, HTTP status, method and body.
/// - No envelope and a status outside 200 to 299 gives
///   [`ErrorKind::Http`](crate::ErrorKind).
/// - No envelope, a 2xx status and a body that is not JSON gives
///   [`ErrorKind::Decode`](crate::ErrorKind).
/// - Anything else is a [`Raw`].
///
/// An envelope is a top-level object whose `error` member is an integer or a
/// string of digits. A successful payload with an `error` key nested
/// somewhere inside is not mistaken for one. A body with more than one
/// top-level `error` member is ambiguous and is a
/// [`ErrorKind::Decode`](crate::ErrorKind) at every status.
///
/// ```
/// use scrobl::protocol::{self, HttpResponse, Request, methods};
/// use scrobl::{ApiErrorCode, ErrorKind};
///
/// let request = Request::new(&methods::USER_GET_INFO).param("user", "nobody");
/// // An error can arrive with HTTP 200.
/// let response = HttpResponse::new(200, r#"{"error":6,"message":"User not found"}"#);
/// let Err(error) = protocol::decode(&request, response) else {
///     unreachable!()
/// };
/// assert_eq!(error.kind(), ErrorKind::Api);
/// assert_eq!(error.api_code(), Some(ApiErrorCode::INVALID_PARAMETERS));
/// assert_eq!(error.api_message(), Some("User not found"));
/// ```
pub fn decode(request: &Request, response: HttpResponse) -> Result<Raw, Error> {
    let spec = request.spec();
    let HttpResponse {
        status,
        body,
        headers,
    } = response;
    let success = (200..300).contains(&status);

    // JSON is UTF-8. The probe skips most of the document without looking at
    // its strings, so the whole body is checked here.
    let probe = std::str::from_utf8(&body)
        .ok()
        .and_then(|text| serde_json::from_str::<Probe>(text).ok());

    match probe {
        Some(Probe::Ambiguous) => Err(Error::decode("the body has more than one `error` member")
            .with_method(spec)
            .with_response(status, &body)),
        Some(Probe::Envelope(envelope)) => Err(Error::api(
            spec,
            status,
            ApiErrorCode::new(envelope.code),
            envelope.message.as_deref(),
            &body,
        )),
        Some(Probe::Plain) if success => Ok(Raw {
            request: request.clone(),
            status,
            headers,
            body,
        }),
        _ if !success => Err(Error::http(spec, status, &body)),
        _ => Err(Error::decode("the body is not valid JSON")
            .with_method(spec)
            .with_response(status, &body)),
    }
}

/// The Last.fm error envelope, if a body holds one.
enum Probe {
    /// Not an envelope.
    Plain,
    Envelope(Envelope),
    /// More than one top-level `error` member, so no reading is trustworthy.
    Ambiguous,
}

struct Envelope {
    code: u32,
    message: Option<String>,
}

/// The only two members of a response that `decode` reads.
#[derive(Deserialize)]
#[serde(field_identifier, rename_all = "lowercase")]
enum Field {
    Error,
    Message,
    #[serde(other)]
    Other,
}

/// A code Last.fm could have sent: an integer, or a string of digits.
/// Anything else, however it is shaped, reads as no code.
struct Code(Option<u32>);

impl<'de> Deserialize<'de> for Code {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        deserializer.deserialize_any(CodeVisitor)
    }
}

struct CodeVisitor;

impl<'de> Visitor<'de> for CodeVisitor {
    type Value = Code;

    fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("any JSON value")
    }

    fn visit_u64<E>(self, n: u64) -> Result<Code, E> {
        Ok(Code(u32::try_from(n).ok()))
    }

    fn visit_i64<E>(self, n: i64) -> Result<Code, E> {
        Ok(Code(u32::try_from(n).ok()))
    }

    fn visit_str<E>(self, s: &str) -> Result<Code, E> {
        let digits = !s.is_empty() && s.bytes().all(|b| b.is_ascii_digit());
        Ok(Code(if digits { s.parse().ok() } else { None }))
    }

    fn visit_map<A: MapAccess<'de>>(self, mut map: A) -> Result<Code, A::Error> {
        while map.next_entry::<IgnoredAny, IgnoredAny>()?.is_some() {}
        Ok(Code(None))
    }

    fn visit_seq<A: SeqAccess<'de>>(self, mut seq: A) -> Result<Code, A::Error> {
        while seq.next_element::<IgnoredAny>()?.is_some() {}
        Ok(Code(None))
    }

    fn visit_bool<E>(self, _: bool) -> Result<Code, E> {
        Ok(Code(None))
    }

    fn visit_f64<E>(self, _: f64) -> Result<Code, E> {
        Ok(Code(None))
    }

    fn visit_unit<E>(self) -> Result<Code, E> {
        Ok(Code(None))
    }
}

/// A message: a string, or nothing.
struct Message(Option<String>);

impl<'de> Deserialize<'de> for Message {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        deserializer.deserialize_any(MessageVisitor)
    }
}

struct MessageVisitor;

impl<'de> Visitor<'de> for MessageVisitor {
    type Value = Message;

    fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("any JSON value")
    }

    fn visit_str<E>(self, s: &str) -> Result<Message, E> {
        Ok(Message(Some(s.to_owned())))
    }

    fn visit_map<A: MapAccess<'de>>(self, mut map: A) -> Result<Message, A::Error> {
        while map.next_entry::<IgnoredAny, IgnoredAny>()?.is_some() {}
        Ok(Message(None))
    }

    fn visit_seq<A: SeqAccess<'de>>(self, mut seq: A) -> Result<Message, A::Error> {
        while seq.next_element::<IgnoredAny>()?.is_some() {}
        Ok(Message(None))
    }

    fn visit_bool<E>(self, _: bool) -> Result<Message, E> {
        Ok(Message(None))
    }

    fn visit_i64<E>(self, _: i64) -> Result<Message, E> {
        Ok(Message(None))
    }

    fn visit_u64<E>(self, _: u64) -> Result<Message, E> {
        Ok(Message(None))
    }

    fn visit_f64<E>(self, _: f64) -> Result<Message, E> {
        Ok(Message(None))
    }

    fn visit_unit<E>(self) -> Result<Message, E> {
        Ok(Message(None))
    }
}

impl<'de> Deserialize<'de> for Probe {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        deserializer.deserialize_any(ProbeVisitor)
    }
}

/// Reads one JSON document, keeping only a top-level `error` and `message`.
/// Everything else is checked for syntax and skipped, so the same pass also
/// proves the body is JSON.
struct ProbeVisitor;

impl<'de> Visitor<'de> for ProbeVisitor {
    type Value = Probe;

    fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("any JSON value")
    }

    fn visit_map<A: MapAccess<'de>>(self, mut map: A) -> Result<Probe, A::Error> {
        let mut code = None;
        let mut message = None;
        let mut errors = 0_usize;
        while let Some(field) = map.next_key::<Field>()? {
            match field {
                Field::Error => {
                    errors += 1;
                    code = map.next_value::<Code>()?.0;
                }
                Field::Message => {
                    let next = map.next_value::<Message>()?.0;
                    message = message.or(next);
                }
                Field::Other => {
                    map.next_value::<IgnoredAny>()?;
                }
            }
        }
        Ok(match (errors, code) {
            (2.., _) => Probe::Ambiguous,
            (_, Some(code)) => Probe::Envelope(Envelope { code, message }),
            _ => Probe::Plain,
        })
    }

    fn visit_seq<A: SeqAccess<'de>>(self, mut seq: A) -> Result<Probe, A::Error> {
        while seq.next_element::<IgnoredAny>()?.is_some() {}
        Ok(Probe::Plain)
    }

    fn visit_bool<E>(self, _: bool) -> Result<Probe, E> {
        Ok(Probe::Plain)
    }

    fn visit_i64<E>(self, _: i64) -> Result<Probe, E> {
        Ok(Probe::Plain)
    }

    fn visit_u64<E>(self, _: u64) -> Result<Probe, E> {
        Ok(Probe::Plain)
    }

    fn visit_f64<E>(self, _: f64) -> Result<Probe, E> {
        Ok(Probe::Plain)
    }

    fn visit_str<E>(self, _: &str) -> Result<Probe, E> {
        Ok(Probe::Plain)
    }

    fn visit_unit<E>(self) -> Result<Probe, E> {
        Ok(Probe::Plain)
    }
}

#[cfg(test)]
mod tests;
