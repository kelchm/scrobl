//! A fake Last.fm on a real socket, written by hand so that it can misbehave
//! at the socket level: stall, truncate, flood, hang up, redirect.
//!
//! provenance: synthetic. Nothing this server sends was recorded. A normal
//! answer is whatever [`respond_to`](super::respond_to) makes from the
//! synthetic [`Dataset`], the same function the pure tests use, so the same
//! dataset and fault switches serve both. The only other normal answer is a
//! bare `{}` for `track.love`, which is invented. Every other body is
//! scripted by the test that asks for it.
//!
//! Normally one request per connection, `Connection: close`. Each connection
//! gets a number, counted from 0 in the order it was accepted, and the script
//! picks the [`Behaviour`] by that number. After
//! [`keep_alive`](FakeLastfm::keep_alive) a connection serves request after
//! request and says `Connection: keep-alive`; the script then picks the
//! behaviour by the number of the request, counted from 0 over all
//! connections, and [`Recorded`] says which connection carried it.

use std::net::SocketAddr;
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, PoisonError};
use std::time::Duration;

use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::{Notify, watch};
use tokio::task::{JoinHandle, JoinSet};
use tokio::time::Instant;

use super::{Dataset, respond_to};

/// The most of a request head the server reads.
const MAX_HEAD: usize = 64 * 1024;

/// What the server does with one connection.
#[derive(Debug, Clone)]
pub enum Behaviour {
    /// Answer as the synthetic service would.
    Normal,
    /// Answer with this status, body and headers.
    Reply {
        status: u16,
        body: Vec<u8>,
        headers: Vec<(String, String)>,
    },
    /// Read the request, wait `by`, then answer 200 with this body.
    Delayed { by: Duration, body: Vec<u8> },
    /// Read the request, then say nothing and keep the socket open.
    StallBeforeResponse,
    /// Send the headers and the start of a body, then say nothing.
    StallMidBody,
    /// Advertise a long body, send a little of it and hang up.
    TruncatedBody,
    /// Send a body of `total` bytes, announced by `Content-Length` or sent
    /// chunked, until the client stops reading.
    OversizeBody { total: usize, chunked: bool },
    /// Read the request and hang up without a word.
    Close,
}

impl Behaviour {
    /// A reply with a status and a body.
    pub fn status(status: u16, body: impl Into<Vec<u8>>) -> Self {
        Self::Reply {
            status,
            body: body.into(),
            headers: Vec::new(),
        }
    }

    /// A reply with a status, a body and extra headers.
    pub fn status_with(status: u16, body: impl Into<Vec<u8>>, headers: &[(&str, &str)]) -> Self {
        Self::Reply {
            status,
            body: body.into(),
            headers: headers
                .iter()
                .map(|(n, v)| ((*n).to_owned(), (*v).to_owned()))
                .collect(),
        }
    }

    /// An error envelope with HTTP 200, as Last.fm sends some.
    pub fn api_error(code: u32) -> Self {
        Self::status(200, error_envelope(code))
    }

    /// A 302 to `location`.
    pub fn redirect(location: &str) -> Self {
        Self::status_with(302, "", &[("Location", location)])
    }

    /// An HTML page with a 5xx status, as a proxy in front of the service
    /// would send.
    pub fn html(status: u16) -> Self {
        Self::status_with(
            status,
            "<html><body>Synthetic upstream failure</body></html>",
            &[("Content-Type", "text/html")],
        )
    }
}

