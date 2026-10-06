//! Request and response logging utilities with automatic secret redaction.
//!
//! This module provides logging capabilities for HTTP requests and responses,
//! with built-in automatic redaction of sensitive information including:
//! - Authorization headers
//! - API keys in query parameters
//! - Values matching configured `x-aperture-secret` environment variables

use crate::cache::models::CachedSpec;
use crate::config::models::GlobalConfig;
use base64::{engine::general_purpose::STANDARD, Engine as _};
use tracing::{debug, info, trace};

/// Minimum length for a secret to be redacted in body content.
/// Shorter secrets might cause false positives in legitimate content.
const MIN_SECRET_LENGTH_FOR_BODY_REDACTION: usize = 8;

/// Context containing resolved secret values for dynamic redaction.
///
/// This struct collects actual secret values from environment variables
/// referenced by `x-aperture-secret` extensions and config-based secrets,
/// allowing them to be redacted from logs wherever they appear.
#[derive(Default, Clone)]
pub struct SecretContext {
    /// Resolved configured secret values that should be redacted.
    secrets: Vec<String>,
    /// Final active credential values, including per-invocation overrides.
    active_secrets: Vec<String>,
    /// Final request carries credentials; untrusted bodies must not be diagnosed.
    authenticated: bool,
}

// Never expose resolved credentials through SDK/debug diagnostics.
impl std::fmt::Debug for SecretContext {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("SecretContext")
            .field("authenticated", &self.authenticated)
            .finish_non_exhaustive()
    }
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

        Self {
            secrets,
            active_secrets: Vec::new(),
            authenticated: false,
        }
    }

    /// Adds final values from active operation security headers to redaction.
    #[must_use]
    pub fn with_active_operation_headers(
        mut self,
        spec: &CachedSpec,
        operation: &crate::cache::models::CachedCommand,
        headers: &reqwest::header::HeaderMap,
    ) -> Self {
        for (name, value) in headers {
            if !value.is_sensitive()
                && !should_redact_operation_header(name.as_str(), spec, operation)
            {
                continue;
            }
            // Sensitivity does not depend on UTF-8 decoding or credential length.
            self.authenticated = true;
            let Ok(value) = std::str::from_utf8(value.as_bytes()) else {
                continue;
            };
            self.add_active_credential(value);
            if name == reqwest::header::AUTHORIZATION {
                self.add_authorization_forms(value);
            }
        }
        self.active_secrets.sort();
        self.active_secrets.dedup();
        self
    }

    /// Includes Basic credentials that reqwest derives implicitly from URL userinfo.
    #[must_use]
    pub fn with_request_url(mut self, url: &str) -> Self {
        let Ok(url) = reqwest::Url::parse(url) else {
            return self;
        };
        if url.username().is_empty() && url.password().is_none() {
            return self;
        }
        self.authenticated = true;
        self.add_active_credential(url.username());
        self.add_active_credential(url.password().unwrap_or_default());
        let username =
            urlencoding::decode(url.username()).unwrap_or_else(|_| url.username().into());
        let password = url.password().unwrap_or_default();
        let password = urlencoding::decode(password).unwrap_or_else(|_| password.into());
        let pair = format!("{username}:{password}");
        let authorization = format!("Basic {}", STANDARD.encode(pair));
        self.add_active_credential(&authorization);
        self.add_authorization_forms(&authorization);
        self
    }

    /// Includes raw and encoded Basic credentials from a selected proxy URL.
    /// Scheme-less authorities follow reqwest's local URL normalization.
    #[must_use]
    pub fn with_proxy_url(self, url: &str) -> Self {
        let parsed = reqwest::Url::parse(url)
            .ok()
            .filter(reqwest::Url::has_host)
            .or_else(|| reqwest::Url::parse(&format!("http://{url}")).ok());
        match parsed {
            Some(url) => self.with_request_url(url.as_str()),
            None => self,
        }
    }

    /// Includes explicit selected config-proxy Basic authentication, without persisting it.
    #[must_use]
    pub fn with_proxy_basic_auth(mut self, username: &str, password: &str) -> Self {
        self.authenticated = true;
        self.add_active_credential(username);
        let authorization = format!(
            "Basic {}",
            STANDARD.encode(format!("{username}:{password}"))
        );
        self.add_active_credential(&authorization);
        self.add_authorization_forms(&authorization);
        self
    }

    /// Marks transport authentication that is attached outside the operation headers.
    /// This is conservative for authenticated proxies excluded by `NO_PROXY`.
    #[must_use]
    pub const fn with_authenticated_transport(mut self, authenticated: bool) -> Self {
        self.authenticated |= authenticated;
        self
    }

    fn add_active_credential(&mut self, value: &str) {
        if !value.is_empty() {
            self.active_secrets.push(value.to_string());
        }
    }

    fn add_authorization_forms(&mut self, value: &str) {
        // Header grammar allows HTAB and repeated spaces. This is diagnostic
        // extraction only: it never normalizes or rejects the outgoing value.
        let Some((scheme, credential)) = value.split_once([' ', '\t']) else {
            return;
        };
        let credential = credential.trim_matches([' ', '\t']);
        self.add_active_credential(credential);
        if !scheme.eq_ignore_ascii_case("basic") {
            return;
        }
        let Ok(pair) = STANDARD.decode(credential) else {
            return;
        };
        self.add_utf8_credential(&pair);
        if let Some(colon) = pair.iter().position(|byte| *byte == b':') {
            // Invalid username bytes must not hide a compatible password.
            self.add_utf8_credential(&pair[colon + 1..]);
        }
    }

    fn add_utf8_credential(&mut self, bytes: &[u8]) {
        if let Ok(value) = std::str::from_utf8(bytes) {
            self.add_active_credential(value);
        }
    }

    /// Authenticated outgoing metadata may contain transformed credentials or
    /// non-UTF8 bytes. Omission avoids a false guarantee from literal/lossy matching.
    #[must_use]
    pub fn diagnostic_url(
        &self,
        url: &str,
        operation_context: Option<(&CachedSpec, &crate::cache::models::CachedCommand)>,
    ) -> String {
        if self.is_authenticated() {
            "<authenticated request URL omitted>".to_string()
        } else {
            self.redact_secrets_in_text(&redact_operation_url(url, operation_context))
        }
    }

    /// Whether active operation or transport credentials make body diagnostics unsafe.
    /// Configured but unused secrets alone do not make an anonymous request authenticated.
    #[must_use]
    pub const fn is_authenticated(&self) -> bool {
        self.authenticated
    }

    /// Renders an error diagnostic, never an authenticated server-controlled body.
    /// Literal redaction is only a best-effort aid for anonymous diagnostics.
    #[must_use]
    pub fn diagnostic_body(&self, body: &str) -> String {
        if self.is_authenticated() {
            "<authenticated response body omitted>".to_string()
        } else {
            self.redact_secrets_in_text(body)
        }
    }

    /// Checks if a value exactly matches any configured or active secret.
    #[must_use]
    pub fn is_secret(&self, value: &str) -> bool {
        self.secrets
            .iter()
            .chain(&self.active_secrets)
            .any(|secret| secret == value)
    }

    /// Redacts all occurrences of secrets in the given text.
    ///
    /// Configured values use `MIN_SECRET_LENGTH_FOR_BODY_REDACTION` to limit false
    /// positives. Final active credential forms are redacted regardless of length.
    /// This cannot recognize arbitrary transformations; use `diagnostic_body`
    /// for untrusted error bodies.
    #[must_use]
    pub fn redact_secrets_in_text(&self, text: &str) -> String {
        let mut result = text.to_string();
        for secret in &self.secrets {
            if secret.len() >= MIN_SECRET_LENGTH_FOR_BODY_REDACTION {
                result = result.replace(secret, "[REDACTED]");
            }
        }
        for secret in &self.active_secrets {
            result = result.replace(secret, "[REDACTED]");
        }
        result
    }

    /// Returns true if this context has any secrets to redact.
    #[must_use]
    pub const fn has_secrets(&self) -> bool {
        !self.secrets.is_empty() || !self.active_secrets.is_empty()
    }
}

