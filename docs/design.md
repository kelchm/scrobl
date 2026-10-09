# scrobl design

Status: 2026-10-08. Transport and coverage are decided; license and minimum Rust are still open. See [decisions](#decisions). Code blocks are API sketches, not finished signatures.

## Scope

The 57 methods in the official Last.fm API index (snapshot of 2026-10-05) plus the web, desktop and mobile authentication flows, over JSON. Out of scope, each because the official page marks it deprecated or because it is only another encoding of the same methods: the Radio API, the Playlists API, Submissions Protocol 1.2.1, XML-RPC and XML output. Website scraping and history editing are out of scope permanently.

Older documentation and other libraries also list `track.ban`, `track.unban`, `user.getArtistTracks` and `user.getNewReleases`. The index no longer has them, so they are not in the table and cannot be called through it. They are left out on purpose and would come back only if the index lists them again.

[`endpoints.md`](endpoints.md) lists every method with its verb, credentials, paging and verification status.

## Layout

One Cargo workspace, one library crate. A second crate (the backup application) can be added under `crates/` later without moving anything.

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
    model/             typed views, one module per API package
    history.rs         Window, RecentTracks, WindowScan: the validated recent-tracks scan, I/O-free
    response.rs        Response<T>: a typed value next to the exact response
    client/            feature "client": reqwest executor, pacing, retries, typed history access
  tests/               integration tests
    support/           synthetic dataset (pure) and server.rs, the fake Last.fm on a real socket
  fixtures/            labelled recorded / derived / synthetic
```

Features: `client` (default) enables the async client and pulls in `reqwest` and `tokio`. `--no-default-features` is the protocol core alone and must build and pass its tests.

## Layers

### Protocol core

No sockets, clocks, sleeps, tasks or runtime. Everything a transport needs is plain data.

```rust
use scrobl::protocol::{self, methods, Credentials, HttpRequest, HttpResponse, Request};

let request = Request::new(&methods::USER_GET_RECENT_TRACKS)
    .param("user", "rj")
    .param("limit", 200);

let credentials = Credentials::new(api_key);          // .with_secret(..), .with_session(..)
let http: HttpRequest = protocol::prepare(&credentials, &request)?;
// http.verb(), http.url(), http.body(): send with any HTTP client

let response = HttpResponse::new(status, body_bytes);
let raw: Raw = protocol::decode(&request, response)?; // Err for any Last.fm error envelope
```

Rules:

- `prepare` adds `method`, `api_key` and `format=json`, then `sk` and `api_sig` when the method's `Auth` needs them. It fails with `ErrorKind::InvalidRequest` when a required credential is missing, when the caller sets a reserved name (`method`, `api_key`, `api_sig`, `sk`, `format`, `callback`), or when a parameter name repeats.
- The signature is the MD5 hex digest of every sent parameter except `format`, `callback` and `api_sig` itself, as `name` then `value`, ordered by the UTF-8 bytes of the name, followed by the secret. Byte ordering puts `artist[10]` before `artist[1]`, which is what the service expects.
- `Request::as_user()` sends `sk` and `api_sig` with a method that does not require them, so a read is made as the session's user. This is how a hidden history would be read. The official pages describe the mode without documenting it, so it is unverified.
- `Get` puts parameters in the query string; `Post` puts all of them, including `method`, in a form-encoded body. The root is always `https://ws.audioscrobbler.com/2.0/`. A different root can be set only for tests.
- `decode` looks for the JSON error envelope (`{"error": N, "message": ".."}`) at every HTTP status before anything else. HTTP 200 carrying an error is an error. A non-2xx status without an envelope is `ErrorKind::Http`. A 2xx body that is not JSON is `ErrorKind::Decode`.
- `Raw` is the status, a small set of headers, the exact body bytes and the `Request` it answers. Typed views are decoded from it and never replace it. `Request` compares by method, parameters in order and the `as_user` flag, so a consumer can tell whether a response answers the request it holds.
- `HttpRequest`'s `Debug` and every error redact `api_key`, `api_sig`, `sk`, `token` and `password` values. No error carries a URL.

There is no public transport trait. A caller with another HTTP client uses `prepare` and `decode` directly.

### Typed views

Typed models are decoded from `Raw` on request. Fields are private with accessors. Each item keeps its own decoded JSON, so unknown fields and the difference between a missing and an empty field stay reachable:

```rust
let page = RecentTracksPage::decode(&raw)?;
for scrobble in page.scrobbles() {
    scrobble.timestamp();        // seconds since the epoch, always present for a scrobble
    scrobble.track().name();
    scrobble.artist().mbid();    // None when missing or empty
    scrobble.loved();            // None unless the page was requested with extended=1
    scrobble.json();             // &serde_json::Value, the row exactly as decoded
}
page.now_playing();              // Option<&NowPlaying>, never mixed into scrobbles()
page.attr();                     // user, page, per_page, total_pages, total
```

Decoding rules: counts and timestamps accept a JSON number or a numeric string, and nothing else. A field that can be one object or an array of them decodes to a list either way. A malformed value is an error naming the field, such as `recenttracks.track[3].date.uts`; it is never dropped or defaulted. A history row with neither a valid `date.uts` nor the now-playing flag is an error, not a skipped row. A row flagged now-playing is never a scrobble, whether or not it carries a date; a row whose flag is false is an ordinary historical row. `track` may be an array or a single object, and for an empty page absent, `[]` or `""`, but those three only when the page's own `@attr` allows no rows (`total` is 0, or `page` is beyond `totalPages`); a missing or misspelled `track` on a page that promises rows is an error, as is anything else. An `artist` that carries both `name` and `#text` must carry the same string in each, and each one present must be a string. An object that repeats a member name, at any depth, is a decode error rather than a silent last-one-wins; the error gives a line and column and never the name. `loved()` reflects the field as returned, which is present only on extended pages. `Debug` on a model leaves out the response text.

### History scan

`WindowScan` is the validated read of one time window of `user.getRecentTracks`. It is an I/O-free state machine, so every pagination rule is tested without a socket; the client drives it.

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

A scan is made only by `RecentTracks::scan`, from the builder's user, window, `extended`, `as_user` and `limit`. It has no setters, so a running scan cannot change: every request it issues is equal to the others apart from `page`. It pages by page number under bounds that never change, with the same `limit` on every request: the builder's `limit`, or 200 when unset, and never more than 200, the documented maximum. `scan` fails with `ErrorKind::InvalidRequest` when `limit` is outside 1 to 200 or when `page` was set, since a scan chooses its own pages. `RecentTracks::request` builds one such request on its own and rejects a `limit` outside 1 to 200 or a `page` of 0 the same way; nothing is clamped. It never uses a timestamp as a cursor and never removes duplicates, so same-second and identical scrobbles survive.

`accept` first checks that the response answers the outstanding request, by comparing `Raw::request` with it. A response to any other request, for another window, page, user, limit or `extended`, is `ErrorKind::Inconsistent` ("the response answers a different request") and is refused without being decoded. Each page must then satisfy all of the following, and any failure is `ErrorKind::Inconsistent` naming the rule, after which the scan yields nothing further:

1. `@attr.user` equals the requested user, ignoring case.
2. `@attr.page` equals the requested page and `@attr.perPage` equals the requested limit.
3. `@attr.total` and `@attr.totalPages` are present and identical on every page of the scan, and `totalPages` equals `ceil(total / perPage)`; when `total` is 0 it may be 0 or 1.
4. The number of scrobbles on the page is exactly what the totals imply: `perPage` on every page but the last, the remainder on the last. A now-playing row is not a scrobble and is not counted.
5. Every scrobble timestamp lies in `[from, to)`: `from` inclusive, `to` exclusive.
6. Timestamps never increase, within a page or from one page to the next.
7. At `finish`, the scrobbles seen equal `total`.

The scan ends after `totalPages` pages, or after page 1 when `total` is 0. It never issues a request with altered bounds. `next_request` returns the same request until a page is accepted, so a transport failure can be retried without skipping or repeating a page. A response that does not reach `accept` (an API error envelope, a non-JSON body) does not poison the scan, and an empty window is a successful scan with `total` 0, which an error is not. A scan without an upper bound is allowed. It sees scrobbles that arrive while it runs, which usually changes `total` and fails rule 3, but not always (see below); callers who want a more stable read fix `to` first.

The client drives it as `Scan` (below). `next_page` sends the outstanding request through the same paced, retried `call` as any read and hands the result to `accept`. An error from the transport or the API leaves the page outstanding, so calling `next_page` again sends the identical request. An error from `accept` kills the scan: later calls fail with `Inconsistent` and send nothing, and never read as a finished scan. The core offers no request once a scan is dead, which on its own would look like completion, so `Scan` remembers the failure and reports it. The request handed to `call` and then to `accept` is exactly the one `next_request` returned, since `accept` refuses a response to any other.

The `[from, to)` rule comes from one recorded third-party exchange, not a documented guarantee. That is why it is checked on every page rather than assumed.

#### What a completed scan does not prove

Success means every page satisfied the rules against the service's own totals at the time that page was read. It does not mean the rows are what the window held at any one moment. Plainly:

- A scrobble arriving and another being deleted between two page reads leaves `total` unchanged and can shift a row across a page boundary, giving one duplicate and one omission. A fixed `to` keeps arrivals at the head out of the window, but not backdated additions or deletions inside it.
- If the service changes the order of same-second rows between page reads, a row can repeat and another go missing with no rule broken.
- A page repeated under a new page number is caught by rule 6 only if its timestamps differ from the previous page's last. Listens can be identical, so equal rows cannot be rejected.
- A coherent wrong answer cannot be detected from the response alone: another user's rows under the right `@attr.user`, or rows left out with the totals reduced to match.
- Fixed bounds are not snapshot isolation.

An application that needs more reads the window again and compares the two reads. The library never deduplicates. The test suite pins the first three cases as documented behaviour, so a change in detection is a visible decision.

### Client

One concrete async client over `reqwest`. Cheap to clone (two `Arc`s), `Clone + Send + Sync + 'static`, every future `Send`. The crate spawns no task, starts no thread and owns no runtime; it runs on whatever Tokio runtime polls it, a current-thread one included, provided the runtime has its I/O and time drivers enabled (`enable_all`); without them the first call panics inside Tokio, which the client does not hide. `reqwest` itself spawns its connection tasks on that runtime, and resolving a host name uses the runtime's blocking pool. A client can be built outside a runtime.

```rust
impl Client {
    pub fn builder(api_key: ApiKey) -> ClientBuilder;
    pub fn with_session(&self, session: SessionKey) -> Client;   // shares the pool and the pacing clock
    pub async fn call(&self, request: &Request) -> Result<Raw, Error>;   // any of the 57 methods
    pub fn user(&self, name: impl Into<String>) -> User;
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
    pub fn build(self) -> Result<Client, Error>;
}

impl User { pub fn recent_tracks(&self) -> RecentTracksQuery; }
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

`Response<T>` is how every typed call returns its result: the typed value next to the exact `Raw`, so the bytes can be archived and a field the model lacks stays reachable. `value` and `into_value` reach the typed value without `Deref`, for a method of `T` that a method of `Response` would shadow. `Debug` of `Response`, `Raw` and `HttpResponse` shows header names and never header values, which are text the network chose; `header()` still returns them.

`scan` mirrors `RecentTracks::scan`: the query's window (`Window::ALL` when unset), `limit` as the constant page size (200 when unset), `extended` and `as_user` become the scan, and it cannot be changed afterwards. A `page` on the query, or a `limit` outside 1 to 200, is an `InvalidRequest` from `scan` itself, before anything is sent.

Fixed behaviour: HTTPS only (the client refuses a plain-HTTP URL before connecting), no redirects, `reqwest`'s own retries off, no cookies, rustls, and no response decompression: every `reqwest` decoder (`gzip`, `brotli`, `deflate`, `zstd`) is switched off explicitly, because Cargo unifies features and another crate in the program could otherwise turn one on, making the client send `Accept-Encoding` and `Raw::body` something other than the bytes the service sent. The one hook that changes the root, `ClientBuilder::base_url` (hidden, for tests), accepts only a literal loopback address with no user information, query or fragment, so it cannot turn HTTPS-only off for a real host or send a key elsewhere; anything else is a `Config` error that does not repeat the URL, and `Debug` says only whether a root is set. A 3xx is returned to `decode` and becomes `ErrorKind::Http`; the target of a `Location` receives nothing. The proxy environment variables (`HTTPS_PROXY`, `ALL_PROXY`, `NO_PROXY`) are honoured; through an HTTP proxy only a `CONNECT` naming the host and port is visible to the proxy and the key stays inside TLS. `ClientBuilder::no_proxy` opts out.

One attempt is: wait for admission, send, read the body up to the cap, `decode`. The request is built once, before the first admission, so an invalid request waits for nothing and sends nothing.

Pacing: every attempt, including a retry, is admitted at least one interval after the previous admission, across all clones. Admission is serialised and measured when it happens: a caller takes an async (`tokio::sync::Mutex`, first come first served) lock over the time of the last admission, sleeps whatever remains of the interval *while holding the lock*, reads the clock again, records that as the new last admission and releases. So the spacing holds however late a task is polled (a runtime stalled for several intervals delays the queued callers; it does not release them together), nothing is reserved ahead, and a caller dropped while queued or sleeping leaves no debt: the next one waits only for what remains since the last real admission. The arithmetic is on `Duration`s, an interval over 24 hours is a `Config` error, and a zero interval skips pacing entirely. "Start" means admission to the transport, measured in this process; connection setup can still make arrivals at the server bunch slightly.

Retries: a read is attempted up to `read_attempts` times in total, and only when `Error::retry()` is `Later` or `AfterBackoff`: transport failures and timeouts at any point, HTTP 5xx, API codes 11 and 16 (`Later`), HTTP 429 and API code 29 (`AfterBackoff`). Before retry `n` it waits `retry_delay * 2^(n-1)`, five times that for `AfterBackoff`, and at least a numeric `Retry-After`, none longer than five minutes, on top of pacing. A body that is too large, a body that does not decode and every other status or code are not retried. **A write is never retried by the client**, whatever `retry()` advises; `delivery()` tells the caller what happened.

Timeouts and the cap: `connect_timeout` bounds connecting, the TLS handshake included; `timeout` is the longest the client waits for one attempt, from the start to the last body byte, so a server that sends headers and then stalls is cut off. Either is `ErrorKind::Timeout`. The timeout limits waiting, not polling: `tokio::time::timeout` polls the exchange before its timer, so a response that has already arrived in full and decoded when the future is next polled is returned even if the deadline has passed. It is not discarded, because for a write that would turn a known outcome into `Delivery::Unknown`. Pacing waits and retry delays are outside the per-attempt timeout; an overall deadline is a `tokio::time::timeout` around the whole loop, and cancelling a scan that way leaves the same page outstanding. The body is read up to the cap: a `Content-Length` over it fails before any body is read, an unannounced or chunked body fails as soon as it passes the cap. The cap bounds the length of the body the client keeps and the buffer it asks for; the transport's own buffer for the chunk in hand is on top, bounded by its read buffer.

Errors from this layer carry the method. A failure to connect is `Transport` and certainly sent nothing; a failure after the request may have left is `Transport` with a possibly-sent request. The URL is stripped, and the cause kept on the `source()` chain is a snapshot of the `Display` of each link, because the `Debug` of some errors underneath prints the address connected to. Each message is scrubbed before it is kept: every occurrence of the client's own API key, secret and session key (and their form-encoded spellings) and of the prepared request's query string and body becomes `<redacted>`, control characters are dropped, and the message is cut to 256 bytes last, so a cut cannot leave half a credential. Whatever a dependency prints, no credential the client holds appears on the chain; no error text contains a URL or response text.

Cancellation: dropping a future abandons the attempt and closes its connection. Nothing shared is left half-updated, and a caller dropped while waiting for admission leaves nothing spent. For a write, a dropped future means the delivery is unknown. A dropped `next_page` leaves the scan where it was.

There is no blocking adapter. A synchronous caller, such as the backup application, drives this client on a current-thread Tokio runtime.

## Errors

One opaque `Error` with accessors, so categories can grow without breaking callers.

```rust
impl Error {
    pub fn kind(&self) -> ErrorKind;               // Copy, #[non_exhaustive]
    pub fn api_code(&self) -> Option<ApiErrorCode>;
    pub fn api_message(&self) -> Option<&str>;
    pub fn http_status(&self) -> Option<u16>;
    pub fn method(&self) -> Option<&'static str>;  // the Last.fm method name
    pub fn body(&self) -> Option<&[u8]>;           // the response body, capped at 64 KiB
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
| `Config` | The client could not be built: an unusable user agent, a zero timeout, cap or attempt count, a pacing interval over 24 hours, a test root that is not loopback, or an HTTP client that failed to initialise |

`ApiErrorCode` wraps the number and keeps unknown codes. Named constants exist for the documented ones, including `LOGIN_REQUIRED` (17) and `SUSPENDED_KEY` (26), which callers must be able to tell apart from an empty history.

`Retry` is advice, and depends on the method as well as the code: `No`, `Later` (transient: codes 11 and 16, HTTP 5xx, transport failures and timeouts of a read, and a transport failure of a write that certainly sent nothing; a write whose delivery is `Unknown` is `No`), `AfterBackoff` (29, HTTP 429) and `AfterReauthentication` (9). `track.updateNowPlaying` is always `No`. The client acts on the advice for reads only.

`Delivery` says what a failed write did: `NotSent`, `Rejected` (the service answered with an error envelope) or `Unknown` (sent, or possibly sent, with no readable answer). `delivery()` returns `Option<Delivery>` and is `None` for a read. An application persists `Unknown` and decides for itself; the library never replays it, and `retry()` is `No` whenever delivery is `Unknown`. Every timeout is `Unknown`, including one while connecting, which in fact sent nothing: the error does not say where the deadline fell, and wrongly assuming `NotSent` is the dangerous mistake. A TLS handshake that never completes is the observed case: the connect timeout fires, the error is `Timeout`, and for a write `delivery()` is `Unknown`.

`Display` gives a one-line message that is safe to log. Truncated diagnostics cut on a UTF-8 boundary.

## Consumer sketches

### Backup application

```rust
let client = scrobl::Client::builder(api_key)
    .user_agent("example-backup/0.1 (+https://example.org)")
    .build()?;

// One closed window, read completely or not at all.
let mut scan = client.user("rj").recent_tracks()
    .window(Window::new(from, to)?)
    .extended(true)
    .scan()?;

while let Some(page) = scan.next_page().await? {
    archive.write(page.raw().body())?;       // the exact bytes Last.fm sent
    for scrobble in page.scrobbles() { /* derived view */ }
}
let summary = scan.finish()?;                // fails unless every rule held

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

The backup application runs this on a current-thread Tokio runtime. Storage, scheduling, window selection and reconciliation are its own concern.

### Tauri music app

```rust
#[tauri::command]
async fn recent(client: tauri::State<'_, scrobl::Client>, user: String) -> Result<Vec<Row>, UiError> {
    let page = client.user(&user).recent_tracks().limit(50).send().await?;
    Ok(page.scrobbles().iter().map(Row::from).collect())
}
```

`Client` lives in Tauri managed state and is cloned into commands. `UiError` is the app's own serializable type built from `kind()`, `api_code()` and `retry()`; secrets never cross IPC because the library's types do not serialize them.

Authentication and writes (stage 4; shape only):

```rust
// Desktop flow. The library builds URLs and exchanges tokens; the app opens the browser and stores the session.
let token = client.auth().token().await?;
open_browser(token.authorization_url());
let session = client.auth().session(&token).await?;   // Err(code 14) until the user approves

let user = client.with_session(session.key().clone());
let report = user.track().scrobble(&batch).await;     // at most 50 per call
match report {
    Ok(report) => for (sent, outcome) in report.items() {
        // outcome: Accepted { corrections } | Ignored { code, message }
    },
    Err(e) if e.delivery() == Some(Delivery::Unknown) => { /* persist as uncertain */ }
    Err(e) => { /* e.retry() says whether to keep it queued */ }
}
```

A scrobble reply is checked against the request: one outcome per item, `accepted + ignored` equal to the batch size. A reply that fails this check is `Delivery::Unknown` with the raw body, never a success. Both spellings of the outcome keys (`ignoredMessage`, `ignoredmessage`) are accepted. Submitted metadata is kept as sent; corrections are reported next to it.

## Coverage

Every method gets a typed model for v1. One recorded response exists so far, for `user.getRecentTracks`, which is also the only method with a typed model, so most models will be first written from the official samples and community documentation, and a model written that way can be wrong about the live service. Three things keep that honest:

- `endpoints.md` records the level each method has reached, and that table is the release claim. A model checked against a recorded response is at a different level from one derived from documentation.
- The exact response is always reachable next to the typed view, so a model that fails to decode never blocks a caller.
- A derived model is promoted only when an approved live read confirms it.

Levels:

| Level | Meaning |
|---|---|
| `inventoried` | In the table with its documented verb, credentials and parameters |
| `request-verified` | Request construction tested against the official parameter snapshot; raw response and errors handled |
| `typed-derived` | A typed model written from documentation, tested against derived fixtures only |
| `fixture-verified` | A typed model checked against a recorded response |
| `live-verified` | Exercised against Last.fm under explicit owner approval |

What the live pass of 2026-10-08 showed for `user.getRecentTracks`, with an API key only and the owner's public history (`tests/live.rs` repeats it on request):

- A page decodes, and `@attr` echoes the user, page and `perPage` asked for.
- A week of 29 scrobbles passes every scan rule as one page of 200 and as five pages of 7, with the same count.
- The bounds are `[from, to)`: for a scrobble at `t`, `[t, t+1)` holds it, `[t+1, ..)` does not, and `[from, t)` leaves it out.
- An empty window answers `total` 0 and `totalPages` 0.
- An unknown user is error 6 with HTTP 404, and a bad key is error 10 with HTTP 403: the envelope arrives on a non-2xx status.

Not yet seen live: a signed read (`as_user`), a hidden history, a now-playing row, several scrobbles in one second across a page boundary, rate limiting and `Retry-After`, and any other method.

Order of typing, most costly to get wrong first:

1. The authentication replies. A session key in a reply is modelled as `SessionKey`, so it is redacted like one the caller supplied.
2. The scrobble reply, with the checks described under Errors. A wrong model here loses data silently.
3. The read models. Those are written with shared helpers over the `de` primitives; a wrong one never blocks a caller, because the exact response is always there.

A live acceptance pass, under explicit owner approval and with the owner's own captures as fixtures, comes before each remaining model is promoted.

## Rust version

The minimum is Rust 1.88, the lowest version that works: the code uses let chains, stable since 1.88, and the locked dependency tree needs 1.88 too. CI runs the whole test suite on it, with and without the client. Tauri 2 itself declares 1.90, so the music app will need more than the library does; the library does not adopt that, because the backup application and other users have no reason to be held to it. Raising it is allowed in a minor release, only when the code or a dependency needs it, and never past a version less than six months old.

## Fixtures

Every fixture is listed in `crates/scrobl/fixtures/README.md` as one of:

- **recorded**: a real exchange captured by someone else, with source repository, commit, path and license. Only what a test needs is kept.
- **derived**: written from official documentation or a recorded fixture, with the source named.
- **synthetic**: invented, including everything the fake server generates.

No fixture comes from the owner's account and no test calls Last.fm. Credentials in fixtures are obvious sentinels.

## Decisions

| Decision | State |
|---|---|
| Transport | Decided: one async `reqwest` client. No blocking adapter and no public transport trait until a consumer needs one. |
| Coverage | Decided: a typed model for every method in v1, with the verification level of each recorded in `endpoints.md`. |
| License | Decided: MIT. The workspace stays `publish = false` until the owner releases. |
| Minimum Rust | Decided: 1.88, tested in CI. See Rust version. |
| Visibility | Decided: the repository is public. Nothing is published to crates.io yet. |
