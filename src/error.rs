use crate::constants;
use serde::{Deserialize, Serialize};
use std::borrow::Cow;
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
    RequestTimeout { attempts: usize, timeout_ms: u64 },
    #[error("Retry limit exceeded: {attempts} attempts failed over {duration_ms}ms. Last error: {last_error}")]
    RetryLimitExceeded {
        attempts: usize,
        duration_ms: u64,
        last_error: String,
    },
    #[error("Transient network error - request can be retried: {reason}")]
    TransientNetworkError { reason: String, retryable: bool },

    // Server variable resolution errors
    #[error("Missing required server variable '{name}' with no default value")]
    MissingServerVariable { name: String },
    #[error("Unknown server variable '{name}'. Available variables: {available:?}")]
    UnknownServerVariable {
        name: String,
        available: Vec<String>,
    },
    #[error("Invalid server variable format '{arg}': {reason}")]
    InvalidServerVarFormat { arg: String, reason: String },
    #[error(
        "Invalid value '{value}' for server variable '{name}'. Allowed values: {allowed_values:?}"
    )]
    InvalidServerVarValue {
        name: String,
        value: String,
        allowed_values: Vec<String>,
    },
    #[error("Unresolved template variable '{name}' in URL '{url}'")]
    UnresolvedTemplateVariable { name: String, url: String },

    // Consolidated error variant using new infrastructure
    #[error("{kind}: {message}")]
    Internal {
        kind: ErrorKind,
        message: Cow<'static, str>,
        context: Option<ErrorContext>,
    },

    #[error(transparent)]
    Anyhow(#[from] anyhow::Error),
}

/// Error categories for consolidated error handling
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ErrorKind {
    /// Specification-related errors (not found, already exists, cache issues)
    Specification,
    /// Authentication and authorization errors
    Authentication,
    /// Input validation and configuration errors
    Validation,
    /// HTTP request/response errors
    HttpRequest,
    /// Header processing errors
    Headers,
    /// Interactive input errors
    Interactive,
    /// Server variable resolution errors
    ServerVariable,
    /// Runtime operation errors
    Runtime,
}

/// Additional context for consolidated errors
#[derive(Debug, Clone)]
pub struct ErrorContext {
    /// Structured details for programmatic access
    pub details: Option<serde_json::Value>,
    /// Human-readable suggestion for resolving the error
    pub suggestion: Option<Cow<'static, str>>,
}

impl ErrorContext {
    /// Create a new error context with details and suggestion
    #[must_use]
    pub const fn new(
        details: Option<serde_json::Value>,
        suggestion: Option<Cow<'static, str>>,
    ) -> Self {
        Self {
            details,
            suggestion,
        }
    }

    /// Create error context with only details
    #[must_use]
    pub const fn with_details(details: serde_json::Value) -> Self {
        Self {
            details: Some(details),
            suggestion: None,
        }
    }

    /// Create error context with only suggestion
    #[must_use]
    pub const fn with_suggestion(suggestion: Cow<'static, str>) -> Self {
        Self {
            details: None,
            suggestion: Some(suggestion),
        }
    }
}

impl ErrorKind {
    /// Get the string identifier for this error kind
    #[must_use]
    pub const fn as_str(&self) -> &'static str {
        match self {
            Self::Specification => "Specification",
            Self::Authentication => "Authentication",
            Self::Validation => "Validation",
            Self::HttpRequest => "HttpRequest",
            Self::Headers => "Headers",
            Self::Interactive => "Interactive",
            Self::ServerVariable => "ServerVariable",
            Self::Runtime => "Runtime",
        }
    }
}

impl std::fmt::Display for ErrorKind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.as_str())
    }
}

/// JSON representation of an error for structured output
#[derive(Debug, Serialize, Deserialize)]
pub struct JsonError {
    pub error_type: Cow<'static, str>,
    pub message: String,
    pub context: Option<Cow<'static, str>>,
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
            Self::Network(e) => {
                Self::Config(format!("Operation '{operation}' on API '{api}': {e}"))
            }
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

