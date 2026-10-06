use crate::cache::models::{CachedCommand, CachedSecurityScheme, CachedSpec};
use crate::config::models::{GlobalConfig, ProxyConfig};
use crate::config::url_resolver::BaseUrlResolver;
use crate::constants;
use crate::error::Error;
use crate::invocation::{ExecutionResult, ProxyOverride, RequestBody};
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

const DEFAULT_USER_AGENT: &str = concat!("aperture/", env!("CARGO_PKG_VERSION"));
type HttpResponseBytes = (reqwest::StatusCode, HashMap<String, String>, Vec<u8>);

#[cfg(feature = "jq")]
use jaq_core::{data, unwrap_valr, Ctx, Vars};
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

/// Private routing identity and its bounded diagnostic projection.
///
/// Route strings are retained only for the legacy transport fingerprint. Even
/// userinfo-free URLs and bypass entries can contain transformed credentials;
/// neither output nor Debug may expose them, including before auth is resolved.
#[derive(Clone, Default, PartialEq, Eq)]
struct ProxyDiagnostics {
    source: &'static str,
    disabled: bool,
    all: Option<String>,
    http: Option<String>,
    https: Option<String>,
    no_proxy: Vec<String>,
}

impl std::fmt::Debug for ProxyDiagnostics {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.to_json().fmt(formatter)
    }
}

impl ProxyDiagnostics {
    fn to_json(&self) -> serde_json::Value {
        serde_json::json!({
            "source": self.source,
            "disabled": self.disabled,
            "all": self.all.as_ref().map(|_| "[PROXY URL OMITTED]"),
            "http": self.http.as_ref().map(|_| "[PROXY URL OMITTED]"),
            "https": self.https.as_ref().map(|_| "[PROXY URL OMITTED]"),
            "no_proxy": [],
            "no_proxy_count": self.no_proxy.len(),
        })
    }

    /// Preserve the pre-omission transport identity bytes. This value must only
    /// feed the private digest, never diagnostics or persisted response data.
    fn transport_identity_json(&self) -> serde_json::Value {
        serde_json::json!({
            "source": self.source,
            "disabled": self.disabled,
            "all": self.all.as_deref().map(crate::config::settings::sanitize_proxy_url),
            "http": self.http.as_deref().map(crate::config::settings::sanitize_proxy_url),
            "https": self.https.as_deref().map(crate::config::settings::sanitize_proxy_url),
            "no_proxy": self.no_proxy,
        })
    }
}

/// Context-owned HTTP clients, shared across cloned contexts and batch operations.
///
/// Resolved proxy settings, effective timeout and redirect policy form the key
/// so configuration changes cannot reuse a client with different transport rules.
/// No process-global client is retained.
#[derive(Clone, Default)]
pub struct HttpClientPool(std::sync::Arc<std::sync::Mutex<HashMap<String, reqwest::Client>>>);

impl std::fmt::Debug for HttpClientPool {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // reqwest client Debug includes proxy URIs and arbitrary bypass domains.
        // Do not delegate or lock: cached transport state is never diagnostic data.
        formatter
            .debug_struct("HttpClientPool")
            .finish_non_exhaustive()
    }
}

fn transport_key(
    ctx: &crate::invocation::ExecutionContext,
    diagnostics: &ProxyDiagnostics,
) -> String {
    let mut digest = Sha256::new();
    digest.update(diagnostics.transport_identity_json().to_string());
    digest.update(effective_timeout_secs(ctx).to_be_bytes());
    digest.update(format!("{:?}", ctx.proxy_override));
    digest.update(format!(
        "{:?}",
        ctx.global_config.as_ref().map(|config| &config.proxy)
    ));
    if let Some(password_env) = ctx
        .global_config
        .as_ref()
        .and_then(|config| non_empty(config.proxy.password_env.as_deref()))
    {
        // Password rotation must not reuse a client holding old proxy credentials.
        digest.update(format!("{:?}", std::env::var(password_env).ok()));
    }
    for names in [
        ["HTTP_PROXY", "http_proxy"],
        ["HTTPS_PROXY", "https_proxy"],
        ["ALL_PROXY", "all_proxy"],
        ["NO_PROXY", "no_proxy"],
    ] {
        // reqwest's environment precedence can differ from diagnostics. Include
        // both spellings so a change to either cannot reuse a stale route.
        for name in names {
            digest.update(format!("{:?}", std::env::var(name).ok()));
        }
    }
    format!("{:x}", digest.finalize())
}

fn ensure_tls_provider() {
    if rustls::crypto::CryptoProvider::get_default().is_some() {
        return;
    }
    #[cfg(not(windows))]
    let _ = rustls::crypto::ring::default_provider().install_default();
    #[cfg(windows)]
    let _ = rustls::crypto::aws_lc_rs::default_provider().install_default();
}

struct ProxyBuildResult {
    client: reqwest::Client,
    diagnostics: ProxyDiagnostics,
}

fn configure_proxy(
    builder: reqwest::ClientBuilder,
    ctx: &crate::invocation::ExecutionContext,
) -> Result<(reqwest::ClientBuilder, ProxyDiagnostics), Error> {
    match &ctx.proxy_override {
        ProxyOverride::Disable => Ok((builder.no_proxy(), disabled_proxy_diagnostics())),
        ProxyOverride::Use(url) => configure_cli_proxy(builder, url),
        ProxyOverride::Default => configure_default_proxy(builder, ctx.global_config.as_ref()),
    }
}

fn disabled_proxy_diagnostics() -> ProxyDiagnostics {
    ProxyDiagnostics {
        source: "cli",
        disabled: true,
        ..ProxyDiagnostics::default()
    }
}

fn configure_cli_proxy(
    builder: reqwest::ClientBuilder,
    url: &str,
) -> Result<(reqwest::ClientBuilder, ProxyDiagnostics), Error> {
    let proxy = proxy_all(url, "CLI")?;
    let diagnostics = ProxyDiagnostics {
        source: "cli",
        all: Some(url.to_string()),
        ..ProxyDiagnostics::default()
    };
    Ok((builder.no_proxy().proxy(proxy), diagnostics))
}

fn configure_default_proxy(
    builder: reqwest::ClientBuilder,
    global_config: Option<&GlobalConfig>,
) -> Result<(reqwest::ClientBuilder, ProxyDiagnostics), Error> {
    if let Some(diagnostics) = env_proxy_diagnostics() {
        return Ok((builder, diagnostics));
    }

    let Some(config) = global_config else {
        return Ok((builder, ProxyDiagnostics::default()));
    };
    configure_config_proxy(builder, &config.proxy)
}

fn env_proxy_diagnostics() -> Option<ProxyDiagnostics> {
    let http = first_env_value(&["HTTP_PROXY", "http_proxy"]);
    let https = first_env_value(&["HTTPS_PROXY", "https_proxy"]);
    let all = first_env_value(&["ALL_PROXY", "all_proxy"]);
    let no_proxy = first_env_value(&["NO_PROXY", "no_proxy"]);

    if http.is_none() && https.is_none() && all.is_none() {
        return None;
    }

    Some(ProxyDiagnostics {
        source: "environment",
        all,
        http,
        https,
        no_proxy: no_proxy.map_or_else(Vec::new, |value| parse_no_proxy_entries(&value)),
        ..ProxyDiagnostics::default()
    })
}

/// Proxy credentials can select an account even when the origin is anonymous.
/// Conservatively bypass caching for the selected authenticated proxy setup,
/// including destinations excluded by `NO_PROXY`; never persist proxy identity.
fn proxy_requires_cache_bypass(ctx: &crate::invocation::ExecutionContext) -> bool {
    match &ctx.proxy_override {
        ProxyOverride::Disable => false,
        ProxyOverride::Use(url) => proxy_url_has_credentials(url),
        ProxyOverride::Default => default_proxy_has_credentials(ctx.global_config.as_ref()),
    }
}

fn proxy_url_has_credentials(url: &str) -> bool {
    // reqwest accepts scheme-less proxy authorities by adding http://. An
    // opaque URL like `alice:password@host:port` parses without userinfo, so
    // require a host before accepting the first parse for this policy check.
    let parsed = reqwest::Url::parse(url)
        .ok()
        .filter(reqwest::Url::has_host)
        .or_else(|| reqwest::Url::parse(&format!("http://{url}")).ok());
    parsed.is_some_and(|url| !url.username().is_empty() || url.password().is_some())
}

fn default_proxy_has_credentials(config: Option<&GlobalConfig>) -> bool {
    let environment: Vec<String> = [
        ["HTTP_PROXY", "http_proxy"],
        ["HTTPS_PROXY", "https_proxy"],
        ["ALL_PROXY", "all_proxy"],
    ]
    .iter()
    .filter_map(|names| first_env_value(names))
    .collect();
    if !environment.is_empty() {
        return environment.iter().any(|url| proxy_url_has_credentials(url));
    }
    let Some(proxy) = config.map(|config| &config.proxy) else {
        return false;
    };
    has_config_proxy(proxy)
        && (non_empty(proxy.username.as_deref()).is_some()
            || non_empty(proxy.password_env.as_deref()).is_some()
            || [proxy.http.as_deref(), proxy.https.as_deref()]
                .into_iter()
                .flatten()
                .any(proxy_url_has_credentials))
}

/// Collect only the selected proxy setup; mirror transport precedence without
/// changing routing, `NO_PROXY`, or client/cache identity.
fn with_selected_proxy_secrets(
    mut secrets: logging::SecretContext,
    ctx: &crate::invocation::ExecutionContext,
) -> logging::SecretContext {
    match &ctx.proxy_override {
        ProxyOverride::Disable => secrets,
        ProxyOverride::Use(url) => secrets.with_proxy_url(url),
        ProxyOverride::Default => {
            let environment: Vec<String> = [
                ["HTTP_PROXY", "http_proxy"],
                ["HTTPS_PROXY", "https_proxy"],
                ["ALL_PROXY", "all_proxy"],
            ]
            .iter()
            .filter_map(|names| first_env_value(names))
            .collect();
            if !environment.is_empty() {
                for url in environment {
                    secrets = secrets.with_proxy_url(&url);
                }
                return secrets;
            }
            with_config_proxy_secrets(secrets, ctx.global_config.as_ref())
        }
    }
}

fn with_config_proxy_secrets(
    mut secrets: logging::SecretContext,
    config: Option<&GlobalConfig>,
) -> logging::SecretContext {
    let Some(proxy) = config
        .map(|config| &config.proxy)
        .filter(|proxy| has_config_proxy(proxy))
    else {
        return secrets;
    };
    for url in [proxy.http.as_deref(), proxy.https.as_deref()]
        .into_iter()
        .flatten()
    {
        secrets = secrets.with_proxy_url(url);
    }
    let (Some(username), Some(password_env)) = (
        non_empty(proxy.username.as_deref()),
        non_empty(proxy.password_env.as_deref()),
    ) else {
        return secrets;
    };
    let Ok(password) = std::env::var(password_env) else {
        return secrets;
    };
    secrets.with_proxy_basic_auth(username, &password)
}

