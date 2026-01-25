use crate::cache::models::{CachedCommand, CachedParameter, CachedSecurityScheme, CachedSpec};
use crate::cli::OutputFormat;
use crate::config::models::GlobalConfig;
use crate::config::url_resolver::BaseUrlResolver;
use crate::constants;
use crate::error::Error;
use crate::resilience::{calculate_retry_delay_with_header, is_retryable_status};
use crate::response_cache::{
    CacheConfig, CacheKey, CachedRequestInfo, CachedResponse, ResponseCache,
};
use crate::utils::to_kebab_case;
use base64::{engine::general_purpose, Engine as _};
use clap::ArgMatches;
use reqwest::header::{HeaderMap, HeaderName, HeaderValue};
use reqwest::Method;
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, HashMap};
use std::fmt::Write;
use std::str::FromStr;
use std::time::Duration;
use tabled::Table;
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

        // GET, HEAD, OPTIONS, TRACE are idempotent
        self.method.as_ref().is_some_and(|m| {
            matches!(
                m.to_uppercase().as_str(),
                "GET" | "HEAD" | "OPTIONS" | "TRACE"
            )
        })
    }
}

/// Maximum number of rows to display in table format to prevent memory exhaustion
const MAX_TABLE_ROWS: usize = 1000;

// Helper functions

/// Extract server variable arguments from CLI matches
fn extract_server_var_args(matches: &ArgMatches) -> Vec<String> {
    matches
        .try_get_many::<String>("server-var")
        .ok()
        .flatten()
        .map(|values| values.cloned().collect())
        .unwrap_or_default()
}

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

/// Extract request body from matches
fn extract_request_body(
    operation: &CachedCommand,
    matches: &ArgMatches,
) -> Result<Option<String>, Error> {
    if operation.request_body.is_none() {
        return Ok(None);
    }

    // Get to the deepest subcommand matches
    let mut current_matches = matches;
    while let Some((_name, sub_matches)) = current_matches.subcommand() {
        current_matches = sub_matches;
    }

    if let Some(body_value) = current_matches.get_one::<String>("body") {
        // Validate JSON
        let _json_body: Value = serde_json::from_str(body_value)
            .map_err(|e| Error::invalid_json_body(e.to_string()))?;
        Ok(Some(body_value.clone()))
    } else {
        Ok(None)
    }
}

/// Handle dry-run mode
fn handle_dry_run(
    dry_run: bool,
    method: &reqwest::Method,
    url: &str,
    headers: &reqwest::header::HeaderMap,
    body: Option<&str>,
    operation: &CachedCommand,
    capture_output: bool,
) -> Result<Option<String>, Error> {
    if !dry_run {
        return Ok(None);
    }

    let headers_map: HashMap<String, String> = headers
        .iter()
        .map(|(k, v)| {
            let value = if is_sensitive_header(k.as_str()) {
                "<REDACTED>".to_string()
            } else {
                v.to_str().unwrap_or("<binary>").to_string()
            };
            (k.as_str().to_string(), value)
        })
        .collect();

    let dry_run_info = serde_json::json!({
        "dry_run": true,
        "method": method.to_string(),
        "url": url,
        "headers": headers_map,
        "body": body,
        "operation_id": operation.operation_id
    });

    let output = serde_json::to_string_pretty(&dry_run_info).map_err(|e| {
        Error::serialization_error(format!("Failed to serialize dry run info: {e}"))
    })?;

    if capture_output {
        Ok(Some(output))
    } else {
        // ast-grep-ignore: no-println
        println!("{output}");
        Ok(None)
    }
}

