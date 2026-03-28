use crate::cache::models::{CachedCommand, CachedSecurityScheme, CachedSpec};
use crate::config::models::GlobalConfig;
use crate::config::url_resolver::BaseUrlResolver;
use crate::constants;
use crate::error::Error;
use crate::invocation::ExecutionResult;
use crate::logging;
use crate::resilience::{
    calculate_retry_delay_with_header, is_retryable_status, parse_retry_after_value, RetryConfig,
};
use crate::response_cache::{
    is_auth_header, scrub_auth_headers, CacheConfig, CacheKey, CachedRequestInfo, CachedResponse,
    ResponseCache,
};
use crate::utils::to_kebab_case;
use base64::{engine::general_purpose, Engine as _};
use reqwest::header::{HeaderMap, HeaderName, HeaderValue};
use reqwest::Method;
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::str::FromStr;
use tokio::time::sleep;

#[cfg(feature = "jq")]
use jaq_core::{Ctx, RcIter};
#[cfg(feature = "jq")]
use jaq_json::Val;

/// Represents supported authentication schemes
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AuthScheme {
    Bearer,
    Basic,
    Token,
    DSN,
    ApiKey,
    Custom(String),
}

impl From<&str> for AuthScheme {
    fn from(s: &str) -> Self {
        match s.to_lowercase().as_str() {
            constants::AUTH_SCHEME_BEARER => Self::Bearer,
            constants::AUTH_SCHEME_BASIC => Self::Basic,
            "token" => Self::Token,
            "dsn" => Self::DSN,
            constants::AUTH_SCHEME_APIKEY => Self::ApiKey,
            _ => Self::Custom(s.to_string()),
        }
    }
}

/// Configuration for request retry behavior.
#[derive(Debug, Clone)]
pub struct RetryContext {
    /// Maximum number of retry attempts (0 = disabled)
    pub max_attempts: u32,
    /// Initial delay between retries in milliseconds
    pub initial_delay_ms: u64,
    /// Maximum delay cap in milliseconds
    pub max_delay_ms: u64,
    /// Whether to force retry on non-idempotent requests without idempotency key
    pub force_retry: bool,
    /// HTTP method (used to check idempotency)
    pub method: Option<String>,
    /// Whether an idempotency key is set
    pub has_idempotency_key: bool,
}

impl Default for RetryContext {
    fn default() -> Self {
        Self {
            max_attempts: 0, // Disabled by default
            initial_delay_ms: 500,
            max_delay_ms: 30_000,
            force_retry: false,
            method: None,
            has_idempotency_key: false,
        }
    }
}

impl RetryContext {
    /// Returns true if retries are enabled.
    #[must_use]
    pub const fn is_enabled(&self) -> bool {
        self.max_attempts > 0
    }

    /// Returns true if the request method is safe to retry (idempotent or has key).
    #[must_use]
    pub fn is_safe_to_retry(&self) -> bool {
        if self.force_retry || self.has_idempotency_key {
            return true;
        }

        // GET, HEAD, PUT, OPTIONS, TRACE are idempotent per HTTP semantics
        self.method.as_ref().is_some_and(|m| {
            matches!(
                m.to_uppercase().as_str(),
                "GET" | "HEAD" | "PUT" | "OPTIONS" | "TRACE"
            )
        })
    }
}

// Helper functions

/// Build HTTP client with default timeout
fn build_http_client() -> Result<reqwest::Client, Error> {
    reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(30))
        .build()
        .map_err(|e| {
            Error::request_failed(
                reqwest::StatusCode::INTERNAL_SERVER_ERROR,
                format!("Failed to create HTTP client: {e}"),
            )
        })
}

/// Send HTTP request and get response
async fn send_request(
    request: reqwest::RequestBuilder,
    secret_ctx: Option<&logging::SecretContext>,
) -> Result<(reqwest::StatusCode, HashMap<String, String>, String), Error> {
    let start_time = std::time::Instant::now();

    let response = request
        .send()
        .await
        .map_err(|e| Error::network_request_failed(e.to_string()))?;

    let status = response.status();
    let duration_ms = start_time.elapsed().as_millis();

    // Copy headers before consuming response
    let mut response_headers_map = reqwest::header::HeaderMap::new();
    for (name, value) in response.headers() {
        response_headers_map.insert(name.clone(), value.clone());
    }

    let response_headers: HashMap<String, String> = response
        .headers()
        .iter()
        .map(|(k, v)| (k.as_str().to_string(), v.to_str().unwrap_or("").to_string()))
        .collect();

    let response_text = response
        .text()
        .await
        .map_err(|e| Error::response_read_error(e.to_string()))?;

    // Log response with secret redaction
    logging::log_response(
        status.as_u16(),
        duration_ms,
        Some(&response_headers_map),
        Some(&response_text),
        logging::get_max_body_len(),
        secret_ctx,
    );

    Ok((status, response_headers, response_text))
}

/// Send HTTP request with retry logic
#[allow(clippy::too_many_arguments)]
#[allow(clippy::too_many_lines)]
async fn send_request_with_retry(
    client: &reqwest::Client,
    method: Method,
    url: &str,
    headers: HeaderMap,
    body: Option<String>,
    retry_context: Option<&RetryContext>,
    operation: &CachedCommand,
    secret_ctx: Option<&logging::SecretContext>,
) -> Result<(reqwest::StatusCode, HashMap<String, String>, String), Error> {
    use crate::resilience::RetryConfig;

    logging::log_request(
        method.as_str(),
        url,
        Some(&headers),
        body.as_deref(),
        secret_ctx,
    );

    let Some(ctx) = retry_context.filter(|ctx| ctx.is_enabled()) else {
        return send_request_once(client, method, url, headers, body, secret_ctx).await;
    };

    if !ctx.is_safe_to_retry() {
        tracing::warn!(
            method = %method,
            operation_id = %operation.operation_id,
            "Retries disabled - method is not idempotent and no idempotency key provided. \
             Use --force-retry or provide --idempotency-key"
        );
        return send_request_once(client, method.clone(), url, headers, body, secret_ctx).await;
    }

    let retry_config = RetryConfig {
        max_attempts: ctx.max_attempts as usize,
        initial_delay_ms: ctx.initial_delay_ms,
        max_delay_ms: ctx.max_delay_ms,
        backoff_multiplier: 2.0,
        jitter: true,
    };

    retry_request_with_backoff(
        client,
        method,
        url,
        headers,
        body,
        ctx,
        &retry_config,
        operation,
        secret_ctx,
    )
    .await
}

