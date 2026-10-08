//! Request signatures.
//!
//! Last.fm authenticates a call by an `api_sig` parameter: an MD5 digest over
//! the call's parameters and the API secret. [`prepare`](super::prepare)
//! computes it for the methods that need one. [`sign`] is public for callers
//! that build requests with their own HTTP stack.

use std::fmt::Write as _;

use md5::{Digest, Md5};

use crate::secret::ApiSecret;

/// Parameters that are sent but never signed.
const UNSIGNED: [&str; 3] = ["format", "callback", "api_sig"];

/// The `api_sig` value for a call, as 32 lowercase hexadecimal characters.
///
/// `params` is every parameter sent, as raw (not URL-encoded) name and value
/// pairs. The order does not matter. `format` and `callback` are left out of
/// the digest, as the specification requires, and so is `api_sig` itself, so
/// a request can be signed with everything it carries.
///
/// The digest is taken over `name` immediately followed by `value` for each
/// parameter, ordered by the UTF-8 bytes of the name, then the secret. Byte
/// order puts `artist[10]` before `artist[1]`, which is what Last.fm expects.
///
/// ```
/// use scrobl::{protocol::sign, ApiSecret};
///
/// // The example from the authentication specification.
/// let params = [
///     ("method", "auth.getSession"),
///     ("api_key", "xxxxxxxxxx"),
///     ("token", "yyyyyy"),
///     ("format", "json"),
/// ];
/// let signature = sign(params, &ApiSecret::new("ilovecher"));
/// assert_eq!(signature, "b87d61da3cda91a8b6746c4aef55d6f8");
/// ```
pub fn sign<'a>(
    params: impl IntoIterator<Item = (&'a str, &'a str)>,
    secret: &ApiSecret,
) -> String {
    let mut signed: Vec<(&str, &str)> = params
        .into_iter()
        .filter(|(name, _)| !UNSIGNED.contains(name))
        .collect();
    signed.sort_unstable();

    let mut hasher = Md5::new();
    for (name, value) in signed {
        hasher.update(name.as_bytes());
        hasher.update(value.as_bytes());
    }
    hasher.update(secret.expose().as_bytes());

    let digest = hasher.finalize();
    let mut hex = String::with_capacity(32);
    for byte in digest.iter() {
        // Writing to a `String` cannot fail.
        let _ = write!(hex, "{byte:02x}");
    }
    hex
}

