use crate::error::Error;
use openapiv3::{OpenAPI, Operation, Parameter, ReferenceOr, RequestBody, SecurityScheme};
use std::collections::HashMap;

/// Result of validating an `OpenAPI` specification
#[derive(Debug, Default)]
pub struct ValidationResult {
    /// Validation warnings for skipped endpoints
    pub warnings: Vec<ValidationWarning>,
    /// Validation errors that prevent spec usage
    pub errors: Vec<Error>,
}

impl ValidationResult {
    /// Creates a new empty validation result
    #[must_use]
    pub const fn new() -> Self {
        Self {
            warnings: Vec::new(),
            errors: Vec::new(),
        }
    }

    /// Converts to a Result for backward compatibility
    ///
    /// # Errors
    ///
    /// Returns the first validation error if any exist
    pub fn into_result(self) -> Result<(), Error> {
        self.errors.into_iter().next().map_or_else(|| Ok(()), Err)
    }

    /// Checks if validation passed (may have warnings but no errors)
    #[must_use]
    pub const fn is_valid(&self) -> bool {
        self.errors.is_empty()
    }

    /// Adds a validation error
    pub fn add_error(&mut self, error: Error) {
        self.errors.push(error);
    }

    /// Adds a validation warning
    pub fn add_warning(&mut self, warning: ValidationWarning) {
        self.warnings.push(warning);
    }
}

/// Warning about skipped functionality
#[derive(Debug, Clone)]
pub struct ValidationWarning {
    /// The endpoint that was skipped
    pub endpoint: UnsupportedEndpoint,
    /// Human-readable reason for skipping
    pub reason: String,
}

/// Details about an unsupported endpoint
#[derive(Debug, Clone)]
pub struct UnsupportedEndpoint {
    /// HTTP path (e.g., "/api/upload")
    pub path: String,
    /// HTTP method (e.g., "POST")
    pub method: String,
    /// Content type that caused the skip (e.g., "multipart/form-data")
    pub content_type: String,
}

/// Validates `OpenAPI` specifications for compatibility with Aperture
pub struct SpecValidator;

impl SpecValidator {
    /// Creates a new `SpecValidator` instance
    #[must_use]
    pub const fn new() -> Self {
        Self
    }

