//! Tests against a mock server on the loopback address, and against a stand-in for the llama.cpp
//! server program that is compiled on the spot.
#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::io::{BufRead, BufReader, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use application::CancellationToken;
use application::ports::{LlmEngine, LlmRequest};
use domain::ErrorCode;
use domain::tools::ai::{AiBackend, AiOptions, DEFAULT_MODEL, MODELS};

use super::*;

#[derive(Clone)]
enum Behavior {
    Stream(Vec<String>),
    Plain(String),
    Error(u16, String),
    Hang,
}

#[derive(Clone, Debug)]
struct Seen {
    path: String,
    headers: String,
    body: String,
}

struct Mock {
    port: u16,
    seen: Arc<Mutex<Vec<Seen>>>,
}

impl Mock {
    fn start(behavior: Behavior, models: &[&str]) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let seen = Arc::new(Mutex::new(Vec::new()));
        let log = Arc::clone(&seen);
        let models: Vec<String> = models.iter().map(|m| (*m).to_owned()).collect();
        std::thread::spawn(move || {
            for stream in listener.incoming() {
                let Ok(stream) = stream else { break };
                let behavior = behavior.clone();
                let log = Arc::clone(&log);
                let models = models.clone();
                std::thread::spawn(move || serve(stream, &behavior, &log, &models));
            }
        });
        Self { port, seen }
    }

    fn url(&self) -> String {
        format!("http://127.0.0.1:{}", self.port)
    }

    fn chats(&self) -> Vec<Seen> {
        self.seen
            .lock()
            .unwrap()
            .iter()
            .filter(|s| s.path == "/v1/chat/completions")
            .cloned()
            .collect()
    }
}

fn serve(stream: TcpStream, behavior: &Behavior, log: &Mutex<Vec<Seen>>, models: &[String]) {
    let mut reader = BufReader::new(stream.try_clone().unwrap());
    let mut first = String::new();
    if reader.read_line(&mut first).unwrap_or(0) == 0 {
        return;
    }
    let mut parts = first.split_whitespace();
    let method = parts.next().unwrap_or("").to_owned();
    let path = parts.next().unwrap_or("").to_owned();
    let mut headers = String::new();
    let mut length = 0usize;
    loop {
        let mut line = String::new();
        if reader.read_line(&mut line).unwrap_or(0) == 0 || line == "\r\n" {
            break;
        }
        if let Some(v) = line.to_ascii_lowercase().strip_prefix("content-length:") {
            length = v.trim().parse().unwrap_or(0);
        }
        headers.push_str(&line.to_ascii_lowercase());
    }
    let mut body = vec![0u8; length];
    reader.read_exact(&mut body).unwrap();
    log.lock().unwrap().push(Seen {
        path: path.clone(),
        headers,
        body: String::from_utf8_lossy(&body).into_owned(),
    });
    let mut out = stream;
    match (method.as_str(), path.as_str()) {
        ("GET", "/v1/models") => {
            let list: Vec<String> = models
                .iter()
                .map(|m| format!("{{\"id\":\"{m}\",\"object\":\"model\"}}"))
                .collect();
            let json = format!("{{\"data\":[{}]}}", list.join(","));
            let _ = write!(
                out,
                "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{json}",
                json.len()
            );
        }
        ("POST", "/v1/chat/completions") => match behavior {
            Behavior::Stream(pieces) => {
                let _ = write!(
                    out,
                    "HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nConnection: close\r\n\r\n"
                );
                for piece in pieces {
                    let data = serde_json::json!({"choices":[{"delta":{"content":piece}}]});
                    let _ = write!(out, "data: {data}\n\n");
                    let _ = out.flush();
                }
                let _ = write!(out, "data: [DONE]\n\n");
            }
            Behavior::Plain(text) => {
                let json =
                    serde_json::json!({"choices":[{"message":{"content":text}}]}).to_string();
                let _ = write!(
                    out,
                    "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{json}",
                    json.len()
                );
            }
            Behavior::Error(status, text) => {
                let json = serde_json::json!({"error":{"message":text}}).to_string();
                let _ = write!(
                    out,
                    "HTTP/1.1 {status} Error\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{json}",
                    json.len()
                );
            }
            Behavior::Hang => {
                let _ = write!(
                    out,
                    "HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nConnection: close\r\n\r\n"
                );
                let _ = out.flush();
                std::thread::sleep(Duration::from_secs(30));
            }
        },
        _ => {
            let _ = write!(
                out,
                "HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"
            );
        }
    }
}

