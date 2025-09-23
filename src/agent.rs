use crate::cache::models::{
    CachedApertureSecret, CachedCommand, CachedParameter, CachedRequestBody, CachedSpec,
};
use crate::config::models::GlobalConfig;
use crate::config::url_resolver::BaseUrlResolver;
use crate::constants;
use crate::error::Error;
use crate::spec::resolve_parameter_reference;
use crate::utils::to_kebab_case;
use openapiv3::{OpenAPI, Operation, Parameter as OpenApiParameter, ReferenceOr, SecurityScheme};
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
    /// Security schemes available for this API
    pub security_schemes: HashMap<String, SecuritySchemeInfo>,
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
    /// Operation summary
    #[serde(skip_serializing_if = "Option::is_none")]
    pub summary: Option<String>,
    /// Operation ID from the `OpenAPI` spec
    pub operation_id: String,
    /// Parameters for this operation
    pub parameters: Vec<ParameterInfo>,
    /// Request body information if applicable
    pub request_body: Option<RequestBodyInfo>,
    /// Security requirements for this operation
    #[serde(skip_serializing_if = "Vec::is_empty", default)]
    pub security_requirements: Vec<String>,
    /// Tags associated with this operation
    #[serde(skip_serializing_if = "Vec::is_empty", default)]
    pub tags: Vec<String>,
    /// Whether this operation is deprecated
    #[serde(skip_serializing_if = "std::ops::Not::not", default)]
    pub deprecated: bool,
    /// External documentation URL
    #[serde(skip_serializing_if = "Option::is_none")]
    pub external_docs_url: Option<String>,
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
    /// Parameter format (e.g., int32, int64, date-time)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub format: Option<String>,
    /// Default value if specified
    #[serde(skip_serializing_if = "Option::is_none")]
    pub default_value: Option<String>,
    /// Enumeration of valid values
    #[serde(skip_serializing_if = "Vec::is_empty", default)]
    pub enum_values: Vec<String>,
    /// Example value
    #[serde(skip_serializing_if = "Option::is_none")]
    pub example: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct RequestBodyInfo {
    /// Whether the request body is required
    pub required: bool,
    /// Content type (e.g., "application/json")
    pub content_type: String,
    /// Description of the request body
    pub description: Option<String>,
    /// Example of the request body
    #[serde(skip_serializing_if = "Option::is_none")]
    pub example: Option<String>,
}

/// Detailed, parsable security scheme description
#[derive(Debug, Serialize, Deserialize)]
pub struct SecuritySchemeInfo {
    /// Type of security scheme (http, apiKey)
    #[serde(rename = "type")]
    pub scheme_type: String,
    /// Optional description of the security scheme
    pub description: Option<String>,
    /// Detailed scheme configuration
    #[serde(flatten)]
    pub details: SecuritySchemeDetails,
    /// Aperture-specific secret mapping
    #[serde(rename = "x-aperture-secret", skip_serializing_if = "Option::is_none")]
    pub aperture_secret: Option<CachedApertureSecret>,
}

/// Detailed security scheme configuration
#[derive(Debug, Serialize, Deserialize)]
#[serde(tag = "scheme", rename_all = "camelCase")]
pub enum SecuritySchemeDetails {
    /// HTTP authentication scheme (e.g., bearer, basic)
    #[serde(rename = "bearer")]
    HttpBearer {
        /// Optional bearer token format
        #[serde(skip_serializing_if = "Option::is_none")]
        bearer_format: Option<String>,
    },
    /// HTTP basic authentication
    #[serde(rename = "basic")]
    HttpBasic,
    /// API Key authentication
    #[serde(rename = "apiKey")]
    ApiKey {
        /// Location of the API key (header, query, cookie)
        #[serde(rename = "in")]
        location: String,
        /// Name of the parameter/header
        name: String,
    },
}

