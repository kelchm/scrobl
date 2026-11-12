//! A user's profile, and the profiles of their friends: the replies to
//! `user.getInfo` and `user.getFriends`.

use std::fmt;

use serde_json::{Map, Value};

use crate::de;
use crate::error::Error;
use crate::model::Page;
use crate::model::shared::{self, Image, missing_or};
use crate::protocol::{Raw, methods};

/// A user's profile: the reply to `user.getInfo`, and a row of
/// `user.getFriends`. [`json`](Self::json) is the profile exactly as
/// decoded.
///
/// A row of a friends list is a shorter profile. It has none of the three
/// counts of artists, albums and tracks, and in every row recorded its
/// `playcount` and `subscriber` were 0, whatever the friend's own profile
/// says.
///
/// ```
/// use scrobl::model::UserInfo;
/// use scrobl::protocol::{self, HttpResponse, Request, methods};
///
/// let body = r##"{"user": {
///     "name": "rj", "realname": "", "url": "https://www.last.fm/user/rj",
///     "country": "None", "playcount": "150000", "artist_count": "12000",
///     "subscriber": "1", "registered": {"unixtime": "1037793040", "#text": 1037793040}
/// }}"##;
/// let request = Request::new(&methods::USER_GET_INFO).param("user", "rj");
/// let raw = protocol::decode(&request, HttpResponse::new(200, body))?;
///
/// let user = UserInfo::decode(&raw)?;
/// assert_eq!(user.name(), "rj");
/// assert_eq!(user.playcount(), 150_000);
/// assert_eq!(user.artist_count(), Some(12_000));
/// assert_eq!(user.album_count(), None);
/// assert_eq!(user.country(), None);
/// assert_eq!(user.registered(), 1_037_793_040);
/// assert_eq!(user.subscriber(), Some(true));
/// # Ok::<(), scrobl::Error>(())
/// ```
#[derive(Clone)]
pub struct UserInfo {
    name: String,
    realname: Option<String>,
    url: Option<String>,
    country: Option<String>,
    playcount: u64,
    artist_count: Option<u64>,
    album_count: Option<u64>,
    track_count: Option<u64>,
    registered: u64,
    subscriber: Option<bool>,
    images: Vec<Image>,
    json: Value,
}

impl UserInfo {
    /// Decodes a `user.getInfo` response.
    ///
    /// # Errors
    ///
    /// [`ErrorKind::Decode`](crate::ErrorKind) when the response is for
    /// another method, when `user`, its `name`, its `playcount` or its
    /// `registered.unixtime` is missing or malformed, when an object repeats
    /// a member name, or when another field has the wrong type. The message
    /// names the field, such as `user.registered.unixtime`, and never
    /// repeats response text.
    pub fn decode(raw: &Raw) -> Result<Self, Error> {
        shared::decode(raw, &methods::USER_GET_INFO, "user", |user| {
            parse(user, "user")
        })
    }

    /// The user name, in the service's spelling.
    pub fn name(&self) -> &str {
        &self.name
    }

    /// The name the user gives as their own, if they give one.
    pub fn realname(&self) -> Option<&str> {
        self.realname.as_deref()
    }

    /// The Last.fm page of the user, if the service sent a non-empty one.
    pub fn url(&self) -> Option<&str> {
        self.url.as_deref()
    }

    /// The country the user gives. `None` when the field is missing, empty
    /// or the word `None`, which is what the service sends for a user who
    /// gives no country; [`json`](Self::json) tells them apart.
    pub fn country(&self) -> Option<&str> {
        self.country.as_deref()
    }

    /// How many scrobbles the user has. In a row of a friends list the
    /// service has sent 0 here whatever the friend has scrobbled, so read it
    /// from the friend's own profile.
    pub fn playcount(&self) -> u64 {
        self.playcount
    }

    /// How many artists the user has played, where the service says.
    pub fn artist_count(&self) -> Option<u64> {
        self.artist_count
    }

    /// How many albums the user has played, where the service says.
    pub fn album_count(&self) -> Option<u64> {
        self.album_count
    }

    /// How many tracks the user has played, where the service says.
    pub fn track_count(&self) -> Option<u64> {
        self.track_count
    }

    /// When the user registered, in seconds since the Unix epoch, from
    /// `registered.unixtime`.
    pub fn registered(&self) -> u64 {
        self.registered
    }

    /// Whether the user pays for a subscription, where the service says. In
    /// a row of a friends list it has said no for every friend, so read
    /// this too from the friend's own profile.
    pub fn subscriber(&self) -> Option<bool> {
        self.subscriber
    }

    /// The user's image in the sizes the service sent, in its order.
    /// Empty when it sent none. An entry whose address is missing or empty
    /// is left out; [`json`](Self::json) still holds it.
    pub fn images(&self) -> &[Image] {
        &self.images
    }

    /// The profile exactly as decoded.
    pub fn json(&self) -> &Value {
        &self.json
    }
}

impl fmt::Debug for UserInfo {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("UserInfo")
            .field("playcount", &self.playcount)
            .field("registered", &self.registered)
            .finish_non_exhaustive()
    }
}

/// Decodes a `user.getFriends` response: one page of the users a user
/// follows. The page's subject is the user whose friends these are.
///
/// The service answers a page beyond the last with
/// [`INVALID_PARAMETERS`](crate::ApiErrorCode::INVALID_PARAMETERS), not
/// with an empty page.
///
/// # Errors
///
/// [`ErrorKind::Decode`](crate::ErrorKind) under [the rules every list
/// shares](crate::model::decode#errors). `@attr.user` is required, and a row
/// must be a profile as [`UserInfo::decode`] requires.
pub fn user_get_friends(raw: &Raw) -> Result<Page<UserInfo>, Error> {
    shared::paged(
        raw,
        &methods::USER_GET_FRIENDS,
        "friends",
        "user",
        Some("user"),
        parse,
    )
}

fn parse(user: Map<String, Value>, at: &str) -> Result<UserInfo, Error> {
    let registered = user
        .get("registered")
        .and_then(|registered| registered.get("unixtime"))
        .and_then(de::uint)
        .ok_or_else(|| missing_or(&format!("{at}.registered.unixtime"), "an unsigned integer"))?;
    let subscriber =
        match user.get("subscriber") {
            None => None,
            Some(value) => Some(de::flag(value).ok_or_else(|| {
                Error::decode_field(&format!("{at}.subscriber"), "not a 0/1 flag")
            })?),
        };
    Ok(UserInfo {
        name: shared::required_str(&user, at, "name")?.to_owned(),
        realname: shared::optional_str(&user, at, "realname")?,
        url: shared::optional_str(&user, at, "url")?,
        country: shared::optional_str(&user, at, "country")?.filter(|country| country != "None"),
        playcount: shared::required_uint(&user, at, "playcount")?,
        artist_count: shared::optional_uint(&user, at, "artist_count")?,
        album_count: shared::optional_uint(&user, at, "album_count")?,
        track_count: shared::optional_uint(&user, at, "track_count")?,
        registered,
        subscriber,
        images: shared::parse_images(&user, at)?,
        json: Value::Object(user),
    })
}

#[cfg(test)]
mod tests;
