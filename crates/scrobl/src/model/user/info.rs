//! A user, and the users they follow: the replies to `user.getInfo` and
//! `user.getFriends`.

use std::fmt;

use serde_json::{Map, Value};

use crate::de;
use crate::error::Error;
use crate::model::Page;
use crate::model::entity::{self, Images};
use crate::model::shared::{self, missing_or};
use crate::protocol::{Raw, methods};

/// A user. [`json`](Self::json) is the object exactly as decoded.
///
/// A profile, the reply to `user.getInfo`, says how much the user has
/// played. A row of a friends list does not: the service sends zeros there
/// whatever the friend has played, so for a friend the counts and
/// [`subscriber`](Self::subscriber) are `None`. Ask for the friend's own
/// profile to learn them.
///
/// ```
/// use scrobl::model::decode;
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
/// let user = decode::user_get_info(&raw)?;
/// assert_eq!(user.name(), "rj");
/// assert_eq!(user.playcount(), Some(150_000));
/// assert_eq!(user.artist_count(), Some(12_000));
/// assert_eq!(user.album_count(), None);
/// assert_eq!(user.country(), None);
/// assert_eq!(user.registered_at(), 1_037_793_040);
/// assert_eq!(user.subscriber(), Some(true));
/// # Ok::<(), scrobl::Error>(())
/// ```
#[derive(Clone)]
pub struct User {
    name: String,
    realname: Option<String>,
    url: Option<String>,
    country: Option<String>,
    images: Images,
    registered_at: u64,
    playcount: Option<u64>,
    artist_count: Option<u64>,
    album_count: Option<u64>,
    track_count: Option<u64>,
    subscriber: Option<bool>,
    json: Value,
}

impl User {
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

    /// The country the user gives. `None` when the member is missing, empty
    /// or the word `None`, which is what the service sends for a user who
    /// gives no country; [`json`](Self::json) tells them apart.
    pub fn country(&self) -> Option<&str> {
        self.country.as_deref()
    }

    /// The user's image in the sizes the service sent.
    pub fn images(&self) -> &Images {
        &self.images
    }

    /// When the user registered, in seconds since the Unix epoch, from
    /// `registered.unixtime`.
    pub fn registered_at(&self) -> u64 {
        self.registered_at
    }

    /// How many scrobbles the user has. `None` in a friends list.
    pub fn playcount(&self) -> Option<u64> {
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

    /// Whether the user pays for a subscription, where the service says.
    /// `None` in a friends list.
    pub fn subscriber(&self) -> Option<bool> {
        self.subscriber
    }

    /// The object exactly as decoded.
    pub fn json(&self) -> &Value {
        &self.json
    }
}

impl AsRef<User> for User {
    fn as_ref(&self) -> &User {
        self
    }
}

impl fmt::Debug for User {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("User")
            .field("registered_at", &self.registered_at)
            .field("playcount", &self.playcount)
            .finish_non_exhaustive()
    }
}

/// Decodes a `user.getInfo` response: a user's profile.
///
/// # Errors
///
/// [`ErrorKind::Decode`](crate::ErrorKind) when the response is for another
/// method, when `user`, its `name`, its `playcount` or its
/// `registered.unixtime` is missing or malformed, when an object repeats a
/// member name, or when another field has the wrong type. The message names
/// the field, such as `user.registered.unixtime`, and never repeats response
/// text.
pub fn user_get_info(raw: &Raw) -> Result<User, Error> {
    shared::decode(raw, &methods::USER_GET_INFO, "user", |user| {
        parse(user, "user", Counts::Read)
    })
}

/// Decodes a `user.getFriends` response: one page of the users a user
/// follows. The page's subject is the user whose friends these are.
///
/// A friend's counts and `subscriber` are `None`: see [`User`]. The service
/// answers a page beyond the last with
/// [`INVALID_PARAMETERS`](crate::ApiErrorCode::INVALID_PARAMETERS), not
/// with an empty page.
///
/// # Errors
///
/// [`ErrorKind::Decode`](crate::ErrorKind) under [the rules every list
/// shares](crate::model::decode#errors). `@attr.user` is required, and a row
/// needs a `name` and a `registered.unixtime`.
pub fn user_get_friends(raw: &Raw) -> Result<Page<User>, Error> {
    shared::paged(
        raw,
        &methods::USER_GET_FRIENDS,
        "friends",
        "user",
        Some("user"),
        |row, at| parse(row, at, Counts::Leave),
    )
}

/// Whether a user's object says how much they have played. A profile does.
/// A row of a friends list has the members and fills them with zeros.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Counts {
    Read,
    Leave,
}

fn parse(user: Map<String, Value>, at: &str, counts: Counts) -> Result<User, Error> {
    let registered_at = user
        .get("registered")
        .and_then(|registered| registered.get("unixtime"))
        .and_then(de::uint)
        .ok_or_else(|| missing_or(&format!("{at}.registered.unixtime"), "an unsigned integer"))?;
    let (playcount, artist_count, album_count, track_count, subscriber) = match counts {
        Counts::Read => (
            Some(shared::required_uint(&user, at, "playcount")?),
            shared::optional_uint(&user, at, "artist_count")?,
            shared::optional_uint(&user, at, "album_count")?,
            shared::optional_uint(&user, at, "track_count")?,
            entity::optional_flag(&user, at, "subscriber")?,
        ),
        Counts::Leave => (None, None, None, None, None),
    };
    Ok(User {
        name: shared::required_str(&user, at, "name")?.to_owned(),
        realname: shared::optional_str(&user, at, "realname")?,
        url: shared::optional_str(&user, at, "url")?,
        country: shared::optional_str(&user, at, "country")?.filter(|country| country != "None"),
        images: entity::images(&user, at)?,
        registered_at,
        playcount,
        artist_count,
        album_count,
        track_count,
        subscriber,
        json: Value::Object(user),
    })
}

#[cfg(test)]
mod tests;