/// Send HTTP request and get response
async fn send_request(
    request: reqwest::RequestBuilder,
) -> Result<(reqwest::StatusCode, HashMap<String, String>, String), Error> {
    let response = request
        .send()
        .await
        .map_err(|e| Error::network_request_failed(e.to_string()))?;

    let status = response.status();
    let response_headers: HashMap<String, String> = response
        .headers()
        .iter()
        .map(|(k, v)| (k.as_str().to_string(), v.to_str().unwrap_or("").to_string()))
        .collect();

    let response_text = response
        .text()
        .await
        .map_err(|e| Error::response_read_error(e.to_string()))?;

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
) -> Result<(reqwest::StatusCode, HashMap<String, String>, String), Error> {
    use crate::resilience::RetryConfig;

    // If no retry context or retries disabled, just send once
    let Some(ctx) = retry_context else {
        let request = build_request(client, method, url, headers, body);
        return send_request(request).await;
    };

    if !ctx.is_enabled() {
        let request = build_request(client, method, url, headers, body);
        return send_request(request).await;
    }

    // Check if safe to retry non-GET requests
    if !ctx.is_safe_to_retry() {
        eprintln!(
            "Warning: Retries disabled for {} {} - method is not idempotent and no --idempotency-key provided",
            method,
            operation.operation_id
        );
        eprintln!(
            "         Use --force-retry to enable retries anyway, or provide --idempotency-key"
        );
        let request = build_request(client, method.clone(), url, headers, body);
        return send_request(request).await;
    }

    // Create a RetryConfig from the RetryContext
    let retry_config = RetryConfig {
        max_attempts: ctx.max_attempts as usize,
        initial_delay_ms: ctx.initial_delay_ms,
        max_delay_ms: ctx.max_delay_ms,
        backoff_multiplier: 2.0,
        jitter: true,
    };

    let max_attempts = ctx.max_attempts;
    let mut attempt: u32 = 0;
    let mut last_error: Option<Error> = None;
    let mut last_status: Option<reqwest::StatusCode> = None;
    let mut last_response_headers: Option<HashMap<String, String>> = None;
    let mut last_response_text: Option<String> = None;

    while attempt < max_attempts {
        attempt += 1;

        let request = build_request(client, method.clone(), url, headers.clone(), body.clone());
        let result = send_request(request).await;

        match result {
            Ok((status, response_headers, response_text)) => {
                // Success - return immediately
                if status.is_success() {
                    return Ok((status, response_headers, response_text));
                }

                // Check if we should retry this status code
                if !is_retryable_status(status.as_u16()) {
                    return Ok((status, response_headers, response_text));
                }

                // Parse Retry-After header if present (convert to HeaderMap for parsing)
                let retry_after = response_headers.get("retry-after").and_then(|v| {
                    // Parse as seconds first
                    if let Ok(seconds) = v.parse::<u64>() {
                        return Some(Duration::from_secs(seconds));
                    }
                    // Try parsing as HTTP-date
                    if let Ok(date) = httpdate::parse_http_date(v) {
                        if let Ok(duration) = date.duration_since(std::time::SystemTime::now()) {
                            return Some(duration);
                        }
                    }
                    None
                });

                // Calculate delay using the retry config
                let delay = calculate_retry_delay_with_header(
                    &retry_config,
                    (attempt - 1) as usize, // 0-indexed for delay calculation
                    retry_after,
                );

                // Check if we have more attempts
                if attempt < max_attempts {
                    eprintln!(
                        "Retry {}/{}: {} {} returned {} - retrying in {}ms",
                        attempt,
                        max_attempts,
                        method,
                        operation.operation_id,
                        status.as_u16(),
                        delay.as_millis()
                    );
                    sleep(delay).await;
                }

                // Save for potential final error
                last_status = Some(status);
                last_response_headers = Some(response_headers);
                last_response_text = Some(response_text);
            }
            Err(e) => {
                // Network error - check if we should retry
                let should_retry = matches!(&e, Error::Network(_));

                if !should_retry {
                    return Err(e);
                }

                // Calculate delay
                let delay =
                    calculate_retry_delay_with_header(&retry_config, (attempt - 1) as usize, None);

                if attempt < max_attempts {
                    eprintln!(
                        "Retry {}/{}: {} {} failed - retrying in {}ms: {}",
                        attempt,
                        max_attempts,
                        method,
                        operation.operation_id,
                        delay.as_millis(),
                        e
                    );
                    sleep(delay).await;
                }

                last_error = Some(e);
            }
        }
    }

    // All retries exhausted - return last result
    if let (Some(status), Some(headers), Some(text)) =
        (last_status, last_response_headers, last_response_text)
    {
        eprintln!(
            "Retry exhausted: {} {} failed after {} attempts",
            method, operation.operation_id, max_attempts
        );
        return Ok((status, headers, text));
    }

    // Return last error if we have one
    if let Some(e) = last_error {
        eprintln!(
            "Retry exhausted: {} {} failed after {} attempts",
            method, operation.operation_id, max_attempts
        );
        return Err(e);
    }

    // Should not happen, but handle gracefully
    Err(Error::network_request_failed(
        "Request failed with no response".to_string(),
    ))
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
    if let Some(body_str) = body {
        if let Ok(json_body) = serde_json::from_str::<Value>(&body_str) {
            request = request.json(&json_body);
        }
    }
    request
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

    let cached_request_info = CachedRequestInfo {
        method: method.to_string(),
        url,
        headers: headers
            .iter()
            .map(|(k, v)| (k.as_str().to_string(), v.to_str().unwrap_or("").to_string()))
            .collect(),
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

/// Executes HTTP requests based on parsed CLI arguments and cached spec data.
///
/// This module handles the mapping from CLI arguments back to API operations,
/// resolves authentication secrets, builds HTTP requests, and validates responses.
///
/// # Arguments
/// * `spec` - The cached specification containing operation details
/// * `matches` - Parsed CLI arguments from clap
/// * `base_url` - Optional base URL override. If None, uses `BaseUrlResolver`
/// * `dry_run` - If true, show request details without executing
/// * `idempotency_key` - Optional idempotency key for safe retries
/// * `global_config` - Optional global configuration for URL resolution
/// * `output_format` - Format for response output (json, yaml, table)
/// * `jq_filter` - Optional JQ filter expression to apply to response
/// * `cache_config` - Optional cache configuration for response caching
/// * `capture_output` - If true, captures output and returns it instead of printing to stdout
/// * `retry_context` - Optional retry configuration for automatic request retries
///
/// # Returns
/// * `Ok(Option<String>)` - Request executed successfully. Returns Some(output) if `capture_output` is true
/// * `Err(Error)` - Request failed or validation error
///
/// # Errors
/// Returns errors for authentication failures, network issues, response validation, or JQ filter errors
///
/// # Panics
/// Panics if JSON serialization of dry-run information fails (extremely unlikely)
#[allow(clippy::too_many_lines)]
#[allow(clippy::too_many_arguments)]
#[allow(clippy::missing_panics_doc)]
#[allow(clippy::missing_errors_doc)]
pub async fn execute_request(
    spec: &CachedSpec,
    matches: &ArgMatches,
    base_url: Option<&str>,
    dry_run: bool,
    idempotency_key: Option<&str>,
    global_config: Option<&GlobalConfig>,
    output_format: &OutputFormat,
    jq_filter: Option<&str>,
    cache_config: Option<&CacheConfig>,
    capture_output: bool,
    retry_context: Option<&RetryContext>,
) -> Result<Option<String>, Error> {
    // Find the operation from the command hierarchy (also returns the operation's ArgMatches)
    let (operation, operation_matches) = find_operation_with_matches(spec, matches)?;

    // Check if --show-examples flag is present in the operation's matches
    // Only check if the flag exists in the matches (it won't exist in some test scenarios)
    if operation_matches
        .try_contains_id("show-examples")
        .unwrap_or(false)
        && operation_matches.get_flag("show-examples")
    {
        print_extended_examples(operation);
        return Ok(None);
    }

    // Extract server variable arguments
    let server_var_args = extract_server_var_args(matches);

    // Resolve base URL using the new priority hierarchy with server variable support
    let resolver = BaseUrlResolver::new(spec);
    let resolver = if let Some(config) = global_config {
        resolver.with_global_config(config)
    } else {
        resolver
    };
    let base_url = resolver.resolve_with_variables(base_url, &server_var_args)?;

    // Build the full URL with path parameters
    let url = build_url(&base_url, &operation.path, operation, operation_matches)?;

    // Create HTTP client
    let client = build_http_client()?;

    // Build headers including authentication and idempotency
    let mut headers = build_headers(
        spec,
        operation,
        operation_matches,
        &spec.name,
        global_config,
    )?;

    // Add idempotency key if provided
    if let Some(key) = idempotency_key {
        headers.insert(
            HeaderName::from_static("idempotency-key"),
            HeaderValue::from_str(key).map_err(|_| Error::invalid_idempotency_key())?,
        );
    }

    // Build request
    let method = Method::from_str(&operation.method)
        .map_err(|_| Error::invalid_http_method(&operation.method))?;

    let headers_clone = headers.clone(); // For dry-run output

    // Extract request body
    let request_body = extract_request_body(operation, operation_matches)?;

    // Prepare cache context
    let cache_context = prepare_cache_context(
        cache_config,
        &spec.name,
        &operation.operation_id,
        &method,
        &url,
        &headers_clone,
        request_body.as_deref(),
    )?;

    // Check cache for existing response
    if let Some(cached_response) = check_cache(cache_context.as_ref()).await? {
        let output = print_formatted_response(
            &cached_response.body,
            output_format,
            jq_filter,
            capture_output,
        )?;
        return Ok(output);
    }

    // Handle dry-run mode
    if let Some(output) = handle_dry_run(
        dry_run,
        &method,
        &url,
        &headers_clone,
        request_body.as_deref(),
        operation,
        capture_output,
    )? {
        return Ok(Some(output));
    }
    if dry_run {
        return Ok(None);
    }

    // Send request with retry support
    let (status, response_headers, response_text) = send_request_with_retry(
        &client,
        method.clone(),
        &url,
        headers,
        request_body.clone(),
        retry_context,
        operation,
    )
    .await?;

    // Check if request was successful
    if !status.is_success() {
        return Err(handle_http_error(status, response_text, spec, operation));
    }

    // Store response in cache
    store_in_cache(
        cache_context,
        &response_text,
        status,
        &response_headers,
        method,
        url,
        &headers_clone,
        request_body.as_deref(),
        cache_config,
    )
    .await?;

    // Print response in the requested format
    if response_text.is_empty() {
        Ok(None)
    } else {
        print_formatted_response(&response_text, output_format, jq_filter, capture_output)
    }
}

/// Finds the operation from the command hierarchy
/// Print extended examples for a command
fn print_extended_examples(operation: &CachedCommand) {
    // ast-grep-ignore: no-println
    println!("Command: {}\n", to_kebab_case(&operation.operation_id));

    if let Some(ref summary) = operation.summary {
        // ast-grep-ignore: no-println
        println!("Description: {summary}\n");
    }

    // ast-grep-ignore: no-println
    println!("Method: {} {}\n", operation.method, operation.path);

    if operation.examples.is_empty() {
        // ast-grep-ignore: no-println
        println!("No examples available for this command.");
        return;
    }

    // ast-grep-ignore: no-println
    println!("Examples:\n");
    for (i, example) in operation.examples.iter().enumerate() {
        // ast-grep-ignore: no-println
        println!("{}. {}", i + 1, example.description);
        // ast-grep-ignore: no-println
        println!("   {}", example.command_line);
        if let Some(ref explanation) = example.explanation {
            // ast-grep-ignore: no-println
            println!("   {explanation}");
        }
        // ast-grep-ignore: no-println
        println!();
    }

    // Additional helpful information
    if operation.parameters.is_empty() {
        return;
    }

    // ast-grep-ignore: no-println
    println!("Parameters:");
    for param in &operation.parameters {
        let required = if param.required { " (required)" } else { "" };
        let param_type = param.schema_type.as_deref().unwrap_or("string");
        // ast-grep-ignore: no-println
        println!("  --{}{} [{}]", param.name, required, param_type);

        let Some(ref desc) = param.description else {
            continue;
        };
        // ast-grep-ignore: no-println
        println!("      {desc}");
    }
    // ast-grep-ignore: no-println
    println!();

    if operation.request_body.is_some() {
        // ast-grep-ignore: no-println
        println!("Request Body:");
        // ast-grep-ignore: no-println
        println!("  --body JSON (required)");
        // ast-grep-ignore: no-println
        println!("      JSON data to send in the request body");
    }
}

#[allow(dead_code)]
fn find_operation<'a>(
    spec: &'a CachedSpec,
    matches: &ArgMatches,
) -> Result<&'a CachedCommand, Error> {
    // Get the subcommand path from matches
    let mut current_matches = matches;
    let mut subcommand_path = Vec::new();

    while let Some((name, sub_matches)) = current_matches.subcommand() {
        subcommand_path.push(name);
        current_matches = sub_matches;
    }

    // For now, just find the first matching operation
    // In a real implementation, we'd match based on the full path
    let Some(operation_name) = subcommand_path.last() else {
        let operation_name = "unknown".to_string();
        let suggestions = crate::suggestions::suggest_similar_operations(spec, &operation_name);
        return Err(Error::operation_not_found_with_suggestions(
            operation_name,
            &suggestions,
        ));
    };

    for command in &spec.commands {
        // Convert operation_id to kebab-case for comparison
        let kebab_id = to_kebab_case(&command.operation_id);
        if &kebab_id == operation_name || command.method.to_lowercase() == *operation_name {
            return Ok(command);
        }
    }

    let operation_name = subcommand_path
        .last()
        .map_or_else(|| "unknown".to_string(), ToString::to_string);

    // Generate suggestions for similar operations
    let suggestions = crate::suggestions::suggest_similar_operations(spec, &operation_name);

    Err(Error::operation_not_found_with_suggestions(
        operation_name,
        &suggestions,
    ))
}

