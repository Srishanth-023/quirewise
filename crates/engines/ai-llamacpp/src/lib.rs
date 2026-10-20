//! Local language model adapter (Phase 12): the llama.cpp server as a sidecar with a model file
//! from the Components screen, or an Ollama server that already runs on this computer.
//!
//! Both speak the OpenAI-compatible chat API over plain http on the loopback address; nothing is
//! sent anywhere else, and no proxy is used. The server is started when the first request
//! needs it and stopped when the engine is dropped.
#![forbid(unsafe_code)]

mod http;
mod server;

use application::AppError;
use application::CancellationToken;
use application::ports::{ComponentStore, LlmEngine, LlmRequest};
use domain::ErrorCode;
use domain::tools::ai::{AiBackend, AiOptions, loopback_endpoint};
use std::path::PathBuf;
use std::sync::Arc;

pub use http::Endpoint;
pub use server::{Locations, PlainSpawner, SERVER_COMPONENT, Sidecar, Spawner};

/// [`LlmEngine`] over llama.cpp and Ollama.
pub struct LlamaCpp {
    sidecar: Sidecar,
}

impl LlamaCpp {
    /// Creates the engine. `components` finds installed components; the folders list places
    /// that may hold a copy, such as a development tree.
    pub fn new(
        components: Option<Arc<dyn ComponentStore>>,
        server_dirs: Vec<PathBuf>,
        model_dirs: Vec<PathBuf>,
    ) -> Self {
        Self::with_spawner(components, server_dirs, model_dirs, Arc::new(PlainSpawner))
    }

    /// Like [`LlamaCpp::new`], starting the server program the way `spawner` says.
    pub fn with_spawner(
        components: Option<Arc<dyn ComponentStore>>,
        server_dirs: Vec<PathBuf>,
        model_dirs: Vec<PathBuf>,
        spawner: Arc<dyn Spawner>,
    ) -> Self {
        Self {
            sidecar: Sidecar::new(
                Locations {
                    components,
                    server_dirs,
                    model_dirs,
                },
                spawner,
            ),
        }
    }

    /// True when the program and at least one model are installed.
    pub fn bundled_ready(&self) -> bool {
        self.sidecar.locations().server().is_some() && !self.sidecar.installed_models().is_empty()
    }

    fn ollama(ai: &AiOptions) -> Result<Endpoint, AppError> {
        let (host, port) = loopback_endpoint(&ai.ollama_url).ok_or_else(|| {
            AppError::new(
                ErrorCode::OptionInvalid,
                "Only an Ollama server on this computer can be used.",
            )
        })?;
        Ok(Endpoint {
            base: format!("http://{host}:{port}"),
            key: None,
        })
    }
}

impl LlmEngine for LlamaCpp {
    fn models(&self, ai: &AiOptions) -> Result<Vec<String>, AppError> {
        match ai.backend {
            AiBackend::Bundled => Ok(self.sidecar.installed_models()),
            AiBackend::Ollama => http::list_models(&Self::ollama(ai)?),
        }
    }

    fn complete(
        &self,
        ai: &AiOptions,
        request: &LlmRequest,
        cancel: &CancellationToken,
    ) -> Result<String, AppError> {
        cancel.check()?;
        match ai.backend {
            AiBackend::Bundled => {
                let endpoint = self.sidecar.endpoint(ai.bundled_model(), cancel)?;
                http::chat(&endpoint, None, request, cancel, || self.sidecar.stop())
            }
            AiBackend::Ollama => {
                let endpoint = Self::ollama(ai)?;
                let model = match &ai.model {
                    Some(model) => model.clone(),
                    None => http::list_models(&endpoint)?.into_iter().next().ok_or_else(|| {
                        AppError::new(
                            ErrorCode::ComponentMissing,
                            "Ollama has no model installed. Run `ollama pull` for one, or choose the built-in model.",
                        )
                    })?,
                };
                http::chat(&endpoint, Some(&model), request, cancel, || {})
            }
        }
    }
}

#[cfg(test)]
mod tests;
