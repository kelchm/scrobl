//! How one Last.fm API method is called. The table of every method lives in
//! [`methods`](super::methods).

/// The HTTP verb a method is sent with.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Verb {
    /// Parameters travel in the query string.
    Get,
    /// Parameters travel in a form-encoded body.
    Post,
}

/// What a method needs beyond its own parameters.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Auth {
    /// An API key only.
    ApiKey,
    /// An API key and a signature made with the API secret, but no session.
    Signed,
    /// An API key, a signature and a user's session key.
    Session,
}

/// Whether the official documentation requires a parameter.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Requirement {
    /// Always required.
    Required,
    /// Required unless an alternative is given, such as `artist` unless `mbid`.
    Conditional,
    /// Optional.
    Optional,
}

/// How a method pages its results.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Paging {
    /// The method returns everything in one response.
    None,
    /// The method takes `limit` but not `page`.
    LimitOnly,
    /// The method takes `page` and `limit`.
    PageAndLimit,
}

/// One documented parameter of a method.
///
/// `api_key`, `api_sig`, `sk`, `method` and `format` are implied by
/// [`Auth`] and are never listed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub struct ParamSpec {
    /// The parameter name as sent, without any batch index.
    pub name: &'static str,
    /// Whether the documentation requires it.
    pub requirement: Requirement,
    /// Whether the name takes a batch index, as in `artist[3]`.
    pub indexed: bool,
}

/// One Last.fm API method.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub struct MethodSpec {
    /// The method name as sent, such as `user.getRecentTracks`.
    pub name: &'static str,
    /// The HTTP verb.
    pub verb: Verb,
    /// The credentials the method needs.
    pub auth: Auth,
    /// Whether the method changes anything on the account.
    pub write: bool,
    /// How results are paged.
    pub paging: Paging,
    /// The documented parameters.
    pub params: &'static [ParamSpec],
}