/// Returns the canonical status text for an HTTP status code
#[must_use]
fn http_status_text(status: u16) -> &'static str {
    reqwest::StatusCode::from_u16(status)
        .ok()
        .and_then(|status_code| status_code.canonical_reason())
        .unwrap_or("")
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

/// Extends standard redaction with custom API-key header names declared by an operation.
#[must_use]
pub fn should_redact_operation_header(
    header_name: &str,
    spec: &CachedSpec,
    operation: &crate::cache::models::CachedCommand,
) -> bool {
    should_redact_header(header_name)
        || operation
            .security_requirements
            .iter()
            .flatten()
            .any(|scheme_name| {
                spec.security_schemes
                    .get(scheme_name)
                    .is_some_and(|scheme| {
                        scheme.scheme_type == crate::constants::AUTH_SCHEME_APIKEY
                            && scheme.location.as_deref() == Some(crate::constants::LOCATION_HEADER)
                            && scheme
                                .parameter_name
                                .as_deref()
                                .is_some_and(|name| name.eq_ignore_ascii_case(header_name))
                    })
            })
}

/// Conservative pre-resolution policy; no environment values are read here.
pub(crate) fn custom_headers_reference_environment(headers: &[String]) -> bool {
    headers.iter().any(|header| {
        header
            .split_once(':')
            .is_some_and(|(_, value)| value.contains("${"))
    })
}