        let (error_type, message, context, details): (&str, String, Option<Cow<'static, str>>, Option<serde_json::Value>) = match self {
            Self::Config(msg) => ("Configuration", msg.clone(), None, None),
            Self::Io(io_err) => {
                let context = match io_err.kind() {
                    std::io::ErrorKind::NotFound => {
                        Some(Cow::Borrowed(constants::ERR_FILE_NOT_FOUND))
                    }
                    std::io::ErrorKind::PermissionDenied => {
                        Some(Cow::Borrowed(constants::ERR_PERMISSION))
                    }
                    _ => None,
                };
                (
                    "FileSystem",
                    io_err.to_string(),
                    context,
                    None,
                )
            }
            Self::Network(req_err) => {
                let context = if req_err.is_connect() {
                    Some(Cow::Borrowed(constants::ERR_CONNECTION))
                } else if req_err.is_timeout() {
                    Some(Cow::Borrowed(constants::ERR_TIMEOUT))
                } else if req_err.is_status() {
                    req_err.status().and_then(|status| match status.as_u16() {
                        401 => Some(Cow::Borrowed(constants::ERR_API_CREDENTIALS)),
                        403 => Some(Cow::Borrowed(constants::ERR_PERMISSION_DENIED)),
                        404 => Some(Cow::Borrowed(constants::ERR_ENDPOINT_NOT_FOUND)),
                        429 => Some(Cow::Borrowed(constants::ERR_RATE_LIMITED)),
                        500..=599 => Some(Cow::Borrowed(constants::ERR_SERVER_ERROR)),
                        _ => None,
                    })
                } else {
                    None
                };
                ("Network", req_err.to_string(), context, None)
            }
            Self::Yaml(yaml_err) => (
                "YAMLParsing",
                yaml_err.to_string(),
                Some(Cow::Borrowed(constants::ERR_YAML_SYNTAX)),
                None,
            ),
            Self::Json(json_err) => (
                "JSONParsing",
                json_err.to_string(),
                Some(Cow::Borrowed(constants::ERR_JSON_SYNTAX)),
                None,
            ),
            Self::Validation(msg) => (
                "Validation",
                msg.clone(),
                Some(Cow::Borrowed(constants::ERR_OPENAPI_FORMAT)),
                None,
            ),
            Self::Toml(toml_err) => (
                "TOMLParsing",
                toml_err.to_string(),
                Some(Cow::Borrowed(constants::ERR_TOML_SYNTAX)),
                None,
            ),
            Self::SpecNotFound { name } => (
                "SpecNotFound",
                format!("API specification '{name}' not found"),
                Some(Cow::Borrowed(constants::MSG_USE_CONFIG_LIST)),
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
                Some(Cow::Borrowed("Try removing and re-adding the specification.")),
                Some(json!({ "spec_name": name, "corruption_reason": reason })),
            ),
            Self::CacheVersionMismatch { name, found, expected } => (
                "CacheVersionMismatch",
                format!("Cache format version mismatch for '{name}': found v{found}, expected v{expected}"),
                Some(Cow::Borrowed("Run 'aperture config reinit' to regenerate the cache.")),
                Some(json!({ "spec_name": name, "found_version": found, "expected_version": expected })),
            ),
            Self::SecretNotSet { scheme_name, env_var } => (
                "SecretNotSet",
                format!("Environment variable '{env_var}' required for authentication '{scheme_name}' is not set"),
                Some(Cow::Owned(format!("Set the environment variable: export {env_var}=<your-secret>"))),
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
                Some(Cow::Borrowed("Set your preferred editor: export EDITOR=vim")),
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
                Some(Cow::Borrowed("Only 'bearer' and 'basic' schemes are supported.")),
                Some(json!({ "scheme": scheme })),
            ),
            Self::UnsupportedSecurityScheme { scheme_type } => (
                "UnsupportedSecurityScheme",
                format!("Unsupported security scheme type: {scheme_type}"),
                Some(Cow::Borrowed("Only 'apiKey' and 'http' security schemes are supported.")),
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
                Some(Cow::Borrowed("Check the TOML syntax in your configuration file.")),
                Some(json!({ "reason": reason })),
            ),
            Self::HomeDirectoryNotFound => (
                "HomeDirectoryNotFound",
                "Could not determine home directory".to_string(),
                Some(Cow::Borrowed("Ensure HOME environment variable is set.")),
                None,
            ),
            Self::InvalidJsonBody { reason } => (
                "InvalidJsonBody",
                format!("Invalid JSON body: {reason}"),
                Some(Cow::Borrowed(constants::ERR_JSON_SYNTAX)),
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
                            Some(Cow::Borrowed(constants::ERR_API_CREDENTIALS))
                        } else {
                            let env_vars: Vec<String> = security_schemes.iter()
                                .map(|scheme| format!("Check environment variable for '{scheme}' authentication"))
                                .collect();
                            Some(Cow::Owned(env_vars.join("; ")))
                        }
                    },
                    403 => Some(Cow::Borrowed(constants::ERR_PERMISSION_DENIED)),
                    404 => Some(Cow::Borrowed(constants::ERR_ENDPOINT_NOT_FOUND)),
                    429 => Some(Cow::Borrowed(constants::ERR_RATE_LIMITED)),
                    500..=599 => Some(Cow::Borrowed(constants::ERR_SERVER_ERROR)),
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
                Some(Cow::Borrowed(constants::MSG_USE_HELP)),
                Some(json!({ "context": context, "reason": reason })),
            ),
            Self::OperationNotFound => (
                "OperationNotFound",
                "Could not find operation from command path".to_string(),
                Some(Cow::Borrowed("Check that the command matches an available operation.")),
                None,
            ),
            Self::InvalidIdempotencyKey => (
                "InvalidIdempotencyKey",
                "Invalid idempotency key".to_string(),
                Some(Cow::Borrowed("Idempotency key must be a valid header value.")),
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
                Some(Cow::Borrowed("Check your JQ filter syntax. Common examples: '.name', '.[] | select(.active)'")),
                Some(json!({ "reason": reason })),
            ),
            Self::InvalidPath { path, reason } => (
                "InvalidPath",
                format!("Invalid path '{path}': {reason}"),
                Some(Cow::Borrowed("Check that the path is valid and properly formatted.")),
                Some(json!({ "path": path, "reason": reason })),
            ),
            Self::InteractiveInputTooLong { provided, max, suggestion } => (
                "InteractiveInputTooLong",
                format!("Input too long: {provided} characters (max: {max}). {suggestion}"),
                Some(Cow::Borrowed("Consider shortening your input or breaking it into multiple parts.")),
                Some(json!({ "provided_length": provided, "max_length": max, "suggestion": suggestion })),
            ),
            Self::InteractiveInvalidCharacters { invalid_chars, suggestion } => (
                "InteractiveInvalidCharacters",
                format!("Input contains invalid characters: {invalid_chars}. {suggestion}"),
                Some(Cow::Borrowed("Use only alphanumeric characters, underscores, and hyphens.")),
                Some(json!({ "invalid_characters": invalid_chars, "suggestion": suggestion })),
            ),
            Self::InteractiveTimeout { timeout_secs, suggestion } => (
                "InteractiveTimeout",
                format!("Interactive operation timed out after {timeout_secs} seconds. {suggestion}"),
                Some(Cow::Borrowed("Try again with a faster response or increase the timeout.")),
                Some(json!({ "timeout_seconds": timeout_secs, "suggestion": suggestion })),
            ),
            Self::InteractiveRetriesExhausted { max_attempts, last_error, suggestions } => (
                "InteractiveRetriesExhausted",
                format!("Maximum retry attempts ({max_attempts}) exceeded. Last error: {last_error}"),
                Some(Cow::Owned(suggestions.join("; "))),
                Some(json!({ "max_attempts": max_attempts, "last_error": last_error, "suggestions": suggestions })),
            ),
            Self::InvalidEnvironmentVariableName { name, reason, suggestion } => (
                "InvalidEnvironmentVariableName",
                format!("Environment variable name '{name}' is invalid: {reason}. {suggestion}"),
                Some(Cow::Borrowed("Use uppercase letters, numbers, and underscores only.")),
                Some(json!({ "variable_name": name, "reason": reason, "suggestion": suggestion })),
            ),
            Self::RequestTimeout { attempts, timeout_ms } => (
                "RequestTimeout",
                format!("Request timed out after {attempts} retries (max timeout: {timeout_ms}ms)"),
                Some(Cow::Borrowed("The server may be slow or unresponsive. Try again later or increase timeout.")),
                Some(json!({ "retry_attempts": attempts, "timeout_ms": timeout_ms })),
            ),
            Self::RetryLimitExceeded { attempts, duration_ms, last_error } => (
                "RetryLimitExceeded",
                format!("Retry limit exceeded: {attempts} attempts failed over {duration_ms}ms. Last error: {last_error}"),
                Some(Cow::Borrowed("The service may be experiencing issues. Check API status or try again later.")),
                Some(json!({ "retry_attempts": attempts, "duration_ms": duration_ms, "last_error": last_error })),
            ),
            Self::TransientNetworkError { reason, retryable } => (
                "TransientNetworkError",
                format!("Transient network error - request can be retried: {reason}"),
                if *retryable { Some(Cow::Borrowed("This error is retryable. The request will be automatically retried.")) }
                else { Some(Cow::Borrowed("This error is not retryable. Check your network connection and API configuration.")) },
                Some(json!({ "reason": reason, "retryable": retryable })),
            ),
            Self::MissingServerVariable { name } => (
                "MissingServerVariable",
                format!("Missing required server variable '{name}' with no default value"),
                Some(Cow::Borrowed("Provide the missing server variable using --server-var name=value")),
                Some(json!({ "variable_name": name })),
            ),
            Self::UnknownServerVariable { name, available } => (
                "UnknownServerVariable",
                format!("Unknown server variable '{name}'. Available variables: {available:?}"),
                Some(Cow::Owned(format!("Use one of the available variables: {}", available.join(", ")))),
                Some(json!({ "variable_name": name, "available_variables": available })),
            ),
            Self::InvalidServerVarFormat { arg, reason } => (
                "InvalidServerVarFormat",
                format!("Invalid server variable format '{arg}': {reason}"),
                Some(Cow::Borrowed("Use the format --server-var key=value")),
                Some(json!({ "argument": arg, "reason": reason })),
            ),
            Self::InvalidServerVarValue { name, value, allowed_values } => (
                "InvalidServerVarValue",
                format!("Invalid value '{value}' for server variable '{name}'. Allowed values: {allowed_values:?}"),
                Some(Cow::Owned(format!("Use one of the allowed values: {}", allowed_values.join(", ")))),
                Some(json!({ "variable_name": name, "provided_value": value, "allowed_values": allowed_values })),
            ),
            Self::UnresolvedTemplateVariable { name, url } => (
                "UnresolvedTemplateVariable",
                format!("Unresolved template variable '{name}' in URL '{url}'"),
                Some(Cow::Borrowed("Ensure all template variables are provided with --server-var")),
                Some(json!({ "variable_name": name, "template_url": url })),
            ),
            Self::Internal { kind, message, context } => (
                kind.as_str(),
                message.to_string(),
                context.as_ref().and_then(|ctx| ctx.suggestion.clone()),
                context.as_ref().and_then(|ctx| ctx.details.clone()),
            ),
            Self::Anyhow(err) => (
                "Unexpected",
                err.to_string(),
                Some(Cow::Borrowed(
                    "This may be a bug. Please report it with the command you were running."
                )),
                None,
            ),
        };

        JsonError {
            error_type: Cow::Borrowed(error_type),
            message,
            context,
            details,
        }
    }
}