/// Generates a capability manifest from an `OpenAPI` specification.
///
/// This function creates a comprehensive JSON description of all available commands,
/// parameters, and security requirements directly from the original `OpenAPI` spec,
/// preserving all metadata that might be lost in the cached representation.
///
/// # Arguments
/// * `api_name` - The name of the API context
/// * `spec` - The original `OpenAPI` specification
/// * `global_config` - Optional global configuration for URL resolution
///
/// # Returns
/// * `Ok(String)` - JSON-formatted capability manifest
/// * `Err(Error)` - If JSON serialization fails
///
/// # Errors
/// Returns an error if JSON serialization fails
pub fn generate_capability_manifest_from_openapi(
    api_name: &str,
    spec: &OpenAPI,
    global_config: Option<&GlobalConfig>,
) -> Result<String, Error> {
    // First, convert the OpenAPI spec to a temporary CachedSpec for URL resolution
    let base_url = spec.servers.first().map(|s| s.url.clone());
    let servers: Vec<String> = spec.servers.iter().map(|s| s.url.clone()).collect();

    let temp_cached_spec = CachedSpec {
        cache_format_version: crate::cache::models::CACHE_FORMAT_VERSION,
        name: api_name.to_string(),
        version: spec.info.version.clone(),
        commands: vec![], // We'll generate commands directly from OpenAPI
        base_url,
        servers,
        security_schemes: HashMap::new(), // We'll extract these directly too
        skipped_endpoints: vec![],        // No endpoints are skipped for agent manifest
        server_variables: HashMap::new(), // We'll extract these later if needed
    };

    // Resolve base URL using the same priority hierarchy as executor
    let resolver = BaseUrlResolver::new(&temp_cached_spec);
    let resolver = if let Some(config) = global_config {
        resolver.with_global_config(config)
    } else {
        resolver
    };
    let resolved_base_url = resolver.resolve(None);

    // Extract commands directly from OpenAPI spec
    let mut command_groups: HashMap<String, Vec<CommandInfo>> = HashMap::new();

    for (path, path_item) in &spec.paths.paths {
        if let ReferenceOr::Item(item) = path_item {
            // Process each HTTP method
            for (method, operation) in crate::spec::http_methods_iter(item) {
                if let Some(op) = operation {
                    let command_info = convert_openapi_operation_to_info(
                        method,
                        path,
                        op,
                        spec,
                        spec.security.as_ref(),
                    );

                    // Group by first tag or "default", converted to lowercase
                    let group_name = op
                        .tags
                        .first()
                        .cloned()
                        .unwrap_or_else(|| constants::DEFAULT_GROUP.to_string())
                        .to_lowercase();

                    command_groups
                        .entry(group_name)
                        .or_default()
                        .push(command_info);
                }
            }
        }
    }

    // Extract security schemes directly from OpenAPI
    let security_schemes = extract_security_schemes_from_openapi(spec);

    // Create the manifest
    let manifest = ApiCapabilityManifest {
        api: ApiInfo {
            name: spec.info.title.clone(),
            version: spec.info.version.clone(),
            description: spec.info.description.clone(),
            base_url: resolved_base_url,
        },
        commands: command_groups,
        security_schemes,
    };

    // Serialize to JSON
    serde_json::to_string_pretty(&manifest)
        .map_err(|e| Error::serialization_error(format!("Failed to serialize agent manifest: {e}")))
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
            constants::DEFAULT_GROUP.to_string()
        } else {
            cached_command.name.to_lowercase()
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
        security_schemes: extract_security_schemes(spec),
    };

    // Serialize to JSON
    serde_json::to_string_pretty(&manifest)
        .map_err(|e| Error::serialization_error(format!("Failed to serialize agent manifest: {e}")))
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
        summary: cached_command.summary.clone(),
        operation_id: cached_command.operation_id.clone(),
        parameters,
        request_body,
        security_requirements: cached_command.security_requirements.clone(),
        tags: cached_command.tags.clone(),
        deprecated: cached_command.deprecated,
        external_docs_url: cached_command.external_docs_url.clone(),
    }
}

/// Converts a `CachedParameter` to `ParameterInfo` for the manifest
fn convert_cached_parameter_to_info(cached_param: &CachedParameter) -> ParameterInfo {
    ParameterInfo {
        name: cached_param.name.clone(),
        location: cached_param.location.clone(),
        required: cached_param.required,
        param_type: cached_param
            .schema_type
            .clone()
            .unwrap_or_else(|| constants::SCHEMA_TYPE_STRING.to_string()),
        description: cached_param.description.clone(),
        format: cached_param.format.clone(),
        default_value: cached_param.default_value.clone(),
        enum_values: cached_param.enum_values.clone(),
        example: cached_param.example.clone(),
    }
}

