//! Error handling module for Aperture CLI
//!
//! This module provides a consolidated error handling system that categorizes
//! all application errors into 9 distinct kinds. The design follows these principles:
//!
//! 1. **Error Consolidation**: All errors are mapped to one of 9 `ErrorKind` categories
//! 2. **Structured Context**: Each error can include structured JSON details and suggestions
//! 3. **Builder Pattern**: `ErrorContext` provides fluent builder methods for error construction
//! 4. **JSON Support**: All errors can be serialized to JSON for programmatic consumption

use crate::constants;
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::borrow::Cow;
use thiserror::Error;

#[derive(Error, Debug)]
pub enum Error {
    // Keep essential external errors that can't be consolidated
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
///
/// This enum represents the 8 primary error categories used throughout
/// the application. All internal errors are mapped to one of these categories
/// to provide consistent error handling and reporting.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ErrorKind {
    /// Specification-related errors (not found, already exists, cache issues)
    Specification,
    /// Authentication and authorization errors
    Authentication,
    /// Input validation and configuration errors
    Validation,
    /// Network connectivity and transport errors (connection, DNS, timeouts)
    Network,
    /// HTTP request/response errors (status codes, API errors)
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

    /// Builder method to add a single detail field
    #[must_use]
    pub fn with_detail(key: &str, value: impl serde::Serialize) -> Self {
        Self {
            details: Some(json!({ key: value })),
            suggestion: None,
        }
    }

    /// Builder method to add name and reason details
    #[must_use]
    pub fn with_name_reason(name_field: &str, name: &str, reason: &str) -> Self {
        Self {
            details: Some(json!({ name_field: name, "reason": reason })),
            suggestion: None,
        }
    }