fn error_envelope(code: u32) -> Vec<u8> {
    format!(r#"{{"error":{code},"message":"Synthetic error"}}"#).into_bytes()
}

/// One request as the server saw it.
#[derive(Debug, Clone)]
pub struct Recorded {
    /// The connection number, from 0.
    pub number: usize,
    /// The place of this request on its connection, from 0. Always 0 unless
    /// the server keeps connections alive.
    pub index: usize,
    /// When the connection was accepted; for a later request on a kept-alive
    /// connection, when its head had been read.
    pub arrived: Instant,
    pub verb: String,
    pub path: String,
    pub query: Vec<(String, String)>,
    /// Header names in lower case.
    pub headers: Vec<(String, String)>,
    pub body: Vec<u8>,
}

impl Recorded {
    pub fn header(&self, name: &str) -> Option<&str> {
        let name = name.to_ascii_lowercase();
        self.headers
            .iter()
            .find(|(n, _)| *n == name)
            .map(|(_, v)| v.as_str())
    }

    pub fn query_param(&self, name: &str) -> Option<&str> {
        self.query
            .iter()
            .find(|(n, _)| n == name)
            .map(|(_, v)| v.as_str())
    }

    /// The body as form fields.
    pub fn form(&self) -> Vec<(String, String)> {
        form(&self.body)
    }

    pub fn form_param(&self, name: &str) -> Option<String> {
        self.form()
            .into_iter()
            .find(|(n, _)| n == name)
            .map(|(_, v)| v)
    }

    /// Every sent parameter, from the query or the body.
    pub fn params(&self) -> Vec<(String, String)> {
        if self.verb == "POST" {
            self.form()
        } else {
            self.query.clone()
        }
    }

    pub fn param(&self, name: &str) -> Option<String> {
        self.params()
            .into_iter()
            .find(|(n, _)| n == name)
            .map(|(_, v)| v)
    }
}

fn form(bytes: &[u8]) -> Vec<(String, String)> {
    form_urlencoded::parse(bytes)
        .map(|(k, v)| (k.into_owned(), v.into_owned()))
        .collect()
}

struct Script {
    steps: Vec<Behaviour>,
    /// What every connection after the steps gets.
    rest: Behaviour,
}

struct Shared {
    dataset: Dataset,
    script: Mutex<Script>,
    connections: AtomicUsize,
    /// Requests started, over all connections.
    served: AtomicUsize,
    keep_alive: AtomicBool,
    /// Counts up to tell idle kept-alive connections to hang up.
    hang_up: watch::Sender<u64>,
    requests: Mutex<Vec<Recorded>>,
    /// Connections whose handler has ended: the client hung up, or the
    /// script did.
    closed: AtomicUsize,
    /// Woken whenever a request is recorded or a connection ends.
    arrived: Notify,
    bytes_written: AtomicU64,
}

impl Shared {
    fn behaviour(&self, number: usize) -> Behaviour {
        let script = self.script.lock().unwrap_or_else(PoisonError::into_inner);
        script.steps.get(number).unwrap_or(&script.rest).clone()
    }
}

/// A running fake service. Dropping it stops it, and closes every socket,
/// stalled ones included.
pub struct FakeLastfm {
    addr: SocketAddr,
    shared: Arc<Shared>,
    task: JoinHandle<()>,
}

impl FakeLastfm {
    /// Starts serving `dataset`, normally.
    pub async fn start(dataset: Dataset) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let shared = Arc::new(Shared {
            dataset,
            script: Mutex::new(Script {
                steps: Vec::new(),
                rest: Behaviour::Normal,
            }),
            connections: AtomicUsize::new(0),
            served: AtomicUsize::new(0),
            keep_alive: AtomicBool::new(false),
            hang_up: watch::channel(0).0,
            requests: Mutex::new(Vec::new()),
            closed: AtomicUsize::new(0),
            arrived: Notify::new(),
            bytes_written: AtomicU64::new(0),
        });
        let task = tokio::spawn(accept_loop(listener, Arc::clone(&shared)));
        Self { addr, shared, task }
    }

    /// Gives connection 0 the first behaviour, connection 1 the second, and
    /// so on. Connections after the last behave normally.
    #[must_use]
    pub fn script(self, steps: impl IntoIterator<Item = Behaviour>) -> Self {
        self.shared
            .script
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .steps = steps.into_iter().collect();
        self
    }

    /// Makes every connection after the scripted ones behave like this
    /// instead of normally.
    #[must_use]
    pub fn then(self, rest: Behaviour) -> Self {
        self.shared
            .script
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .rest = rest;
        self
    }

    /// Serves many requests on one connection instead of one, and keeps it
    /// open until the client or [`hang_up_idle`](Self::hang_up_idle) ends it.
    #[must_use]
    pub fn keep_alive(self) -> Self {
        self.shared.keep_alive.store(true, Ordering::SeqCst);
        self
    }

    /// Closes every kept-alive connection that is waiting for its next
    /// request, as a server does when it times an idle connection out.
    pub fn hang_up_idle(&self) {
        self.shared.hang_up.send_modify(|count| *count += 1);
    }

    pub fn addr(&self) -> SocketAddr {
        self.addr
    }

    /// The root URL to give the client.
    pub fn base_url(&self) -> String {
        format!("http://{}/2.0/", self.addr)
    }

    /// Connections accepted, whether or not they sent a request.
    pub fn connections(&self) -> usize {
        self.shared.connections.load(Ordering::SeqCst)
    }

    /// The requests whose heads arrived, in connection order.
    pub fn requests(&self) -> Vec<Recorded> {
        let mut requests = self
            .shared
            .requests
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone();
        requests.sort_by_key(|r| (r.number, r.index));
        requests
    }

    pub fn request_count(&self) -> usize {
        self.shared
            .requests
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .len()
    }

    /// Body bytes the server managed to write, over all connections.
    pub fn bytes_written(&self) -> u64 {
        self.shared.bytes_written.load(Ordering::SeqCst)
    }

    /// Waits until `n` requests have arrived. The caller bounds the wait.
    pub async fn wait_for_requests(&self, n: usize) {
        self.wait_until(|| self.request_count() >= n).await;
    }

    /// Waits until `n` connections have ended, whichever side ended them.
    /// A flood that the client walked away from has stopped by then. The
    /// caller bounds the wait.
    pub async fn wait_for_closed(&self, n: usize) {
        self.wait_until(|| self.shared.closed.load(Ordering::SeqCst) >= n)
            .await;
    }

    async fn wait_until(&self, done: impl Fn() -> bool) {
        loop {
            // Ask to be woken before looking, so a change that lands in
            // between is not missed.
            let changed = self.shared.arrived.notified();
            tokio::pin!(changed);
            changed.as_mut().enable();
            if done() {
                return;
            }
            changed.await;
        }
    }
}