#[allow(clippy::too_many_arguments)]
async fn retry_request_with_backoff(
    client: &reqwest::Client,
    method: Method,
    url: &str,
    headers: HeaderMap,
    body: Option<String>,
    ctx: &RetryContext,
    retry_config: &crate::resilience::RetryConfig,
    operation: &CachedCommand,
    secret_ctx: Option<&logging::SecretContext>,
) -> Result<(reqwest::StatusCode, HashMap<String, String>, String), Error> {
    let max_attempts = ctx.max_attempts;
    let mut attempt: u32 = 0;
    let mut last_error: Option<Error> = None;
    let mut last_status: Option<reqwest::StatusCode> = None;
    let mut last_response_headers: Option<HashMap<String, String>> = None;
    let mut last_response_text: Option<String> = None;

    while attempt < max_attempts {
        attempt += 1;

        let request = build_request(client, method.clone(), url, headers.clone(), body.clone());
        match send_request(request, secret_ctx).await {
            Ok((status, response_headers, response_text)) => {
                match handle_retryable_http_response(
                    retry_config,
                    attempt,
                    max_attempts,
                    &method,
                    operation,
                    status,
                    response_headers,
                    response_text,
                )
                .await
                {
                    RetryableHttpResponse::Return(result) => return Ok(result),
                    RetryableHttpResponse::Retry {
                        status,
                        response_headers,
                        response_text,
                    } => {
                        last_status = Some(status);
                        last_response_headers = Some(response_headers);
                        last_response_text = Some(response_text);
                    }
                }
            }
            Err(error) => match handle_retryable_network_error(
                retry_config,
                attempt,
                max_attempts,
                &method,
                operation,
                error,
            )
            .await
            {
                RetryableNetworkError::Return(error) => return Err(error),
                RetryableNetworkError::Retry(error) => {
                    last_error = Some(error);
                }
            },
        }
    }

    finish_retry_result(
        max_attempts,
        attempt,
        last_status,
        last_response_headers,
        last_response_text,
        last_error,
        ctx,
        &method,
        operation,
    )
}

#[allow(clippy::too_many_arguments)]
fn finish_retry_result(
    max_attempts: u32,
    attempt: u32,
    last_status: Option<reqwest::StatusCode>,
    last_response_headers: Option<HashMap<String, String>>,
    last_response_text: Option<String>,
    last_error: Option<Error>,
    ctx: &RetryContext,
    method: &Method,
    operation: &CachedCommand,
) -> Result<(reqwest::StatusCode, HashMap<String, String>, String), Error> {
    if let (Some(status), Some(headers), Some(text)) =
        (last_status, last_response_headers, last_response_text)
    {
        tracing::warn!(
            method = %method,
            operation_id = %operation.operation_id,
            max_attempts,
            "Retry exhausted"
        );
        return Ok((status, headers, text));
    }

    if let Some(error) = last_error {
        tracing::warn!(
            method = %method,
            operation_id = %operation.operation_id,
            max_attempts,
            "Retry exhausted"
        );
        return Err(Error::retry_limit_exceeded_detailed(
            max_attempts,
            attempt,
            error.to_string(),
            ctx.initial_delay_ms,
            ctx.max_delay_ms,
            None,
            &operation.operation_id,
        ));
    }

    Err(Error::retry_limit_exceeded_detailed(
        max_attempts,
        attempt,
        "Request failed with no response",
        ctx.initial_delay_ms,
        ctx.max_delay_ms,
        None,
        &operation.operation_id,
    ))
}

enum RetryableHttpResponse {
    Return((reqwest::StatusCode, HashMap<String, String>, String)),
    Retry {
        status: reqwest::StatusCode,
        response_headers: HashMap<String, String>,
        response_text: String,
    },
}

enum RetryableNetworkError {
    Return(Error),
    Retry(Error),
}

#[allow(clippy::too_many_arguments)]
async fn handle_retryable_http_response(
    retry_config: &RetryConfig,
    attempt: u32,
    max_attempts: u32,
    method: &Method,
    operation: &CachedCommand,
    status: reqwest::StatusCode,
    response_headers: HashMap<String, String>,
    response_text: String,
) -> RetryableHttpResponse {
    if status.is_success() {
        return RetryableHttpResponse::Return((status, response_headers, response_text));
    }

    if !is_retryable_status(status.as_u16()) {
        return RetryableHttpResponse::Return((status, response_headers, response_text));
    }

    let retry_after = response_headers
        .get("retry-after")
        .and_then(|value| parse_retry_after_value(value));
    let delay =
        calculate_retry_delay_with_header(retry_config, (attempt - 1) as usize, retry_after);

    if attempt < max_attempts {
        tracing::warn!(
            attempt,
            max_attempts,
            method = %method,
            operation_id = %operation.operation_id,
            status = status.as_u16(),
            delay_ms = delay.as_millis(),
            "Retrying after HTTP error"
        );
        sleep(delay).await;
    }

    RetryableHttpResponse::Retry {
        status,
        response_headers,
        response_text,
    }
}

async fn handle_retryable_network_error(
    retry_config: &RetryConfig,
    attempt: u32,
    max_attempts: u32,
    method: &Method,
    operation: &CachedCommand,
    error: Error,
) -> RetryableNetworkError {
    if !matches!(&error, Error::Network(_)) {
        return RetryableNetworkError::Return(error);
    }

    let delay = calculate_retry_delay_with_header(retry_config, (attempt - 1) as usize, None);

    if attempt < max_attempts {
        tracing::warn!(
            attempt,
            max_attempts,
            method = %method,
            operation_id = %operation.operation_id,
            delay_ms = delay.as_millis(),
            error = %error,
            "Retrying after network error"
        );
        sleep(delay).await;
    }

    RetryableNetworkError::Retry(error)
}