impl Error {
    /// Create a specification not found error
    pub fn spec_not_found(name: impl Into<String>) -> Self {
        use serde_json::json;
        let name = name.into();
        Self::Internal {
            kind: ErrorKind::Specification,
            message: Cow::Owned(format!("API specification '{name}' not found")),
            context: Some(ErrorContext::new(
                Some(json!({ "spec_name": name })),
                Some(Cow::Borrowed(constants::MSG_USE_CONFIG_LIST)),
            )),
        }
    }

    /// Create a specification already exists error
    pub fn spec_already_exists(name: impl Into<String>) -> Self {
        use serde_json::json;
        let name = name.into();
        Self::Internal {
            kind: ErrorKind::Specification,
            message: Cow::Owned(format!(
                "API specification '{name}' already exists. Use --force to overwrite"
            )),
            context: Some(ErrorContext::with_details(json!({ "spec_name": name }))),
        }
    }

    /// Create a cached spec not found error
    pub fn cached_spec_not_found(name: impl Into<String>) -> Self {
        use serde_json::json;
        let name = name.into();
        Self::Internal {
            kind: ErrorKind::Specification,
            message: Cow::Owned(format!(
                "No cached spec found for '{name}'. Run 'aperture config add {name}' first"
            )),
            context: Some(ErrorContext::with_details(json!({ "spec_name": name }))),
        }
    }