fn request() -> LlmRequest {
    LlmRequest {
        system: "You summarize.".into(),
        user: "Some text.".into(),
        max_tokens: 200,
        json: false,
    }
}

fn ollama(url: &str, model: Option<&str>) -> AiOptions {
    AiOptions {
        backend: AiBackend::Ollama,
        model: model.map(str::to_owned),
        ollama_url: url.to_owned(),
        allow_ollama: true,
    }
}

fn engine() -> LlamaCpp {
    LlamaCpp::new(None, Vec::new(), Vec::new())
}

#[test]
fn a_streamed_answer_is_joined_and_the_request_carries_the_model() {
    let mock = Mock::start(
        Behavior::Stream(vec!["Hello".into(), ", ".into(), "world".into()]),
        &["llama3.2:3b"],
    );
    let mut r = request();
    r.json = true;
    let text = engine()
        .complete(
            &ollama(&mock.url(), Some("llama3.2:3b")),
            &r,
            &CancellationToken::new(),
        )
        .unwrap();
    assert_eq!(text, "Hello, world");
    let seen = mock.chats();
    assert_eq!(seen.len(), 1);
    let body: serde_json::Value = serde_json::from_str(&seen[0].body).unwrap();
    assert_eq!(body["model"], "llama3.2:3b");
    assert_eq!(body["stream"], true);
    assert_eq!(body["response_format"]["type"], "json_object");
    assert_eq!(body["messages"][0]["role"], "system");
    assert_eq!(body["messages"][1]["content"], "Some text.");
    assert_eq!(body["max_tokens"], 200);
    assert!(
        !seen[0].headers.contains("authorization"),
        "no key for Ollama"
    );
}

#[test]
fn the_first_model_is_used_when_none_is_named() {
    let mock = Mock::start(Behavior::Stream(vec!["ok".into()]), &["a:1b", "b:2b"]);
    let ai = ollama(&mock.url(), None);
    assert_eq!(engine().models(&ai).unwrap(), ["a:1b", "b:2b"]);
    engine()
        .complete(&ai, &request(), &CancellationToken::new())
        .unwrap();
    let body: serde_json::Value = serde_json::from_str(&mock.chats()[0].body).unwrap();
    assert_eq!(body["model"], "a:1b");
}

#[test]
fn no_models_in_ollama_is_a_clear_error() {
    let mock = Mock::start(Behavior::Stream(vec![]), &[]);
    let err = engine()
        .complete(
            &ollama(&mock.url(), None),
            &request(),
            &CancellationToken::new(),
        )
        .unwrap_err();
    assert_eq!(err.code, ErrorCode::ComponentMissing);
}

#[test]
fn an_answer_that_is_not_streamed_is_read_too() {
    let mock = Mock::start(Behavior::Plain("whole answer".into()), &["m"]);
    let text = engine()
        .complete(
            &ollama(&mock.url(), Some("m")),
            &request(),
            &CancellationToken::new(),
        )
        .unwrap();
    assert_eq!(text, "whole answer");
}

#[test]
fn a_server_error_is_reported_with_its_message() {
    let mock = Mock::start(Behavior::Error(500, "model not loaded".into()), &["m"]);
    let err = engine()
        .complete(
            &ollama(&mock.url(), Some("m")),
            &request(),
            &CancellationToken::new(),
        )
        .unwrap_err();
    assert!(
        err.message.contains("500") && err.message.contains("model not loaded"),
        "{}",
        err.message
    );
}

#[test]
fn a_server_that_is_not_running_is_reported_as_unreachable() {
    let port = {
        let l = TcpListener::bind("127.0.0.1:0").unwrap();
        l.local_addr().unwrap().port()
    };
    let err = engine()
        .complete(
            &ollama(&format!("http://127.0.0.1:{port}"), Some("m")),
            &request(),
            &CancellationToken::new(),
        )
        .unwrap_err();
    assert_eq!(err.code, ErrorCode::NetworkUnavailable);
}

#[test]
fn another_computer_is_refused_before_anything_is_sent() {
    let err = engine()
        .complete(
            &ollama("http://192.168.0.9:11434", Some("m")),
            &request(),
            &CancellationToken::new(),
        )
        .unwrap_err();
    assert_eq!(err.code, ErrorCode::OptionInvalid);
}

