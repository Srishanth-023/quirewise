//! The OpenAI-compatible chat API that llama.cpp's server and Ollama both speak, over plain http
//! to this computer. The answer is read as it streams, so a cancelled run stops at once.

use std::io::{BufRead, BufReader, Read};
use std::sync::mpsc;
use std::time::{Duration, Instant};

use application::AppError;
use application::CancellationToken;
use application::ports::LlmRequest;
use domain::ErrorCode;
use serde_json::{Value, json};

/// Longest the server may take to start answering: it reads the whole prompt first.
const FIRST_BYTE: Duration = Duration::from_secs(15 * 60);
/// Longest one answer may take.
const WHOLE_ANSWER: Duration = Duration::from_secs(30 * 60);
/// Longest answer kept, in bytes.
const MAX_ANSWER: usize = 512 * 1024;
/// Longest error text kept.
const MAX_ERROR: usize = 2_000;
/// How often a waiting request looks at the cancel flag.
const POLL: Duration = Duration::from_millis(100);

/// Where a server listens and how to prove who is asking.
#[derive(Debug, Clone)]
pub struct Endpoint {
    /// `http://host:port`, no trailing slash.
    pub base: String,
    /// Sent as a bearer token when set.
    pub key: Option<String>,
}

fn agent() -> ureq::Agent {
    ureq::Agent::config_builder()
        // The model is on this computer: never through a proxy that the environment names.
        .proxy(None)
        .http_status_as_error(false)
        .timeout_recv_response(Some(FIRST_BYTE))
        .timeout_global(Some(WHOLE_ANSWER))
        .build()
        .into()
}

fn unreachable(base: &str, why: &str) -> AppError {
    AppError::new(
        ErrorCode::NetworkUnavailable,
        format!("The language model at {base} did not answer ({why}). Is it running?"),
    )
}

fn clipped(mut text: String) -> String {
    if text.len() > MAX_ERROR {
        let mut cut = MAX_ERROR;
        while !text.is_char_boundary(cut) {
            cut -= 1;
        }
        text.truncate(cut);
    }
    text
}

fn server_error(status: u16, body: &str) -> AppError {
    let detail = serde_json::from_str::<Value>(body)
        .ok()
        .and_then(|v| {
            v["error"]["message"]
                .as_str()
                .or_else(|| v["error"].as_str())
                .map(str::to_owned)
        })
        .unwrap_or_else(|| body.trim().to_owned());
    AppError::new(
        ErrorCode::Internal,
        clipped(format!(
            "The language model answered with an error ({status}): {detail}"
        )),
    )
}

/// The ids or names of the models a server lists.
pub fn list_models(endpoint: &Endpoint) -> Result<Vec<String>, AppError> {
    let url = format!("{}/v1/models", endpoint.base);
    let mut request = agent().get(&url);
    if let Some(key) = &endpoint.key {
        request = request.header("Authorization", format!("Bearer {key}"));
    }
    let mut response = request
        .call()
        .map_err(|e| unreachable(&endpoint.base, &e.to_string()))?;
    let status = response.status().as_u16();
    let body = response
        .body_mut()
        .read_to_string()
        .map_err(|e| unreachable(&endpoint.base, &e.to_string()))?;
    if status != 200 {
        return Err(server_error(status, &body));
    }
    let value: Value = serde_json::from_str(&body).map_err(|_| {
        AppError::new(
            ErrorCode::Internal,
            "The model server sent an unreadable list.",
        )
    })?;
    Ok(value["data"]
        .as_array()
        .map(|list| {
            list.iter()
                .filter_map(|m| m["id"].as_str().map(str::to_owned))
                .collect()
        })
        .unwrap_or_default())
}

/// True once the server answers its health check: llama.cpp answers 200 when the model is loaded.
pub fn healthy(endpoint: &Endpoint) -> bool {
    let url = format!("{}/health", endpoint.base);
    agent()
        .get(&url)
        .call()
        .is_ok_and(|r| r.status().as_u16() == 200)
}

enum Message {
    Text(String),
    Done,
    Failed(AppError),
}

fn body(model: Option<&str>, request: &LlmRequest) -> String {
    let mut value = json!({
        "messages": [
            { "role": "system", "content": request.system },
            { "role": "user", "content": request.user },
        ],
        "max_tokens": request.max_tokens,
        "temperature": 0.2,
        "stream": true,
    });
    if let Some(model) = model {
        value["model"] = json!(model);
    }
    if request.json {
        value["response_format"] = json!({ "type": "json_object" });
    }
    value.to_string()
}

