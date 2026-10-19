//! HTTPS downloads for components. This is the only place the app talks to the network, and it
//! only runs after the person agreed to a download. Certificates are checked by Windows.

use application::AppError;
use application::ports::Downloader;
use domain::ErrorCode;
use std::io::Read;
use std::time::Duration;
use ureq::config::Config;
use ureq::tls::{TlsConfig, TlsProvider};

/// Longest a whole download may take.
const TOTAL_TIMEOUT: Duration = Duration::from_secs(60 * 60);
/// Longest the server may stay silent.
const IDLE_TIMEOUT: Duration = Duration::from_secs(30);
/// Size of the pieces handed to the caller.
const CHUNK: usize = 64 * 1024;

/// [`Downloader`] over `ureq` and the Windows TLS stack.
#[derive(Debug, Clone)]
pub struct HttpsDownloader {
    agent: ureq::Agent,
}

impl Default for HttpsDownloader {
    fn default() -> Self {
        Self::new()
    }
}

impl HttpsDownloader {
    /// Creates a downloader that refuses plain HTTP, also after a redirect.
    pub fn new() -> Self {
        let config = Config::builder()
            .https_only(true)
            .timeout_global(Some(TOTAL_TIMEOUT))
            .timeout_recv_response(Some(IDLE_TIMEOUT))
            .tls_config(
                TlsConfig::builder()
                    .provider(TlsProvider::NativeTls)
                    .build(),
            )
            .build();
        Self {
            agent: config.into(),
        }
    }
}

fn unavailable(why: &str) -> AppError {
    AppError::new(
        ErrorCode::NetworkUnavailable,
        format!(
            "The download did not work ({why}). Check your internet connection and try again, or install from a package file."
        ),
    )
}

impl Downloader for HttpsDownloader {
    fn download(
        &self,
        url: &str,
        max_bytes: u64,
        sink: &mut dyn FnMut(&[u8]) -> Result<(), AppError>,
    ) -> Result<(), AppError> {
        if !url.starts_with("https://") {
            return Err(unavailable("only secure addresses are used"));
        }
        let mut response = self.agent.get(url).call().map_err(|e| match e {
            ureq::Error::StatusCode(code) => unavailable(&format!("the server answered {code}")),
            _ => unavailable("the server could not be reached"),
        })?;
        let mut reader = response.body_mut().with_config().limit(max_bytes).reader();
        let mut buffer = vec![0u8; CHUNK];
        loop {
            let n = reader.read(&mut buffer).map_err(|_| {
                unavailable("the connection broke or the file is larger than announced")
            })?;
            if n == 0 {
                return Ok(());
            }
            sink(&buffer[..n])?;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plain_http_is_refused_before_any_connection() {
        let err = HttpsDownloader::new()
            .download("http://example.invalid/a", 10, &mut |_| Ok(()))
            .unwrap_err();
        assert_eq!(err.code, ErrorCode::NetworkUnavailable);
    }

    #[test]
    fn an_unreachable_host_is_a_network_error_not_a_crash() {
        let err = HttpsDownloader::new()
            .download("https://nonexistent.invalid/a", 10, &mut |_| Ok(()))
            .unwrap_err();
        assert_eq!(err.code, ErrorCode::NetworkUnavailable);
    }
}
