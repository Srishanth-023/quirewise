//! The little of the Chrome DevTools protocol that printing a page needs.
//!
//! One WebSocket to the browser carries every command. Pages are addressed through a flat
//! session, so no second connection is opened.

use application::AppError;
use application::CancellationToken;
use base64::Engine as _;
use base64::engine::general_purpose::STANDARD;
use domain::ErrorCode;
use serde_json::{Value, json};
use std::net::TcpStream;
use std::time::{Duration, Instant};
use tungstenite::stream::MaybeTlsStream;
use tungstenite::{Message, WebSocket};

/// How long one wait for the browser's reply lasts before cancellation and the clock are checked.
const POLL: Duration = Duration::from_millis(100);
/// Longest answer accepted from the browser (a very long printed page).
const MAX_MESSAGE: usize = 1024 * 1024 * 1024;
/// Longest the printing itself may take after the page has loaded.
const PRINT_TIMEOUT: Duration = Duration::from_secs(5 * 60);
/// Schemes that never leave the computer.
const LOCAL_SCHEMES: [&str; 4] = ["file:", "data:", "blob:", "about:"];

/// What to print and how.
pub struct PrintJob<'a> {
    /// The address to load.
    pub address: &'a str,
    /// Longest the page may take to load.
    pub load_timeout: Duration,
    /// Stop every request that is not local.
    pub block_network: bool,
    /// The `Page.printToPDF` parameters.
    pub print: Value,
}

fn protocol_error(what: &str) -> AppError {
    AppError::new(
        ErrorCode::OutputVerifyFailed,
        format!("The web page printer answered in a way Quirewise does not understand ({what})."),
    )
}

fn timeout(what: &str) -> AppError {
    AppError::new(
        ErrorCode::EngineTimeout,
        format!(
            "The page did not finish {what} in time. Raise the load timeout, or check the address."
        ),
    )
}

struct Connection<'a> {
    socket: WebSocket<MaybeTlsStream<TcpStream>>,
    next_id: u64,
    session: Option<String>,
    cancel: &'a CancellationToken,
    block_network: bool,
    /// Events seen so far that were not the answer being waited for.
    loaded: bool,
}

impl<'a> Connection<'a> {
    fn open(
        endpoint: &str,
        cancel: &'a CancellationToken,
        block_network: bool,
    ) -> Result<Self, AppError> {
        let config = tungstenite::protocol::WebSocketConfig::default()
            .max_message_size(Some(MAX_MESSAGE))
            .max_frame_size(Some(MAX_MESSAGE));
        let (mut socket, _) = tungstenite::client::connect_with_config(endpoint, Some(config), 3)
            .map_err(|_| protocol_error("no connection"))?;
        if let MaybeTlsStream::Plain(stream) = socket.get_mut() {
            stream
                .set_read_timeout(Some(POLL))
                .map_err(|_| protocol_error("socket"))?;
        }
        Ok(Self {
            socket,
            next_id: 0,
            session: None,
            cancel,
            block_network,
            loaded: false,
        })
    }

    fn send(&mut self, method: &str, params: Value) -> Result<u64, AppError> {
        self.next_id += 1;
        let mut message = json!({ "id": self.next_id, "method": method, "params": params });
        if let Some(session) = &self.session {
            message["sessionId"] = json!(session);
        }
        self.socket
            .send(Message::text(message.to_string()))
            .map_err(|_| protocol_error("sending"))?;
        Ok(self.next_id)
    }

    /// Reads one message, or `None` when nothing arrived within the poll time.
    fn read(&mut self) -> Result<Option<Value>, AppError> {
        match self.socket.read() {
            Ok(Message::Text(text)) => serde_json::from_str(text.as_str())
                .map(Some)
                .map_err(|_| protocol_error("reading")),
            Ok(Message::Close(_)) => Err(protocol_error("the browser closed")),
            Ok(_) => Ok(None),
            Err(tungstenite::Error::Io(e))
                if matches!(
                    e.kind(),
                    std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut
                ) =>
            {
                Ok(None)
            }
            Err(_) => Err(protocol_error("the connection broke")),
        }
    }