fn find_operation_with_matches<'a>(
    spec: &'a CachedSpec,
    matches: &'a ArgMatches,
) -> Result<(&'a CachedCommand, &'a ArgMatches), Error> {
    // Get the subcommand path from matches
    let mut current_matches = matches;
    let mut subcommand_path = Vec::new();

    while let Some((name, sub_matches)) = current_matches.subcommand() {
        subcommand_path.push(name);
        current_matches = sub_matches;
    }

    // For now, just find the first matching operation
    // In a real implementation, we'd match based on the full path
    let Some(operation_name) = subcommand_path.last() else {
        let operation_name = "unknown".to_string();
        let suggestions = crate::suggestions::suggest_similar_operations(spec, &operation_name);
        return Err(Error::operation_not_found_with_suggestions(
            operation_name,
            &suggestions,
        ));
    };

    for command in &spec.commands {
        // Convert operation_id to kebab-case for comparison
        let kebab_id = to_kebab_case(&command.operation_id);
        if &kebab_id == operation_name || command.method.to_lowercase() == *operation_name {
            // Return current_matches (the deepest subcommand) which contains the operation's arguments
            return Ok((command, current_matches));
        }
    }

    let operation_name = subcommand_path
        .last()
        .map_or_else(|| "unknown".to_string(), ToString::to_string);

    // Generate suggestions for similar operations
    let suggestions = crate::suggestions::suggest_similar_operations(spec, &operation_name);

    Err(Error::operation_not_found_with_suggestions(
        operation_name,
        &suggestions,
    ))
}