#[test]
fn a_cancelled_request_stops_while_the_model_is_silent() {
    let mock = Mock::start(Behavior::Hang, &["m"]);
    let cancel = CancellationToken::new();
    let flag = cancel.clone();
    std::thread::spawn(move || {
        std::thread::sleep(Duration::from_millis(400));
        flag.cancel();
    });
    let started = Instant::now();
    let err = engine()
        .complete(&ollama(&mock.url(), Some("m")), &request(), &cancel)
        .unwrap_err();
    assert_eq!(err.code, ErrorCode::JobCancelled);
    assert!(started.elapsed() < Duration::from_secs(5));
}

// ----- the llama.cpp server program ------------------------------------------------------------

/// A stand-in for `llama-server.exe`: reads `-m`, `--port` and `LLAMA_API_KEY`, becomes healthy
/// after a short while, and answers a chat with the contents of the model file.
const FAKE_SERVER: &str = r#"
use std::io::{BufRead, BufReader, Read, Write};
use std::net::TcpListener;
use std::time::{Duration, Instant};

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let mut model = String::new();
    let mut port = 0u16;
    let mut parallel = String::new();
    let mut context = String::new();
    for i in 0..args.len() {
        match args[i].as_str() {
            "-m" => model = args[i + 1].clone(),
            "--port" => port = args[i + 1].parse().unwrap(),
            "-np" => parallel = args[i + 1].clone(),
            "-c" => context = args[i + 1].clone(),
            _ => {}
        }
    }
    let key = std::env::var("LLAMA_API_KEY").unwrap_or_default();
    let exe = std::env::current_exe().unwrap();
    let marker = exe.parent().unwrap().join("starts.txt");
    let mut f = std::fs::OpenOptions::new().create(true).append(true).open(marker).unwrap();
    writeln!(f, "started np={parallel} c={context} key_len={}", key.len()).unwrap();
    let started = Instant::now();
    let listener = TcpListener::bind(("127.0.0.1", port)).unwrap();
    for stream in listener.incoming() {
        let Ok(mut stream) = stream else { continue };
        let mut reader = BufReader::new(stream.try_clone().unwrap());
        let mut first = String::new();
        reader.read_line(&mut first).unwrap();
        let path = first.split_whitespace().nth(1).unwrap_or("").to_string();
        let mut length = 0usize;
        let mut auth = String::new();
        loop {
            let mut line = String::new();
            reader.read_line(&mut line).unwrap();
            if line == "\r\n" || line.is_empty() { break; }
            let lower = line.to_ascii_lowercase();
            if let Some(v) = lower.strip_prefix("content-length:") { length = v.trim().parse().unwrap(); }
            if lower.starts_with("authorization:") { auth = line.split_whitespace().nth(2).unwrap_or("").to_string(); }
        }
        let mut body = vec![0u8; length];
        reader.read_exact(&mut body).unwrap();
        if path == "/health" {
            let ready = started.elapsed() > Duration::from_millis(500);
            let (code, text) = if ready { (200, "OK") } else { (503, "Loading") };
            write!(stream, "HTTP/1.1 {code} {text}\r\nContent-Length: 0\r\nConnection: close\r\n\r\n").unwrap();
        } else if path == "/v1/chat/completions" {
            if auth != key {
                write!(stream, "HTTP/1.1 401 Unauthorized\r\nContent-Length: 0\r\nConnection: close\r\n\r\n").unwrap();
                continue;
            }
            let reply = std::fs::read_to_string(&model).unwrap();
            write!(stream, "HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nConnection: close\r\n\r\n").unwrap();
            let data = format!("{{\"choices\":[{{\"delta\":{{\"content\":{:?}}}}}]}}", reply.trim());
            write!(stream, "data: {data}\n\ndata: [DONE]\n\n").unwrap();
        } else {
            write!(stream, "HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\nConnection: close\r\n\r\n").unwrap();
        }
    }
}
"#;

fn compile_fake(dir: &Path) -> PathBuf {
    let source = dir.join("fake_llama_server.rs");
    std::fs::write(&source, FAKE_SERVER).unwrap();
    let exe = dir.join("llama-server.exe");
    let status = std::process::Command::new("rustc")
        .args(["--edition", "2021", "-O", "-o"])
        .arg(&exe)
        .arg(&source)
        .status()
        .expect("rustc is on the path while cargo tests run");
    assert!(status.success());
    exe
}

fn bundled(models_dir: &Path, server_dir: &Path) -> LlamaCpp {
    LlamaCpp::new(
        None,
        vec![server_dir.to_path_buf()],
        vec![models_dir.to_path_buf()],
    )
}

