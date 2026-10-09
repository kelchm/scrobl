//! What the transport error that caused a failure said, kept as text and
//! scrubbed of anything the client must not repeat.

use std::cmp::Reverse;
use std::error::Error as StdError;
use std::fmt;

use crate::error::truncate_utf8;
use crate::protocol::{Credentials, HttpRequest};

/// The most of one message that is kept.
const MAX_MESSAGE: usize = 256;

/// What replaces text that was scrubbed.
const REDACTED: &str = "<redacted>";

/// Text to remove from a message, whatever the error that carries it prints.
///
/// The messages of the errors under the HTTP client are written by libraries
/// and today hold nothing of ours, but some dependency errors include bytes
/// or host names a peer chose, and one may echo the request URL. So the
/// client does not rely on what they happen to print: every credential it
/// holds, and the whole query string and body of the request it sent, are
/// removed from every message before it is kept.
pub(super) struct Redactor {
    /// Longest first, so the longest match at a position wins.
    needles: Vec<String>,
}

impl Redactor {
    /// Scrubs these texts. Control characters are dropped from them too, as
    /// they are from the messages, and empty ones are ignored.
    pub(super) fn from_needles<'a>(needles: impl IntoIterator<Item = &'a str>) -> Self {
        let mut needles: Vec<String> = needles
            .into_iter()
            .map(without_controls)
            .filter(|needle| !needle.is_empty())
            .collect();
        needles.sort_by(|a, b| (Reverse(a.len()), a).cmp(&(Reverse(b.len()), b)));
        needles.dedup();
        Self { needles }
    }

    /// Scrubs what this client holds and what this request carried: each
    /// credential as it is and as a form encodes it, and the request's query
    /// string and body.
    pub(super) fn for_request(credentials: &Credentials, request: &HttpRequest) -> Self {
        let mut needles: Vec<String> = Vec::new();
        for value in credentials.exposed() {
            let encoded: String = form_urlencoded::byte_serialize(value.as_bytes()).collect();
            // A space is `+` in a form and `%20` in some URL writers.
            needles.push(encoded.replace('+', "%20"));
            needles.push(encoded);
            needles.push(value.to_owned());
        }
        if let Some((_, query)) = request.url().split_once('?') {
            needles.push(query.to_owned());
        }
        if let Some(body) = request.body() {
            needles.push(String::from_utf8_lossy(body).into_owned());
        }
        Self::from_needles(needles.iter().map(String::as_str))
    }

    /// `message` without control characters or any needle, cut to the cap.
    ///
    /// Controls go first, so they cannot be used to split a needle that
    /// would then fail to match; the cut comes last, so it cannot leave half
    /// of a credential behind.
    fn scrub(&self, message: &str) -> String {
        let clean = without_controls(message);
        let mut out = String::with_capacity(clean.len());
        let mut rest = clean.as_str();
        while !rest.is_empty() {
            if let Some(after) = self
                .needles
                .iter()
                .find_map(|needle| rest.strip_prefix(needle.as_str()))
            {
                out.push_str(REDACTED);
                rest = after;
            } else {
                let mut chars = rest.chars();
                if let Some(next) = chars.next() {
                    out.push(next);
                }
                rest = chars.as_str();
            }
        }
        truncate_utf8(&out, MAX_MESSAGE).to_owned()
    }
}

fn without_controls(text: &str) -> String {
    text.chars().filter(|c| !c.is_control()).collect()
}

/// The messages of an error and of every error it was caused by.
///
/// The `Debug` output of an error from the HTTP stack can print more than
/// its `Display` does, such as the address a connection was made to. Keeping
/// only the `Display` of each link, scrubbed by a [`Redactor`], makes what a
/// [`crate::Error`] exposes on its [`source`](StdError::source) chain the
/// same under `{}` and `{:?}`, and free of the credentials the client holds.
#[derive(Debug)]
pub(super) struct Cause {
    message: String,
    source: Option<Box<Cause>>,
}