    /// Create a cached spec corrupted error
    pub fn cached_spec_corrupted(name: impl Into<String>, reason: impl Into<String>) -> Self {
        use serde_json::json;
        let name = name.into();
        let reason = reason.into();
        Self::Internal {
            kind: ErrorKind::Specification,
            message: Cow::Owned(format!(
                "Failed to deserialize cached spec '{name}': {reason}. The cache may be corrupted"
            )),
            context: Some(ErrorContext::new(
                Some(json!({ "spec_name": name, "corruption_reason": reason })),
                Some(Cow::Borrowed(
                    "Try removing and re-adding the specification.",
                )),
            )),
        }
    }

    /// Create a cache version mismatch error
    pub fn cache_version_mismatch(name: impl Into<String>, found: u32, expected: u32) -> Self {
        use serde_json::json;
        let name = name.into();
        Self::Internal {
            kind: ErrorKind::Specification,
            message: Cow::Owned(format!(
                "Cache format version mismatch for '{name}': found v{found}, expected v{expected}"
            )),
            context: Some(ErrorContext::new(
                Some(
                    json!({ "spec_name": name, "found_version": found, "expected_version": expected }),
                ),
                Some(Cow::Borrowed(
                    "Run 'aperture config reinit' to regenerate the cache.",
                )),
            )),
        }
    }