    /// Reacts to events that matter whichever command is being waited for.
    fn handle_event(&mut self, message: &Value) -> Result<(), AppError> {
        match message["method"].as_str() {
            Some("Page.loadEventFired") => self.loaded = true,
            Some("Fetch.requestPaused") => {
                let id = message["params"]["requestId"].clone();
                let url = message["params"]["request"]["url"].as_str().unwrap_or("");
                let local = LOCAL_SCHEMES.iter().any(|s| url.starts_with(s));
                if local || !self.block_network {
                    self.send("Fetch.continueRequest", json!({ "requestId": id }))?;
                } else {
                    self.send(
                        "Fetch.failRequest",
                        json!({ "requestId": id, "errorReason": "BlockedByClient" }),
                    )?;
                }
            }
            _ => {}
        }
        Ok(())
    }

    /// Sends a command and waits for its answer, handling events on the way.
    fn call(&mut self, method: &str, params: Value, limit: Duration) -> Result<Value, AppError> {
        let id = self.send(method, params)?;
        let started = Instant::now();
        loop {
            self.cancel.check()?;
            if started.elapsed() > limit {
                return Err(timeout(method));
            }
            let Some(message) = self.read()? else {
                continue;
            };
            if message["id"].as_u64() == Some(id) {
                if let Some(error) = message.get("error") {
                    return Err(protocol_error(error["message"].as_str().unwrap_or("error")));
                }
                return Ok(message["result"].clone());
            }
            self.handle_event(&message)?;
        }
    }

    /// Waits until the page's load event has fired.
    fn wait_for_load(&mut self, limit: Duration) -> Result<(), AppError> {
        let started = Instant::now();
        while !self.loaded {
            self.cancel.check()?;
            if started.elapsed() > limit {
                return Err(timeout("loading"));
            }
            if let Some(message) = self.read()? {
                self.handle_event(&message)?;
            }
        }
        Ok(())
    }
}

/// Loads the page and returns the PDF the browser prints from it.
pub fn print(
    endpoint: &str,
    job: &PrintJob<'_>,
    cancel: &CancellationToken,
) -> Result<Vec<u8>, AppError> {
    let mut c = Connection::open(endpoint, cancel, job.block_network)?;
    let quick = Duration::from_secs(30);
    let target = c.call(
        "Target.createTarget",
        json!({ "url": "about:blank" }),
        quick,
    )?;
    let target_id = target["targetId"]
        .as_str()
        .ok_or_else(|| protocol_error("no page"))?
        .to_owned();
    let attached = c.call(
        "Target.attachToTarget",
        json!({ "targetId": target_id, "flatten": true }),
        quick,
    )?;
    c.session = Some(
        attached["sessionId"]
            .as_str()
            .ok_or_else(|| protocol_error("no session"))?
            .to_owned(),
    );
    c.call("Page.enable", json!({}), quick)?;
    // Every request is held until it is answered, so a blocked one never leaves the computer.
    c.call(
        "Fetch.enable",
        json!({ "patterns": [{ "urlPattern": "*" }] }),
        quick,
    )?;
    let navigated = c.call(
        "Page.navigate",
        json!({ "url": job.address }),
        job.load_timeout,
    )?;
    if let Some(reason) = navigated["errorText"].as_str() {
        return Err(AppError::new(
            ErrorCode::NetworkUnavailable,
            format!(
                "The page could not be loaded ({reason}). Check the address and your connection."
            ),
        ));
    }
    c.wait_for_load(job.load_timeout)?;
    // Web fonts load after the page does; wait for them so the text prints in its own font.
    let _ = c.call(
        "Runtime.evaluate",
        json!({ "expression": "document.fonts.ready.then(() => true)", "awaitPromise": true }),
        Duration::from_secs(10),
    );
    let printed = c.call("Page.printToPDF", job.print.clone(), PRINT_TIMEOUT)?;
    let data = printed["data"]
        .as_str()
        .ok_or_else(|| protocol_error("no PDF"))?;
    let pdf = STANDARD
        .decode(data)
        .map_err(|_| protocol_error("PDF data"))?;
    Ok(pdf)
}
