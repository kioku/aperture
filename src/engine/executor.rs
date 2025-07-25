use crate::cache::models::{CachedCommand, CachedSecurityScheme, CachedSpec};
use crate::cli::OutputFormat;
use crate::config::models::GlobalConfig;
use crate::config::url_resolver::BaseUrlResolver;
use crate::error::Error;
use crate::response_cache::{CacheConfig, CacheKey, CachedRequestInfo, ResponseCache};
use clap::ArgMatches;
use reqwest::header::{HeaderMap, HeaderName, HeaderValue};
use reqwest::Method;
use serde_json::Value;
use std::collections::HashMap;
use std::str::FromStr;

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
            "bearer" => Self::Bearer,
            "basic" => Self::Basic,
            "token" => Self::Token,
            "dsn" => Self::DSN,
            "apikey" => Self::ApiKey,
            _ => Self::Custom(s.to_string()),
        }
    }
}

/// Maximum number of rows to display in table format to prevent memory exhaustion
const MAX_TABLE_ROWS: usize = 1000;

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
) -> Result<Option<String>, Error> {
    // Find the operation from the command hierarchy
    let operation = find_operation(spec, matches)?;

    // Resolve base URL using the new priority hierarchy
    let resolver = BaseUrlResolver::new(spec);
    let resolver = if let Some(config) = global_config {
        resolver.with_global_config(config)
    } else {
        resolver
    };
    let base_url = resolver.resolve(base_url);

    // Build the full URL with path parameters
    let url = build_url(&base_url, &operation.path, operation, matches)?;

    // Create HTTP client with timeout
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(30))
        .build()
        .map_err(|e| Error::RequestFailed {
            reason: format!("Failed to create HTTP client: {e}"),
        })?;

    // Build headers including authentication and idempotency
    let mut headers = build_headers(spec, operation, matches)?;

    // Add idempotency key if provided
    if let Some(key) = idempotency_key {
        headers.insert(
            HeaderName::from_static("idempotency-key"),
            HeaderValue::from_str(key).map_err(|_| Error::InvalidIdempotencyKey)?,
        );
    }

    // Build request
    let method = Method::from_str(&operation.method).map_err(|_| Error::InvalidHttpMethod {
        method: operation.method.clone(),
    })?;

    let headers_clone = headers.clone(); // For dry-run output
    let mut request = client.request(method.clone(), &url).headers(headers);

    // Add request body if present
    // Get to the deepest subcommand matches
    let mut current_matches = matches;
    while let Some((_name, sub_matches)) = current_matches.subcommand() {
        current_matches = sub_matches;
    }

    let request_body = if operation.request_body.is_some() {
        if let Some(body_value) = current_matches.get_one::<String>("body") {
            let json_body: Value =
                serde_json::from_str(body_value).map_err(|e| Error::InvalidJsonBody {
                    reason: e.to_string(),
                })?;
            request = request.json(&json_body);
            Some(body_value.clone())
        } else {
            None
        }
    } else {
        None
    };

    // Check cache for response if caching is enabled
    let cache_key = if let Some(cache_cfg) = cache_config {
        if cache_cfg.enabled {
            // Create cache key from request details
            let header_map: HashMap<String, String> = headers_clone
                .iter()
                .map(|(k, v)| (k.as_str().to_string(), v.to_str().unwrap_or("").to_string()))
                .collect();

            let cache_key = CacheKey::from_request(
                &spec.name,
                &operation.operation_id,
                method.as_ref(),
                &url,
                &header_map,
                request_body.as_deref(),
            )?;

            let response_cache = ResponseCache::new(cache_cfg.clone())?;

            // Try to get cached response
            if let Some(cached_response) = response_cache.get(&cache_key).await? {
                // Use cached response
                let output = print_formatted_response(
                    &cached_response.body,
                    output_format,
                    jq_filter,
                    capture_output,
                )?;
                return Ok(output);
            }

            Some((cache_key, response_cache))
        } else {
            None
        }
    } else {
        None
    };

    // Handle dry-run mode - show request details without executing
    if dry_run {
        let dry_run_info = serde_json::json!({
            "dry_run": true,
            "method": operation.method,
            "url": url,
            "headers": headers_clone.iter().map(|(k, v)| (k.as_str(), v.to_str().unwrap_or("<binary>"))).collect::<std::collections::HashMap<_, _>>(),
            "operation_id": operation.operation_id
        });
        let dry_run_output =
            serde_json::to_string_pretty(&dry_run_info).map_err(|e| Error::SerializationError {
                reason: format!("Failed to serialize dry-run info: {e}"),
            })?;

        if capture_output {
            return Ok(Some(dry_run_output));
        }
        println!("{dry_run_output}");
        return Ok(None);
    }

    // Execute request
    let response = request.send().await.map_err(|e| Error::RequestFailed {
        reason: e.to_string(),
    })?;

    let status = response.status();
    let response_headers: HashMap<String, String> = response
        .headers()
        .iter()
        .map(|(k, v)| (k.as_str().to_string(), v.to_str().unwrap_or("").to_string()))
        .collect();

    let response_text = response
        .text()
        .await
        .map_err(|e| Error::ResponseReadError {
            reason: e.to_string(),
        })?;

    // Check if request was successful
    if !status.is_success() {
        // Gather context for enhanced error reporting
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

        return Err(Error::HttpErrorWithContext {
            status: status.as_u16(),
            body: if response_text.is_empty() {
                "(empty response)".to_string()
            } else {
                response_text
            },
            api_name,
            operation_id,
            security_schemes,
        });
    }

    // Store response in cache if caching is enabled
    if let Some((cache_key, response_cache)) = cache_key {
        // Create cached request info
        let cached_request_info = CachedRequestInfo {
            method: method.to_string(),
            url: url.clone(),
            headers: headers_clone
                .iter()
                .map(|(k, v)| (k.as_str().to_string(), v.to_str().unwrap_or("").to_string()))
                .collect(),
            body_hash: request_body.as_ref().map(|body| {
                use sha2::{Digest, Sha256};
                let mut hasher = Sha256::new();
                hasher.update(body.as_bytes());
                format!("{:x}", hasher.finalize())
            }),
        };

        // Store in cache with custom TTL if specified
        let cache_ttl = cache_config.and_then(|cfg| {
            if cfg.default_ttl.as_secs() > 0 {
                Some(cfg.default_ttl)
            } else {
                None
            }
        });

        let _ = response_cache
            .store(
                &cache_key,
                &response_text,
                status.as_u16(),
                &response_headers,
                cached_request_info,
                cache_ttl,
            )
            .await;
    }

    // Print response in the requested format
    if response_text.is_empty() {
        Ok(None)
    } else {
        print_formatted_response(&response_text, output_format, jq_filter, capture_output)
    }
}