fn first_env_value(names: &[&str]) -> Option<String> {
    names.iter().find_map(|name| {
        std::env::var(name)
            .ok()
            .filter(|value| !value.trim().is_empty())
    })
}

fn configure_config_proxy(
    mut builder: reqwest::ClientBuilder,
    config: &ProxyConfig,
) -> Result<(reqwest::ClientBuilder, ProxyDiagnostics), Error> {
    if !has_config_proxy(config) {
        return Ok((builder, ProxyDiagnostics::default()));
    }

    builder = builder.no_proxy();
    let no_proxy_env = first_env_value(&["NO_PROXY", "no_proxy"]);
    let no_proxy = config_no_proxy(config, no_proxy_env.as_deref());
    let mut diagnostics = ProxyDiagnostics {
        source: "config",
        no_proxy: no_proxy_env.map_or_else(
            || config.no_proxy.clone(),
            |value| parse_no_proxy_entries(&value),
        ),
        ..ProxyDiagnostics::default()
    };

    let (builder, http) = add_config_http_proxy(builder, config, no_proxy.clone())?;
    diagnostics.http = http;
    let (builder, https) = add_config_https_proxy(builder, config, no_proxy)?;
    diagnostics.https = https;

    Ok((builder, diagnostics))
}

fn has_config_proxy(config: &ProxyConfig) -> bool {
    non_empty(config.http.as_deref()).is_some() || non_empty(config.https.as_deref()).is_some()
}

fn add_config_http_proxy(
    builder: reqwest::ClientBuilder,
    config: &ProxyConfig,
    no_proxy: Option<reqwest::NoProxy>,
) -> Result<(reqwest::ClientBuilder, Option<String>), Error> {
    let Some(url) = non_empty(config.http.as_deref()) else {
        return Ok((builder, None));
    };
    let proxy = proxy_http(url, "config HTTP")?.no_proxy(no_proxy);
    let builder = builder.proxy(apply_config_proxy_auth(proxy, config)?);
    Ok((builder, Some(url.to_string())))
}

fn add_config_https_proxy(
    builder: reqwest::ClientBuilder,
    config: &ProxyConfig,
    no_proxy: Option<reqwest::NoProxy>,
) -> Result<(reqwest::ClientBuilder, Option<String>), Error> {
    let Some(url) = non_empty(config.https.as_deref()) else {
        return Ok((builder, None));
    };
    let proxy = proxy_https(url, "config HTTPS")?.no_proxy(no_proxy);
    let builder = builder.proxy(apply_config_proxy_auth(proxy, config)?);
    Ok((builder, Some(url.to_string())))
}

fn config_no_proxy(config: &ProxyConfig, env_no_proxy: Option<&str>) -> Option<reqwest::NoProxy> {
    env_no_proxy
        .and_then(reqwest::NoProxy::from_string)
        .or_else(|| reqwest::NoProxy::from_string(&config.no_proxy.join(",")))
}

fn non_empty(value: Option<&str>) -> Option<&str> {
    value.map(str::trim).filter(|value| !value.is_empty())
}

fn apply_config_proxy_auth(
    proxy: reqwest::Proxy,
    config: &ProxyConfig,
) -> Result<reqwest::Proxy, Error> {
    match (
        non_empty(config.username.as_deref()),
        non_empty(config.password_env.as_deref()),
    ) {
        (Some(username), Some(password_env)) => {
            let password = std::env::var(password_env).map_err(|_| {
                Error::invalid_config("Proxy password environment variable is unavailable")
            })?;
            Ok(proxy.basic_auth(username, &password))
        }
        (None, None) => Ok(proxy),
        _ => Err(Error::invalid_config(
            "Proxy authentication requires both proxy.username and proxy.password_env",
        )),
    }
}

fn proxy_http(url: &str, label: &'static str) -> Result<reqwest::Proxy, Error> {
    reqwest::Proxy::http(url).map_err(|_| invalid_proxy_url(label, url))
}

fn proxy_https(url: &str, label: &'static str) -> Result<reqwest::Proxy, Error> {
    reqwest::Proxy::https(url).map_err(|_| invalid_proxy_url(label, url))
}

fn proxy_all(url: &str, label: &'static str) -> Result<reqwest::Proxy, Error> {
    reqwest::Proxy::all(url).map_err(|_| invalid_proxy_url(label, url))
}

fn invalid_proxy_url(label: &'static str, _url: &str) -> Error {
    // Parsing fails before operation sensitivity exists. Arbitrary URL components
    // and malformed tails cannot be made safe by removing guessed userinfo.
    Error::invalid_config(format!("Invalid {label} proxy URL (value omitted)"))
}

fn parse_no_proxy_entries(value: &str) -> Vec<String> {
    value
        .split(',')
        .map(str::trim)
        .filter(|entry| !entry.is_empty())
        .map(ToString::to_string)
        .collect()
}

fn log_proxy_diagnostics(diagnostics: &ProxyDiagnostics) {
    if diagnostics.disabled {
        tracing::debug!(target: "aperture::executor", "Proxy routing disabled for request");
        return;
    }

    if diagnostics.source.is_empty() {
        tracing::debug!(target: "aperture::executor", "No proxy configuration selected");
        return;
    }

    tracing::debug!(
        target: "aperture::executor",
        source = diagnostics.source,
        all_configured = diagnostics.all.is_some(),
        http_configured = diagnostics.http.is_some(),
        https_configured = diagnostics.https.is_some(),
        no_proxy_count = diagnostics.no_proxy.len(),
        "Proxy configuration selected"
    );
}

/// CLI translation applies explicit timeout overrides to `global_config` first.
fn effective_timeout_secs(ctx: &crate::invocation::ExecutionContext) -> u64 {
    ctx.global_config
        .as_ref()
        .map_or(30, |config| config.default_timeout_secs)
}

fn effective_max_response_bytes(ctx: &crate::invocation::ExecutionContext) -> Result<usize, Error> {
    let value = ctx.max_response_bytes.unwrap_or_else(|| {
        ctx.global_config.as_ref().map_or(
            crate::response_limit::DEFAULT_MAX_RESPONSE_BYTES,
            |config| config.max_response_bytes,
        )
    });
    crate::response_limit::validate(value)
}

/// Build HTTP client with effective timeout and resolved proxy behavior.
fn build_http_client(
    ctx: &crate::invocation::ExecutionContext,
    pagination: bool,
) -> Result<ProxyBuildResult, Error> {
    ensure_tls_provider();
    let (builder, diagnostics) = configure_proxy(reqwest::Client::builder(), ctx)?;
    let key = format!(
        "{}:origin-bound-v1:{pagination}",
        transport_key(ctx, &diagnostics)
    );
    let mut clients = ctx
        .http_clients
        .0
        .lock()
        .map_err(|_| Error::invalid_config("HTTP client pool lock poisoned"))?;
    if let Some(client) = clients.get(&key) {
        return Ok(ProxyBuildResult {
            client: client.clone(),
            diagnostics,
        });
    }

    // Pagination follows only explicitly validated links, never redirects.
    let builder = if pagination {
        builder.redirect(reqwest::redirect::Policy::none())
    } else {
        builder.redirect(operation_redirect_policy())
    };
    let client = builder
        .timeout(std::time::Duration::from_secs(effective_timeout_secs(ctx)))
        .build()
        .map_err(|_| {
            Error::request_failed(
                reqwest::StatusCode::INTERNAL_SERVER_ERROR,
                "Failed to create HTTP client",
            )
        })?;

    clients.insert(key, client.clone());
    drop(clients);
    log_proxy_diagnostics(&diagnostics);
    Ok(ProxyBuildResult {
        client,
        diagnostics,
    })
}

/// Bind every hop to the original request origin, independent of header names.
/// Generic errors deliberately omit attacker-controlled redirect destinations.
fn operation_redirect_policy() -> reqwest::redirect::Policy {
    reqwest::redirect::Policy::custom(|attempt| {
        if attempt.previous().len() >= 10 {
            return attempt.error("Operation redirect limit exceeded");
        }
        let target = attempt.url();
        let Some(original) = attempt.previous().first() else {
            return attempt.error("Unsafe operation redirect");
        };
        if !target.username().is_empty()
            || target.password().is_some()
            || target.origin() != original.origin()
        {
            return attempt.error("Unsafe operation redirect");
        }
        attempt.follow()
    })
}

/// Preserve the complete Link list and reject undecodable header data rather
/// than mistaking it for a missing next page.
fn collect_response_headers(headers: &HeaderMap) -> Result<HashMap<String, String>, Error> {
    let mut response_headers: HashMap<String, String> = headers
        .iter()
        .map(|(k, v)| (k.as_str().to_string(), v.to_str().unwrap_or("").to_string()))
        .collect();
    // Multiple Link fields form one list. Dropping fields could hide an
    // ambiguous next target or silently report the traversal as complete.
    let links = headers
        .get_all(reqwest::header::LINK)
        .iter()
        .map(|value| {
            value
                .to_str()
                .map_err(|_| Error::validation_error("Invalid pagination Link header encoding"))
        })
        .collect::<Result<Vec<_>, _>>()?;
    if !links.is_empty() {
        response_headers.insert(constants::HEADER_LINK.to_string(), links.join(", "));
    }
    Ok(response_headers)
}

/// Send HTTP request and retain response bytes until the operation's media type is known.
async fn send_request(
    request: reqwest::RequestBuilder,
    spec: &CachedSpec,
    operation: &CachedCommand,
    secret_ctx: Option<&logging::SecretContext>,
    max_response_bytes: usize,
) -> Result<HttpResponseBytes, Error> {
    let start_time = std::time::Instant::now();
    // Remove only the URL: native error classification and typed causes survive.
    let mut response = request
        .send()
        .await
        .map_err(|error| Error::Network(error.without_url()))?;
    let status = response.status();
    let duration_ms = start_time.elapsed().as_millis();
    let mut response_headers_map = reqwest::header::HeaderMap::new();
    for (name, value) in response.headers() {
        response_headers_map.insert(name.clone(), value.clone());
    }
    let response_headers = collect_response_headers(response.headers())?;
    let response_bytes = read_response_bounded(&mut response, max_response_bytes).await?;

    if operation.has_binary_response() {
        tracing::debug!(
            status = status.as_u16(),
            duration_ms,
            byte_count = response_bytes.len(),
            "Received binary response"
        );
    } else {
        let response_text = String::from_utf8_lossy(&response_bytes);
        logging::log_operation_response(
            status.as_u16(),
            duration_ms,
            Some(&response_headers_map),
            Some(&response_text),
            logging::get_max_body_len(),
            secret_ctx,
            (spec, operation),
        );
    }

    Ok((status, response_headers, response_bytes))
}