impl Drop for FakeLastfm {
    fn drop(&mut self) {
        // The handlers belong to the accept task, so this closes them too.
        self.task.abort();
    }
}

/// A root URL nothing listens on: a port that was bound and released.
pub fn dead_base_url() -> String {
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();
    drop(listener);
    format!("http://{addr}/2.0/")
}

async fn accept_loop(listener: TcpListener, shared: Arc<Shared>) {
    let mut handlers = JoinSet::new();
    while let Ok((stream, _)) = listener.accept().await {
        let arrived = Instant::now();
        let number = shared.connections.fetch_add(1, Ordering::SeqCst);
        handlers.spawn(handle(Arc::clone(&shared), stream, number, arrived));
        while handlers.try_join_next().is_some() {}
    }
}

async fn handle(shared: Arc<Shared>, stream: TcpStream, number: usize, arrived: Instant) {
    serve_connection(&shared, stream, number, arrived).await;
    shared.closed.fetch_add(1, Ordering::SeqCst);
    shared.arrived.notify_waiters();
}

async fn serve_connection(shared: &Shared, mut stream: TcpStream, number: usize, arrived: Instant) {
    let keep_alive = shared.keep_alive.load(Ordering::SeqCst);
    let mut hang_up = shared.hang_up.subscribe();
    let mut index = 0;
    loop {
        let request = tokio::select! {
            request = read_request(&mut stream, number, index, arrived) => request,
            // Only a connection that is waiting for its next request hangs up.
            Ok(()) = hang_up.changed(), if index > 0 => None,
        };
        let Some(mut request) = request else {
            return;
        };
        if index > 0 {
            request.arrived = Instant::now();
        }
        let params = request.params();
        shared
            .requests
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .push(request);
        shared.arrived.notify_waiters();

        let started = shared.served.fetch_add(1, Ordering::SeqCst);
        let step = if keep_alive { started } else { number };
        let reusable = serve_one(shared, &mut stream, &params, step, keep_alive).await;
        if !(keep_alive && reusable) {
            return;
        }
        index += 1;
    }
}