/// Finds the operation from the command hierarchy
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
    if let Some(operation_name) = subcommand_path.last() {
        for command in &spec.commands {
            // Convert operation_id to kebab-case for comparison
            let kebab_id = to_kebab_case(&command.operation_id);
            if &kebab_id == operation_name || command.method.to_lowercase() == *operation_name {
                return Ok(command);
            }
        }
    }

    Err(Error::OperationNotFound)
}

/// Builds the full URL with path parameters substituted
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
        if let Some(close) = url[open_pos..].find('}') {
            let close_pos = open_pos + close;
            let param_name = &url[open_pos + 1..close_pos];

            if let Some(value) = current_matches.get_one::<String>(param_name) {
                url.replace_range(open_pos..=close_pos, value);
                start = open_pos + value.len();
            } else {
                return Err(Error::MissingPathParameter {
                    name: param_name.to_string(),
                });
            }
        } else {
            break;
        }
    }

    // Add query parameters
    let mut query_params = Vec::new();
    for arg in current_matches.ids() {
        let arg_str = arg.as_str();
        // Skip non-query args - only process query parameters from the operation
        let is_query_param = operation
            .parameters
            .iter()
            .any(|p| p.name == arg_str && p.location == "query");
        if is_query_param {
            if let Some(value) = current_matches.get_one::<String>(arg_str) {
                query_params.push(format!("{}={}", arg_str, urlencoding::encode(value)));
            }
        }
    }

    if !query_params.is_empty() {
        url.push('?');
        url.push_str(&query_params.join("&"));
    }

    Ok(url)
}