/// Enforce advertised and actual sizes before decoding, logging or retaining output.
async fn read_response_bounded(
    response: &mut reqwest::Response,
    limit: usize,
) -> Result<Vec<u8>, Error> {
    if response
        .content_length()
        .is_some_and(|length| usize::try_from(length).map_or(true, |length| length > limit))
    {
        return Err(crate::response_limit::exceeded(limit));
    }
    let mut bytes = Vec::new();
    while let Some(chunk) = response
        .chunk()
        .await
        .map_err(|error| Error::Network(error.without_url()))?
    {
        if chunk.len() > limit - bytes.len() {
            return Err(crate::response_limit::exceeded(limit));
        }
        bytes.extend_from_slice(&chunk);
    }
    Ok(bytes)
}

/// Send HTTP request with retry logic
#[allow(clippy::too_many_arguments)]
#[allow(clippy::too_many_lines)]
async fn send_request_with_retry(
    client: &reqwest::Client,
    method: Method,
    url: &str,
    headers: HeaderMap,
    body: Option<RequestBody>,
    retry_context: Option<&RetryContext>,
    spec: &CachedSpec,
    operation: &CachedCommand,
    secret_ctx: Option<&logging::SecretContext>,
    max_response_bytes: usize,
) -> Result<HttpResponseBytes, Error> {
    use crate::resilience::RetryConfig;

    match body.as_ref() {
        Some(RequestBody::Binary(bytes)) => tracing::debug!(
            method = %method,
            operation_id = %operation.operation_id,
            byte_count = bytes.len(),
            "Sending binary request body"
        ),
        Some(RequestBody::Json(json)) => {
            logging::log_operation_request(
                method.as_str(),
                url,
                Some(&headers),
                Some(json),
                secret_ctx,
                spec,
                operation,
            );
        }
        None => {
            logging::log_operation_request(
                method.as_str(),
                url,
                Some(&headers),
                None,
                secret_ctx,
                spec,
                operation,
            );
        }
    }

    let Some(ctx) = retry_context.filter(|ctx| ctx.is_enabled()) else {
        return send_request_once(
            client,
            method,
            url,
            headers,
            body,
            spec,
            operation,
            secret_ctx,
            max_response_bytes,
        )
        .await;
    };

    if !ctx.is_safe_to_retry() {
        tracing::warn!(
            method = %method,
            operation_id = %operation.operation_id,
            "Retries disabled - method is not idempotent and no idempotency key provided. \
             Use --force-retry or provide --idempotency-key"
        );
        return send_request_once(
            client,
            method.clone(),
            url,
            headers,
            body,
            spec,
            operation,
            secret_ctx,
            max_response_bytes,
        )
        .await;
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
        spec,
        operation,
        secret_ctx,
        max_response_bytes,
    )
    .await
}

