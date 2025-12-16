use crate::cache::models::{
    CachedApertureSecret, CachedCommand, CachedParameter, CachedRequestBody, CachedResponse,
    CachedSecurityScheme, CachedSpec, CommandExample, SkippedEndpoint, CACHE_FORMAT_VERSION,
};
use crate::constants;
use crate::error::Error;
use crate::utils::to_kebab_case;
use openapiv3::{OpenAPI, Operation, Parameter, ReferenceOr, RequestBody, SecurityScheme};
use serde_json;
use std::collections::HashMap;
use std::fmt::Write;

/// Type alias for schema type information extracted from a schema kind
/// Returns: (`schema_type`, `format`, `default_value`, `enum_values`)
type SchemaTypeInfo = (String, Option<String>, Option<String>, Vec<String>);

/// Type alias for parameter schema information
/// Returns: (`schema_json`, `schema_type`, `format`, `default_value`, `enum_values`)
type ParameterSchemaInfo = (
    Option<String>,
    Option<String>,
    Option<String>,
    Option<String>,
    Vec<String>,
);

/// Options for transforming an `OpenAPI` specification
#[derive(Debug, Clone)]
pub struct TransformOptions {
    /// The name of the API
    pub name: String,
    /// Endpoints to skip during transformation
    pub skip_endpoints: Vec<(String, String)>,
    /// Validation warnings to include in the cached spec
    pub warnings: Vec<crate::spec::validator::ValidationWarning>,
}

impl TransformOptions {
    /// Creates new transform options with the given API name
    #[must_use]
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            skip_endpoints: Vec::new(),
            warnings: Vec::new(),
        }
    }

    /// Sets the endpoints to skip
    #[must_use]
    pub fn with_skip_endpoints(mut self, endpoints: Vec<(String, String)>) -> Self {
        self.skip_endpoints = endpoints;
        self
    }

    /// Sets the validation warnings
    #[must_use]
    pub fn with_warnings(
        mut self,
        warnings: Vec<crate::spec::validator::ValidationWarning>,
    ) -> Self {
        self.warnings = warnings;
        self
    }
}

/// Transforms `OpenAPI` specifications into Aperture's cached format
pub struct SpecTransformer;

impl SpecTransformer {
    /// Creates a new `SpecTransformer` instance
    #[must_use]
    pub const fn new() -> Self {
        Self
    }

    /// Transforms an `OpenAPI` specification into a cached representation using options
    ///
    /// This method converts the full `OpenAPI` spec into an optimized format
    /// that can be quickly loaded and used for CLI generation.
    ///
    /// # Errors
    ///
    /// Returns an error if parameter reference resolution fails
    pub fn transform_with_options(
        &self,
        spec: &OpenAPI,
        options: &TransformOptions,
    ) -> Result<CachedSpec, Error> {
        self.transform_with_warnings(
            &options.name,
            spec,
            &options.skip_endpoints,
            &options.warnings,
        )
    }

    /// Transforms an `OpenAPI` specification into a cached representation
    ///
    /// This method converts the full `OpenAPI` spec into an optimized format
    /// that can be quickly loaded and used for CLI generation.
    ///
    /// # Errors
    ///
    /// Returns an error if parameter reference resolution fails
    pub fn transform(&self, name: &str, spec: &OpenAPI) -> Result<CachedSpec, Error> {
        self.transform_with_filter(name, spec, &[])
    }

    /// Transforms an `OpenAPI` specification into a cached representation with endpoint filtering
    ///
    /// This method converts the full `OpenAPI` spec into an optimized format
    /// that can be quickly loaded and used for CLI generation, filtering out specified endpoints.
    ///
    /// # Arguments
    ///
    /// * `name` - The name for the cached spec
    /// * `spec` - The `OpenAPI` specification to transform
    /// * `skip_endpoints` - List of endpoints to skip (path, method pairs)
    ///
    /// # Errors
    ///
    /// Returns an error if parameter reference resolution fails
    pub fn transform_with_filter(
        &self,
        name: &str,
        spec: &OpenAPI,
        skip_endpoints: &[(String, String)],
    ) -> Result<CachedSpec, Error> {
        self.transform_with_warnings(name, spec, skip_endpoints, &[])
    }

