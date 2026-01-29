//! Request and response logging utilities with automatic secret redaction.
//!
//! This module provides logging capabilities for HTTP requests and responses,
//! with built-in automatic redaction of sensitive information including:
//! - Authorization headers
//! - API keys in query parameters
//! - Values matching configured `x-aperture-secret` environment variables

use crate::cache::models::CachedSpec;
use crate::config::models::GlobalConfig;
use tracing::{debug, info, trace};

/// Minimum length for a secret to be redacted in body content.
/// Shorter secrets might cause false positives in legitimate content.
const MIN_SECRET_LENGTH_FOR_BODY_REDACTION: usize = 8;

/// Context containing resolved secret values for dynamic redaction.
///
/// This struct collects actual secret values from environment variables
/// referenced by `x-aperture-secret` extensions and config-based secrets,
/// allowing them to be redacted from logs wherever they appear.
#[derive(Debug, Default, Clone)]
pub struct SecretContext {
    /// Resolved secret values that should be redacted
    secrets: Vec<String>,
}

/// Collects non-empty secret values from spec's security schemes.
fn collect_secrets_from_spec(spec: &CachedSpec, secrets: &mut Vec<String>) {
    for scheme in spec.security_schemes.values() {
        let Some(ref aperture_secret) = scheme.aperture_secret else {
            continue;
        };
        let Ok(value) = std::env::var(&aperture_secret.name) else {
            continue;
        };
        if !value.is_empty() {
            secrets.push(value);
        }
    }
}

/// Collects non-empty secret values from config-based secrets.
fn collect_secrets_from_config(
    global_config: Option<&GlobalConfig>,
    api_name: &str,
    secrets: &mut Vec<String>,
) {
    let Some(config) = global_config else {
        return;
    };
    let Some(api_config) = config.api_configs.get(api_name) else {
        return;
    };
    for secret in api_config.secrets.values() {
        let Ok(value) = std::env::var(&secret.name) else {
            continue;
        };
        if !value.is_empty() {
            secrets.push(value);
        }
    }
}

impl SecretContext {
    /// Creates an empty `SecretContext` with no secrets to redact.
    #[must_use]
    pub fn empty() -> Self {
        Self::default()
    }

    /// Creates a `SecretContext` by collecting secrets from the spec and config.
    ///
    /// This resolves environment variables referenced by:
    /// 1. `x-aperture-secret` extensions in the `OpenAPI` spec's security schemes
    /// 2. Config-based secrets in the global configuration
    ///
    /// # Arguments
    /// * `spec` - The cached API specification containing security schemes
    /// * `api_name` - The name of the API (used to look up config-based secrets)
    /// * `global_config` - Optional global configuration with config-based secrets
    #[must_use]
    pub fn from_spec_and_config(
        spec: &CachedSpec,
        api_name: &str,
        global_config: Option<&GlobalConfig>,
    ) -> Self {
        let mut secrets = Vec::new();

        // Collect secrets from x-aperture-secret extensions in security schemes
        collect_secrets_from_spec(spec, &mut secrets);

        // Collect secrets from config-based secrets
        collect_secrets_from_config(global_config, api_name, &mut secrets);

        // Remove duplicates while preserving order
        secrets.sort();
        secrets.dedup();

        Self { secrets }
    }

    /// Checks if a value exactly matches any of the secrets.
    #[must_use]
    pub fn is_secret(&self, value: &str) -> bool {
        self.secrets.iter().any(|s| s == value)
    }

    /// Redacts all occurrences of secrets in the given text.
    ///
    /// Only redacts secrets that are at least `MIN_SECRET_LENGTH_FOR_BODY_REDACTION`
    /// characters long to avoid false positives with short values.
    #[must_use]
    pub fn redact_secrets_in_text(&self, text: &str) -> String {
        let mut result = text.to_string();
        for secret in &self.secrets {
            if secret.len() >= MIN_SECRET_LENGTH_FOR_BODY_REDACTION {
                result = result.replace(secret, "[REDACTED]");
            }
        }
        result
    }

    /// Returns true if this context has any secrets to redact.
    #[must_use]
    pub const fn has_secrets(&self) -> bool {
        !self.secrets.is_empty()
    }
}

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
///
/// # Arguments
/// * `method` - HTTP method (GET, POST, etc.)
/// * `url` - Request URL (sensitive query params will be redacted)
/// * `headers` - Optional request headers (sensitive headers will be redacted)
/// * `body` - Optional request body
/// * `secret_ctx` - Optional context for dynamic secret redaction
pub fn log_request(
    method: &str,
    url: &str,
    headers: Option<&reqwest::header::HeaderMap>,
    body: Option<&str>,
    secret_ctx: Option<&SecretContext>,
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
            let redacted_body = secret_ctx.map_or_else(
                || body_content.to_string(),
                |ctx| ctx.redact_secrets_in_text(body_content),
            );
            trace!(
                target: "aperture::executor",
                "Request body: {}",
                redacted_body
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
        let raw_value = String::from_utf8_lossy(value.as_bytes()).to_string();
        let display_value = redact_header_value(header_str, &raw_value, secret_ctx);
        debug!(
            target: "aperture::executor",
            "  {}: {}",
            header_str,
            display_value
        );
    }

    // Log body at trace level
    if let Some(body_content) = body {
        let redacted_body = secret_ctx.map_or_else(
            || body_content.to_string(),
            |ctx| ctx.redact_secrets_in_text(body_content),
        );
        trace!(
            target: "aperture::executor",
            "Request body: {}",
            redacted_body
        );
    }
}

