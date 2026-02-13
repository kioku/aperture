//! Error display formatting for the CLI.

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
#[allow(clippy::too_many_lines)]
pub fn print_error(error: &Error) {
    match error {
        Error::Internal {
            kind,
            message,
            context,
        } => {
            eprintln!("{kind}: {message}");
            let Some(ctx) = context else { return };
            if let Some(suggestion) = &ctx.suggestion {
                eprintln!("\nHint: {suggestion}");
            }
        }
        Error::Io(io_err) => match io_err.kind() {
            std::io::ErrorKind::NotFound => {
                eprintln!(
                    "File Not Found\n{io_err}\n\nHint: {}",
                    constants::ERR_FILE_NOT_FOUND
                );
            }
            std::io::ErrorKind::PermissionDenied => {
                eprintln!(
                    "Permission Denied\n{io_err}\n\nHint: {}",
                    constants::ERR_PERMISSION
                );
            }
            _ => eprintln!("File System Error\n{io_err}"),
        },
        Error::Network(req_err) => {
            if req_err.is_connect() {
                eprintln!(
                    "Connection Error\n{req_err}\n\nHint: {}",
                    constants::ERR_CONNECTION
                );
                return;
            }
            if req_err.is_timeout() {
                eprintln!(
                    "Timeout Error\n{req_err}\n\nHint: {}",
                    constants::ERR_TIMEOUT
                );
                return;
            }
            if !req_err.is_status() {
                eprintln!("Network Error\n{req_err}");
                return;
            }
            let Some(status) = req_err.status() else {
                eprintln!("Network Error\n{req_err}");
                return;
            };
            match status.as_u16() {
                401 => eprintln!(
                    "Authentication Error\n{req_err}\n\nHint: {}",
                    constants::ERR_API_CREDENTIALS
                ),
                403 => eprintln!(
                    "Permission Error\n{req_err}\n\nHint: {}",
                    constants::ERR_PERMISSION_DENIED
                ),
                404 => eprintln!(
                    "Not Found Error\n{req_err}\n\nHint: {}",
                    constants::ERR_ENDPOINT_NOT_FOUND
                ),
                429 => eprintln!(
                    "Rate Limited\n{req_err}\n\nHint: {}",
                    constants::ERR_RATE_LIMITED
                ),
                500..=599 => eprintln!(
                    "Server Error\n{req_err}\n\nHint: {}",
                    constants::ERR_SERVER_ERROR
                ),
                _ => eprintln!("HTTP Error\n{req_err}"),
            }
        }
        Error::Yaml(yaml_err) => {
            eprintln!(
                "YAML Parsing Error\n{yaml_err}\n\nHint: {}",
                constants::ERR_YAML_SYNTAX
            );
        }
        Error::Json(json_err) => {
            eprintln!(
                "JSON Parsing Error\n{json_err}\n\nHint: {}",
                constants::ERR_JSON_SYNTAX
            );
        }
        Error::Toml(toml_err) => {
            eprintln!(
                "TOML Parsing Error\n{toml_err}\n\nHint: {}",
                constants::ERR_TOML_SYNTAX
            );
        }
        Error::Anyhow(anyhow_err) => {
            eprintln!("Error\n{anyhow_err}");
        }
    }
}