    /// Transforms an `OpenAPI` specification with full warning information
    ///
    /// # Arguments
    ///
    /// * `name` - The name for the cached spec
    /// * `spec` - The `OpenAPI` specification to transform
    /// * `skip_endpoints` - List of endpoints to skip (path, method pairs)
    /// * `warnings` - Validation warnings to store in the cached spec
    ///
    /// # Errors
    ///
    /// Returns an error if parameter reference resolution fails
    pub fn transform_with_warnings(
        &self,
        name: &str,
        spec: &OpenAPI,
        skip_endpoints: &[(String, String)],
        warnings: &[crate::spec::validator::ValidationWarning],
    ) -> Result<CachedSpec, Error> {
        let mut commands = Vec::new();

        // Extract version from info
        let version = spec.info.version.clone();

        // Extract server URLs
        let servers: Vec<String> = spec.servers.iter().map(|s| s.url.clone()).collect();
        let base_url = servers.first().cloned();

        // Extract server variables from the first server (if any)
        let server_variables: HashMap<String, crate::cache::models::ServerVariable> = spec
            .servers
            .first()
            .and_then(|server| server.variables.as_ref())
            .map(|vars| {
                vars.iter()
                    .map(|(name, variable)| {
                        (
                            name.clone(),
                            crate::cache::models::ServerVariable {
                                default: Some(variable.default.clone()),
                                enum_values: variable.enumeration.clone(),
                                description: variable.description.clone(),
                            },
                        )
                    })
                    .collect()
            })
            .unwrap_or_default();

        // Extract global security requirements
        let global_security_requirements: Vec<String> = spec
            .security
            .iter()
            .flat_map(|security_vec| {
                security_vec
                    .iter()
                    .flat_map(|security_req| security_req.keys().cloned())
            })
            .collect();

        // Process all paths and operations
        for (path, path_item) in spec.paths.iter() {
            Self::process_path_item(
                spec,
                path,
                path_item,
                skip_endpoints,
                &global_security_requirements,
                &mut commands,
            )?;
        }

        // Extract security schemes
        let security_schemes = Self::extract_security_schemes(spec);

        // Convert warnings to skipped endpoints
        let skipped_endpoints: Vec<SkippedEndpoint> = warnings
            .iter()
            .map(|w| SkippedEndpoint {
                path: w.endpoint.path.clone(),
                method: w.endpoint.method.clone(),
                content_type: w.endpoint.content_type.clone(),
                reason: w.reason.clone(),
            })
            .collect();

        Ok(CachedSpec {
            cache_format_version: CACHE_FORMAT_VERSION,
            name: name.to_string(),
            version,
            commands,
            base_url,
            servers,
            security_schemes,
            skipped_endpoints,
            server_variables,
        })
    }

    /// Process a single path item and its operations
    fn process_path_item(
        spec: &OpenAPI,
        path: &str,
        path_item: &ReferenceOr<openapiv3::PathItem>,
        skip_endpoints: &[(String, String)],
        global_security_requirements: &[String],
        commands: &mut Vec<CachedCommand>,
    ) -> Result<(), Error> {
        let ReferenceOr::Item(item) = path_item else {
            return Ok(());
        };

        // Process each HTTP method
        for (method, operation) in crate::spec::http_methods_iter(item) {
            let Some(op) = operation else {
                continue;
            };

            if Self::should_skip_endpoint(path, method, skip_endpoints) {
                continue;
            }

            let command =
                Self::transform_operation(spec, method, path, op, global_security_requirements)?;
            commands.push(command);
        }

        Ok(())
    }

    /// Check if an endpoint should be skipped
    fn should_skip_endpoint(path: &str, method: &str, skip_endpoints: &[(String, String)]) -> bool {
        skip_endpoints.iter().any(|(skip_path, skip_method)| {
            skip_path == path && skip_method.eq_ignore_ascii_case(method)
        })
    }

    /// Transforms a single operation into a cached command
    #[allow(clippy::too_many_lines)]
    fn transform_operation(
        spec: &OpenAPI,
        method: &str,
        path: &str,
        operation: &Operation,
        global_security_requirements: &[String],
    ) -> Result<CachedCommand, Error> {
        // Extract operation metadata
        let operation_id = operation
            .operation_id
            .clone()
            .unwrap_or_else(|| format!("{method}_{path}"));

        // Use first tag as command namespace, or "default" if no tags
        let name = operation
            .tags
            .first()
            .cloned()
            .unwrap_or_else(|| constants::DEFAULT_GROUP.to_string());

        // Transform parameters
        let mut parameters = Vec::new();
        for param_ref in &operation.parameters {
            match param_ref {
                ReferenceOr::Item(param) => {
                    parameters.push(Self::transform_parameter(param));
                }
                ReferenceOr::Reference { reference } => {
                    let param = Self::resolve_parameter_reference(spec, reference)?;
                    parameters.push(Self::transform_parameter(&param));
                }
            }
        }

        // Transform request body
        let request_body = operation
            .request_body
            .as_ref()
            .and_then(Self::transform_request_body);

        // Transform responses
        let responses = operation
            .responses
            .responses
            .iter()
            .map(|(code, response_ref)| {
                match response_ref {
                    ReferenceOr::Item(response) => {
                        // Get description
                        let description = if response.description.is_empty() {
                            None
                        } else {
                            Some(response.description.clone())
                        };

                        // Get first content type and schema if available
                        let (content_type, schema) =
                            if let Some((ct, media_type)) = response.content.iter().next() {
                                let schema = media_type.schema.as_ref().and_then(|schema_ref| {
                                    match schema_ref {
                                        ReferenceOr::Item(schema) => {
                                            serde_json::to_string(schema).ok()
                                        }
                                        ReferenceOr::Reference { .. } => None,
                                    }
                                });
                                (Some(ct.clone()), schema)
                            } else {
                                (None, None)
                            };

                        CachedResponse {
                            status_code: code.to_string(),
                            description,
                            content_type,
                            schema,
                        }
                    }
                    ReferenceOr::Reference { .. } => CachedResponse {
                        status_code: code.to_string(),
                        description: None,
                        content_type: None,
                        schema: None,
                    },
                }
            })
            .collect();

        // Extract security requirements - use operation-level if defined, else global
        let security_requirements = operation.security.as_ref().map_or_else(
            || global_security_requirements.to_vec(),
            |security_reqs| {
                security_reqs
                    .iter()
                    .flat_map(|security_req| security_req.keys().cloned())
                    .collect()
            },
        );

        // Generate examples for this command
        let examples = Self::generate_command_examples(
            &name,
            &operation_id,
            method,
            path,
            &parameters,
            request_body.as_ref(),
        );

        Ok(CachedCommand {
            name,
            description: operation.description.clone(),
            summary: operation.summary.clone(),
            operation_id,
            method: method.to_uppercase(),
            path: path.to_string(),
            parameters,
            request_body,
            responses,
            security_requirements,
            tags: operation.tags.clone(),
            deprecated: operation.deprecated,
            external_docs_url: operation
                .external_docs
                .as_ref()
                .map(|docs| docs.url.clone()),
            examples,
        })
    }