    /// Create a secret not set error
    pub fn secret_not_set(scheme_name: impl Into<String>, env_var: impl Into<String>) -> Self {
        use serde_json::json;
        let scheme_name = scheme_name.into();
        let env_var = env_var.into();
        Self::Internal {
            kind: ErrorKind::Authentication,
            message: Cow::Owned(format!(
                "Environment variable '{env_var}' required for authentication '{scheme_name}' is not set"
            )),
            context: Some(ErrorContext::new(
                Some(json!({ "scheme_name": scheme_name, "env_var": env_var })),
                Some(Cow::Owned(format!("Set the environment variable: export {env_var}=<your-secret>"))),
            )),
        }
    }

    /// Create an unsupported auth scheme error
    pub fn unsupported_auth_scheme(scheme: impl Into<String>) -> Self {
        use serde_json::json;
        let scheme = scheme.into();
        Self::Internal {
            kind: ErrorKind::Authentication,
            message: Cow::Owned(format!("Unsupported HTTP authentication scheme: {scheme}")),
            context: Some(ErrorContext::new(
                Some(json!({ "scheme": scheme })),
                Some(Cow::Borrowed(
                    "Only 'bearer' and 'basic' schemes are supported.",
                )),
            )),
        }
    }

    /// Create an unsupported security scheme error
    pub fn unsupported_security_scheme(scheme_type: impl Into<String>) -> Self {
        use serde_json::json;
        let scheme_type = scheme_type.into();
        Self::Internal {
            kind: ErrorKind::Authentication,
            message: Cow::Owned(format!("Unsupported security scheme type: {scheme_type}")),
            context: Some(ErrorContext::new(
                Some(json!({ "scheme_type": scheme_type })),
                Some(Cow::Borrowed(
                    "Only 'apiKey' and 'http' security schemes are supported.",
                )),
            )),
        }
    }
}
