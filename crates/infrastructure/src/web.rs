//! Questions to servers that sign their answers: time stamps, OCSP and revocation lists. It only
//! runs after the person agreed to the connection for that run.
//!
//! Plain `http` is allowed here, unlike for downloads: these services are published over `http`
//! on purpose (a secure connection would need a certificate check that itself needs revocation
//! data), and every answer is signed and checked by the engine.

use application::AppError;
use application::signing::WebClient;
use domain::ErrorCode;
use std::time::Duration;
use ureq::config::Config;
use ureq::tls::{TlsConfig, TlsProvider};

/// Longest a whole request may take.
const TOTAL_TIMEOUT: Duration = Duration::from_secs(60);
/// Longest the server may stay silent.
const IDLE_TIMEOUT: Duration = Duration::from_secs(20);

/// [`WebClient`] over `ureq` and the Windows TLS stack.
#[derive(Debug, Clone)]
pub struct HttpWeb {
    agent: ureq::Agent,
}

impl Default for HttpWeb {
    fn default() -> Self {
        Self::new()
    }
}

impl HttpWeb {
    /// Creates a client.
    pub fn new() -> Self {
        let config = Config::builder()
            .https_only(false)
            .http_status_as_error(true)
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

fn unavailable(what: &str, why: &str) -> AppError {
    AppError::new(
        ErrorCode::NetworkUnavailable,
        format!(
            "The request to the {what} did not work ({why}). Check your internet connection, or sign at a lower level that needs no server."
        ),
    )
}

fn check_address(url: &str) -> Result<(), AppError> {
    if url.starts_with("http://") || url.starts_with("https://") {
        Ok(())
    } else {
        Err(unavailable(
            "server",
            "only http and https addresses are used",
        ))
    }
}

fn reason(error: &ureq::Error) -> String {
    match error {
        ureq::Error::StatusCode(code) => format!("the server answered {code}"),
        ureq::Error::Timeout(_) => "the server took too long".to_owned(),
        _ => "the server could not be reached".to_owned(),
    }
}

impl WebClient for HttpWeb {
    fn get(&self, url: &str, max_bytes: u64) -> Result<Vec<u8>, AppError> {
        check_address(url)?;
        let mut response = self
            .agent
            .get(url)
            .call()
            .map_err(|e| unavailable("server", &reason(&e)))?;
        response
            .body_mut()
            .with_config()
            .limit(max_bytes)
            .read_to_vec()
            .map_err(|_| unavailable("server", "the answer is too large or broke off"))
    }

    fn post(
        &self,
        url: &str,
        content_type: &str,
        body: &[u8],
        max_bytes: u64,
    ) -> Result<Vec<u8>, AppError> {
        check_address(url)?;
        let mut response = self
            .agent
            .post(url)
            .content_type(content_type)
            .send(body)
            .map_err(|e| unavailable("server", &reason(&e)))?;
        response
            .body_mut()
            .with_config()
            .limit(max_bytes)
            .read_to_vec()
            .map_err(|_| unavailable("server", "the answer is too large or broke off"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{Read, Write};
    use std::net::TcpListener;

    /// A one-shot server that answers every request with `answer` and sends the request bytes
    /// back through the channel.
    fn serve(answer: &'static [u8]) -> (String, std::sync::mpsc::Receiver<Vec<u8>>) {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let url = format!("http://{}/x", listener.local_addr().unwrap());
        let (tx, rx) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            if let Ok((mut stream, _)) = listener.accept() {
                let mut buf = vec![0u8; 8192];
                let mut got = Vec::new();
                stream
                    .set_read_timeout(Some(Duration::from_millis(500)))
                    .ok();
                // Read the headers, then as much body as they announce.
                loop {
                    let text = String::from_utf8_lossy(&got).to_lowercase();
                    if let Some(end) = text.find(
                        "

",
                    ) {
                        let wanted: usize = text
                            .lines()
                            .find_map(|l| l.strip_prefix("content-length:"))
                            .and_then(|v| v.trim().parse().ok())
                            .unwrap_or(0);
                        if got.len() >= end + 4 + wanted {
                            break;
                        }
                    }
                    match stream.read(&mut buf) {
                        Ok(0) | Err(_) => break,
                        Ok(n) => got.extend_from_slice(&buf[..n]),
                    }
                }
                let head = format!(
                    "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                    answer.len()
                );
                stream.write_all(head.as_bytes()).ok();
                stream.write_all(answer).ok();
                tx.send(got).ok();
            }
        });
        (url, rx)
    }

    #[test]
    fn a_get_returns_the_body() {
        let (url, _rx) = serve(b"hello");
        assert_eq!(HttpWeb::new().get(&url, 100).unwrap(), b"hello");
    }

    #[test]
    fn a_post_sends_the_body_with_its_content_type() {
        let (url, rx) = serve(b"answer");
        let got = HttpWeb::new()
            .post(&url, "application/timestamp-query", b"QUESTION", 100)
            .unwrap();
        assert_eq!(got, b"answer");
        let request = String::from_utf8_lossy(&rx.recv().unwrap()).to_lowercase();
        assert!(request.starts_with("post /x"), "{request}");
        assert!(
            request.contains("content-type: application/timestamp-query"),
            "{request}"
        );
        assert!(
            request.contains("question") || request.contains("QUESTION"),
            "{request}"
        );
    }

    #[test]
    fn an_answer_larger_than_the_limit_is_refused() {
        let (url, _rx) = serve(b"0123456789");
        let err = HttpWeb::new().get(&url, 4).unwrap_err();
        assert_eq!(err.code, ErrorCode::NetworkUnavailable);
    }

    #[test]
    fn other_schemes_are_not_used() {
        for url in ["ftp://x/y", "file:///etc/passwd", "gopher://x"] {
            assert_eq!(
                HttpWeb::new().get(url, 10).unwrap_err().code,
                ErrorCode::NetworkUnavailable,
                "{url}"
            );
        }
    }

    #[test]
    fn an_unreachable_server_is_a_clear_error() {
        let err = HttpWeb::new()
            .get("http://127.0.0.1:9/none", 10)
            .unwrap_err();
        assert_eq!(err.code, ErrorCode::NetworkUnavailable);
        assert!(err.message.contains("internet"), "{}", err.message);
    }
}
