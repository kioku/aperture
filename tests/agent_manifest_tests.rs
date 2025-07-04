use aperture_cli::agent::{
    generate_capability_manifest, ApiCapabilityManifest, SecuritySchemeDetails,
};
use aperture_cli::cache::models::{
    CachedApertureSecret, CachedCommand, CachedParameter, CachedRequestBody, CachedResponse,
    CachedSecurityScheme, CachedSpec,
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
            scheme: Some("bearer".to_string()),
            location: Some("header".to_string()),
            parameter_name: Some("Authorization".to_string()),
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
            scheme: Some("basic".to_string()),
            location: Some("header".to_string()),
            parameter_name: Some("Authorization".to_string()),
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
                    content_type: Some("application/json".to_string()),
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
                content_type: "application/json".to_string(),
                schema: r#"{"type": "object", "required": ["name", "email"], "properties": {"name": {"type": "string"}, "email": {"type": "string", "format": "email"}}}"#.to_string(),
                required: true,
                description: Some("User data for creation".to_string()),
                example: Some(r#"{"name": "John Doe", "email": "john@example.com"}"#.to_string()),
            }),
            responses: vec![CachedResponse {
                status_code: "201".to_string(),
                description: Some("User created successfully".to_string()),
                content_type: Some("application/json".to_string()),
                schema: None,
            }],
            security_requirements: vec!["apiKeyAuth".to_string()],
            tags: vec!["users".to_string()],
            deprecated: true,
            external_docs_url: None,
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
                content_type: Some("application/json".to_string()),
                schema: Some(r#"{"type": "object", "properties": {"status": {"type": "string"}}}"#.to_string()),
            }],
            security_requirements: vec![],
            tags: vec!["health".to_string()],
            deprecated: false,
            external_docs_url: None,
        },
    ];

    CachedSpec {
        name: "Test API".to_string(),
        version: "1.0.0".to_string(),
        commands,
        base_url: Some("https://api.example.com".to_string()),
        servers: vec!["https://api.example.com".to_string()],
        security_schemes,
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
    assert_eq!(request_body.content_type, "application/json");
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
    assert_eq!(bearer_auth["scheme"], "bearer");
    assert_eq!(bearer_auth["type"], "http");

    let api_key_auth = &json["security_schemes"]["apiKeyAuth"];
    assert_eq!(api_key_auth["scheme"], "apiKey");
    assert_eq!(api_key_auth["in"], "header");
    assert_eq!(api_key_auth["name"], "X-API-Key");
}

#[test]
fn test_manifest_with_global_config() {
    use aperture_cli::config::models::{ApiConfig, GlobalConfig};

    let spec = create_comprehensive_test_spec();

    // Create global config with URL override
    let mut api_configs = HashMap::new();
    api_configs.insert(
        "Test API".to_string(),
        ApiConfig {
            base_url_override: Some("https://override.example.com".to_string()),
            environment_urls: HashMap::new(),
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