/// Get query parameter value formatted for URL
/// Returns None if the parameter value should be skipped
fn get_query_param_value(
    param: &CachedParameter,
    current_matches: &ArgMatches,
    arg_str: &str,
) -> Option<String> {
    let is_boolean = param.schema_type.as_ref().is_some_and(|t| t == "boolean");

    if is_boolean {
        // Boolean parameters are flags - add only if set
        current_matches
            .get_flag(arg_str)
            .then(|| format!("{arg_str}=true"))
    } else {
        // Non-boolean parameters have string values
        current_matches
            .get_one::<String>(arg_str)
            .map(|value| format!("{arg_str}={}", urlencoding::encode(value)))
    }
}

/// Builds the full URL with path parameters substituted
///
/// Note: Server variable substitution is now handled by `BaseUrlResolver.resolve_with_variables()`
/// before calling this function, so `base_url` should already have server variables resolved.
fn build_url(
    base_url: &str,
    path_template: &str,
    operation: &CachedCommand,
    matches: &ArgMatches,
) -> Result<String, Error> {
    let mut url = format!("{}{}", base_url.trim_end_matches('/'), path_template);

    // Get to the deepest subcommand matches
    let mut current_matches = matches;
    while let Some((_name, sub_matches)) = current_matches.subcommand() {
        current_matches = sub_matches;
    }

    // Substitute path parameters
    // Look for {param} patterns and replace with values from matches
    let mut start = 0;
    while let Some(open) = url[start..].find('{') {
        let open_pos = start + open;
        let Some(close) = url[open_pos..].find('}') else {
            break;
        };

        let close_pos = open_pos + close;
        let param_name = &url[open_pos + 1..close_pos];

        // Check if this is a boolean parameter
        let param = operation.parameters.iter().find(|p| p.name == param_name);
        let is_boolean = param
            .and_then(|p| p.schema_type.as_ref())
            .is_some_and(|t| t == "boolean");

        let value = if is_boolean {
            // Boolean path parameters are flags
            if current_matches.get_flag(param_name) {
                "true"
            } else {
                "false"
            }
            .to_string()
        } else {
            match current_matches
                .try_get_one::<String>(param_name)
                .ok()
                .flatten()
            {
                Some(string_value) => string_value.clone(),
                None => return Err(Error::missing_path_parameter(param_name)),
            }
        };

        url.replace_range(open_pos..=close_pos, &value);
        start = open_pos + value.len();
    }

    // Add query parameters
    let mut query_params = Vec::new();
    for arg in current_matches.ids() {
        let arg_str = arg.as_str();
        // Skip non-query args - only process query parameters from the operation
        let param = operation
            .parameters
            .iter()
            .find(|p| p.name == arg_str && p.location == "query");

        let Some(param) = param else {
            continue;
        };

        // Get query param value using helper (handles boolean vs string params)
        if let Some(value) = get_query_param_value(param, current_matches, arg_str) {
            query_params.push(value);
        }
    }

    if !query_params.is_empty() {
        url.push('?');
        url.push_str(&query_params.join("&"));
    }

    Ok(url)
}