/// Build a request from components
fn build_request(
    client: &reqwest::Client,
    method: Method,
    url: &str,
    headers: HeaderMap,
    body: Option<String>,
) -> reqwest::RequestBuilder {
    let mut request = client.request(method, url).headers(headers);
    if let Some(json_body) = body.and_then(|s| serde_json::from_str::<Value>(&s).ok()) {
        request = request.json(&json_body);
    }
    request
}

async fn send_request_once(
    client: &reqwest::Client,
    method: Method,
    url: &str,
    headers: HeaderMap,
    body: Option<String>,
    secret_ctx: Option<&logging::SecretContext>,
) -> Result<(reqwest::StatusCode, HashMap<String, String>, String), Error> {
    let request = build_request(client, method, url, headers, body);
    send_request(request, secret_ctx).await
}

/// Handle HTTP error responses
fn handle_http_error(
    status: reqwest::StatusCode,
    response_text: String,
    spec: &CachedSpec,
    operation: &CachedCommand,
) -> Error {
    let api_name = spec.name.clone();
    let operation_id = Some(operation.operation_id.clone());

    let security_schemes: Vec<String> = operation
        .security_requirements
        .iter()
        .filter_map(|scheme_name| {
            spec.security_schemes
                .get(scheme_name)
                .and_then(|scheme| scheme.aperture_secret.as_ref())
                .map(|aperture_secret| aperture_secret.name.clone())
        })
        .collect();

    Error::http_error_with_context(
        status.as_u16(),
        if response_text.is_empty() {
            constants::EMPTY_RESPONSE.to_string()
        } else {
            response_text
        },
        api_name,
        operation_id,
        &security_schemes,
    )
}

/// Prepare cache context if caching is enabled
fn prepare_cache_context(
    cache_config: Option<&CacheConfig>,
    spec_name: &str,
    operation_id: &str,
    method: &reqwest::Method,
    url: &str,
    headers: &reqwest::header::HeaderMap,
    body: Option<&str>,
) -> Result<Option<(CacheKey, ResponseCache)>, Error> {
    let Some(cache_cfg) = cache_config else {
        return Ok(None);
    };

    if !cache_cfg.enabled {
        return Ok(None);
    }

    // Skip caching for authenticated requests unless explicitly allowed
    let has_auth_headers = headers.iter().any(|(k, _)| is_auth_header(k.as_str()));
    if has_auth_headers && !cache_cfg.allow_authenticated {
        return Ok(None);
    }

    let header_map: HashMap<String, String> = headers
        .iter()
        .map(|(k, v)| (k.as_str().to_string(), v.to_str().unwrap_or("").to_string()))
        .collect();

    let cache_key = CacheKey::from_request(
        spec_name,
        operation_id,
        method.as_ref(),
        url,
        &header_map,
        body,
    )?;

    let response_cache = ResponseCache::new(cache_cfg.clone())?;
    Ok(Some((cache_key, response_cache)))
}

/// Check cache for existing response
async fn check_cache(
    cache_context: Option<&(CacheKey, ResponseCache)>,
) -> Result<Option<CachedResponse>, Error> {
    if let Some((cache_key, response_cache)) = cache_context {
        response_cache.get(cache_key).await
    } else {
        Ok(None)
    }
}

/// Store response in cache
#[allow(clippy::too_many_arguments)]
async fn store_in_cache(
    cache_context: Option<(CacheKey, ResponseCache)>,
    response_text: &str,
    status: reqwest::StatusCode,
    response_headers: &HashMap<String, String>,
    method: reqwest::Method,
    url: String,
    headers: &reqwest::header::HeaderMap,
    body: Option<&str>,
    cache_config: Option<&CacheConfig>,
) -> Result<(), Error> {
    let Some((cache_key, response_cache)) = cache_context else {
        return Ok(());
    };

    // Convert headers to HashMap and scrub auth headers before caching
    let raw_headers: HashMap<String, String> = headers
        .iter()
        .map(|(k, v)| (k.as_str().to_string(), v.to_str().unwrap_or("").to_string()))
        .collect();
    let scrubbed_headers = scrub_auth_headers(&raw_headers);

    let cached_request_info = CachedRequestInfo {
        method: method.to_string(),
        url,
        headers: scrubbed_headers,
        body_hash: body.map(|b| {
            let mut hasher = Sha256::new();
            hasher.update(b.as_bytes());
            format!("{:x}", hasher.finalize())
        }),
    };

    let cache_ttl = cache_config.and_then(|cfg| {
        if cfg.default_ttl.as_secs() > 0 {
            Some(cfg.default_ttl)
        } else {
            None
        }
    });

    response_cache
        .store(
            &cache_key,
            response_text,
            status.as_u16(),
            response_headers,
            cached_request_info,
            cache_ttl,
        )
        .await?;

    Ok(())
}

/// Legacy compatibility wrapper retained for existing tests and callers.
///
/// The implementation lives in the CLI layer to keep this engine module free
/// of direct clap/rendering dependencies.
pub use crate::cli::legacy_execute::execute_request;

/// Validates that a header value doesn't contain control characters
fn validate_header_value(name: &str, value: &str) -> Result<(), Error> {
    if value.chars().any(|c| c == '\r' || c == '\n' || c == '\0') {
        return Err(Error::invalid_header_value(
            name,
            "Header value contains invalid control characters (newline, carriage return, or null)",
        ));
    }
    Ok(())
}