    /// Returns a human-readable reason for why a content type is not supported
    fn get_unsupported_content_type_reason(content_type: &str) -> &'static str {
        match content_type {
            // Binary file types
            "multipart/form-data" => "file uploads are not supported",
            "application/octet-stream" => "binary data uploads are not supported",
            ct if ct.starts_with("image/") => "image uploads are not supported",
            "application/pdf" => "PDF uploads are not supported",

            // Alternative text formats
            "application/xml" | "text/xml" => "XML content is not supported",
            "application/x-www-form-urlencoded" => "form-encoded data is not supported",
            "text/plain" => "plain text content is not supported",
            "text/csv" => "CSV content is not supported",

            // JSON-compatible formats
            "application/x-ndjson" => "newline-delimited JSON is not supported",
            "application/graphql" => "GraphQL content is not supported",

            // Generic fallback
            _ => "is not supported",
        }
    }

    /// Validates an `OpenAPI` specification for Aperture compatibility
    ///
    /// # Errors
    ///
    /// Returns an error if:
    /// - The spec contains unsupported security schemes (`OAuth2`, `OpenID` Connect)
    /// - The spec uses $ref references in security schemes, parameters, or request bodies
    /// - Required x-aperture-secret extensions are missing
    /// - Parameters use content-based serialization
    /// - Request bodies use non-JSON content types
    #[deprecated(
        since = "0.1.2",
        note = "Use `validate_with_mode()` instead. This method defaults to strict mode which may not be desired."
    )]
    pub fn validate(&self, spec: &OpenAPI) -> Result<(), Error> {
        self.validate_with_mode(spec, true).into_result()
    }

    /// Validates an `OpenAPI` specification with configurable strictness
    ///
    /// # Arguments
    ///
    /// * `spec` - The `OpenAPI` specification to validate
    /// * `strict` - If true, returns errors for unsupported features. If false, collects warnings.
    ///
    /// # Returns
    ///
    /// Returns a `ValidationResult` containing any errors and/or warnings found
    #[must_use]
    pub fn validate_with_mode(&self, spec: &OpenAPI, strict: bool) -> ValidationResult {
        let mut result = ValidationResult::new();

        // Validate security schemes and track unsupported ones
        let mut unsupported_schemes = HashMap::new();
        if let Some(components) = &spec.components {
            for (name, scheme_ref) in &components.security_schemes {
                match scheme_ref {
                    ReferenceOr::Item(scheme) => match Self::validate_security_scheme(name, scheme)
                    {
                        Ok(_) => {}
                        Err(e) => {
                            if strict {
                                result.add_error(e);
                            } else if let Error::Validation(msg) = &e {
                                if msg.contains("unsupported authentication") {
                                    // Track unsupported schemes for later operation validation
                                    unsupported_schemes.insert(name.clone(), msg.clone());
                                } else {
                                    // Other validation errors are still errors
                                    result.add_error(e);
                                }
                            } else {
                                result.add_error(e);
                            }
                        }
                    },
                    ReferenceOr::Reference { .. } => {
                        result.add_error(Error::Validation(format!(
                            "Security scheme references are not supported: '{name}'"
                        )));
                    }
                }
            }
        }

        // Validate operations
        for (path, path_item_ref) in spec.paths.iter() {
            if let ReferenceOr::Item(path_item) = path_item_ref {
                for (method, operation_opt) in crate::spec::http_methods_iter(path_item) {
                    if let Some(operation) = operation_opt {
                        Self::validate_operation(
                            path,
                            &method.to_lowercase(),
                            operation,
                            &mut result,
                            strict,
                            &unsupported_schemes,
                            spec,
                        );
                    }
                }
            }
        }

        result
    }

    /// Validates a single security scheme and returns the scheme type for tracking
    fn validate_security_scheme(
        name: &str,
        scheme: &SecurityScheme,
    ) -> Result<Option<String>, Error> {
        // First validate the scheme type and identify if it's unsupported
        let unsupported_reason = match scheme {
            SecurityScheme::APIKey { .. } => None, // API Key schemes are supported
            SecurityScheme::HTTP {
                scheme: http_scheme,
                ..
            } => {
                // Only reject known complex schemes that we explicitly don't support
                // All other schemes are treated as bearer-like tokens
                let unsupported_complex_schemes = ["negotiate", "oauth", "oauth2", "openidconnect"];
                if unsupported_complex_schemes.contains(&http_scheme.to_lowercase().as_str()) {
                    Some(format!(
                        "HTTP scheme '{http_scheme}' requires complex authentication flows"
                    ))
                } else {
                    None // Any other HTTP scheme (bearer, basic, token, apikey, custom, etc.) is allowed
                }
            }
            SecurityScheme::OAuth2 { .. } => {
                Some("OAuth2 authentication is not supported".to_string())
            }
            SecurityScheme::OpenIDConnect { .. } => {
                Some("OpenID Connect authentication is not supported".to_string())
            }
        };

        // If we found an unsupported scheme, return it as an error (to be converted to warning later)
        if let Some(reason) = unsupported_reason {
            return Err(Error::Validation(format!(
                "Security scheme '{name}' uses unsupported authentication: {reason}"
            )));
        }

        // Now validate x-aperture-secret extension if present
        let (SecurityScheme::APIKey { extensions, .. } | SecurityScheme::HTTP { extensions, .. }) =
            scheme
        else {
            return Ok(None);
        };

        if let Some(aperture_secret) = extensions.get("x-aperture-secret") {
            // Validate that it's an object
            let secret_obj = aperture_secret.as_object().ok_or_else(|| {
                Error::Validation(format!(
                    "Invalid x-aperture-secret in security scheme '{name}': must be an object"
                ))
            })?;

            // Validate required 'source' field
            let source = secret_obj
                .get("source")
                .ok_or_else(|| {
                    Error::Validation(format!(
                        "Missing 'source' field in x-aperture-secret for security scheme '{name}'"
                    ))
                })?
                .as_str()
                .ok_or_else(|| {
                    Error::Validation(format!(
                        "Invalid 'source' field in x-aperture-secret for security scheme '{name}': must be a string"
                    ))
                })?;

            // Currently only 'env' source is supported
            if source != "env" {
                return Err(Error::Validation(format!(
                    "Unsupported source '{source}' in x-aperture-secret for security scheme '{name}'. Only 'env' is supported."
                )));
            }

            // Validate required 'name' field
            let env_name = secret_obj
                .get("name")
                .ok_or_else(|| {
                    Error::Validation(format!(
                        "Missing 'name' field in x-aperture-secret for security scheme '{name}'"
                    ))
                })?
                .as_str()
                .ok_or_else(|| {
                    Error::Validation(format!(
                        "Invalid 'name' field in x-aperture-secret for security scheme '{name}': must be a string"
                    ))
                })?;

            // Validate environment variable name format
            if env_name.is_empty() {
                return Err(Error::Validation(format!(
                    "Empty 'name' field in x-aperture-secret for security scheme '{name}'"
                )));
            }

            // Check for valid environment variable name (alphanumeric and underscore, not starting with digit)
            if !env_name.chars().all(|c| c.is_alphanumeric() || c == '_')
                || env_name.chars().next().is_some_and(char::is_numeric)
            {
                return Err(Error::Validation(format!(
                    "Invalid environment variable name '{env_name}' in x-aperture-secret for security scheme '{name}'. Must contain only alphanumeric characters and underscores, and not start with a digit."
                )));
            }
        }

        Ok(None)
    }

    /// Check if operation should be skipped because all its auth schemes are unsupported
    fn should_skip_due_to_auth(
        security_reqs: &[openapiv3::SecurityRequirement],
        unsupported_schemes: &HashMap<String, String>,
    ) -> bool {
        security_reqs.iter().all(|req| {
            req.keys()
                .all(|scheme| unsupported_schemes.contains_key(scheme))
        })
    }

    /// Validates an operation against Aperture's supported features
    fn validate_operation(
        path: &str,
        method: &str,
        operation: &Operation,
        result: &mut ValidationResult,
        strict: bool,
        unsupported_schemes: &HashMap<String, String>,
        spec: &OpenAPI,
    ) {
        // Skip auth validation in strict mode or when no schemes are unsupported
        if strict || unsupported_schemes.is_empty() {
            // Fall through to normal validation
        } else if let Some(reqs) = operation.security.as_ref().or(spec.security.as_ref()) {
            // Skip if empty security requirements
            if reqs.is_empty() {
                // Fall through to normal validation
            } else if Self::should_skip_due_to_auth(reqs, unsupported_schemes) {
                // All auth schemes are unsupported - skip this operation
                let schemes: Vec<String> =
                    reqs.iter().flat_map(|req| req.keys()).cloned().collect();

                result.add_warning(ValidationWarning {
                    endpoint: UnsupportedEndpoint {
                        path: path.to_string(),
                        method: method.to_uppercase(),
                        content_type: String::new(),
                    },
                    reason: format!(
                        "endpoint requires unsupported authentication schemes: {}",
                        schemes.join(", ")
                    ),
                });
                return;
            }
        }

        // Validate parameters
        for param_ref in &operation.parameters {
            match param_ref {
                ReferenceOr::Item(param) => {
                    if let Err(e) = Self::validate_parameter(path, method, param) {
                        result.add_error(e);
                    }
                }
                ReferenceOr::Reference { .. } => {
                    // Parameter references are now allowed and will be resolved during transformation
                }
            }
        }

        // Validate request body
        if let Some(request_body_ref) = &operation.request_body {
            match request_body_ref {
                ReferenceOr::Item(request_body) => {
                    Self::validate_request_body(path, method, request_body, result, strict);
                }
                ReferenceOr::Reference { .. } => {
                    result.add_error(Error::Validation(format!(
                        "Request body references are not supported in {method} {path}."
                    )));
                }
            }
        }
    }

    /// Validates a parameter against Aperture's supported features
    fn validate_parameter(path: &str, method: &str, param: &Parameter) -> Result<(), Error> {
        let param_data = match param {
            Parameter::Query { parameter_data, .. }
            | Parameter::Header { parameter_data, .. }
            | Parameter::Path { parameter_data, .. }
            | Parameter::Cookie { parameter_data, .. } => parameter_data,
        };

        match &param_data.format {
            openapiv3::ParameterSchemaOrContent::Schema(_) => Ok(()),
            openapiv3::ParameterSchemaOrContent::Content(_) => {
                Err(Error::Validation(format!(
                    "Parameter '{}' in {method} {path} uses unsupported content-based serialization. Only schema-based parameters are supported.",
                    param_data.name
                )))
            }
        }
    }

    /// Helper function to check if a content type is JSON
    fn is_json_content_type(content_type: &str) -> bool {
        // Extract base type before any parameters (e.g., "application/json; charset=utf-8" -> "application/json")
        let base_type = content_type
            .split(';')
            .next()
            .unwrap_or(content_type)
            .trim();

        // Support standard JSON and all JSON variants (e.g., application/vnd.api+json, application/ld+json)
        base_type.eq_ignore_ascii_case("application/json")
            || base_type.to_lowercase().ends_with("+json")
    }

    /// Validates a request body against Aperture's supported features
    fn validate_request_body(
        path: &str,
        method: &str,
        request_body: &RequestBody,
        result: &mut ValidationResult,
        strict: bool,
    ) {
        let (has_json, unsupported_types) = Self::categorize_content_types(request_body);

        if unsupported_types.is_empty() {
            return;
        }

        if strict {
            Self::add_strict_mode_errors(path, method, &unsupported_types, result);
        } else {
            Self::add_non_strict_warning(path, method, has_json, &unsupported_types, result);
        }
    }

    /// Categorize content types into JSON and unsupported
    fn categorize_content_types(request_body: &RequestBody) -> (bool, Vec<&String>) {
        let mut has_json = false;
        let mut unsupported_types = Vec::new();

        for content_type in request_body.content.keys() {
            if Self::is_json_content_type(content_type) {
                has_json = true;
            } else {
                unsupported_types.push(content_type);
            }
        }

        (has_json, unsupported_types)
    }

    /// Add errors for unsupported content types in strict mode
    fn add_strict_mode_errors(
        path: &str,
        method: &str,
        unsupported_types: &[&String],
        result: &mut ValidationResult,
    ) {
        for content_type in unsupported_types {
            let error = Error::Validation(format!(
                "Unsupported request body content type '{content_type}' in {method} {path}. Only 'application/json' is supported in v1.0."
            ));
            result.add_error(error);
        }
    }

    /// Add warning for unsupported content types in non-strict mode
    fn add_non_strict_warning(
        path: &str,
        method: &str,
        has_json: bool,
        unsupported_types: &[&String],
        result: &mut ValidationResult,
    ) {
        let content_types: Vec<String> = unsupported_types
            .iter()
            .map(|ct| {
                let reason = Self::get_unsupported_content_type_reason(ct);
                format!("{ct} ({reason})")
            })
            .collect();

        let reason = if has_json {
            "endpoint has unsupported content types alongside JSON"
        } else {
            "endpoint has no supported content types"
        };

        let warning = ValidationWarning {
            endpoint: UnsupportedEndpoint {
                path: path.to_string(),
                method: method.to_uppercase(),
                content_type: content_types.join(", "),
            },
            reason: reason.to_string(),
        };

        result.add_warning(warning);
    }
}

