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

    // Specific error variants for better error handling
    #[error("API specification '{name}' not found")]
    SpecNotFound { name: String },
    #[error("API specification '{name}' already exists. Use --force to overwrite")]
    SpecAlreadyExists { name: String },
    #[error("No cached spec found for '{name}'. Run 'aperture config add {name}' first")]
    CachedSpecNotFound { name: String },
    #[error("Failed to deserialize cached spec '{name}': {reason}. The cache may be corrupted")]
    CachedSpecCorrupted { name: String, reason: String },
    #[error("Cache format version mismatch for '{name}': found v{found}, expected v{expected}")]
    CacheVersionMismatch {
        name: String,
        found: u32,
        expected: u32,
    },
    #[error(
        "Environment variable '{env_var}' required for authentication '{scheme_name}' is not set"
    )]
    SecretNotSet {
        scheme_name: String,
        env_var: String,
    },
    #[error("Invalid header format '{header}'. Expected 'Name: Value'")]
    InvalidHeaderFormat { header: String },
    #[error("Invalid header name '{name}': {reason}")]
    InvalidHeaderName { name: String, reason: String },
    #[error("Invalid header value for '{name}': {reason}")]
    InvalidHeaderValue { name: String, reason: String },
    #[error("EDITOR environment variable not set")]
    EditorNotSet,
    #[error("Editor command failed for spec '{name}'")]
    EditorFailed { name: String },
    #[error("Invalid HTTP method: {method}")]
    InvalidHttpMethod { method: String },
    #[error("Missing path parameter: {name}")]
    MissingPathParameter { name: String },
    #[error("Unsupported HTTP authentication scheme: {scheme}")]
    UnsupportedAuthScheme { scheme: String },
    #[error("Unsupported security scheme type: {scheme_type}")]
    UnsupportedSecurityScheme { scheme_type: String },
    #[error("Failed to serialize cached spec: {reason}")]
    SerializationError { reason: String },
    #[error("Invalid config.toml: {reason}")]
    InvalidConfig { reason: String },
    #[error("Could not determine home directory")]
    HomeDirectoryNotFound,
    #[error("Invalid JSON body: {reason}")]
    InvalidJsonBody { reason: String },
    #[error("Request failed: {reason}")]
    RequestFailed { reason: String },
    #[error("Failed to read response: {reason}")]
    ResponseReadError { reason: String },
    #[error("Request failed with status {status}: {body}")]
    HttpErrorWithContext {
        status: u16,
        body: String,
        api_name: String,
        operation_id: Option<String>,
        security_schemes: Vec<String>,
    },
    #[error("Invalid command for API '{context}': {reason}")]
    InvalidCommand { context: String, reason: String },
    #[error("Could not find operation from command path")]
    OperationNotFound,
    #[error("Invalid idempotency key")]
    InvalidIdempotencyKey,
    #[error("Header name cannot be empty")]
    EmptyHeaderName,
    #[error("JQ filter error: {reason}")]
    JqFilterError { reason: String },
    #[error("Invalid path '{path}': {reason}")]
    InvalidPath { path: String, reason: String },

    // Interactive error handling enhancements
    #[error("Input too long: {provided} characters (max: {max}). {suggestion}")]
    InteractiveInputTooLong {
        provided: usize,
        max: usize,
        suggestion: String,
    },
    #[error("Input contains invalid characters: {invalid_chars}. {suggestion}")]
    InteractiveInvalidCharacters {
        invalid_chars: String,
        suggestion: String,
    },
    #[error("Interactive operation timed out after {timeout_secs} seconds. {suggestion}")]
    InteractiveTimeout {
        timeout_secs: u64,
        suggestion: String,
    },
    #[error("Maximum retry attempts ({max_attempts}) exceeded. Last error: {last_error}")]
    InteractiveRetriesExhausted {
        max_attempts: usize,
        last_error: String,
        suggestions: Vec<String>,
    },
    #[error("Environment variable name '{name}' is invalid: {reason}. {suggestion}")]
    InvalidEnvironmentVariableName {
        name: String,
        reason: String,
        suggestion: String,
    },

    // Network resilience error handling
    #[error("Request timed out after {attempts} retries (max timeout: {timeout_ms}ms)")]
    RequestTimeout {
        attempts: usize,
        timeout_ms: u64,
    },
    #[error("Retry limit exceeded: {attempts} attempts failed over {duration_ms}ms. Last error: {last_error}")]
    RetryLimitExceeded {
        attempts: usize,
        duration_ms: u64,
        last_error: String,
    },
    #[error("Transient network error - request can be retried: {reason}")]
    TransientNetworkError {
        reason: String,
        retryable: bool,
    },

    #[error(transparent)]
    Anyhow(#[from] anyhow::Error),
}

