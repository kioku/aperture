use crate::cache::models::{CachedCommand, CachedParameter, CachedRequestBody, CachedSpec};
use crate::config::models::GlobalConfig;
use crate::config::url_resolver::BaseUrlResolver;
use crate::error::Error;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// JSON manifest describing all available commands and parameters for an API context.
/// This is output when the `--describe-json` flag is used.
#[derive(Debug, Serialize, Deserialize)]
pub struct ApiCapabilityManifest {
    /// Basic API metadata
    pub api: ApiInfo,
    /// Available command groups organized by tags
    pub commands: HashMap<String, Vec<CommandInfo>>,
    /// Security requirements for this API
    pub security: Option<SecurityInfo>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct ApiInfo {
    /// API name
    pub name: String,
    /// API version
    pub version: String,
    /// API description
    pub description: Option<String>,
    /// Base URL for the API
    pub base_url: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct CommandInfo {
    /// Command name (kebab-case operation ID)
    pub name: String,
    /// HTTP method
    pub method: String,
    /// API path with parameters
    pub path: String,
    /// Operation description
    pub description: Option<String>,
    /// Operation ID from the `OpenAPI` spec
    pub operation_id: String,
    /// Parameters for this operation
    pub parameters: Vec<ParameterInfo>,
    /// Request body information if applicable
    pub request_body: Option<RequestBodyInfo>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct ParameterInfo {
    /// Parameter name
    pub name: String,
    /// Parameter location (path, query, header)
    pub location: String,
    /// Whether the parameter is required
    pub required: bool,
    /// Parameter type
    pub param_type: String,
    /// Parameter description
    pub description: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct RequestBodyInfo {
    /// Whether the request body is required
    pub required: bool,
    /// Content type (e.g., "application/json")
    pub content_type: String,
    /// Description of the request body
    pub description: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct SecurityInfo {
    /// Type of security scheme (apiKey, http)
    pub scheme_type: String,
    /// Additional security details
    pub details: HashMap<String, String>,
}

/// Converts a kebab-case string from operationId to a CLI command name
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

/// Generates a capability manifest from a cached API specification.
///
/// This function creates a comprehensive JSON description of all available commands,
/// parameters, and security requirements for the given API context.
///
/// # Arguments
/// * `spec` - The cached API specification
/// * `global_config` - Optional global configuration for URL resolution
///
/// # Returns
/// * `Ok(String)` - JSON-formatted capability manifest
/// * `Err(Error)` - If JSON serialization fails
///
/// # Errors
/// Returns an error if JSON serialization fails
pub fn generate_capability_manifest(
    spec: &CachedSpec,
    global_config: Option<&GlobalConfig>,
) -> Result<String, Error> {
    let mut command_groups: HashMap<String, Vec<CommandInfo>> = HashMap::new();

    // Group commands by their tag (namespace) and convert to CommandInfo
    for cached_command in &spec.commands {
        let group_name = if cached_command.name.is_empty() {
            "default".to_string()
        } else {
            cached_command.name.clone()
        };

        let command_info = convert_cached_command_to_info(cached_command);
        command_groups
            .entry(group_name)
            .or_default()
            .push(command_info);
    }

    // Resolve base URL using the same priority hierarchy as executor
    let resolver = BaseUrlResolver::new(spec);
    let resolver = if let Some(config) = global_config {
        resolver.with_global_config(config)
    } else {
        resolver
    };
    let base_url = resolver.resolve(None);

    // Create the manifest
    let manifest = ApiCapabilityManifest {
        api: ApiInfo {
            name: spec.name.clone(),
            version: spec.version.clone(),
            description: None, // Not available in cached spec
            base_url,
        },
        commands: command_groups,
        security: extract_security_info(spec),
    };

    // Serialize to JSON
    serde_json::to_string_pretty(&manifest).map_err(Error::Json)
}

/// Converts a `CachedCommand` to `CommandInfo` for the manifest
fn convert_cached_command_to_info(cached_command: &CachedCommand) -> CommandInfo {
    let command_name = if cached_command.operation_id.is_empty() {
        cached_command.method.to_lowercase()
    } else {
        to_kebab_case(&cached_command.operation_id)
    };

    let parameters: Vec<ParameterInfo> = cached_command
        .parameters
        .iter()
        .map(convert_cached_parameter_to_info)
        .collect();

    let request_body = cached_command
        .request_body
        .as_ref()
        .map(convert_cached_request_body_to_info);

    CommandInfo {
        name: command_name,
        method: cached_command.method.clone(),
        path: cached_command.path.clone(),
        description: cached_command.description.clone(),
        operation_id: cached_command.operation_id.clone(),
        parameters,
        request_body,
    }
}

/// Converts a `CachedParameter` to `ParameterInfo` for the manifest
fn convert_cached_parameter_to_info(cached_param: &CachedParameter) -> ParameterInfo {
    ParameterInfo {
        name: cached_param.name.clone(),
        location: cached_param.location.clone(),
        required: cached_param.required,
        param_type: cached_param
            .schema
            .clone()
            .unwrap_or_else(|| "string".to_string()),
        description: None, // Not available in cached parameter
    }
}

/// Converts a `CachedRequestBody` to `RequestBodyInfo` for the manifest
fn convert_cached_request_body_to_info(cached_body: &CachedRequestBody) -> RequestBodyInfo {
    RequestBodyInfo {
        required: cached_body.required,
        content_type: cached_body.content.clone(), // Using content field as content_type
        description: None,                         // Not available in cached request body
    }
}

/// Extracts security information from the cached spec for the capability manifest
fn extract_security_info(spec: &CachedSpec) -> Option<SecurityInfo> {
    if spec.security_schemes.is_empty() {
        return None;
    }

    // For now, we'll create a summary of all available security schemes
    // In a real implementation, you might want to be more specific about which
    // schemes are required for which operations
    let mut details = HashMap::new();
    let mut primary_scheme_type = String::new();

    for (name, scheme) in &spec.security_schemes {
        details.insert(format!("{name}_type"), scheme.scheme_type.clone());

        if let Some(location) = &scheme.location {
            details.insert(format!("{name}_location"), location.clone());
        }

        if let Some(param_name) = &scheme.parameter_name {
            details.insert(format!("{name}_parameter"), param_name.clone());
        }

        if let Some(http_scheme) = &scheme.scheme {
            details.insert(format!("{name}_scheme"), http_scheme.clone());
        }

        // Add information about x-aperture-secret mapping if present
        if let Some(aperture_secret) = &scheme.aperture_secret {
            details.insert(format!("{name}_env_var"), aperture_secret.name.clone());
            details.insert(format!("{name}_source"), aperture_secret.source.clone());
        }

        // Use the first scheme as the primary type for the summary
        if primary_scheme_type.is_empty() {
            primary_scheme_type.clone_from(&scheme.scheme_type);
        }
    }

    // Add a summary of available schemes
    let scheme_names: Vec<String> = spec.security_schemes.keys().cloned().collect();
    details.insert("available_schemes".to_string(), scheme_names.join(", "));

    Some(SecurityInfo {
        scheme_type: if primary_scheme_type.is_empty() {
            "mixed".to_string()
        } else {
            primary_scheme_type
        },
        details,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cache::models::{CachedCommand, CachedParameter, CachedSpec};

    #[test]
    fn test_to_kebab_case() {
        assert_eq!(to_kebab_case("getUserById"), "get-user-by-id");
        assert_eq!(to_kebab_case("createUser"), "create-user");
        assert_eq!(to_kebab_case("list"), "list");
        assert_eq!(to_kebab_case("GET"), "get");
    }

    #[test]
    fn test_generate_capability_manifest() {
        use crate::cache::models::{CachedApertureSecret, CachedSecurityScheme};

        let mut security_schemes = HashMap::new();
        security_schemes.insert(
            "bearerAuth".to_string(),
            CachedSecurityScheme {
                name: "bearerAuth".to_string(),
                scheme_type: "http".to_string(),
                scheme: Some("bearer".to_string()),
                location: Some("header".to_string()),
                parameter_name: Some("Authorization".to_string()),
                aperture_secret: Some(CachedApertureSecret {
                    source: "env".to_string(),
                    name: "API_TOKEN".to_string(),
                }),
            },
        );

        let spec = CachedSpec {
            name: "Test API".to_string(),
            version: "1.0.0".to_string(),
            commands: vec![CachedCommand {
                name: "users".to_string(),
                operation_id: "getUserById".to_string(),
                method: "GET".to_string(),
                path: "/users/{id}".to_string(),
                description: Some("Get user by ID".to_string()),
                parameters: vec![CachedParameter {
                    name: "id".to_string(),
                    location: "path".to_string(),
                    required: true,
                    schema: Some("string".to_string()),
                }],
                request_body: None,
                responses: vec![],
                security_requirements: vec!["bearerAuth".to_string()],
            }],
            base_url: Some("https://test-api.example.com".to_string()),
            servers: vec!["https://test-api.example.com".to_string()],
            security_schemes,
        };

        let manifest_json = generate_capability_manifest(&spec, None).unwrap();
        let manifest: ApiCapabilityManifest = serde_json::from_str(&manifest_json).unwrap();

        assert_eq!(manifest.api.name, "Test API");
        assert_eq!(manifest.api.version, "1.0.0");
        assert!(manifest.commands.contains_key("users"));

        let users_commands = &manifest.commands["users"];
        assert_eq!(users_commands.len(), 1);
        assert_eq!(users_commands[0].name, "get-user-by-id");
        assert_eq!(users_commands[0].method, "GET");
        assert_eq!(users_commands[0].parameters.len(), 1);
        assert_eq!(users_commands[0].parameters[0].name, "id");

        // Test security information extraction
        assert!(manifest.security.is_some());
        let security = manifest.security.unwrap();
        assert_eq!(security.scheme_type, "http");
        assert!(security.details.contains_key("bearerAuth_type"));
        assert_eq!(security.details["bearerAuth_type"], "http");
        assert!(security.details.contains_key("bearerAuth_env_var"));
        assert_eq!(security.details["bearerAuth_env_var"], "API_TOKEN");
    }
}
