use crate::cache::models::{CachedCommand, CachedSecurityScheme, CachedSpec};
use crate::config::models::GlobalConfig;
use crate::config::url_resolver::BaseUrlResolver;
use crate::error::Error;
use clap::ArgMatches;
use reqwest::header::{HeaderMap, HeaderName, HeaderValue};
use reqwest::Method;
use serde_json::Value;
use std::str::FromStr;

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
///
/// # Returns
/// * `Ok(())` - Request executed successfully or dry-run completed
/// * `Err(Error)` - Request failed or validation error
///
/// # Errors
/// Returns errors for authentication failures, network issues, or response validation
///
/// # Panics
/// Panics if JSON serialization of dry-run information fails (extremely unlikely)
pub async fn execute_request(
    spec: &CachedSpec,
    matches: &ArgMatches,
    base_url: Option<&str>,
    dry_run: bool,
    idempotency_key: Option<&str>,
    global_config: Option<&GlobalConfig>,
) -> Result<(), Error> {
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

    // Create HTTP client
    let client = reqwest::Client::new();

    // Build headers including authentication and idempotency
    let mut headers = build_headers(spec, operation, matches)?;

    // Add idempotency key if provided
    if let Some(key) = idempotency_key {
        headers.insert(
            HeaderName::from_static("idempotency-key"),
            HeaderValue::from_str(key)
                .map_err(|_| Error::Config("Invalid idempotency key".to_string()))?,
        );
    }

    // Build request
    let method = Method::from_str(&operation.method)
        .map_err(|_| Error::Config(format!("Invalid HTTP method: {}", operation.method)))?;

    let headers_clone = headers.clone(); // For dry-run output
    let mut request = client.request(method.clone(), &url).headers(headers);

    // Add request body if present
    // Get to the deepest subcommand matches
    let mut current_matches = matches;
    while let Some((_name, sub_matches)) = current_matches.subcommand() {
        current_matches = sub_matches;
    }

    // Only check for body if the operation expects one
    if operation.request_body.is_some() {
        if let Some(body_value) = current_matches.get_one::<String>("body") {
            let json_body: Value = serde_json::from_str(body_value)
                .map_err(|e| Error::Config(format!("Invalid JSON body: {e}")))?;
            request = request.json(&json_body);
        }
    }

    // Handle dry-run mode - show request details without executing
    if dry_run {
        let dry_run_info = serde_json::json!({
            "dry_run": true,
            "method": operation.method,
            "url": url,
            "headers": headers_clone.iter().map(|(k, v)| (k.as_str(), v.to_str().unwrap_or("<binary>"))).collect::<std::collections::HashMap<_, _>>(),
            "operation_id": operation.operation_id
        });
        println!("{}", serde_json::to_string_pretty(&dry_run_info).unwrap());
        return Ok(());
    }

    // Execute request
    println!("Executing {method} {url}");
    let response = request
        .send()
        .await
        .map_err(|e| Error::Config(format!("Request failed: {e}")))?;

    let status = response.status();
    let response_text = response
        .text()
        .await
        .map_err(|e| Error::Config(format!("Failed to read response: {e}")))?;

    // Check if request was successful
    if !status.is_success() {
        return Err(Error::Config(format!(
            "Request failed with status {}: {}",
            status,
            if response_text.is_empty() {
                "(empty response)"
            } else {
                &response_text
            }
        )));
    }

    // Print response
    if !response_text.is_empty() {
        // Try to pretty-print JSON
        if let Ok(json_value) = serde_json::from_str::<Value>(&response_text) {
            if let Ok(pretty) = serde_json::to_string_pretty(&json_value) {
                println!("{pretty}");
            } else {
                println!("{response_text}");
            }
        } else {
            println!("{response_text}");
        }
    }

    Ok(())
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

    Err(Error::Config(
        "Could not find operation from command path".to_string(),
    ))
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
                return Err(Error::Config(format!(
                    "Missing path parameter: {param_name}"
                )));
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
                let header_name = HeaderName::from_str(&param.name).map_err(|e| {
                    Error::Config(format!("Invalid header name {}: {e}", param.name))
                })?;
                let header_value = HeaderValue::from_str(value).map_err(|e| {
                    Error::Config(format!("Invalid header value for {}: {e}", param.name))
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
            let header_name = HeaderName::from_str(&name)
                .map_err(|e| Error::Config(format!("Invalid header name '{name}': {e}")))?;
            let header_value = HeaderValue::from_str(&value)
                .map_err(|e| Error::Config(format!("Invalid header value for '{name}': {e}")))?;
            headers.insert(header_name, header_value);
        }
    }

    Ok(headers)
}

