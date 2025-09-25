use aperture_cli::agent::{
    generate_capability_manifest, generate_capability_manifest_from_openapi, ApiCapabilityManifest,
    SecuritySchemeDetails,
};
use aperture_cli::cache::models::{
    CachedApertureSecret, CachedCommand, CachedParameter, CachedRequestBody, CachedResponse,
    CachedSecurityScheme, CachedSpec,
};
use aperture_cli::config::models::{ApiConfig, GlobalConfig};
use aperture_cli::constants;
use indexmap::IndexMap;
use openapiv3::{
    APIKeyLocation, BooleanType, Components, ExternalDocumentation, Info, MediaType, OpenAPI,
    Operation, Parameter, ParameterData, ParameterSchemaOrContent, PathItem, Paths, QueryStyle,
    ReferenceOr, RequestBody, Responses, Schema, SchemaData, SchemaKind, SecurityRequirement,
    SecurityScheme, Server, ServerVariable, StringType, Type,
};
use std::collections::HashMap;

/// Creates a comprehensive test spec with various security schemes and operations
fn create_comprehensive_test_spec() -> CachedSpec {
    let mut security_schemes = HashMap::new();

    // Add Bearer token auth
    security_schemes.insert(
        "bearerAuth".to_string(),
        CachedSecurityScheme {
            name: "bearerAuth".to_string(),
            scheme_type: "http".to_string(),
            scheme: Some(constants::AUTH_SCHEME_BEARER.to_string()),
            location: Some("header".to_string()),
            parameter_name: Some(constants::HEADER_AUTHORIZATION.to_string()),
            description: None,
            bearer_format: None,
            aperture_secret: Some(CachedApertureSecret {
                source: "env".to_string(),
                name: "API_TOKEN".to_string(),
            }),
        },
    );

    // Add API Key auth
    security_schemes.insert(
        "apiKeyAuth".to_string(),
        CachedSecurityScheme {
            name: "apiKeyAuth".to_string(),
            scheme_type: "apiKey".to_string(),
            scheme: None,
            location: Some("header".to_string()),
            parameter_name: Some("X-API-Key".to_string()),
            description: None,
            bearer_format: None,
            aperture_secret: Some(CachedApertureSecret {
                source: "env".to_string(),
                name: "API_KEY".to_string(),
            }),
        },
    );

    // Add Basic auth
    security_schemes.insert(
        "basicAuth".to_string(),
        CachedSecurityScheme {
            name: "basicAuth".to_string(),
            scheme_type: "http".to_string(),
            scheme: Some(constants::AUTH_SCHEME_BASIC.to_string()),
            location: Some("header".to_string()),
            parameter_name: Some(constants::HEADER_AUTHORIZATION.to_string()),
            description: None,
            bearer_format: None,
            aperture_secret: None, // No x-aperture-secret defined
        },
    );

    let commands = vec![
        // Command with all enriched fields
        CachedCommand {
            name: "users".to_string(),
            description: Some("Get user by ID with full details".to_string()),
            summary: Some("Get user by ID".to_string()),
            operation_id: "getUserById".to_string(),
            method: "GET".to_string(),
            path: "/users/{id}".to_string(),
            parameters: vec![
                CachedParameter {
                    name: "id".to_string(),
                    location: "path".to_string(),
                    required: true,
                    description: Some("The user identifier".to_string()),
                    schema: Some(r#"{"type": "integer", "format": "int64"}"#.to_string()),
                    schema_type: Some("integer".to_string()),
                    format: Some("int64".to_string()),
                    default_value: None,
                    enum_values: vec![],
                    example: Some("12345".to_string()),
                },
                CachedParameter {
                    name: "include".to_string(),
                    location: "query".to_string(),
                    required: false,
                    description: Some("Fields to include in response".to_string()),
                    schema: Some(r#"{"type": "string", "enum": ["profile", "settings", "history"]}"#.to_string()),
                    schema_type: Some("string".to_string()),
                    format: None,
                    default_value: Some("profile".to_string()),
                    enum_values: vec!["profile".to_string(), "settings".to_string(), "history".to_string()],
                    example: Some("profile,settings".to_string()),
                },
            ],
            request_body: None,
            responses: vec![
                CachedResponse {
                    status_code: "200".to_string(),
                    description: Some("Successful response with user data".to_string()),
                    content_type: Some(constants::CONTENT_TYPE_JSON.to_string()),
                    schema: Some(r#"{"type": "object", "properties": {"id": {"type": "integer"}, "name": {"type": "string"}}}"#.to_string()),
                },
                CachedResponse {
                    status_code: "404".to_string(),
                    description: Some("User not found".to_string()),
                    content_type: None,
                    schema: None,
                },
            ],
            security_requirements: vec!["bearerAuth".to_string()],
            tags: vec!["users".to_string(), "admin".to_string()],
            deprecated: false,
            external_docs_url: Some("https://docs.example.com/users".to_string()),
            examples: vec![],
        },
        // Deprecated command with request body
        CachedCommand {
            name: "users".to_string(),
            description: Some("Create a new user (deprecated, use v2)".to_string()),
            summary: None,
            operation_id: "createUser".to_string(),
            method: "POST".to_string(),
            path: "/users".to_string(),
            parameters: vec![],
            request_body: Some(CachedRequestBody {
                content_type: constants::CONTENT_TYPE_JSON.to_string(),
                schema: r#"{"type": "object", "required": ["name", "email"], "properties": {"name": {"type": "string"}, "email": {"type": "string", "format": "email"}}}"#.to_string(),
                required: true,
                description: Some("User data for creation".to_string()),
                example: Some(r#"{"name": "John Doe", "email": "john@example.com"}"#.to_string()),
            }),
            responses: vec![CachedResponse {
                status_code: "201".to_string(),
                description: Some("User created successfully".to_string()),
                content_type: Some(constants::CONTENT_TYPE_JSON.to_string()),
                schema: None,
            }],
            security_requirements: vec!["apiKeyAuth".to_string()],
            tags: vec!["users".to_string()],
            deprecated: true,
            external_docs_url: None,
            examples: vec![],
        },
        // Public endpoint with no auth
        CachedCommand {
            name: "health".to_string(),
            description: Some("Check API health status".to_string()),
            summary: Some("Health check".to_string()),
            operation_id: "healthCheck".to_string(),
            method: "GET".to_string(),
            path: "/health".to_string(),
            parameters: vec![],
            request_body: None,
            responses: vec![CachedResponse {
                status_code: "200".to_string(),
                description: Some("API is healthy".to_string()),
                content_type: Some(constants::CONTENT_TYPE_JSON.to_string()),
                schema: Some(r#"{"type": "object", "properties": {"status": {"type": "string"}}}"#.to_string()),
            }],
            security_requirements: vec![],
            tags: vec!["health".to_string()],
            deprecated: false,
            external_docs_url: None,
            examples: vec![],
        },
    ];

    CachedSpec {
        cache_format_version: aperture_cli::cache::models::CACHE_FORMAT_VERSION,
        name: "Test API".to_string(),
        version: "1.0.0".to_string(),
        commands,
        base_url: Some("https://api.example.com".to_string()),
        servers: vec!["https://api.example.com".to_string()],
        security_schemes,
        skipped_endpoints: vec![],
        server_variables: HashMap::new(),
    }
}

#[test]
fn test_comprehensive_manifest_generation() {
    let spec = create_comprehensive_test_spec();
    let manifest_json = generate_capability_manifest(&spec, None).unwrap();
    let manifest: ApiCapabilityManifest = serde_json::from_str(&manifest_json).unwrap();

    // Test API info
    assert_eq!(manifest.api.name, "Test API");
    assert_eq!(manifest.api.version, "1.0.0");
    assert_eq!(manifest.api.base_url, "https://api.example.com");

    // Test security schemes
    assert_eq!(manifest.security_schemes.len(), 3);

    // Test Bearer auth
    let bearer = &manifest.security_schemes["bearerAuth"];
    assert_eq!(bearer.scheme_type, "http");
    assert!(matches!(
        &bearer.details,
        SecuritySchemeDetails::HttpBearer {
            bearer_format: None
        }
    ));
    assert!(bearer.aperture_secret.is_some());
    assert_eq!(bearer.aperture_secret.as_ref().unwrap().name, "API_TOKEN");

    // Test API Key auth
    let api_key = &manifest.security_schemes["apiKeyAuth"];
    assert_eq!(api_key.scheme_type, "apiKey");
    if let SecuritySchemeDetails::ApiKey { location, name } = &api_key.details {
        assert_eq!(location, "header");
        assert_eq!(name, "X-API-Key");
    } else {
        panic!("Expected ApiKey security scheme details");
    }
    assert!(api_key.aperture_secret.is_some());
    assert_eq!(api_key.aperture_secret.as_ref().unwrap().name, "API_KEY");

    // Test Basic auth
    let basic = &manifest.security_schemes["basicAuth"];
    assert_eq!(basic.scheme_type, "http");
    assert!(matches!(&basic.details, SecuritySchemeDetails::HttpBasic));
    assert!(basic.aperture_secret.is_none());

    // Test commands
    assert_eq!(manifest.commands.len(), 2); // "users" and "health" groups

    // Test users commands
    let users_commands = &manifest.commands["users"];
    assert_eq!(users_commands.len(), 2);

    // Find GET /users/{id} command
    let get_user = users_commands
        .iter()
        .find(|c| c.method == "GET")
        .expect("GET user command not found");

    assert_eq!(get_user.name, "get-user-by-id");
    assert_eq!(get_user.path, "/users/{id}");
    assert_eq!(
        get_user.description,
        Some("Get user by ID with full details".to_string())
    );
    assert_eq!(get_user.summary, Some("Get user by ID".to_string()));
    assert_eq!(get_user.security_requirements, vec!["bearerAuth"]);
    assert_eq!(get_user.tags, vec!["users", "admin"]);
    assert!(!get_user.deprecated);
    assert_eq!(
        get_user.external_docs_url,
        Some("https://docs.example.com/users".to_string())
    );

    // Test parameters
    assert_eq!(get_user.parameters.len(), 2);
    let id_param = &get_user.parameters[0];
    assert_eq!(id_param.name, "id");
    assert_eq!(id_param.location, "path");
    assert!(id_param.required);
    assert_eq!(id_param.param_type, "integer");
    assert_eq!(id_param.format, Some("int64".to_string()));
    assert_eq!(id_param.example, Some("12345".to_string()));

    let include_param = &get_user.parameters[1];
    assert_eq!(include_param.name, "include");
    assert_eq!(include_param.location, "query");
    assert!(!include_param.required);
    assert_eq!(include_param.default_value, Some("profile".to_string()));
    assert_eq!(include_param.enum_values.len(), 3);
    assert!(include_param.enum_values.contains(&"profile".to_string()));

    // Find POST /users command
    let create_user = users_commands
        .iter()
        .find(|c| c.method == "POST")
        .expect("POST user command not found");

    assert_eq!(create_user.name, "create-user");
    assert!(create_user.deprecated);
    assert_eq!(create_user.security_requirements, vec!["apiKeyAuth"]);
    assert!(create_user.request_body.is_some());

    let request_body = create_user.request_body.as_ref().unwrap();
    assert!(request_body.required);
    assert_eq!(request_body.content_type, constants::CONTENT_TYPE_JSON);
    assert_eq!(
        request_body.description,
        Some("User data for creation".to_string())
    );
    assert!(request_body.example.is_some());

    // Test health command
    let health_commands = &manifest.commands["health"];
    assert_eq!(health_commands.len(), 1);
    let health_check = &health_commands[0];
    assert_eq!(health_check.name, "health-check");
    assert!(health_check.security_requirements.is_empty());
}

#[test]
fn test_manifest_json_structure() {
    let spec = create_comprehensive_test_spec();
    let manifest_json = generate_capability_manifest(&spec, None).unwrap();

    // Parse as generic JSON to test structure
    let json: serde_json::Value = serde_json::from_str(&manifest_json).unwrap();

    // Verify that empty arrays and false booleans are omitted
    let health_command = &json["commands"]["health"][0];
    assert!(!health_command
        .as_object()
        .unwrap()
        .contains_key("security_requirements"));
    assert!(!health_command
        .as_object()
        .unwrap()
        .contains_key("deprecated"));
    assert!(!health_command
        .as_object()
        .unwrap()
        .contains_key("external_docs_url"));

    // Verify that x-aperture-secret is properly serialized
    let bearer_auth = &json["security_schemes"]["bearerAuth"];
    assert!(bearer_auth["x-aperture-secret"].is_object());
    assert_eq!(bearer_auth["x-aperture-secret"]["name"], "API_TOKEN");
    assert_eq!(bearer_auth["x-aperture-secret"]["source"], "env");

    // Verify security scheme details are flattened with "scheme" tag
    assert_eq!(bearer_auth["scheme"], constants::AUTH_SCHEME_BEARER);
    assert_eq!(bearer_auth["type"], "http");

    let api_key_auth = &json["security_schemes"]["apiKeyAuth"];
    assert_eq!(api_key_auth["scheme"], "apiKey");
    assert_eq!(api_key_auth["in"], "header");
    assert_eq!(api_key_auth["name"], "X-API-Key");
}

#[test]
fn test_manifest_with_global_config() {
    let spec = create_comprehensive_test_spec();

    // Create global config with URL override
    let mut api_configs = HashMap::new();
    api_configs.insert(
        "Test API".to_string(),
        ApiConfig {
            base_url_override: Some("https://override.example.com".to_string()),
            environment_urls: HashMap::new(),
            strict_mode: false,
            secrets: HashMap::new(),
        },
    );

    let global_config = GlobalConfig {
        default_timeout_secs: 30,
        agent_defaults: Default::default(),
        api_configs,
    };

    let manifest_json = generate_capability_manifest(&spec, Some(&global_config)).unwrap();
    let manifest: ApiCapabilityManifest = serde_json::from_str(&manifest_json).unwrap();

    // Verify that the base URL is overridden
    assert_eq!(manifest.api.base_url, "https://override.example.com");
}

#[test]
fn test_manifest_from_openapi() {
    // Create a complete OpenAPI spec
    let mut paths = Paths::default();

    // Create an operation with all metadata
    let operation = Operation {
        tags: vec!["users".to_string()],
        summary: Some("Get user summary".to_string()),
        description: Some("Get detailed user information by ID".to_string()),
        external_docs: Some(ExternalDocumentation {
            description: Some("Learn more".to_string()),
            url: "https://docs.example.com/users".to_string(),
            extensions: Default::default(),
        }),
        operation_id: Some("getUserById".to_string()),
        parameters: vec![ReferenceOr::Item(Parameter::Query {
            parameter_data: ParameterData {
                name: "include".to_string(),
                description: Some("Fields to include".to_string()),
                required: false,
                deprecated: Some(false),
                format: ParameterSchemaOrContent::Schema(ReferenceOr::Item(Schema {
                    schema_data: Default::default(),
                    schema_kind: SchemaKind::Type(Type::String(StringType {
                        format: Default::default(),
                        pattern: None,
                        enumeration: vec![Some("profile".into()), Some("settings".into())],
                        min_length: None,
                        max_length: None,
                    })),
                })),
                example: Some(serde_json::json!("profile")),
                examples: Default::default(),
                explode: None,
                extensions: Default::default(),
            },
            style: QueryStyle::default(),
            allow_reserved: false,
            allow_empty_value: None,
        })],
        request_body: Some(ReferenceOr::Item({
            let mut body = RequestBody {
                description: Some("User update data".to_string()),
                content: Default::default(),
                required: true,
                extensions: Default::default(),
            };
            body.content.insert(
                constants::CONTENT_TYPE_JSON.to_string(),
                MediaType {
                    schema: None,
                    example: Some(serde_json::json!({"name": "John Doe"})),
                    examples: Default::default(),
                    encoding: Default::default(),
                    extensions: Default::default(),
                },
            );
            body
        })),
        responses: Responses::default(),
        deprecated: true,
        security: Some(vec![{
            let mut req = SecurityRequirement::new();
            req.insert("bearerAuth".to_string(), vec![]);
            req
        }]),
        servers: vec![],
        callbacks: Default::default(),
        extensions: Default::default(),
    };

    let mut path_item = PathItem::default();
    path_item.get = Some(operation);
    paths
        .paths
        .insert("/users/{id}".to_string(), ReferenceOr::Item(path_item));

    // Create components with security schemes
    let mut components = Components::default();

    // Add a bearer auth scheme with x-aperture-secret
    let mut bearer_extensions = serde_json::Map::new();
    bearer_extensions.insert(
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
            bearer_format: Some("JWT".to_string()),
            description: Some("Bearer token authentication".to_string()),
            extensions: bearer_extensions.into_iter().collect(),
        }),
    );

    // Add an API key scheme
    components.security_schemes.insert(
        "apiKey".to_string(),
        ReferenceOr::Item(SecurityScheme::APIKey {
            location: APIKeyLocation::Header,
            name: "X-API-Key".to_string(),
            description: Some("API Key authentication".to_string()),
            extensions: Default::default(),
        }),
    );

    let spec = OpenAPI {
        openapi: "3.0.0".to_string(),
        info: Info {
            title: "Test API from OpenAPI".to_string(),
            version: "2.0.0".to_string(),
            description: Some("A test API with full metadata".to_string()),
            terms_of_service: None,
            contact: None,
            license: None,
            extensions: Default::default(),
        },
        servers: vec![Server {
            url: "https://api.test.com".to_string(),
            description: Some("Production server".to_string()),
            variables: Default::default(),
            extensions: Default::default(),
        }],
        paths,
        components: Some(components),
        security: None,
        tags: vec![],
        external_docs: None,
        extensions: Default::default(),
    };

    // Generate manifest from OpenAPI
    let manifest_json = generate_capability_manifest_from_openapi("test-api", &spec, None).unwrap();
    let manifest: ApiCapabilityManifest = serde_json::from_str(&manifest_json).unwrap();

    // Verify API info preserves all metadata
    assert_eq!(manifest.api.name, "Test API from OpenAPI");
    assert_eq!(manifest.api.version, "2.0.0");
    assert_eq!(
        manifest.api.description,
        Some("A test API with full metadata".to_string())
    );
    assert_eq!(manifest.api.base_url, "https://api.test.com");

    // Verify command preserves all metadata
    let users_commands = &manifest.commands["users"];
    assert_eq!(users_commands.len(), 1);
    let cmd = &users_commands[0];

    assert_eq!(cmd.name, "get-user-by-id");
    assert_eq!(cmd.summary, Some("Get user summary".to_string()));
    assert_eq!(
        cmd.description,
        Some("Get detailed user information by ID".to_string())
    );
    assert!(cmd.deprecated);
    assert_eq!(
        cmd.external_docs_url,
        Some("https://docs.example.com/users".to_string())
    );
    assert_eq!(cmd.security_requirements, vec!["bearerAuth"]);

    // Verify parameter metadata
    assert_eq!(cmd.parameters.len(), 1);
    let param = &cmd.parameters[0];
    assert_eq!(param.enum_values.len(), 2);
    assert!(param.enum_values.contains(&"\"profile\"".to_string()));

    // Verify request body
    assert!(cmd.request_body.is_some());
    let body = cmd.request_body.as_ref().unwrap();
    assert_eq!(body.description, Some("User update data".to_string()));

    // Verify security schemes preserve metadata
    assert_eq!(manifest.security_schemes.len(), 2);

    let bearer = &manifest.security_schemes["bearerAuth"];
    assert_eq!(bearer.scheme_type, "http");
    assert_eq!(
        bearer.description,
        Some("Bearer token authentication".to_string())
    );
    if let SecuritySchemeDetails::HttpBearer { bearer_format } = &bearer.details {
        assert_eq!(bearer_format, &Some("JWT".to_string()));
    } else {
        panic!("Expected HttpBearer details");
    }
    assert!(bearer.aperture_secret.is_some());
    assert_eq!(bearer.aperture_secret.as_ref().unwrap().name, "API_TOKEN");

    let api_key = &manifest.security_schemes["apiKey"];
    assert_eq!(
        api_key.description,
        Some("API Key authentication".to_string())
    );
}

#[test]
fn test_manifest_with_parameter_references() {
    // Create OpenAPI spec with parameter references
    let spec = OpenAPI {
        openapi: "3.0.0".to_string(),
        info: Info {
            title: "Test API with References".to_string(),
            version: "1.0.0".to_string(),
            ..Default::default()
        },
        servers: vec![Server {
            url: "https://api.example.com".to_string(),
            ..Default::default()
        }],
        paths: {
            let mut paths = Paths::default();

            // Create operation with parameter references
            let mut path_item = PathItem::default();
            path_item.get = Some(Operation {
                operation_id: Some("getUserById".to_string()),
                tags: vec!["users".to_string()],
                parameters: vec![
                    ReferenceOr::Reference {
                        reference: "#/components/parameters/userId".to_string(),
                    },
                    ReferenceOr::Reference {
                        reference: "#/components/parameters/includeDetails".to_string(),
                    },
                ],
                responses: Responses::default(),
                ..Default::default()
            });

            paths
                .paths
                .insert("/users/{userId}".to_string(), ReferenceOr::Item(path_item));
            paths
        },
        components: Some(Components {
            parameters: {
                let mut params = IndexMap::new();

                // Define userId parameter
                params.insert(
                    "userId".to_string(),
                    ReferenceOr::Item(Parameter::Path {
                        parameter_data: ParameterData {
                            name: "userId".to_string(),
                            description: Some("User identifier".to_string()),
                            required: true,
                            deprecated: None,
                            format: ParameterSchemaOrContent::Schema(ReferenceOr::Item(Schema {
                                schema_kind: SchemaKind::Type(Type::String(StringType::default())),
                                schema_data: SchemaData::default(),
                            })),
                            example: Some(serde_json::json!("user123")),
                            examples: IndexMap::default(),
                            explode: None,
                            extensions: IndexMap::default(),
                        },
                        style: Default::default(),
                    }),
                );

                // Define includeDetails parameter
                params.insert(
                    "includeDetails".to_string(),
                    ReferenceOr::Item(Parameter::Query {
                        parameter_data: ParameterData {
                            name: "includeDetails".to_string(),
                            description: Some("Include detailed information".to_string()),
                            required: false,
                            deprecated: None,
                            format: ParameterSchemaOrContent::Schema(ReferenceOr::Item(Schema {
                                schema_kind: SchemaKind::Type(
                                    Type::Boolean(BooleanType::default()),
                                ),
                                schema_data: SchemaData {
                                    default: Some(serde_json::json!(false)),
                                    ..Default::default()
                                },
                            })),
                            example: None,
                            examples: IndexMap::default(),
                            explode: None,
                            extensions: IndexMap::default(),
                        },
                        allow_reserved: false,
                        style: Default::default(),
                        allow_empty_value: None,
                    }),
                );

                params
            },
            ..Default::default()
        }),
        ..Default::default()
    };

    // Generate manifest
    let manifest_json = generate_capability_manifest_from_openapi("test-ref", &spec, None)
        .expect("Failed to generate manifest");
    let manifest: serde_json::Value =
        serde_json::from_str(&manifest_json).expect("Failed to parse manifest JSON");

    // Verify parameters were resolved from references
    let commands = manifest["commands"]["users"]
        .as_array()
        .expect("Commands should be an array");
    assert_eq!(commands.len(), 1);

    let get_user = &commands[0];
    assert_eq!(get_user["operation_id"], "getUserById");

    let parameters = get_user["parameters"]
        .as_array()
        .expect("Parameters should be an array");
    assert_eq!(parameters.len(), 2, "Should have 2 resolved parameters");

    // Check userId parameter
    let user_id_param = &parameters[0];
    assert_eq!(user_id_param["name"], "userId");
    assert_eq!(user_id_param["location"], "path");
    assert_eq!(user_id_param["required"], true);
    assert_eq!(user_id_param["param_type"], "string");
    assert_eq!(user_id_param["description"], "User identifier");
    assert_eq!(user_id_param["example"], "\"user123\"");

    // Check includeDetails parameter
    let include_param = &parameters[1];
    assert_eq!(include_param["name"], "includeDetails");
    assert_eq!(include_param["location"], "query");
    assert_eq!(include_param["required"], false);
    assert_eq!(include_param["param_type"], "boolean");
    assert_eq!(include_param["description"], "Include detailed information");
    assert_eq!(include_param["default_value"], "false");
}
