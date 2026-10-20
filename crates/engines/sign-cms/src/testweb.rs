//! Servers that answer over the "web" without a network, for tests: a time stamp server, and a
//! client that answers from a list of addresses.
// Test helpers fail the test at once when something is wrong, so they may unwrap.
#![allow(clippy::expect_used, clippy::unwrap_used, clippy::panic)]

use std::sync::{Arc, Mutex};

use application::AppError;
use application::signing::WebClient;
use const_oid::ObjectIdentifier as Oid;
use der::asn1::{GeneralizedTime, OctetString, Uint};
use der::{Decode, Encode, Sequence};

use crate::cmsdoc::{self, Content, ID_CT_TSTINFO};
use crate::keys::{Identity, load_pfx};
use crate::testkit::{Pki, Spec, now};
use crate::tsa::{MessageImprint, PkiStatusInfo, TimeStampReq, TimeStampResp};

type Handler = Box<dyn Fn(&[u8]) -> Result<Vec<u8>, AppError> + Send + Sync>;

/// A web client that answers from a list of addresses and remembers what it was asked.
#[derive(Default)]
pub struct MockWeb {
    routes: Mutex<Vec<(String, Handler)>>,
    asked: Mutex<Vec<String>>,
}

impl MockWeb {
    /// An empty client: every address is unreachable.
    pub fn new() -> Self {
        Self::default()
    }

    /// Answers requests to `url` with `handler`, which gets the body (empty for a GET).
    pub fn route(
        self,
        url: &str,
        handler: impl Fn(&[u8]) -> Result<Vec<u8>, AppError> + Send + Sync + 'static,
    ) -> Self {
        self.routes
            .lock()
            .expect("lock")
            .push((url.to_owned(), Box::new(handler)));
        self
    }

    /// The addresses that were asked, in order.
    pub fn asked(&self) -> Vec<String> {
        self.asked.lock().expect("lock").clone()
    }

    fn answer(&self, url: &str, body: &[u8]) -> Result<Vec<u8>, AppError> {
        self.asked.lock().expect("lock").push(url.to_owned());
        let routes = self.routes.lock().expect("lock");
        match routes.iter().find(|(u, _)| u == url) {
            Some((_, h)) => h(body),
            None => Err(AppError::new(
                domain::ErrorCode::NetworkUnavailable,
                "The server could not be reached.",
            )),
        }
    }
}

impl WebClient for MockWeb {
    fn get(&self, url: &str, _max_bytes: u64) -> Result<Vec<u8>, AppError> {
        self.answer(url, &[])
    }

    fn post(
        &self,
        url: &str,
        _content_type: &str,
        body: &[u8],
        _max_bytes: u64,
    ) -> Result<Vec<u8>, AppError> {
        self.answer(url, body)
    }
}

/// A web client that gives the same bytes to every request.
pub struct FixedAnswer(pub Vec<u8>);

impl WebClient for FixedAnswer {
    fn get(&self, _url: &str, _max_bytes: u64) -> Result<Vec<u8>, AppError> {
        Ok(self.0.clone())
    }

    fn post(
        &self,
        _url: &str,
        _content_type: &str,
        _body: &[u8],
        _max_bytes: u64,
    ) -> Result<Vec<u8>, AppError> {
        Ok(self.0.clone())
    }
}

#[derive(Sequence)]
struct TstInfo {
    version: u8,
    policy: Oid,
    message_imprint: MessageImprint,
    serial_number: Uint,
    gen_time: GeneralizedTime,
    #[asn1(optional = "true")]
    nonce: Option<Uint>,
}

/// How a test time stamp server behaves.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Mode {
    Normal,
    Refusing,
    OtherData,
    Replaying,
}

struct State {
    identity: Identity,
    mode: Mode,
    first: Mutex<Option<Vec<u8>>>,
    serial: Mutex<u32>,
    time: Mutex<i64>,
}

/// Where the test server listens.
pub const TSA_URL: &str = "http://tsa.test/ts";

/// A time stamp server with its own certificate authority.
pub struct Tsa {
    /// The authority behind the server.
    pub pki: Pki,
    state: Arc<State>,
}