/// Reads a streamed answer and sends the pieces.
fn read_stream(
    base: &str,
    reader: impl Read,
    send: &mpsc::Sender<Message>,
) -> Result<(), AppError> {
    let mut lines = BufReader::new(reader).lines();
    let mut plain = String::new();
    let mut streamed = false;
    while let Some(line) = lines.next() {
        let line = line.map_err(|e| unreachable(base, &e.to_string()))?;
        let line = line.trim();
        if line.is_empty() || line.starts_with(':') {
            continue;
        }
        let Some(data) = line.strip_prefix("data:") else {
            // Not a stream: the server answered with one JSON document.
            plain.push_str(line);
            for rest in lines.by_ref() {
                let rest = rest.map_err(|e| unreachable(base, &e.to_string()))?;
                plain.push_str(&rest);
                if plain.len() > MAX_ANSWER * 2 {
                    break;
                }
            }
            break;
        };
        let data = data.trim();
        if data == "[DONE]" {
            break;
        }
        let Ok(value) = serde_json::from_str::<Value>(data) else {
            continue;
        };
        if let Some(message) = value["error"]["message"].as_str() {
            return Err(AppError::new(
                ErrorCode::Internal,
                clipped(format!("The language model stopped: {message}")),
            ));
        }
        if let Some(text) = value["choices"][0]["delta"]["content"].as_str() {
            streamed = true;
            if send.send(Message::Text(text.to_owned())).is_err() {
                return Ok(());
            }
        }
    }
    if !streamed && !plain.is_empty() {
        let value: Value = serde_json::from_str(&plain).map_err(|_| {
            AppError::new(
                ErrorCode::Internal,
                "The model server sent an unreadable answer.",
            )
        })?;
        if let Some(text) = value["choices"][0]["message"]["content"].as_str() {
            let _ = send.send(Message::Text(text.to_owned()));
        }
    }
    Ok(())
}

fn run(
    endpoint: &Endpoint,
    model: Option<&str>,
    request: &LlmRequest,
    send: &mpsc::Sender<Message>,
) -> Result<(), AppError> {
    let url = format!("{}/v1/chat/completions", endpoint.base);
    let mut call = agent()
        .post(&url)
        .header("Content-Type", "application/json");
    if let Some(key) = &endpoint.key {
        call = call.header("Authorization", format!("Bearer {key}"));
    }
    let mut response = call
        .send(body(model, request).as_bytes())
        .map_err(|e| unreachable(&endpoint.base, &e.to_string()))?;
    let status = response.status().as_u16();
    if status != 200 {
        let mut text = String::new();
        let _ = response
            .body_mut()
            .as_reader()
            .take(MAX_ERROR as u64)
            .read_to_string(&mut text);
        return Err(server_error(status, &text));
    }
    read_stream(&endpoint.base, response.body_mut().as_reader(), send)
}

/// The answer of the model. The request runs on its own thread so that `cancel` is looked at
/// while the model is still reading the prompt. `on_cancel` is called when the run is cancelled,
/// to stop a server that this program started.
pub fn chat(
    endpoint: &Endpoint,
    model: Option<&str>,
    request: &LlmRequest,
    cancel: &CancellationToken,
    on_cancel: impl FnOnce(),
) -> Result<String, AppError> {
    let (send, receive) = mpsc::channel();
    let thread_endpoint = endpoint.clone();
    let thread_model = model.map(str::to_owned);
    let thread_request = request.clone();
    std::thread::spawn(move || {
        let result = run(
            &thread_endpoint,
            thread_model.as_deref(),
            &thread_request,
            &send,
        );
        let _ = send.send(match result {
            Ok(()) => Message::Done,
            Err(e) => Message::Failed(e),
        });
    });
    let started = Instant::now();
    let mut answer = String::new();
    loop {
        match receive.recv_timeout(POLL) {
            Ok(Message::Text(piece)) => {
                answer.push_str(&piece);
                if answer.len() > MAX_ANSWER {
                    on_cancel();
                    return Err(AppError::new(
                        ErrorCode::Internal,
                        "The language model wrote far more than was asked for and was stopped.",
                    ));
                }
            }
            Ok(Message::Done) => return Ok(answer),
            Ok(Message::Failed(e)) => return Err(e),
            Err(mpsc::RecvTimeoutError::Timeout) => {}
            Err(mpsc::RecvTimeoutError::Disconnected) => {
                return Err(AppError::internal(
                    "the model request ended without an answer",
                ));
            }
        }
        if cancel.is_cancelled() {
            on_cancel();
            return Err(AppError::cancelled());
        }
        if started.elapsed() > WHOLE_ANSWER + Duration::from_secs(30) {
            on_cancel();
            return Err(AppError::new(
                ErrorCode::EngineTimeout,
                "The language model took too long. Try fewer pages or a smaller model.",
            ));
        }
    }
}
