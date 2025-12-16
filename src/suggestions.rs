//! Smart suggestions for error messages and command discovery

use crate::cache::models::CachedSpec;
use crate::search::CommandSearcher;
use std::collections::BTreeMap;

/// Generate suggestions for an operation that wasn't found
#[must_use]
pub fn suggest_similar_operations(spec: &CachedSpec, attempted_operation: &str) -> Vec<String> {
    let searcher = CommandSearcher::new();

    // Create a BTreeMap with just this spec
    let mut specs = BTreeMap::new();
    specs.insert(spec.name.clone(), spec.clone());

    // Search for similar commands
    let results = searcher.search(&specs, attempted_operation, None);

    // Take top 3 most relevant suggestions
    results.map_or_else(
        |_| Vec::new(), // Return empty vec if search fails
        |search_results| {
            search_results
                .into_iter()
                .take(3)
                .map(|result| {
                    format!(
                        "aperture api {} {}",
                        result.api_context, result.command_path
                    )
                })
                .collect()
        },
    )
}

/// Generate suggestions for missing parameters
#[must_use]
pub fn suggest_parameter_format(param_name: &str, param_type: Option<&str>) -> String {
    let type_hint = param_type.unwrap_or("value");
    format!("--{param_name} <{type_hint}>")
}

/// Generate suggestions for invalid parameter values
#[must_use]
pub fn suggest_valid_values(param_name: &str, valid_values: &[String]) -> String {
    if valid_values.is_empty() {
        return format!("Check the parameter documentation for valid values of '{param_name}'");
    }

    let values = valid_values
        .iter()
        .take(5)
        .map(|v| format!("'{v}'"))
        .collect::<Vec<_>>()
        .join(", ");

    let suffix = if valid_values.len() > 5 {
        "include"
    } else {
        "are"
    };
    let ellipsis = if valid_values.len() > 5 { ", ..." } else { "" };

    format!("Valid values for '{param_name}' {suffix}: {values}{ellipsis}")
}

/// Generate suggestions for authentication errors
#[must_use]
pub fn suggest_auth_fix(scheme_name: &str, env_var: Option<&str>) -> String {
    env_var.map_or_else(
        || format!("Configure authentication for '{scheme_name}' using 'aperture config secrets'"),
        |var| format!("Set the {var} environment variable or run 'aperture config secrets' to configure authentication")
    )
}

/// Generate suggestions for network errors
#[must_use]
pub fn suggest_network_fix(url: &str, error: &str) -> String {
    match () {
        () if error.contains("DNS") || error.contains("resolve") => {
            format!("Check that the host '{url}' is reachable and the URL is correct")
        }
        () if error.contains("timeout") => {
            "Try increasing the timeout with --timeout or check your network connection".to_string()
        }
        () if error.contains("refused") => {
            format!("The server at '{url}' refused the connection. Check if the service is running")
        }
        () => "Check your network connection and try again".to_string(),
    }
}