/// Conservative declared/recognized-header preparation policy, without resolving
/// environment values. Unused configured mappings alone do not imply sensitivity.
pub(crate) fn operation_preparation_is_sensitive<'a>(
    spec: &CachedSpec,
    operation: &crate::cache::models::CachedCommand,
    mut header_names: impl Iterator<Item = &'a str>,
) -> bool {
    operation
        .security_requirements
        .iter()
        .any(|group| !group.is_empty())
        || header_names.any(|name| should_redact_operation_header(name, spec, operation))
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
    redact_operation_url(url, None)
}

fn redact_url_userinfo(url: &str) -> String {
    let Some(start) = url.find("://").map(|offset| offset + 3) else {
        return url.to_string();
    };
    let end = url[start..]
        .find(['/', '?', '#'])
        .map_or(url.len(), |offset| start + offset);
    let Some(at) = url[start..end].rfind('@').map(|offset| start + offset) else {
        return url.to_string();
    };
    format!("{}[REDACTED]{}", &url[..start], &url[at..])
}

fn sensitive_operation_query(
    name: &str,
    operation_context: Option<(&CachedSpec, &crate::cache::models::CachedCommand)>,
) -> bool {
    should_redact_query_param(name)
        || operation_context.is_some_and(|(spec, operation)| {
            operation
                .security_requirements
                .iter()
                .flatten()
                .any(|scheme_name| {
                    spec.security_schemes
                        .get(scheme_name)
                        .is_some_and(|scheme| {
                            scheme.scheme_type == crate::constants::AUTH_SCHEME_APIKEY
                                && scheme.location.as_deref() == Some("query")
                                && scheme.parameter_name.as_deref() == Some(name)
                        })
                })
        })
}