#[allow(clippy::too_many_arguments)]
async fn retry_request_with_backoff(
    client: &reqwest::Client,
    method: Method,
    url: &str,
    headers: HeaderMap,
    body: Option<RequestBody>,
    ctx: &RetryContext,
    retry_config: &crate::resilience::RetryConfig,
    spec: &CachedSpec,
    operation: &CachedCommand,
    secret_ctx: Option<&logging::SecretContext>,
    max_response_bytes: usize,
) -> Result<HttpResponseBytes, Error> {
    let max_attempts = ctx.max_attempts;
    let mut attempt: u32 = 0;
    let mut last_error: Option<Error> = None;
    let mut last_status: Option<reqwest::StatusCode> = None;
    let mut last_response_headers: Option<HashMap<String, String>> = None;
    let mut last_response_text: Option<Vec<u8>> = None;

    while attempt < max_attempts {
        attempt += 1;

        let request = build_request(client, method.clone(), url, headers.clone(), body.clone());
        match send_request(request, spec, operation, secret_ctx, max_response_bytes).await {
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
                        last_error = None;
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
                    // Exhaustion must describe the last attempt, not a stale
                    // HTTP response from before the transport failure.
                    last_status = None;
                    last_response_headers = None;
                    last_response_text = None;
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
    last_response_text: Option<Vec<u8>>,
    last_error: Option<Error>,
    ctx: &RetryContext,
    method: &Method,
    operation: &CachedCommand,
) -> Result<HttpResponseBytes, Error> {
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
    Return(HttpResponseBytes),
    Retry {
        status: reqwest::StatusCode,
        response_headers: HashMap<String, String>,
        response_text: Vec<u8>,
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
    response_text: Vec<u8>,
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

fn transport_stage(error: &reqwest::Error) -> bool {
    error.is_connect()
        || error.is_timeout()
        || error.is_body()
        || (error.is_request() && !error.is_builder() && !error.is_redirect())
}

/// `Response::bytes` wraps body transport failures in a Decode error. Inspect
/// typed sources, not messages; unrelated decoding failures remain terminal.
fn is_retryable_network_error(error: &Error) -> bool {
    let Error::Network(network) = error else {
        return false;
    };
    if transport_stage(network) {
        return true;
    }
    let mut source = std::error::Error::source(network);
    while let Some(error) = source {
        if error
            .downcast_ref::<reqwest::Error>()
            .is_some_and(transport_stage)
        {
            return true;
        }
        source = error.source();
    }
    false
}

async fn handle_retryable_network_error(
    retry_config: &RetryConfig,
    attempt: u32,
    max_attempts: u32,
    method: &Method,
    operation: &CachedCommand,
    error: Error,
) -> RetryableNetworkError {
    // Only transport-stage failures are eligible. Builder, redirect, status,
    // and unrelated decoding errors are terminal. The caller separately enforces the
    // idempotent-method/idempotency-key/force policy before entering this loop.
    if !is_retryable_network_error(&error) {
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
    body: Option<RequestBody>,
) -> reqwest::RequestBuilder {
    let request = client.request(method, url).headers(headers);
    match body {
        Some(RequestBody::Json(source)) => {
            let json = serde_json::from_str::<Value>(&source)
                .expect("JSON bodies are validated before execution");
            request.json(&json)
        }
        Some(RequestBody::Binary(bytes)) => request.body(bytes),
        None => request,
    }
}

#[allow(clippy::too_many_arguments)]
async fn send_request_once(
    client: &reqwest::Client,
    method: Method,
    url: &str,
    headers: HeaderMap,
    body: Option<RequestBody>,
    spec: &CachedSpec,
    operation: &CachedCommand,
    secret_ctx: Option<&logging::SecretContext>,
    max_response_bytes: usize,
) -> Result<HttpResponseBytes, Error> {
    let request = build_request(client, method, url, headers, body);
    send_request(request, spec, operation, secret_ctx, max_response_bytes).await
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
        .flatten()
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

fn request_requires_cache_bypass(headers: &HeaderMap, url: &str) -> bool {
    // The cache's single-value header map cannot represent repeated fields.
    // Skip these requests rather than discard a value from the request identity.
    if headers
        .iter()
        .any(|(name, value)| value.is_sensitive() || is_auth_header(name.as_str()))
        || headers
            .keys()
            .any(|name| headers.get_all(name).iter().count() > 1)
    {
        return true;
    }
    reqwest::Url::parse(url)
        .is_ok_and(|parsed| !parsed.username().is_empty() || parsed.password().is_some())
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

    if !cache_cfg.enabled || !matches!(*method, Method::GET | Method::HEAD) {
        return Ok(None);
    }

    // Authenticated caching is disabled even for the legacy opt-in flag.
    if request_requires_cache_bypass(headers, url) {
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
    max_response_bytes: usize,
) -> Result<Option<CachedResponse>, Error> {
    if let Some((cache_key, response_cache)) = cache_context {
        response_cache
            .get_with_limit(
                cache_key,
                u64::try_from(max_response_bytes)
                    .map_err(|_| Error::invalid_config("max_response_bytes overflow"))?,
            )
            .await
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
    // Replaying a session-creating response without its cookie changes semantics.
    if response_headers
        .keys()
        .any(|name| name.eq_ignore_ascii_case("set-cookie"))
    {
        return Ok(());
    }

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
fn validate_header_value(_name: &str, value: &str) -> Result<(), Error> {
    if value.chars().any(|c| c == '\r' || c == '\n' || c == '\0') {
        return Err(Error::invalid_header_control_characters());
    }
    Ok(())
}

/// Parse and resolve invocation-only headers. Sensitivity belongs to the final
/// value, so normal `HeaderMap` replacement and cloning preserve its policy.
fn parse_custom_header(header_str: &str) -> Result<(HeaderName, HeaderValue), Error> {
    let (name, value) = header_str
        .split_once(':')
        .ok_or_else(|| Error::invalid_header_format(header_str))?;
    let name = name.trim();
    let header_name = parse_custom_header_name(name)?;
    let (expanded, sensitive) = expand_header_environment(value.trim())?;
    validate_header_value(name, &expanded)?;
    let mut value = HeaderValue::from_str(&expanded)
        .map_err(|e| Error::invalid_header_value(name, e.to_string()))?;
    value.set_sensitive(sensitive);
    Ok((header_name, value))
}

fn parse_custom_header_name(name: &str) -> Result<HeaderName, Error> {
    if name.is_empty() {
        return Err(Error::empty_header_name());
    }
    HeaderName::from_str(name).map_err(|e| Error::invalid_header_name(name, e.to_string()))
}

fn header_environment_error() -> Error {
    Error::validation_error(
        "Invalid header environment reference: use ${NAME} with a nonempty Unicode environment value (input omitted)",
    )
}

/// Expand only explicit ${NAME} references, once; retain other dollar/braces
/// literally. Never include a name, value, or partial expansion in errors.
fn expand_header_environment(value: &str) -> Result<(String, bool), Error> {
    let mut remaining = value;
    let mut expanded = String::new();
    let mut sensitive = false;
    while let Some(start) = remaining.find("${") {
        expanded.push_str(&remaining[..start]);
        let reference = &remaining[start + 2..];
        let end = reference.find('}').ok_or_else(header_environment_error)?;
        let name = &reference[..end];
        validate_header_environment_name(name)?;
        let resolved = std::env::var(name).map_err(|_| header_environment_error())?;
        if resolved.is_empty() {
            return Err(header_environment_error());
        }
        expanded.push_str(&resolved);
        sensitive = true;
        remaining = &reference[end + 1..];
    }
    expanded.push_str(remaining);
    Ok((expanded, sensitive))
}

fn validate_header_environment_name(name: &str) -> Result<(), Error> {
    let mut bytes = name.bytes();
    if !bytes
        .next()
        .is_some_and(|byte| byte.is_ascii_alphabetic() || byte == b'_')
        || !bytes.all(|byte| byte.is_ascii_alphanumeric() || byte == b'_')
    {
        return Err(header_environment_error());
    }
    Ok(())
}

struct ResolvedAuthenticationSecret {
    value: String,
    source: &'static str,
}

fn resolve_authentication_secret(
    security_scheme: &CachedSecurityScheme,
    api_name: &str,
    global_config: Option<&GlobalConfig>,
) -> Result<Option<ResolvedAuthenticationSecret>, Error> {
    let configured_secret = global_config
        .and_then(|config| config.api_configs.get(api_name))
        .and_then(|api_config| api_config.secrets.get(&security_scheme.name));

    if let Some(secret) = configured_secret {
        return resolve_secret_from_env(&security_scheme.name, &secret.name, "config").map(Some);
    }

    let Some(aperture_secret) = &security_scheme.aperture_secret else {
        return Ok(None);
    };

    resolve_secret_from_env(
        &security_scheme.name,
        &aperture_secret.name,
        "x-aperture-secret",
    )
    .map(Some)
}

fn resolve_secret_from_env(
    scheme_name: &str,
    env_var_name: &str,
    source: &'static str,
) -> Result<ResolvedAuthenticationSecret, Error> {
    let value = std::env::var(env_var_name).map_err(|_| {
        Error::secret_not_set(scheme_name, env_var_name).omit_diagnostic_inputs(
            "Required authentication secret not set (environment variable unavailable)",
            "Check the operation's credential mapping and environment variable availability.",
        )
    })?;

    Ok(ResolvedAuthenticationSecret { value, source })
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

/// RFC 6750 b64token syntax only; grants and expiry remain the provider's concern.
fn valid_external_bearer_token(token: &str) -> bool {
    let content = token.trim_end_matches('=');
    !content.is_empty()
        && content
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || b"-._~+/".contains(&byte))
}

fn validate_external_bearer_token(token: &str) -> Result<(), Error> {
    if valid_external_bearer_token(token) {
        return Ok(());
    }
    Err(Error::validation_error(
        "Invalid external OAuth2 bearer token syntax",
    ))
}

fn insert_http_authorization_header(
    headers: &mut HeaderMap,
    security_scheme: &CachedSecurityScheme,
    secret_value: &str,
) -> Result<(), Error> {
    let scheme_str = if security_scheme.scheme_type == "oauth2" {
        validate_external_bearer_token(secret_value)?;
        constants::AUTH_SCHEME_BEARER
    } else {
        let Some(scheme) = security_scheme.scheme.as_deref() else {
            return Ok(());
        };
        scheme
    };

    let auth_value = build_http_authorization_value(scheme_str, secret_value);
    let header_value = HeaderValue::from_str(&auth_value)
        .map_err(|e| Error::invalid_header_value(constants::HEADER_AUTHORIZATION, e.to_string()))?;
    headers.insert(constants::HEADER_AUTHORIZATION, header_value);

    tracing::debug!("Added HTTP authentication header");
    Ok(())
}

/// Adds an authentication header based on a security scheme
fn add_authentication_header(
    headers: &mut HeaderMap,
    security_scheme: &CachedSecurityScheme,
    api_name: &str,
    global_config: Option<&GlobalConfig>,
) -> Result<(), Error> {
    tracing::debug!("Adding authentication header");

    let Some(resolved_secret) =
        resolve_authentication_secret(security_scheme, api_name, global_config)?
    else {
        return Ok(());
    };

    tracing::debug!(source = resolved_secret.source, "Resolved secret");

    validate_header_value(constants::HEADER_AUTHORIZATION, &resolved_secret.value)?;

    match security_scheme.scheme_type.as_str() {
        constants::AUTH_SCHEME_APIKEY => {
            insert_api_key_header(headers, security_scheme, &resolved_secret.value)?;
        }
        "http" | "oauth2" => {
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
    let Some(key) = idempotency_key else {
        return Ok(());
    };
    if key.trim().is_empty() {
        return Err(Error::invalid_idempotency_key());
    }
    headers.insert(
        HeaderName::from_static("idempotency-key"),
        HeaderValue::from_str(key).map_err(|_| Error::invalid_idempotency_key())?,
    );
    Ok(())
}

async fn cached_execution_result(
    cache_context: Option<&(CacheKey, ResponseCache)>,
    max_response_bytes: usize,
    diagnostics_sensitive: bool,
) -> Result<Option<ExecutionResult>, Error> {
    if let Some(cached_response) = check_cache(cache_context, max_response_bytes).await? {
        return Ok(Some(ExecutionResult::Cached {
            body: cached_response.body,
            status: cached_response.status_code,
            headers: cached_response.headers,
            diagnostics_sensitive,
        }));
    }

    Ok(None)
}

#[allow(clippy::too_many_arguments)]
fn build_dry_run_result(
    dry_run: bool,
    method: &Method,
    url: &str,
    headers: &HeaderMap,
    body: Option<&RequestBody>,
    spec: &CachedSpec,
    operation: &CachedCommand,
    proxy: &ProxyDiagnostics,
    secret_ctx: &logging::SecretContext,
) -> Option<ExecutionResult> {
    if !dry_run {
        return None;
    }

    // Omit names and values: outgoing metadata can contain transformed secrets.
    let headers_map: HashMap<String, String> = headers
        .iter()
        .filter(|_| !secret_ctx.is_authenticated())
        .map(|(k, v)| {
            let value = logging::redact_operation_header_value(
                k.as_str(),
                v.to_str().unwrap_or("<binary>"),
                Some(secret_ctx),
                Some((spec, operation)),
            );
            (k.as_str().to_string(), value)
        })
        .collect();

    let body_info = match body {
        Some(RequestBody::Json(_)) if secret_ctx.is_authenticated() => {
            serde_json::Value::String("<authenticated request body omitted>".to_string())
        }
        Some(RequestBody::Json(source)) => serde_json::Value::String(source.clone()),
        Some(RequestBody::Binary(bytes)) => serde_json::json!({
            "binary": true,
            "byte_count": bytes.len()
        }),
        None => serde_json::Value::Null,
    };
    let request_info = serde_json::json!({
        "dry_run": true,
        "method": method.to_string(),
        "url": secret_ctx.diagnostic_url(url, Some((spec, operation))),
        "headers": headers_map,
        "body": body_info,
        "operation_id": operation.operation_id,
        "proxy": proxy.to_json()
    });

    Some(ExecutionResult::DryRun { request_info })
}

#[allow(clippy::too_many_arguments)]
async fn finalize_execution_result(
    status: reqwest::StatusCode,
    response_headers: HashMap<String, String>,
    response_bytes: Vec<u8>,
    spec: &CachedSpec,
    operation: &CachedCommand,
    method: Method,
    url: String,
    headers: &HeaderMap,
    body: Option<&RequestBody>,
    cache_context: Option<(CacheKey, ResponseCache)>,
    cache_config: Option<&CacheConfig>,
    secret_ctx: &logging::SecretContext,
) -> Result<ExecutionResult, Error> {
    if !status.is_success() {
        let error_body = if operation.has_binary_response() {
            format!("<{} binary response bytes>", response_bytes.len())
        } else {
            secret_ctx.diagnostic_body(&String::from_utf8_lossy(&response_bytes))
        };
        return Err(handle_http_error(status, error_body, spec, operation));
    }

    if operation.has_binary_response() {
        return Ok(ExecutionResult::Binary {
            body: response_bytes,
            status: status.as_u16(),
            headers: response_headers,
        });
    }

    let response_text = String::from_utf8_lossy(&response_bytes).into_owned();
    store_in_cache(
        cache_context,
        &response_text,
        status,
        &response_headers,
        method,
        url,
        headers,
        body.and_then(RequestBody::as_json),
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
            diagnostics_sensitive: secret_ctx.is_authenticated(),
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
struct PreExecutionInput<'a> {
    max_response_bytes: usize,
    cache_context: Option<&'a (CacheKey, ResponseCache)>,
    dry_run: bool,
    method: &'a Method,
    url: &'a str,
    headers: &'a HeaderMap,
    body: Option<&'a RequestBody>,
    spec: &'a CachedSpec,
    operation: &'a CachedCommand,
    proxy: &'a ProxyDiagnostics,
    secret_ctx: &'a logging::SecretContext,
}

async fn resolve_pre_execution_result(
    input: PreExecutionInput<'_>,
) -> Result<Option<ExecutionResult>, Error> {
    let dry_run = build_dry_run_result(
        input.dry_run,
        input.method,
        input.url,
        input.headers,
        input.body,
        input.spec,
        input.operation,
        input.proxy,
        input.secret_ctx,
    );
    if dry_run.is_some() {
        return Ok(dry_run);
    }
    cached_execution_result(
        input.cache_context,
        input.max_response_bytes,
        input.secret_ctx.is_authenticated(),
    )
    .await
}

/// Executes an API operation using CLI-agnostic domain types.
///
/// # Errors
///
/// Returns errors for authentication failures, network issues, invalid
/// parameters, and response validation problems.
pub async fn execute(
    spec: &CachedSpec,
    call: crate::invocation::OperationCall,
    ctx: crate::invocation::ExecutionContext,
) -> Result<crate::invocation::ExecutionResult, Error> {
    let max_response_bytes = effective_max_response_bytes(&ctx)?;
    let prepared = prepare_execution(spec, call, &ctx)?;

    if let Some(result) = resolve_pre_execution_result(PreExecutionInput {
        cache_context: prepared.cache_context.as_ref(),
        dry_run: ctx.dry_run,
        method: &prepared.method,
        url: &prepared.url,
        headers: &prepared.headers_clone,
        body: prepared.body.as_ref(),
        spec,
        operation: prepared.operation,
        proxy: &prepared.proxy_diagnostics,
        max_response_bytes,
        secret_ctx: &prepared.secret_ctx,
    })
    .await?
    {
        return Ok(result);
    }

    let (status, response_headers, response_text) = send_request_with_retry(
        prepared
            .client
            .as_ref()
            .ok_or_else(|| Error::invalid_config("Missing HTTP client"))?,
        prepared.method.clone(),
        &prepared.url,
        prepared.headers,
        prepared.body.clone(),
        prepared.retry_ctx.as_ref(),
        spec,
        prepared.operation,
        Some(&prepared.secret_ctx),
        max_response_bytes,
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
        prepared.body.as_ref(),
        prepared.cache_context,
        prepared.cache_config,
        &prepared.secret_ctx,
    )
    .await
}

struct PreparedExecution<'a> {
    operation: &'a CachedCommand,
    method: Method,
    url: String,
    client: Option<reqwest::Client>,
    proxy_diagnostics: ProxyDiagnostics,
    headers: HeaderMap,
    headers_clone: HeaderMap,
    cache_context: Option<(CacheKey, ResponseCache)>,
    retry_ctx: Option<RetryContext>,
    secret_ctx: logging::SecretContext,
    body: Option<RequestBody>,
    cache_config: Option<&'a CacheConfig>,
}

struct PreparedRequest<'a> {
    operation: &'a CachedCommand,
    method: Method,
    url: String,
    client: Option<reqwest::Client>,
    proxy_diagnostics: ProxyDiagnostics,
    headers: HeaderMap,
    headers_clone: HeaderMap,
    body: Option<RequestBody>,
}

struct PreparedRuntimeContext<'a> {
    cache_context: Option<(CacheKey, ResponseCache)>,
    retry_ctx: Option<RetryContext>,
    secret_ctx: logging::SecretContext,
    cache_config: Option<&'a CacheConfig>,
}

fn prepare_execution<'a>(
    spec: &'a CachedSpec,
    call: crate::invocation::OperationCall,
    ctx: &'a crate::invocation::ExecutionContext,
) -> Result<PreparedExecution<'a>, Error> {
    let strict_pagination = ctx.auto_paginate || call.pagination_url.is_some();
    let request = prepare_request(spec, call, ctx)?;
    let runtime = prepare_runtime_context(spec, &request, ctx, strict_pagination)?;

    Ok(PreparedExecution {
        operation: request.operation,
        method: request.method,
        url: request.url,
        client: request.client,
        proxy_diagnostics: request.proxy_diagnostics,
        headers: request.headers,
        headers_clone: request.headers_clone,
        cache_context: runtime.cache_context,
        retry_ctx: runtime.retry_ctx,
        secret_ctx: runtime.secret_ctx,
        body: request.body,
        cache_config: runtime.cache_config,
    })
}

fn validate_binary_request_body(body: &RequestBody) -> Result<(), Error> {
    match body {
        RequestBody::Binary(_) => Ok(()),
        RequestBody::Json(_) => Err(Error::validation_error(
            "JSON request text does not match the operation's declared request body",
        )),
    }
}

fn validate_json_request_body(body: &RequestBody) -> Result<(), Error> {
    let RequestBody::Json(source) = body else {
        return Err(Error::validation_error(
            "Binary request bytes do not match the operation's declared request body",
        ));
    };
    serde_json::from_str::<Value>(source)
        .map(|_| ())
        .map_err(|error| Error::validation_error(format!("Invalid JSON request body: {error}")))
}

fn validate_declared_request_body(
    declared: &crate::cache::models::CachedRequestBody,
    body: Option<&RequestBody>,
) -> Result<(), Error> {
    let Some(body) = body else {
        return if declared.required {
            Err(Error::validation_error(
                "This operation requires a request body",
            ))
        } else {
            Ok(())
        };
    };
    if declared.is_binary() {
        return validate_binary_request_body(body);
    }
    if declared.is_json() {
        return validate_json_request_body(body);
    }
    Err(Error::validation_error(
        "The operation's declared request body is not supported",
    ))
}

fn validate_operation_call_body(
    operation: &CachedCommand,
    body: Option<&RequestBody>,
) -> Result<(), Error> {
    if operation.has_ambiguous_binary_response() {
        return Err(Error::validation_error(
            "Operation mixes binary and non-binary successful responses; execution is blocked before network access",
        ));
    }
    if let Some(declared) = operation.request_body.as_ref() {
        return validate_declared_request_body(declared, body);
    }
    if body.is_some() {
        return Err(Error::validation_error(
            "This operation does not declare a request body",
        ));
    }
    Ok(())
}

fn find_validated_operation<'a>(
    spec: &'a CachedSpec,
    call: &crate::invocation::OperationCall,
) -> Result<&'a CachedCommand, Error> {
    let operation = find_operation_by_id(spec, &call.operation_id)?;
    validate_operation_call_body(operation, call.body.as_ref())?;
    Ok(operation)
}

fn prepare_transport(
    ctx: &crate::invocation::ExecutionContext,
    pagination: bool,
) -> Result<(Option<reqwest::Client>, ProxyDiagnostics), Error> {
    if ctx.dry_run {
        let (_, diagnostics) = configure_proxy(reqwest::Client::builder(), ctx)?;
        return Ok((None, diagnostics));
    }
    let result = build_http_client(ctx, pagination)?;
    Ok((Some(result.client), result.diagnostics))
}

fn prepare_request<'a>(
    spec: &'a CachedSpec,
    call: crate::invocation::OperationCall,
    ctx: &'a crate::invocation::ExecutionContext,
) -> Result<PreparedRequest<'a>, Error> {
    let operation = find_validated_operation(spec, &call)?;
    let url = pagination_request_url(spec, &call, ctx)?.to_string();
    let pagination = ctx.auto_paginate || call.pagination_url.is_some();
    let (client, proxy_diagnostics) = prepare_transport(ctx, pagination)?;
    let mut headers = build_headers_from_params(
        spec,
        operation,
        &call.header_params,
        &call.custom_headers,
        call.body.is_some(),
        &spec.name,
        ctx.global_config.as_ref(),
    )?;
    add_idempotency_key(&mut headers, ctx.idempotency_key.as_ref())?;
    let method = Method::from_str(&operation.method).map_err(|_| {
        Error::invalid_http_method(&operation.method).omit_diagnostic_inputs(
            "Invalid request HTTP method",
            "Use a valid HTTP method in the operation definition.",
        )
    })?;
    let headers_clone = headers.clone();

    Ok(PreparedRequest {
        operation,
        method,
        url,
        client,
        proxy_diagnostics,
        headers,
        headers_clone,
        body: call.body,
    })
}

/// Detect selected transport authentication without resolving credentials or
/// emitting resolver errors. A userinfo delimiter in the authority is treated
/// conservatively even when a template or malformed URL cannot yet be parsed.
pub(crate) fn preparation_transport_is_sensitive(
    spec: &CachedSpec,
    ctx: &crate::invocation::ExecutionContext,
) -> bool {
    let base = resolve_base_url_resolver(spec, ctx.global_config.as_ref())
        .resolve_basic(ctx.base_url.as_deref());
    let userinfo = base.split_once("://").is_some_and(|(_, rest)| {
        rest.split(['/', '?', '#'])
            .next()
            .is_some_and(|authority| authority.contains('@'))
    });
    userinfo || proxy_requires_cache_bypass(ctx)
}

fn execution_bypasses_cache(
    operation: &CachedCommand,
    ctx: &crate::invocation::ExecutionContext,
) -> bool {
    ctx.dry_run || !operation.security_requirements.is_empty() || proxy_requires_cache_bypass(ctx)
}

fn prepare_runtime_context<'a>(
    spec: &'a CachedSpec,
    request: &PreparedRequest<'a>,
    ctx: &'a crate::invocation::ExecutionContext,
    strict_pagination: bool,
) -> Result<PreparedRuntimeContext<'a>, Error> {
    let operation = request.operation;
    let method = &request.method;
    let url = &request.url;
    let headers = &request.headers_clone;
    let body = request.body.as_ref();
    if operation.has_binary_io()
        && ctx
            .cache_config
            .as_ref()
            .is_some_and(|config| config.enabled)
    {
        return Err(Error::validation_error(
            "--cache is not supported for operations with binary request or response bodies",
        ));
    }
    // Strict pagination cannot read bodies/Link headers from ordinary requests
    // that may have followed a redirect under the original URL. Partition both
    // policies. The proxy policy revision also misses entries that older
    // executors may have populated with authenticated proxy responses.
    let cache_context = prepare_cache_context(
        if execution_bypasses_cache(operation, ctx) {
            None
        } else {
            ctx.cache_config.as_ref()
        },
        &spec.name,
        &format!(
            "{}:redirects={}:origin-bound=v1:proxy-auth-bypass=v2",
            operation.operation_id, !strict_pagination
        ),
        method,
        url,
        headers,
        body.and_then(RequestBody::as_json),
    )?;
    let retry_ctx = ctx.retry_context.clone().map(|mut rc| {
        rc.method = Some(method.to_string());
        // SDK context fields can disagree; only a key actually sent on the
        // request authorizes retries of non-idempotent methods.
        rc.has_idempotency_key = headers
            .get("idempotency-key")
            .is_some_and(|value| !value.as_bytes().iter().all(u8::is_ascii_whitespace));
        rc
    });
    let secret_ctx =
        logging::SecretContext::from_spec_and_config(spec, &spec.name, ctx.global_config.as_ref())
            .with_active_operation_headers(spec, operation, headers)
            .with_request_url(url)
            .with_authenticated_transport(proxy_requires_cache_bypass(ctx));

    let secret_ctx = with_selected_proxy_secrets(secret_ctx, ctx);

    Ok(PreparedRuntimeContext {
        cache_context,
        retry_ctx,
        secret_ctx,
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
    parameters: &[crate::cache::models::CachedParameter],
) -> Result<String, Error> {
    let template = format!("{}{}", base_url.trim_end_matches('/'), path_template);
    let url = super::url_serialization::expand_path_template(&template, path_params, parameters)?;
    super::url_serialization::validate_path_segments(&url)?;
    let mut url = reqwest::Url::parse(&url)
        .map_err(|e| Error::validation_error(format!("Invalid request URL: {e}")))?;
    if !query_params.is_empty() {
        let pairs = super::url_serialization::query_parameters(parameters, query_params)?;
        url.query_pairs_mut().extend_pairs(pairs);
    }
    Ok(url.to_string())
}

/// Resolve the effective URL without constructing a client or reading secrets.
pub(crate) fn pagination_request_url(
    spec: &CachedSpec,
    call: &crate::invocation::OperationCall,
    ctx: &crate::invocation::ExecutionContext,
) -> Result<reqwest::Url, Error> {
    let operation = find_operation_by_id(spec, &call.operation_id)?;
    let sensitive = logging::custom_headers_reference_environment(&call.custom_headers)
        || preparation_transport_is_sensitive(spec, ctx)
        || logging::operation_preparation_is_sensitive(
            spec,
            operation,
            call.header_params.keys().map(String::as_str).chain(
                call.custom_headers
                    .iter()
                    .filter_map(|header| header.split_once(':').map(|(name, _)| name.trim())),
            ),
        );
    resolve_operation_request_url(spec, operation, call, ctx).map_err(|error| {
        if sensitive {
            error.omit_diagnostic_inputs(
                "Invalid request URL or URL parameters (input omitted)",
                "Check the base URL, path/query parameters, server variables and pagination origin.",
            )
        } else {
            error
        }
    })
}

fn resolve_operation_request_url(
    spec: &CachedSpec,
    operation: &CachedCommand,
    call: &crate::invocation::OperationCall,
    ctx: &crate::invocation::ExecutionContext,
) -> Result<reqwest::Url, Error> {
    let resolver = resolve_base_url_resolver(spec, ctx.global_config.as_ref());
    let base = resolver.resolve_with_variables(ctx.base_url.as_deref(), &ctx.server_var_args)?;
    let url = build_url_from_params(
        &base,
        &operation.path,
        &call.path_params,
        &call.query_params,
        &operation.parameters,
    )?;
    validate_pagination_url(&url, call.pagination_url.as_ref())
}

/// Credentials and operation headers are retained only within the original origin.
fn validate_pagination_url(
    original: &str,
    target: Option<&reqwest::Url>,
) -> Result<reqwest::Url, Error> {
    let original = reqwest::Url::parse(original)
        .map_err(|e| Error::validation_error(format!("Invalid request URL: {e}")))?;
    let Some(target) = target else {
        return Ok(original);
    };
    validate_pagination_target(&original, target)?;
    Ok(target.clone())
}

/// Validate before attaching any operation authentication or custom headers.
pub(crate) fn validate_pagination_target(
    original: &reqwest::Url,
    target: &reqwest::Url,
) -> Result<(), Error> {
    if target.origin() != original.origin()
        || !target.username().is_empty()
        || target.password().is_some()
        || target.fragment().is_some()
    {
        return Err(Error::validation_error(
            "Pagination next URL must be same-origin and contain no credentials or fragment",
        ));
    }
    Ok(())
}

/// Builds HTTP headers from pre-extracted header parameter maps.
#[allow(clippy::too_many_arguments)]
fn build_headers_from_params(
    spec: &CachedSpec,
    operation: &CachedCommand,
    header_params: &HashMap<String, String>,
    custom_headers: &[String],
    has_body: bool,
    api_name: &str,
    global_config: Option<&GlobalConfig>,
) -> Result<HeaderMap, Error> {
    let mut headers = default_request_headers();
    apply_operation_media_headers(&mut headers, operation, has_body)?;
    apply_header_parameters(&mut headers, header_params)?;
    apply_security_headers(&mut headers, spec, operation, api_name, global_config)?;
    apply_custom_headers(&mut headers, custom_headers)?;
    Ok(headers)
}

fn apply_operation_media_headers(
    headers: &mut HeaderMap,
    operation: &CachedCommand,
    has_body: bool,
) -> Result<(), Error> {
    if let Some(request_body) = operation.request_body.as_ref().filter(|_| has_body) {
        headers.insert(
            constants::HEADER_CONTENT_TYPE,
            HeaderValue::from_str(&request_body.content_type).map_err(|e| {
                Error::invalid_header_value(constants::HEADER_CONTENT_TYPE, e.to_string())
            })?,
        );
    }
    if let Some(content_type) = operation.binary_response_content_type() {
        headers.insert(
            constants::HEADER_ACCEPT,
            HeaderValue::from_str(content_type).map_err(|e| {
                Error::invalid_header_value(constants::HEADER_ACCEPT, e.to_string())
            })?,
        );
    }
    Ok(())
}

fn default_request_headers() -> HeaderMap {
    let mut headers = HeaderMap::new();
    headers.insert("User-Agent", HeaderValue::from_static(DEFAULT_USER_AGENT));
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
    if operation.security_requirements.is_empty() {
        return Ok(());
    }
    let mut unavailable = None;
    for group in &operation.security_requirements {
        if let Some(error) = security_group_unavailable(group, spec, api_name, global_config)? {
            unavailable.get_or_insert(error);
        } else {
            let mut selected = headers.clone();
            for name in group {
                let scheme = &spec.security_schemes[name];
                add_authentication_header(&mut selected, scheme, api_name, global_config)?;
            }
            *headers = selected;
            return Ok(());
        }
    }
    Err(unavailable
        .unwrap_or_else(|| Error::validation_error("No satisfiable security alternative")))
}

/// Check credential availability, including `OAuth2` token syntax, before selection.
/// Structural cache errors still propagate rather than weakening requirements.
fn security_group_unavailable(
    group: &[String],
    spec: &CachedSpec,
    api_name: &str,
    global_config: Option<&GlobalConfig>,
) -> Result<Option<Error>, Error> {
    validate_security_group(group, spec)?;
    for name in group {
        let scheme = &spec.security_schemes[name];
        let env_name = authentication_env_name(scheme, api_name, global_config);
        let Some(env_name) = env_name else {
            return Ok(Some(Error::validation_error(
                "No credential configured for security scheme (name omitted)",
            )));
        };
        match std::env::var(env_name) {
            Ok(value) if scheme.scheme_type == "oauth2" => {
                if let Err(error) = validate_external_bearer_token(&value) {
                    return Ok(Some(error));
                }
            }
            Ok(_) => {}
            Err(std::env::VarError::NotPresent) => {
                return Ok(Some(Error::secret_not_set(name, env_name).omit_diagnostic_inputs(
                    "Required authentication secret not set (environment variable unavailable)",
                    "Check the operation's credential mapping and environment variable availability.",
                )))
            }
            // VarError's Display includes the raw non-Unicode value, which is
            // a credential here. Report the configuration error without it.
            Err(std::env::VarError::NotUnicode(_)) => {
                return Err(Error::validation_error(
                    "Credential for security scheme is not valid unicode",
                ))
            }
        }
    }
    Ok(None)
}

/// Reject groups we cannot apply completely before checking credentials. Two
/// schemes targeting the same header cannot both be satisfied by overwriting it.
fn validate_security_group(group: &[String], spec: &CachedSpec) -> Result<(), Error> {
    let mut destinations = std::collections::HashSet::new();
    for name in group {
        let scheme = spec
            .security_schemes
            .get(name)
            .ok_or_else(|| Error::validation_error("Unknown security scheme (name omitted)"))?;
        let destination = security_header_destination(scheme)?;
        if !destinations.insert(destination) {
            return Err(Error::validation_error(
                "Security group contains conflicting authentication headers",
            ));
        }
    }
    Ok(())
}

fn security_header_destination(scheme: &CachedSecurityScheme) -> Result<HeaderName, Error> {
    match scheme.scheme_type.as_str() {
        constants::AUTH_SCHEME_APIKEY => {
            if scheme.location.as_deref() != Some("header") {
                return Err(Error::unsupported_security_scheme("apiKey outside headers"));
            }
            let name = scheme.parameter_name.as_deref().ok_or_else(|| {
                Error::validation_error("API key security scheme has no header name")
            })?;
            HeaderName::from_str(name)
                .map_err(|error| Error::invalid_header_name(name, error.to_string()))
        }
        "http" | "oauth2" => authorization_destination(scheme),
        other => Err(
            Error::unsupported_security_scheme(other).omit_diagnostic_inputs(
                "Unsupported security scheme type (input omitted)",
                "Use header apiKey, HTTP or external OAuth2 authentication.",
            ),
        ),
    }
}

fn authorization_destination(scheme: &CachedSecurityScheme) -> Result<HeaderName, Error> {
    if scheme.scheme_type == "http" && scheme.scheme.as_deref().is_none_or(str::is_empty) {
        return Err(Error::validation_error(
            "HTTP security scheme has no authentication scheme",
        ));
    }
    Ok(HeaderName::from_static("authorization"))
}

/// Configured secrets take precedence over specification extensions.
fn authentication_env_name<'a>(
    scheme: &'a CachedSecurityScheme,
    api_name: &str,
    config: Option<&'a GlobalConfig>,
) -> Option<&'a String> {
    config
        .and_then(|config| config.api_configs.get(api_name))
        .and_then(|config| config.secrets.get(&scheme.name))
        .map(|secret| &secret.name)
        .or_else(|| scheme.aperture_secret.as_ref().map(|secret| &secret.name))
}

fn apply_custom_headers(headers: &mut HeaderMap, custom_headers: &[String]) -> Result<(), Error> {
    for header_str in custom_headers {
        let (name, value) = parse_custom_header(header_str)?;
        headers.insert(name, value);
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

/// Filter caller-supplied text with an explicit diagnostic sensitivity policy.
///
/// Successful output stays exact; sensitive failures never retain input, filter
/// source or parser/runtime errors. The context-free helper remains for owned
/// anonymous data; operation renderers use this policy-aware boundary.
///
/// # Errors
/// Returns a Validation error with a static JQ hint on sensitive failures.
pub fn apply_jq_filter_with_diagnostics(
    response_text: &str,
    filter: &str,
    diagnostics_sensitive: bool,
) -> Result<String, Error> {
    apply_jq_filter(response_text, filter).map_err(|error| {
        if diagnostics_sensitive {
            error.omit_diagnostic_inputs(
                "JQ filter error (authenticated input omitted)",
                "Check JQ filter syntax and data structure compatibility.",
            )
        } else {
            error
        }
    })
}

#[cfg(feature = "jq")]
fn apply_jq_filter_value(json_value: Value, filter: &str) -> Result<String, Error> {
    // Use jaq v3.x (pure Rust implementation)
    use jaq_core::load::{Arena, File, Loader};
    use jaq_core::Compiler;

    let program = File {
        code: filter,
        path: (),
    };

    let defs: Vec<_> = jaq_core::defs()
        .chain(jaq_std::defs())
        .chain(jaq_json::defs())
        .collect();
    let funs: Vec<_> = jaq_core::funs::<data::JustLut<Val>>()
        .chain(jaq_std::funs())
        .chain(jaq_json::funs())
        .collect();

    let loader = Loader::new(defs);
    let arena = Arena::default();

    let modules = loader
        .load(&arena, program)
        .map_err(|errs| Error::jq_filter_error(filter, format!("Parse error: {errs:?}")))?;

    let filter_fn = Compiler::default()
        .with_funs(funs)
        .compile(modules)
        .map_err(|errs| Error::jq_filter_error(filter, format!("Compilation error: {errs:?}")))?;

    let jaq_value: Val = serde_json::from_value(json_value)
        .map_err(|e| Error::serialization_error(format!("Failed to convert filter input: {e}")))?;
    let ctx = Ctx::<data::JustLut<Val>>::new(&filter_fn.lut, Vars::new([]));
    let output = filter_fn.id.run((ctx, jaq_value)).map(unwrap_valr);
    let results: Result<Vec<Val>, _> = output.collect();

    format_jaq_results(results, filter)
}

#[cfg(feature = "jq")]
fn format_jaq_results<E: std::fmt::Display>(
    results: Result<Vec<Val>, E>,
    filter: &str,
) -> Result<String, Error> {
    match results {
        Ok(vals) if vals.is_empty() => Ok(constants::NULL_VALUE.to_string()),
        Ok(vals) if vals.len() == 1 => format_single_jaq_result(&vals[0]),
        Ok(vals) => format_multiple_jaq_results(vals),
        Err(e) => Err(Error::jq_filter_error(
            format!("{filter:?}"),
            format!("Filter execution error: {e}"),
        )),
    }
}

#[cfg(feature = "jq")]
fn format_single_jaq_result(val: &Val) -> Result<String, Error> {
    let json_val: Value = serde_json::from_str(&val.to_string())
        .map_err(|e| Error::serialization_error(format!("Failed to convert result: {e}")))?;
    serde_json::to_string_pretty(&json_val)
        .map_err(|e| Error::serialization_error(format!("Failed to serialize result: {e}")))
}

#[cfg(feature = "jq")]
fn format_multiple_jaq_results(vals: Vec<Val>) -> Result<String, Error> {
    let json_vals: Vec<Value> = vals
        .into_iter()
        .map(|val| serde_json::from_str(&val.to_string()))
        .collect::<Result<_, _>>()
        .map_err(|e| Error::serialization_error(format!("Failed to convert results: {e}")))?;
    serde_json::to_string_pretty(&Value::Array(json_vals))
        .map_err(|e| Error::serialization_error(format!("Failed to serialize results: {e}")))
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
    field_path
        .split('.')
        .filter(|part| !part.is_empty())
        .try_fold(json_value, resolve_nested_field_segment)
        .cloned()
        .unwrap_or(Value::Null)
}

#[cfg(not(feature = "jq"))]
fn resolve_nested_field_segment<'a>(current: &'a Value, part: &str) -> Option<&'a Value> {
    if let Some(index) = parse_bracket_index(part) {
        return current.as_array().and_then(|arr| arr.get(index));
    }

    match current {
        Value::Object(obj) => obj.get(part),
        Value::Array(arr) => part.parse::<usize>().ok().and_then(|index| arr.get(index)),
        _ => None,
    }
}

#[cfg(not(feature = "jq"))]
fn parse_bracket_index(part: &str) -> Option<usize> {
    if part.starts_with('[') && part.ends_with(']') {
        part[1..part.len() - 1].parse::<usize>().ok()
    } else {
        None
    }
}

#[cfg(test)]
#[path = "operation_redirect_tests.rs"]
mod operation_redirect_tests;

#[cfg(test)]
mod tests {
    #[test]
    fn environment_header_provenance_survives_clone_and_final_overrides() {
        let variable = "APERTURE_265_PROVENANCE_TEST";
        std::env::set_var(variable, "synthetic-265");
        let mut headers = HeaderMap::new();
        apply_custom_headers(&mut headers, &[format!("X-Unknown: ${{{variable}}}")]).unwrap();
        std::env::remove_var(variable);
        assert!(headers["X-Unknown"].is_sensitive());
        assert!(headers.clone()["X-Unknown"].is_sensitive());
        assert!(request_requires_cache_bypass(
            &headers,
            "http://localhost/test"
        ));
        let spec = security_test_spec();
        let context = logging::SecretContext::empty().with_active_operation_headers(
            &spec,
            &spec.commands[0],
            &headers,
        );
        assert!(context.is_authenticated());
        apply_custom_headers(&mut headers, &["X-Unknown: public".into()]).unwrap();
        assert!(!headers["X-Unknown"].is_sensitive());
        assert!(!request_requires_cache_bypass(
            &headers,
            "http://localhost/test"
        ));
    }

    #[test]
    fn environment_header_references_fail_closed() {
        for value in [
            "${}",
            "${APERTURE_265_MISSING}",
            "${UNCLOSED",
            "${BAD NAME}",
        ] {
            assert!(
                apply_custom_headers(&mut HeaderMap::new(), &[format!("X-Unknown: {value}")])
                    .is_err()
            );
        }
    }

    #[test]
    fn environment_header_multiple_unicode_and_literal_compatibility() {
        let first = "APERTURE_265_MULTIPLE_FIRST";
        let second = "APERTURE_265_MULTIPLE_SECOND";
        std::env::set_var(first, "synthetic-é");
        std::env::set_var(second, "second");
        let (name, value) = parse_custom_header(&format!(
            "X-Unknown: prefix ${{{first}}}/${{{second}}} suffix"
        ))
        .unwrap();
        std::env::remove_var(first);
        std::env::remove_var(second);
        assert_eq!(name, "x-unknown");
        assert_eq!(
            value.as_bytes(),
            "prefix synthetic-é/second suffix".as_bytes()
        );
        assert!(value.is_sensitive());
        for literal in ["", "$NAME", "{NAME}", "dollar$", "none", "foo:bar"] {
            let (_, value) = parse_custom_header(&format!("X-Literal: {literal}")).unwrap();
            assert_eq!(value.as_bytes(), literal.as_bytes());
            assert!(!value.is_sensitive());
        }
    }

    #[test]
    fn environment_header_empty_and_control_values_are_input_free() {
        let variable = "APERTURE_265_INVALID_VALUE";
        for value in [
            "",
            "synthetic-secret\r",
            "synthetic-secret\n",
            "synthetic-secret\u{1}",
        ] {
            std::env::set_var(variable, value);
            let error = parse_custom_header(&format!("X-Unknown: ${{{variable}}}")).unwrap_err();
            let diagnostic = format!("{error:?} {error}");
            assert!(!diagnostic.contains("synthetic-secret"));
            assert!(!diagnostic.contains(variable));
        }
        std::env::remove_var(variable);
    }

    #[cfg(unix)]
    #[test]
    fn environment_header_non_unicode_value_is_input_free() {
        use std::os::unix::ffi::OsStringExt;
        let variable = "APERTURE_265_NON_UNICODE_VALUE";
        std::env::set_var(
            variable,
            std::ffi::OsString::from_vec(b"synthetic-secret-\xff".to_vec()),
        );
        let error = parse_custom_header(&format!("X-Unknown: ${{{variable}}}")).unwrap_err();
        std::env::remove_var(variable);
        let diagnostic = format!("{error:?} {error}");
        assert!(!diagnostic.contains("synthetic-secret"));
        assert!(!diagnostic.contains(variable));
    }

    #[test]
    fn environment_header_explicit_metadata_without_ascii_conversion() {
        let mut value = HeaderValue::from_bytes(b"\xff").unwrap();
        value.set_sensitive(true);
        let mut headers = HeaderMap::new();
        headers.insert("x-unknown", value);
        let spec = security_test_spec();
        assert!(logging::SecretContext::empty()
            .with_active_operation_headers(&spec, &spec.commands[0], &headers)
            .is_authenticated());
        assert!(request_requires_cache_bypass(
            &headers,
            "http://localhost/test"
        ));
    }

    fn security_test_spec() -> CachedSpec {
        let document = serde_json::json!({
            "openapi":"3.0.3", "info":{"title":"Security", "version":"1"},
            "paths":{"/test":{"get":{"operationId":"test", "security":[{"available":[]},{"missing":[]}], "responses":{}}}},
            "components":{"securitySchemes":{
                "available":{"type":"apiKey","in":"header","name":"X-Available","x-aperture-secret":{"source":"env","name":"PATH"}},
                "second":{"type":"apiKey","in":"header","name":"X-Second","x-aperture-secret":{"source":"env","name":"PATH"}},
                "missing":{"type":"apiKey","in":"header","name":"X-Missing","x-aperture-secret":{"source":"env","name":"APERTURE_SECURITY_TEST_UNSET_221"}}
            }}
        });
        let openapi = serde_json::from_value(document).unwrap();
        crate::spec::SpecTransformer::new()
            .transform("security", &openapi)
            .unwrap()
    }

    #[cfg(unix)]
    #[test]
    fn security_non_unicode_credentials_do_not_leak_in_errors() {
        use std::os::unix::ffi::OsStringExt;
        let env_name = "APERTURE_SECURITY_NON_UNICODE_C2";
        let mut spec = security_test_spec();
        spec.security_schemes
            .get_mut("available")
            .unwrap()
            .aperture_secret
            .as_mut()
            .unwrap()
            .name = env_name.into();
        std::env::set_var(
            env_name,
            std::ffi::OsString::from_vec(b"synthetic-secret-\xff".to_vec()),
        );
        let result = apply_security_headers(
            &mut HeaderMap::new(),
            &spec,
            &spec.commands[0],
            "security",
            None,
        );
        std::env::remove_var(env_name);
        let error = result.unwrap_err().to_string();
        assert!(
            !error.contains("synthetic-secret"),
            "credential leaked: {error}"
        );
        assert!(error.contains("unicode"));
    }

    #[test]
    fn security_selects_one_alternative_without_other_credentials() {
        let spec = security_test_spec();
        let mut operation = spec.commands[0].clone();
        let mut headers = HeaderMap::new();
        apply_security_headers(&mut headers, &spec, &operation, "security", None).unwrap();
        assert!(headers.contains_key("X-Available"));
        assert!(!headers.contains_key("X-Missing"));
        operation.security_requirements.reverse();
        headers.clear();
        apply_security_headers(&mut headers, &spec, &operation, "security", None).unwrap();
        assert!(headers.contains_key("X-Available"));
    }

    #[test]
    fn security_combined_group_is_atomic_and_empty_group_is_optional() {
        let spec = security_test_spec();
        let mut operation = spec.commands[0].clone();
        operation.security_requirements = vec![vec!["available".into(), "missing".into()]];
        let mut headers = HeaderMap::new();
        assert!(apply_security_headers(&mut headers, &spec, &operation, "security", None).is_err());
        assert!(headers.is_empty());
        operation.security_requirements.push(vec![]);
        apply_security_headers(&mut headers, &spec, &operation, "security", None).unwrap();
        assert!(headers.is_empty());
        operation.security_requirements = vec![vec!["available".into(), "second".into()]];
        apply_security_headers(&mut headers, &spec, &operation, "security", None).unwrap();
        assert!(headers.contains_key("X-Available"));
        assert!(headers.contains_key("X-Second"));
    }

    #[test]
    fn security_rejects_unapplied_or_conflicting_credentials() {
        for location in ["query", "cookie"] {
            let mut spec = security_test_spec();
            spec.security_schemes.get_mut("available").unwrap().location = Some(location.into());
            let mut headers = HeaderMap::new();
            assert!(apply_security_headers(
                &mut headers,
                &spec,
                &spec.commands[0],
                "security",
                None
            )
            .is_err());
            assert!(headers.is_empty());
        }
        let mut spec = security_test_spec();
        spec.security_schemes
            .get_mut("second")
            .unwrap()
            .parameter_name = Some("x-available".into());
        let mut operation = spec.commands[0].clone();
        operation.security_requirements = vec![vec!["available".into(), "second".into()]];
        assert!(
            apply_security_headers(&mut HeaderMap::new(), &spec, &operation, "security", None)
                .is_err()
        );
    }

    #[test]
    fn security_does_not_hide_invalid_header_or_unknown_scheme() {
        let mut spec = security_test_spec();
        let mut operation = spec.commands[0].clone();
        operation.security_requirements.push(vec![]);
        spec.security_schemes
            .get_mut("available")
            .unwrap()
            .parameter_name = Some("invalid\nheader".into());
        assert!(
            apply_security_headers(&mut HeaderMap::new(), &spec, &operation, "security", None)
                .is_err()
        );
        operation.security_requirements = vec![vec!["unknown".into()], vec![]];
        assert!(
            apply_security_headers(&mut HeaderMap::new(), &spec, &operation, "security", None)
                .is_err()
        );
    }

    use super::*;

    #[test]
    fn url_userinfo_and_cookies_disable_cache() {
        assert!(request_requires_cache_bypass(
            &HeaderMap::new(),
            "https://alice:secret@example.com/items"
        ));
        let mut headers = HeaderMap::new();
        headers.insert("cookie", "session=secret".parse().unwrap());
        assert!(request_requires_cache_bypass(
            &headers,
            "https://example.com/items"
        ));
        assert!(!request_requires_cache_bypass(
            &HeaderMap::new(),
            "https://example.com/items"
        ));
        let mut duplicates = HeaderMap::new();
        duplicates.append("x-tenant-secret", "alice".parse().unwrap());
        duplicates.append("x-tenant-secret", "bob".parse().unwrap());
        assert!(request_requires_cache_bypass(
            &duplicates,
            "https://example.com/items"
        ));
    }

    #[test]
    fn proxy_credential_detection_covers_scheme_less_authorities() {
        for value in [
            "http://alice:synthetic@localhost:8080",
            "alice:synthetic@localhost:8080",
            "alice@localhost:8080",
            ":synthetic@localhost:8080",
            "http://:synthetic@localhost:8080",
            "socks5://alice:synthetic@localhost:1080",
        ] {
            assert!(proxy_url_has_credentials(value), "{value}");
        }
        for value in ["http://localhost:8080", "localhost:8080", "", " "] {
            assert!(!proxy_url_has_credentials(value), "{value}");
        }
    }

    #[test]
    fn selected_proxy_forms_follow_overrides_and_config_rotation() {
        let password_env = "APERTURE_REPAIR264_PROXY_PASSWORD";
        let mut config = GlobalConfig::default();
        config.proxy.http = Some("http://localhost:8080".into());
        config.proxy.username = Some("proxy".into());
        config.proxy.password_env = Some(password_env.into());
        std::env::set_var(password_env, "first-secret-264");
        let first = with_config_proxy_secrets(logging::SecretContext::empty(), Some(&config));
        std::env::set_var(password_env, "second-secret-264");
        let second = with_config_proxy_secrets(logging::SecretContext::empty(), Some(&config));
        std::env::remove_var(password_env);
        assert!(first.is_secret("first-secret-264"));
        assert!(!second.is_secret("first-secret-264"));
        assert!(second.is_secret("second-secret-264"));
        let context = crate::invocation::ExecutionContext {
            global_config: Some(config),
            proxy_override: ProxyOverride::Disable,
            ..Default::default()
        };
        assert!(
            !with_selected_proxy_secrets(logging::SecretContext::empty(), &context)
                .is_authenticated()
        );
        let context = crate::invocation::ExecutionContext {
            proxy_override: ProxyOverride::Use("proxy:selected-secret-264@localhost:8080".into()),
            ..context
        };
        let selected = with_selected_proxy_secrets(logging::SecretContext::empty(), &context);
        assert!(selected.is_secret("selected-secret-264"));
        assert!(!selected.is_secret("second-secret-264"));
    }

    #[test]
    fn proxy_metadata_omits_untrusted_routes_before_auth_context() {
        let diagnostics = ProxyDiagnostics {
            source: "environment",
            all: Some("http://u:proxy-fresh-264@proxy/proxy-fresh-264?echo=462-hserf-yxorp".into()),
            http: Some("http://café-secret.example/%63af%C3%A9?echo=Y2Fmw6k=#secret".into()),
            https: Some("unknown://u:secret@[bad/secret".into()),
            no_proxy: vec!["secret.example".into(), "462-hserf-yxorp".into()],
            ..Default::default()
        };
        let output = diagnostics.to_json();
        assert_eq!(output["all"], "[PROXY URL OMITTED]");
        assert_eq!(output["http"], "[PROXY URL OMITTED]");
        assert_eq!(output["https"], "[PROXY URL OMITTED]");
        assert_eq!(output["no_proxy"], serde_json::json!([]));
        assert_eq!(output["no_proxy_count"], 2);
        let debug = format!("{diagnostics:?}");
        assert!(!debug.contains("secret"));
        assert!(!debug.contains("proxy-fresh-264"));
        assert!(!debug.contains("462-hserf-yxorp"));
    }

    #[test]
    fn invalid_proxy_errors_omit_all_input_not_only_userinfo() {
        for url in [
            "http://u:invalid-proxy-secret-264@[bad/invalid-proxy-secret-264",
            "unknown://u:secret@host/terces?echo=c2VjcmV0#secret",
            "secret",
            "",
            " ",
            "none",
            "http://secret@[bad",
            "http://host/\nsecret",
        ] {
            let error = invalid_proxy_url("CLI", url);
            assert_eq!(
                error.to_string(),
                "Validation: Invalid configuration: Invalid CLI proxy URL (value omitted)"
            );
            assert!(!format!("{error:?}").contains("secret"));
        }
    }

    #[test]
    fn proxy_client_pool_debug_does_not_delegate_untrusted_routes() {
        ensure_tls_provider();
        let client = reqwest::Client::builder()
            .no_proxy()
            .proxy(
                reqwest::Proxy::all("http://u:secret@localhost:8080/terces")
                    .unwrap()
                    .no_proxy(reqwest::NoProxy::from_string(
                        "secret.example,terces.example",
                    )),
            )
            .build()
            .unwrap();
        let pool = HttpClientPool::default();
        pool.0
            .lock()
            .unwrap()
            .insert("opaque-digest".into(), client);
        let rendered = format!("{pool:?}");
        assert!(!rendered.contains("secret"), "{rendered}");
        assert!(!rendered.contains("terces"), "{rendered}");
        assert!(!rendered.contains("localhost"), "{rendered}");
    }

    #[test]
    fn proxy_missing_password_errors_do_not_reflect_configured_names() {
        let config = ProxyConfig {
            username: Some("proxy-secret-264".into()),
            password_env: Some("APERTURE264_MISSING_PROXY_SECRET_ENV_NAME".into()),
            ..Default::default()
        };
        let proxy = reqwest::Proxy::all("http://localhost:8080").unwrap();
        let error = apply_config_proxy_auth(proxy, &config).unwrap_err();
        let rendered = format!("{error} {error:?}");
        assert!(!rendered.contains("proxy-secret-264"));
        assert!(!rendered.contains("APERTURE264_MISSING_PROXY_SECRET_ENV_NAME"));
        assert!(rendered.contains("Proxy password environment variable is unavailable"));
    }

    #[test]
    fn proxy_safe_projection_does_not_replace_transport_identity() {
        let ctx = crate::invocation::ExecutionContext::default();
        let first = ProxyDiagnostics {
            source: "config",
            http: Some("http://u:first@localhost:8080/first?echo=tsrif#first".into()),
            no_proxy: vec!["first.example".into()],
            ..Default::default()
        };
        assert_eq!(
            first.transport_identity_json(),
            serde_json::json!({
                "source": "config", "disabled": false, "all": null,
                "http": "http://localhost:8080/first?echo=tsrif#first",
                "https": null, "no_proxy": ["first.example"]
            })
        );
        let mut second = first.clone();
        second.http = Some("http://u:second@localhost:8080/second?echo=dnoces#second".into());
        second.no_proxy = vec!["second.example".into()];
        assert_eq!(first.to_json(), second.to_json());
        assert_ne!(transport_key(&ctx, &first), transport_key(&ctx, &second));
        assert!(!format!("{first:?}").contains("first"));
        assert!(!format!("{second:?}").contains("second"));
    }

    #[test]
    fn transport_keys_distinguish_rotated_proxy_passwords() {
        let password_env = "APERTURE_REVIEW_PROXY_PASSWORD";
        let mut config = GlobalConfig::default();
        config.proxy.password_env = Some(format!(" {password_env} "));
        let ctx = crate::invocation::ExecutionContext {
            global_config: Some(config),
            ..Default::default()
        };
        std::env::set_var(password_env, "first");
        let first = transport_key(&ctx, &ProxyDiagnostics::default());
        std::env::set_var(password_env, "second");
        let second = transport_key(&ctx, &ProxyDiagnostics::default());
        std::env::remove_var(password_env);
        assert_ne!(first, second);
        assert!(!second.contains("second"));
    }

    #[test]
    fn transport_keys_distinguish_redacted_proxy_credentials() {
        let mut ctx = crate::invocation::ExecutionContext {
            proxy_override: ProxyOverride::Use("http://alice:secret@localhost:8080".into()),
            ..Default::default()
        };
        let (_, first_diagnostics) = configure_proxy(reqwest::Client::builder(), &ctx).unwrap();
        let first_key = transport_key(&ctx, &first_diagnostics);
        ctx.proxy_override = ProxyOverride::Use("http://bob:other@localhost:8080".into());
        let (_, second_diagnostics) = configure_proxy(reqwest::Client::builder(), &ctx).unwrap();
        assert_eq!(first_diagnostics.to_json(), second_diagnostics.to_json());
        assert_ne!(first_key, transport_key(&ctx, &second_diagnostics));
        assert!(!first_key.contains("secret"));
    }

    fn operation_with_body(
        request_body: Option<crate::cache::models::CachedRequestBody>,
    ) -> CachedCommand {
        CachedCommand {
            name: "upload".to_string(),
            description: None,
            summary: None,
            operation_id: "upload".to_string(),
            method: "POST".to_string(),
            path: "/upload".to_string(),
            parameters: vec![],
            request_body,
            responses: vec![],
            security_scopes: Vec::new(),
            security_requirements: vec![],
            tags: vec![],
            deprecated: false,
            external_docs_url: None,
            examples: vec![],
            display_group: None,
            display_name: None,
            aliases: vec![],
            hidden: false,
            pagination: crate::cache::models::PaginationInfo::default(),
        }
    }

    #[test]
    fn direct_request_body_validation_rejects_mismatches_and_invalid_json() {
        let binary = operation_with_body(Some(crate::cache::models::CachedRequestBody {
            content_type: "image/png".to_string(),
            schema: r#"{"type":"string","format":"binary"}"#.to_string(),
            required: true,
            description: None,
            example: None,
        }));
        assert!(
            validate_operation_call_body(&binary, Some(&RequestBody::Json("{}".to_string())))
                .is_err()
        );
        assert!(
            validate_operation_call_body(&binary, Some(&RequestBody::Binary(vec![0xff]))).is_ok()
        );

        let json = operation_with_body(Some(crate::cache::models::CachedRequestBody {
            content_type: "application/json".to_string(),
            schema: r#"{"type":"object"}"#.to_string(),
            required: true,
            description: None,
            example: None,
        }));
        assert!(
            validate_operation_call_body(&json, Some(&RequestBody::Binary(vec![0xff]))).is_err()
        );
        assert!(validate_operation_call_body(
            &json,
            Some(&RequestBody::Json("{invalid".to_string()))
        )
        .is_err());
        assert!(validate_operation_call_body(
            &json,
            Some(&RequestBody::Json(r#"{"ok":true}"#.to_string()))
        )
        .is_ok());
    }

    #[test]
    fn test_default_request_headers_use_current_package_version() {
        let headers = default_request_headers();
        let user_agent = headers
            .get("User-Agent")
            .and_then(|value| value.to_str().ok())
            .expect("user agent header should be present and valid");

        assert_eq!(user_agent, concat!("aperture/", env!("CARGO_PKG_VERSION")));
    }

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
            &[],
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
    fn test_apply_jq_filter_preserves_json_values() {
        let json = r#"{"large":18446744073709551615,"negative":-9223372036854775808,"float":1.25,"text":"line\n雪","bool":true,"null":null,"nested":[{"key":"value"}]}"#;
        let result = apply_jq_filter(json, ".").unwrap();
        assert_eq!(
            serde_json::from_str::<Value>(&result).unwrap(),
            serde_json::from_str::<Value>(json).unwrap()
        );
    }

    #[cfg(feature = "jq")]
    #[test]
    fn test_apply_jq_filter_empty_and_runtime_error() {
        assert_eq!(apply_jq_filter("null", "empty").unwrap(), "null");
        let err = apply_jq_filter("null", r#"error("failed")"#).unwrap_err();
        assert!(err.to_string().contains("Filter execution error"));
        assert!(err.to_string().contains("failed"));
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