/// Parses a custom header string in the format "Name: Value" or "Name:Value"
fn parse_custom_header(header_str: &str) -> Result<(String, String), Error> {
    // Find the colon separator
    let colon_pos = header_str
        .find(':')
        .ok_or_else(|| Error::invalid_header_format(header_str))?;

    let name = header_str[..colon_pos].trim();
    let value = header_str[colon_pos + 1..].trim();

    if name.is_empty() {
        return Err(Error::empty_header_name());
    }

    // Support environment variable expansion in header values
    let expanded_value = if value.starts_with("${") && value.ends_with('}') {
        // Extract environment variable name
        let var_name = &value[2..value.len() - 1];
        std::env::var(var_name).unwrap_or_else(|_| value.to_string())
    } else {
        value.to_string()
    };

    // Validate the header value
    validate_header_value(name, &expanded_value)?;

    Ok((name.to_string(), expanded_value))
}

struct ResolvedAuthenticationSecret {
    value: String,
    env_var_name: String,
    source: &'static str,
}

fn resolve_authentication_secret(
    security_scheme: &CachedSecurityScheme,
    api_name: &str,
    global_config: Option<&GlobalConfig>,
) -> Result<Option<ResolvedAuthenticationSecret>, Error> {
    let secret_config = global_config
        .and_then(|config| config.api_configs.get(api_name))
        .and_then(|api_config| api_config.secrets.get(&security_scheme.name));

    match (secret_config, &security_scheme.aperture_secret) {
        (Some(config_secret), _) => {
            let value = std::env::var(&config_secret.name)
                .map_err(|_| Error::secret_not_set(&security_scheme.name, &config_secret.name))?;
            Ok(Some(ResolvedAuthenticationSecret {
                value,
                env_var_name: config_secret.name.clone(),
                source: "config",
            }))
        }
        (None, Some(aperture_secret)) => {
            let value = std::env::var(&aperture_secret.name)
                .map_err(|_| Error::secret_not_set(&security_scheme.name, &aperture_secret.name))?;
            Ok(Some(ResolvedAuthenticationSecret {
                value,
                env_var_name: aperture_secret.name.clone(),
                source: "x-aperture-secret",
            }))
        }
        (None, None) => Ok(None),
    }
}

fn insert_api_key_header(
    headers: &mut HeaderMap,
    security_scheme: &CachedSecurityScheme,
    secret_value: &str,
) -> Result<(), Error> {
    let (Some(location), Some(param_name)) =
        (&security_scheme.location, &security_scheme.parameter_name)
    else {
        return Ok(());
    };

    if location == "header" {
        let header_name = HeaderName::from_str(param_name)
            .map_err(|e| Error::invalid_header_name(param_name, e.to_string()))?;
        let header_value = HeaderValue::from_str(secret_value)
            .map_err(|e| Error::invalid_header_value(param_name, e.to_string()))?;
        headers.insert(header_name, header_value);
    }

    Ok(())
}

fn build_http_authorization_value(scheme_str: &str, secret_value: &str) -> String {
    let auth_scheme: AuthScheme = AuthScheme::from(scheme_str);
    match &auth_scheme {
        AuthScheme::Bearer => format!("Bearer {secret_value}"),
        AuthScheme::Basic => {
            let encoded = general_purpose::STANDARD.encode(secret_value);
            format!("Basic {encoded}")
        }
        AuthScheme::Token | AuthScheme::DSN | AuthScheme::ApiKey | AuthScheme::Custom(_) => {
            format!("{scheme_str} {secret_value}")
        }
    }
}

fn insert_http_authorization_header(
    headers: &mut HeaderMap,
    security_scheme: &CachedSecurityScheme,
    secret_value: &str,
) -> Result<(), Error> {
    let Some(scheme_str) = &security_scheme.scheme else {
        return Ok(());
    };

    let auth_value = build_http_authorization_value(scheme_str, secret_value);
    let header_value = HeaderValue::from_str(&auth_value)
        .map_err(|e| Error::invalid_header_value(constants::HEADER_AUTHORIZATION, e.to_string()))?;
    headers.insert(constants::HEADER_AUTHORIZATION, header_value);

    tracing::debug!(scheme = %scheme_str, "Added HTTP authentication header");
    Ok(())
}

/// Adds an authentication header based on a security scheme
fn add_authentication_header(
    headers: &mut HeaderMap,
    security_scheme: &CachedSecurityScheme,
    api_name: &str,
    global_config: Option<&GlobalConfig>,
) -> Result<(), Error> {
    tracing::debug!(
        scheme_name = %security_scheme.name,
        scheme_type = %security_scheme.scheme_type,
        "Adding authentication header"
    );

    let Some(resolved_secret) =
        resolve_authentication_secret(security_scheme, api_name, global_config)?
    else {
        return Ok(());
    };

    tracing::debug!(
        source = resolved_secret.source,
        scheme_name = %security_scheme.name,
        env_var = %resolved_secret.env_var_name,
        "Resolved secret"
    );

    validate_header_value(constants::HEADER_AUTHORIZATION, &resolved_secret.value)?;

    match security_scheme.scheme_type.as_str() {
        constants::AUTH_SCHEME_APIKEY => {
            insert_api_key_header(headers, security_scheme, &resolved_secret.value)?;
        }
        "http" => {
            insert_http_authorization_header(headers, security_scheme, &resolved_secret.value)?;
        }
        _ => {
            return Err(Error::unsupported_security_scheme(
                &security_scheme.scheme_type,
            ));
        }
    }

    Ok(())
}

// ── New domain-type-based API ───────────────────────────────────────

fn resolve_base_url_resolver<'a>(
    spec: &'a CachedSpec,
    global_config: Option<&'a GlobalConfig>,
) -> BaseUrlResolver<'a> {
    let resolver = BaseUrlResolver::new(spec);
    if let Some(config) = global_config {
        resolver.with_global_config(config)
    } else {
        resolver
    }
}

fn add_idempotency_key(
    headers: &mut HeaderMap,
    idempotency_key: Option<&String>,
) -> Result<(), Error> {
    if let Some(key) = idempotency_key {
        headers.insert(
            HeaderName::from_static("idempotency-key"),
            HeaderValue::from_str(key).map_err(|_| Error::invalid_idempotency_key())?,
        );
    }
    Ok(())
}