impl Default for SpecValidator {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use openapiv3::{Components, Info, OpenAPI};

    fn create_test_spec() -> OpenAPI {
        OpenAPI {
            openapi: "3.0.0".to_string(),
            info: Info {
                title: "Test API".to_string(),
                version: "1.0.0".to_string(),
                ..Default::default()
            },
            ..Default::default()
        }
    }

    #[test]
    fn test_validate_empty_spec() {
        let validator = SpecValidator::new();
        let spec = create_test_spec();
        assert!(validator
            .validate_with_mode(&spec, true)
            .into_result()
            .is_ok());
    }

    #[test]
    fn test_validate_oauth2_scheme_rejected() {
        let validator = SpecValidator::new();
        let mut spec = create_test_spec();
        let mut components = Components::default();
        components.security_schemes.insert(
            "oauth".to_string(),
            ReferenceOr::Item(SecurityScheme::OAuth2 {
                flows: Default::default(),
                description: None,
                extensions: Default::default(),
            }),
        );
        spec.components = Some(components);

        let result = validator.validate_with_mode(&spec, true).into_result();
        assert!(result.is_err());
        match result.unwrap_err() {
            Error::Validation(msg) => {
                assert!(msg.contains("OAuth2"));
                assert!(msg.contains("not supported"));
            }
            _ => panic!("Expected Validation error"),
        }
    }

