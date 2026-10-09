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
pub struct ParamSpec {
    pub(super) name: &'static str,
    pub(super) requirement: Requirement,
    pub(super) indexed: bool,
}

impl ParamSpec {
    /// The parameter name as sent, without any batch index.
    pub const fn name(&self) -> &'static str {
        self.name
    }

    /// Whether the documentation requires it.
    pub const fn requirement(&self) -> Requirement {
        self.requirement
    }

    /// Whether the name takes a batch index, as in `artist[3]`.
    pub const fn indexed(&self) -> bool {
        self.indexed
    }
}

/// One Last.fm API method.
///
/// Specs come only from [`methods`](super::methods). A spec cannot be built
/// or changed outside this crate, so a method's verb, credentials and
/// `write` flag are always the table's.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct MethodSpec {
    pub(super) name: &'static str,
    pub(super) verb: Verb,
    pub(super) auth: Auth,
    pub(super) write: bool,
    pub(super) paging: Paging,
    pub(super) params: &'static [ParamSpec],
}

impl MethodSpec {
    /// The method name as sent, such as `user.getRecentTracks`.
    pub const fn name(&self) -> &'static str {
        self.name
    }

    /// The HTTP verb.
    pub const fn verb(&self) -> Verb {
        self.verb
    }

    /// The credentials the method needs.
    pub const fn auth(&self) -> Auth {
        self.auth
    }

    /// Whether the method changes anything on the account.
    pub const fn write(&self) -> bool {
        self.write
    }

    /// How results are paged.
    pub const fn paging(&self) -> Paging {
        self.paging
    }

    /// The documented parameters.
    pub const fn params(&self) -> &'static [ParamSpec] {
        self.params
    }
}