async fn cached_execution_result(
    cache_context: Option<&(CacheKey, ResponseCache)>,
) -> Result<Option<ExecutionResult>, Error> {
    if let Some(cached_response) = check_cache(cache_context).await? {
        return Ok(Some(ExecutionResult::Cached {
            body: cached_response.body,
        }));
    }

    Ok(None)
}

fn build_dry_run_result(
    dry_run: bool,
    method: &Method,
    url: &str,
    headers: &HeaderMap,
    body: Option<&str>,
    operation_id: &str,
) -> Option<ExecutionResult> {
    if !dry_run {
        return None;
    }

    let headers_map: HashMap<String, String> = headers
        .iter()
        .map(|(k, v)| {
            let value = if logging::should_redact_header(k.as_str()) {
                "[REDACTED]".to_string()
            } else {
                v.to_str().unwrap_or("<binary>").to_string()
            };
            (k.as_str().to_string(), value)
        })
        .collect();

    let request_info = serde_json::json!({
        "dry_run": true,
        "method": method.to_string(),
        "url": url,
        "headers": headers_map,
        "body": body,
        "operation_id": operation_id
    });

    Some(ExecutionResult::DryRun { request_info })
}

#[allow(clippy::too_many_arguments)]
async fn finalize_execution_result(
    status: reqwest::StatusCode,
    response_headers: HashMap<String, String>,
    response_text: String,
    spec: &CachedSpec,
    operation: &CachedCommand,
    method: Method,
    url: String,
    headers: &HeaderMap,
    body: Option<&str>,
    cache_context: Option<(CacheKey, ResponseCache)>,
    cache_config: Option<&CacheConfig>,
) -> Result<ExecutionResult, Error> {
    if !status.is_success() {
        return Err(handle_http_error(status, response_text, spec, operation));
    }

    store_in_cache(
        cache_context,
        &response_text,
        status,
        &response_headers,
        method,
        url,
        headers,
        body,
        cache_config,
    )
    .await?;

    if response_text.is_empty() {
        Ok(ExecutionResult::Empty)
    } else {
        Ok(ExecutionResult::Success {
            body: response_text,
            status: status.as_u16(),
            headers: response_headers,
        })
    }
}

/// Executes an API operation using CLI-agnostic domain types.
///
/// This is the primary entry point for the execution engine. It accepts
/// pre-extracted parameters in [`OperationCall`] and execution configuration
/// in [`ExecutionContext`], returning a structured [`ExecutionResult`]
/// instead of printing directly.
///
/// # Errors
///
/// Returns errors for authentication failures, network issues, or response
/// validation problems.
async fn resolve_pre_execution_result(
    cache_context: Option<&(CacheKey, ResponseCache)>,
    dry_run: bool,
    method: &Method,
    url: &str,
    headers: &HeaderMap,
    body: Option<&str>,
    operation_id: &str,
) -> Result<Option<ExecutionResult>, Error> {
    if let Some(result) = cached_execution_result(cache_context).await? {
        return Ok(Some(result));
    }

    Ok(build_dry_run_result(
        dry_run,
        method,
        url,
        headers,
        body,
        operation_id,
    ))
}

/// Executes an API operation using CLI-agnostic domain types.
///
/// # Errors
///
/// Returns errors for authentication failures, network issues, invalid
/// parameters, and response validation problems.
#[allow(clippy::too_many_lines)]
pub async fn execute(
    spec: &CachedSpec,
    call: crate::invocation::OperationCall,
    ctx: crate::invocation::ExecutionContext,
) -> Result<crate::invocation::ExecutionResult, Error> {
    let prepared = prepare_execution(spec, call, &ctx)?;

    if let Some(result) = resolve_pre_execution_result(
        prepared.cache_context.as_ref(),
        ctx.dry_run,
        &prepared.method,
        &prepared.url,
        &prepared.headers_clone,
        prepared.body.as_deref(),
        &prepared.operation.operation_id,
    )
    .await?
    {
        return Ok(result);
    }

    let (status, response_headers, response_text) = send_request_with_retry(
        &prepared.client,
        prepared.method.clone(),
        &prepared.url,
        prepared.headers,
        prepared.body.clone(),
        prepared.retry_ctx.as_ref(),
        prepared.operation,
        Some(&prepared.secret_ctx),
    )
    .await?;

    finalize_execution_result(
        status,
        response_headers,
        response_text,
        spec,
        prepared.operation,
        prepared.method,
        prepared.url,
        &prepared.headers_clone,
        prepared.body.as_deref(),
        prepared.cache_context,
        prepared.cache_config,
    )
    .await
}

struct PreparedExecution<'a> {
    operation: &'a CachedCommand,
    method: Method,
    url: String,
    client: reqwest::Client,
    headers: HeaderMap,
    headers_clone: HeaderMap,
    cache_context: Option<(CacheKey, ResponseCache)>,
    retry_ctx: Option<RetryContext>,
    secret_ctx: logging::SecretContext,
    body: Option<String>,
    cache_config: Option<&'a CacheConfig>,
}

