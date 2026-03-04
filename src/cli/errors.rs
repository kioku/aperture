//! Error display formatting for the CLI.
//!
//! Error output is written directly to `stderr` rather than routed through
//! `tracing`. A tracing subscriber may suppress output depending on the
//! configured log level and would add unwanted structure (timestamps, targets,
//! etc.) to user-facing messages.
//!
//! The formatting logic lives in `write_error<W: Write>`, which accepts an
//! arbitrary writer so tests can capture output without redirecting the
//! process-global stderr. The public `print_error` function wires that writer
//! to `stderr`. `write_error` is private; the test submodule accesses it
//! directly as a child of this module. The `eprintln!` call in
//! `print_error_with_json` is excluded from the `no-println` lint via the
//! rule's `ignores` list.

use crate::constants;
use crate::error::Error;

/// Prints an error message, either as JSON or user-friendly format.
pub fn print_error_with_json(error: &Error, json_format: bool) {
    if !json_format {
        print_error(error);
        return;
    }
    let json_error = error.to_json();
    let Ok(json_output) = serde_json::to_string_pretty(&json_error) else {
        print_error(error);
        return;
    };
    eprintln!("{json_output}");
}

/// Prints a user-friendly error message with context and suggestions.
pub fn print_error(error: &Error) {
    write_error(error, &mut std::io::stderr());
}

