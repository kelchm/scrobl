# scrobl design

What the library and the tool are, and why. The [decisions](#decisions) are in the table at the end. Code blocks are sketches of the API, not finished signatures. Last revised 2026-11-11.

## Scope

The 57 methods in the official Last.fm API index (snapshot of 2026-10-05) plus the web, desktop and mobile authentication flows, over JSON.

Out of scope:

- The Radio API, the Playlists API, Submissions Protocol 1.2.1, XML-RPC and XML output. The official pages mark each deprecated, or it is only another encoding of the same methods.
- Website scraping and history editing, permanently.
- `track.ban`, `track.unban`, `user.getArtistTracks` and `user.getNewReleases`. Older documentation and other libraries list them, but the index no longer does, so they are not in the method table and cannot be called through it. They would come back only if the index lists them again.

[`endpoints.md`](endpoints.md) lists every method with its verb, credentials, paging and verification status.

## Layout

One Cargo workspace with two crates: the library, `scrobl`, and the [command-line tool](#command-line-tool), `scrobl-cli`. The tool depends on the library and never the other way round, so a program that uses the library builds none of the tool's dependencies. Nothing about SQLite or the command line belongs in the library. A change the tool asks of it has to be one another application would want too.

```
crates/scrobl/
  src/
    lib.rs
    secret.rs          ApiKey, ApiSecret, SessionKey: redacted Debug, no Display
    error.rs           Error, ErrorKind, ApiErrorCode, Retry, Delivery
    protocol/          I/O-free core
      method.rs        MethodSpec and friends
      methods.rs       the 57-row table
      sign.rs          api_sig
      request.rs       Request -> HttpRequest
      response.rs      HttpResponse -> Raw, error envelope at every status
    de.rs              tolerant scalar decoding (number-or-string, one-or-many)
    model/             typed views: the shared pieces, then one module per API package
    history.rs         Window, RecentTracks, WindowScan: the validated recent-tracks scan, I/O-free
    response.rs        Response<T>: a typed value next to the exact response
    client/            feature "client": reqwest executor, pacing, retries, typed reads
  tests/               integration tests
    support/           synthetic dataset (pure) and fake_lastfm.rs, the fake Last.fm on a real socket
  fixtures/            recorded and derived responses, each listed with its source
```

```
crates/scrobl-cli/     the binary `scrobl`
  src/
    main.rs            the process: environment, runtime, exit code
    lib.rs             the same code as a library, so the tests need no terminal
    cli.rs             arguments, output and the exit status
    sync.rs            which spans of time to read, and when
    store.rs           the SQLite file
    time.rs            the clock, and seconds as text
    error.rs           the tool's one error type
  tests/               the same fake Last.fm, included from the library's tests by path
```

The feature `client` (default) enables the async client and pulls in `reqwest` and `tokio`. `--no-default-features` is the protocol core alone, which must build and pass its tests. That is checked with `-p scrobl`, because the tool needs the client and would switch the feature on for a build of the whole workspace.

## Layers

### Protocol core

No sockets, clocks, sleeps, tasks or runtime. Everything a transport needs is plain data.

```rust
use scrobl::protocol::{self, methods, Credentials, HttpRequest, HttpResponse, Request};

let request = Request::new(&methods::USER_GET_RECENT_TRACKS)
    .param("user", "rj")
    .param("limit", 200);

let credentials = Credentials::new(api_key);          // .with_secret(..), .with_session(..), .allow_writes()
let http: HttpRequest = protocol::prepare(&credentials, &request)?;
// http.verb(), http.url(), http.body(): send with any HTTP client

let response = HttpResponse::new(status, body_bytes);
let raw: Raw = protocol::decode(&request, response)?; // Err for any Last.fm error envelope
```

Rules:

- **Writes are refused first.** `prepare` refuses a method that changes the account with `ErrorKind::ReadOnly` unless the credentials allow writes (`Credentials::allow_writes`), whatever else is held and whatever the parameters are. See [Reading and writing](#reading-and-writing).
- **What `prepare` adds.** `method`, `api_key` and `format=json`, then `sk` and `api_sig` when the method's `Auth` needs them. It fails with `ErrorKind::InvalidRequest` when a required credential is missing, when the caller sets a reserved name (`method`, `api_key`, `api_sig`, `sk`, `format`, `callback`), or when a parameter name repeats.
- **The signature.** The MD5 hex digest of every sent parameter except `format`, `callback` and `api_sig` itself, as `name` then `value`, ordered by the UTF-8 bytes of the name, followed by the secret. Byte ordering puts `artist[10]` before `artist[1]`, which is what the service expects. Signing is not public: `prepare` is the only way to get a signature, so nothing is signed that it did not check.
- **A read as a user.** `Request::as_user()` sends `sk` and `api_sig` with a method that does not require them, so a read is made as the session's user. This is how a hidden history would be read. The official pages describe the mode without documenting it, so it is unverified.
- **Where parameters go.** `Get` puts them in the query string. `Post` puts all of them, `method` included, in a form-encoded body. The root is always `https://ws.audioscrobbler.com/2.0/`, and a different one can be set only for tests.
- **Errors come first in `decode`.** It looks for the JSON error envelope (`{"error": N, "message": ".."}`) at every HTTP status before anything else, so HTTP 200 carrying an error is an error. A non-2xx status without an envelope is `ErrorKind::Http`. A 2xx body that is not JSON is `ErrorKind::Decode`.
- **`Raw` is the exact response.** The status, the exact body bytes, the `Request` it answers and a small set of headers: `content-type`, `retry-after`, `date`, and the ones a cache goes by (`cache-control`, `expires`, `etag`, `last-modified`, `age`). Typed views are decoded from it and never replace it.
- **A response knows its request.** `Request` compares by method, parameters in order and the `as_user` flag, so a consumer can tell whether a response answers the request it holds.
- **Redaction.** `HttpRequest`'s `Debug` and every error redact `api_key`, `api_sig`, `sk`, `token` and `password` values. No error carries a URL.
- **The method table is closed.** A `MethodSpec` has private fields and read-only getters, and the only ones that exist are the constants in `methods`. A caller cannot build one or change a verb, credential requirement or `write` flag.

There is no public transport trait. A caller with another HTTP client uses `prepare` and `decode` directly.

### Reading and writing

Last.fm has no read-only keys, no scopes and no per-application permissions, and a session key does not expire. A session that can read a user's hidden history can also scrobble, love, unlove and tag for them, so holding a secret and a session key says nothing about whether a program means to write. The library keeps reading and writing apart itself, in two places.

**In the core,** `Credentials` carries a grant that is off by default, and `prepare` refuses the ten write methods without it before it checks or signs anything. Every path to a signed request goes through `prepare`: the client's raw `call`, the typed methods, and a caller with another HTTP client. Because the method table is closed, a description cannot be copied and relabelled as a read.

**In the client,** the grant is a type. A `Client` never holds it and so never writes. A `Writer` holds it, is made only by `ClientBuilder::build_writer`, and dereferences to `Client`, so it reads too, and code handed a `&Client` cannot write. A program that never names `Writer` or `allow_writes` cannot change an account through this crate.

**What counts as a write** is the `write` flag of the method table: the ten methods whose official pages say "This is a write service", which are also exactly the ten that require a session.

- `track.updateNowPlaying` is a write. It stores nothing in the history, but it changes what the profile shows.
- The three `auth` methods are not. They create a token or a session, and `auth.getMobileSession` sends a password, but they change nothing on the account, and a backup application needs them to sign in.
- A read made `as_user` is a read.

This guards against a bug or a careless call. It is not a security boundary:

- The service enforces none of it.
- Code that holds the API secret and a session key can sign a request without this crate.
- Last.fm's signature joins names and values with no delimiter, so the signature of a read with contrived parameters is also the signature of some write. A `HttpRequest` taken apart on purpose can be turned into one.

The promise covers requests as `prepare` built them and as the client sent them.

### Typed views

Typed models are decoded from `Raw` on request. Fields are private with accessors. Each item keeps its own decoded JSON, so unknown fields and the difference between a missing and an empty field stay reachable:

```rust
let page = RecentTracksPage::decode(&raw)?;
for scrobble in page.scrobbles() {
    scrobble.timestamp();        // seconds since the epoch, always present for a scrobble
    scrobble.track().name();
    scrobble.artist().mbid();    // None when missing or empty
    scrobble.loved();            // None unless the page was requested with extended=1
    scrobble.track().images();   // &[Image]: each a size and an address, in the service's order
    scrobble.json();             // &serde_json::Value, the row exactly as decoded
}
page.now_playing();              // Option<&NowPlaying>, never mixed into scrobbles()
page.attr();                     // user, page, per_page, total_pages, total
```

Decoding rules for every model:

- Counts and timestamps accept a JSON number or a numeric string, and nothing else.
- A field that can be one object or an array of them decodes to a list either way.
- A malformed value is an error naming the field, such as `recenttracks.track[3].date.uts`. It is never dropped or defaulted.
- An object that repeats a member name, at any depth, is a decode error, not a silent last-one-wins. The error gives a line and column and never the name.
- A track, an artist, an album and a user each carry their pictures as `images()`: the `image` member, which is a list of sizes. An entry with an empty address is left out, as an empty id reads as none.
- `Debug` on a model leaves out the response text.

And for a page of the history:

- A row flagged now-playing is never a scrobble, whether or not it carries a date. A row whose flag is false is an ordinary historical row.
- A row with neither a valid `date.uts` nor the now-playing flag is an error, not a skipped row.
- `track` may be an array or a single object. For an empty page it may also be absent, `[]` or `""`, but only when the page's own `@attr` allows no rows: `total` is 0, or `page` is beyond `totalPages`. A missing or misspelled `track` on a page that promises rows is an error, as is anything else.
- An `artist` that carries both `name` and `#text` must carry the same string in each, and each one present must be a string.
- `loved()` reflects the field as returned, which is present only on extended pages.

### History scan

`WindowScan` is the validated read of one time window of `user.getRecentTracks`. It is an I/O-free state machine, so every pagination rule is tested without a socket. The client drives it.

```rust
let mut scan = RecentTracks::new(user)
    .window(Window::new(from, to)?)
    .extended(true)
    .scan()?;                          // validates; the scan cannot be changed afterwards
while let Some(request) = scan.next_request() {
    let raw = /* execute */;
    let page = scan.accept(raw)?;      // validates, then yields the page
}
let summary = scan.finish()?;          // total scrobbles, pages, the window
```

A scan is made only by `RecentTracks::scan`, from the builder's user, window, `extended`, `as_user` and `limit`. It has no setters, so every request it issues equals the others apart from `page`.

- It pages by page number with one `limit` throughout: the builder's, or 200 when unset. 200 is the documented maximum.
- `scan` fails with `ErrorKind::InvalidRequest` when `limit` is outside 1 to 200, or when `page` was set, since a scan chooses its own pages. `RecentTracks::request`, which builds one request by itself, refuses a `limit` outside 1 to 200 or a `page` of 0 the same way. Nothing is clamped.
- It never uses a timestamp as a cursor and never removes duplicates, so same-second and identical scrobbles survive.
- It never issues a request with altered bounds.

`accept` first checks that the response answers the outstanding request, by comparing `Raw::request` with it. A response to any other request, for another window, page, user, limit or `extended`, is `ErrorKind::Inconsistent` ("the response answers a different request") and is refused without being decoded. The page is then decoded, and one that does not decode, for want of a total for instance, is `ErrorKind::Decode`. A decoded page must satisfy all of the following, and a failure is `ErrorKind::Inconsistent` naming the rule. After either error the scan yields nothing further:

1. `@attr.user` equals the requested user, ignoring case.
2. `@attr.page` equals the requested page and `@attr.perPage` equals the requested limit.
3. `@attr.total` and `@attr.totalPages` are identical on every page of the scan, and `totalPages` equals `ceil(total / perPage)`; when `total` is 0 it may be 0 or 1.
4. The number of scrobbles on the page is exactly what the totals imply: `perPage` on every page but the last, the remainder on the last. A now-playing row is not a scrobble and is not counted.
5. Every scrobble timestamp lies in `[from, to)`: `from` inclusive, `to` exclusive.
6. Timestamps never increase, within a page or from one page to the next.
7. At `finish`, the scrobbles seen equal `total`.

The scan ends after `totalPages` pages, or after page 1 when `total` is 0.

- `next_request` returns the same request until a page is accepted, so a transport failure can be retried without skipping or repeating a page.
- A response that does not reach `accept` (an API error envelope, a non-JSON body) does not poison the scan.
- An empty window is a successful scan with `total` 0, which an error is not.
- A scan without an upper bound is allowed. It sees scrobbles that arrive while it runs, which usually changes `total` and fails rule 3, but not always (see below). A caller who wants a more stable read fixes `to` first.
- The `[from, to)` rule comes from the live service as seen on 2026-10-08, not from a documented guarantee. That is why it is checked on every page and not assumed.

The client drives the scan as `Scan`:

- `next_page` sends the outstanding request through the same paced, retried `call` as any read and hands the result to `accept`.
- An error from the transport or the API leaves the page outstanding, so calling `next_page` again sends the identical request.
- An error from `accept` kills the scan, whichever of the two kinds it is. Later calls fail with `Inconsistent` and send nothing. The core by itself would offer no further request, which looks like completion, so `Scan` remembers the failure and reports it.
- A page the scan has yielded is provisional until `finish` succeeds, because a later page or `finish` itself can still fail a rule. A consumer stages what it writes and marks it complete only then.

#### What a completed scan does not prove

Success means every page satisfied the rules against the service's own totals at the time that page was read. It does not mean the rows are what the window held at any one moment:

- A scrobble arriving and another being deleted between two page reads leaves `total` unchanged and can shift a row across a page boundary, giving one duplicate and one omission. A fixed `to` keeps arrivals at the head out of the window, but not backdated additions or deletions inside it.
- If the service changes the order of same-second rows between page reads, a row can repeat and another go missing with no rule broken.
- A page repeated under a new page number is caught by rule 6 only if its timestamps differ from the previous page's last. Listens can be identical, so equal rows cannot be rejected.
- A coherent wrong answer cannot be detected from the response alone: another user's rows under the right `@attr.user`, or rows left out with the totals reduced to match.
- Fixed bounds are not snapshot isolation.

An application that needs more reads the window again and compares the two reads. The library never deduplicates. The test suite pins the first three cases as documented behaviour, so a change in detection is a visible decision.

### Client

One concrete async client over `reqwest`. It is cheap to clone (two `Arc`s) and `Clone + Send + Sync + 'static`, and every future is `Send`.

- The crate spawns no task, starts no thread and owns no runtime. It runs on whatever Tokio runtime polls it, a current-thread one included.
- That runtime needs its I/O and time drivers enabled (`enable_all`). Without them the first call panics inside Tokio, which the client does not hide.
- `reqwest` itself spawns its connection tasks on that runtime, and resolving a host name uses the runtime's blocking pool.
- A client can be built outside a runtime.
- There is no blocking adapter. A synchronous caller, such as the command-line tool, drives this client on a current-thread Tokio runtime.

```rust
impl Client {
    pub fn builder(api_key: ApiKey) -> ClientBuilder;
    pub fn with_session(&self, session: SessionKey) -> Client;   // shares the pool and the pacing clock
    pub async fn call(&self, request: &Request) -> Result<Raw, Error>;   // any method that is not a write
    pub fn user(&self, name: impl Into<String>) -> User;
}

impl Writer {   // Clone, Deref<Target = Client>
    pub fn with_session(&self, session: SessionKey) -> Writer;
    pub async fn call(&self, request: &Request) -> Result<Raw, Error>;   // any of the 57 methods
}

impl ClientBuilder {   // each setting has a default; build() fails with ErrorKind::Config
    pub fn secret(self, ApiSecret) -> Self;      pub fn session(self, SessionKey) -> Self;
    pub fn user_agent(self, impl Into<String>) -> Self;   // default scrobl/<version>
    pub fn connect_timeout(self, Duration) -> Self;       // 10 s
    pub fn timeout(self, Duration) -> Self;               // 30 s, the longest the client waits for one attempt, last body byte included
    pub fn max_response_bytes(self, usize) -> Self;       // 8 MiB
    pub fn min_interval(self, Duration) -> Self;          // 1 s, zero turns pacing off, over 24 h is a Config error
    pub fn read_attempts(self, u32) -> Self;              // 3, counting the first; 1 turns retries off
    pub fn retry_delay(self, Duration) -> Self;           // 1 s
    pub fn no_proxy(self) -> Self;                        // ignore HTTPS_PROXY and friends
    pub fn build(self) -> Result<Client, Error>;          // reads and authenticates
    pub fn build_writer(self) -> Result<Writer, Error>;   // may also change the account
}

impl User {
    pub fn recent_tracks(&self) -> RecentTracksQuery;     // the history: one page, or a checked scan
    pub fn loved_tracks(&self) -> LovedTracksQuery;       // one page, newest first; limit 1 to 1000
    pub fn info(&self) -> UserInfoQuery;
    pub fn friends(&self) -> FriendsQuery;
    pub fn top_artists(&self) -> TopArtistsQuery;         // also top_albums, top_tracks, top_tags
    pub fn weekly_chart_list(&self) -> WeeklyChartListQuery;
    pub fn weekly_artist_chart(&self) -> WeeklyArtistChartQuery;   // also weekly_album_chart, weekly_track_chart
    pub fn tagged_artists(&self, tag: impl Into<String>) -> TaggedArtistsQuery;   // also tagged_albums, tagged_tracks
    pub fn library_artists(&self) -> LibraryArtistsQuery;
}
impl TopArtistsQuery {   // the shape of a list query
    pub fn period(self, Period) -> Self;   // the top lists of artists, albums and tracks
    pub fn limit(self, u32) -> Self;  pub fn page(self, u32) -> Self;   // where the method pages
    pub async fn send(self) -> Result<Response<TopArtistsPage>, Error>;   // one page
}
impl RecentTracksQuery {
    pub fn window(self, Window) -> Self;  pub fn limit(self, u32) -> Self;  pub fn page(self, u32) -> Self;
    pub fn extended(self, bool) -> Self;  pub fn as_user(self) -> Self;
    pub async fn send(self) -> Result<Response<RecentTracksPage>, Error>;   // one page
    pub fn scan(self) -> Result<Scan, Error>;   // the window and limit come from the query
}
impl Scan {
    pub fn window(&self) -> Window;  pub fn page_size(&self) -> u32;
    pub async fn next_page(&mut self) -> Result<Option<ScanPage>, Error>;
    pub fn finish(self) -> Result<ScanSummary, Error>;
}

pub struct Response<T>;   // Deref<Target = T>; .raw() -> &Raw; .value() -> &T; .into_value() -> T; .into_parts() -> (T, Raw)
```

**List queries.** `limit` and `page` exist where the method pages. A weekly chart takes only `range(from, to)`, and the profile and the list of weekly charts take nothing. A `limit` or `page` of 0 is an `InvalidRequest` before anything is sent, naming the parameter. Only the history (200) and the loved tracks (1000) check an upper bound on `limit` in the client. For the others the service refuses a `limit` over its own maximum, which differs by method ([Coverage](#coverage) has what was seen).

**`Response<T>`** is how every typed call returns its result: the typed value next to the exact `Raw`, so the bytes can be archived and a field the model lacks stays reachable. `value` and `into_value` reach the typed value without `Deref`, for a method of `T` that a method of `Response` would shadow. `Debug` of `Response`, `Raw` and `HttpResponse` shows header names and never header values, which are text the network chose; `header()` still returns them.

**`scan`** mirrors `RecentTracks::scan`. The query's window (`Window::ALL` when unset), `limit` as the constant page size (200 when unset), `extended` and `as_user` become the scan, and it cannot be changed afterwards. A `page` on the query, or a `limit` outside 1 to 200, is an `InvalidRequest` from `scan` itself, before anything is sent.

#### Fixed behaviour

- **HTTPS only.** The client refuses a plain-HTTP URL before connecting. It uses rustls, keeps no cookies and switches `reqwest`'s own retries off.
- **No redirects.** A 3xx is returned to `decode`, which treats it as any status that is not 2xx: `ErrorKind::Http`, or `Api` when it carries an error envelope. The target of a `Location` receives nothing.
- **No decompression.** Every `reqwest` decoder (`gzip`, `brotli`, `deflate`, `zstd`) is switched off explicitly. Cargo unifies features, so another crate in the program could otherwise turn one on, and then the client would send `Accept-Encoding` and `Raw::body` would not be the bytes the service sent.
- **Proxies.** The environment variables (`HTTPS_PROXY`, `ALL_PROXY`, `NO_PROXY`) are honoured. Through an HTTP proxy only a `CONNECT` naming the host and port is visible to the proxy, and the key stays inside TLS. `ClientBuilder::no_proxy` opts out.
- **The test root.** The one hook that changes the root, `ClientBuilder::base_url` (hidden, for tests), accepts only a literal loopback address with no user information, query or fragment. So it cannot turn HTTPS-only off for a real host or send a key elsewhere. Anything else is a `Config` error that does not repeat the URL, and `Debug` says only whether a root is set.

One attempt is: wait for admission, send, read the body up to the cap, `decode`. The request is built once, before the first admission, so an invalid request, or a write asked of a `Client`, waits for nothing and sends nothing.

#### Pacing

- Every attempt, a retry included, is admitted at least one interval after the previous admission, across all clones.
- Admission is serialised and measured when it happens. A caller takes an async lock (`tokio::sync::Mutex`, first come first served) over the time of the last admission, sleeps whatever remains of the interval while holding the lock, reads the clock again, records that as the new last admission and releases.
- So the spacing holds however late a task is polled. A runtime stalled for several intervals delays the queued callers. It does not release them together.
- Nothing is reserved ahead, and a caller dropped while queued or sleeping leaves no debt: the next one waits only for what remains since the last real admission.
- A zero interval skips pacing entirely. An interval over 24 hours is a `Config` error.
- The interval is measured at admission to the transport, in this process. Connection setup can still make arrivals at the server bunch slightly.
- Pacing is per `Client` and its clones. Separate clients and separate processes do not share it.
- The one-second default is this library's conservative choice. Last.fm's introduction only warns against making several calls per second continuously, and documents no exact allowance.

#### Retries

- A read is attempted up to `read_attempts` times in total, and only when `Error::retry()` is `Later` or `AfterBackoff`.
- `Later` covers transport failures and timeouts at any point, HTTP 5xx, and API codes 8, 11 and 16. `AfterBackoff` covers HTTP 429 and API code 29.
- Before retry `n` the client waits `retry_delay * 2^(n-1)`, five times that for `AfterBackoff`, and at least a numeric `Retry-After`. No wait is longer than five minutes. All of it is on top of pacing.
- A body that is too large, a body that does not decode and every other status or code are not retried.
- Code 8 ("operation failed") is on the list because Last.fm's own text for it is "please try again" and a long history read meets it often. One import of 241,000 scrobbles met it fifteen times, each under HTTP 500, and each time the same page was served when asked for again.
- An application that wants another rule sets `read_attempts(1)` and goes by `retry()`, `api_code()` and `http_status()` itself.
- **A write is never retried by the client**, whatever `retry()` advises. `delivery()` tells the caller what happened.

#### Timeouts and the cap

- `connect_timeout` bounds connecting, the TLS handshake included.
- `timeout` is the longest the client waits for one attempt, from the start to the last body byte, so a server that sends headers and then stalls is cut off. Either one is `ErrorKind::Timeout`.
- The timeout limits waiting, not polling. `tokio::time::timeout` polls the exchange before its timer, so a response that has already arrived in full and decoded when the future is next polled is returned even if the deadline has passed. It is not discarded, because for a write that would turn a known outcome into `Delivery::Unknown`.
- Pacing waits and retry delays are outside the per-attempt timeout. An overall deadline is a `tokio::time::timeout` around the whole loop, and cancelling a scan that way leaves the same page outstanding.
- The body is read up to the cap. A `Content-Length` over it fails before any body is read. An unannounced or chunked body fails as soon as it passes the cap.
- The cap bounds the length of the body the client keeps and the buffer it asks for. The transport's own buffer for the chunk in hand is on top, bounded by its read buffer.

#### Errors from the client

- Each carries the method.
- A failure to connect is `Transport` and certainly sent nothing. A failure after the request may have left is `Transport` with a possibly-sent request.
- The URL is stripped. The cause kept on the `source()` chain is a snapshot of the `Display` of each link, because the `Debug` of some errors underneath prints the address connected to.
- Each message is scrubbed before it is kept. Every occurrence of the client's own API key, secret and session key (and their form-encoded spellings) and of the prepared request's query string and body becomes `<redacted>`, and control characters are dropped. The message is cut to 256 bytes last, so a cut cannot leave half a credential.
- So whatever a dependency prints, no credential the client holds, and no query string or body it sent, appears on the chain. A dependency's message can still name the host.

#### Cancellation

Dropping a future abandons the attempt and closes its connection. Nothing shared is left half-updated, and a caller dropped while waiting for admission leaves nothing spent. For a write, a dropped future means the delivery is unknown. A dropped `next_page` leaves the scan where it was.

## Errors

One opaque `Error` with accessors, so categories can grow without breaking callers.

```rust
impl Error {
    pub fn kind(&self) -> ErrorKind;               // Copy, #[non_exhaustive]
    pub fn api_code(&self) -> Option<ApiErrorCode>;
    pub fn api_message(&self) -> Option<&str>;
    pub fn http_status(&self) -> Option<u16>;
    pub fn method(&self) -> Option<&'static str>;  // the Last.fm method name
    pub fn body(&self) -> Option<&[u8]>;           // the raw response body, capped at 64 KiB; it can hold a session key
    pub fn retry(&self) -> Retry;
    pub fn delivery(&self) -> Option<Delivery>;
}
```

| `ErrorKind` | Meaning |
|---|---|
| `Api` | Last.fm returned an error envelope, at any HTTP status |
| `Http` | Non-2xx status with no error envelope |
| `Decode` | The body was not the expected JSON, or a field was malformed |
| `Inconsistent` | A history page broke a scan rule |
| `Transport` | Connection, TLS or I/O failure |
| `Timeout` | A deadline passed |
| `BodyTooLarge` | The response exceeded the body cap |
| `InvalidRequest` | The request could not be built; nothing was sent |
| `ReadOnly` | A write was asked of a `Client`, or of credentials that do not allow writes; nothing was signed or sent |
| `Config` | The client could not be built: an empty API key, secret or session key, an unusable user agent, a zero timeout, cap or attempt count, a pacing interval over 24 hours, a test root that is not loopback, or an HTTP client that failed to initialise |

`ApiErrorCode` wraps the number and keeps unknown codes. Named constants exist for the documented ones, including `LOGIN_REQUIRED` (17) and `SUSPENDED_KEY` (26), which callers must be able to tell apart from an empty history.

`Display` gives a one-line message that is safe to log. Truncated diagnostics cut on a UTF-8 boundary.

### Retry

`Retry` is advice, and depends on the method as well as the code. The client acts on it for reads only.

| `Retry` | When |
|---|---|
| `Later` | Codes 11 and 16, and HTTP 5xx. For a read, also code 8, a transport failure and a timeout. For a write, a transport failure that certainly sent nothing |
| `AfterBackoff` | Code 29 and HTTP 429 |
| `AfterReauthentication` | Code 9 |
| `No` | Everything else |

Three cases are `No` whatever the row above says:

- `track.updateNowPlaying`, always.
- A write whose delivery is `Unknown`, which includes one answered with code 8.
- `auth.getSession` after a timeout, a transport failure after the request may have left, or code 8, because it spends its token.

### Delivery

`Delivery` says what a failed write did. `delivery()` is `None` for a read.

| `Delivery` | When |
|---|---|
| `NotSent` | Nothing left this process. A refusal as `ReadOnly` is one |
| `Rejected` | The service answered with an error envelope |
| `Unknown` | Sent, or possibly sent, with no readable answer. Also an answer of code 8, which is a failure and not a refusal |

An application persists `Unknown` and decides for itself. The library never replays it.

Every timeout is `Unknown`, including one while connecting, which in fact sent nothing. The error does not say where the deadline fell, and wrongly assuming `NotSent` is the dangerous mistake. A TLS handshake that never completes is the observed case: the connect timeout fires, the error is `Timeout`, and for a write `delivery()` is `Unknown`.

## Consumer sketches

### Backup application

```rust
let client = scrobl::Client::builder(api_key)
    .user_agent("example-backup/0.1 (+https://example.org)")
    .build()?;

// One closed window. Pages come as they are read, so they are provisional
// until `finish()` succeeds: stage what you write, and mark it complete only then.
let mut scan = client.user("rj").recent_tracks()
    .window(Window::new(from, to)?)
    .scan()?;

let mut pages = Vec::new();
while let Some(page) = scan.next_page().await? {
    pages.push(page);                        // page.scrobbles() are its rows, page.raw().body() the exact bytes
}
let summary = scan.finish()?;                // fails unless every rule held; now the pages are complete

// A count probe: one request, the window's total.
let total = client.user("rj").recent_tracks()
    .window(Window::new(from, to)?)
    .limit(1)
    .send().await?
    .attr().total();

match error.api_code() {
    Some(ApiErrorCode::LOGIN_REQUIRED) => { /* access or privacy change, not empty */ }
    _ => {}
}
```

A `Client` cannot write, so the backup application can hold a secret and a session key, to read a history its owner hides, without being able to change the account it backs up. Storage, scheduling, window selection and reconciliation are its own concern. The [command-line tool](#command-line-tool) is this application.

### Tauri music app

```rust
#[tauri::command]
async fn recent(client: tauri::State<'_, scrobl::Client>, user: String) -> Result<Vec<Row>, UiError> {
    let page = client.user(&user).recent_tracks().limit(50).send().await?;
    Ok(page.scrobbles().iter().map(Row::from).collect())
}
```

`Client` lives in Tauri managed state and is cloned into commands. The app builds a `Writer` at start-up and manages both. Managed state is keyed by type, so a command that takes `State<'_, scrobl::Client>` cannot write and one that takes `State<'_, scrobl::Writer>` says that it can. `UiError` is the app's own serializable type built from `kind()`, `api_code()` and `retry()`. Secrets never cross IPC because the library's types do not serialize them.

Authentication and writes (not built; shape only):

```rust
// Desktop flow. The library builds URLs and exchanges tokens; the app opens the browser and stores the session.
let token = client.auth().token().await?;
open_browser(token.authorization_url());
let session = client.auth().session(&token).await?;   // Err(code 14) until the user approves

let user = writer.with_session(session.key().clone());   // a Writer; typed writes exist only on it
let report = user.scrobble(&batch).await;             // at most 50 per call
match report {
    Ok(report) => for (sent, outcome) in report.items() {
        // outcome: Accepted { corrections } | Ignored { code, message }
    },
    Err(e) if e.delivery() == Some(Delivery::Unknown) => { /* persist as uncertain */ }
    Err(e) => { /* e.retry() says whether to keep it queued */ }
}
```

A scrobble reply is checked against the request: one outcome per item, `accepted + ignored` equal to the batch size. A reply that fails this check is `Delivery::Unknown` with the raw body, never a success. Both spellings of the outcome keys (`ignoredMessage`, `ignoredmessage`) are accepted. Submitted metadata is kept as sent, and corrections are reported next to it.

## Command-line tool

`scrobl-cli` builds the binary `scrobl`. It keeps one user's history in one SQLite file, and is the first consumer of the library.

- It has one command today: `scrobl sync --db <file> [--user <name>] [--full]`.
- It reads the API key from `SCROBL_API_KEY`, never from the command line.
- It exits with status 0, or with 1 and one line that says why.
- Each release is the crate on crates.io, a GitHub Release and a container image, `ghcr.io/kelchm/scrobl`, built from the `Dockerfile` at the root of the repository.

### The file

| Table | Holds |
|---|---|
| `scrobbles` | One row per scrobble: timestamp, artist, track, album, and the `mbid` Last.fm sent for each |
| `removed` | Scrobbles the file held that a later reading of the same time did not: the same columns, and when that was noticed |
| `loved` | One row per loved track, as the list stood at the last sync and in its order: when it was loved, artist, track, and the `mbid` Last.fm sent for each |
| `meta` | The user the file belongs to, and when the history and the loved tracks were last read |

- **The file keeps what was listened to, not what Last.fm sent.** A history can be read again in minutes, so nothing is kept in case it is wanted: not the pages, and not each row's JSON, which would be most of the file. Beyond the columns, a row as Last.fm sends it holds four addresses of one cover image, the address of the track's page, the time once more as text, and a flag that was 0 in every row recorded. A column that is wanted later costs one `--full`.
- **Nothing in the file says how it was read:** no windows, no mark of how far a sync got, nothing called done. A span of time `[from, to)` is written by one operation, in one transaction, which makes the file hold exactly what one finished scan read for it. So spans can be written twice and in any order. Whatever a sync leaves wrong, the next finds by its count or reads again because it is recent, so there is no lock, no check of the clock and nothing to resume.
- **Rows are never merged.** Two plays in one second are two rows. A span is replaced whole, oldest row first, so `ORDER BY timestamp, id` is the order of listening, and an `id` is not a lasting name for a scrobble.
- **Time, artist and track tell two scrobbles apart,** counting rows that share all three. The ids and the album are what Last.fm says about a scrobble, not which scrobble it is.
- **The file follows Last.fm and keeps what Last.fm drops.** A scrobble the file has and a new reading lacks was most likely deleted or edited at Last.fm. It leaves `scrobbles` and is copied to `removed` with the time, because a deletion made by mistake should not vanish from a backup overnight, and a scrobble more than about two weeks old cannot be sent to Last.fm again. Nothing prunes `removed`. It is the one thing in the file that cannot be read again. The loved tracks have no such table: a track can be loved again.
- **One user to a file,** named by the first sync and checked by every later one. A new file takes the name once Last.fm has answered for it, so a mistyped name does not claim the file.
- **A file is used only as this version wrote it.** It carries an `application_id` and a layout version in `user_version`, now 5. A file with nothing in it is new. Any other without both marks is refused and left as it is, an earlier version's included, because its history can be read again into a new file. What is in `removed` cannot, so a later change to this layout has to upgrade a file where it is.

### A sync

A sync reads up to the present, by this machine's clock, and has one way of dealing with any span of time:

1. It asks Last.fm how many scrobbles the span holds (one request with `limit=1`) and compares that with the file.
2. If they agree, the span is left alone.
3. If they differ and Last.fm counts more than 5000 scrobbles in the span, it is halved and each half is dealt with the same way, down to a single second.
4. Otherwise the span is read as one scan, and the file's copy of it is replaced.

That one rule is the first import, the daily run and the repair of old history:

- A new file agrees with Last.fm only where the history is empty, so everything is read, and years of silence cost a few counts.
- A sync that fails keeps every span it stored, and the next reads the rest.
- A scrobble added to or removed from a year long past changes that year's count, and the halving leads to it.

**The newest history is always read.** Equal counts do not prove equal rows, and the newest history is where that matters: a scrobble is dated when its track started and sent when it ended, or days later from a device that was offline.

- Every sync reads, whatever the counts say, from fourteen days before the newest scrobble the file holds up to the present. It reads that first, so that the newest history is in the file whatever becomes of the rest.
- Fourteen days because several clients report that Last.fm ignores a scrobble dated more than about two weeks back. Last.fm documents no number. If that holds, a scrobble that arrived since the last sync is dated after where the reading starts, however long ago that sync was.
- A file that holds no scrobble yet, or one dated after the present, is read from fourteen days before the present instead. A scrobble that arrived in a longer gap than that is found by its count, like a change to older history.
- The cost is that the fortnight before the newest scrobble is read by every sync, also when nothing has been played for months.
- `--full` reads every span that way.

**A count only decides where a sync looks.** What is stored comes from a scan, which the library checks in full. So nothing checks a count, and a sync does not count again when it is done.

- With the newest history always read, what a count is left to show is a scrobble deleted from older history and a span an interrupted import never reached.
- A count that is wrong costs a read that was not needed, or leaves a span as it is until a later count is right. For a deletion that means the file keeps a scrobble a while longer.
- Whether Last.fm's counts for the parts of a history always add up to its count for the whole is known for one history on one day: on 2026-11-10 a file put together from 74 spans, each read by itself, agreed with Last.fm's count for everything older than what every sync reads. If they did not add up, every sync would read some spans for nothing, and that is all that would show it.

**A span that changes while it is read.** A scrobble that arrives in a span during the read can make its pages disagree, and then the scan refuses them. That is no fault where the span reaches the present, so the span is read again, three times in all, before the sync fails, leaving that span as it was.

**Pages are asked for in the plain form,** without `extended=1`. The extended form sends every artist's `mbid` empty, and what it adds is the loved flag, which is not a fact about one scrobble. A track's `mbid` is stored as sent and is not always a MusicBrainz recording: of 24 sampled from one real history, six were, and the twelve others looked up each named a track on one particular release.

**Last, the loved tracks.** A sync reads them a thousand to a page and replaces the file's copy in one transaction. Nothing ties one page of that list to the next and nothing checks it: in a list longer than a page, a track loved or unloved during the read can leave a row listed twice or missing until the next sync. A loved track is tied to scrobbles by artist and track name, which is all the two lists share for certain.

### What it does not do yet

- It does not see a change that leaves a count as it was, in history older than what every sync reads: a scrobble removed and another added in one span, or an old play that was edited. `--full` reads everything and finds it, and nothing runs that by itself.
- It does not ask before following a large loss. If Last.fm stopped listing part of a history, those rows would move to `removed`, and come back when Last.fm listed them again.
- It reads a public history only. A hidden one needs the signed read, which is unverified.
- It keeps the loved tracks as they are now, not their history.
- No server for agents, no export and no offline check of the file.

## Coverage

Every method gets a typed model for v1. Fourteen have one so far: the thirteen reads of the `user` package and `library.getArtists`. All fourteen have been read from the live service through the client.

A model can be first written from the official samples and community documentation, and a model written that way can be wrong about the live service. Three things keep that honest:

- `endpoints.md` records the level each method has reached, and that table is the release claim. A model checked against a recorded response is at a different level from one derived from documentation.
- The exact response is always reachable next to the typed view, so a model that fails to decode never blocks a caller.
- A derived model is promoted only when an approved live pass confirms it.

| Level | Meaning |
|---|---|
| `inventoried` | In the table with its documented verb, credentials and parameters |
| `request-verified` | Request construction tested against the official parameter snapshot; raw response and errors handled |
| `typed-derived` | A typed model written from documentation, tested against derived fixtures only |
| `fixture-verified` | A typed model checked against a recorded response |
| `live-verified` | Exercised against Last.fm under explicit owner approval |

Each of the fourteen is tested against a response the owner recorded from their own account, with three exceptions:

- `user.getFriends`: a friends list names other people, so the fixture follows a recording with every name, date and address in it invented.
- The tagged albums of `user.getPersonalTags`: the owner has tagged no album, so that page is made the same way.
- `user.getWeeklyChartList`: the fixture is four weeks of the 1134 recorded.

### What the live passes showed

Each pass used an API key only. `tests/live.rs` repeats the reads through the client on request.

`user.getRecentTracks` on the owner's public history, on 2026-10-08:

- A page decodes, and `@attr` echoes the user, page and `perPage` asked for.
- A week of 29 scrobbles passes every scan rule as one page of 200 and as five pages of 7, with the same count.
- The bounds are `[from, to)`: for a scrobble at `t`, `[t, t+1)` holds it, `[t+1, ..)` does not, and `[from, t)` leaves it out.
- An empty window answers `total` 0 and `totalPages` 0.
- An unknown user is error 6 with HTTP 404, and a bad key is error 10 with HTTP 403: the envelope arrives on a non-2xx status.

The four top lists and `library.getArtists`, on 2026-11-10, read through the client by `tests/live.rs` and recorded with `curl`:

- A page of 1000 artists, albums, tracks and library artists decodes: every row has what its model requires.
- `library.getArtists` answers `limit=1` with `artist` as a single object. The top lists answer it with an array of one.
- A page far beyond the last is HTTP 200 with an empty array and the true `total` and `totalPages`, not an error.
- `@attr.rank` counts through the whole list: page 2 at `limit=5` holds ranks 6 to 10.
- `user.getTopArtists` refuses a `limit` of 0 or above 1000 and a `page` of 0 with error 6 and HTTP 400. Its message names the parameter and its range, but a message is text the network chose, which no error of this library prints. So the client refuses a 0 itself, naming the parameter. It sets no upper bound: the ranges differ by method, and `library.getArtists` applied `limit=2000`.
- 42 of the 1000 most played tracks have a `duration` of `"0"`, a length the service does not know. `TopTrack::duration()` reads it as `None`.
- The `@attr` of `user.getTopTags` holds the user and nothing else: no page and no total.

The weekly charts, the same day:

- The chart list held 1134 weeks of seven days, each ending where the next begins, on a Sunday at noon UTC. It starts in February 2005, before the account existed, and a week without a scrobble is an empty array.
- The three charts of the last listed week hold the same plays: 217 as 158 artists, as 184 albums and as 204 tracks.
- A chart is not always complete. The track chart of a week of 1390 plays had exactly 1000 rows holding 1167 of them, and nothing in the reply says so. That week's album chart held 1336, since a scrobble need not name an album.
- Without `from` and `to` the service chose the seven days up to the last noon UTC, which is not a listed week. `from` alone got the same answer.
- The week after the last listed one, still in progress, had a chart. A `from` later than the `to` was an empty array, not an error.
- The charts apply a `limit` although their pages document none. The three recorded fixtures were made with `limit=3`; the client sends none.

The profile, the friends, the tagged items and the loved tracks, the same day:

- The three counts of a profile are the totals of the three top lists: 10105 artists, 21774 albums and 47059 tracks.
- A row of `user.getFriends` is a shorter profile: no counts of artists, albums or tracks, and `playcount` and `subscriber` were 0 in all 26 rows recorded, of two users' friends. Its `registered.#text` is a date as text, where `user.getInfo` sends a number. A user who gives no country has the word `None` there, which the model reads as none.
- `user.getFriends` answered with the same bytes with `recenttracks=1` and without, so the client does not offer it. A page beyond the last is error 6 with HTTP 400, where `user.getTopArtists`, `library.getArtists` and `user.getPersonalTags` send an empty page. Its `limit` runs from 1 to 500.
- `user.getPersonalTags` nests its rows by kind (`taggings.artists.artist`, `taggings.albums.album`, `taggings.tracks.track`), and one reply holds one kind. Every tagged track recorded has the word `FIXME` as its `duration`, which the model leaves alone. Without a `taggingtype`, or with one it does not know, the method answers error 6.
- `user.getLovedTracks` refuses a `limit` of 0 with error 6, as the top lists do.

Pictures, in the same captures (77 replies, 9,961 rows, 35,542 pictures):

- An artist has five sizes (`small`, `medium`, `large`, `extralarge`, `mega`), an album, a track and a user four, and a track of a weekly chart three. The rows of the weekly artist and album charts have none.
- All 1000 most played artists, all 1000 library artists and all 1000 most played tracks have the same picture, a placeholder. Albums have covers of their own: 979 of the 1000 most played, and the other 21 have empty addresses.

The command-line tool's sync of 2026-11-10 read 242,328 scrobbles of that history and, through the model of `user.getLovedTracks`, its 259 loved tracks, which fit one page.

Not yet seen live:

- A signed read (`as_user`), and a hidden history.
- A now-playing row.
- Several scrobbles in one second across a page boundary.
- Rate limiting and `Retry-After`.
- Through the client, any method that has no typed model yet.

### Order of typing

What can be checked live without changing an account comes first:

1. The read models, the `user` reads first. They are written with shared helpers over the `de` primitives, which take their shape there. A wrong one never blocks a caller, because the exact response is always there, and its live pass changes nothing.
2. The authentication replies. A session key in a reply is modelled as `SessionKey`, so it is redacted like one the caller supplied.
3. The scrobble reply, with the checks described under [Tauri music app](#tauri-music-app), and then the other writes. A wrong model here loses data silently, and its live pass changes the owner's account.

A live acceptance pass, under explicit owner approval and with the owner's own captures as fixtures, comes before each remaining model is promoted.

## Rust version

The library's minimum is Rust 1.88, the lowest version that works: the code uses let chains, stable since 1.88, and its locked dependencies need 1.88 too. The command-line tool's is 1.89, because `rusqlite` 0.41 declares it and is the first to bundle a SQLite without CVE-2026-119375. CI runs the library's tests on 1.88, with and without the client, and the tool's on 1.89.

Tauri 2 itself declares 1.90, so the music app will need more than the library does. The library does not adopt that, because other users have no reason to be held to it. Raising the minimum is allowed in a minor release, only when the code or a dependency needs it, and never past a version less than six months old.

## Fixtures

Every fixture is listed in `crates/scrobl/fixtures/README.md` as one of:

- **recorded**: a real exchange captured by the repository owner from their own account, with the request, the date and the HTTP status. The body is kept byte for byte.
- **derived**: written from official documentation or a recorded fixture, with the source named.
- **synthetic**: invented, including everything the fake server generates.

The recorded fixtures are the owner's own captures of their own account. No credential is in any fixture. No test calls Last.fm except `tests/live.rs`, which runs only when the owner asks.

## Decisions

| Decision | State |
|---|---|
| Transport | Decided: one async `reqwest` client. No blocking adapter and no public transport trait until a consumer needs one. |
| Coverage | Decided: a typed model for every method in v1, with the verification level of each recorded in `endpoints.md`. |
| License | Decided: MIT. |
| Versions | Decided: one version for the library, the tool and its image, set in the workspace manifest, and one tag, `v` and the version, for a release. Before 1.0 a change that breaks the library's API raises the middle number and anything else the last. A release gives both crates the new number even when only one of them changed. |
| Minimum Rust | Decided: 1.88 for the library and 1.89 for the command-line tool, each tested in CI. See [Rust version](#rust-version). |
| Visibility | Decided: the repository is public. A version's tag publishes both crates to crates.io, then releases it on GitHub with a container image of the command-line tool. |
| Writes | Decided: a `Client` never writes; a `Writer`, from `ClientBuilder::build_writer`, is the only thing that does. Enforced in `prepare`. See [Reading and writing](#reading-and-writing). |