/// JSON representation of an error for structured output
#[derive(Debug, Serialize, Deserialize)]
pub struct JsonError {
    pub error_type: String,
    pub message: String,
    pub context: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub details: Option<serde_json::Value>,
}

impl Error {
    /// Add context to an error for better user messaging
    #[must_use]
    pub fn with_context(self, context: &str) -> Self {
        match self {
            Self::Network(e) => Self::Config(format!("{context}: {e}")),
            Self::Io(e) => Self::Config(format!("{context}: {e}")),
            Self::Config(msg) => Self::Config(format!("{context}: {msg}")),
            Self::Validation(msg) => Self::Validation(format!("{context}: {msg}")),
            _ => self,
        }
    }

    /// Add operation context to an error for better debugging
    #[must_use]
    pub fn with_operation_context(self, operation: &str, api: &str) -> Self {
        match self {
            Self::RequestFailed { reason } => Self::RequestFailed {
                reason: format!("Operation '{operation}' on API '{api}': {reason}"),
            },
            Self::ResponseReadError { reason } => Self::ResponseReadError {
                reason: format!("Operation '{operation}' on API '{api}': {reason}"),
            },
            Self::Network(e) => Self::Config(format!("Operation '{operation}' on API '{api}': {e}")),
            _ => self,
        }
    }

    /// Add suggestions to error messages for better user guidance
    #[must_use]
    pub fn with_suggestion(self, suggestion: &str) -> Self {
        match self {
            Self::Config(msg) => Self::Config(format!("{msg}. Suggestion: {suggestion}")),
            Self::Validation(msg) => Self::Validation(format!("{msg}. Suggestion: {suggestion}")),
            Self::InvalidConfig { reason } => Self::InvalidConfig {
                reason: format!("{reason}. Suggestion: {suggestion}"),
            },
            _ => self,
        }
    }