/// Writes a user-friendly error message to `writer`.
///
/// Extracted from `print_error` so that tests can capture output without
/// redirecting the process-global stderr.
#[allow(clippy::too_many_lines)]
fn write_error<W: std::io::Write>(error: &Error, writer: &mut W) {
    match error {
        Error::Internal {
            kind,
            message,
            context,
        } => {
            let _ = writeln!(writer, "{kind}: {message}");
            let Some(ctx) = context else { return };
            if let Some(suggestion) = &ctx.suggestion {
                let _ = writeln!(writer, "\nHint: {suggestion}");
            }
        }
        Error::Io(io_err) => match io_err.kind() {
            std::io::ErrorKind::NotFound => {
                let _ = writeln!(
                    writer,
                    "File Not Found\n{io_err}\n\nHint: {}",
                    constants::ERR_FILE_NOT_FOUND
                );
            }
            std::io::ErrorKind::PermissionDenied => {
                let _ = writeln!(
                    writer,
                    "Permission Denied\n{io_err}\n\nHint: {}",
                    constants::ERR_PERMISSION
                );
            }
            _ => {
                let _ = writeln!(writer, "File System Error\n{io_err}");
            }
        },
        Error::Network(req_err) => {
            if req_err.is_connect() {
                let _ = writeln!(
                    writer,
                    "Connection Error\n{req_err}\n\nHint: {}",
                    constants::ERR_CONNECTION
                );
                return;
            }
            if req_err.is_timeout() {
                let _ = writeln!(
                    writer,
                    "Timeout Error\n{req_err}\n\nHint: {}",
                    constants::ERR_TIMEOUT
                );
                return;
            }
            if !req_err.is_status() {
                let _ = writeln!(writer, "Network Error\n{req_err}");
                return;
            }
            let Some(status) = req_err.status() else {
                let _ = writeln!(writer, "Network Error\n{req_err}");
                return;
            };
            match status.as_u16() {
                401 => {
                    let _ = writeln!(
                        writer,
                        "Authentication Error\n{req_err}\n\nHint: {}",
                        constants::ERR_API_CREDENTIALS
                    );
                }
                403 => {
                    let _ = writeln!(
                        writer,
                        "Permission Error\n{req_err}\n\nHint: {}",
                        constants::ERR_PERMISSION_DENIED
                    );
                }
                404 => {
                    let _ = writeln!(
                        writer,
                        "Not Found Error\n{req_err}\n\nHint: {}",
                        constants::ERR_ENDPOINT_NOT_FOUND
                    );
                }
                429 => {
                    let _ = writeln!(
                        writer,
                        "Rate Limited\n{req_err}\n\nHint: {}",
                        constants::ERR_RATE_LIMITED
                    );
                }
                500..=599 => {
                    let _ = writeln!(
                        writer,
                        "Server Error\n{req_err}\n\nHint: {}",
                        constants::ERR_SERVER_ERROR
                    );
                }
                _ => {
                    let _ = writeln!(writer, "HTTP Error\n{req_err}");
                }
            }
        }
        Error::Yaml(yaml_err) => {
            let _ = writeln!(
                writer,
                "YAML Parsing Error\n{yaml_err}\n\nHint: {}",
                constants::ERR_YAML_SYNTAX
            );
        }
        Error::Json(json_err) => {
            let _ = writeln!(
                writer,
                "JSON Parsing Error\n{json_err}\n\nHint: {}",
                constants::ERR_JSON_SYNTAX
            );
        }
        Error::Toml(toml_err) => {
            let _ = writeln!(
                writer,
                "TOML Parsing Error\n{toml_err}\n\nHint: {}",
                constants::ERR_TOML_SYNTAX
            );
        }
        Error::Anyhow(anyhow_err) => {
            let _ = writeln!(writer, "Error\n{anyhow_err}");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;
    use wiremock::matchers::{method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    fn capture(error: &Error) -> String {
        let mut buf = Vec::new();
        write_error(error, &mut buf);
        String::from_utf8(buf).expect("output is valid UTF-8")
    }

    // ---- Internal / non-Network variants ----

    #[test]
    fn test_internal_without_suggestion() {
        let err = Error::validation_error("bad input");
        let out = capture(&err);
        assert!(out.contains("Validation"));
        assert!(out.contains("bad input"));
    }

    #[test]
    fn test_internal_with_suggestion() {
        let err = Error::spec_not_found("my-api");
        let out = capture(&err);
        assert!(out.contains("Specification"));
        assert!(out.contains("my-api"));
        assert!(out.contains("Hint:"));
    }

    #[test]
    fn test_io_not_found() {
        let io_err = std::io::Error::new(std::io::ErrorKind::NotFound, "no such file");
        let err = Error::Io(io_err);
        let out = capture(&err);
        assert!(out.contains("File Not Found"));
        assert!(out.contains(constants::ERR_FILE_NOT_FOUND));
    }

    #[test]
    fn test_io_permission_denied() {
        let io_err = std::io::Error::new(std::io::ErrorKind::PermissionDenied, "access denied");
        let err = Error::Io(io_err);
        let out = capture(&err);
        assert!(out.contains("Permission Denied"));
        assert!(out.contains(constants::ERR_PERMISSION));
    }

    #[test]
    fn test_io_other() {
        let io_err = std::io::Error::new(std::io::ErrorKind::BrokenPipe, "broken pipe");
        let err = Error::Io(io_err);
        let out = capture(&err);
        assert!(out.contains("File System Error"));
    }

    #[test]
    fn test_yaml_error() {
        let yaml_err = serde_yaml::from_str::<serde_yaml::Value>("key: - value").unwrap_err();
        let err = Error::Yaml(yaml_err);
        let out = capture(&err);
        assert!(out.contains("YAML Parsing Error"));
        assert!(out.contains(constants::ERR_YAML_SYNTAX));
    }

    #[test]
    fn test_json_error() {
        let json_err = serde_json::from_str::<serde_json::Value>("{bad").unwrap_err();
        let err = Error::Json(json_err);
        let out = capture(&err);
        assert!(out.contains("JSON Parsing Error"));
        assert!(out.contains(constants::ERR_JSON_SYNTAX));
    }

    #[test]
    fn test_toml_error() {
        let toml_err = toml::from_str::<toml::Value>("key = ").unwrap_err();
        let err = Error::Toml(toml_err);
        let out = capture(&err);
        assert!(out.contains("TOML Parsing Error"));
        assert!(out.contains(constants::ERR_TOML_SYNTAX));
    }

    #[test]
    fn test_anyhow_error() {
        let err = Error::Anyhow(anyhow::anyhow!("something went wrong"));
        let out = capture(&err);
        assert!(out.contains("Error"));
        assert!(out.contains("something went wrong"));
    }

    // ---- Network variants (require live sockets) ----

    /// Produce a status-bearing `reqwest::Error` by hitting a wiremock endpoint
    /// with `error_for_status()`.
    async fn status_error(status: u16) -> reqwest::Error {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/err"))
            .respond_with(ResponseTemplate::new(status))
            .mount(&server)
            .await;

        reqwest::Client::new()
            .get(format!("{}/err", server.uri()))
            .send()
            .await
            .expect("request reached mock server")
            .error_for_status()
            .expect_err("status >= 400 must produce an error")
    }

    #[tokio::test]
    async fn test_network_connect_error() {
        // Port 1 is not in use on CI machines; produces ECONNREFUSED (is_connect).
        let req_err = reqwest::Client::new()
            .get("http://127.0.0.1:1/")
            .send()
            .await
            .expect_err("port 1 must refuse connections");
        assert!(req_err.is_connect(), "expected a connect error");
        let out = capture(&Error::Network(req_err));
        assert!(out.contains("Connection Error"));
        assert!(out.contains(constants::ERR_CONNECTION));
    }

    #[tokio::test]
    async fn test_network_timeout_error() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/slow"))
            .respond_with(ResponseTemplate::new(200).set_delay(Duration::from_secs(10)))
            .mount(&server)
            .await;

        let req_err = reqwest::Client::builder()
            .timeout(Duration::from_millis(1))
            .build()
            .unwrap()
            .get(format!("{}/slow", server.uri()))
            .send()
            .await
            .expect_err("request must time out");
        assert!(req_err.is_timeout(), "expected a timeout error");
        let out = capture(&Error::Network(req_err));
        assert!(out.contains("Timeout Error"));
        assert!(out.contains(constants::ERR_TIMEOUT));
    }

    #[tokio::test]
    async fn test_network_401() {
        let err = status_error(401).await;
        let out = capture(&Error::Network(err));
        assert!(out.contains("Authentication Error"));
        assert!(out.contains(constants::ERR_API_CREDENTIALS));
    }

    #[tokio::test]
    async fn test_network_403() {
        let err = status_error(403).await;
        let out = capture(&Error::Network(err));
        assert!(out.contains("Permission Error"));
        assert!(out.contains(constants::ERR_PERMISSION_DENIED));
    }

    #[tokio::test]
    async fn test_network_404() {
        let err = status_error(404).await;
        let out = capture(&Error::Network(err));
        assert!(out.contains("Not Found Error"));
        assert!(out.contains(constants::ERR_ENDPOINT_NOT_FOUND));
    }

    #[tokio::test]
    async fn test_network_429() {
        let err = status_error(429).await;
        let out = capture(&Error::Network(err));
        assert!(out.contains("Rate Limited"));
        assert!(out.contains(constants::ERR_RATE_LIMITED));
    }

    #[tokio::test]
    async fn test_network_503() {
        let err = status_error(503).await;
        let out = capture(&Error::Network(err));
        assert!(out.contains("Server Error"));
        assert!(out.contains(constants::ERR_SERVER_ERROR));
    }

    /// 400 (Bad Request) is a 4xx status that is not explicitly matched — exercises
    /// the `_ =>` fallback arm.
    #[tokio::test]
    async fn test_network_400_fallback() {
        let err = status_error(400).await;
        let out = capture(&Error::Network(err));
        assert!(out.contains("HTTP Error"));
    }
}