fn install_model(models: &Path, id: &str, reply: &str) {
    let model = domain::tools::ai::model(id).unwrap();
    std::fs::create_dir_all(models.join(id)).unwrap();
    std::fs::write(models.join(id).join(model.file), reply).unwrap();
}

#[test]
fn the_server_is_started_once_with_the_model_and_a_key_and_stopped_on_drop() {
    let dir = tempfile::tempdir().unwrap();
    let server = dir.path().join("llama");
    std::fs::create_dir_all(&server).unwrap();
    compile_fake(&server);
    let models = dir.path().join("models");
    install_model(&models, DEFAULT_MODEL, "The summary.");
    let engine = bundled(&models, &server);
    assert!(engine.bundled_ready());
    assert_eq!(
        engine.models(&AiOptions::default()).unwrap(),
        [DEFAULT_MODEL.to_owned()]
    );

    let cancel = CancellationToken::new();
    let first = engine
        .complete(&AiOptions::default(), &request(), &cancel)
        .unwrap();
    assert_eq!(first, "The summary.");
    let second = engine
        .complete(&AiOptions::default(), &request(), &cancel)
        .unwrap();
    assert_eq!(second, "The summary.");

    let starts = std::fs::read_to_string(server.join("starts.txt")).unwrap();
    assert_eq!(
        starts.lines().count(),
        1,
        "one start for two requests: {starts}"
    );
    assert!(starts.contains("np=1"), "{starts}");
    assert!(starts.contains("c=8192"), "{starts}");
    assert!(
        starts.contains("key_len=48"),
        "a random key reached the server: {starts}"
    );

    let port = engine.sidecar.port().unwrap();
    drop(engine);
    std::thread::sleep(Duration::from_millis(300));
    assert!(
        TcpStream::connect(("127.0.0.1", port)).is_err(),
        "the server is stopped when the engine goes"
    );
}

#[test]
fn another_model_restarts_the_server() {
    let dir = tempfile::tempdir().unwrap();
    let server = dir.path().join("llama");
    std::fs::create_dir_all(&server).unwrap();
    compile_fake(&server);
    let models = dir.path().join("models");
    install_model(&models, MODELS[0].id, "small");
    install_model(&models, MODELS[1].id, "medium");
    let engine = bundled(&models, &server);
    let cancel = CancellationToken::new();
    let mut ai = AiOptions {
        model: Some(MODELS[0].id.to_owned()),
        ..AiOptions::default()
    };
    assert_eq!(engine.complete(&ai, &request(), &cancel).unwrap(), "small");
    ai.model = Some(MODELS[1].id.to_owned());
    assert_eq!(engine.complete(&ai, &request(), &cancel).unwrap(), "medium");
    let starts = std::fs::read_to_string(server.join("starts.txt")).unwrap();
    assert_eq!(starts.lines().count(), 2);
}

#[test]
fn missing_pieces_say_where_to_get_them() {
    let dir = tempfile::tempdir().unwrap();
    let engine = bundled(&dir.path().join("models"), &dir.path().join("llama"));
    let err = engine
        .complete(&AiOptions::default(), &request(), &CancellationToken::new())
        .unwrap_err();
    assert_eq!(err.code, ErrorCode::ComponentMissing);
    assert!(err.message.contains("Components"), "{}", err.message);

    let server = dir.path().join("llama");
    std::fs::create_dir_all(&server).unwrap();
    compile_fake(&server);
    let engine = bundled(&dir.path().join("models"), &server);
    let err = engine
        .complete(&AiOptions::default(), &request(), &CancellationToken::new())
        .unwrap_err();
    assert_eq!(err.code, ErrorCode::ComponentMissing);
    assert!(err.message.contains("Qwen2.5 1.5B"), "{}", err.message);
}

#[test]
fn a_cancelled_start_stops_the_server() {
    let dir = tempfile::tempdir().unwrap();
    let server = dir.path().join("llama");
    std::fs::create_dir_all(&server).unwrap();
    compile_fake(&server);
    let models = dir.path().join("models");
    install_model(&models, DEFAULT_MODEL, "x");
    let engine = bundled(&models, &server);
    let cancel = CancellationToken::new();
    cancel.cancel();
    let err = engine
        .complete(&AiOptions::default(), &request(), &cancel)
        .unwrap_err();
    assert_eq!(err.code, ErrorCode::JobCancelled);
    assert!(engine.sidecar.port().is_none());
}
