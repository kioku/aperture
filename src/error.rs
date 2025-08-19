use crate::constants;
use serde::{Deserialize, Serialize};
use std::borrow::Cow;
use thiserror::Error;

/// Helper macro for creating error details with a single string field
macro_rules! error_details_single {
    ($field:literal, $value:expr) => {
        serde_json::json!({ $field: $value })
    };
}

/// Helper macro for creating error details with name and reason fields
macro_rules! error_details_name_reason {
    ($name_field:literal, $name:expr, $reason:expr) => {
        serde_json::json!({ $name_field: $name, "reason": $reason })
    };
}

/// Helper macro for creating error context with details and suggestion
macro_rules! error_context {
    ($details:expr, $suggestion:expr) => {
        Some(ErrorContext::new(
            Some($details),
            Some(Cow::Owned($suggestion.into())),
        ))
    };
    (details: $details:expr) => {
        Some(ErrorContext::with_details($details))
    };
    (suggestion: $suggestion:expr) => {
        Some(ErrorContext::with_suggestion(Cow::Owned(
            $suggestion.into(),
        )))
    };
}

/// Helper macro for creating an Internal error with `ErrorKind` and standard patterns
macro_rules! internal_error {
    ($kind:expr, $message:expr) => {
        Self::Internal {
            kind: $kind,
            message: Cow::Owned($message),
            context: None,
        }
    };
    ($kind:expr, $message:expr, $context:expr) => {
        Self::Internal {
            kind: $kind,
            message: Cow::Owned($message),
            context: $context,
        }
    };
}

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
            Self::HttpRequest => "HttpError",
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
        let name = name.into();
        internal_error!(
            ErrorKind::Specification,
            format!("API specification '{name}' not found"),
            error_context!(
                error_details_single!("spec_name", &name),
                constants::MSG_USE_CONFIG_LIST
            )
        )
    }

    /// Create a specification already exists error
    pub fn spec_already_exists(name: impl Into<String>) -> Self {
        let name = name.into();
        internal_error!(
            ErrorKind::Specification,
            format!("API specification '{name}' already exists. Use --force to overwrite"),
            error_context!(details: error_details_single!("spec_name", &name))
        )
    }

    /// Create a cached spec not found error
    pub fn cached_spec_not_found(name: impl Into<String>) -> Self {
        let name = name.into();
        internal_error!(
            ErrorKind::Specification,
            format!("No cached spec found for '{name}'. Run 'aperture config add {name}' first"),
            error_context!(details: error_details_single!("spec_name", &name))
        )
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

    /// Create a generic validation error
    pub fn validation_error(message: impl Into<String>) -> Self {
        let message = message.into();
        Self::Internal {
            kind: ErrorKind::Validation,
            message: Cow::Owned(format!("Validation error: {message}")),
            context: None,
        }
    }

    /// Create an invalid configuration error
    pub fn invalid_config(reason: impl Into<String>) -> Self {
        let reason = reason.into();
        internal_error!(
            ErrorKind::Validation,
            format!("Invalid configuration: {reason}"),
            error_context!(
                error_details_single!("reason", &reason),
                "Check the configuration file syntax and structure."
            )
        )
    }

    /// Create an invalid JSON body error
    pub fn invalid_json_body(reason: impl Into<String>) -> Self {
        use serde_json::json;
        let reason = reason.into();
        Self::Internal {
            kind: ErrorKind::Validation,
            message: Cow::Owned(format!("Invalid JSON body: {reason}")),
            context: Some(ErrorContext::new(
                Some(json!({ "reason": reason })),
                Some(Cow::Borrowed(
                    "Check that the JSON body is properly formatted.",
                )),
            )),
        }
    }

    /// Create an invalid path error
    pub fn invalid_path(path: impl Into<String>, reason: impl Into<String>) -> Self {
        use serde_json::json;
        let path = path.into();
        let reason = reason.into();
        Self::Internal {
            kind: ErrorKind::Validation,
            message: Cow::Owned(format!("Invalid path '{path}': {reason}")),
            context: Some(ErrorContext::new(
                Some(json!({ "path": path, "reason": reason })),
                Some(Cow::Borrowed("Check the path format and ensure it exists.")),
            )),
        }
    }

    /// Create a request failed error
    pub fn request_failed(status: reqwest::StatusCode, reason: impl Into<String>) -> Self {
        use serde_json::json;
        let reason = reason.into();
        Self::Internal {
            kind: ErrorKind::HttpRequest,
            message: Cow::Owned(format!("Request failed with status {status}: {reason}")),
            context: Some(ErrorContext::new(
                Some(json!({ "status_code": status.as_u16(), "reason": reason })),
                Some(Cow::Borrowed(
                    "Check the API endpoint, parameters, and authentication.",
                )),
            )),
        }
    }

    /// Create a response read error
    pub fn response_read_error(reason: impl Into<String>) -> Self {
        use serde_json::json;
        let reason = reason.into();
        Self::Internal {
            kind: ErrorKind::HttpRequest,
            message: Cow::Owned(format!("Failed to read response: {reason}")),
            context: Some(ErrorContext::new(
                Some(json!({ "reason": reason })),
                Some(Cow::Borrowed(
                    "Check network connectivity and server status.",
                )),
            )),
        }
    }

    /// Create an invalid HTTP method error
    pub fn invalid_http_method(method: impl Into<String>) -> Self {
        use serde_json::json;
        let method = method.into();
        Self::Internal {
            kind: ErrorKind::HttpRequest,
            message: Cow::Owned(format!("Invalid HTTP method: {method}")),
            context: Some(ErrorContext::new(
                Some(json!({ "method": method })),
                Some(Cow::Borrowed(
                    "Valid HTTP methods are: GET, POST, PUT, DELETE, PATCH, HEAD, OPTIONS.",
                )),
            )),
        }
    }

    // ---- Header Errors ----

    /// Create an invalid header name error
    pub fn invalid_header_name(name: impl Into<String>, reason: impl Into<String>) -> Self {
        use serde_json::json;
        let name = name.into();
        let reason = reason.into();
        Self::Internal {
            kind: ErrorKind::Headers,
            message: Cow::Owned(format!("Invalid header name '{name}': {reason}")),
            context: Some(ErrorContext::new(
                Some(json!({ "header_name": name, "reason": reason })),
                Some(Cow::Borrowed(
                    "Header names must contain only valid HTTP header characters.",
                )),
            )),
        }
    }

    /// Create an invalid header value error
    pub fn invalid_header_value(name: impl Into<String>, reason: impl Into<String>) -> Self {
        use serde_json::json;
        let name = name.into();
        let reason = reason.into();
        Self::Internal {
            kind: ErrorKind::Headers,
            message: Cow::Owned(format!("Invalid header value for '{name}': {reason}")),
            context: Some(ErrorContext::new(
                Some(json!({ "header_name": name, "reason": reason })),
                Some(Cow::Borrowed(
                    "Header values must contain only valid HTTP header characters.",
                )),
            )),
        }
    }

    /// Create an invalid header format error
    pub fn invalid_header_format(header: impl Into<String>) -> Self {
        use serde_json::json;
        let header = header.into();
        Self::Internal {
            kind: ErrorKind::Headers,
            message: Cow::Owned(format!(
                "Invalid header format '{header}'. Expected 'Name: Value'"
            )),
            context: Some(ErrorContext::new(
                Some(json!({ "header": header })),
                Some(Cow::Borrowed("Headers must be in 'Name: Value' format.")),
            )),
        }
    }

    /// Create an empty header name error
    #[must_use]
    pub const fn empty_header_name() -> Self {
        Self::Internal {
            kind: ErrorKind::Headers,
            message: Cow::Borrowed("Header name cannot be empty"),
            context: Some(ErrorContext::with_suggestion(Cow::Borrowed(
                "Provide a valid header name before the colon.",
            ))),
        }
    }

    // ---- Interactive Errors ----

    /// Create an interactive input too long error
    #[must_use]
    pub fn interactive_input_too_long(max_length: usize) -> Self {
        internal_error!(
            ErrorKind::Interactive,
            format!("Input too long (maximum {max_length} characters)"),
            error_context!(
                error_details_single!("max_length", max_length),
                "Please provide a shorter input."
            )
        )
    }

    /// Create an interactive invalid characters error
    pub fn interactive_invalid_characters(
        invalid_chars: impl Into<String>,
        suggestion: impl Into<String>,
    ) -> Self {
        use serde_json::json;
        let invalid_chars = invalid_chars.into();
        Self::Internal {
            kind: ErrorKind::Interactive,
            message: Cow::Owned(format!("Invalid characters found: {invalid_chars}")),
            context: Some(ErrorContext::new(
                Some(json!({ "invalid_characters": invalid_chars })),
                Some(Cow::Owned(suggestion.into())),
            )),
        }
    }

    /// Create an interactive timeout error
    #[must_use]
    pub fn interactive_timeout() -> Self {
        Self::Internal {
            kind: ErrorKind::Interactive,
            message: Cow::Borrowed("Input timeout - no response received"),
            context: error_context!(suggestion: "Please respond within the timeout period."),
        }
    }

    /// Create an interactive retries exhausted error
    pub fn interactive_retries_exhausted(
        max_retries: usize,
        last_error: impl Into<String>,
        suggestions: &[String],
    ) -> Self {
        use serde_json::json;
        let last_error = last_error.into();
        Self::Internal {
            kind: ErrorKind::Interactive,
            message: Cow::Owned(format!(
                "Maximum retry attempts ({max_retries}) exceeded: {last_error}"
            )),
            context: Some(ErrorContext::new(
                Some(
                    json!({ "max_attempts": max_retries, "last_error": last_error, "suggestions": suggestions }),
                ),
                Some(Cow::Owned(format!(
                    "Suggestions: {}",
                    suggestions.join("; ")
                ))),
            )),
        }
    }

    // ---- Server Variable Errors ----

    /// Create a missing server variable error
    pub fn missing_server_variable(name: impl Into<String>) -> Self {
        let name = name.into();
        internal_error!(
            ErrorKind::ServerVariable,
            format!("Required server variable '{name}' is not provided"),
            error_context!(
                error_details_single!("variable_name", &name),
                format!("Provide the variable with --server-var {name}=<value>")
            )
        )
    }

    /// Create an unknown server variable error
    pub fn unknown_server_variable(name: impl Into<String>, available: &[String]) -> Self {
        use serde_json::json;
        let name = name.into();
        let available_list = available.join(", ");
        Self::Internal {
            kind: ErrorKind::ServerVariable,
            message: Cow::Owned(format!(
                "Unknown server variable '{name}'. Available variables: {available_list}"
            )),
            context: Some(ErrorContext::new(
                Some(json!({ "variable_name": name, "available_variables": available })),
                Some(Cow::Owned(format!("Use one of: {available_list}"))),
            )),
        }
    }

    /// Create an unresolved template variable error
    pub fn unresolved_template_variable(name: impl Into<String>, url: impl Into<String>) -> Self {
        use serde_json::json;
        let name = name.into();
        let url = url.into();
        Self::Internal {
            kind: ErrorKind::ServerVariable,
            message: Cow::Owned(format!(
                "Unresolved template variable '{name}' in URL '{url}'"
            )),
            context: Some(ErrorContext::new(
                Some(json!({ "variable_name": name, "template_url": url })),
                Some(Cow::Borrowed(
                    "Ensure all template variables are provided with --server-var",
                )),
            )),
        }
    }

    /// Create an invalid environment variable name error with suggestion
    pub fn invalid_environment_variable_name(
        name: impl Into<String>,
        reason: impl Into<String>,
        suggestion: impl Into<String>,
    ) -> Self {
        let name = name.into();
        let reason = reason.into();
        internal_error!(
            ErrorKind::Interactive,
            format!("Invalid environment variable name '{name}': {reason}"),
            error_context!(
                error_details_name_reason!("variable_name", &name, &reason),
                suggestion
            )
        )
    }

    /// Create an invalid server variable format error
    pub fn invalid_server_var_format(arg: impl Into<String>, reason: impl Into<String>) -> Self {
        use serde_json::json;
        let arg = arg.into();
        let reason = reason.into();
        Self::Internal {
            kind: ErrorKind::ServerVariable,
            message: Cow::Owned(format!(
                "Invalid server variable format in '{arg}': {reason}"
            )),
            context: Some(ErrorContext::new(
                Some(json!({ "argument": arg, "reason": reason })),
                Some(Cow::Borrowed(
                    "Server variables must be in 'key=value' format.",
                )),
            )),
        }
    }

    /// Create an invalid server variable value error
    pub fn invalid_server_var_value(
        name: impl Into<String>,
        value: impl Into<String>,
        allowed_values: &[String],
    ) -> Self {
        use serde_json::json;
        let name = name.into();
        let value = value.into();
        Self::Internal {
            kind: ErrorKind::ServerVariable,
            message: Cow::Owned(format!(
                "Invalid value '{value}' for server variable '{name}'"
            )),
            context: Some(ErrorContext::new(
                Some(
                    json!({ "variable_name": name, "provided_value": value, "allowed_values": allowed_values }),
                ),
                Some(Cow::Owned(format!(
                    "Allowed values: {}",
                    allowed_values.join(", ")
                ))),
            )),
        }
    }

    // ---- Runtime Errors ----

    /// Create an operation not found error
    pub fn operation_not_found(operation: impl Into<String>) -> Self {
        use serde_json::json;
        let operation = operation.into();
        Self::Internal {
            kind: ErrorKind::Runtime,
            message: Cow::Owned(format!("Operation '{operation}' not found")),
            context: Some(ErrorContext::new(
                Some(json!({ "operation": operation })),
                Some(Cow::Borrowed(
                    "Check available operations with --help or --describe-json",
                )),
            )),
        }
    }

    /// Create a network request failed error
    pub fn network_request_failed(reason: impl Into<String>) -> Self {
        let reason = reason.into();
        internal_error!(
            ErrorKind::HttpRequest,
            format!("Network request failed: {reason}"),
            error_context!(
                error_details_single!("reason", &reason),
                "Check network connectivity and URL validity"
            )
        )
    }

    /// Create a serialization error
    pub fn serialization_error(reason: impl Into<String>) -> Self {
        let reason = reason.into();
        internal_error!(
            ErrorKind::Validation,
            format!("Serialization failed: {reason}"),
            error_context!(
                error_details_single!("reason", &reason),
                "Check data structure validity"
            )
        )
    }

    /// Create a home directory not found error
    #[must_use]
    pub fn home_directory_not_found() -> Self {
        Self::Internal {
            kind: ErrorKind::Runtime,
            message: Cow::Borrowed("Home directory not found"),
            context: Some(ErrorContext::new(
                Some(serde_json::json!({})),
                Some(Cow::Borrowed("Ensure HOME environment variable is set")),
            )),
        }
    }

    /// Create an invalid command error
    pub fn invalid_command(context: impl Into<String>, reason: impl Into<String>) -> Self {
        let context = context.into();
        let reason = reason.into();
        internal_error!(
            ErrorKind::Validation,
            format!("Invalid command for '{context}': {reason}"),
            error_context!(
                error_details_name_reason!("context", &context, &reason),
                "Check available commands with --help or --describe-json"
            )
        )
    }

    /// Create an HTTP error with context
    pub fn http_error_with_context(
        status: u16,
        body: impl Into<String>,
        api_name: impl Into<String>,
        operation_id: Option<impl Into<String>>,
        security_schemes: &[String],
    ) -> Self {
        use serde_json::json;
        let body = body.into();
        let api_name = api_name.into();
        let operation_id = operation_id.map(std::convert::Into::into);

        // Include important parts of response body in message for backward compatibility
        let message = if body.len() <= 200 && !body.is_empty() {
            format!("HTTP {status} error for '{api_name}': {body}")
        } else {
            format!("HTTP {status} error for '{api_name}'")
        };

        Self::Internal {
            kind: ErrorKind::HttpRequest,
            message: Cow::Owned(message),
            context: Some(ErrorContext::new(
                Some(json!({
                    "status": status,
                    "response_body": body,
                    "api_name": api_name,
                    "operation_id": operation_id,
                    "security_schemes": security_schemes
                })),
                Some(Cow::Borrowed(
                    "Check the API endpoint, parameters, and authentication.",
                )),
            )),
        }
    }

    /// Create a JQ filter error
    pub fn jq_filter_error(filter: impl Into<String>, reason: impl Into<String>) -> Self {
        let filter = filter.into();
        let reason = reason.into();
        internal_error!(
            ErrorKind::Validation,
            format!("JQ filter error in '{filter}': {reason}"),
            error_context!(
                error_details_name_reason!("filter", &filter, &reason),
                "Check JQ filter syntax and data structure compatibility"
            )
        )
    }

    /// Create a transient network error
    pub fn transient_network_error(reason: impl Into<String>, retryable: bool) -> Self {
        let reason = reason.into();
        internal_error!(
            ErrorKind::HttpRequest,
            format!("Transient network error: {reason}"),
            error_context!(
                serde_json::json!({
                    "reason": reason,
                    "retryable": retryable
                }),
                if retryable {
                    "This error may be temporary and could succeed on retry"
                } else {
                    "This error is not retryable"
                }
            )
        )
    }

    /// Create a retry limit exceeded error
    pub fn retry_limit_exceeded(max_attempts: u32, last_error: impl Into<String>) -> Self {
        let last_error = last_error.into();
        internal_error!(
            ErrorKind::HttpRequest,
            format!("Retry limit exceeded after {max_attempts} attempts: {last_error}"),
            error_context!(
                serde_json::json!({
                    "max_attempts": max_attempts,
                    "last_error": last_error
                }),
                "Consider checking network connectivity or increasing retry limits"
            )
        )
    }

    /// Create a request timeout error
    #[must_use]
    pub fn request_timeout(timeout_seconds: u64) -> Self {
        internal_error!(
            ErrorKind::HttpRequest,
            format!("Request timed out after {timeout_seconds} seconds"),
            error_context!(
                serde_json::json!({
                    "timeout_seconds": timeout_seconds
                }),
                "Consider increasing the timeout or checking network connectivity"
            )
        )
    }

    /// Create a missing path parameter error
    pub fn missing_path_parameter(name: impl Into<String>) -> Self {
        let name = name.into();
        internal_error!(
            ErrorKind::Validation,
            format!("Missing required path parameter: {name}"),
            error_context!(
                error_details_single!("parameter_name", &name),
                "Provide a value for this required path parameter"
            )
        )
    }
}