/// Converts a `CachedRequestBody` to `RequestBodyInfo` for the manifest
fn convert_cached_request_body_to_info(cached_body: &CachedRequestBody) -> RequestBodyInfo {
    RequestBodyInfo {
        required: cached_body.required,
        content_type: cached_body.content_type.clone(),
        description: cached_body.description.clone(),
        example: cached_body.example.clone(),
    }
}

/// Extracts security schemes from the cached spec for the capability manifest
fn extract_security_schemes(spec: &CachedSpec) -> HashMap<String, SecuritySchemeInfo> {
    let mut security_schemes = HashMap::new();

    for (name, scheme) in &spec.security_schemes {
        let details = match scheme.scheme_type.as_str() {
            constants::SECURITY_TYPE_HTTP => {
                scheme.scheme.as_ref().map_or(
                    SecuritySchemeDetails::HttpBearer {
                        bearer_format: None,
                    },
                    |http_scheme| match http_scheme.as_str() {
                        constants::AUTH_SCHEME_BEARER => SecuritySchemeDetails::HttpBearer {
                            bearer_format: scheme.bearer_format.clone(),
                        },
                        constants::AUTH_SCHEME_BASIC => SecuritySchemeDetails::HttpBasic,
                        _ => {
                            // For other HTTP schemes, default to bearer
                            SecuritySchemeDetails::HttpBearer {
                                bearer_format: None,
                            }
                        }
                    },
                )
            }
            constants::AUTH_SCHEME_APIKEY => SecuritySchemeDetails::ApiKey {
                location: scheme
                    .location
                    .clone()
                    .unwrap_or_else(|| constants::LOCATION_HEADER.to_string()),
                name: scheme
                    .parameter_name
                    .clone()
                    .unwrap_or_else(|| constants::HEADER_AUTHORIZATION.to_string()),
            },
            _ => {
                // Default to bearer for unknown types
                SecuritySchemeDetails::HttpBearer {
                    bearer_format: None,
                }
            }
        };

        let scheme_info = SecuritySchemeInfo {
            scheme_type: scheme.scheme_type.clone(),
            description: scheme.description.clone(),
            details,
            aperture_secret: scheme.aperture_secret.clone(),
        };

        security_schemes.insert(name.clone(), scheme_info);
    }

    security_schemes
}

/// Converts an `OpenAPI` operation to `CommandInfo` with full metadata
fn convert_openapi_operation_to_info(
    method: &str,
    path: &str,
    operation: &Operation,
    spec: &OpenAPI,
    global_security: Option<&Vec<openapiv3::SecurityRequirement>>,
) -> CommandInfo {
    let command_name = operation
        .operation_id
        .as_ref()
        .map_or_else(|| method.to_lowercase(), |op_id| to_kebab_case(op_id));

    // Extract parameters with full metadata, resolving references
    let parameters: Vec<ParameterInfo> = operation
        .parameters
        .iter()
        .filter_map(|param_ref| match param_ref {
            ReferenceOr::Item(param) => Some(convert_openapi_parameter_to_info(param)),
            ReferenceOr::Reference { reference } => resolve_parameter_reference(spec, reference)
                .ok()
                .map(|param| convert_openapi_parameter_to_info(&param)),
        })
        .collect();

    // Extract request body info
    let request_body = operation.request_body.as_ref().and_then(|rb_ref| {
        if let ReferenceOr::Item(body) = rb_ref {
            // Prefer JSON content if available
            let content_type = if body.content.contains_key(constants::CONTENT_TYPE_JSON) {
                constants::CONTENT_TYPE_JSON
            } else {
                body.content.keys().next().map(String::as_str)?
            };

            let media_type = body.content.get(content_type)?;
            let example = media_type
                .example
                .as_ref()
                .map(|ex| serde_json::to_string(ex).unwrap_or_else(|_| ex.to_string()));

            Some(RequestBodyInfo {
                required: body.required,
                content_type: content_type.to_string(),
                description: body.description.clone(),
                example,
            })
        } else {
            None
        }
    });

    // Extract security requirements
    let security_requirements = operation.security.as_ref().map_or_else(
        || {
            global_security.map_or(vec![], |reqs| {
                reqs.iter().flat_map(|req| req.keys().cloned()).collect()
            })
        },
        |op_security| {
            op_security
                .iter()
                .flat_map(|req| req.keys().cloned())
                .collect()
        },
    );

    CommandInfo {
        name: command_name,
        method: method.to_uppercase(),
        path: path.to_string(),
        description: operation.description.clone(),
        summary: operation.summary.clone(),
        operation_id: operation.operation_id.clone().unwrap_or_default(),
        parameters,
        request_body,
        security_requirements,
        tags: operation.tags.clone(),
        deprecated: operation.deprecated,
        external_docs_url: operation
            .external_docs
            .as_ref()
            .map(|docs| docs.url.clone()),
    }
}

