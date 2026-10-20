//! The llama.cpp server as a separate process on this computer.
//!
//! Quirewise starts it on a free port of the loopback address, with a random key that only this
//! process knows (passed in the environment, not on the command line), keeps it for the next
//! request and stops it when it is no longer needed. The model file is a component.

use std::io::Read;
use std::net::TcpListener;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use application::AppError;
use application::CancellationToken;
use application::ports::ComponentStore;
use domain::ErrorCode;
use domain::tools::ai::{self, AiModel};

use crate::http::{Endpoint, healthy};

/// Id of the component that holds the server program.
pub const SERVER_COMPONENT: &str = "llama-cpp";
/// Longest the server may take to load a model.
const START_TIMEOUT: Duration = Duration::from_secs(5 * 60);
/// Tokens of context the server is started with: enough for a page or two of text and the answer.
const CONTEXT_TOKENS: &str = "8192";
/// Bytes of the server's messages that are kept, for the message when it does not start.
const KEEP_LOG: usize = 4 * 1024;

/// Where the pieces may be.
#[derive(Clone, Default)]
pub struct Locations {
    /// Finds installed components.
    pub components: Option<Arc<dyn ComponentStore>>,
    /// Folders that may hold `llama-server.exe` (directly, in `bin`, or in version folders).
    pub server_dirs: Vec<PathBuf>,
    /// Folders that may hold model files (directly, or in a folder named like the model id).
    pub model_dirs: Vec<PathBuf>,
}

/// Starts the server program. The worker's own limits (a memory cap and a CPU time cap per process)
/// suit parsing a PDF but not a language model, so the worker starts the server outside them with
/// a job of its own that ends the server when the worker goes. Anything else uses the plain start.
pub trait Spawner: Send + Sync {
    /// Starts `command`. The second value is kept for as long as the process lives (a job
    /// handle, for example) and dropped when it is stopped.
    fn spawn(&self, command: Command) -> std::io::Result<(Child, Box<dyn Send>)>;
}

/// Starts the server like any other program.
#[derive(Debug, Clone, Copy, Default)]
pub struct PlainSpawner;

impl Spawner for PlainSpawner {
    fn spawn(&self, mut command: Command) -> std::io::Result<(Child, Box<dyn Send>)> {
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            command.creation_flags(0x0800_0000);
        }
        command.spawn().map(|c| (c, Box::new(()) as Box<dyn Send>))
    }
}

/// A server that is running.
struct Running {
    /// Kept so that the job (if any) lives as long as the process.
    _hold: Box<dyn Send>,
    child: Child,
    port: u16,
    key: String,
    model: String,
}

/// Starts, reuses and stops the server.
pub struct Sidecar {
    locations: Locations,
    spawner: Arc<dyn Spawner>,
    state: Mutex<Option<Running>>,
}

fn version_folders(dir: &Path) -> Vec<PathBuf> {
    let mut found = vec![dir.to_path_buf()];
    if let Ok(entries) = std::fs::read_dir(dir) {
        let mut inner: Vec<PathBuf> = entries
            .filter_map(Result::ok)
            .map(|e| e.path())
            .filter(|p| p.is_dir())
            .collect();
        inner.sort();
        found.extend(inner.into_iter().rev());
    }
    found
}

impl Locations {
    /// The server program, when it is installed.
    pub fn server(&self) -> Option<PathBuf> {
        let mut dirs: Vec<PathBuf> = Vec::new();
        if let Some(dir) = self
            .components
            .as_ref()
            .and_then(|c| c.locate(SERVER_COMPONENT))
        {
            dirs.push(dir);
        }
        for dir in &self.server_dirs {
            dirs.extend(version_folders(dir));
        }
        dirs.iter()
            .flat_map(|d| {
                [
                    d.join("llama-server.exe"),
                    d.join("bin").join("llama-server.exe"),
                ]
            })
            .find(|p| p.is_file())
    }

    /// The file of a model of the catalog, when it is installed.
    pub fn model_file(&self, model: &AiModel) -> Option<PathBuf> {
        let mut dirs: Vec<PathBuf> = Vec::new();
        if let Some(dir) = self
            .components
            .as_ref()
            .and_then(|c| c.locate(&format!("model-{}", model.id)))
        {
            dirs.push(dir);
        }
        for dir in &self.model_dirs {
            dirs.push(dir.join(model.id));
            dirs.push(dir.clone());
        }
        dirs.iter()
            .map(|d| d.join(model.file))
            .find(|p| p.is_file())
    }
}

fn missing_server() -> AppError {
    AppError::new(
        ErrorCode::ComponentMissing,
        "The local AI program is not installed. Open Settings, then Components, to install it, or choose Ollama.",
    )
}

fn missing_model(model: &AiModel) -> AppError {
    AppError::new(
        ErrorCode::ComponentMissing,
        format!(
            "The model {} is not installed. Open Settings, then Components, to download it.",
            model.name
        ),
    )
}

fn free_port() -> Result<u16, AppError> {
    let listener = TcpListener::bind(("127.0.0.1", 0))
        .map_err(|e| AppError::from_io("find a free port", &e))?;
    let port = listener
        .local_addr()
        .map_err(|e| AppError::from_io("find a free port", &e))?
        .port();
    Ok(port)
}

fn random_key() -> Result<String, AppError> {
    let mut bytes = [0u8; 24];
    getrandom::fill(&mut bytes)
        .map_err(|_| AppError::internal("the system did not give random bytes"))?;
    Ok(bytes.iter().map(|b| format!("{b:02x}")).collect())
}

fn kill(child: &mut Child) {
    let _ = child.kill();
    let _ = child.wait();
}