/// Redacts a header value based on static rules and dynamic secret context.
fn redact_header_value(
    header_name: &str,
    value: &str,
    secret_ctx: Option<&SecretContext>,
) -> String {
    // Always redact known sensitive headers
    if should_redact_header(header_name) {
        return "[REDACTED]".to_string();
    }

    // Check if the value matches a dynamic secret
    let is_dynamic_secret = secret_ctx.is_some_and(|ctx| ctx.is_secret(value));
    if is_dynamic_secret {
        return "[REDACTED]".to_string();
    }

    value.to_string()
}

/// Logs an HTTP response with optional headers and body
///
/// # Arguments
/// * `status` - HTTP status code
/// * `duration_ms` - Request duration in milliseconds
/// * `headers` - Optional response headers (sensitive headers will be redacted)
/// * `body` - Optional response body
/// * `max_body_len` - Maximum body length to log before truncation
/// * `secret_ctx` - Optional context for dynamic secret redaction
pub fn log_response(
    status: u16,
    duration_ms: u128,
    headers: Option<&reqwest::header::HeaderMap>,
    body: Option<&str>,
    max_body_len: usize,
    secret_ctx: Option<&SecretContext>,
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
        log_response_body(body, max_body_len, secret_ctx);
        return;
    };

    debug!(
        target: "aperture::executor",
        "Response headers:"
    );
    for (name, value) in header_map {
        let header_str = name.as_str();
        let raw_value = String::from_utf8_lossy(value.as_bytes()).to_string();
        let display_value = redact_header_value(header_str, &raw_value, secret_ctx);
        debug!(
            target: "aperture::executor",
            "  {}: {}",
            header_str,
            display_value
        );
    }

    // Log body at trace level with truncation
    log_response_body(body, max_body_len, secret_ctx);
}

/// Helper function to log response body with truncation
fn log_response_body(body: Option<&str>, max_body_len: usize, secret_ctx: Option<&SecretContext>) {
    let Some(body_content) = body else {
        return;
    };

    // Redact secrets in body before logging
    let redacted_body = secret_ctx.map_or_else(
        || body_content.to_string(),
        |ctx| ctx.redact_secrets_in_text(body_content),
    );

    if redacted_body.len() > max_body_len {
        trace!(
            target: "aperture::executor",
            "Response body: {} (truncated at {} chars)",
            &redacted_body[..max_body_len],
            max_body_len
        );
    } else {
        trace!(
            target: "aperture::executor",
            "Response body: {}",
            redacted_body
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

    // Note: Environment variable tests for get_max_body_len have been moved
    // to logging_integration_tests.rs to avoid race conditions when tests
    // run in parallel. Unit tests here should not depend on env vars.

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

    // SecretContext tests

    #[test]
    fn test_secret_context_empty() {
        let ctx = SecretContext::empty();
        assert!(!ctx.has_secrets());
        assert!(!ctx.is_secret("any_value"));
    }

    #[test]
    fn test_secret_context_is_secret() {
        let mut ctx = SecretContext::empty();
        ctx.secrets = vec!["my_secret_token".to_string()];

        assert!(ctx.has_secrets());
        assert!(ctx.is_secret("my_secret_token"));
        assert!(!ctx.is_secret("other_value"));
    }

    #[test]
    fn test_secret_context_redact_secrets_in_text() {
        let mut ctx = SecretContext::empty();
        ctx.secrets = vec!["secret123abc".to_string()]; // 12 chars, above minimum

        let text = "The token is secret123abc and should be hidden";
        let redacted = ctx.redact_secrets_in_text(text);
        assert_eq!(redacted, "The token is [REDACTED] and should be hidden");
    }

    #[test]
    fn test_secret_context_short_secrets_not_redacted_in_body() {
        let mut ctx = SecretContext::empty();
        ctx.secrets = vec!["short".to_string()]; // 5 chars, below minimum

        let text = "This text contains short word";
        let redacted = ctx.redact_secrets_in_text(text);
        // Short secrets should not be redacted in body to avoid false positives
        assert_eq!(redacted, "This text contains short word");
    }

    #[test]
    fn test_secret_context_multiple_secrets() {
        let mut ctx = SecretContext::empty();
        ctx.secrets = vec![
            "first_secret_value".to_string(),
            "second_secret_val".to_string(),
        ];

        let text = "first_secret_value and second_secret_val are both here";
        let redacted = ctx.redact_secrets_in_text(text);
        assert_eq!(redacted, "[REDACTED] and [REDACTED] are both here");
    }

    #[test]
    fn test_redact_header_value_known_header() {
        // Known sensitive headers are always redacted regardless of context
        let result = redact_header_value("Authorization", "Bearer token123", None);
        assert_eq!(result, "[REDACTED]");
    }

    #[test]
    fn test_redact_header_value_dynamic_secret() {
        let mut ctx = SecretContext::empty();
        ctx.secrets = vec!["my_api_key_12345".to_string()];

        // Unknown header but value matches a dynamic secret
        let result = redact_header_value("X-Custom-Header", "my_api_key_12345", Some(&ctx));
        assert_eq!(result, "[REDACTED]");
    }

    #[test]
    fn test_redact_header_value_no_match() {
        let ctx = SecretContext::empty();

        // Unknown header, no secret match
        let result = redact_header_value("X-Custom-Header", "some_value", Some(&ctx));
        assert_eq!(result, "some_value");
    }
}
