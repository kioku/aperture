use crate::cache::models::{
    CachedApertureSecret, CachedCommand, CachedParameter, CachedRequestBody, CachedResponse,
    CachedSecurityScheme, CachedSpec,
};
use openapiv3::{OpenAPI, Operation, Parameter, ReferenceOr, RequestBody, SecurityScheme};
use std::collections::HashMap;

/// Transforms `OpenAPI` specifications into Aperture's cached format
pub struct SpecTransformer;

impl SpecTransformer {
    /// Creates a new `SpecTransformer` instance
    #[must_use]
    pub const fn new() -> Self {
        Self
    }

    /// Transforms an `OpenAPI` specification into a cached representation
    ///
    /// This method converts the full `OpenAPI` spec into an optimized format
    /// that can be quickly loaded and used for CLI generation.
    #[must_use]
    pub fn transform(&self, name: &str, spec: &OpenAPI) -> CachedSpec {
        let mut commands = Vec::new();

        // Extract version from info
        let version = spec.info.version.clone();

        // Extract server URLs
        let servers: Vec<String> = spec.servers.iter().map(|s| s.url.clone()).collect();
        let base_url = servers.first().cloned();

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
            if let ReferenceOr::Item(item) = path_item {
                // Process each HTTP method
                let operations = [
                    ("GET", &item.get),
                    ("POST", &item.post),
                    ("PUT", &item.put),
                    ("DELETE", &item.delete),
                    ("PATCH", &item.patch),
                    ("HEAD", &item.head),
                    ("OPTIONS", &item.options),
                    ("TRACE", &item.trace),
                ];

                for (method, operation) in operations {
                    if let Some(op) = operation {
                        let command = Self::transform_operation(
                            method,
                            path,
                            op,
                            &global_security_requirements,
                        );
                        commands.push(command);
                    }
                }
            }
        }

        // Extract security schemes
        let security_schemes = Self::extract_security_schemes(spec);

        CachedSpec {
            name: name.to_string(),
            version,
            commands,
            base_url,
            servers,
            security_schemes,
        }
    }

    /// Transforms a single operation into a cached command
    fn transform_operation(
        method: &str,
        path: &str,
        operation: &Operation,
        global_security_requirements: &[String],
    ) -> CachedCommand {
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
            .unwrap_or_else(|| "default".to_string());

        // Transform parameters
        let mut parameters = Vec::new();
        for param_ref in &operation.parameters {
            if let ReferenceOr::Item(param) = param_ref {
                parameters.push(Self::transform_parameter(param));
            }
            // Skip references for now - validation should have caught these
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
            .map(|(code, _)| CachedResponse {
                status_code: code.to_string(),
                content: None, // Simplified for now
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

        CachedCommand {
            name,
            description: operation.description.clone(),
            operation_id,
            method: method.to_uppercase(),
            path: path.to_string(),
            parameters,
            request_body,
            responses,
            security_requirements,
        }
    }

    /// Transforms a parameter into cached format
    fn transform_parameter(param: &Parameter) -> CachedParameter {
        let (param_data, location_str) = match param {
            Parameter::Query { parameter_data, .. } => (parameter_data, "query"),
            Parameter::Header { parameter_data, .. } => (parameter_data, "header"),
            Parameter::Path { parameter_data, .. } => (parameter_data, "path"),
            Parameter::Cookie { parameter_data, .. } => (parameter_data, "cookie"),
        };

        CachedParameter {
            name: param_data.name.clone(),
            location: location_str.to_string(),
            required: param_data.required,
            schema: None, // Simplified for now - will be enhanced in Phase 3
        }
    }

    /// Transforms a request body into cached format
    fn transform_request_body(
        request_body: &ReferenceOr<RequestBody>,
    ) -> Option<CachedRequestBody> {
        match request_body {
            ReferenceOr::Item(body) => {
                // For now, we'll just note if JSON content is expected
                let content = if body.content.contains_key("application/json") {
                    "application/json".to_string()
                } else {
                    body.content.keys().next()?.clone()
                };

                Some(CachedRequestBody {
                    content,
                    required: body.required,
                })
            }
            ReferenceOr::Reference { .. } => None, // Skip references for now
        }
    }

    /// Extracts and transforms security schemes from the `OpenAPI` spec
    fn extract_security_schemes(spec: &OpenAPI) -> HashMap<String, CachedSecurityScheme> {
        let mut security_schemes = HashMap::new();

        if let Some(components) = &spec.components {
            for (name, scheme_ref) in &components.security_schemes {
                if let ReferenceOr::Item(scheme) = scheme_ref {
                    if let Some(cached_scheme) = Self::transform_security_scheme(name, scheme) {
                        security_schemes.insert(name.clone(), cached_scheme);
                    }
                }
            }
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
                ..
            } => {
                let aperture_secret = Self::extract_aperture_secret(scheme);
                let location_str = match location {
                    openapiv3::APIKeyLocation::Query => "query",
                    openapiv3::APIKeyLocation::Header => "header",
                    openapiv3::APIKeyLocation::Cookie => "cookie",
                };

                Some(CachedSecurityScheme {
                    name: name.to_string(),
                    scheme_type: "apiKey".to_string(),
                    scheme: None,
                    location: Some(location_str.to_string()),
                    parameter_name: Some(param_name.clone()),
                    aperture_secret,
                })
            }
            SecurityScheme::HTTP {
                scheme: http_scheme,
                ..
            } => {
                let aperture_secret = Self::extract_aperture_secret(scheme);
                Some(CachedSecurityScheme {
                    name: name.to_string(),
                    scheme_type: "http".to_string(),
                    scheme: Some(http_scheme.clone()),
                    location: Some("header".to_string()),
                    parameter_name: Some("Authorization".to_string()),
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
        extensions.get("x-aperture-secret").and_then(|value| {
            // The extension should be an object with "source" and "name" fields
            if let Some(obj) = value.as_object() {
                let source = obj.get("source")?.as_str()?;
                let name = obj.get("name")?.as_str()?;

                // Currently only "env" source is supported
                if source == "env" {
                    return Some(CachedApertureSecret {
                        source: source.to_string(),
                        name: name.to_string(),
                    });
                }
            }
            None
        })
    }
}

impl Default for SpecTransformer {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use openapiv3::{Info, OpenAPI};

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
        let cached = transformer.transform("test", &spec);

        assert_eq!(cached.name, "test");
        assert_eq!(cached.version, "1.0.0");
        assert_eq!(cached.base_url, Some("https://api.example.com".to_string()));
        assert_eq!(cached.servers.len(), 1);
        assert!(cached.commands.is_empty());
    }

    #[test]
    fn test_transform_with_operations() {
        use openapiv3::{Operation, PathItem, ReferenceOr, Responses};

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

        let cached = transformer.transform("test", &spec);

        assert_eq!(cached.commands.len(), 1);
        let command = &cached.commands[0];
        assert_eq!(command.name, "users");
        assert_eq!(command.operation_id, "getUsers");
        assert_eq!(command.method, "GET");
        assert_eq!(command.path, "/users");
        assert_eq!(command.description, Some("Get all users".to_string()));
    }
}