#[cfg(test)]
mod tests {
    #![allow(
        clippy::unwrap_used,
        clippy::expect_used,
        clippy::panic,
        clippy::indexing_slicing
    )]

    use proptest::prelude::*;

    use super::*;

    fn secret(s: &str) -> ApiSecret {
        ApiSecret::new(s)
    }

    // Every expected digest below was computed outside Rust, from the exact
    // string in the comment above it, with the macOS tool:
    //   printf '%s' '<string>' | md5

    // Official authentication specification, section 8. The specification
    // gives the string and the secret but no digest.
    // derived (authspec.txt); digest computed with
    //   printf '%s' 'api_keyxxxxxxxxxxmethodauth.getSessiontokenyyyyyyilovecher' | md5
    #[test]
    fn official_example_string() {
        let params = [
            ("method", "auth.getSession"),
            ("api_key", "xxxxxxxxxx"),
            ("token", "yyyyyy"),
        ];
        assert_eq!(
            sign(params, &secret("ilovecher")),
            "b87d61da3cda91a8b6746c4aef55d6f8"
        );
    }

    // Community documentation, "How to create a signature" (docs_auth_signature.md),
    // first example. Published digest 94539006DE89B3C6B3C030BB1E52B9C4; the same
    // value comes from
    //   printf '%s' 'api_keyYOUR_API_KEYmethodauth.getSessiontokenYOUR_REQUESTED_TOKENYOUR_SECRET' | md5
    // derived (docs_auth_signature.md)
    #[test]
    fn community_example_get_session() {
        let params = [
            ("method", "auth.getSession"),
            ("api_key", "YOUR_API_KEY"),
            ("token", "YOUR_REQUESTED_TOKEN"),
            ("format", "json"),
        ];
        let signature = sign(params, &secret("YOUR_SECRET"));
        assert!(signature.eq_ignore_ascii_case("94539006DE89B3C6B3C030BB1E52B9C4"));
        assert_eq!(signature, "94539006de89b3c6b3c030bb1e52b9c4");
    }

    // Community documentation, the track.love example. Published digest
    // 800B8884B00C9343D1D425ED271E0F42; the same value comes from
    //   printf '%s' 'api_keyYOUR_API_KEYartistKITANO REMmethodtrack.loveskYOUR_SESSION_KEYtrackRAINSICKYOUR_SECRET' | md5
    // derived (docs_auth_signature.md)
    #[test]
    fn community_example_track_love() {
        let params = [
            ("artist", "KITANO REM"),
            ("track", "RAINSICK"),
            ("api_key", "YOUR_API_KEY"),
            ("sk", "YOUR_SESSION_KEY"),
            ("method", "track.love"),
            ("format", "json"),
        ];
        let signature = sign(params, &secret("YOUR_SECRET"));
        assert!(signature.eq_ignore_ascii_case("800B8884B00C9343D1D425ED271E0F42"));
        assert_eq!(signature, "800b8884b00c9343d1d425ed271e0f42");
    }

    // synthetic input; digest from
    //   printf '%s' 'api_keyKEYartistBjörkmethodtrack.loveskSKtrackJógaSECRET' | md5
    #[test]
    fn utf8_values_are_signed_as_raw_utf8() {
        let params = [
            ("method", "track.love"),
            ("api_key", "KEY"),
            ("sk", "SK"),
            ("artist", "Björk"),
            ("track", "Jóga"),
        ];
        assert_eq!(
            sign(params, &secret("SECRET")),
            "740d6c8257e0f9e18327b4dfa53c3247"
        );
    }

    // synthetic input; a character outside the Basic Multilingual Plane
    // (U+1F3B5), digest from
    //   printf '%s' 'api_keyKEYartistBjörkmethodtrack.loveskSKtrack🎵 SongSECRET' | md5
    #[test]
    fn non_bmp_values_are_signed_as_raw_utf8() {
        let params = [
            ("method", "track.love"),
            ("api_key", "KEY"),
            ("sk", "SK"),
            ("artist", "Björk"),
            ("track", "\u{1F3B5} Song"),
        ];
        assert_eq!(
            sign(params, &secret("SECRET")),
            "1bba31f8e0a4bbc5dd0ed4c34f2e2461"
        );
    }

    // synthetic input; digest from
    //   printf '%s' 'api_keyKEYartist[10]Cartist[1]Aartist[2]Bmethodtrack.scrobbleskSKSECRET' | md5
    // `artist[10]` sorts first because '0' (0x30) is below ']' (0x5D).
    #[test]
    fn batch_names_sort_by_bytes() {
        let params = [
            ("method", "track.scrobble"),
            ("api_key", "KEY"),
            ("sk", "SK"),
            ("artist[1]", "A"),
            ("artist[2]", "B"),
            ("artist[10]", "C"),
        ];
        let expected = "7f29b9a82947aabfd2c903668a0852f9";
        assert_eq!(sign(params, &secret("SECRET")), expected);

        // Numeric order would give a different digest:
        //   printf '%s' 'api_keyKEYartist[1]Aartist[2]Bartist[10]Cmethodtrack.scrobbleskSKSECRET' | md5
        assert_ne!(expected, "8ff39f00549e3ba72ac056dff4ad3d11");
    }

    // synthetic input; digest from
    //   printf '%s' 'api_keyKEYmethodtrack.loveskSKSECRET' | md5
    #[test]
    fn format_callback_and_api_sig_are_not_signed() {
        let base = [("method", "track.love"), ("api_key", "KEY"), ("sk", "SK")];
        let expected = "130c7d7cff254be5c507d14cbc36828c";
        assert_eq!(sign(base, &secret("SECRET")), expected);

        let with_extras = [
            ("format", "json"),
            ("method", "track.love"),
            ("callback", "cb"),
            ("api_key", "KEY"),
            ("api_sig", "ffffffffffffffffffffffffffffffff"),
            ("sk", "SK"),
        ];
        assert_eq!(sign(with_extras, &secret("SECRET")), expected);
    }

    #[test]
    fn names_that_only_resemble_unsigned_ones_are_signed() {
        let plain = [("method", "m")];
        let lookalike = [("method", "m"), ("formats", "json")];
        assert_ne!(sign(plain, &secret("S")), sign(lookalike, &secret("S")));
    }

    #[test]
    fn the_secret_matters() {
        let params = [("method", "m")];
        assert_ne!(sign(params, &secret("A")), sign(params, &secret("B")));
    }

    #[test]
    fn output_is_lowercase_hex_of_fixed_length() {
        let signature = sign([("a", "b")], &secret(""));
        assert_eq!(signature.len(), 32);
        assert!(
            signature
                .bytes()
                .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
        );
    }

    fn params_strategy() -> impl Strategy<Value = Vec<(String, String)>> {
        // Unique names, none of them unsigned, any values.
        proptest::collection::btree_map("[a-z_\\[\\]0-9]{1,12}", "\\PC{0,16}", 1..8)
            .prop_map(|map| {
                map.into_iter()
                    .filter(|(name, _)| !UNSIGNED.contains(&name.as_str()))
                    .collect::<Vec<_>>()
            })
            .prop_filter("at least one signed parameter", |v| !v.is_empty())
    }

    fn sign_owned(params: &[(String, String)], secret: &ApiSecret) -> String {
        sign(params.iter().map(|(n, v)| (n.as_str(), v.as_str())), secret)
    }

    proptest! {
        // synthetic
        #[test]
        fn signature_ignores_insertion_order(
            params in params_strategy(),
            seed in any::<u64>(),
        ) {
            let secret = secret("SECRET");
            let mut shuffled = params.clone();
            // A deterministic permutation: rotate, then reverse on odd seeds.
            let len = shuffled.len();
            shuffled.rotate_left((seed as usize) % len);
            if seed % 2 == 1 {
                shuffled.reverse();
            }
            prop_assert_eq!(sign_owned(&params, &secret), sign_owned(&shuffled, &secret));
        }

        // synthetic
        #[test]
        fn changing_any_one_value_changes_the_signature(
            params in params_strategy(),
            index in any::<prop::sample::Index>(),
            suffix in "\\PC{1,4}",
        ) {
            let secret = secret("SECRET");
            let mut changed = params.clone();
            changed[index.index(params.len())].1.push_str(&suffix);
            prop_assert_ne!(sign_owned(&params, &secret), sign_owned(&changed, &secret));
        }

        // synthetic
        #[test]
        fn changing_any_one_name_changes_the_signature(
            params in params_strategy(),
            index in any::<prop::sample::Index>(),
        ) {
            let secret = secret("SECRET");
            let mut changed = params.clone();
            changed[index.index(params.len())].0.push('X');
            prop_assert_ne!(sign_owned(&params, &secret), sign_owned(&changed, &secret));
        }
    }
}