    /// Transforms a parameter into cached format
    #[allow(clippy::too_many_lines)]
    fn transform_parameter(param: &Parameter) -> CachedParameter {
        let (param_data, location_str) = match param {
            Parameter::Query { parameter_data, .. } => {
                (parameter_data, constants::PARAM_LOCATION_QUERY)
            }
            Parameter::Header { parameter_data, .. } => {
                (parameter_data, constants::PARAM_LOCATION_HEADER)
            }
            Parameter::Path { parameter_data, .. } => {
                (parameter_data, constants::PARAM_LOCATION_PATH)
            }
            Parameter::Cookie { parameter_data, .. } => {
                (parameter_data, constants::PARAM_LOCATION_COOKIE)
            }
        };

        // Extract schema information from parameter
        let (schema_json, schema_type, format, default_value, enum_values) =
            Self::extract_parameter_schema_info(&param_data.format);

        // Extract example value
        let example = param_data
            .example
            .as_ref()
            .map(|ex| serde_json::to_string(ex).unwrap_or_else(|_| ex.to_string()));

        CachedParameter {
            name: param_data.name.clone(),
            location: location_str.to_string(),
            required: param_data.required,
            description: param_data.description.clone(),
            schema: schema_json,
            schema_type,
            format,
            default_value,
            enum_values,
            example,
        }
    }