/// Get header value for a parameter from CLI matches
/// Returns None if the parameter value should be skipped
fn get_header_param_value(
    param: &CachedParameter,
    current_matches: &ArgMatches,
) -> Result<Option<HeaderValue>, Error> {
    let is_boolean = matches!(param.schema_type.as_deref(), Some("boolean"));

    // Handle boolean and non-boolean parameters separately to avoid
    // panics from mismatched types (get_flag on string or get_one on bool)
    if is_boolean {
        return Ok(current_matches
            .get_flag(&param.name)
            .then_some(HeaderValue::from_static("true")));
    }

    // Non-boolean: get string value
    current_matches
        .get_one::<String>(&param.name)
        .map(|value| {
            HeaderValue::from_str(value)
                .map_err(|e| Error::invalid_header_value(&param.name, e.to_string()))
        })
        .transpose()
}

/// Builds headers including authentication
fn build_headers(
    spec: &CachedSpec,
    operation: &CachedCommand,
    matches: &ArgMatches,
    api_name: &str,
    global_config: Option<&GlobalConfig>,
) -> Result<HeaderMap, Error> {
    let mut headers = HeaderMap::new();

    // Add default headers
    headers.insert("User-Agent", HeaderValue::from_static("aperture/0.1.0"));
    headers.insert(
        constants::HEADER_ACCEPT,
        HeaderValue::from_static(constants::CONTENT_TYPE_JSON),
    );

    // Get to the deepest subcommand matches
    let mut current_matches = matches;
    while let Some((_name, sub_matches)) = current_matches.subcommand() {
        current_matches = sub_matches;
    }

    // Add header parameters from matches
    for param in &operation.parameters {
        // Skip non-header parameters early
        if param.location != "header" {
            continue;
        }

        let header_name = HeaderName::from_str(&param.name)
            .map_err(|e| Error::invalid_header_name(&param.name, e.to_string()))?;

        // Get header value using helper (handles boolean vs string params)
        let Some(header_value) = get_header_param_value(param, current_matches)? else {
            continue;
        };

        headers.insert(header_name, header_value);
    }

    // Add authentication headers based on security requirements
    for security_scheme_name in &operation.security_requirements {
        let Some(security_scheme) = spec.security_schemes.get(security_scheme_name) else {
            continue;
        };
        add_authentication_header(&mut headers, security_scheme, api_name, global_config)?;
    }

    // Add custom headers from --header/-H flags
    // Use try_get_many to avoid panic when header arg doesn't exist
    let Ok(Some(custom_headers)) = current_matches.try_get_many::<String>("header") else {
        return Ok(headers);
    };

    for header_str in custom_headers {
        let (name, value) = parse_custom_header(header_str)?;
        let header_name = HeaderName::from_str(&name)
            .map_err(|e| Error::invalid_header_name(&name, e.to_string()))?;
        let header_value = HeaderValue::from_str(&value)
            .map_err(|e| Error::invalid_header_value(&name, e.to_string()))?;
        headers.insert(header_name, header_value);
    }

    Ok(headers)
}

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

/// Checks if a header name contains sensitive authentication information
fn is_sensitive_header(header_name: &str) -> bool {
    let name_lower = header_name.to_lowercase();
    matches!(
        name_lower.as_str(),
        "authorization" | "proxy-authorization" | "x-api-key" | "x-api-token" | "x-auth-token"
    )
}