/// Answers one request as the script says. True if the connection can carry
/// another request.
async fn serve_one(
    shared: &Shared,
    stream: &mut TcpStream,
    params: &[(String, String)],
    step: usize,
    keep_alive: bool,
) -> bool {
    match shared.behaviour(step) {
        Behaviour::Normal => {
            let (status, body) = match param(params, "method") {
                Some("track.love") => love(params),
                _ => respond_to(&shared.dataset, params),
            };
            reply(stream, status, &[], &body, keep_alive).await.is_ok()
        }
        Behaviour::Reply {
            status,
            body,
            headers,
        } => {
            let headers: Vec<(&str, &str)> = headers
                .iter()
                .map(|(n, v)| (n.as_str(), v.as_str()))
                .collect();
            reply(stream, status, &headers, &body, keep_alive)
                .await
                .is_ok()
        }
        Behaviour::Delayed { by, body } => {
            tokio::time::sleep(by).await;
            reply(stream, 200, &[], &body, keep_alive).await.is_ok()
        }
        Behaviour::StallBeforeResponse => std::future::pending().await,
        Behaviour::StallMidBody => {
            let _ = write_head(stream, 200, &[("Content-Length", "1000")], false).await;
            let _ = stream.write_all(br#"{"recenttracks":"#).await;
            let _ = stream.flush().await;
            std::future::pending().await
        }
        Behaviour::TruncatedBody => {
            let _ = write_head(stream, 200, &[("Content-Length", "1000")], false).await;
            let _ = stream.write_all(br#"{"recenttracks":"#).await;
            let _ = stream.shutdown().await;
            false
        }
        Behaviour::OversizeBody { total, chunked } => {
            flood(shared, stream, total, chunked, keep_alive).await
        }
        Behaviour::Close => {
            let _ = stream.shutdown().await;
            false
        }
    }
}

fn param<'a>(params: &'a [(String, String)], name: &str) -> Option<&'a str> {
    params
        .iter()
        .find(|(n, _)| n == name)
        .map(|(_, v)| v.as_str())
}

/// The invented answer to `track.love`.
fn love(params: &[(String, String)]) -> (u16, Vec<u8>) {
    if param(params, "sk").is_none() || param(params, "api_sig").is_none() {
        return (403, error_envelope(9));
    }
    (200, b"{}".to_vec())
}

/// Sends `total` bytes in blocks until done or the client stops reading.
/// True if all of them went and the connection can carry another request.
async fn flood(
    shared: &Shared,
    stream: &mut TcpStream,
    total: usize,
    chunked: bool,
    keep_alive: bool,
) -> bool {
    const BLOCK: usize = 16 * 1024;
    let head = if chunked {
        write_head(stream, 200, &[("Transfer-Encoding", "chunked")], keep_alive).await
    } else {
        write_head(
            stream,
            200,
            &[("Content-Length", &total.to_string())],
            keep_alive,
        )
        .await
    };
    if head.is_err() {
        return false;
    }
    let block = vec![b' '; BLOCK];
    let mut sent = 0;
    while sent < total {
        let size = BLOCK.min(total - sent);
        let result = async {
            if chunked {
                stream.write_all(format!("{size:x}\r\n").as_bytes()).await?;
            }
            stream.write_all(&block[..size]).await?;
            if chunked {
                stream.write_all(b"\r\n").await?;
            }
            Ok::<(), std::io::Error>(())
        }
        .await;
        if result.is_err() {
            return false;
        }
        shared
            .bytes_written
            .fetch_add(size as u64, Ordering::SeqCst);
        sent += size;
    }
    if chunked && stream.write_all(b"0\r\n\r\n").await.is_err() {
        return false;
    }
    if keep_alive {
        return true;
    }
    let _ = stream.shutdown().await;
    false
}

