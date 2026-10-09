# scrobl design

Status: 2026-10-08. Transport and coverage are decided; license and minimum Rust are still open. See [decisions](#decisions). Code blocks are API sketches, not finished signatures.

## Scope

The 57 methods in the official Last.fm API index plus the web, desktop and mobile authentication flows, over JSON. Out of scope, each because the official page marks it deprecated or because it is only another encoding of the same methods: the Radio API, the Playlists API, Submissions Protocol 1.2.1, XML-RPC and XML output. Website scraping and history editing are out of scope permanently.

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
    client/            feature "client": reqwest executor, pacing, retries
  tests/               integration tests, fake Last.fm server
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
- The signature is the MD5 hex digest of every sent parameter except `format` and `callback`, as `name` then `value`, ordered by the UTF-8 bytes of the name, followed by the secret. Byte ordering puts `artist[10]` before `artist[1]`, which is what the service expects.
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

One concrete async client over `reqwest`. Cheap to clone, `Send + Sync`, every future `Send`. It spawns nothing and owns no runtime.

```rust
let client = scrobl::Client::builder(api_key)
    .user_agent("example-backup/0.1 (+https://example.org)")
    .build()?;
```

Fixed behaviour: HTTPS only, no redirects, `reqwest`'s own retries off, rustls. Configurable with defaults: connect timeout 10 s, total request timeout 30 s, response body cap 8 MiB, at most one request start per second across all clones, read retry budget of 3 attempts. The user agent defaults to `scrobl/<version>`.

Every attempt, including a retry, takes a pacing slot. A slot is reserved under a lock and waited for outside it, so concurrent callers queue rather than each resetting the clock. Reads are retried only for transport failures before a response, HTTP 5xx or 429, and API codes 11, 16 and 29. Writes are never retried by the client.

Cancellation: dropping a future abandons the request. Nothing shared is left half-updated; a reserved pacing slot is simply spent. For a write, a dropped future means the delivery is unknown.

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
| `Config` | The client could not be built |

`ApiErrorCode` wraps the number and keeps unknown codes. Named constants exist for the documented ones, including `LOGIN_REQUIRED` (17) and `SUSPENDED_KEY` (26), which callers must be able to tell apart from an empty history.

`Retry` is advice, and depends on the method as well as the code: `No`, `Later` (transient: codes 11 and 16, HTTP 5xx, pre-response transport failures of a read), `AfterBackoff` (29, HTTP 429) and `AfterReauthentication` (9). `track.updateNowPlaying` is always `No`.

`Delivery` says what a failed write did: `NotSent`, `Rejected` (the service answered with an error envelope) or `Unknown` (sent, or possibly sent, with no readable answer). `delivery()` returns `Option<Delivery>` and is `None` for a read. An application persists `Unknown` and decides for itself; the library never replays it, and `retry()` is `No` whenever delivery is `Unknown`.

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

Every method gets a typed model for v1. Recorded responses exist for only a few methods, so most models are first written from the official samples and community documentation, and a model written that way can be wrong about the live service. Three things keep that honest:

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
| License | Open; likely MIT. Until it is chosen there is no `LICENSE` file and the workspace is `publish = false`. |
| Minimum Rust | Open. 1.90, the Tauri 2 floor, is declared provisionally and checked in CI. |
| Visibility | Decided: private until v1 passes its gates. |