fn prepare_execution<'a>(
    spec: &'a CachedSpec,
    call: crate::invocation::OperationCall,
    ctx: &'a crate::invocation::ExecutionContext,
) -> Result<PreparedExecution<'a>, Error> {
    let operation = find_operation_by_id(spec, &call.operation_id)?;
    let resolver = resolve_base_url_resolver(spec, ctx.global_config.as_ref());
    let base_url =
        resolver.resolve_with_variables(ctx.base_url.as_deref(), &ctx.server_var_args)?;
    let url = build_url_from_params(
        &base_url,
        &operation.path,
        &call.path_params,
        &call.query_params,
    )?;
    let client = build_http_client()?;
    let mut headers = build_headers_from_params(
        spec,
        operation,
        &call.header_params,
        &call.custom_headers,
        &spec.name,
        ctx.global_config.as_ref(),
    )?;
    add_idempotency_key(&mut headers, ctx.idempotency_key.as_ref())?;
    let method = Method::from_str(&operation.method)
        .map_err(|_| Error::invalid_http_method(&operation.method))?;
    let headers_clone = headers.clone();
    let cache_context = prepare_cache_context(
        ctx.cache_config.as_ref(),
        &spec.name,
        &operation.operation_id,
        &method,
        &url,
        &headers_clone,
        call.body.as_deref(),
    )?;
    let retry_ctx = ctx.retry_context.clone().map(|mut rc| {
        rc.method = Some(method.to_string());
        rc
    });
    let secret_ctx =
        logging::SecretContext::from_spec_and_config(spec, &spec.name, ctx.global_config.as_ref());

    Ok(PreparedExecution {
        operation,
        method,
        url,
        client,
        headers,
        headers_clone,
        cache_context,
        retry_ctx,
        secret_ctx,
        body: call.body,
        cache_config: ctx.cache_config.as_ref(),
    })
}

/// Finds an operation by its `operation_id` in the spec.
fn find_operation_by_id<'a>(
    spec: &'a CachedSpec,
    operation_id: &str,
) -> Result<&'a CachedCommand, Error> {
    spec.commands
        .iter()
        .find(|cmd| cmd.operation_id == operation_id)
        .ok_or_else(|| {
            let kebab_id = to_kebab_case(operation_id);
            let suggestions = crate::suggestions::suggest_similar_operations(spec, &kebab_id);
            Error::operation_not_found_with_suggestions(operation_id, &suggestions)
        })
}

/// Builds the full URL from pre-extracted path and query parameter maps.
fn build_url_from_params(
    base_url: &str,
    path_template: &str,
    path_params: &HashMap<String, String>,
    query_params: &HashMap<String, String>,
) -> Result<String, Error> {
    let mut url = format!("{}{}", base_url.trim_end_matches('/'), path_template);

    // Substitute path parameters: replace {param} with values from the map
    let mut start = 0;
    while let Some(open) = url[start..].find('{') {
        let open_pos = start + open;
        let Some(close) = url[open_pos..].find('}') else {
            break;
        };
        let close_pos = open_pos + close;
        let param_name = url[open_pos + 1..close_pos].to_string();

        let value = path_params
            .get(&param_name)
            .ok_or_else(|| Error::missing_path_parameter(&param_name))?;

        url.replace_range(open_pos..=close_pos, value);
        start = open_pos + value.len();
    }

    // Append query parameters
    if !query_params.is_empty() {
        let mut qs_pairs: Vec<(&String, &String)> = query_params.iter().collect();
        qs_pairs.sort_by(|(k1, _), (k2, _)| k1.cmp(k2));

        let qs: Vec<String> = qs_pairs
            .into_iter()
            .map(|(k, v)| format!("{}={}", k, urlencoding::encode(v)))
            .collect();

        url.push('?');
        url.push_str(&qs.join("&"));
    }

    Ok(url)
}

/// Builds HTTP headers from pre-extracted header parameter maps.
#[allow(clippy::too_many_arguments)]
fn build_headers_from_params(
    spec: &CachedSpec,
    operation: &CachedCommand,
    header_params: &HashMap<String, String>,
    custom_headers: &[String],
    api_name: &str,
    global_config: Option<&GlobalConfig>,
) -> Result<HeaderMap, Error> {
    let mut headers = default_request_headers();
    apply_header_parameters(&mut headers, header_params)?;
    apply_security_headers(&mut headers, spec, operation, api_name, global_config)?;
    apply_custom_headers(&mut headers, custom_headers)?;
    Ok(headers)
}

fn default_request_headers() -> HeaderMap {
    let mut headers = HeaderMap::new();
    headers.insert("User-Agent", HeaderValue::from_static("aperture/0.1.0"));
    headers.insert(
        constants::HEADER_ACCEPT,
        HeaderValue::from_static(constants::CONTENT_TYPE_JSON),
    );
    headers
}

fn apply_header_parameters(
    headers: &mut HeaderMap,
    header_params: &HashMap<String, String>,
) -> Result<(), Error> {
    for (name, value) in header_params {
        let header_name = HeaderName::from_str(name)
            .map_err(|e| Error::invalid_header_name(name, e.to_string()))?;
        let header_value = HeaderValue::from_str(value)
            .map_err(|e| Error::invalid_header_value(name, e.to_string()))?;
        headers.insert(header_name, header_value);
    }
    Ok(())
}

fn apply_security_headers(
    headers: &mut HeaderMap,
    spec: &CachedSpec,
    operation: &CachedCommand,
    api_name: &str,
    global_config: Option<&GlobalConfig>,
) -> Result<(), Error> {
    for security_scheme_name in &operation.security_requirements {
        let Some(security_scheme) = spec.security_schemes.get(security_scheme_name) else {
            continue;
        };
        add_authentication_header(headers, security_scheme, api_name, global_config)?;
    }
    Ok(())
}

fn apply_custom_headers(headers: &mut HeaderMap, custom_headers: &[String]) -> Result<(), Error> {
    for header_str in custom_headers {
        let (name, value) = parse_custom_header(header_str)?;
        let header_name = HeaderName::from_str(&name)
            .map_err(|e| Error::invalid_header_name(&name, e.to_string()))?;
        let header_value = HeaderValue::from_str(&value)
            .map_err(|e| Error::invalid_header_value(&name, e.to_string()))?;
        headers.insert(header_name, header_value);
    }
    Ok(())
}

/// Applies a JQ filter to the response text
///
/// # Errors
///
/// Returns an error if:
/// - The response text is not valid JSON
/// - The JQ filter expression is invalid
/// - The filter execution fails
pub fn apply_jq_filter(response_text: &str, filter: &str) -> Result<String, Error> {
    let json_value: Value = serde_json::from_str(response_text)
        .map_err(|e| Error::jq_filter_error(filter, format!("Response is not valid JSON: {e}")))?;

    apply_jq_filter_value(json_value, filter)
}

