//! What the transport error that caused a failure said, kept as text.

use std::error::Error as StdError;
use std::fmt;

use crate::error::truncate_utf8;

/// The most of one message that is kept.
const MAX_MESSAGE: usize = 256;

/// The messages of an error and of every error it was caused by.
///
/// The `Debug` output of an error from the HTTP stack can print more than
/// its `Display` does, such as the address a connection was made to. Keeping
/// only the `Display` of each link makes what a [`crate::Error`] exposes on
/// its [`source`](StdError::source) chain the same under `{}` and `{:?}`.
#[derive(Debug)]
pub(super) struct Cause {
    message: String,
    source: Option<Box<Cause>>,
}

impl Cause {
    /// A snapshot of `error` and its sources.
    pub(super) fn of(error: &(dyn StdError + 'static)) -> Self {
        let mut messages = Vec::new();
        let mut link = Some(error);
        while let Some(current) = link {
            messages.push(truncate_utf8(&current.to_string(), MAX_MESSAGE).to_owned());
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