/// Adds an authentication header based on a security scheme
#[allow(clippy::too_many_lines)]
fn add_authentication_header(
    headers: &mut HeaderMap,
    security_scheme: &CachedSecurityScheme,
    api_name: &str,
    global_config: Option<&GlobalConfig>,
) -> Result<(), Error> {
    // Debug logging when RUST_LOG is set
    if std::env::var("RUST_LOG").is_ok() {
        // ast-grep-ignore: no-println
        eprintln!(
            "[DEBUG] Adding authentication header for scheme: {} (type: {})",
            security_scheme.name, security_scheme.scheme_type
        );
    }

    // Priority 1: Check config-based secrets first
    let secret_config = global_config
        .and_then(|config| config.api_configs.get(api_name))
        .and_then(|api_config| api_config.secrets.get(&security_scheme.name));

    let (secret_value, env_var_name) = match (secret_config, &security_scheme.aperture_secret) {
        (Some(config_secret), _) => {
            // Use config-based secret
            let secret_value = std::env::var(&config_secret.name)
                .map_err(|_| Error::secret_not_set(&security_scheme.name, &config_secret.name))?;
            (secret_value, config_secret.name.clone())
        }
        (None, Some(aperture_secret)) => {
            // Priority 2: Fall back to x-aperture-secret extension
            let secret_value = std::env::var(&aperture_secret.name)
                .map_err(|_| Error::secret_not_set(&security_scheme.name, &aperture_secret.name))?;
            (secret_value, aperture_secret.name.clone())
        }
        (None, None) => {
            // No authentication configuration found - skip this scheme
            return Ok(());
        }
    };

    // Debug logging for resolved secret source
    if std::env::var("RUST_LOG").is_ok() {
        let source = if secret_config.is_some() {
            "config"
        } else {
            "x-aperture-secret"
        };
        // ast-grep-ignore: no-println
        eprintln!(
            "[DEBUG] Using secret from {source} for scheme '{}': env var '{env_var_name}'",
            security_scheme.name
        );
    }

    // Validate the secret doesn't contain control characters
    validate_header_value(constants::HEADER_AUTHORIZATION, &secret_value)?;

    // Build the appropriate header based on scheme type
    match security_scheme.scheme_type.as_str() {
        constants::AUTH_SCHEME_APIKEY => {
            let (Some(location), Some(param_name)) =
                (&security_scheme.location, &security_scheme.parameter_name)
            else {
                return Ok(());
            };

            if location == "header" {
                let header_name = HeaderName::from_str(param_name)
                    .map_err(|e| Error::invalid_header_name(param_name, e.to_string()))?;
                let header_value = HeaderValue::from_str(&secret_value)
                    .map_err(|e| Error::invalid_header_value(param_name, e.to_string()))?;
                headers.insert(header_name, header_value);
            }
            // Note: query and cookie locations are handled differently in request building
        }
        "http" => {
            let Some(scheme_str) = &security_scheme.scheme else {
                return Ok(());
            };

            let auth_scheme: AuthScheme = scheme_str.as_str().into();
            let auth_value = match &auth_scheme {
                AuthScheme::Bearer => {
                    format!("Bearer {secret_value}")
                }
                AuthScheme::Basic => {
                    // Basic auth expects "username:password" format in the secret
                    // The secret should contain the raw "username:password" string
                    // We'll base64 encode it before adding to the header
                    let encoded = general_purpose::STANDARD.encode(&secret_value);
                    format!("Basic {encoded}")
                }
                AuthScheme::Token
                | AuthScheme::DSN
                | AuthScheme::ApiKey
                | AuthScheme::Custom(_) => {
                    // Treat any other HTTP scheme as a bearer-like token
                    // Format: "Authorization: <scheme> <token>"
                    // This supports Token, ApiKey, DSN, and any custom schemes
                    format!("{scheme_str} {secret_value}")
                }
            };

            let header_value = HeaderValue::from_str(&auth_value).map_err(|e| {
                Error::invalid_header_value(constants::HEADER_AUTHORIZATION, e.to_string())
            })?;
            headers.insert(constants::HEADER_AUTHORIZATION, header_value);

            // Debug logging
            if std::env::var("RUST_LOG").is_ok() {
                match &auth_scheme {
                    AuthScheme::Bearer => {
                        // ast-grep-ignore: no-println
                        eprintln!("[DEBUG] Added Bearer authentication header");
                    }
                    AuthScheme::Basic => {
                        // ast-grep-ignore: no-println
                        eprintln!("[DEBUG] Added Basic authentication header (base64 encoded)");
                    }
                    _ => {
                        // ast-grep-ignore: no-println
                        eprintln!(
                            "[DEBUG] Added custom HTTP auth header with scheme: {scheme_str}"
                        );
                    }
                }
            }
        }
        _ => {
            return Err(Error::unsupported_security_scheme(
                &security_scheme.scheme_type,
            ));
        }
    }

    Ok(())
}

/// Prints the response text in the specified format
fn print_formatted_response(
    response_text: &str,
    output_format: &OutputFormat,
    jq_filter: Option<&str>,
    capture_output: bool,
) -> Result<Option<String>, Error> {
    // Apply JQ filter if provided
    let processed_text = if let Some(filter) = jq_filter {
        apply_jq_filter(response_text, filter)?
    } else {
        response_text.to_string()
    };

    match output_format {
        OutputFormat::Json => {
            // Try to pretty-print JSON (default behavior)
            let output = serde_json::from_str::<Value>(&processed_text)
                .ok()
                .and_then(|json_value| serde_json::to_string_pretty(&json_value).ok())
                .unwrap_or_else(|| processed_text.clone());

            if capture_output {
                return Ok(Some(output));
            }
            // ast-grep-ignore: no-println
            println!("{output}");
        }
        OutputFormat::Yaml => {
            // Convert JSON to YAML
            let output = serde_json::from_str::<Value>(&processed_text)
                .ok()
                .and_then(|json_value| serde_yaml::to_string(&json_value).ok())
                .unwrap_or_else(|| processed_text.clone());

            if capture_output {
                return Ok(Some(output));
            }
            // ast-grep-ignore: no-println
            println!("{output}");
        }
        OutputFormat::Table => {
            // Convert JSON to table format
            let Ok(json_value) = serde_json::from_str::<Value>(&processed_text) else {
                // If not JSON, output as-is
                if capture_output {
                    return Ok(Some(processed_text));
                }
                // ast-grep-ignore: no-println
                println!("{processed_text}");
                return Ok(None);
            };

            let table_output = print_as_table(&json_value, capture_output)?;
            if capture_output {
                return Ok(table_output);
            }
        }
    }

    Ok(None)
}