impl Tsa {
    fn with(mode: Mode, time_stamping: bool) -> Self {
        let pki = Pki::build(&Spec {
            leaf_name: "Test TSA".to_owned(),
            root_name: "Test TSA Root".to_owned(),
            time_stamping,
            ..Spec::default()
        });
        let identity = load_pfx(&pki.pfx("x"), "x").expect("an identity");
        let state = Arc::new(State {
            identity,
            mode,
            first: Mutex::new(None),
            serial: Mutex::new(1000),
            time: Mutex::new(i64::try_from(now()).expect("a time")),
        });
        Self { pki, state }
    }

    /// A server that stamps what it is asked.
    pub fn new() -> Self {
        Self::with(Mode::Normal, true)
    }

    /// A server whose certificate does not say it is for time stamps.
    pub fn unmarked() -> Self {
        Self::with(Mode::Normal, false)
    }

    /// A server that refuses every request.
    pub fn refusing() -> Self {
        Self::with(Mode::Refusing, true)
    }

    /// A server that stamps other data than it was sent.
    pub fn stamping_other_data() -> Self {
        Self::with(Mode::OtherData, true)
    }

    /// A server that answers every request after the first with its first answer.
    pub fn replaying() -> Self {
        Self::with(Mode::Replaying, true)
    }

    /// Sets the time the server says it is.
    pub fn set_time(&self, unix: i64) {
        *self.state.time.lock().expect("lock") = unix;
    }

    /// A web client with this server at [`TSA_URL`].
    pub fn client(&self) -> MockWeb {
        self.route_into(MockWeb::new())
    }

    /// Adds this server to a web client.
    pub fn route_into(&self, web: MockWeb) -> MockWeb {
        let state = Arc::clone(&self.state);
        web.route(TSA_URL, move |body| respond(&state, body))
    }

    /// The root certificate of the server, for trusting it in a test.
    pub fn root_der(&self) -> Vec<u8> {
        self.pki.root_der.clone()
    }
}

impl Default for Tsa {
    fn default() -> Self {
        Self::new()
    }
}

fn respond(state: &State, body: &[u8]) -> Result<Vec<u8>, AppError> {
    let req =
        TimeStampReq::from_der(body).map_err(|_| AppError::internal("reading a test request"))?;
    if state.mode == Mode::Refusing {
        let resp = TimeStampResp {
            status: PkiStatusInfo {
                status: 2,
                status_string: None,
                fail_info: None,
            },
            time_stamp_token: None,
        };
        return Ok(resp.to_der().expect("der"));
    }
    if state.mode == Mode::Replaying
        && let Some(first) = state.first.lock().expect("lock").clone()
    {
        return Ok(first);
    }
    let mut imprint = req.message_imprint.clone();
    if state.mode == Mode::OtherData {
        let len = imprint.hashed_message.as_bytes().len();
        imprint.hashed_message = OctetString::new(vec![9u8; len]).expect("octets");
    }
    let serial = {
        let mut s = state.serial.lock().expect("lock");
        *s += 1;
        *s
    };
    let time = *state.time.lock().expect("lock");
    let info = TstInfo {
        version: 1,
        policy: "1.2.3.4.1".parse().expect("oid"),
        message_imprint: imprint,
        serial_number: Uint::new(&serial.to_be_bytes()).expect("uint"),
        gen_time: GeneralizedTime::from_unix_duration(std::time::Duration::from_secs(
            u64::try_from(time).expect("time"),
        ))
        .expect("a time"),
        nonce: req.nonce.clone(),
    };
    let content = info.to_der().expect("der");
    let hash = state.identity.key.hash();
    let built = cmsdoc::sign(
        &state.identity,
        &Content {
            oid: ID_CT_TSTINFO,
            attached: Some(&content),
            digest: hash.digest(&content),
        },
    )
    .expect("a token");
    let token = built.to_der(Vec::new()).expect("der");
    let resp = TimeStampResp {
        status: PkiStatusInfo {
            status: 0,
            status_string: None,
            fail_info: None,
        },
        time_stamp_token: Some(der::Any::from_der(&token).expect("any")),
    };
    let out = resp.to_der().expect("der");
    let mut first = state.first.lock().expect("lock");
    if first.is_none() {
        *first = Some(out.clone());
    }
    Ok(out)
}