#[cfg(feature = "jq")]
fn apply_jq_filter_value(json_value: Value, filter: &str) -> Result<String, Error> {
    // Use jaq v2.x (pure Rust implementation)
    use jaq_core::load::{Arena, File, Loader};
    use jaq_core::Compiler;

    let program = File {
        code: filter,
        path: (),
    };

    let defs: Vec<_> = jaq_std::defs().chain(jaq_json::defs()).collect();
    let funs: Vec<_> = jaq_std::funs().chain(jaq_json::funs()).collect();

    let loader = Loader::new(defs);
    let arena = Arena::default();

    let modules = match loader.load(&arena, program) {
        Ok(modules) => modules,
        Err(errs) => {
            return Err(Error::jq_filter_error(
                filter,
                format!("Parse error: {errs:?}"),
            ));
        }
    };

    let filter_fn = match Compiler::default().with_funs(funs).compile(modules) {
        Ok(filter) => filter,
        Err(errs) => {
            return Err(Error::jq_filter_error(
                filter,
                format!("Compilation error: {errs:?}"),
            ));
        }
    };

    let jaq_value = Val::from(json_value);
    let inputs = RcIter::new(core::iter::empty());
    let ctx = Ctx::new([], &inputs);
    let output = filter_fn.run((ctx, jaq_value));
    let results: Result<Vec<Val>, _> = output.collect();

    match results {
        Ok(vals) => {
            if vals.is_empty() {
                return Ok(constants::NULL_VALUE.to_string());
            }

            if vals.len() == 1 {
                let json_val = serde_json::Value::from(vals[0].clone());
                return serde_json::to_string_pretty(&json_val).map_err(|e| {
                    Error::serialization_error(format!("Failed to serialize result: {e}"))
                });
            }

            let json_vals: Vec<Value> = vals.into_iter().map(serde_json::Value::from).collect();
            let array = Value::Array(json_vals);
            serde_json::to_string_pretty(&array).map_err(|e| {
                Error::serialization_error(format!("Failed to serialize results: {e}"))
            })
        }
        Err(e) => Err(Error::jq_filter_error(
            format!("{filter:?}"),
            format!("Filter execution error: {e}"),
        )),
    }
}

#[cfg(not(feature = "jq"))]
#[allow(clippy::needless_pass_by_value)]
fn apply_jq_filter_value(json_value: Value, filter: &str) -> Result<String, Error> {
    apply_basic_jq_filter(&json_value, filter)
}

#[cfg(not(feature = "jq"))]
const BASIC_JQ_ADVANCED_FEATURES: &[&str] = &["[", "]", "|", "(", ")", "select", "map", "length"];

#[cfg(not(feature = "jq"))]
fn uses_advanced_jq_features(filter: &str) -> bool {
    BASIC_JQ_ADVANCED_FEATURES
        .iter()
        .any(|needle| filter.contains(needle))
}

#[cfg(not(feature = "jq"))]
fn array_iteration_value(json_value: &Value) -> Value {
    match json_value {
        Value::Array(arr) => Value::Array(arr.clone()),
        Value::Object(obj) => Value::Array(obj.values().cloned().collect()),
        _ => Value::Null,
    }
}

#[cfg(not(feature = "jq"))]
fn length_value(json_value: &Value) -> Value {
    match json_value {
        Value::Array(arr) => Value::Number(arr.len().into()),
        Value::Object(obj) => Value::Number(obj.len().into()),
        Value::String(s) => Value::Number(s.len().into()),
        _ => Value::Null,
    }
}

#[cfg(not(feature = "jq"))]
fn map_array_field(json_value: &Value, field_path: &str) -> Value {
    match json_value {
        Value::Array(arr) => Value::Array(
            arr.iter()
                .map(|item| get_nested_field(item, field_path))
                .collect(),
        ),
        _ => Value::Null,
    }
}

#[cfg(not(feature = "jq"))]
fn basic_jq_filter_value(json_value: &Value, filter: &str) -> Result<Value, Error> {
    match filter {
        "." => Ok(json_value.clone()),
        ".[]" => Ok(array_iteration_value(json_value)),
        ".length" => Ok(length_value(json_value)),
        filter if filter.starts_with(".[].") => Ok(map_array_field(json_value, &filter[4..])),
        filter if filter.starts_with('.') => Ok(get_nested_field(json_value, &filter[1..])),
        _ => Err(Error::jq_filter_error(
            filter,
            "Unsupported JQ filter. Only basic field access like '.name' or '.metadata.role' is supported without the full jq library.",
        )),
    }
}

#[cfg(not(feature = "jq"))]
/// Basic JQ-like functionality for common cases
fn apply_basic_jq_filter(json_value: &Value, filter: &str) -> Result<String, Error> {
    if uses_advanced_jq_features(filter) {
        tracing::warn!(
            "Advanced JQ features require building with --features jq. \
             Currently only basic field access is supported (e.g., '.field', '.nested.field'). \
             To enable full JQ support: cargo install aperture-cli --features jq"
        );
    }

    let result = basic_jq_filter_value(json_value, filter)?;

    serde_json::to_string_pretty(&result).map_err(|e| {
        Error::serialization_error(format!("Failed to serialize filtered result: {e}"))
    })
}