fn reason(status: u16) -> &'static str {
    match status {
        200 => "OK",
        301 => "Moved Permanently",
        302 => "Found",
        303 => "See Other",
        400 => "Bad Request",
        403 => "Forbidden",
        404 => "Not Found",
        429 => "Too Many Requests",
        500 => "Internal Server Error",
        503 => "Service Unavailable",
        _ => "Synthetic",
    }
}

async fn write_head(
    stream: &mut TcpStream,
    status: u16,
    headers: &[(&str, &str)],
    keep_alive: bool,
) -> std::io::Result<()> {
    let mut head = format!("HTTP/1.1 {status} {}\r\n", reason(status));
    for (name, value) in headers {
        head.push_str(&format!("{name}: {value}\r\n"));
    }
    head.push_str(if keep_alive {
        "Connection: keep-alive\r\n\r\n"
    } else {
        "Connection: close\r\n\r\n"
    });
    stream.write_all(head.as_bytes()).await
}

/// A whole answer with a `Content-Length`, then a clean close, or not if the
/// connection is kept alive.
async fn reply(
    stream: &mut TcpStream,
    status: u16,
    headers: &[(&str, &str)],
    body: &[u8],
    keep_alive: bool,
) -> std::io::Result<()> {
    let length = body.len().to_string();
    let mut all = vec![
        ("Content-Type", "application/json; charset=utf-8"),
        ("Content-Length", length.as_str()),
    ];
    // A header the test sets replaces the default of the same name.
    all.retain(|(name, _)| {
        !headers
            .iter()
            .any(|(set, _)| set.eq_ignore_ascii_case(name))
    });
    all.extend_from_slice(headers);
    write_head(stream, status, &all, keep_alive).await?;
    stream.write_all(body).await?;
    if keep_alive {
        return stream.flush().await;
    }
    stream.shutdown().await
}

/// Reads one request: head, then a body of the announced length.
async fn read_request(
    stream: &mut TcpStream,
    number: usize,
    index: usize,
    arrived: Instant,
) -> Option<Recorded> {
    let mut buffer = Vec::new();
    let mut chunk = [0_u8; 4096];
    let head_end = loop {
        if let Some(at) = buffer.windows(4).position(|w| w == b"\r\n\r\n") {
            break at;
        }
        if buffer.len() > MAX_HEAD {
            return None;
        }
        let n = stream.read(&mut chunk).await.ok()?;
        if n == 0 {
            return None;
        }
        buffer.extend_from_slice(&chunk[..n]);
    };
    let head = String::from_utf8_lossy(&buffer[..head_end]).into_owned();
    let mut lines = head.split("\r\n");
    let mut request_line = lines.next()?.split(' ');
    let verb = request_line.next()?.to_owned();
    let target = request_line.next()?;
    let (path, query) = target.split_once('?').unwrap_or((target, ""));
    let headers: Vec<(String, String)> = lines
        .filter_map(|line| line.split_once(':'))
        .map(|(n, v)| (n.trim().to_ascii_lowercase(), v.trim().to_owned()))
        .collect();
    let length: usize = headers
        .iter()
        .find(|(n, _)| n == "content-length")
        .and_then(|(_, v)| v.parse().ok())
        .unwrap_or(0);

    let mut body = buffer[head_end + 4..].to_vec();
    while body.len() < length {
        let n = stream.read(&mut chunk).await.ok()?;
        if n == 0 {
            return None;
        }
        body.extend_from_slice(&chunk[..n]);
    }
    body.truncate(length);

    Some(Recorded {
        number,
        index,
        arrived,
        verb,
        path: path.to_owned(),
        query: form(query.as_bytes()),
        headers,
        body,
    })
}
