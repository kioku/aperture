//! Request and response logging utilities with automatic secret redaction.
//!
//! This module provides logging capabilities for HTTP requests and responses,
//! with built-in automatic redaction of sensitive information including:
//! - Authorization headers
//! - API keys in query parameters
//! - Values matching configured `x-aperture-secret` environment variables

use tracing::{debug, info, trace};

/// Redacts sensitive values from strings
#[must_use]
pub fn redact_sensitive_value(value: &str) -> String {
    if value.is_empty() {
        value.to_string()
    } else {
        "[REDACTED]".to_string()
    }
}

/// Checks if a header name should be redacted
#[must_use]
pub fn should_redact_header(header_name: &str) -> bool {
    let lower = header_name.to_lowercase();
    matches!(
        lower.as_str(),
        "authorization"
            | "x-api-key"
            | "x-access-token"
            | "x-auth-token"
            | "api-key"
            | "api_key"
            | "token"
            | "secret"
            | "password"
            | "x-secret-token"
            | "x-webhook-secret"
    )
}

/// Logs an HTTP request with optional headers and body
pub fn log_request(
    method: &str,
    url: &str,
    headers: Option<&reqwest::header::HeaderMap>,
    body: Option<&str>,
) {
    // Log at info level: method, URL, and duration (duration added by caller)
    info!(
        target: "aperture::executor",
        "→ {} {}",
        method.to_uppercase(),
        url
    );

    // Log headers at debug level
    let Some(header_map) = headers else {
        if let Some(body_content) = body {
            trace!(
                target: "aperture::executor",
                "Request body: {}",
                body_content
            );
        }
        return;
    };

    debug!(
        target: "aperture::executor",
        "Request headers:"
    );
    for (name, value) in header_map {
        let header_str = name.as_str();
        let display_value = if should_redact_header(header_str) {
            "[REDACTED]".to_string()
        } else {
            String::from_utf8_lossy(value.as_bytes()).to_string()
        };
        debug!(
            target: "aperture::executor",
            "  {}: {}",
            header_str,
            display_value
        );
    }

    // Log body at trace level
    if let Some(body_content) = body {
        trace!(
            target: "aperture::executor",
            "Request body: {}",
            body_content
        );
    }
}

/// Logs an HTTP response with optional headers and body
pub fn log_response(
    status: u16,
    duration_ms: u128,
    headers: Option<&reqwest::header::HeaderMap>,
    body: Option<&str>,
    max_body_len: usize,
) {
    // Log at info level: status and duration
    info!(
        target: "aperture::executor",
        "← {} OK ({}ms)",
        status,
        duration_ms
    );

    // Log headers at debug level
    let Some(header_map) = headers else {
        log_response_body(body, max_body_len);
        return;
    };

    debug!(
        target: "aperture::executor",
        "Response headers:"
    );
    for (name, value) in header_map {
        let header_str = name.as_str();
        let display_value = if should_redact_header(header_str) {
            "[REDACTED]".to_string()
        } else {
            String::from_utf8_lossy(value.as_bytes()).to_string()
        };
        debug!(
            target: "aperture::executor",
            "  {}: {}",
            header_str,
            display_value
        );
    }

    // Log body at trace level with truncation
    log_response_body(body, max_body_len);
}

/// Helper function to log response body with truncation
fn log_response_body(body: Option<&str>, max_body_len: usize) {
    let Some(body_content) = body else {
        return;
    };

    if body_content.len() > max_body_len {
        trace!(
            target: "aperture::executor",
            "Response body: {} (truncated at {} chars)",
            &body_content[..max_body_len],
            max_body_len
        );
    } else {
        trace!(
            target: "aperture::executor",
            "Response body: {}",
            body_content
        );
    }
}

/// Gets the maximum body length from `APERTURE_LOG_MAX_BODY` environment variable
#[must_use]
pub fn get_max_body_len() -> usize {
    std::env::var("APERTURE_LOG_MAX_BODY")
        .ok()
        .and_then(|s| s.parse::<usize>().ok())
        .unwrap_or(1000)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_should_redact_header_authorization() {
        assert!(should_redact_header("Authorization"));
        assert!(should_redact_header("AUTHORIZATION"));
        assert!(should_redact_header("authorization"));
    }

    #[test]
    fn test_should_redact_header_api_key_variants() {
        assert!(should_redact_header("X-API-Key"));
        assert!(should_redact_header("X-Api-Key"));
        assert!(should_redact_header("api-key"));
        assert!(should_redact_header("API_KEY"));
        assert!(should_redact_header("api_key"));
    }

    #[test]
    fn test_should_not_redact_regular_header() {
        assert!(!should_redact_header("Content-Type"));
        assert!(!should_redact_header("User-Agent"));
        assert!(!should_redact_header("Accept"));
    }

    #[test]
    fn test_redact_sensitive_value() {
        assert_eq!(redact_sensitive_value("secret123"), "[REDACTED]");
        assert_eq!(redact_sensitive_value(""), "");
    }

    #[test]
    fn test_get_max_body_len_default() {
        // If APERTURE_LOG_MAX_BODY is not set, should return 1000
        std::env::remove_var("APERTURE_LOG_MAX_BODY");
        assert_eq!(get_max_body_len(), 1000);
    }

    #[test]
    fn test_get_max_body_len_custom() {
        // Set custom value and verify it's used
        std::env::set_var("APERTURE_LOG_MAX_BODY", "5000");
        assert_eq!(get_max_body_len(), 5000);
        std::env::remove_var("APERTURE_LOG_MAX_BODY");
    }

    #[test]
    fn test_get_max_body_len_invalid_value() {
        // If the value is invalid, should return default 1000
        std::env::set_var("APERTURE_LOG_MAX_BODY", "invalid");
        assert_eq!(get_max_body_len(), 1000);
        std::env::remove_var("APERTURE_LOG_MAX_BODY");
    }
}