fn drain(mut stream: impl Read + Send + 'static, log: Arc<Mutex<String>>) {
    std::thread::spawn(move || {
        let mut buf = [0u8; 1024];
        while let Ok(n) = stream.read(&mut buf) {
            if n == 0 {
                break;
            }
            if let Ok(mut text) = log.lock() {
                text.push_str(&String::from_utf8_lossy(&buf[..n]));
                if text.len() > KEEP_LOG {
                    let mut cut = text.len() - KEEP_LOG;
                    while !text.is_char_boundary(cut) {
                        cut += 1;
                    }
                    text.drain(..cut);
                }
            }
        }
    });
}

impl Sidecar {
    /// Creates a manager that has not started anything.
    pub fn new(locations: Locations, spawner: Arc<dyn Spawner>) -> Self {
        Self {
            locations,
            spawner,
            state: Mutex::new(None),
        }
    }

    /// Where the pieces are looked for.
    pub fn locations(&self) -> &Locations {
        &self.locations
    }

    /// The models of the catalog whose files are installed.
    pub fn installed_models(&self) -> Vec<String> {
        ai::MODELS
            .iter()
            .filter(|m| self.locations.model_file(m).is_some())
            .map(|m| m.id.to_owned())
            .collect()
    }

    /// The endpoint of a server that runs `model`, started now unless one already runs it.
    pub fn endpoint(
        &self,
        model_id: &str,
        cancel: &CancellationToken,
    ) -> Result<Endpoint, AppError> {
        let model = ai::model(model_id).ok_or_else(|| {
            AppError::new(
                ErrorCode::OptionInvalid,
                format!("{model_id} is not a model of the catalog."),
            )
        })?;
        let mut state = self
            .state
            .lock()
            .map_err(|_| AppError::internal("the model server state is broken"))?;
        if let Some(run) = state.as_mut() {
            let alive = matches!(run.child.try_wait(), Ok(None));
            if alive && run.model == model.id {
                return Ok(endpoint_of(run));
            }
            kill(&mut run.child);
            *state = None;
        }
        let server = self.locations.server().ok_or_else(missing_server)?;
        let file = self
            .locations
            .model_file(model)
            .ok_or_else(|| missing_model(model))?;
        let run = start(self.spawner.as_ref(), &server, &file, model, cancel)?;
        let endpoint = endpoint_of(&run);
        *state = Some(run);
        Ok(endpoint)
    }

    /// The port of the server that runs now, if any.
    #[cfg(test)]
    pub(crate) fn port(&self) -> Option<u16> {
        self.state.lock().ok()?.as_ref().map(|r| r.port)
    }

    /// Stops the server, when one runs.
    pub fn stop(&self) {
        if let Ok(mut state) = self.state.lock()
            && let Some(mut run) = state.take()
        {
            kill(&mut run.child);
        }
    }
}

impl Drop for Sidecar {
    fn drop(&mut self) {
        self.stop();
    }
}

fn endpoint_of(run: &Running) -> Endpoint {
    Endpoint {
        base: format!("http://127.0.0.1:{}", run.port),
        key: Some(run.key.clone()),
    }
}

fn start(
    spawner: &dyn Spawner,
    server: &Path,
    file: &Path,
    model: &AiModel,
    cancel: &CancellationToken,
) -> Result<Running, AppError> {
    let port = free_port()?;
    let key = random_key()?;
    let mut command = Command::new(server);
    command
        .arg("-m")
        .arg(file)
        .args(["--host", "127.0.0.1", "--port"])
        .arg(port.to_string())
        // One slot, so that the whole context belongs to the one request.
        .args(["-c", CONTEXT_TOKENS, "-np", "1"])
        .env("LLAMA_API_KEY", &key)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    if let Some(dir) = server.parent() {
        command.current_dir(dir);
    }
    let (mut child, hold) = spawner
        .spawn(command)
        .map_err(|e| AppError::from_io("start the local AI program", &e))?;
    let log = Arc::new(Mutex::new(String::new()));
    if let Some(out) = child.stdout.take() {
        drain(out, Arc::clone(&log));
    }
    if let Some(err) = child.stderr.take() {
        drain(err, Arc::clone(&log));
    }
    let run = Running {
        _hold: hold,
        child,
        port,
        key,
        model: model.id.to_owned(),
    };
    wait_until_ready(run, &log, cancel)
}

fn wait_until_ready(
    mut run: Running,
    log: &Arc<Mutex<String>>,
    cancel: &CancellationToken,
) -> Result<Running, AppError> {
    let started = Instant::now();
    let endpoint = endpoint_of(&run);
    loop {
        if cancel.is_cancelled() {
            kill(&mut run.child);
            return Err(AppError::cancelled());
        }
        if let Ok(Some(status)) = run.child.try_wait() {
            let tail = log.lock().map(|t| t.trim().to_owned()).unwrap_or_default();
            let last: String = tail
                .lines()
                .rev()
                .take(3)
                .collect::<Vec<_>>()
                .into_iter()
                .rev()
                .collect::<Vec<_>>()
                .join(" ");
            return Err(AppError::new(
                ErrorCode::Internal,
                format!(
                    "The local AI program stopped while loading the model ({status}). The computer may not have enough free memory for it. {last}"
                ),
            ));
        }
        if healthy(&endpoint) {
            return Ok(run);
        }
        if started.elapsed() > START_TIMEOUT {
            kill(&mut run.child);
            return Err(AppError::new(
                ErrorCode::EngineTimeout,
                "The model took too long to load. Try a smaller model.",
            ));
        }
        std::thread::sleep(Duration::from_millis(200));
    }
}