#[cfg(not(feature = "jq"))]
/// Get a nested field from JSON using dot notation
fn get_nested_field(json_value: &Value, field_path: &str) -> Value {
    let parts: Vec<&str> = field_path.split('.').collect();
    let mut current = json_value;

    for part in parts {
        if part.is_empty() {
            continue;
        }

        // Handle array index notation like [0]
        if part.starts_with('[') && part.ends_with(']') {
            let index_str = &part[1..part.len() - 1];
            let Ok(index) = index_str.parse::<usize>() else {
                return Value::Null;
            };

            match current {
                Value::Array(arr) => {
                    let Some(item) = arr.get(index) else {
                        return Value::Null;
                    };
                    current = item;
                }
                _ => return Value::Null,
            }
            continue;
        }

        match current {
            Value::Object(obj) => {
                if let Some(field) = obj.get(part) {
                    current = field;
                } else {
                    return Value::Null;
                }
            }
            Value::Array(arr) => {
                // Handle numeric string as array index
                let Ok(index) = part.parse::<usize>() else {
                    return Value::Null;
                };

                let Some(item) = arr.get(index) else {
                    return Value::Null;
                };
                current = item;
            }
            _ => return Value::Null,
        }
    }

    current.clone()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_build_url_from_params_sorts_query_parameters() {
        let mut query = std::collections::HashMap::new();
        query.insert("b".to_string(), "2".to_string());
        query.insert("a".to_string(), "1".to_string());

        let url = build_url_from_params(
            "https://example.com",
            "/items",
            &std::collections::HashMap::new(),
            &query,
        )
        .expect("url build should succeed");

        assert_eq!(url, "https://example.com/items?a=1&b=2");
    }

    #[test]
    fn test_apply_jq_filter_simple_field_access() {
        let json = r#"{"name": "Alice", "age": 30}"#;
        let result = apply_jq_filter(json, ".name").unwrap();
        let parsed: Value = serde_json::from_str(&result).unwrap();
        assert_eq!(parsed, serde_json::json!("Alice"));
    }

    #[test]
    fn test_apply_jq_filter_nested_field_access() {
        let json = r#"{"user": {"name": "Bob", "id": 123}}"#;
        let result = apply_jq_filter(json, ".user.name").unwrap();
        let parsed: Value = serde_json::from_str(&result).unwrap();
        assert_eq!(parsed, serde_json::json!("Bob"));
    }

    #[cfg(feature = "jq")]
    #[test]
    fn test_apply_jq_filter_array_index() {
        let json = r#"{"items": ["first", "second", "third"]}"#;
        let result = apply_jq_filter(json, ".items[1]").unwrap();
        let parsed: Value = serde_json::from_str(&result).unwrap();
        assert_eq!(parsed, serde_json::json!("second"));
    }

    #[cfg(feature = "jq")]
    #[test]
    fn test_apply_jq_filter_array_iteration() {
        let json = r#"[{"id": 1}, {"id": 2}, {"id": 3}]"#;
        let result = apply_jq_filter(json, ".[].id").unwrap();
        let parsed: Value = serde_json::from_str(&result).unwrap();
        // JQ returns multiple results as an array
        assert_eq!(parsed, serde_json::json!([1, 2, 3]));
    }

    #[cfg(feature = "jq")]
    #[test]
    fn test_apply_jq_filter_complex_expression() {
        let json = r#"{"users": [{"name": "Alice", "age": 30}, {"name": "Bob", "age": 25}]}"#;
        let result = apply_jq_filter(json, ".users | map(.name)").unwrap();
        let parsed: Value = serde_json::from_str(&result).unwrap();
        assert_eq!(parsed, serde_json::json!(["Alice", "Bob"]));
    }

    #[cfg(feature = "jq")]
    #[test]
    fn test_apply_jq_filter_select() {
        let json =
            r#"[{"id": 1, "active": true}, {"id": 2, "active": false}, {"id": 3, "active": true}]"#;
        let result = apply_jq_filter(json, "[.[] | select(.active)]").unwrap();
        let parsed: Value = serde_json::from_str(&result).unwrap();
        assert_eq!(
            parsed,
            serde_json::json!([{"id": 1, "active": true}, {"id": 3, "active": true}])
        );
    }

    #[test]
    fn test_apply_jq_filter_invalid_json() {
        let json = "not valid json";
        let result = apply_jq_filter(json, ".field");
        assert!(result.is_err());
        if let Err(err) = result {
            let error_msg = err.to_string();
            assert!(error_msg.contains("JQ filter error"));
            assert!(error_msg.contains(".field"));
            assert!(error_msg.contains("Response is not valid JSON"));
        } else {
            panic!("Expected error");
        }
    }

    #[cfg(feature = "jq")]
    #[test]
    fn test_apply_jq_filter_invalid_expression() {
        let json = r#"{"name": "test"}"#;
        let result = apply_jq_filter(json, "invalid..expression");
        assert!(result.is_err());
        if let Err(err) = result {
            let error_msg = err.to_string();
            assert!(error_msg.contains("JQ filter error") || error_msg.contains("Parse error"));
            assert!(error_msg.contains("invalid..expression"));
        } else {
            panic!("Expected error");
        }
    }

    #[test]
    fn test_apply_jq_filter_null_result() {
        let json = r#"{"name": "test"}"#;
        let result = apply_jq_filter(json, ".missing_field").unwrap();
        let parsed: Value = serde_json::from_str(&result).unwrap();
        assert_eq!(parsed, serde_json::json!(null));
    }

    #[cfg(feature = "jq")]
    #[test]
    fn test_apply_jq_filter_arithmetic() {
        let json = r#"{"x": 10, "y": 20}"#;
        let result = apply_jq_filter(json, ".x + .y").unwrap();
        let parsed: Value = serde_json::from_str(&result).unwrap();
        assert_eq!(parsed, serde_json::json!(30));
    }

    #[cfg(feature = "jq")]
    #[test]
    fn test_apply_jq_filter_string_concatenation() {
        let json = r#"{"first": "Hello", "second": "World"}"#;
        let result = apply_jq_filter(json, r#".first + " " + .second"#).unwrap();
        let parsed: Value = serde_json::from_str(&result).unwrap();
        assert_eq!(parsed, serde_json::json!("Hello World"));
    }

    #[cfg(feature = "jq")]
    #[test]
    fn test_apply_jq_filter_length() {
        let json = r#"{"items": [1, 2, 3, 4, 5]}"#;
        let result = apply_jq_filter(json, ".items | length").unwrap();
        let parsed: Value = serde_json::from_str(&result).unwrap();
        assert_eq!(parsed, serde_json::json!(5));
    }
}