    /// Extracts schema information from parameter schema or content
    fn extract_parameter_schema_info(
        format: &openapiv3::ParameterSchemaOrContent,
    ) -> ParameterSchemaInfo {
        let openapiv3::ParameterSchemaOrContent::Schema(schema_ref) = format else {
            // No schema provided, use defaults
            return (
                Some(r#"{"type": "string"}"#.to_string()),
                Some(constants::SCHEMA_TYPE_STRING.to_string()),
                None,
                None,
                vec![],
            );
        };

        match schema_ref {
            ReferenceOr::Item(schema) => {
                let schema_json = serde_json::to_string(schema).ok();

                // Extract type information
                let (schema_type, format, default, enums) =
                    Self::extract_schema_type_info(&schema.schema_kind);

                // Extract default value if present
                let default_value = schema
                    .schema_data
                    .default
                    .as_ref()
                    .map(|v| serde_json::to_string(v).unwrap_or_else(|_| v.to_string()));

                (
                    schema_json,
                    Some(schema_type),
                    format,
                    default_value.or(default),
                    enums,
                )
            }
            ReferenceOr::Reference { .. } => {
                // For references, use basic defaults
                (
                    Some(r#"{"type": "string"}"#.to_string()),
                    Some(constants::SCHEMA_TYPE_STRING.to_string()),
                    None,
                    None,
                    vec![],
                )
            }
        }
    }

    /// Extracts type information from schema kind
    fn extract_schema_type_info(schema_kind: &openapiv3::SchemaKind) -> SchemaTypeInfo {
        let openapiv3::SchemaKind::Type(type_val) = schema_kind else {
            return (
                constants::SCHEMA_TYPE_STRING.to_string(),
                None,
                None,
                vec![],
            );
        };

        match type_val {
            openapiv3::Type::String(string_type) => Self::extract_string_type_info(string_type),
            openapiv3::Type::Number(number_type) => Self::extract_number_type_info(number_type),
            openapiv3::Type::Integer(integer_type) => Self::extract_integer_type_info(integer_type),
            openapiv3::Type::Boolean(_) => (
                constants::SCHEMA_TYPE_BOOLEAN.to_string(),
                None,
                None,
                vec![],
            ),
            openapiv3::Type::Array(_) => {
                (constants::SCHEMA_TYPE_ARRAY.to_string(), None, None, vec![])
            }
            openapiv3::Type::Object(_) => (
                constants::SCHEMA_TYPE_OBJECT.to_string(),
                None,
                None,
                vec![],
            ),
        }
    }

    /// Extracts information from a string type schema
    fn extract_string_type_info(
        string_type: &openapiv3::StringType,
    ) -> (String, Option<String>, Option<String>, Vec<String>) {
        let enum_values: Vec<String> = string_type
            .enumeration
            .iter()
            .filter_map(|v| v.as_ref())
            .map(|v| serde_json::to_string(v).unwrap_or_else(|_| v.clone()))
            .collect();

        let format = Self::extract_format_string(&string_type.format);

        (
            constants::SCHEMA_TYPE_STRING.to_string(),
            format,
            None,
            enum_values,
        )
    }

    /// Extracts information from a number type schema
    fn extract_number_type_info(
        number_type: &openapiv3::NumberType,
    ) -> (String, Option<String>, Option<String>, Vec<String>) {
        let format = match &number_type.format {
            openapiv3::VariantOrUnknownOrEmpty::Item(fmt) => Some(format!("{fmt:?}")),
            _ => None,
        };
        ("number".to_string(), format, None, vec![])
    }

    /// Extracts information from an integer type schema
    fn extract_integer_type_info(
        integer_type: &openapiv3::IntegerType,
    ) -> (String, Option<String>, Option<String>, Vec<String>) {
        let format = match &integer_type.format {
            openapiv3::VariantOrUnknownOrEmpty::Item(fmt) => Some(format!("{fmt:?}")),
            _ => None,
        };
        (
            constants::SCHEMA_TYPE_INTEGER.to_string(),
            format,
            None,
            vec![],
        )
    }

    /// Extracts format string from a variant or unknown or empty type
    fn extract_format_string(
        format: &openapiv3::VariantOrUnknownOrEmpty<openapiv3::StringFormat>,
    ) -> Option<String> {
        match format {
            openapiv3::VariantOrUnknownOrEmpty::Item(fmt) => Some(format!("{fmt:?}")),
            _ => None,
        }
    }

    /// Transforms a request body into cached format
    fn transform_request_body(
        request_body: &ReferenceOr<RequestBody>,
    ) -> Option<CachedRequestBody> {
        match request_body {
            ReferenceOr::Item(body) => {
                // Prefer JSON content if available
                let content_type = if body.content.contains_key(constants::CONTENT_TYPE_JSON) {
                    constants::CONTENT_TYPE_JSON
                } else {
                    body.content.keys().next()?
                };

                // Extract schema and example from the content
                let media_type = body.content.get(content_type)?;
                let schema = media_type
                    .schema
                    .as_ref()
                    .and_then(|schema_ref| match schema_ref {
                        ReferenceOr::Item(schema) => serde_json::to_string(schema).ok(),
                        ReferenceOr::Reference { .. } => None,
                    })
                    .unwrap_or_else(|| "{}".to_string());

                let example = media_type
                    .example
                    .as_ref()
                    .map(|ex| serde_json::to_string(ex).unwrap_or_else(|_| ex.to_string()));

                Some(CachedRequestBody {
                    content_type: content_type.to_string(),
                    schema,
                    required: body.required,
                    description: body.description.clone(),
                    example,
                })
            }
            ReferenceOr::Reference { .. } => None, // Skip references for now
        }
    }

    /// Extracts and transforms security schemes from the `OpenAPI` spec
    fn extract_security_schemes(spec: &OpenAPI) -> HashMap<String, CachedSecurityScheme> {
        let mut security_schemes = HashMap::new();

        let Some(components) = &spec.components else {
            return security_schemes;
        };

        for (name, scheme_ref) in &components.security_schemes {
            let ReferenceOr::Item(scheme) = scheme_ref else {
                continue;
            };

            let Some(cached_scheme) = Self::transform_security_scheme(name, scheme) else {
                continue;
            };

            security_schemes.insert(name.clone(), cached_scheme);
        }

        security_schemes
    }

    /// Transforms a single security scheme into cached format
    fn transform_security_scheme(
        name: &str,
        scheme: &SecurityScheme,
    ) -> Option<CachedSecurityScheme> {
        match scheme {
            SecurityScheme::APIKey {
                location,
                name: param_name,
                description,
                ..
            } => {
                let aperture_secret = Self::extract_aperture_secret(scheme);
                let location_str = match location {
                    openapiv3::APIKeyLocation::Query => constants::PARAM_LOCATION_QUERY,
                    openapiv3::APIKeyLocation::Header => constants::PARAM_LOCATION_HEADER,
                    openapiv3::APIKeyLocation::Cookie => constants::PARAM_LOCATION_COOKIE,
                };

                Some(CachedSecurityScheme {
                    name: name.to_string(),
                    scheme_type: constants::AUTH_SCHEME_APIKEY.to_string(),
                    scheme: None,
                    location: Some(location_str.to_string()),
                    parameter_name: Some(param_name.clone()),
                    description: description.clone(),
                    bearer_format: None,
                    aperture_secret,
                })
            }
            SecurityScheme::HTTP {
                scheme: http_scheme,
                bearer_format,
                description,
                ..
            } => {
                let aperture_secret = Self::extract_aperture_secret(scheme);
                Some(CachedSecurityScheme {
                    name: name.to_string(),
                    scheme_type: constants::SECURITY_TYPE_HTTP.to_string(),
                    scheme: Some(http_scheme.clone()),
                    location: Some(constants::LOCATION_HEADER.to_string()),
                    parameter_name: Some(constants::HEADER_AUTHORIZATION.to_string()),
                    description: description.clone(),
                    bearer_format: bearer_format.clone(),
                    aperture_secret,
                })
            }
            // OAuth2 and OpenID Connect should be rejected in validation
            SecurityScheme::OAuth2 { .. } | SecurityScheme::OpenIDConnect { .. } => None,
        }
    }

    /// Extracts x-aperture-secret extension from a security scheme
    fn extract_aperture_secret(scheme: &SecurityScheme) -> Option<CachedApertureSecret> {
        // Get extensions from the security scheme
        let extensions = match scheme {
            SecurityScheme::APIKey { extensions, .. } | SecurityScheme::HTTP { extensions, .. } => {
                extensions
            }
            SecurityScheme::OAuth2 { .. } | SecurityScheme::OpenIDConnect { .. } => return None,
        };

        // Parse the x-aperture-secret extension
        extensions
            .get(crate::constants::EXT_APERTURE_SECRET)
            .and_then(|value| {
                // The extension should be an object with "source" and "name" fields
                let obj = value.as_object()?;
                let source = obj.get(crate::constants::EXT_KEY_SOURCE)?.as_str()?;
                let name = obj.get(crate::constants::EXT_KEY_NAME)?.as_str()?;

                // Currently only "env" source is supported
                if source != constants::SOURCE_ENV {
                    return None;
                }

                Some(CachedApertureSecret {
                    source: source.to_string(),
                    name: name.to_string(),
                })
            })
    }

    /// Resolves a parameter reference to its actual parameter definition
    fn resolve_parameter_reference(spec: &OpenAPI, reference: &str) -> Result<Parameter, Error> {
        crate::spec::resolve_parameter_reference(spec, reference)
    }

    /// Generate examples for a command
    #[allow(clippy::too_many_lines)]
    fn generate_command_examples(
        tag: &str,
        operation_id: &str,
        method: &str,
        path: &str,
        parameters: &[CachedParameter],
        request_body: Option<&CachedRequestBody>,
    ) -> Vec<CommandExample> {
        let mut examples = Vec::new();
        let operation_kebab = to_kebab_case(operation_id);
        let tag_kebab = to_kebab_case(tag);

        // Build base command
        let base_cmd = format!("aperture api myapi {tag_kebab} {operation_kebab}");

        // Example 1: Simple required parameters only
        let required_params: Vec<&CachedParameter> =
            parameters.iter().filter(|p| p.required).collect();

        if !required_params.is_empty() {
            let mut cmd = base_cmd.clone();
            for param in &required_params {
                write!(
                    &mut cmd,
                    " --{} {}",
                    param.name,
                    param.example.as_deref().unwrap_or("<value>")
                )
                .expect("writing to String cannot fail");
            }

            examples.push(CommandExample {
                description: "Basic usage with required parameters".to_string(),
                command_line: cmd,
                explanation: Some(format!("{method} {path}")),
            });
        }

        // Example 2: With request body if present
        if let Some(_body) = request_body {
            let mut cmd = base_cmd.clone();

            // Add required path/query parameters (only path and query params)
            let path_query_params = required_params
                .iter()
                .filter(|p| p.location == "path" || p.location == "query");

            for param in path_query_params {
                write!(
                    &mut cmd,
                    " --{} {}",
                    param.name,
                    param.example.as_deref().unwrap_or("123")
                )
                .expect("writing to String cannot fail");
            }

            // Add body example
            cmd.push_str(r#" --body '{"name": "example", "value": 42}'"#);

            examples.push(CommandExample {
                description: "With request body".to_string(),
                command_line: cmd,
                explanation: Some("Sends JSON data in the request body".to_string()),
            });
        }

        // Example 3: With optional parameters
        let optional_params: Vec<&CachedParameter> = parameters
            .iter()
            .filter(|p| !p.required && p.location == "query")
            .take(2) // Limit to 2 optional params for brevity
            .collect();

        if !optional_params.is_empty() && !required_params.is_empty() {
            let mut cmd = base_cmd.clone();

            // Add required parameters
            for param in &required_params {
                write!(
                    &mut cmd,
                    " --{} {}",
                    param.name,
                    param.example.as_deref().unwrap_or("value")
                )
                .expect("writing to String cannot fail");
            }

            // Add optional parameters
            for param in &optional_params {
                write!(
                    &mut cmd,
                    " --{} {}",
                    param.name,
                    param.example.as_deref().unwrap_or("optional")
                )
                .expect("writing to String cannot fail");
            }

            examples.push(CommandExample {
                description: "With optional parameters".to_string(),
                command_line: cmd,
                explanation: Some(
                    "Includes optional query parameters for filtering or customization".to_string(),
                ),
            });
        }

        // If no examples were generated, create a simple one
        if examples.is_empty() {
            examples.push(CommandExample {
                description: "Basic usage".to_string(),
                command_line: base_cmd,
                explanation: Some(format!("Executes {method} {path}")),
            });
        }

        examples
    }
}

impl Default for SpecTransformer {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
#[allow(clippy::default_trait_access)]
#[allow(clippy::field_reassign_with_default)]
#[allow(clippy::too_many_lines)]
mod tests {
    use super::*;
    use openapiv3::{
        Components, Info, OpenAPI, Operation, Parameter, ParameterData, ParameterSchemaOrContent,
        PathItem, ReferenceOr, Responses, Schema, SchemaData, SchemaKind, Type,
    };

    fn create_test_spec() -> OpenAPI {
        OpenAPI {
            openapi: "3.0.0".to_string(),
            info: Info {
                title: "Test API".to_string(),
                version: "1.0.0".to_string(),
                ..Default::default()
            },
            servers: vec![openapiv3::Server {
                url: "https://api.example.com".to_string(),
                ..Default::default()
            }],
            paths: Default::default(),
            ..Default::default()
        }
    }

    #[test]
    fn test_transform_basic_spec() {
        let transformer = SpecTransformer::new();
        let spec = create_test_spec();
        let cached = transformer
            .transform("test", &spec)
            .expect("Transform should succeed");

        assert_eq!(cached.name, "test");
        assert_eq!(cached.version, "1.0.0");
        assert_eq!(cached.base_url, Some("https://api.example.com".to_string()));
        assert_eq!(cached.servers.len(), 1);
        assert!(cached.commands.is_empty());
        assert!(cached.server_variables.is_empty());
    }

    #[test]
    fn test_transform_spec_with_server_variables() {
        let mut variables = indexmap::IndexMap::new();
        variables.insert(
            "region".to_string(),
            openapiv3::ServerVariable {
                default: "us".to_string(),
                description: Some("The regional instance".to_string()),
                enumeration: vec!["us".to_string(), "eu".to_string()],
                extensions: indexmap::IndexMap::new(),
            },
        );

        let spec = OpenAPI {
            openapi: "3.0.0".to_string(),
            info: Info {
                title: "Test API".to_string(),
                version: "1.0.0".to_string(),
                ..Default::default()
            },
            servers: vec![openapiv3::Server {
                url: "https://{region}.api.example.com".to_string(),
                description: Some("Regional server".to_string()),
                variables: Some(variables),
                extensions: indexmap::IndexMap::new(),
            }],
            ..Default::default()
        };

        let transformer = SpecTransformer::new();
        let cached = transformer.transform("test", &spec).unwrap();

        // Test server variable extraction
        assert_eq!(cached.server_variables.len(), 1);
        assert!(cached.server_variables.contains_key("region"));

        let region_var = &cached.server_variables["region"];
        assert_eq!(region_var.default, Some("us".to_string()));
        assert_eq!(
            region_var.description,
            Some("The regional instance".to_string())
        );
        assert_eq!(
            region_var.enum_values,
            vec!["us".to_string(), "eu".to_string()]
        );

        // Basic spec info
        assert_eq!(cached.name, "test");
        assert_eq!(
            cached.base_url,
            Some("https://{region}.api.example.com".to_string())
        );
    }

    #[test]
    fn test_transform_spec_with_empty_default_server_variable() {
        let mut variables = indexmap::IndexMap::new();
        variables.insert(
            "prefix".to_string(),
            openapiv3::ServerVariable {
                default: String::new(), // Empty string default should be preserved
                description: Some("Optional prefix".to_string()),
                enumeration: vec![],
                extensions: indexmap::IndexMap::new(),
            },
        );

        let spec = OpenAPI {
            openapi: "3.0.0".to_string(),
            info: Info {
                title: "Test API".to_string(),
                version: "1.0.0".to_string(),
                ..Default::default()
            },
            servers: vec![openapiv3::Server {
                url: "https://{prefix}api.example.com".to_string(),
                description: Some("Server with empty default".to_string()),
                variables: Some(variables),
                extensions: indexmap::IndexMap::new(),
            }],
            ..Default::default()
        };

        let transformer = SpecTransformer::new();
        let cached = transformer.transform("test", &spec).unwrap();

        // Verify empty string default is preserved
        assert!(cached.server_variables.contains_key("prefix"));
        let prefix_var = &cached.server_variables["prefix"];
        assert_eq!(prefix_var.default, Some(String::new()));
        assert_eq!(prefix_var.description, Some("Optional prefix".to_string()));
    }

    #[test]
    fn test_transform_with_operations() {
        let transformer = SpecTransformer::new();
        let mut spec = create_test_spec();

        let mut path_item = PathItem::default();
        path_item.get = Some(Operation {
            operation_id: Some("getUsers".to_string()),
            tags: vec!["users".to_string()],
            description: Some("Get all users".to_string()),
            responses: Responses::default(),
            ..Default::default()
        });

        spec.paths
            .paths
            .insert("/users".to_string(), ReferenceOr::Item(path_item));

        let cached = transformer
            .transform("test", &spec)
            .expect("Transform should succeed");

        assert_eq!(cached.commands.len(), 1);
        let command = &cached.commands[0];
        assert_eq!(command.name, "users");
        assert_eq!(command.operation_id, "getUsers");
        assert_eq!(command.method, constants::HTTP_METHOD_GET);
        assert_eq!(command.path, "/users");
        assert_eq!(command.description, Some("Get all users".to_string()));
    }

    #[test]
    fn test_transform_with_parameter_reference() {
        let transformer = SpecTransformer::new();
        let mut spec = create_test_spec();

        // Add a parameter to components
        let mut components = Components::default();
        let user_id_param = Parameter::Path {
            parameter_data: ParameterData {
                name: "userId".to_string(),
                description: Some("Unique identifier of the user".to_string()),
                required: true,
                deprecated: Some(false),
                format: ParameterSchemaOrContent::Schema(ReferenceOr::Item(Schema {
                    schema_data: SchemaData::default(),
                    schema_kind: SchemaKind::Type(Type::String(Default::default())),
                })),
                example: None,
                examples: Default::default(),
                explode: None,
                extensions: Default::default(),
            },
            style: Default::default(),
        };
        components
            .parameters
            .insert("userId".to_string(), ReferenceOr::Item(user_id_param));
        spec.components = Some(components);

        // Create operation with parameter reference
        let mut path_item = PathItem::default();
        path_item.get = Some(Operation {
            operation_id: Some("getUserById".to_string()),
            tags: vec!["users".to_string()],
            parameters: vec![ReferenceOr::Reference {
                reference: "#/components/parameters/userId".to_string(),
            }],
            responses: Responses::default(),
            ..Default::default()
        });

        spec.paths
            .paths
            .insert("/users/{userId}".to_string(), ReferenceOr::Item(path_item));

        let cached = transformer
            .transform("test", &spec)
            .expect("Transform should succeed with parameter reference");

        // Verify the parameter was resolved
        assert_eq!(cached.commands.len(), 1);
        let command = &cached.commands[0];
        assert_eq!(command.parameters.len(), 1);
        let param = &command.parameters[0];
        assert_eq!(param.name, "userId");
        assert_eq!(param.location, constants::PARAM_LOCATION_PATH);
        assert!(param.required);
        assert_eq!(
            param.description,
            Some("Unique identifier of the user".to_string())
        );
    }

    #[test]
    fn test_transform_with_invalid_parameter_reference() {
        let transformer = SpecTransformer::new();
        let mut spec = create_test_spec();

        // Create operation with invalid parameter reference
        let mut path_item = PathItem::default();
        path_item.get = Some(Operation {
            parameters: vec![ReferenceOr::Reference {
                reference: "#/invalid/reference/format".to_string(),
            }],
            responses: Responses::default(),
            ..Default::default()
        });

        spec.paths
            .paths
            .insert("/users".to_string(), ReferenceOr::Item(path_item));

        let result = transformer.transform("test", &spec);
        assert!(result.is_err());
        match result.unwrap_err() {
            crate::error::Error::Internal {
                kind: crate::error::ErrorKind::Validation,
                message: msg,
                ..
            } => {
                assert!(msg.contains("Invalid parameter reference format"));
            }
            _ => panic!("Expected Validation error"),
        }
    }

    #[test]
    fn test_transform_with_missing_parameter_reference() {
        let transformer = SpecTransformer::new();
        let mut spec = create_test_spec();

        // Add empty components
        spec.components = Some(Components::default());

        // Create operation with reference to non-existent parameter
        let mut path_item = PathItem::default();
        path_item.get = Some(Operation {
            parameters: vec![ReferenceOr::Reference {
                reference: "#/components/parameters/nonExistent".to_string(),
            }],
            responses: Responses::default(),
            ..Default::default()
        });

        spec.paths
            .paths
            .insert("/users".to_string(), ReferenceOr::Item(path_item));

        let result = transformer.transform("test", &spec);
        assert!(result.is_err());
        match result.unwrap_err() {
            crate::error::Error::Internal {
                kind: crate::error::ErrorKind::Validation,
                message: msg,
                ..
            } => {
                assert!(msg.contains("Parameter 'nonExistent' not found in components"));
            }
            _ => panic!("Expected Validation error"),
        }
    }

    #[test]
    fn test_transform_with_nested_parameter_reference() {
        let transformer = SpecTransformer::new();
        let mut spec = create_test_spec();

        let mut components = Components::default();

        // Add a parameter that references another parameter
        components.parameters.insert(
            "userIdRef".to_string(),
            ReferenceOr::Reference {
                reference: "#/components/parameters/userId".to_string(),
            },
        );

        // Add the actual parameter
        let user_id_param = Parameter::Path {
            parameter_data: ParameterData {
                name: "userId".to_string(),
                description: Some("User ID parameter".to_string()),
                required: true,
                deprecated: Some(false),
                format: ParameterSchemaOrContent::Schema(ReferenceOr::Item(Schema {
                    schema_data: SchemaData::default(),
                    schema_kind: SchemaKind::Type(Type::String(Default::default())),
                })),
                example: None,
                examples: Default::default(),
                explode: None,
                extensions: Default::default(),
            },
            style: Default::default(),
        };
        components
            .parameters
            .insert("userId".to_string(), ReferenceOr::Item(user_id_param));
        spec.components = Some(components);

        // Create operation with nested parameter reference
        let mut path_item = PathItem::default();
        path_item.get = Some(Operation {
            parameters: vec![ReferenceOr::Reference {
                reference: "#/components/parameters/userIdRef".to_string(),
            }],
            responses: Responses::default(),
            ..Default::default()
        });

        spec.paths
            .paths
            .insert("/users/{userId}".to_string(), ReferenceOr::Item(path_item));

        let cached = transformer
            .transform("test", &spec)
            .expect("Transform should succeed with nested parameter reference");

        // Verify the nested reference was resolved
        assert_eq!(cached.commands.len(), 1);
        let command = &cached.commands[0];
        assert_eq!(command.parameters.len(), 1);
        let param = &command.parameters[0];
        assert_eq!(param.name, "userId");
        assert_eq!(param.description, Some("User ID parameter".to_string()));
    }

    #[test]
    fn test_transform_with_circular_parameter_reference() {
        let transformer = SpecTransformer::new();
        let mut spec = create_test_spec();

        let mut components = Components::default();

        // Create direct circular reference: paramA -> paramA
        components.parameters.insert(
            "paramA".to_string(),
            ReferenceOr::Reference {
                reference: "#/components/parameters/paramA".to_string(),
            },
        );

        spec.components = Some(components);

        // Create operation with circular parameter reference
        let mut path_item = PathItem::default();
        path_item.get = Some(Operation {
            parameters: vec![ReferenceOr::Reference {
                reference: "#/components/parameters/paramA".to_string(),
            }],
            responses: Responses::default(),
            ..Default::default()
        });

        spec.paths
            .paths
            .insert("/test".to_string(), ReferenceOr::Item(path_item));

        let result = transformer.transform("test", &spec);
        assert!(result.is_err());
        match result.unwrap_err() {
            crate::error::Error::Internal {
                kind: crate::error::ErrorKind::Validation,
                message: msg,
                ..
            } => {
                assert!(
                    msg.contains("Circular reference detected"),
                    "Error message should mention circular reference: {msg}"
                );
            }
            _ => panic!("Expected Validation error for circular reference"),
        }
    }

    #[test]
    fn test_transform_with_indirect_circular_reference() {
        let transformer = SpecTransformer::new();
        let mut spec = create_test_spec();

        let mut components = Components::default();

        // Create indirect circular reference: paramA -> paramB -> paramA
        components.parameters.insert(
            "paramA".to_string(),
            ReferenceOr::Reference {
                reference: "#/components/parameters/paramB".to_string(),
            },
        );

        components.parameters.insert(
            "paramB".to_string(),
            ReferenceOr::Reference {
                reference: "#/components/parameters/paramA".to_string(),
            },
        );

        spec.components = Some(components);

        // Create operation with circular parameter reference
        let mut path_item = PathItem::default();
        path_item.get = Some(Operation {
            parameters: vec![ReferenceOr::Reference {
                reference: "#/components/parameters/paramA".to_string(),
            }],
            responses: Responses::default(),
            ..Default::default()
        });

        spec.paths
            .paths
            .insert("/test".to_string(), ReferenceOr::Item(path_item));

        let result = transformer.transform("test", &spec);
        assert!(result.is_err());
        match result.unwrap_err() {
            crate::error::Error::Internal {
                kind: crate::error::ErrorKind::Validation,
                message: msg,
                ..
            } => {
                assert!(
                    msg.contains("Circular reference detected") || msg.contains("reference cycle"),
                    "Error message should mention circular reference: {msg}"
                );
            }
            _ => panic!("Expected Validation error for circular reference"),
        }
    }

    #[test]
    fn test_transform_with_complex_circular_reference() {
        let transformer = SpecTransformer::new();
        let mut spec = create_test_spec();

        let mut components = Components::default();

        // Create complex circular reference: paramA -> paramB -> paramC -> paramA
        components.parameters.insert(
            "paramA".to_string(),
            ReferenceOr::Reference {
                reference: "#/components/parameters/paramB".to_string(),
            },
        );

        components.parameters.insert(
            "paramB".to_string(),
            ReferenceOr::Reference {
                reference: "#/components/parameters/paramC".to_string(),
            },
        );

        components.parameters.insert(
            "paramC".to_string(),
            ReferenceOr::Reference {
                reference: "#/components/parameters/paramA".to_string(),
            },
        );

        spec.components = Some(components);

        // Create operation with circular parameter reference
        let mut path_item = PathItem::default();
        path_item.get = Some(Operation {
            parameters: vec![ReferenceOr::Reference {
                reference: "#/components/parameters/paramA".to_string(),
            }],
            responses: Responses::default(),
            ..Default::default()
        });

        spec.paths
            .paths
            .insert("/test".to_string(), ReferenceOr::Item(path_item));

        let result = transformer.transform("test", &spec);
        assert!(result.is_err());
        match result.unwrap_err() {
            crate::error::Error::Internal {
                kind: crate::error::ErrorKind::Validation,
                message: msg,
                ..
            } => {
                assert!(
                    msg.contains("Circular reference detected") || msg.contains("reference cycle"),
                    "Error message should mention circular reference: {msg}"
                );
            }
            _ => panic!("Expected Validation error for circular reference"),
        }
    }

    #[test]
    fn test_transform_with_depth_limit() {
        let transformer = SpecTransformer::new();
        let mut spec = create_test_spec();

        let mut components = Components::default();

        // Create a chain of references that exceeds MAX_REFERENCE_DEPTH
        for i in 0..12 {
            let param_name = format!("param{i}");
            let next_param = format!("param{}", i + 1);

            if i < 11 {
                // Reference to next parameter
                components.parameters.insert(
                    param_name,
                    ReferenceOr::Reference {
                        reference: format!("#/components/parameters/{next_param}"),
                    },
                );
            } else {
                // Last parameter is actual parameter definition
                let actual_param = Parameter::Path {
                    parameter_data: ParameterData {
                        name: "deepParam".to_string(),
                        description: Some("Very deeply nested parameter".to_string()),
                        required: true,
                        deprecated: Some(false),
                        format: ParameterSchemaOrContent::Schema(ReferenceOr::Item(Schema {
                            schema_data: SchemaData::default(),
                            schema_kind: SchemaKind::Type(Type::String(Default::default())),
                        })),
                        example: None,
                        examples: Default::default(),
                        explode: None,
                        extensions: Default::default(),
                    },
                    style: Default::default(),
                };
                components
                    .parameters
                    .insert(param_name, ReferenceOr::Item(actual_param));
            }
        }

        spec.components = Some(components);

        // Create operation with deeply nested parameter reference
        let mut path_item = PathItem::default();
        path_item.get = Some(Operation {
            parameters: vec![ReferenceOr::Reference {
                reference: "#/components/parameters/param0".to_string(),
            }],
            responses: Responses::default(),
            ..Default::default()
        });

        spec.paths
            .paths
            .insert("/test".to_string(), ReferenceOr::Item(path_item));

        let result = transformer.transform("test", &spec);
        assert!(result.is_err());
        match result.unwrap_err() {
            crate::error::Error::Internal {
                kind: crate::error::ErrorKind::Validation,
                message: msg,
                ..
            } => {
                assert!(
                    msg.contains("Maximum reference depth") && msg.contains("10"),
                    "Error message should mention depth limit: {msg}"
                );
            }
            _ => panic!("Expected Validation error for depth limit"),
        }
    }
}