    #[test]
    fn test_validate_reference_rejected() {
        let validator = SpecValidator::new();
        let mut spec = create_test_spec();
        let mut components = Components::default();
        components.security_schemes.insert(
            "auth".to_string(),
            ReferenceOr::Reference {
                reference: "#/components/securitySchemes/BasicAuth".to_string(),
            },
        );
        spec.components = Some(components);

        let result = validator.validate_with_mode(&spec, true).into_result();
        assert!(result.is_err());
        match result.unwrap_err() {
            Error::Validation(msg) => {
                assert!(msg.contains("references are not supported"));
            }
            _ => panic!("Expected Validation error"),
        }
    }

    #[test]
    fn test_validate_supported_schemes() {
        let validator = SpecValidator::new();
        let mut spec = create_test_spec();
        let mut components = Components::default();

        // Add API key scheme
        components.security_schemes.insert(
            "apiKey".to_string(),
            ReferenceOr::Item(SecurityScheme::APIKey {
                location: openapiv3::APIKeyLocation::Header,
                name: "X-API-Key".to_string(),
                description: None,
                extensions: Default::default(),
            }),
        );

        // Add HTTP bearer scheme
        components.security_schemes.insert(
            "bearer".to_string(),
            ReferenceOr::Item(SecurityScheme::HTTP {
                scheme: "bearer".to_string(),
                bearer_format: Some("JWT".to_string()),
                description: None,
                extensions: Default::default(),
            }),
        );

        // Add HTTP basic scheme (now supported)
        components.security_schemes.insert(
            "basic".to_string(),
            ReferenceOr::Item(SecurityScheme::HTTP {
                scheme: "basic".to_string(),
                bearer_format: None,
                description: None,
                extensions: Default::default(),
            }),
        );

        spec.components = Some(components);

        assert!(validator
            .validate_with_mode(&spec, true)
            .into_result()
            .is_ok());
    }