/// Parses a custom header string in the format "Name: Value" or "Name:Value"
fn parse_custom_header(header_str: &str) -> Result<(String, String), Error> {
    // Find the colon separator
    let colon_pos = header_str.find(':').ok_or_else(|| {
        Error::Config(format!(
            "Invalid header format '{header_str}'. Expected 'Name: Value'"
        ))
    })?;

    let name = header_str[..colon_pos].trim();
    let value = header_str[colon_pos + 1..].trim();

    if name.is_empty() {
        return Err(Error::Config(format!(
            "Invalid header format '{header_str}'. Header name cannot be empty"
        )));
    }

    // Support environment variable expansion in header values
    let expanded_value = if value.starts_with("${") && value.ends_with('}') {
        // Extract environment variable name
        let var_name = &value[2..value.len() - 1];
        std::env::var(var_name).unwrap_or_else(|_| value.to_string())
    } else {
        value.to_string()
    };

    Ok((name.to_string(), expanded_value))
}

/// Adds an authentication header based on a security scheme
fn add_authentication_header(
    headers: &mut HeaderMap,
    security_scheme: &CachedSecurityScheme,
) -> Result<(), Error> {
    // Only process schemes that have aperture_secret mappings
    if let Some(aperture_secret) = &security_scheme.aperture_secret {
        // Read the secret from the environment variable
        let secret_value = std::env::var(&aperture_secret.name).map_err(|_| {
            Error::Config(format!(
                "Environment variable '{}' required for authentication '{}' is not set",
                aperture_secret.name, security_scheme.name
            ))
        })?;

        // Build the appropriate header based on scheme type
        match security_scheme.scheme_type.as_str() {
            "apiKey" => {
                if let (Some(location), Some(param_name)) =
                    (&security_scheme.location, &security_scheme.parameter_name)
                {
                    if location == "header" {
                        let header_name = HeaderName::from_str(param_name).map_err(|e| {
                            Error::Config(format!("Invalid header name '{param_name}': {e}"))
                        })?;
                        let header_value = HeaderValue::from_str(&secret_value).map_err(|e| {
                            Error::Config(format!("Invalid header value for '{param_name}': {e}"))
                        })?;
                        headers.insert(header_name, header_value);
                    }
                    // Note: query and cookie locations are handled differently in request building
                }
            }
            "http" => {
                if let Some(scheme) = &security_scheme.scheme {
                    match scheme.as_str() {
                        "bearer" => {
                            let auth_value = format!("Bearer {secret_value}");
                            let header_value = HeaderValue::from_str(&auth_value).map_err(|e| {
                                Error::Config(format!("Invalid Authorization header value: {e}"))
                            })?;
                            headers.insert("Authorization", header_value);
                        }
                        "basic" => {
                            // Basic auth expects "username:password" format in the secret
                            let auth_value = format!("Basic {secret_value}");
                            let header_value = HeaderValue::from_str(&auth_value).map_err(|e| {
                                Error::Config(format!("Invalid Authorization header value: {e}"))
                            })?;
                            headers.insert("Authorization", header_value);
                        }
                        _ => {
                            return Err(Error::Config(format!(
                                "Unsupported HTTP authentication scheme: {scheme}"
                            )));
                        }
                    }
                }
            }
            _ => {
                return Err(Error::Config(format!(
                    "Unsupported security scheme type: {}",
                    security_scheme.scheme_type
                )));
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