    /// Convert error to JSON representation for structured output
    #[must_use]
    #[allow(clippy::too_many_lines)]
    pub fn to_json(&self) -> JsonError {
        use serde_json::json;

        let (error_type, message, context, details) = match self {
            Self::Config(msg) => ("Configuration", msg.clone(), None, None),
            Self::Io(io_err) => {
                let context = match io_err.kind() {
                    std::io::ErrorKind::NotFound => {
                        Some("Check that the file path is correct and the file exists.")
                    }
                    std::io::ErrorKind::PermissionDenied => {
                        Some("Check file permissions or run with appropriate privileges.")
                    }
                    _ => None,
                };
                (
                    "FileSystem",
                    io_err.to_string(),
                    context.map(str::to_string),
                    None,
                )
            }
            Self::Network(req_err) => {
                let context = if req_err.is_connect() {
                    Some("Check that the API server is running and accessible.")
                } else if req_err.is_timeout() {
                    Some("The API server may be slow or unresponsive. Try again later.")
                } else if req_err.is_status() {
                    req_err.status().and_then(|status| match status.as_u16() {
                        401 => Some("Check your API credentials and authentication configuration."),
                        403 => Some(
                            "Your credentials may be valid but lack permission for this operation.",
                        ),
                        404 => Some("Check that the API endpoint and parameters are correct."),
                        429 => {
                            Some("You're making requests too quickly. Wait before trying again.")
                        }
                        500..=599 => {
                            Some("The API server is experiencing issues. Try again later.")
                        }
                        _ => None,
                    })
                } else {
                    None
                };
                ("Network", req_err.to_string(), context.map(str::to_string), None)
            }
            Self::Yaml(yaml_err) => (
                "YAMLParsing",
                yaml_err.to_string(),
                Some("Check that your OpenAPI specification is valid YAML syntax.".to_string()),
                None,
            ),
            Self::Json(json_err) => (
                "JSONParsing",
                json_err.to_string(),
                Some("Check that your request body or response contains valid JSON.".to_string()),
                None,
            ),
            Self::Validation(msg) => (
                "Validation",
                msg.clone(),
                Some(
                    "Check that your OpenAPI specification follows the required format."
                        .to_string(),
                ),
                None,
            ),
            Self::Toml(toml_err) => (
                "TOMLParsing",
                toml_err.to_string(),
                Some("Check that your configuration file is valid TOML syntax.".to_string()),
                None,
            ),
            Self::SpecNotFound { name } => (
                "SpecNotFound",
                format!("API specification '{name}' not found"),
                Some("Use 'aperture config list' to see available specifications.".to_string()),
                Some(json!({ "spec_name": name })),
            ),
            Self::SpecAlreadyExists { name } => (
                "SpecAlreadyExists",
                format!("API specification '{name}' already exists. Use --force to overwrite"),
                None,
                Some(json!({ "spec_name": name })),
            ),
            Self::CachedSpecNotFound { name } => (
                "CachedSpecNotFound",
                format!("No cached spec found for '{name}'. Run 'aperture config add {name}' first"),
                None,
                Some(json!({ "spec_name": name })),
            ),
            Self::CachedSpecCorrupted { name, reason } => (
                "CachedSpecCorrupted",
                format!("Failed to deserialize cached spec '{name}': {reason}. The cache may be corrupted"),
                Some("Try removing and re-adding the specification.".to_string()),
                Some(json!({ "spec_name": name, "corruption_reason": reason })),
            ),
            Self::CacheVersionMismatch { name, found, expected } => (
                "CacheVersionMismatch",
                format!("Cache format version mismatch for '{name}': found v{found}, expected v{expected}"),
                Some("Run 'aperture config reinit' to regenerate the cache.".to_string()),
                Some(json!({ "spec_name": name, "found_version": found, "expected_version": expected })),
            ),
            Self::SecretNotSet { scheme_name, env_var } => (
                "SecretNotSet",
                format!("Environment variable '{env_var}' required for authentication '{scheme_name}' is not set"),
                Some(format!("Set the environment variable: export {env_var}=<your-secret>")),
                Some(json!({ "scheme_name": scheme_name, "env_var": env_var })),
            ),
            Self::InvalidHeaderFormat { header } => (
                "InvalidHeaderFormat",
                format!("Invalid header format '{header}'. Expected 'Name: Value'"),
                None,
                Some(json!({ "header": header })),
            ),
            Self::InvalidHeaderName { name, reason } => (
                "InvalidHeaderName",
                format!("Invalid header name '{name}': {reason}"),
                None,
                Some(json!({ "name": name, "reason": reason })),
            ),
            Self::InvalidHeaderValue { name, reason } => (
                "InvalidHeaderValue",
                format!("Invalid header value for '{name}': {reason}"),
                None,
                Some(json!({ "name": name, "reason": reason })),
            ),
            Self::EditorNotSet => (
                "EditorNotSet",
                "EDITOR environment variable not set".to_string(),
                Some("Set your preferred editor: export EDITOR=vim".to_string()),
                None,
            ),
            Self::EditorFailed { name } => (
                "EditorFailed",
                format!("Editor command failed for spec '{name}'"),
                None,
                Some(json!({ "spec_name": name })),
            ),
            Self::InvalidHttpMethod { method } => (
                "InvalidHttpMethod",
                format!("Invalid HTTP method: {method}"),
                None,
                Some(json!({ "method": method })),
            ),
            Self::MissingPathParameter { name } => (
                "MissingPathParameter",
                format!("Missing path parameter: {name}"),
                None,
                Some(json!({ "parameter_name": name })),
            ),
            Self::UnsupportedAuthScheme { scheme } => (
                "UnsupportedAuthScheme",
                format!("Unsupported HTTP authentication scheme: {scheme}"),
                Some("Only 'bearer' and 'basic' schemes are supported.".to_string()),
                Some(json!({ "scheme": scheme })),
            ),
            Self::UnsupportedSecurityScheme { scheme_type } => (
                "UnsupportedSecurityScheme",
                format!("Unsupported security scheme type: {scheme_type}"),
                Some("Only 'apiKey' and 'http' security schemes are supported.".to_string()),
                Some(json!({ "scheme_type": scheme_type })),
            ),
            Self::SerializationError { reason } => (
                "SerializationError",
                format!("Failed to serialize cached spec: {reason}"),
                None,
                Some(json!({ "reason": reason })),
            ),
            Self::InvalidConfig { reason } => (
                "InvalidConfig",
                format!("Invalid config.toml: {reason}"),
                Some("Check the TOML syntax in your configuration file.".to_string()),
                Some(json!({ "reason": reason })),
            ),
            Self::HomeDirectoryNotFound => (
                "HomeDirectoryNotFound",
                "Could not determine home directory".to_string(),
                Some("Ensure HOME environment variable is set.".to_string()),
                None,
            ),
            Self::InvalidJsonBody { reason } => (
                "InvalidJsonBody",
                format!("Invalid JSON body: {reason}"),
                Some("Check your JSON syntax and ensure all quotes are properly escaped.".to_string()),
                Some(json!({ "reason": reason })),
            ),
            Self::RequestFailed { reason } => (
                "RequestFailed",
                format!("Request failed: {reason}"),
                None,
                Some(json!({ "reason": reason })),
            ),
            Self::ResponseReadError { reason } => (
                "ResponseReadError",
                format!("Failed to read response: {reason}"),
                None,
                Some(json!({ "reason": reason })),
            ),
            Self::HttpErrorWithContext { status, body, api_name, operation_id, security_schemes } => {
                let context_hint = match status {
                    401 => {
                        if security_schemes.is_empty() {
                            Some("Check your API credentials and authentication configuration.".to_string())
                        } else {
                            let env_vars: Vec<String> = security_schemes.iter()
                                .map(|scheme| format!("Check environment variable for '{scheme}' authentication"))
                                .collect();
                            Some(env_vars.join("; "))
                        }
                    },
                    403 => Some("Your credentials may be valid but lack permission for this operation.".to_string()),
                    404 => Some("Check that the API endpoint and parameters are correct.".to_string()),
                    429 => Some("You're making requests too quickly. Wait before trying again.".to_string()),
                    500..=599 => Some("The API server is experiencing issues. Try again later.".to_string()),
                    _ => None,
                };
                (
                    "HttpError",
                    format!("Request failed with status {status}: {body}"),
                    context_hint,
                    Some(json!({
                        "status": status,
                        "body": body,
                        "api_name": api_name,
                        "operation_id": operation_id,
                        "security_schemes": security_schemes
                    })),
                )
            },
            Self::InvalidCommand { context, reason } => (
                "InvalidCommand",
                format!("Invalid command for API '{context}': {reason}"),
                Some("Use --help to see available commands.".to_string()),
                Some(json!({ "context": context, "reason": reason })),
            ),
            Self::OperationNotFound => (
                "OperationNotFound",
                "Could not find operation from command path".to_string(),
                Some("Check that the command matches an available operation.".to_string()),
                None,
            ),
            Self::InvalidIdempotencyKey => (
                "InvalidIdempotencyKey",
                "Invalid idempotency key".to_string(),
                Some("Idempotency key must be a valid header value.".to_string()),
                None,
            ),
            Self::EmptyHeaderName => (
                "EmptyHeaderName",
                "Header name cannot be empty".to_string(),
                None,
                None,
            ),
            Self::JqFilterError { reason } => (
                "JqFilterError",
                format!("JQ filter error: {reason}"),
                Some("Check your JQ filter syntax. Common examples: '.name', '.[] | select(.active)'".to_string()),
                Some(json!({ "reason": reason })),
            ),
            Self::InvalidPath { path, reason } => (
                "InvalidPath",
                format!("Invalid path '{path}': {reason}"),
                Some("Check that the path is valid and properly formatted.".to_string()),
                Some(json!({ "path": path, "reason": reason })),
            ),
            Self::InteractiveInputTooLong { provided, max, suggestion } => (
                "InteractiveInputTooLong",
                format!("Input too long: {provided} characters (max: {max}). {suggestion}"),
                Some("Consider shortening your input or breaking it into multiple parts.".to_string()),
                Some(json!({ "provided_length": provided, "max_length": max, "suggestion": suggestion })),
            ),
            Self::InteractiveInvalidCharacters { invalid_chars, suggestion } => (
                "InteractiveInvalidCharacters",
                format!("Input contains invalid characters: {invalid_chars}. {suggestion}"),
                Some("Use only alphanumeric characters, underscores, and hyphens.".to_string()),
                Some(json!({ "invalid_characters": invalid_chars, "suggestion": suggestion })),
            ),
            Self::InteractiveTimeout { timeout_secs, suggestion } => (
                "InteractiveTimeout",
                format!("Interactive operation timed out after {timeout_secs} seconds. {suggestion}"),
                Some("Try again with a faster response or increase the timeout.".to_string()),
                Some(json!({ "timeout_seconds": timeout_secs, "suggestion": suggestion })),
            ),
            Self::InteractiveRetriesExhausted { max_attempts, last_error, suggestions } => (
                "InteractiveRetriesExhausted",
                format!("Maximum retry attempts ({max_attempts}) exceeded. Last error: {last_error}"),
                Some(suggestions.join("; ")),
                Some(json!({ "max_attempts": max_attempts, "last_error": last_error, "suggestions": suggestions })),
            ),
            Self::InvalidEnvironmentVariableName { name, reason, suggestion } => (
                "InvalidEnvironmentVariableName",
                format!("Environment variable name '{name}' is invalid: {reason}. {suggestion}"),
                Some("Use uppercase letters, numbers, and underscores only.".to_string()),
                Some(json!({ "variable_name": name, "reason": reason, "suggestion": suggestion })),
            ),
            Self::RequestTimeout { attempts, timeout_ms } => (
                "RequestTimeout",
                format!("Request timed out after {attempts} retries (max timeout: {timeout_ms}ms)"),
                Some("The server may be slow or unresponsive. Try again later or increase timeout.".to_string()),
                Some(json!({ "retry_attempts": attempts, "timeout_ms": timeout_ms })),
            ),
            Self::RetryLimitExceeded { attempts, duration_ms, last_error } => (
                "RetryLimitExceeded",
                format!("Retry limit exceeded: {attempts} attempts failed over {duration_ms}ms. Last error: {last_error}"),
                Some("The service may be experiencing issues. Check API status or try again later.".to_string()),
                Some(json!({ "retry_attempts": attempts, "duration_ms": duration_ms, "last_error": last_error })),
            ),
            Self::TransientNetworkError { reason, retryable } => (
                "TransientNetworkError",
                format!("Transient network error - request can be retried: {reason}"),
                if *retryable { Some("This error is retryable. The request will be automatically retried.".to_string()) }
                else { Some("This error is not retryable. Check your network connection and API configuration.".to_string()) },
                Some(json!({ "reason": reason, "retryable": retryable })),
            ),
            Self::Anyhow(err) => (
                "Unexpected",
                err.to_string(),
                Some(
                    "This may be a bug. Please report it with the command you were running."
                        .to_string(),
                ),
                None,
            ),
        };

        JsonError {
            error_type: error_type.to_string(),
            message,
            context,
            details,
        }
    }
}