    #[test]
    fn test_validate_with_mode_non_strict_mixed_content() {
        use openapiv3::{
            MediaType, Operation, PathItem, ReferenceOr as PathRef, RequestBody, Responses,
        };

        let validator = SpecValidator::new();
        let mut spec = create_test_spec();

        // Endpoint with both JSON and multipart - should be accepted without warnings
        let mut request_body = RequestBody::default();
        request_body
            .content
            .insert("multipart/form-data".to_string(), MediaType::default());
        request_body
            .content
            .insert("application/json".to_string(), MediaType::default());
        request_body.required = true;

        let mut path_item = PathItem::default();
        path_item.post = Some(Operation {
            operation_id: Some("uploadFile".to_string()),
            tags: vec!["files".to_string()],
            request_body: Some(ReferenceOr::Item(request_body)),
            responses: Responses::default(),
            ..Default::default()
        });

        spec.paths
            .paths
            .insert("/upload".to_string(), PathRef::Item(path_item));

        // Non-strict mode should produce warnings for unsupported types even when JSON is supported
        let result = validator.validate_with_mode(&spec, false);
        assert!(result.is_valid(), "Non-strict mode should be valid");
        assert_eq!(
            result.warnings.len(),
            1,
            "Should have one warning for mixed content types"
        );
        assert_eq!(result.errors.len(), 0, "Should have no errors");

        // Check the warning details
        let warning = &result.warnings[0];
        assert_eq!(warning.endpoint.path, "/upload");
        assert_eq!(warning.endpoint.method, "POST");
        assert!(warning
            .endpoint
            .content_type
            .contains("multipart/form-data"));
        assert!(warning
            .reason
            .contains("unsupported content types alongside JSON"));
    }

    #[test]
    fn test_validate_with_mode_non_strict_only_unsupported() {
        use openapiv3::{
            MediaType, Operation, PathItem, ReferenceOr as PathRef, RequestBody, Responses,
        };

        let validator = SpecValidator::new();
        let mut spec = create_test_spec();

        // Endpoint with only unsupported content type - should produce warning
        let mut request_body = RequestBody::default();
        request_body
            .content
            .insert("multipart/form-data".to_string(), MediaType::default());
        request_body.required = true;

        let mut path_item = PathItem::default();
        path_item.post = Some(Operation {
            operation_id: Some("uploadFile".to_string()),
            tags: vec!["files".to_string()],
            request_body: Some(ReferenceOr::Item(request_body)),
            responses: Responses::default(),
            ..Default::default()
        });

        spec.paths
            .paths
            .insert("/upload".to_string(), PathRef::Item(path_item));

        // Non-strict mode should produce warning for endpoint with no supported types
        let result = validator.validate_with_mode(&spec, false);
        assert!(result.is_valid(), "Non-strict mode should be valid");
        assert_eq!(result.warnings.len(), 1, "Should have one warning");
        assert_eq!(result.errors.len(), 0, "Should have no errors");

        let warning = &result.warnings[0];
        assert_eq!(warning.endpoint.path, "/upload");
        assert_eq!(warning.endpoint.method, "POST");
        assert!(warning
            .endpoint
            .content_type
            .contains("multipart/form-data"));
        assert!(warning.reason.contains("no supported content types"));
    }

