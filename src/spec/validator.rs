use crate::error::Error;
use openapiv3::{OpenAPI, Operation, Parameter, ReferenceOr, RequestBody, SecurityScheme};

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

        // Validate security schemes
        if let Some(components) = &spec.components {
            for (name, scheme_ref) in &components.security_schemes {
                match scheme_ref {
                    ReferenceOr::Item(scheme) => {
                        if let Err(e) = Self::validate_security_scheme(name, scheme) {
                            result.add_error(e);
                        }
                    }
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
                        );
                    }
                }
            }
        }

        result
    }

    /// Validates a single security scheme
    fn validate_security_scheme(name: &str, scheme: &SecurityScheme) -> Result<(), Error> {
        // First validate the scheme type
        match scheme {
            SecurityScheme::APIKey { .. } => {
                // API Key schemes are supported
            }
            SecurityScheme::HTTP {
                scheme: http_scheme,
                ..
            } => {
                if http_scheme != "bearer" && http_scheme != "basic" {
                    return Err(Error::Validation(format!(
                        "Unsupported HTTP scheme '{http_scheme}' in security scheme '{name}'. Only 'bearer' and 'basic' are supported."
                    )));
                }
            }
            SecurityScheme::OAuth2 { .. } => {
                return Err(Error::Validation(format!(
                    "OAuth2 security scheme '{name}' is not supported in v1.0."
                )));
            }
            SecurityScheme::OpenIDConnect { .. } => {
                return Err(Error::Validation(format!(
                    "OpenID Connect security scheme '{name}' is not supported in v1.0."
                )));
            }
        }

        // Now validate x-aperture-secret extension if present
        let (SecurityScheme::APIKey { extensions, .. } | SecurityScheme::HTTP { extensions, .. }) =
            scheme
        else {
            return Ok(());
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

        Ok(())
    }

    /// Validates an operation against Aperture's supported features
    fn validate_operation(
        path: &str,
        method: &str,
        operation: &Operation,
        result: &mut ValidationResult,
        strict: bool,
    ) {
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

    /// Validates a request body against Aperture's supported features
    fn validate_request_body(
        path: &str,
        method: &str,
        request_body: &RequestBody,
        result: &mut ValidationResult,
        strict: bool,
    ) {
        // Check for unsupported content types
        for (content_type, _) in &request_body.content {
            if content_type != "application/json" {
                let error = Error::Validation(format!(
                    "Unsupported request body content type '{content_type}' in {method} {path}. Only 'application/json' is supported in v1.0."
                ));

                if strict {
                    result.add_error(error);
                } else {
                    // In non-strict mode, add as warning
                    let warning = ValidationWarning {
                        endpoint: UnsupportedEndpoint {
                            path: path.to_string(),
                            method: method.to_uppercase(),
                            content_type: content_type.clone(),
                        },
                        reason: format!("content type '{content_type}' is not supported"),
                    };
                    result.add_warning(warning);
                }
            }
        }
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
        assert!(validator.validate(&spec).is_ok());
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

        let result = validator.validate(&spec);
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

        let result = validator.validate(&spec);
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

        assert!(validator.validate(&spec).is_ok());
    }

    #[test]
    fn test_validate_with_mode_non_strict() {
        use openapiv3::{
            MediaType, Operation, PathItem, ReferenceOr as PathRef, RequestBody, Responses,
        };

        let validator = SpecValidator::new();
        let mut spec = create_test_spec();

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

        // Non-strict mode should produce warnings, not errors
        let result = validator.validate_with_mode(&spec, false);
        assert!(result.is_valid(), "Non-strict mode should be valid");
        assert_eq!(result.warnings.len(), 1, "Should have one warning");
        assert_eq!(result.errors.len(), 0, "Should have no errors");

        let warning = &result.warnings[0];
        assert_eq!(warning.endpoint.path, "/upload");
        assert_eq!(warning.endpoint.method, "POST");
        assert_eq!(warning.endpoint.content_type, "multipart/form-data");
        assert!(warning.reason.contains("not supported"));
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
    fn test_validate_unsupported_http_scheme() {
        let validator = SpecValidator::new();
        let mut spec = create_test_spec();
        let mut components = Components::default();

        components.security_schemes.insert(
            "digest".to_string(),
            ReferenceOr::Item(SecurityScheme::HTTP {
                scheme: "digest".to_string(),
                bearer_format: None,
                description: None,
                extensions: Default::default(),
            }),
        );

        spec.components = Some(components);

        let result = validator.validate(&spec);
        assert!(result.is_err());
        match result.unwrap_err() {
            Error::Validation(msg) => {
                assert!(msg.contains("Unsupported HTTP scheme 'digest'"));
            }
            _ => panic!("Expected Validation error"),
        }
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
        let result = validator.validate(&spec);
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

        let result = validator.validate(&spec);
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

        assert!(validator.validate(&spec).is_ok());
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

        let result = validator.validate(&spec);
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

        let result = validator.validate(&spec);
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

        let result = validator.validate(&spec);
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

        let result = validator.validate(&spec);
        assert!(result.is_err());
        match result.unwrap_err() {
            Error::Validation(msg) => {
                assert!(msg.contains("Unsupported source 'file'"));
            }
            _ => panic!("Expected Validation error"),
        }
    }
}
