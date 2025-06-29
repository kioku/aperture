use serde::{Deserialize, Serialize};
use thiserror::Error;

#[derive(Error, Debug)]
pub enum Error {
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),
    #[error("Network error: {0}")]
    Network(#[from] reqwest::Error),
    #[error("YAML parsing error: {0}")]
    Yaml(#[from] serde_yaml::Error),
    #[error("JSON parsing error: {0}")]
    Json(#[from] serde_json::Error),
    #[error("TOML parsing error: {0}")]
    Toml(#[from] toml::de::Error),
    #[error("Configuration error: {0}")]
    Config(String),
    #[error("Validation error: {0}")]
    Validation(String),
    #[error(transparent)]
    Anyhow(#[from] anyhow::Error),
}

/// JSON representation of an error for structured output
#[derive(Debug, Serialize, Deserialize)]
pub struct JsonError {
    pub error_type: String,
    pub message: String,
    pub context: Option<String>,
}

impl Error {
    /// Add context to an error for better user messaging
    #[must_use]
    pub fn with_context(self, context: &str) -> Self {
        match self {
            Self::Network(e) => Self::Config(format!("{context}: {e}")),
            Self::Io(e) => Self::Config(format!("{context}: {e}")),
            _ => self,
        }
    }

    /// Convert error to JSON representation for structured output
    #[must_use]
    pub fn to_json(&self) -> JsonError {
        let (error_type, message, context) = match self {
            Self::Config(msg) => ("Configuration", msg.clone(), None),
            Self::Io(io_err) => {
                let context = match io_err.kind() {
                    std::io::ErrorKind::NotFound => Some("Check that the file path is correct and the file exists."),
                    std::io::ErrorKind::PermissionDenied => Some("Check file permissions or run with appropriate privileges."),
                    _ => None,
                };
                ("FileSystem", io_err.to_string(), context.map(str::to_string))
            }
            Self::Network(req_err) => {
                let context = if req_err.is_connect() {
                    Some("Check that the API server is running and accessible.")
                } else if req_err.is_timeout() {
                    Some("The API server may be slow or unresponsive. Try again later.")
                } else if req_err.is_status() {
                    req_err.status().and_then(|status| match status.as_u16() {
                        401 => Some("Check your API credentials and authentication configuration."),
                        403 => Some("Your credentials may be valid but lack permission for this operation."),
                        404 => Some("Check that the API endpoint and parameters are correct."),
                        429 => Some("You're making requests too quickly. Wait before trying again."),
                        500..=599 => Some("The API server is experiencing issues. Try again later."),
                        _ => None,
                    })
                } else {
                    None
                };
                ("Network", req_err.to_string(), context.map(str::to_string))
            }
            Self::Yaml(yaml_err) => ("YAMLParsing", yaml_err.to_string(), Some("Check that your OpenAPI specification is valid YAML syntax.".to_string())),
            Self::Json(json_err) => ("JSONParsing", json_err.to_string(), Some("Check that your request body or response contains valid JSON.".to_string())),
            Self::Validation(msg) => ("Validation", msg.clone(), Some("Check that your OpenAPI specification follows the required format.".to_string())),
            Self::Toml(toml_err) => ("TOMLParsing", toml_err.to_string(), Some("Check that your configuration file is valid TOML syntax.".to_string())),
            Self::Anyhow(err) => ("Unexpected", err.to_string(), Some("This may be a bug. Please report it with the command you were running.".to_string())),
        };

        JsonError {
            error_type: error_type.to_string(),
            message,
            context,
        }
    }
}
