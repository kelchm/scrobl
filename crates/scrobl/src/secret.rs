//! Secrets that must not leak through logs, errors or serialization.
//!
//! The three types here wrap a string and nothing more. `Debug` prints a
//! fixed redaction, there is no `Display`, no `Serialize` and no
//! `PartialEq`, so a secret reaches the outside only through an explicit
//! call to `expose`.
//!
//! ```
//! use scrobl::ApiKey;
//!
//! let key = ApiKey::new("not-a-real-key");
//! assert_eq!(format!("{key:?}"), "ApiKey(<redacted>)");
//! assert_eq!(key.expose(), "not-a-real-key");
//! ```

macro_rules! secret {
    ($(#[$doc:meta])* $name:ident) => {
        $(#[$doc])*
        #[derive(Clone)]
        pub struct $name(String);

        impl $name {
            /// Wraps a secret.
            pub fn new(value: impl Into<String>) -> Self {
                Self(value.into())
            }

            /// The secret itself. Take care what you do with it.
            pub fn expose(&self) -> &str {
                &self.0
            }

            /// Whether it is empty or only whitespace, without showing it.
            #[cfg(feature = "client")]
            pub(crate) fn is_blank(&self) -> bool {
                self.0.trim().is_empty()
            }
        }

        impl std::fmt::Debug for $name {
            fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                f.write_str(concat!(stringify!($name), "(<redacted>)"))
            }
        }
    };
}

secret! {
    /// The API key Last.fm issued to the application.
    ApiKey
}

secret! {
    /// The shared secret that goes with an [`ApiKey`]. It signs requests and
    /// is never sent.
    ApiSecret
}

secret! {
    /// A user's session key, as returned by the authentication flows.
    SessionKey
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn expose_returns_the_wrapped_value() {
        assert_eq!(ApiKey::new("k").expose(), "k");
        assert_eq!(ApiSecret::new(String::from("s")).expose(), "s");
        assert_eq!(SessionKey::new("sk").expose(), "sk");
    }

    #[cfg(feature = "client")]
    #[test]
    fn a_secret_that_is_empty_or_only_whitespace_is_blank() {
        for blank in ["", " ", "\t\n ", "\u{a0}"] {
            assert!(ApiKey::new(blank).is_blank(), "{blank:?}");
            assert!(ApiSecret::new(blank).is_blank());
            assert!(SessionKey::new(blank).is_blank());
        }
        assert!(!ApiKey::new("k").is_blank());
        assert!(!ApiKey::new(" k ").is_blank());
    }

    #[test]
    fn debug_is_a_fixed_redaction() {
        const SENTINEL: &str = "SENTINEL_API_KEY_0001";
        assert_eq!(format!("{:?}", ApiKey::new(SENTINEL)), "ApiKey(<redacted>)");
        assert_eq!(
            format!("{:?}", ApiSecret::new(SENTINEL)),
            "ApiSecret(<redacted>)"
        );
        assert_eq!(
            format!("{:#?}", SessionKey::new(SENTINEL)),
            "SessionKey(<redacted>)"
        );
        assert_eq!(
            format!("{:?}", Some(vec![ApiKey::new(SENTINEL)])),
            "Some([ApiKey(<redacted>)])"
        );
    }

    #[test]
    fn clones_are_equal_in_content() {
        let key = ApiKey::new("k");
        assert_eq!(key.clone().expose(), key.expose());
    }
}