// ---------------------------------------------------------------------------------------------
// A real HTTP server on the loopback address, for end to end tests of the whole stack.

use std::io::{Read, Write};
use std::net::TcpListener;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

type PathHandler = Box<dyn Fn(&str, &[u8]) -> Option<Vec<u8>> + Send + Sync>;

/// A server that listens until it is dropped. Bind first to learn the address, then start it.
pub struct TestServer {
    listener: TcpListener,
    stop: Arc<AtomicBool>,
}

impl TestServer {
    /// Binds to a free port on the loopback address.
    pub fn bind() -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").expect("a port");
        listener.set_nonblocking(true).expect("non-blocking");
        Self {
            listener,
            stop: Arc::new(AtomicBool::new(false)),
        }
    }

    /// The address, such as `http://127.0.0.1:5000`.
    pub fn base(&self) -> String {
        format!("http://{}", self.listener.local_addr().expect("an address"))
    }

    /// Starts answering. `handler` gets the path and the body and returns the answer, or `None`
    /// for a 404. The server stops when the returned guard is dropped.
    pub fn start(
        self,
        handler: impl Fn(&str, &[u8]) -> Option<Vec<u8>> + Send + Sync + 'static,
    ) -> ServerGuard {
        let handler: Arc<PathHandler> = Arc::new(Box::new(handler));
        let stop = Arc::clone(&self.stop);
        let listener = self.listener;
        let thread = std::thread::spawn(move || {
            while !stop.load(Ordering::SeqCst) {
                match listener.accept() {
                    Ok((stream, _)) => {
                        let handler = Arc::clone(&handler);
                        std::thread::spawn(move || serve_one(stream, &handler));
                    }
                    Err(_) => std::thread::sleep(Duration::from_millis(5)),
                }
            }
        });
        ServerGuard {
            stop: self.stop,
            thread: Some(thread),
        }
    }
}

/// Keeps a [`TestServer`] running.
pub struct ServerGuard {
    stop: Arc<AtomicBool>,
    thread: Option<std::thread::JoinHandle<()>>,
}

impl Drop for ServerGuard {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
        if let Some(t) = self.thread.take() {
            let _ = t.join();
        }
    }
}

fn serve_one(mut stream: std::net::TcpStream, handler: &PathHandler) {
    stream.set_nonblocking(false).ok();
    stream.set_read_timeout(Some(Duration::from_secs(5))).ok();
    let mut got: Vec<u8> = Vec::new();
    let mut buf = [0u8; 8192];
    let (path, body) = loop {
        let text = String::from_utf8_lossy(&got).into_owned();
        if let Some(end) = text.find("\r\n\r\n") {
            let wanted: usize = text
                .to_lowercase()
                .lines()
                .find_map(|l| l.strip_prefix("content-length:").map(str::to_owned))
                .and_then(|v| v.trim().parse().ok())
                .unwrap_or(0);
            if got.len() >= end + 4 + wanted {
                let path = text.split_whitespace().nth(1).unwrap_or("/").to_owned();
                break (path, got[end + 4..end + 4 + wanted].to_vec());
            }
        }
        match stream.read(&mut buf) {
            Ok(0) | Err(_) => return,
            Ok(n) => got.extend_from_slice(&buf[..n]),
        }
    };
    let (status, answer) = match handler(&path, &body) {
        Some(a) => ("200 OK", a),
        None => ("404 Not Found", Vec::new()),
    };
    let head = format!(
        "HTTP/1.1 {status}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
        answer.len()
    );
    let _ = stream.write_all(head.as_bytes());
    let _ = stream.write_all(&answer);
}

impl Tsa {
    /// The answer of this server to a request body.
    pub fn answer(&self, body: &[u8]) -> Result<Vec<u8>, AppError> {
        respond(&self.state, body)
    }
}
