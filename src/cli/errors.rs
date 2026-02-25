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
    // ast-grep-ignore: no-println
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
            // ast-grep-ignore: no-println
            eprintln!("{kind}: {message}");
            let Some(ctx) = context else { return };
            if let Some(suggestion) = &ctx.suggestion {
                // ast-grep-ignore: no-println
                eprintln!("\nHint: {suggestion}");
            }
        }
        Error::Io(io_err) => match io_err.kind() {
            std::io::ErrorKind::NotFound => {
                // ast-grep-ignore: no-println
                eprintln!(
                    "File Not Found\n{io_err}\n\nHint: {}",
                    constants::ERR_FILE_NOT_FOUND
                );
            }
            std::io::ErrorKind::PermissionDenied => {
                // ast-grep-ignore: no-println
                eprintln!(
                    "Permission Denied\n{io_err}\n\nHint: {}",
                    constants::ERR_PERMISSION
                );
            }
            // ast-grep-ignore: no-println
            _ => eprintln!("File System Error\n{io_err}"),
        },
        Error::Network(req_err) => {
            if req_err.is_connect() {
                // ast-grep-ignore: no-println
                eprintln!(
                    "Connection Error\n{req_err}\n\nHint: {}",
                    constants::ERR_CONNECTION
                );
                return;
            }
            if req_err.is_timeout() {
                // ast-grep-ignore: no-println
                eprintln!(
                    "Timeout Error\n{req_err}\n\nHint: {}",
                    constants::ERR_TIMEOUT
                );
                return;
            }
            if !req_err.is_status() {
                // ast-grep-ignore: no-println
                eprintln!("Network Error\n{req_err}");
                return;
            }
            let Some(status) = req_err.status() else {
                // ast-grep-ignore: no-println
                eprintln!("Network Error\n{req_err}");
                return;
            };
            match status.as_u16() {
                // ast-grep-ignore: no-println
                401 => eprintln!(
                    "Authentication Error\n{req_err}\n\nHint: {}",
                    constants::ERR_API_CREDENTIALS
                ),
                // ast-grep-ignore: no-println
                403 => eprintln!(
                    "Permission Error\n{req_err}\n\nHint: {}",
                    constants::ERR_PERMISSION_DENIED
                ),
                // ast-grep-ignore: no-println
                404 => eprintln!(
                    "Not Found Error\n{req_err}\n\nHint: {}",
                    constants::ERR_ENDPOINT_NOT_FOUND
                ),
                // ast-grep-ignore: no-println
                429 => eprintln!(
                    "Rate Limited\n{req_err}\n\nHint: {}",
                    constants::ERR_RATE_LIMITED
                ),
                // ast-grep-ignore: no-println
                500..=599 => eprintln!(
                    "Server Error\n{req_err}\n\nHint: {}",
                    constants::ERR_SERVER_ERROR
                ),
                // ast-grep-ignore: no-println
                _ => eprintln!("HTTP Error\n{req_err}"),
            }
        }
        Error::Yaml(yaml_err) => {
            // ast-grep-ignore: no-println
            eprintln!(
                "YAML Parsing Error\n{yaml_err}\n\nHint: {}",
                constants::ERR_YAML_SYNTAX
            );
        }
        Error::Json(json_err) => {
            // ast-grep-ignore: no-println
            eprintln!(
                "JSON Parsing Error\n{json_err}\n\nHint: {}",
                constants::ERR_JSON_SYNTAX
            );
        }
        Error::Toml(toml_err) => {
            // ast-grep-ignore: no-println
            eprintln!(
                "TOML Parsing Error\n{toml_err}\n\nHint: {}",
                constants::ERR_TOML_SYNTAX
            );
        }
        Error::Anyhow(anyhow_err) => {
            // ast-grep-ignore: no-println
            eprintln!("Error\n{anyhow_err}");
        }
    }
}