impl Cause {
    /// A snapshot of `error` and its sources.
    pub(super) fn of(error: &(dyn StdError + 'static), redactor: &Redactor) -> Self {
        let mut messages = Vec::new();
        let mut link = Some(error);
        while let Some(current) = link {
            messages.push(redactor.scrub(&current.to_string()));
            link = current.source();
        }
        let mut cause = None;
        for message in messages.into_iter().rev() {
            cause = Some(Box::new(Self {
                message,
                source: cause,
            }));
        }
        // The chain has at least the error itself.
        *cause.unwrap_or_else(|| {
            Box::new(Self {
                message: String::new(),
                source: None,
            })
        })
    }
}

impl fmt::Display for Cause {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.message)
    }
}

impl StdError for Cause {
    fn source(&self) -> Option<&(dyn StdError + 'static)> {
        self.source
            .as_deref()
            .map(|source| source as &(dyn StdError + 'static))
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]

    use std::error::Error as StdError;
    use std::fmt;

    use super::*;

    /// An error whose message and source are whatever a test says.
    #[derive(Debug)]
    struct Fake {
        message: String,
        source: Option<Box<Fake>>,
    }

    impl fmt::Display for Fake {
        fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
            f.write_str(&self.message)
        }
    }

    impl StdError for Fake {
        fn source(&self) -> Option<&(dyn StdError + 'static)> {
            self.source
                .as_deref()
                .map(|source| source as &(dyn StdError + 'static))
        }
    }

    fn chain(messages: &[&str]) -> Fake {
        let mut error = None;
        for message in messages.iter().rev() {
            error = Some(Fake {
                message: (*message).to_owned(),
                source: error.map(Box::new),
            });
        }
        error.unwrap()
    }

    fn texts(cause: &Cause) -> Vec<String> {
        let mut out = Vec::new();
        let mut link: Option<&(dyn StdError + 'static)> = Some(cause);
        while let Some(current) = link {
            out.push(current.to_string());
            out.push(format!("{current:?}"));
            link = current.source();
        }
        out
    }

    const KEY: &str = "SENTINEL_API_KEY_0001";
    const SECRET: &str = "SENTINEL_API_SECRET_0002";
    const SESSION: &str = "SENTINEL_SESSION_KEY_0003";
    const TAIL: &str = "api_key=SENTINEL_API_KEY_0001&method=user.getRecentTracks&format=json";

    fn redactor() -> Redactor {
        Redactor::from_needles([KEY, SECRET, SESSION, TAIL, "sk=a+b%2Fc&body=1"])
    }

    #[test]
    fn every_credential_the_client_holds_is_scrubbed_from_every_link() {
        let error = chain(&[
            &format!("error sending request for {KEY}"),
            &format!("connect to https://host.invalid/2.0/?{TAIL} failed"),
            &format!("{SECRET} then {SESSION} then {KEY}{KEY}"),
            "the request was sk=a+b%2Fc&body=1",
        ]);
        let cause = Cause::of(&error, &redactor());
        let shown = texts(&cause);
        assert_eq!(shown.len(), 8, "four links, shown twice each");
        for text in shown {
            for sentinel in [KEY, SECRET, SESSION, "api_key=", "sk=a+b%2Fc", "SENTINEL"] {
                assert!(!text.contains(sentinel), "`{sentinel}` in {text}");
            }
        }
        // What is left says what happened.
        assert_eq!(cause.to_string(), "error sending request for <redacted>");
        assert_eq!(
            cause.source().unwrap().to_string(),
            "connect to https://host.invalid/2.0/?<redacted> failed"
        );
    }

    #[test]
    fn the_longer_match_wins_and_overlaps_leave_nothing_behind() {
        let redactor = Redactor::from_needles(["KEY", "KEYKEY2", "ABC"]);
        let cause = Cause::of(&chain(&["KEYKEY2 KEYABC ABCABC KEY"]), &redactor);
        assert_eq!(
            cause.to_string(),
            "<redacted> <redacted><redacted> <redacted><redacted> <redacted>"
        );
    }

    #[test]
    fn control_characters_are_stripped_and_cannot_hide_a_credential() {
        let split = format!("{}\u{0}\n{}", &KEY[..8], &KEY[8..]);
        let message = format!("line one\r\nline two\ttab\u{1b}[31m {split} \u{7f}end");
        let cause = Cause::of(&chain(&[&message]), &redactor());
        let shown = cause.to_string();
        assert!(!shown.contains("SENTINEL"), "{shown}");
        assert!(
            shown.chars().all(|c| !c.is_control()),
            "control character in {shown:?}"
        );
        assert_eq!(shown, "line oneline twotab[31m <redacted> end");
    }

    #[test]
    fn a_credential_across_the_length_cap_is_gone_not_cut() {
        for padding in 240..=256 {
            let message = format!("{}{KEY}{}", "x".repeat(padding), "y".repeat(40));
            let cause = Cause::of(&chain(&[&message]), &redactor());
            let shown = cause.to_string();
            assert!(shown.len() <= MAX_MESSAGE, "{} bytes", shown.len());
            assert!(!shown.contains("SENTINEL"), "padding {padding}: {shown}");
            assert!(!shown.contains("SENTI"), "padding {padding}: {shown}");
        }
    }

    #[test]
    fn a_long_message_is_cut_on_a_character_boundary() {
        let cause = Cause::of(&chain(&[&"\u{e9}".repeat(300)]), &redactor());
        assert!(cause.to_string().len() <= MAX_MESSAGE);
        assert!(cause.to_string().chars().all(|c| c == '\u{e9}'));
    }

    #[test]
    fn nothing_to_scrub_changes_nothing_and_empty_needles_are_ignored() {
        let redactor = Redactor::from_needles(["", KEY]);
        let cause = Cause::of(&chain(&["plain", "chain"]), &redactor);
        assert_eq!(cause.to_string(), "plain");
        assert_eq!(cause.source().unwrap().to_string(), "chain");
        assert!(cause.source().unwrap().source().is_none());
    }

    #[test]
    fn a_prepared_request_is_scrubbed_in_every_spelling_it_can_be_echoed_in() {
        use crate::protocol::{Request, methods, prepare};
        use crate::secret::{ApiKey, ApiSecret, SessionKey};

        let credentials = Credentials::new(ApiKey::new("SENTINEL_API_KEY_0001"))
            .allow_writes()
            .with_secret(ApiSecret::new("SENTINEL_API_SECRET_0002"))
            .with_session(SessionKey::new("SENTINEL SESSION/0003+x&y"));
        let read = prepare(
            &credentials,
            &Request::new(&methods::USER_GET_RECENT_TRACKS)
                .param("user", "rj")
                .as_user(),
        )
        .unwrap();
        let write = prepare(
            &credentials,
            &Request::new(&methods::TRACK_LOVE)
                .param("track", "Synthetic Track")
                .param("artist", "Synthetic Artist"),
        )
        .unwrap();
        assert!(read.url().contains('?'));

        for request in [&read, &write] {
            let body = request
                .body()
                .map(|body| String::from_utf8_lossy(body).into_owned())
                .unwrap_or_default();
            let redactor = Redactor::for_request(&credentials, request);
            let echoed = [
                request.url().to_owned(),
                body,
                "SENTINEL_API_KEY_0001 SENTINEL_API_SECRET_0002".to_owned(),
                "SENTINEL SESSION/0003+x&y".to_owned(),
                "SENTINEL+SESSION%2F0003%2Bx%26y".to_owned(),
                "SENTINEL%20SESSION%2F0003%2Bx%26y".to_owned(),
            ]
            .join(" | ");
            let shown = Cause::of(&chain(&[&echoed]), &redactor).to_string();
            for sentinel in ["SENTINEL", "SESSION", "api_sig=", "api_key="] {
                assert!(!shown.contains(sentinel), "`{sentinel}` in {shown}");
            }
            assert!(shown.contains(REDACTED));
        }
    }
}