/// Builds headers including authentication
fn build_headers(
    spec: &CachedSpec,
    operation: &CachedCommand,
    matches: &ArgMatches,
) -> Result<HeaderMap, Error> {
    let mut headers = HeaderMap::new();

    // Add default headers
    headers.insert("User-Agent", HeaderValue::from_static("aperture/0.1.0"));
    headers.insert("Accept", HeaderValue::from_static("application/json"));

    // Get to the deepest subcommand matches
    let mut current_matches = matches;
    while let Some((_name, sub_matches)) = current_matches.subcommand() {
        current_matches = sub_matches;
    }

    // Add header parameters from matches
    for param in &operation.parameters {
        if param.location == "header" {
            if let Some(value) = current_matches.get_one::<String>(&param.name) {
                let header_name =
                    HeaderName::from_str(&param.name).map_err(|e| Error::InvalidHeaderName {
                        name: param.name.clone(),
                        reason: e.to_string(),
                    })?;
                let header_value =
                    HeaderValue::from_str(value).map_err(|e| Error::InvalidHeaderValue {
                        name: param.name.clone(),
                        reason: e.to_string(),
                    })?;
                headers.insert(header_name, header_value);
            }
        }
    }

    // Add authentication headers based on security requirements
    for security_scheme_name in &operation.security_requirements {
        if let Some(security_scheme) = spec.security_schemes.get(security_scheme_name) {
            add_authentication_header(&mut headers, security_scheme)?;
        }
    }

    // Add custom headers from --header/-H flags
    // Use try_get_many to avoid panic when header arg doesn't exist
    if let Ok(Some(custom_headers)) = current_matches.try_get_many::<String>("header") {
        for header_str in custom_headers {
            let (name, value) = parse_custom_header(header_str)?;
            let header_name =
                HeaderName::from_str(&name).map_err(|e| Error::InvalidHeaderName {
                    name: name.clone(),
                    reason: e.to_string(),
                })?;
            let header_value =
                HeaderValue::from_str(&value).map_err(|e| Error::InvalidHeaderValue {
                    name: name.clone(),
                    reason: e.to_string(),
                })?;
            headers.insert(header_name, header_value);
        }
    }

    Ok(headers)
}

/// Validates that a header value doesn't contain control characters
fn validate_header_value(name: &str, value: &str) -> Result<(), Error> {
    if value.chars().any(|c| c == '\r' || c == '\n' || c == '\0') {
        return Err(Error::InvalidHeaderValue {
            name: name.to_string(),
            reason: "Header value contains invalid control characters (newline, carriage return, or null)".to_string(),
        });
    }
    Ok(())
}