/// Converts an `OpenAPI` parameter to `ParameterInfo` with full metadata
fn convert_openapi_parameter_to_info(param: &OpenApiParameter) -> ParameterInfo {
    let (param_data, location_str) = match param {
        OpenApiParameter::Query { parameter_data, .. } => {
            (parameter_data, constants::PARAM_LOCATION_QUERY)
        }
        OpenApiParameter::Header { parameter_data, .. } => {
            (parameter_data, constants::PARAM_LOCATION_HEADER)
        }
        OpenApiParameter::Path { parameter_data, .. } => {
            (parameter_data, constants::PARAM_LOCATION_PATH)
        }
        OpenApiParameter::Cookie { parameter_data, .. } => {
            (parameter_data, constants::PARAM_LOCATION_COOKIE)
        }
    };

    // Extract schema information
    let (schema_type, format, default_value, enum_values, example) =
        if let openapiv3::ParameterSchemaOrContent::Schema(schema_ref) = &param_data.format {
            match schema_ref {
                ReferenceOr::Item(schema) => {
                    let (schema_type, format, enums) = match &schema.schema_kind {
                        openapiv3::SchemaKind::Type(type_val) => match type_val {
                            openapiv3::Type::String(string_type) => {
                                let enum_values: Vec<String> = string_type
                                    .enumeration
                                    .iter()
                                    .filter_map(|v| v.as_ref())
                                    .map(|v| {
                                        serde_json::to_string(v).unwrap_or_else(|_| v.to_string())
                                    })
                                    .collect();
                                (constants::SCHEMA_TYPE_STRING.to_string(), None, enum_values)
                            }
                            openapiv3::Type::Number(_) => {
                                (constants::SCHEMA_TYPE_NUMBER.to_string(), None, vec![])
                            }
                            openapiv3::Type::Integer(_) => {
                                (constants::SCHEMA_TYPE_INTEGER.to_string(), None, vec![])
                            }
                            openapiv3::Type::Boolean(_) => {
                                (constants::SCHEMA_TYPE_BOOLEAN.to_string(), None, vec![])
                            }
                            openapiv3::Type::Array(_) => {
                                (constants::SCHEMA_TYPE_ARRAY.to_string(), None, vec![])
                            }
                            openapiv3::Type::Object(_) => {
                                (constants::SCHEMA_TYPE_OBJECT.to_string(), None, vec![])
                            }
                        },
                        _ => (constants::SCHEMA_TYPE_STRING.to_string(), None, vec![]),
                    };

                    let default_value = schema
                        .schema_data
                        .default
                        .as_ref()
                        .map(|v| serde_json::to_string(v).unwrap_or_else(|_| v.to_string()));

                    (Some(schema_type), format, default_value, enums, None)
                }
                ReferenceOr::Reference { .. } => (
                    Some(constants::SCHEMA_TYPE_STRING.to_string()),
                    None,
                    None,
                    vec![],
                    None,
                ),
            }
        } else {
            (
                Some(constants::SCHEMA_TYPE_STRING.to_string()),
                None,
                None,
                vec![],
                None,
            )
        };

    // Extract example from parameter data
    let example = param_data
        .example
        .as_ref()
        .map(|ex| serde_json::to_string(ex).unwrap_or_else(|_| ex.to_string()))
        .or(example);

    ParameterInfo {
        name: param_data.name.clone(),
        location: location_str.to_string(),
        required: param_data.required,
        param_type: schema_type.unwrap_or_else(|| constants::SCHEMA_TYPE_STRING.to_string()),
        description: param_data.description.clone(),
        format,
        default_value,
        enum_values,
        example,
    }
}