    /// Add suggestion to existing context
    #[must_use]
    pub fn and_suggestion(mut self, suggestion: impl Into<String>) -> Self {
        self.suggestion = Some(Cow::Owned(suggestion.into()));
        self
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
            Self::Network => "Network",
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
            Self::Network(e) => Self::invalid_config(format!("{context}: {e}")),
            Self::Io(e) => Self::invalid_config(format!("{context}: {e}")),
            Self::Internal {
                kind,
                message,
                context: ctx,
            } => Self::Internal {
                kind,
                message: Cow::Owned(format!("{context}: {message}")),
                context: ctx,
            },
            _ => self,
        }
    }

    /// Add operation context to an error for better debugging
    #[must_use]
    pub fn with_operation_context(self, operation: &str, api: &str) -> Self {
        match self {
            Self::Internal {
                kind,
                message,
                context,
            } => Self::Internal {
                kind,
                message: Cow::Owned(format!("Operation '{operation}' on API '{api}': {message}")),
                context,
            },
            Self::Network(e) => {
                Self::invalid_config(format!("Operation '{operation}' on API '{api}': {e}"))
            }
            _ => self,
        }
    }

    /// Add suggestions to error messages for better user guidance
    #[must_use]
    pub fn with_suggestion(self, suggestion: &str) -> Self {
        match self {
            Self::Internal {
                kind,
                message,
                context,
            } => Self::Internal {
                kind,
                message,
                context: context.map_or_else(
                    || {
                        Some(ErrorContext::with_suggestion(Cow::Owned(
                            suggestion.to_string(),
                        )))
                    },
                    |mut ctx| {
                        ctx.suggestion = Some(Cow::Owned(suggestion.to_string()));
                        Some(ctx)
                    },
                ),
            },
            _ => self,
        }
    }

    /// Convert error to JSON representation for structured output
    #[must_use]
    pub fn to_json(&self) -> JsonError {
        let (error_type, message, context, details): (
            &str,
            String,
            Option<Cow<'static, str>>,
            Option<serde_json::Value>,
        ) = match self {
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
                ("FileSystem", io_err.to_string(), context, None)
            }
            Self::Network(req_err) => {
                let context = match () {
                    () if req_err.is_connect() => Some(Cow::Borrowed(constants::ERR_CONNECTION)),
                    () if req_err.is_timeout() => Some(Cow::Borrowed(constants::ERR_TIMEOUT)),
                    () if req_err.is_status() => {
                        req_err.status().and_then(|status| match status.as_u16() {
                            401 => Some(Cow::Borrowed(constants::ERR_API_CREDENTIALS)),
                            403 => Some(Cow::Borrowed(constants::ERR_PERMISSION_DENIED)),
                            404 => Some(Cow::Borrowed(constants::ERR_ENDPOINT_NOT_FOUND)),
                            429 => Some(Cow::Borrowed(constants::ERR_RATE_LIMITED)),
                            500..=599 => Some(Cow::Borrowed(constants::ERR_SERVER_ERROR)),
                            _ => None,
                        })
                    }
                    () => None,
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
            Self::Toml(toml_err) => (
                "TOMLParsing",
                toml_err.to_string(),
                Some(Cow::Borrowed(constants::ERR_TOML_SYNTAX)),
                None,
            ),
            Self::Internal {
                kind,
                message,
                context: ctx,
            } => {
                let context = ctx.as_ref().and_then(|c| c.suggestion.clone());
                let details = ctx.as_ref().and_then(|c| c.details.clone());
                (kind.as_str(), message.to_string(), context, details)
            }
            Self::Anyhow(anyhow_err) => ("Unknown", anyhow_err.to_string(), None, None),
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
        Self::Internal {
            kind: ErrorKind::Specification,
            message: Cow::Owned(format!("API specification '{name}' not found")),
            context: Some(
                ErrorContext::with_detail("spec_name", &name)
                    .and_suggestion(constants::MSG_USE_CONFIG_LIST),
            ),
        }
    }

    /// Create a specification already exists error
    pub fn spec_already_exists(name: impl Into<String>) -> Self {
        let name = name.into();
        Self::Internal {
            kind: ErrorKind::Specification,
            message: Cow::Owned(format!(
                "API specification '{name}' already exists. Use --force to overwrite"
            )),
            context: Some(ErrorContext::with_detail("spec_name", &name)),
        }
    }

    /// Create a cached spec not found error
    pub fn cached_spec_not_found(name: impl Into<String>) -> Self {
        let name = name.into();
        Self::Internal {
            kind: ErrorKind::Specification,
            message: Cow::Owned(format!(
                "No cached spec found for '{name}'. Run 'aperture config add {name}' first"
            )),
            context: Some(ErrorContext::with_detail("spec_name", &name)),
        }
    }

    /// Create a cached spec corrupted error
    pub fn cached_spec_corrupted(name: impl Into<String>, reason: impl Into<String>) -> Self {
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
        let scheme_name = scheme_name.into();
        let env_var = env_var.into();
        let suggestion = crate::suggestions::suggest_auth_fix(&scheme_name, Some(&env_var));
        Self::Internal {
            kind: ErrorKind::Authentication,
            message: Cow::Owned(format!(
                "Environment variable '{env_var}' required for authentication '{scheme_name}' is not set"
            )),
            context: Some(ErrorContext::new(
                Some(json!({ "scheme_name": scheme_name, "env_var": env_var })),
                Some(Cow::Owned(suggestion)),
            )),
        }
    }

    /// Create an unsupported auth scheme error
    pub fn unsupported_auth_scheme(scheme: impl Into<String>) -> Self {
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
        Self::Internal {
            kind: ErrorKind::Validation,
            message: Cow::Owned(format!("Invalid configuration: {reason}")),
            context: Some(
                ErrorContext::with_detail("reason", &reason)
                    .and_suggestion("Check the configuration file syntax and structure."),
            ),
        }
    }

    /// Create an invalid JSON body error
    pub fn invalid_json_body(reason: impl Into<String>) -> Self {
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
        Self::Internal {
            kind: ErrorKind::Interactive,
            message: Cow::Owned(format!("Input too long (maximum {max_length} characters)")),
            context: Some(
                ErrorContext::with_detail("max_length", max_length)
                    .and_suggestion("Please provide a shorter input."),
            ),
        }
    }

    /// Create an interactive invalid characters error
    pub fn interactive_invalid_characters(
        invalid_chars: impl Into<String>,
        suggestion: impl Into<String>,
    ) -> Self {
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
    pub const fn interactive_timeout() -> Self {
        Self::Internal {
            kind: ErrorKind::Interactive,
            message: Cow::Borrowed("Input timeout - no response received"),
            context: Some(ErrorContext::with_suggestion(Cow::Borrowed(
                "Please respond within the timeout period.",
            ))),
        }
    }

    /// Create an interactive retries exhausted error
    pub fn interactive_retries_exhausted(
        max_retries: usize,
        last_error: impl Into<String>,
        suggestions: &[String],
    ) -> Self {
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
        Self::Internal {
            kind: ErrorKind::ServerVariable,
            message: Cow::Owned(format!("Required server variable '{name}' is not provided")),
            context: Some(
                ErrorContext::with_detail("variable_name", &name).and_suggestion(format!(
                    "Provide the variable with --server-var {name}=<value>"
                )),
            ),
        }
    }

    /// Create an unknown server variable error
    pub fn unknown_server_variable(name: impl Into<String>, available: &[String]) -> Self {
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
        Self::Internal {
            kind: ErrorKind::Interactive,
            message: Cow::Owned(format!(
                "Invalid environment variable name '{name}': {reason}"
            )),
            context: Some(
                ErrorContext::with_name_reason("variable_name", &name, &reason)
                    .and_suggestion(suggestion),
            ),
        }
    }

    /// Create an invalid server variable format error
    pub fn invalid_server_var_format(arg: impl Into<String>, reason: impl Into<String>) -> Self {
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

    /// Create an operation not found error with suggestions
    pub fn operation_not_found_with_suggestions(
        operation: impl Into<String>,
        suggestions: &[String],
    ) -> Self {
        let operation = operation.into();
        let suggestion_text = if suggestions.is_empty() {
            "Check available operations with --help or --describe-json".to_string()
        } else {
            format!("Did you mean one of these?\n{}", suggestions.join("\n"))
        };

        Self::Internal {
            kind: ErrorKind::Validation,
            message: Cow::Owned(format!("Operation '{operation}' not found")),
            context: Some(ErrorContext::new(
                Some(json!({
                    "operation": operation,
                    "suggestions": suggestions
                })),
                Some(Cow::Owned(suggestion_text)),
            )),
        }
    }

    /// Create a network request failed error
    pub fn network_request_failed(reason: impl Into<String>) -> Self {
        let reason = reason.into();
        Self::Internal {
            kind: ErrorKind::Network,
            message: Cow::Owned(format!("Network request failed: {reason}")),
            context: Some(
                ErrorContext::with_detail("reason", &reason)
                    .and_suggestion("Check network connectivity and URL validity"),
            ),
        }
    }

    /// Create a serialization error
    pub fn serialization_error(reason: impl Into<String>) -> Self {
        let reason = reason.into();
        Self::Internal {
            kind: ErrorKind::Validation,
            message: Cow::Owned(format!("Serialization failed: {reason}")),
            context: Some(
                ErrorContext::with_detail("reason", &reason)
                    .and_suggestion("Check data structure validity"),
            ),
        }
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
        Self::Internal {
            kind: ErrorKind::Validation,
            message: Cow::Owned(format!("Invalid command for '{context}': {reason}")),
            context: Some(
                ErrorContext::with_name_reason("context", &context, &reason)
                    .and_suggestion("Check available commands with --help or --describe-json"),
            ),
        }
    }

    /// Create an HTTP error with context
    pub fn http_error_with_context(
        status: u16,
        body: impl Into<String>,
        api_name: impl Into<String>,
        operation_id: Option<impl Into<String>>,
        security_schemes: &[String],
    ) -> Self {
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
        Self::Internal {
            kind: ErrorKind::Validation,
            message: Cow::Owned(format!("JQ filter error in '{filter}': {reason}")),
            context: Some(
                ErrorContext::with_name_reason("filter", &filter, &reason)
                    .and_suggestion("Check JQ filter syntax and data structure compatibility"),
            ),
        }
    }

    /// Create a transient network error
    pub fn transient_network_error(reason: impl Into<String>, retryable: bool) -> Self {
        let reason = reason.into();
        Self::Internal {
            kind: ErrorKind::Network,
            message: Cow::Owned(format!("Transient network error: {reason}")),
            context: Some(ErrorContext::new(
                Some(serde_json::json!({
                    "reason": reason,
                    "retryable": retryable
                })),
                Some(Cow::Borrowed(if retryable {
                    "This error may be temporary and could succeed on retry"
                } else {
                    "This error is not retryable"
                })),
            )),
        }
    }

    /// Create a retry limit exceeded error
    pub fn retry_limit_exceeded(max_attempts: u32, last_error: impl Into<String>) -> Self {
        let last_error = last_error.into();
        Self::Internal {
            kind: ErrorKind::Network,
            message: Cow::Owned(format!(
                "Retry limit exceeded after {max_attempts} attempts: {last_error}"
            )),
            context: Some(ErrorContext::new(
                Some(serde_json::json!({
                    "max_attempts": max_attempts,
                    "last_error": last_error
                })),
                Some(Cow::Borrowed(
                    "Consider checking network connectivity or increasing retry limits",
                )),
            )),
        }
    }

    /// Create a retry limit exceeded error with detailed retry information
    #[allow(clippy::too_many_arguments)]
    pub fn retry_limit_exceeded_detailed(
        max_attempts: u32,
        attempts_made: u32,
        last_error: impl Into<String>,
        initial_delay_ms: u64,
        max_delay_ms: u64,
        last_status_code: Option<u16>,
        operation_id: impl Into<String>,
    ) -> Self {
        let last_error = last_error.into();
        let operation_id = operation_id.into();
        Self::Internal {
            kind: ErrorKind::Network,
            message: Cow::Owned(format!(
                "Retry limit exceeded after {attempts_made}/{max_attempts} attempts for {operation_id}: {last_error}"
            )),
            context: Some(ErrorContext::new(
                Some(serde_json::json!({
                    "retry_info": {
                        "max_attempts": max_attempts,
                        "attempts_made": attempts_made,
                        "initial_delay_ms": initial_delay_ms,
                        "max_delay_ms": max_delay_ms,
                        "last_status_code": last_status_code,
                        "operation_id": operation_id
                    },
                    "last_error": last_error
                })),
                Some(Cow::Borrowed(
                    "Consider checking network connectivity, API availability, or increasing retry limits",
                )),
            )),
        }
    }

    /// Create a request timeout error
    #[must_use]
    pub fn request_timeout(timeout_seconds: u64) -> Self {
        Self::Internal {
            kind: ErrorKind::Network,
            message: Cow::Owned(format!("Request timed out after {timeout_seconds} seconds")),
            context: Some(ErrorContext::new(
                Some(serde_json::json!({
                    "timeout_seconds": timeout_seconds
                })),
                Some(Cow::Borrowed(
                    "Consider increasing the timeout or checking network connectivity",
                )),
            )),
        }
    }

    /// Create a missing path parameter error
    pub fn missing_path_parameter(name: impl Into<String>) -> Self {
        let name = name.into();
        Self::Internal {
            kind: ErrorKind::Validation,
            message: Cow::Owned(format!("Missing required path parameter: {name}")),
            context: Some(
                ErrorContext::with_detail("parameter_name", &name)
                    .and_suggestion("Provide a value for this required path parameter"),
            ),
        }
    }

    /// Create a general I/O error
    pub fn io_error(message: impl Into<String>) -> Self {
        let message = message.into();
        Self::Internal {
            kind: ErrorKind::Runtime,
            message: Cow::Owned(message),
            context: None,
        }
    }

    /// Create an invalid idempotency key error
    #[must_use]
    pub const fn invalid_idempotency_key() -> Self {
        Self::Internal {
            kind: ErrorKind::Headers,
            message: Cow::Borrowed("Invalid idempotency key format"),
            context: Some(ErrorContext::new(
                None,
                Some(Cow::Borrowed(
                    "Ensure the idempotency key contains only valid header characters",
                )),
            )),
        }
    }

    /// Create an editor not set error
    #[must_use]
    pub const fn editor_not_set() -> Self {
        Self::Internal {
            kind: ErrorKind::Interactive,
            message: Cow::Borrowed("EDITOR environment variable not set"),
            context: Some(ErrorContext::new(
                None,
                Some(Cow::Borrowed(
                    "Set your preferred editor: export EDITOR=vim",
                )),
            )),
        }
    }

    /// Create an editor failed error
    pub fn editor_failed(name: impl Into<String>) -> Self {
        let name = name.into();
        Self::Internal {
            kind: ErrorKind::Interactive,
            message: Cow::Owned(format!("Editor '{name}' failed to complete")),
            context: Some(ErrorContext::new(
                Some(serde_json::json!({ "editor": name })),
                Some(Cow::Borrowed(
                    "Check if the editor is properly installed and configured",
                )),
            )),
        }
    }

    // ---- API Context Name Errors ----

    /// Create an invalid API context name error
    pub fn invalid_api_context_name(name: impl Into<String>, reason: impl Into<String>) -> Self {
        let name = name.into();
        let reason = reason.into();
        Self::Internal {
            kind: ErrorKind::Validation,
            message: Cow::Owned(format!("Invalid API context name '{name}': {reason}")),
            context: Some(ErrorContext::new(
                Some(json!({ "name": name, "reason": reason })),
                Some(Cow::Borrowed(
                    "API names must start with a letter or digit and contain only letters, digits, dots, hyphens, or underscores (max 64 chars).",
                )),
            )),
        }
    }

    // ---- Settings Errors ----

    /// Create an unknown setting key error
    pub fn unknown_setting_key(key: impl Into<String>) -> Self {
        let key = key.into();
        Self::Internal {
            kind: ErrorKind::Validation,
            message: Cow::Owned(format!("Unknown setting key: '{key}'")),
            context: Some(ErrorContext::new(
                Some(json!({ "key": key })),
                Some(Cow::Borrowed(
                    "Run 'aperture config settings' to see available settings.",
                )),
            )),
        }
    }

    /// Create an invalid setting value error
    pub fn invalid_setting_value(
        key: crate::config::settings::SettingKey,
        value: impl Into<String>,
    ) -> Self {
        let value = value.into();
        let expected_type = key.type_name();
        Self::Internal {
            kind: ErrorKind::Validation,
            message: Cow::Owned(format!(
                "Invalid value for '{key}': expected {expected_type}, got '{value}'"
            )),
            context: Some(ErrorContext::new(
                Some(json!({
                    "key": key.as_str(),
                    "value": value,
                    "expected_type": expected_type
                })),
                Some(Cow::Owned(format!(
                    "Provide a valid {expected_type} value for this setting."
                ))),
            )),
        }
    }

    /// Create a setting value out of range error
    pub fn setting_value_out_of_range(
        key: crate::config::settings::SettingKey,
        value: impl Into<String>,
        reason: &str,
    ) -> Self {
        let value = value.into();
        Self::Internal {
            kind: ErrorKind::Validation,
            message: Cow::Owned(format!(
                "Value '{value}' out of range for '{key}': {reason}"
            )),
            context: Some(ErrorContext::new(
                Some(json!({
                    "key": key.as_str(),
                    "value": value,
                    "reason": reason
                })),
                Some(Cow::Owned(format!(
                    "Provide a value within the valid range: {reason}"
                ))),
            )),
        }
    }
}