/// Parses a custom header string in the format "Name: Value" or "Name:Value"
fn parse_custom_header(header_str: &str) -> Result<(String, String), Error> {
    // Find the colon separator
    let colon_pos = header_str
        .find(':')
        .ok_or_else(|| Error::InvalidHeaderFormat {
            header: header_str.to_string(),
        })?;

    let name = header_str[..colon_pos].trim();
    let value = header_str[colon_pos + 1..].trim();

    if name.is_empty() {
        return Err(Error::EmptyHeaderName);
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

/// Adds an authentication header based on a security scheme
fn add_authentication_header(
    headers: &mut HeaderMap,
    security_scheme: &CachedSecurityScheme,
) -> Result<(), Error> {
    // Debug logging when RUST_LOG is set
    if std::env::var("RUST_LOG").is_ok() {
        eprintln!(
            "[DEBUG] Adding authentication header for scheme: {} (type: {})",
            security_scheme.name, security_scheme.scheme_type
        );
    }
    // Only process schemes that have aperture_secret mappings
    if let Some(aperture_secret) = &security_scheme.aperture_secret {
        // Read the secret from the environment variable
        let secret_value =
            std::env::var(&aperture_secret.name).map_err(|_| Error::SecretNotSet {
                scheme_name: security_scheme.name.clone(),
                env_var: aperture_secret.name.clone(),
            })?;

        // Validate the secret doesn't contain control characters
        validate_header_value("Authorization", &secret_value)?;

        // Build the appropriate header based on scheme type
        match security_scheme.scheme_type.as_str() {
            "apiKey" => {
                if let (Some(location), Some(param_name)) =
                    (&security_scheme.location, &security_scheme.parameter_name)
                {
                    if location == "header" {
                        let header_name = HeaderName::from_str(param_name).map_err(|e| {
                            Error::InvalidHeaderName {
                                name: param_name.clone(),
                                reason: e.to_string(),
                            }
                        })?;
                        let header_value = HeaderValue::from_str(&secret_value).map_err(|e| {
                            Error::InvalidHeaderValue {
                                name: param_name.clone(),
                                reason: e.to_string(),
                            }
                        })?;
                        headers.insert(header_name, header_value);
                    }
                    // Note: query and cookie locations are handled differently in request building
                }
            }
            "http" => {
                if let Some(scheme_str) = &security_scheme.scheme {
                    let auth_scheme: AuthScheme = scheme_str.as_str().into();
                    let auth_value = match &auth_scheme {
                        AuthScheme::Bearer => {
                            format!("Bearer {secret_value}")
                        }
                        AuthScheme::Basic => {
                            // Basic auth expects "username:password" format in the secret
                            // The secret should contain the raw "username:password" string
                            // We'll base64 encode it before adding to the header
                            use base64::{engine::general_purpose, Engine as _};
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
                        Error::InvalidHeaderValue {
                            name: "Authorization".to_string(),
                            reason: e.to_string(),
                        }
                    })?;
                    headers.insert("Authorization", header_value);

                    // Debug logging
                    if std::env::var("RUST_LOG").is_ok() {
                        match &auth_scheme {
                            AuthScheme::Bearer => {
                                eprintln!("[DEBUG] Added Bearer authentication header");
                            }
                            AuthScheme::Basic => eprintln!(
                                "[DEBUG] Added Basic authentication header (base64 encoded)"
                            ),
                            _ => eprintln!(
                                "[DEBUG] Added custom HTTP auth header with scheme: {scheme_str}"
                            ),
                        }
                    }
                }
            }
            _ => {
                return Err(Error::UnsupportedSecurityScheme {
                    scheme_type: security_scheme.scheme_type.clone(),
                });
            }
        }
    }

    Ok(())
}

/// Converts a string to kebab-case (copied from generator.rs)
fn to_kebab_case(s: &str) -> String {
    let mut result = String::new();
    let mut prev_lowercase = false;

    for (i, ch) in s.chars().enumerate() {
        if ch.is_uppercase() && i > 0 && prev_lowercase {
            result.push('-');
        }
        result.push(ch.to_ascii_lowercase());
        prev_lowercase = ch.is_lowercase();
    }

    result
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
            println!("{output}");
        }
        OutputFormat::Table => {
            // Convert JSON to table format
            if let Ok(json_value) = serde_json::from_str::<Value>(&processed_text) {
                let table_output = print_as_table(&json_value, capture_output)?;
                if capture_output {
                    return Ok(table_output);
                }
            } else {
                // If not JSON, output as-is
                if capture_output {
                    return Ok(Some(processed_text));
                }
                println!("{processed_text}");
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

/// Prints JSON data as a formatted table
#[allow(clippy::unnecessary_wraps, clippy::too_many_lines)]
fn print_as_table(json_value: &Value, capture_output: bool) -> Result<Option<String>, Error> {
    use std::collections::BTreeMap;
    use tabled::Table;

    match json_value {
        Value::Array(items) => {
            if items.is_empty() {
                if capture_output {
                    return Ok(Some("(empty array)".to_string()));
                }
                println!("(empty array)");
                return Ok(None);
            }

            // Check if array is too large
            if items.len() > MAX_TABLE_ROWS {
                let msg1 = format!(
                    "Array too large: {} items (max {} for table display)",
                    items.len(),
                    MAX_TABLE_ROWS
                );
                let msg2 = "Use --format json or --jq to process the full data";

                if capture_output {
                    return Ok(Some(format!("{msg1}\n{msg2}")));
                }
                println!("{msg1}");
                println!("{msg2}");
                return Ok(None);
            }

            // Try to create a table from array of objects
            if let Some(Value::Object(_)) = items.first() {
                // Create table for array of objects
                let mut table_data: Vec<BTreeMap<String, String>> = Vec::new();

                for item in items {
                    if let Value::Object(obj) = item {
                        let mut row = BTreeMap::new();
                        for (key, value) in obj {
                            row.insert(key.clone(), format_value_for_table(value));
                        }
                        table_data.push(row);
                    }
                }

                if !table_data.is_empty() {
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
                    if capture_output {
                        return Ok(Some(table.to_string()));
                    }
                    println!("{table}");
                    return Ok(None);
                }
            }

            // Fallback: print array as numbered list
            if capture_output {
                let mut output = String::new();
                for (i, item) in items.iter().enumerate() {
                    use std::fmt::Write;
                    writeln!(&mut output, "{}: {}", i, format_value_for_table(item)).unwrap();
                }
                return Ok(Some(output.trim_end().to_string()));
            }
            for (i, item) in items.iter().enumerate() {
                println!("{}: {}", i, format_value_for_table(item));
            }
        }
        Value::Object(obj) => {
            // Check if object has too many fields
            if obj.len() > MAX_TABLE_ROWS {
                let msg1 = format!(
                    "Object too large: {} fields (max {} for table display)",
                    obj.len(),
                    MAX_TABLE_ROWS
                );
                let msg2 = "Use --format json or --jq to process the full data";

                if capture_output {
                    return Ok(Some(format!("{msg1}\n{msg2}")));
                }
                println!("{msg1}");
                println!("{msg2}");
                return Ok(None);
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
            if capture_output {
                return Ok(Some(table.to_string()));
            }
            println!("{table}");
        }
        _ => {
            // For primitive values, just print them
            let formatted = format_value_for_table(json_value);
            if capture_output {
                return Ok(Some(formatted));
            }
            println!("{formatted}");
        }
    }

    Ok(None)
}

/// Formats a JSON value for display in a table cell
fn format_value_for_table(value: &Value) -> String {
    match value {
        Value::Null => "null".to_string(),
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
    let json_value: Value =
        serde_json::from_str(response_text).map_err(|e| Error::JqFilterError {
            reason: format!("Response is not valid JSON: {e}"),
        })?;

    #[cfg(feature = "jq")]
    {
        // Use jaq (pure Rust implementation) when available
        use jaq_interpret::{Ctx, FilterT, ParseCtx, RcIter, Val};
        use jaq_parse::parse;
        use jaq_std::std;

        // Parse the filter expression
        let (expr, errs) = parse(filter, jaq_parse::main());
        if !errs.is_empty() {
            return Err(Error::JqFilterError {
                reason: format!("Parse error in jq expression: {}", errs[0]),
            });
        }

        // Create parsing context and compile the filter
        let mut ctx = ParseCtx::new(Vec::new());
        ctx.insert_defs(std());
        let filter = ctx.compile(expr.unwrap());

        // Convert serde_json::Value to jaq Val
        let jaq_value = serde_json_to_jaq_val(&json_value);

        // Execute the filter
        let inputs = RcIter::new(core::iter::empty());
        let ctx = Ctx::new([], &inputs);
        let results: Result<Vec<Val>, _> = filter.run((ctx, jaq_value.into())).collect();

        match results {
            Ok(vals) => {
                if vals.is_empty() {
                    Ok("null".to_string())
                } else if vals.len() == 1 {
                    // Single result - convert back to JSON
                    let json_val = jaq_val_to_serde_json(&vals[0]);
                    serde_json::to_string_pretty(&json_val).map_err(|e| Error::JqFilterError {
                        reason: format!("Failed to serialize result: {e}"),
                    })
                } else {
                    // Multiple results - return as JSON array
                    let json_vals: Vec<Value> = vals.iter().map(jaq_val_to_serde_json).collect();
                    let array = Value::Array(json_vals);
                    serde_json::to_string_pretty(&array).map_err(|e| Error::JqFilterError {
                        reason: format!("Failed to serialize results: {e}"),
                    })
                }
            }
            Err(e) => Err(Error::JqFilterError {
                reason: format!("Filter execution error: {e}"),
            }),
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
        eprintln!("Warning: Advanced JQ features require building with --features jq");
        eprintln!("         Currently only basic field access is supported (e.g., '.field', '.nested.field')");
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
            return Err(Error::JqFilterError {
                reason: format!("Unsupported JQ filter: '{filter}'. Only basic field access like '.name' or '.metadata.role' is supported without the full jq library."),
            });
        }
    };

    serde_json::to_string_pretty(&result).map_err(|e| Error::JqFilterError {
        reason: format!("Failed to serialize filtered result: {e}"),
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
            if let Ok(index) = index_str.parse::<usize>() {
                match current {
                    Value::Array(arr) => {
                        if let Some(item) = arr.get(index) {
                            current = item;
                        } else {
                            return Value::Null;
                        }
                    }
                    _ => return Value::Null,
                }
            } else {
                return Value::Null;
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
                if let Ok(index) = part.parse::<usize>() {
                    if let Some(item) = arr.get(index) {
                        current = item;
                    } else {
                        return Value::Null;
                    }
                } else {
                    return Value::Null;
                }
            }
            _ => return Value::Null,
        }
    }

    current.clone()
}

#[cfg(feature = "jq")]
/// Convert serde_json::Value to jaq Val
fn serde_json_to_jaq_val(value: &Value) -> jaq_interpret::Val {
    use jaq_interpret::Val;
    use std::rc::Rc;

    match value {
        Value::Null => Val::Null,
        Value::Bool(b) => Val::Bool(*b),
        Value::Number(n) => {
            if let Some(i) = n.as_i64() {
                // Convert i64 to isize safely
                if let Ok(isize_val) = isize::try_from(i) {
                    Val::Int(isize_val)
                } else {
                    // Fallback to float for large numbers
                    Val::Float(i as f64)
                }
            } else if let Some(f) = n.as_f64() {
                Val::Float(f)
            } else {
                Val::Null
            }
        }
        Value::String(s) => Val::Str(s.clone().into()),
        Value::Array(arr) => {
            let jaq_arr: Vec<Val> = arr.iter().map(serde_json_to_jaq_val).collect();
            Val::Arr(Rc::new(jaq_arr))
        }
        Value::Object(obj) => {
            let mut jaq_obj = indexmap::IndexMap::with_hasher(ahash::RandomState::new());
            for (k, v) in obj {
                jaq_obj.insert(Rc::new(k.clone()), serde_json_to_jaq_val(v));
            }
            Val::Obj(Rc::new(jaq_obj))
        }
    }
}

#[cfg(feature = "jq")]
/// Convert jaq Val to serde_json::Value
fn jaq_val_to_serde_json(val: &jaq_interpret::Val) -> Value {
    use jaq_interpret::Val;

    match val {
        Val::Null => Value::Null,
        Val::Bool(b) => Value::Bool(*b),
        Val::Int(i) => {
            // Convert isize to i64
            Value::Number((*i as i64).into())
        }
        Val::Float(f) => {
            if let Some(num) = serde_json::Number::from_f64(*f) {
                Value::Number(num)
            } else {
                Value::Null
            }
        }
        Val::Str(s) => Value::String(s.to_string()),
        Val::Arr(arr) => {
            let json_arr: Vec<Value> = arr.iter().map(jaq_val_to_serde_json).collect();
            Value::Array(json_arr)
        }
        Val::Obj(obj) => {
            let mut json_obj = serde_json::Map::new();
            for (k, v) in obj.iter() {
                json_obj.insert(k.to_string(), jaq_val_to_serde_json(v));
            }
            Value::Object(json_obj)
        }
        _ => Value::Null, // Handle any other Val variants as null
    }
}