/// Extracts security schemes directly from `OpenAPI` spec
fn extract_security_schemes_from_openapi(spec: &OpenAPI) -> HashMap<String, SecuritySchemeInfo> {
    let mut security_schemes = HashMap::new();

    if let Some(components) = &spec.components {
        for (name, scheme_ref) in &components.security_schemes {
            if let ReferenceOr::Item(scheme) = scheme_ref {
                if let Some(scheme_info) = convert_openapi_security_scheme(name, scheme) {
                    security_schemes.insert(name.clone(), scheme_info);
                }
            }
        }
    }

    security_schemes
}

/// Converts an `OpenAPI` security scheme to `SecuritySchemeInfo`
fn convert_openapi_security_scheme(
    _name: &str,
    scheme: &SecurityScheme,
) -> Option<SecuritySchemeInfo> {
    match scheme {
        SecurityScheme::APIKey {
            location,
            name: param_name,
            description,
            ..
        } => {
            let location_str = match location {
                openapiv3::APIKeyLocation::Query => constants::PARAM_LOCATION_QUERY,
                openapiv3::APIKeyLocation::Header => constants::PARAM_LOCATION_HEADER,
                openapiv3::APIKeyLocation::Cookie => constants::PARAM_LOCATION_COOKIE,
            };

            let aperture_secret = extract_aperture_secret_from_extensions(scheme);

            Some(SecuritySchemeInfo {
                scheme_type: constants::AUTH_SCHEME_APIKEY.to_string(),
                description: description.clone(),
                details: SecuritySchemeDetails::ApiKey {
                    location: location_str.to_string(),
                    name: param_name.clone(),
                },
                aperture_secret,
            })
        }
        SecurityScheme::HTTP {
            scheme: http_scheme,
            bearer_format,
            description,
            ..
        } => {
            let details = match http_scheme.as_str() {
                constants::AUTH_SCHEME_BEARER => SecuritySchemeDetails::HttpBearer {
                    bearer_format: bearer_format.clone(),
                },
                constants::AUTH_SCHEME_BASIC => SecuritySchemeDetails::HttpBasic,
                _ => SecuritySchemeDetails::HttpBearer {
                    bearer_format: None,
                },
            };

            let aperture_secret = extract_aperture_secret_from_extensions(scheme);

            Some(SecuritySchemeInfo {
                scheme_type: constants::SECURITY_TYPE_HTTP.to_string(),
                description: description.clone(),
                details,
                aperture_secret,
            })
        }
        SecurityScheme::OAuth2 { .. } | SecurityScheme::OpenIDConnect { .. } => None,
    }
}