// Define table structures at module level to avoid clippy::items_after_statements
#[derive(tabled::Tabled)]
struct TableRow {
    #[tabled(rename = "Key")]
    key: String,
    #[tabled(rename = "Value")]
    value: String,
}

#[derive(tabled::Tabled)]
struct KeyValue {
    #[tabled(rename = "Key")]
    key: String,
    #[tabled(rename = "Value")]
    value: String,
}

/// Prints items as a numbered list
fn print_numbered_list(items: &[Value], capture_output: bool) -> Option<String> {
    if capture_output {
        let mut output = String::new();
        for (i, item) in items.iter().enumerate() {
            writeln!(&mut output, "{}: {}", i, format_value_for_table(item))
                .expect("writing to String cannot fail");
        }
        return Some(output.trim_end().to_string());
    }

    for (i, item) in items.iter().enumerate() {
        // ast-grep-ignore: no-println
        println!("{}: {}", i, format_value_for_table(item));
    }
    None
}

/// Helper to output or capture a message
fn output_or_capture(message: &str, capture_output: bool) -> Option<String> {
    if capture_output {
        return Some(message.to_string());
    }
    // ast-grep-ignore: no-println
    println!("{message}");
    None
}

/// Prints JSON data as a formatted table
#[allow(clippy::unnecessary_wraps, clippy::too_many_lines)]
fn print_as_table(json_value: &Value, capture_output: bool) -> Result<Option<String>, Error> {
    match json_value {
        Value::Array(items) => {
            if items.is_empty() {
                return Ok(output_or_capture(constants::EMPTY_ARRAY, capture_output));
            }

            // Check if array is too large
            if items.len() > MAX_TABLE_ROWS {
                let msg = format!(
                    "Array too large: {} items (max {} for table display)\nUse --format json or --jq to process the full data",
                    items.len(),
                    MAX_TABLE_ROWS
                );
                return Ok(output_or_capture(&msg, capture_output));
            }

            // Try to create a table from array of objects
            let Some(Value::Object(_)) = items.first() else {
                // Continue to fallback case
                return Ok(print_numbered_list(items, capture_output));
            };

            // Create table for array of objects
            let mut table_data: Vec<BTreeMap<String, String>> = Vec::new();

            for item in items {
                let Value::Object(obj) = item else {
                    continue;
                };
                let mut row = BTreeMap::new();
                for (key, value) in obj {
                    row.insert(key.clone(), format_value_for_table(value));
                }
                table_data.push(row);
            }

            if table_data.is_empty() {
                // Fallback to numbered list
                return Ok(print_numbered_list(items, capture_output));
            }

            // For now, use a simple key-value representation
            // In the future, we could implement a more sophisticated table structure
            let mut rows = Vec::new();
            for (i, row) in table_data.iter().enumerate() {
                if i > 0 {
                    rows.push(TableRow {
                        key: "---".to_string(),
                        value: "---".to_string(),
                    });
                }
                for (key, value) in row {
                    rows.push(TableRow {
                        key: key.clone(),
                        value: value.clone(),
                    });
                }
            }

            let table = Table::new(&rows);
            Ok(output_or_capture(&table.to_string(), capture_output))
        }
        Value::Object(obj) => {
            // Check if object has too many fields
            if obj.len() > MAX_TABLE_ROWS {
                let msg = format!(
                    "Object too large: {} fields (max {} for table display)\nUse --format json or --jq to process the full data",
                    obj.len(),
                    MAX_TABLE_ROWS
                );
                return Ok(output_or_capture(&msg, capture_output));
            }

            // Create a simple key-value table for objects
            let rows: Vec<KeyValue> = obj
                .iter()
                .map(|(key, value)| KeyValue {
                    key: key.clone(),
                    value: format_value_for_table(value),
                })
                .collect();

            let table = Table::new(&rows);
            Ok(output_or_capture(&table.to_string(), capture_output))
        }
        _ => {
            // For primitive values, just print them
            let formatted = format_value_for_table(json_value);
            Ok(output_or_capture(&formatted, capture_output))
        }
    }
}

