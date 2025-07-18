use crate::error::Error;
use openapiv3::{OpenAPI, Operation, Parameter, ReferenceOr, RequestBody, SecurityScheme};

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
        // Validate security schemes
        if let Some(components) = &spec.components {
            for (name, scheme_ref) in &components.security_schemes {
                match scheme_ref {
                    ReferenceOr::Item(scheme) => {
                        Self::validate_security_scheme(name, scheme)?;
                    }
                    ReferenceOr::Reference { .. } => {
                        return Err(Error::Validation(format!(
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
                        Self::validate_operation(path, &method.to_lowercase(), operation)?;
                    }
                }
            }
        }

        Ok(())
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
    fn validate_operation(path: &str, method: &str, operation: &Operation) -> Result<(), Error> {
        // Validate parameters
        for param_ref in &operation.parameters {
            match param_ref {
                ReferenceOr::Item(param) => {
                    Self::validate_parameter(path, method, param)?;
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
                    Self::validate_request_body(path, method, request_body)?;
                }
                ReferenceOr::Reference { .. } => {
                    return Err(Error::Validation(format!(
                        "Request body references are not supported in {method} {path}."
                    )));
                }
            }
        }

        Ok(())
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
    ) -> Result<(), Error> {
        // Check for unsupported content types
        for (content_type, _) in &request_body.content {
            if content_type != "application/json" {
                return Err(Error::Validation(format!(
                    "Unsupported request body content type '{content_type}' in {method} {path}. Only 'application/json' is supported in v1.0."
                )));
            }
        }

        Ok(())
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