/// Extracts x-aperture-secret extension from a security scheme's extensions
fn extract_aperture_secret_from_extensions(
    scheme: &SecurityScheme,
) -> Option<CachedApertureSecret> {
    let extensions = match scheme {
        SecurityScheme::APIKey { extensions, .. } | SecurityScheme::HTTP { extensions, .. } => {
            extensions
        }
        SecurityScheme::OAuth2 { .. } | SecurityScheme::OpenIDConnect { .. } => return None,
    };

    extensions
        .get(constants::EXT_APERTURE_SECRET)
        .and_then(|value| {
            if let Some(obj) = value.as_object() {
                let source = obj.get(constants::EXT_KEY_SOURCE)?.as_str()?;
                let name = obj.get(constants::EXT_KEY_NAME)?.as_str()?;

                if source == constants::SOURCE_ENV {
                    return Some(CachedApertureSecret {
                        source: source.to_string(),
                        name: name.to_string(),
                    });
                }
            }
            None
        })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cache::models::{CachedCommand, CachedParameter, CachedSpec};

    #[test]
    fn test_command_name_conversion() {
        // Test that command names are properly converted
        assert_eq!(to_kebab_case("getUserById"), "get-user-by-id");
        assert_eq!(to_kebab_case("createUser"), "create-user");
        assert_eq!(to_kebab_case("list"), "list");
        assert_eq!(to_kebab_case("GET"), "get");
        assert_eq!(
            to_kebab_case("List an Organization's Issues"),
            "list-an-organizations-issues"
        );
    }

    #[test]
    fn test_generate_capability_manifest() {
        use crate::cache::models::{CachedApertureSecret, CachedSecurityScheme};

        let mut security_schemes = HashMap::new();
        security_schemes.insert(
            "bearerAuth".to_string(),
            CachedSecurityScheme {
                name: "bearerAuth".to_string(),
                scheme_type: constants::SECURITY_TYPE_HTTP.to_string(),
                scheme: Some(constants::AUTH_SCHEME_BEARER.to_string()),
                location: Some(constants::LOCATION_HEADER.to_string()),
                parameter_name: Some(constants::HEADER_AUTHORIZATION.to_string()),
                description: None,
                bearer_format: None,
                aperture_secret: Some(CachedApertureSecret {
                    source: constants::SOURCE_ENV.to_string(),
                    name: "API_TOKEN".to_string(),
                }),
            },
        );

        let spec = CachedSpec {
            cache_format_version: crate::cache::models::CACHE_FORMAT_VERSION,
            name: "Test API".to_string(),
            version: "1.0.0".to_string(),
            commands: vec![CachedCommand {
                name: "users".to_string(),
                description: Some("Get user by ID".to_string()),
                summary: None,
                operation_id: "getUserById".to_string(),
                method: constants::HTTP_METHOD_GET.to_string(),
                path: "/users/{id}".to_string(),
                parameters: vec![CachedParameter {
                    name: "id".to_string(),
                    location: constants::PARAM_LOCATION_PATH.to_string(),
                    required: true,
                    description: None,
                    schema: Some(constants::SCHEMA_TYPE_STRING.to_string()),
                    schema_type: Some(constants::SCHEMA_TYPE_STRING.to_string()),
                    format: None,
                    default_value: None,
                    enum_values: vec![],
                    example: None,
                }],
                request_body: None,
                responses: vec![],
                security_requirements: vec!["bearerAuth".to_string()],
                tags: vec!["users".to_string()],
                deprecated: false,
                external_docs_url: None,
                examples: vec![],
            }],
            base_url: Some("https://test-api.example.com".to_string()),
            servers: vec!["https://test-api.example.com".to_string()],
            security_schemes,
            skipped_endpoints: vec![],
            server_variables: HashMap::new(),
        };

        let manifest_json = generate_capability_manifest(&spec, None).unwrap();
        let manifest: ApiCapabilityManifest = serde_json::from_str(&manifest_json).unwrap();

        assert_eq!(manifest.api.name, "Test API");
        assert_eq!(manifest.api.version, "1.0.0");
        assert!(manifest.commands.contains_key("users"));

        let users_commands = &manifest.commands["users"];
        assert_eq!(users_commands.len(), 1);
        assert_eq!(users_commands[0].name, "get-user-by-id");
        assert_eq!(users_commands[0].method, constants::HTTP_METHOD_GET);
        assert_eq!(users_commands[0].parameters.len(), 1);
        assert_eq!(users_commands[0].parameters[0].name, "id");

        // Test security information extraction
        assert!(!manifest.security_schemes.is_empty());
        assert!(manifest.security_schemes.contains_key("bearerAuth"));
        let bearer_auth = &manifest.security_schemes["bearerAuth"];
        assert_eq!(bearer_auth.scheme_type, constants::SECURITY_TYPE_HTTP);
        assert!(matches!(
            &bearer_auth.details,
            SecuritySchemeDetails::HttpBearer { .. }
        ));
        assert!(bearer_auth.aperture_secret.is_some());
        let aperture_secret = bearer_auth.aperture_secret.as_ref().unwrap();
        assert_eq!(aperture_secret.name, "API_TOKEN");
        assert_eq!(aperture_secret.source, constants::SOURCE_ENV);
    }
}
