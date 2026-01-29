//! Request and response logging utilities with automatic secret redaction.
//!
//! This module provides logging capabilities for HTTP requests and responses,
//! with built-in automatic redaction of sensitive information including:
//! - Authorization headers
//! - API keys in query parameters
//! - Values matching configured `x-aperture-secret` environment variables

use tracing::{debug, info, trace};

/// Returns the canonical status text for an HTTP status code
#[must_use]
const fn http_status_text(status: u16) -> &'static str {
    match status {
        // 2xx Success
        200 => "OK",
        201 => "Created",
        202 => "Accepted",
        204 => "No Content",
        // 3xx Redirection
        301 => "Moved Permanently",
        302 => "Found",
        304 => "Not Modified",
        307 => "Temporary Redirect",
        308 => "Permanent Redirect",
        // 4xx Client Error
        400 => "Bad Request",
        401 => "Unauthorized",
        403 => "Forbidden",
        404 => "Not Found",
        405 => "Method Not Allowed",
        409 => "Conflict",
        410 => "Gone",
        422 => "Unprocessable Entity",
        429 => "Too Many Requests",
        // 5xx Server Error
        500 => "Internal Server Error",
        501 => "Not Implemented",
        502 => "Bad Gateway",
        503 => "Service Unavailable",
        504 => "Gateway Timeout",
        // Default fallback
        _ => "",
    }
}

/// Redacts sensitive values from strings
#[must_use]
pub fn redact_sensitive_value(value: &str) -> String {
    if value.is_empty() {
        value.to_string()
    } else {
        "[REDACTED]".to_string()
    }
}

/// Checks if a header name should be redacted.
///
/// This is the single source of truth for sensitive header identification.
/// Used by both logging and request building to ensure consistent redaction.
#[must_use]
pub fn should_redact_header(header_name: &str) -> bool {
    let lower = header_name.to_lowercase();
    matches!(
        lower.as_str(),
        // Standard authentication headers
        "authorization"
            | "proxy-authorization"
            // API key variants
            | "x-api-key"
            | "x-api-token"
            | "api-key"
            | "api_key"
            // Auth token variants
            | "x-access-token"
            | "x-auth-token"
            | "x-secret-token"
            // Generic sensitive headers
            | "token"
            | "secret"
            | "password"
            // Webhook secrets
            | "x-webhook-secret"
            // Session/cookie headers
            | "cookie"
            | "set-cookie"
            // CSRF tokens
            | "x-csrf-token"
            | "x-xsrf-token"
            // Cloud provider tokens
            | "x-amz-security-token"
            // Platform-specific tokens
            | "private-token"
    )
}

/// Checks if a query parameter name should be redacted
#[must_use]
fn should_redact_query_param(param_name: &str) -> bool {
    let lower = param_name.to_lowercase();
    matches!(
        lower.as_str(),
        // API key variants
        "api_key"
            | "apikey"
            | "api-key"
            | "key"
            // Token variants
            | "token"
            | "access_token"
            | "accesstoken"
            | "auth_token"
            | "authtoken"
            | "bearer_token"
            | "refresh_token"
            // Secret variants
            | "secret"
            | "api_secret"
            | "client_secret"
            // Password variants
            | "password"
            | "passwd"
            | "pwd"
            // Signature variants
            | "signature"
            | "sig"
            // Session IDs
            | "session_id"
            | "sessionid"
            // Other common sensitive params
            | "auth"
            | "authorization"
            | "credentials"
    )
}

/// Redacts sensitive query parameters from a URL
///
/// Returns the URL with sensitive parameter values replaced with `[REDACTED]`.
#[must_use]
pub fn redact_url_query_params(url: &str) -> String {
    // Find the query string start
    let Some(query_start) = url.find('?') else {
        return url.to_string();
    };

    let base_url = &url[..query_start];
    let query_string = &url[query_start + 1..];

    // Handle fragment if present
    let (query_part, fragment) =
        query_string
            .find('#')
            .map_or((query_string, None), |frag_start| {
                (
                    &query_string[..frag_start],
                    Some(&query_string[frag_start..]),
                )
            });

    // Process each query parameter
    let redacted_params: Vec<String> = query_part
        .split('&')
        .map(|param| {
            param.find('=').map_or_else(
                || param.to_string(),
                |eq_pos| {
                    let name = &param[..eq_pos];
                    if should_redact_query_param(name) {
                        format!("{name}=[REDACTED]")
                    } else {
                        param.to_string()
                    }
                },
            )
        })
        .collect();

    let mut result = format!("{base_url}?{}", redacted_params.join("&"));
    if let Some(frag) = fragment {
        result.push_str(frag);
    }
    result
}