    #[test]
    fn test_validate_with_mode_strict() {
        use openapiv3::{
            MediaType, Operation, PathItem, ReferenceOr as PathRef, RequestBody, Responses,
        };

        let validator = SpecValidator::new();
        let mut spec = create_test_spec();

        let mut request_body = RequestBody::default();
        request_body
            .content
            .insert("multipart/form-data".to_string(), MediaType::default());
        request_body.required = true;

        let mut path_item = PathItem::default();
        path_item.post = Some(Operation {
            operation_id: Some("uploadFile".to_string()),
            tags: vec!["files".to_string()],
            request_body: Some(ReferenceOr::Item(request_body)),
            responses: Responses::default(),
            ..Default::default()
        });

        spec.paths
            .paths
            .insert("/upload".to_string(), PathRef::Item(path_item));

        // Strict mode should produce errors
        let result = validator.validate_with_mode(&spec, true);
        assert!(!result.is_valid(), "Strict mode should be invalid");
        assert_eq!(result.warnings.len(), 0, "Should have no warnings");
        assert_eq!(result.errors.len(), 1, "Should have one error");

        match &result.errors[0] {
            Error::Validation(msg) => {
                assert!(msg.contains("multipart/form-data"));
                assert!(msg.contains("v1.0"));
            }
            _ => panic!("Expected Validation error"),
        }
    }

    #[test]
    fn test_validate_with_mode_multiple_content_types() {
        use openapiv3::{
            MediaType, Operation, PathItem, ReferenceOr as PathRef, RequestBody, Responses,
        };

        let validator = SpecValidator::new();
        let mut spec = create_test_spec();

        // Add multiple endpoints with different content types
        let mut path_item1 = PathItem::default();
        let mut request_body1 = RequestBody::default();
        request_body1
            .content
            .insert("application/xml".to_string(), MediaType::default());
        path_item1.post = Some(Operation {
            operation_id: Some("postXml".to_string()),
            tags: vec!["data".to_string()],
            request_body: Some(ReferenceOr::Item(request_body1)),
            responses: Responses::default(),
            ..Default::default()
        });
        spec.paths
            .paths
            .insert("/xml".to_string(), PathRef::Item(path_item1));

        let mut path_item2 = PathItem::default();
        let mut request_body2 = RequestBody::default();
        request_body2
            .content
            .insert("text/plain".to_string(), MediaType::default());
        path_item2.put = Some(Operation {
            operation_id: Some("putText".to_string()),
            tags: vec!["data".to_string()],
            request_body: Some(ReferenceOr::Item(request_body2)),
            responses: Responses::default(),
            ..Default::default()
        });
        spec.paths
            .paths
            .insert("/text".to_string(), PathRef::Item(path_item2));

        // Non-strict mode should have warnings for both
        let result = validator.validate_with_mode(&spec, false);
        assert!(result.is_valid());
        assert_eq!(result.warnings.len(), 2);

        let warning_paths: Vec<&str> = result
            .warnings
            .iter()
            .map(|w| w.endpoint.path.as_str())
            .collect();
        assert!(warning_paths.contains(&"/xml"));
        assert!(warning_paths.contains(&"/text"));
    }

    #[test]
    fn test_validate_with_mode_multiple_unsupported_types_single_endpoint() {
        use openapiv3::{
            MediaType, Operation, PathItem, ReferenceOr as PathRef, RequestBody, Responses,
        };

        let validator = SpecValidator::new();
        let mut spec = create_test_spec();

        // Endpoint with multiple unsupported content types - should produce single warning
        let mut request_body = RequestBody::default();
        request_body
            .content
            .insert("multipart/form-data".to_string(), MediaType::default());
        request_body
            .content
            .insert("application/xml".to_string(), MediaType::default());
        request_body
            .content
            .insert("text/plain".to_string(), MediaType::default());
        request_body.required = true;

        let mut path_item = PathItem::default();
        path_item.post = Some(Operation {
            operation_id: Some("uploadData".to_string()),
            tags: vec!["data".to_string()],
            request_body: Some(ReferenceOr::Item(request_body)),
            responses: Responses::default(),
            ..Default::default()
        });

        spec.paths
            .paths
            .insert("/data".to_string(), PathRef::Item(path_item));

        // Non-strict mode should produce single warning listing all unsupported types
        let result = validator.validate_with_mode(&spec, false);
        assert!(result.is_valid(), "Non-strict mode should be valid");
        assert_eq!(result.warnings.len(), 1, "Should have exactly one warning");
        assert_eq!(result.errors.len(), 0, "Should have no errors");

        let warning = &result.warnings[0];
        assert_eq!(warning.endpoint.path, "/data");
        assert_eq!(warning.endpoint.method, "POST");
        // Check that all content types are mentioned
        assert!(warning
            .endpoint
            .content_type
            .contains("multipart/form-data"));
        assert!(warning.endpoint.content_type.contains("application/xml"));
        assert!(warning.endpoint.content_type.contains("text/plain"));
        assert!(warning.reason.contains("no supported content types"));
    }