/// Redacts URL credentials and declared operation query security parameters.
#[must_use]
pub fn redact_operation_url(
    url: &str,
    operation_context: Option<(&CachedSpec, &crate::cache::models::CachedCommand)>,
) -> String {
    let sanitized = redact_url_userinfo(url);
    let url = sanitized.as_str();
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
                    let decoded_name = urlencoding::decode(name).unwrap_or_else(|_| name.into());
                    if sensitive_operation_query(&decoded_name, operation_context) {
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
    log_request_with_operation(method, url, headers, body, secret_ctx, None);
}

/// Logs a request while redacting active operation-specific security header names.
pub fn log_operation_request(
    method: &str,
    url: &str,
    headers: Option<&reqwest::header::HeaderMap>,
    body: Option<&str>,
    secret_ctx: Option<&SecretContext>,
    spec: &CachedSpec,
    operation: &crate::cache::models::CachedCommand,
) {
    log_request_with_operation(
        method,
        url,
        headers,
        body,
        secret_ctx,
        Some((spec, operation)),
    );
}

/// Redacts standard, declared, and final credential forms from a header value.
#[must_use]
pub fn redact_operation_header_value(
    header_name: &str,
    value: &str,
    secret_ctx: Option<&SecretContext>,
    operation_context: Option<(&CachedSpec, &crate::cache::models::CachedCommand)>,
) -> String {
    if operation_context.is_some_and(|(spec, operation)| {
        should_redact_operation_header(header_name, spec, operation)
    }) {
        "[REDACTED]".to_string()
    } else {
        redact_header_value(header_name, value, secret_ctx)
    }
}

fn log_request_with_operation(
    method: &str,
    url: &str,
    headers: Option<&reqwest::header::HeaderMap>,
    body: Option<&str>,
    secret_ctx: Option<&SecretContext>,
    operation_context: Option<(&CachedSpec, &crate::cache::models::CachedCommand)>,
) {
    // Redact sensitive query parameters from URL before logging
    let redacted_url = if tracing::enabled!(target: "aperture::executor", tracing::Level::INFO) {
        secret_ctx.map_or_else(
            || redact_operation_url(url, operation_context),
            |ctx| ctx.diagnostic_url(url, operation_context),
        )
    } else {
        String::new()
    };

    // Log at info level: method, URL, and duration (duration added by caller)
    info!(
        target: "aperture::executor",
        "→ {} {}",
        method.to_uppercase(),
        redacted_url
    );

    // Names as well as values are caller-controlled. Never render authenticated
    // metadata, even when a credential cannot be represented as UTF-8.
    if secret_ctx.is_some_and(SecretContext::is_authenticated) {
        return;
    }

    // Log headers at debug level
    let Some(header_map) =
        headers.filter(|_| tracing::enabled!(target: "aperture::executor", tracing::Level::DEBUG))
    else {
        log_request_body(body, secret_ctx);
        return;
    };

    debug!(
        target: "aperture::executor",
        "Request headers:"
    );
    for (name, value) in header_map {
        let header_str = name.as_str();
        let raw_value = String::from_utf8_lossy(value.as_bytes()).to_string();
        let display_value =
            redact_operation_header_value(header_str, &raw_value, secret_ctx, operation_context);
        debug!(
            target: "aperture::executor",
            "  {}: {}",
            header_str,
            display_value
        );
    }

    log_request_body(body, secret_ctx);
}

fn log_request_body(body: Option<&str>, secret_ctx: Option<&SecretContext>) {
    if !tracing::enabled!(target: "aperture::executor", tracing::Level::TRACE) {
        return;
    }
    if secret_ctx.is_some_and(SecretContext::is_authenticated) {
        return;
    }
    let Some(body_content) = body else {
        return;
    };
    let redacted_body = secret_ctx.map_or_else(
        || body_content.to_string(),
        |ctx| ctx.redact_secrets_in_text(body_content),
    );
    trace!(target: "aperture::executor", "Request body: {}", redacted_body);
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

    secret_ctx.map_or_else(
        || value.to_string(),
        |ctx| ctx.redact_secrets_in_text(value),
    )
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
    log_response_with_operation(
        status,
        duration_ms,
        headers,
        body,
        max_body_len,
        secret_ctx,
        None,
    );
}

/// Logs a response while redacting active operation-specific security headers.
pub fn log_operation_response(
    status: u16,
    duration_ms: u128,
    headers: Option<&reqwest::header::HeaderMap>,
    body: Option<&str>,
    max_body_len: usize,
    secret_ctx: Option<&SecretContext>,
    operation_context: (&CachedSpec, &crate::cache::models::CachedCommand),
) {
    log_response_with_operation(
        status,
        duration_ms,
        headers,
        body,
        max_body_len,
        secret_ctx,
        Some(operation_context),
    );
}

#[allow(clippy::too_many_arguments)]
fn log_response_with_operation(
    status: u16,
    duration_ms: u128,
    headers: Option<&reqwest::header::HeaderMap>,
    body: Option<&str>,
    max_body_len: usize,
    secret_ctx: Option<&SecretContext>,
    operation_context: Option<(&CachedSpec, &crate::cache::models::CachedCommand)>,
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

    // Header names and values are server-controlled and can encode credentials
    // using arbitrary transformations. Only locally derived metadata is safe.
    if secret_ctx.is_some_and(SecretContext::is_authenticated) {
        debug!(target: "aperture::executor", "Response headers omitted (authenticated request)");
        log_response_body(body, max_body_len, secret_ctx);
        return;
    }

    // Log anonymous headers at debug level.
    let Some(header_map) =
        headers.filter(|_| tracing::enabled!(target: "aperture::executor", tracing::Level::DEBUG))
    else {
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
        let display_value =
            redact_operation_header_value(header_str, &raw_value, secret_ctx, operation_context);
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

/// Truncates a string to at most `max_chars` characters, ensuring we don't
/// split in the middle of a multi-byte UTF-8 character.
fn truncate_string(s: &str, max_chars: usize) -> &str {
    match s.char_indices().nth(max_chars) {
        Some((byte_idx, _)) => &s[..byte_idx],
        None => s, // String is shorter than max_chars
    }
}

/// Helper function to log response body with truncation
fn log_response_body(body: Option<&str>, max_body_len: usize, secret_ctx: Option<&SecretContext>) {
    if !tracing::enabled!(target: "aperture::executor", tracing::Level::TRACE) {
        return;
    }
    if secret_ctx.is_some_and(SecretContext::is_authenticated) {
        return;
    }
    let Some(body_content) = body else {
        return;
    };

    // Redact secrets in body before logging
    let redacted_body = secret_ctx.map_or_else(
        || body_content.to_string(),
        |ctx| ctx.redact_secrets_in_text(body_content),
    );

    // Check character count, not byte length, for truncation
    let char_count = redacted_body.chars().count();
    if char_count > max_body_len {
        let truncated = truncate_string(&redacted_body, max_body_len);
        trace!(
            target: "aperture::executor",
            "Response body: {} (truncated at {} chars)",
            truncated,
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
    fn adversarial_url_redaction_handles_userinfo_encoded_and_duplicate_names() {
        for url in [
            "https://alice:secret@example.com/items",
            "https://example.com/items?%74oken=secret&token=other&extra=public",
            "https://example.com/items?TOKEN=secret&token=other",
        ] {
            let redacted = redact_url_query_params(url);
            assert!(!redacted.contains("secret"));
            assert!(!redacted.contains("other"));
        }
        for url in [
            "",
            " ",
            "none",
            "https://example.com/?%ZZ=public",
            "https://example.com/?extra=public&extra=none",
        ] {
            assert_eq!(redact_url_query_params(url), url);
        }
    }

    #[test]
    fn custom_security_query_names_are_redacted() {
        let spec: CachedSpec = serde_json::from_value(serde_json::json!({
            "cache_format_version": crate::cache::models::CACHE_FORMAT_VERSION,
            "name": "test", "version": "1", "base_url": "https://example.com", "servers": [],
            "security_schemes": {
                "tenant": { "name": "tenant", "scheme_type": "apiKey", "location": "query", "parameter_name": "tenant-secret" },
                "secondary": { "name": "secondary", "scheme_type": "apiKey", "location": "query", "parameter_name": "secondary-secret" }
            },
            "commands": [{ "name": "items", "operation_id": "items", "method": "GET", "path": "/items", "parameters": [], "responses": [], "security_requirements": [[], ["tenant", "secondary"]], "tags": [], "deprecated": false, "examples": [], "aliases": [], "hidden": false }]
        })).unwrap();
        let redacted = redact_operation_url(
            "https://example.com/?tenant%2Dsecret=secret&secondary-secret=other-credential&extra=public",
            Some((&spec, &spec.commands[0])),
        );
        assert!(!redacted.contains("=secret"));
        assert!(!redacted.contains("other-credential"));
        assert!(redacted.contains("extra=public"));
    }

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

    #[test]
    fn test_truncate_string_ascii() {
        let text = "Hello, World!";
        assert_eq!(truncate_string(text, 5), "Hello");
        assert_eq!(truncate_string(text, 100), "Hello, World!");
        assert_eq!(truncate_string(text, 0), "");
    }

    #[test]
    fn test_truncate_string_unicode() {
        // Japanese text: "こんにちは世界" (7 characters, 21 bytes)
        let text = "こんにちは世界";
        assert_eq!(truncate_string(text, 3), "こんに");
        assert_eq!(truncate_string(text, 7), "こんにちは世界");
        assert_eq!(truncate_string(text, 100), "こんにちは世界");
    }

    #[test]
    fn test_truncate_string_four_byte_unicode() {
        // Deseret letters occupy four UTF-8 bytes but each counts as one character.
        let text = "Hello 𐐀𐐨!";
        assert_eq!(truncate_string(text, 6), "Hello ");
        assert_eq!(truncate_string(text, 7), "Hello 𐐀");
        assert_eq!(truncate_string(text, 8), "Hello 𐐀𐐨");
    }
}