/// Logs an HTTP request with optional headers and body
pub fn log_request(
    method: &str,
    url: &str,
    headers: Option<&reqwest::header::HeaderMap>,
    body: Option<&str>,
) {
    // Redact sensitive query parameters from URL before logging
    let redacted_url = redact_url_query_params(url);

    // Log at info level: method, URL, and duration (duration added by caller)
    info!(
        target: "aperture::executor",
        "→ {} {}",
        method.to_uppercase(),
        redacted_url
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
    let status_text = http_status_text(status);
    info!(
        target: "aperture::executor",
        "← {} {} ({}ms)",
        status,
        status_text,
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
    fn test_should_redact_proxy_authorization() {
        assert!(should_redact_header("Proxy-Authorization"));
        assert!(should_redact_header("proxy-authorization"));
    }

    #[test]
    fn test_should_redact_session_headers() {
        assert!(should_redact_header("Cookie"));
        assert!(should_redact_header("Set-Cookie"));
        assert!(should_redact_header("cookie"));
        assert!(should_redact_header("set-cookie"));
    }

    #[test]
    fn test_should_redact_csrf_tokens() {
        assert!(should_redact_header("X-CSRF-Token"));
        assert!(should_redact_header("X-XSRF-Token"));
        assert!(should_redact_header("x-csrf-token"));
        assert!(should_redact_header("x-xsrf-token"));
    }

    #[test]
    fn test_should_redact_cloud_tokens() {
        assert!(should_redact_header("X-Amz-Security-Token"));
        assert!(should_redact_header("x-amz-security-token"));
        assert!(should_redact_header("Private-Token"));
        assert!(should_redact_header("private-token"));
    }

    #[test]
    fn test_should_not_redact_regular_header() {
        assert!(!should_redact_header("Content-Type"));
        assert!(!should_redact_header("User-Agent"));
        assert!(!should_redact_header("Accept"));
        assert!(!should_redact_header("Cache-Control"));
        assert!(!should_redact_header("X-Request-Id"));
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

    #[test]
    fn test_http_status_text() {
        // Success codes
        assert_eq!(http_status_text(200), "OK");
        assert_eq!(http_status_text(201), "Created");
        assert_eq!(http_status_text(204), "No Content");

        // Client error codes
        assert_eq!(http_status_text(400), "Bad Request");
        assert_eq!(http_status_text(401), "Unauthorized");
        assert_eq!(http_status_text(403), "Forbidden");
        assert_eq!(http_status_text(404), "Not Found");
        assert_eq!(http_status_text(429), "Too Many Requests");

        // Server error codes
        assert_eq!(http_status_text(500), "Internal Server Error");
        assert_eq!(http_status_text(502), "Bad Gateway");
        assert_eq!(http_status_text(503), "Service Unavailable");

        // Unknown codes return empty string
        assert_eq!(http_status_text(999), "");
    }

    #[test]
    fn test_should_redact_query_param() {
        // API key variants
        assert!(should_redact_query_param("api_key"));
        assert!(should_redact_query_param("apikey"));
        assert!(should_redact_query_param("API_KEY"));
        assert!(should_redact_query_param("key"));

        // Token variants
        assert!(should_redact_query_param("token"));
        assert!(should_redact_query_param("access_token"));
        assert!(should_redact_query_param("auth_token"));

        // Secret variants
        assert!(should_redact_query_param("secret"));
        assert!(should_redact_query_param("client_secret"));

        // Password variants
        assert!(should_redact_query_param("password"));

        // Non-sensitive params
        assert!(!should_redact_query_param("page"));
        assert!(!should_redact_query_param("limit"));
        assert!(!should_redact_query_param("id"));
        assert!(!should_redact_query_param("filter"));
    }

    #[test]
    fn test_redact_url_query_params_with_api_key() {
        let url = "https://api.example.com/users?api_key=secret123&page=1";
        let redacted = redact_url_query_params(url);
        assert_eq!(
            redacted,
            "https://api.example.com/users?api_key=[REDACTED]&page=1"
        );
    }

    #[test]
    fn test_redact_url_query_params_multiple_sensitive() {
        let url = "https://api.example.com/auth?token=abc123&secret=xyz789&user=john";
        let redacted = redact_url_query_params(url);
        assert_eq!(
            redacted,
            "https://api.example.com/auth?token=[REDACTED]&secret=[REDACTED]&user=john"
        );
    }

    #[test]
    fn test_redact_url_query_params_no_query_string() {
        let url = "https://api.example.com/users";
        let redacted = redact_url_query_params(url);
        assert_eq!(redacted, "https://api.example.com/users");
    }

    #[test]
    fn test_redact_url_query_params_with_fragment() {
        let url = "https://api.example.com/users?api_key=secret123#section";
        let redacted = redact_url_query_params(url);
        assert_eq!(
            redacted,
            "https://api.example.com/users?api_key=[REDACTED]#section"
        );
    }

    #[test]
    fn test_redact_url_query_params_empty_value() {
        let url = "https://api.example.com/users?api_key=&page=1";
        let redacted = redact_url_query_params(url);
        assert_eq!(
            redacted,
            "https://api.example.com/users?api_key=[REDACTED]&page=1"
        );
    }

    #[test]
    fn test_redact_url_query_params_no_sensitive() {
        let url = "https://api.example.com/users?page=1&limit=10";
        let redacted = redact_url_query_params(url);
        assert_eq!(redacted, "https://api.example.com/users?page=1&limit=10");
    }
}