    #[test]
    fn test_validate_unsupported_http_scheme() {
        let validator = SpecValidator::new();
        let mut spec = create_test_spec();
        let mut components = Components::default();

        // Use 'negotiate' which is explicitly rejected
        components.security_schemes.insert(
            "negotiate".to_string(),
            ReferenceOr::Item(SecurityScheme::HTTP {
                scheme: "negotiate".to_string(),
                bearer_format: None,
                description: None,
                extensions: Default::default(),
            }),
        );

        spec.components = Some(components);

        let result = validator.validate_with_mode(&spec, true).into_result();
        assert!(result.is_err());
        match result.unwrap_err() {
            Error::Validation(msg) => {
                assert!(msg.contains("requires complex authentication flows"));
            }
            _ => panic!("Expected Validation error"),
        }
    }

    #[test]
    fn test_validate_custom_http_schemes_allowed() {
        let validator = SpecValidator::new();
        let mut spec = create_test_spec();
        let mut components = Components::default();

        // Test various custom schemes that should now be allowed
        let custom_schemes = vec!["digest", "token", "apikey", "dsn", "custom-auth"];

        for scheme in custom_schemes {
            components.security_schemes.insert(
                format!("{}_auth", scheme),
                ReferenceOr::Item(SecurityScheme::HTTP {
                    scheme: scheme.to_string(),
                    bearer_format: None,
                    description: None,
                    extensions: Default::default(),
                }),
            );
        }

        spec.components = Some(components);

        // All custom schemes should be valid
        let result = validator.validate_with_mode(&spec, true);
        assert!(result.is_valid(), "Custom HTTP schemes should be allowed");
    }

    #[test]
    fn test_validate_parameter_reference_allowed() {
        use openapiv3::{Operation, PathItem, ReferenceOr as PathRef, Responses};

        let validator = SpecValidator::new();
        let mut spec = create_test_spec();

        let mut path_item = PathItem::default();
        path_item.get = Some(Operation {
            parameters: vec![ReferenceOr::Reference {
                reference: "#/components/parameters/UserId".to_string(),
            }],
            responses: Responses::default(),
            ..Default::default()
        });

        spec.paths
            .paths
            .insert("/users/{id}".to_string(), PathRef::Item(path_item));

        // Parameter references should now be allowed
        let result = validator.validate_with_mode(&spec, true).into_result();
        assert!(result.is_ok());
    }

    #[test]
    fn test_validate_request_body_non_json_rejected() {
        use openapiv3::{
            MediaType, Operation, PathItem, ReferenceOr as PathRef, RequestBody, Responses,
        };

        let validator = SpecValidator::new();
        let mut spec = create_test_spec();

        let mut request_body = RequestBody::default();
        request_body
            .content
            .insert("application/xml".to_string(), MediaType::default());
        request_body.required = true;

        let mut path_item = PathItem::default();
        path_item.post = Some(Operation {
            request_body: Some(ReferenceOr::Item(request_body)),
            responses: Responses::default(),
            ..Default::default()
        });

        spec.paths
            .paths
            .insert("/users".to_string(), PathRef::Item(path_item));

        let result = validator.validate_with_mode(&spec, true).into_result();
        assert!(result.is_err());
        match result.unwrap_err() {
            Error::Validation(msg) => {
                assert!(msg.contains("Unsupported request body content type 'application/xml'"));
            }
            _ => panic!("Expected Validation error"),
        }
    }

    #[test]
    fn test_validate_x_aperture_secret_valid() {
        let validator = SpecValidator::new();
        let mut spec = create_test_spec();
        let mut components = Components::default();

        // Create a bearer auth scheme with valid x-aperture-secret
        let mut extensions = serde_json::Map::new();
        extensions.insert(
            "x-aperture-secret".to_string(),
            serde_json::json!({
                "source": "env",
                "name": "API_TOKEN"
            }),
        );

        components.security_schemes.insert(
            "bearerAuth".to_string(),
            ReferenceOr::Item(SecurityScheme::HTTP {
                scheme: "bearer".to_string(),
                bearer_format: None,
                description: None,
                extensions: extensions.into_iter().collect(),
            }),
        );
        spec.components = Some(components);

        assert!(validator
            .validate_with_mode(&spec, true)
            .into_result()
            .is_ok());
    }