/// Formats a JSON value for display in a table cell
fn format_value_for_table(value: &Value) -> String {
    match value {
        Value::Null => constants::NULL_VALUE.to_string(),
        Value::Bool(b) => b.to_string(),
        Value::Number(n) => n.to_string(),
        Value::String(s) => s.clone(),
        Value::Array(arr) => {
            if arr.len() <= 3 {
                format!(
                    "[{}]",
                    arr.iter()
                        .map(format_value_for_table)
                        .collect::<Vec<_>>()
                        .join(", ")
                )
            } else {
                format!("[{} items]", arr.len())
            }
        }
        Value::Object(obj) => {
            if obj.len() <= 2 {
                format!(
                    "{{{}}}",
                    obj.iter()
                        .map(|(k, v)| format!("{}: {}", k, format_value_for_table(v)))
                        .collect::<Vec<_>>()
                        .join(", ")
                )
            } else {
                format!("{{object with {} fields}}", obj.len())
            }
        }
    }
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
    // Parse the response as JSON
    let json_value: Value = serde_json::from_str(response_text)
        .map_err(|e| Error::jq_filter_error(filter, format!("Response is not valid JSON: {e}")))?;

    #[cfg(feature = "jq")]
    {
        // Use jaq v2.x (pure Rust implementation)
        use jaq_core::load::{Arena, File, Loader};
        use jaq_core::Compiler;

        // Create the program from the filter string
        let program = File {
            code: filter,
            path: (),
        };

        // Collect both standard library and JSON definitions into vectors
        // This avoids hanging issues with lazy iterator chains
        let defs: Vec<_> = jaq_std::defs().chain(jaq_json::defs()).collect();
        let funs: Vec<_> = jaq_std::funs().chain(jaq_json::funs()).collect();

        // Create loader with both standard library and JSON definitions
        let loader = Loader::new(defs);
        let arena = Arena::default();

        // Parse the filter
        let modules = match loader.load(&arena, program) {
            Ok(modules) => modules,
            Err(errs) => {
                return Err(Error::jq_filter_error(
                    filter,
                    format!("Parse error: {:?}", errs),
                ));
            }
        };

        // Compile the filter with both standard library and JSON functions
        let filter_fn = match Compiler::default().with_funs(funs).compile(modules) {
            Ok(filter) => filter,
            Err(errs) => {
                return Err(Error::jq_filter_error(
                    filter,
                    format!("Compilation error: {:?}", errs),
                ));
            }
        };

        // Convert serde_json::Value to jaq Val
        let jaq_value = Val::from(json_value);

        // Execute the filter
        let inputs = RcIter::new(core::iter::empty());
        let ctx = Ctx::new([], &inputs);

        // Run the filter on the input value
        let output = filter_fn.run((ctx, jaq_value));

        // Collect all results
        let results: Result<Vec<Val>, _> = output.collect();

        match results {
            Ok(vals) => {
                if vals.is_empty() {
                    return Ok(constants::NULL_VALUE.to_string());
                }

                if vals.len() == 1 {
                    // Single result - convert back to JSON
                    let json_val = serde_json::Value::from(vals[0].clone());
                    return serde_json::to_string_pretty(&json_val).map_err(|e| {
                        Error::serialization_error(format!("Failed to serialize result: {e}"))
                    });
                }

                // Multiple results - return as JSON array
                let json_vals: Vec<Value> = vals.into_iter().map(serde_json::Value::from).collect();
                let array = Value::Array(json_vals);
                serde_json::to_string_pretty(&array).map_err(|e| {
                    Error::serialization_error(format!("Failed to serialize results: {e}"))
                })
            }
            Err(e) => Err(Error::jq_filter_error(
                format!("{:?}", filter),
                format!("Filter execution error: {e}"),
            )),
        }
    }

    #[cfg(not(feature = "jq"))]
    {
        // Basic JQ-like functionality without full jq library
        apply_basic_jq_filter(&json_value, filter)
    }
}

#[cfg(not(feature = "jq"))]
/// Basic JQ-like functionality for common cases
fn apply_basic_jq_filter(json_value: &Value, filter: &str) -> Result<String, Error> {
    // Check if the filter uses advanced features
    let uses_advanced_features = filter.contains('[')
        || filter.contains(']')
        || filter.contains('|')
        || filter.contains('(')
        || filter.contains(')')
        || filter.contains("select")
        || filter.contains("map")
        || filter.contains("length");

    if uses_advanced_features {
        // ast-grep-ignore: no-println
        eprintln!(
            "{} Advanced JQ features require building with --features jq",
            crate::constants::MSG_WARNING_PREFIX
        );
        // ast-grep-ignore: no-println
        eprintln!("         Currently only basic field access is supported (e.g., '.field', '.nested.field')");
        // ast-grep-ignore: no-println
        eprintln!("         To enable full JQ support: cargo install aperture-cli --features jq");
    }

    let result = match filter {
        "." => json_value.clone(),
        ".[]" => {
            // Handle array iteration
            match json_value {
                Value::Array(arr) => {
                    // Return array elements as a JSON array
                    Value::Array(arr.clone())
                }
                Value::Object(obj) => {
                    // Return object values as an array
                    Value::Array(obj.values().cloned().collect())
                }
                _ => Value::Null,
            }
        }
        ".length" => {
            // Handle length operation
            match json_value {
                Value::Array(arr) => Value::Number(arr.len().into()),
                Value::Object(obj) => Value::Number(obj.len().into()),
                Value::String(s) => Value::Number(s.len().into()),
                _ => Value::Null,
            }
        }
        filter if filter.starts_with(".[].") => {
            // Handle array map like .[].name
            let field_path = &filter[4..]; // Remove ".[].""
            match json_value {
                Value::Array(arr) => {
                    let mapped: Vec<Value> = arr
                        .iter()
                        .map(|item| get_nested_field(item, field_path))
                        .collect();
                    Value::Array(mapped)
                }
                _ => Value::Null,
            }
        }
        filter if filter.starts_with('.') => {
            // Handle simple field access like .name, .metadata.role
            let field_path = &filter[1..]; // Remove the leading dot
            get_nested_field(json_value, field_path)
        }
        _ => {
            return Err(Error::jq_filter_error(filter, "Unsupported JQ filter. Only basic field access like '.name' or '.metadata.role' is supported without the full jq library."));
        }
    };

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