    #[test]
    fn test_validate_x_aperture_secret_missing_source() {
        let validator = SpecValidator::new();
        let mut spec = create_test_spec();
        let mut components = Components::default();

        // Create a bearer auth scheme with invalid x-aperture-secret (missing source)
        let mut extensions = serde_json::Map::new();
        extensions.insert(
            "x-aperture-secret".to_string(),
            serde_json::json!({
                "name": "API_TOKEN"
            }),
        );

        components.security_schemes.insert(
            "bearerAuth".to_string(),
            ReferenceOr::Item(SecurityScheme::HTTP {
                scheme: "bearer".to_string(),
                bearer_format: None,
                description: None,
                extensions: extensions.into_iter().collect(),
            }),
        );
        spec.components = Some(components);

        let result = validator.validate_with_mode(&spec, true).into_result();
        assert!(result.is_err());
        match result.unwrap_err() {
            Error::Validation(msg) => {
                assert!(msg.contains("Missing 'source' field"));
            }
            _ => panic!("Expected Validation error"),
        }
    }

    #[test]
    fn test_validate_x_aperture_secret_missing_name() {
        let validator = SpecValidator::new();
        let mut spec = create_test_spec();
        let mut components = Components::default();

        // Create a bearer auth scheme with invalid x-aperture-secret (missing name)
        let mut extensions = serde_json::Map::new();
        extensions.insert(
            "x-aperture-secret".to_string(),
            serde_json::json!({
                "source": "env"
            }),
        );

        components.security_schemes.insert(
            "bearerAuth".to_string(),
            ReferenceOr::Item(SecurityScheme::HTTP {
                scheme: "bearer".to_string(),
                bearer_format: None,
                description: None,
                extensions: extensions.into_iter().collect(),
            }),
        );
        spec.components = Some(components);

        let result = validator.validate_with_mode(&spec, true).into_result();
        assert!(result.is_err());
        match result.unwrap_err() {
            Error::Validation(msg) => {
                assert!(msg.contains("Missing 'name' field"));
            }
            _ => panic!("Expected Validation error"),
        }
    }

    #[test]
    fn test_validate_x_aperture_secret_invalid_env_name() {
        let validator = SpecValidator::new();
        let mut spec = create_test_spec();
        let mut components = Components::default();

        // Create a bearer auth scheme with invalid environment variable name
        let mut extensions = serde_json::Map::new();
        extensions.insert(
            "x-aperture-secret".to_string(),
            serde_json::json!({
                "source": "env",
                "name": "123_INVALID"  // Starts with digit
            }),
        );

        components.security_schemes.insert(
            "bearerAuth".to_string(),
            ReferenceOr::Item(SecurityScheme::HTTP {
                scheme: "bearer".to_string(),
                bearer_format: None,
                description: None,
                extensions: extensions.into_iter().collect(),
            }),
        );
        spec.components = Some(components);

        let result = validator.validate_with_mode(&spec, true).into_result();
        assert!(result.is_err());
        match result.unwrap_err() {
            Error::Validation(msg) => {
                assert!(msg.contains("Invalid environment variable name"));
            }
            _ => panic!("Expected Validation error"),
        }
    }

    #[test]
    fn test_validate_x_aperture_secret_unsupported_source() {
        let validator = SpecValidator::new();
        let mut spec = create_test_spec();
        let mut components = Components::default();

        // Create a bearer auth scheme with unsupported source
        let mut extensions = serde_json::Map::new();
        extensions.insert(
            "x-aperture-secret".to_string(),
            serde_json::json!({
                "source": "file",  // Not supported
                "name": "API_TOKEN"
            }),
        );

        components.security_schemes.insert(
            "bearerAuth".to_string(),
            ReferenceOr::Item(SecurityScheme::HTTP {
                scheme: "bearer".to_string(),
                bearer_format: None,
                description: None,
                extensions: extensions.into_iter().collect(),
            }),
        );
        spec.components = Some(components);

        let result = validator.validate_with_mode(&spec, true).into_result();
        assert!(result.is_err());
        match result.unwrap_err() {
            Error::Validation(msg) => {
                assert!(msg.contains("Unsupported source 'file'"));
            }
            _ => panic!("Expected Validation error"),
        }
    }
}
