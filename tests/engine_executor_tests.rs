use aperture_cli::cache::models::{CachedCommand, CachedParameter, CachedSpec};
use aperture_cli::cli::OutputFormat;
use aperture_cli::config::models::{ApiConfig, GlobalConfig};
use aperture_cli::engine::executor::execute_request;
use clap::{Arg, Command};
use std::collections::HashMap;
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

// Helper macros for creating test data
macro_rules! cached_parameter {
    ($name:expr, $location:expr, $required:expr) => {
        CachedParameter {
            name: $name.to_string(),
            location: $location.to_string(),
            required: $required,
            description: None,
            schema: Some(r#"{"type": "string"}"#.to_string()),
            schema_type: Some("string".to_string()),
            format: None,
            default_value: None,
            enum_values: vec![],
            example: None,
        }
    };
}

macro_rules! cached_command {
    ($name:expr, $op_id:expr, $method:expr, $path:expr, $params:expr) => {
        CachedCommand {
            name: $name.to_string(),
            description: None,
            summary: None,
            operation_id: $op_id.to_string(),
            method: $method.to_string(),
            path: $path.to_string(),
            parameters: $params,
            request_body: None,
            responses: vec![],
            security_requirements: vec![],
            tags: vec![$name.to_string()],
            deprecated: false,
            external_docs_url: None,
        }
    };
}

fn create_test_spec() -> CachedSpec {
    CachedSpec {
        cache_format_version: aperture_cli::cache::models::CACHE_FORMAT_VERSION,
        name: "test-api".to_string(),
        version: "1.0.0".to_string(),
        commands: vec![{
            let mut cmd = cached_command!(
                "users",
                "getUserById",
                "GET",
                "/users/{id}",
                vec![cached_parameter!("id", "path", true)]
            );
            cmd.description = Some("Get user by ID".to_string());
            cmd
        }],
        base_url: Some("https://api.example.com".to_string()),
        servers: vec!["https://api.example.com".to_string()],
        security_schemes: HashMap::new(),
        skipped_endpoints: vec![],
        server_variables: HashMap::new(),
    }
}

#[tokio::test]
async fn test_execute_request_basic_get() {
    let mock_server = MockServer::start().await;

    // Configure mock response
    Mock::given(method("GET"))
        .and(path("/users/123"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "id": "123",
            "name": "Test User"
        })))
        .expect(1)
        .mount(&mock_server)
        .await;

    let spec = create_test_spec();

    // Create command tree to match our generator's output
    let command = Command::new("api").subcommand(
        Command::new("users")
            .subcommand(Command::new("get-user-by-id").arg(Arg::new("id").required(true))),
    );

    let matches = command.get_matches_from(vec!["api", "users", "get-user-by-id", "123"]);

    // Execute the request with mock server URL
    let result = execute_request(
        &spec,
        &matches,
        Some(&mock_server.uri()),
        false,
        None,
        None,
        &OutputFormat::Json,
        None,
        None,  // cache_config
        false, // capture_output
    )
    .await;
    assert!(result.is_ok());
}

#[tokio::test]
async fn test_execute_request_with_query_params() {
    let mock_server = MockServer::start().await;

    let spec = CachedSpec {
        cache_format_version: aperture_cli::cache::models::CACHE_FORMAT_VERSION,
        name: "test-api".to_string(),
        version: "1.0.0".to_string(),
        commands: vec![{
            let mut cmd = cached_command!(
                "users",
                "listUsers",
                "GET",
                "/users",
                vec![cached_parameter!("limit", "query", false)]
            );
            cmd.description = Some("List users".to_string());
            cmd
        }],
        base_url: Some("https://api.example.com".to_string()),
        servers: vec!["https://api.example.com".to_string()],
        security_schemes: HashMap::new(),
        skipped_endpoints: vec![],
        server_variables: HashMap::new(),
    };

    Mock::given(method("GET"))
        .and(path("/users"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!([])))
        .mount(&mock_server)
        .await;

    let command = Command::new("api").subcommand(
        Command::new("users")
            .subcommand(Command::new("list-users").arg(Arg::new("limit").long("limit"))),
    );

    let matches = command.get_matches_from(vec!["api", "users", "list-users", "--limit", "10"]);

    let result = execute_request(
        &spec,
        &matches,
        Some(&mock_server.uri()),
        false,
        None,
        None,
        &OutputFormat::Json,
        None,
        None,  // cache_config
        false, // capture_output
    )
    .await;
    assert!(result.is_ok());
}

#[tokio::test]
async fn test_build_url_with_server_template_variables() {
    let spec = CachedSpec {
        cache_format_version: aperture_cli::cache::models::CACHE_FORMAT_VERSION,
        name: "sentry-api".to_string(),
        version: "1.0.0".to_string(),
        commands: vec![{
            let mut cmd = cached_command!(
                "events",
                "listEvents",
                "GET",
                "/api/0/projects/{organization}/{project}/events/",
                vec![
                    cached_parameter!("organization", "path", true),
                    cached_parameter!("project", "path", true),
                ]
            );
            cmd.description = Some("List events".to_string());
            cmd
        }],
        base_url: Some("https://{region}.sentry.io".to_string()),
        servers: vec!["https://{region}.sentry.io".to_string()],
        security_schemes: HashMap::new(),
        skipped_endpoints: vec![],
        server_variables: HashMap::new(),
    };

    let command = Command::new("api").subcommand(
        Command::new("events").subcommand(
            Command::new("list-events")
                .arg(Arg::new("organization").required(true))
                .arg(Arg::new("project").required(true)),
        ),
    );

    let matches =
        command.get_matches_from(vec!["api", "events", "list-events", "my-org", "my-project"]);

    // Execute the request - should fail with UnresolvedTemplateVariable error
    // because {region} in base URL cannot be resolved when no server variables are defined
    let result = execute_request(
        &spec,
        &matches,
        None,
        false,
        None,
        None,
        &OutputFormat::Json,
        None,
        None,  // cache_config
        false, // capture_output
    )
    .await;

    assert!(result.is_err());
    if let Err(e) = result {
        match e {
            aperture_cli::error::Error::Internal {
                kind: aperture_cli::error::ErrorKind::ServerVariable,
                message,
                ..
            } => {
                assert!(message.contains("region"));
            }
            _ => panic!("Expected Internal ServerVariable error, got: {:?}", e),
        }
    }
}

#[tokio::test]
async fn test_execute_request_error_response() {
    let mock_server = MockServer::start().await;

    Mock::given(method("GET"))
        .and(path("/users/999"))
        .respond_with(ResponseTemplate::new(404).set_body_json(serde_json::json!({
            "error": "User not found"
        })))
        .mount(&mock_server)
        .await;

    let spec = create_test_spec();

    let command = Command::new("api").subcommand(
        Command::new("users")
            .subcommand(Command::new("get-user-by-id").arg(Arg::new("id").required(true))),
    );

    let matches = command.get_matches_from(vec!["api", "users", "get-user-by-id", "999"]);

    let result = execute_request(
        &spec,
        &matches,
        Some(&mock_server.uri()),
        false,
        None,
        None,
        &OutputFormat::Json,
        None,
        None,  // cache_config
        false, // capture_output
    )
    .await;
    assert!(result.is_err());

    if let Err(e) = result {
        let error_msg = e.to_string();
        // Check for 404 in the main message
        assert!(error_msg.contains("404"));

        // For backward compatibility, check if it's the old or new format
        // Old format includes JSON in the main message
        // New format has JSON in the context
        match &e {
            aperture_cli::error::Error::Internal { context, .. } => {
                if let Some(ctx) = context {
                    if let Some(details) = &ctx.details {
                        let response_body = details.get("response_body").unwrap();
                        assert!(response_body
                            .as_str()
                            .unwrap()
                            .contains(r#""error":"User not found"#));
                    }
                }
            }
            _ => panic!("Unexpected error type: {:?}", e),
        }
    }
}

#[tokio::test]
async fn test_execute_request_with_global_config_base_url() {
    let mock_server = MockServer::start().await;

    // Configure mock response
    Mock::given(method("GET"))
        .and(path("/users/123"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "id": "123",
            "name": "Test User"
        })))
        .expect(1)
        .mount(&mock_server)
        .await;

    // Create spec WITHOUT base_url (this should force use of global config)
    let spec = CachedSpec {
        cache_format_version: aperture_cli::cache::models::CACHE_FORMAT_VERSION,
        name: "test-api".to_string(),
        version: "1.0.0".to_string(),
        commands: vec![{
            let mut cmd = cached_command!(
                "users",
                "getUserById",
                "GET",
                "/users/{id}",
                vec![cached_parameter!("id", "path", true)]
            );
            cmd.description = Some("Get user by ID".to_string());
            cmd
        }],
        base_url: None, // No base URL in spec
        servers: vec![],
        security_schemes: HashMap::new(),
        skipped_endpoints: vec![],
        server_variables: HashMap::new(),
    };

    // Create global config with API override
    let mut api_configs = HashMap::new();
    api_configs.insert(
        "test-api".to_string(),
        ApiConfig {
            base_url_override: Some(mock_server.uri()),
            environment_urls: HashMap::new(),
            strict_mode: false,
            secrets: HashMap::new(),
        },
    );

    let global_config = GlobalConfig {
        api_configs,
        ..Default::default()
    };

    let command = Command::new("api").subcommand(
        Command::new("users")
            .subcommand(Command::new("get-user-by-id").arg(Arg::new("id").required(true))),
    );

    let matches = command.get_matches_from(vec!["api", "users", "get-user-by-id", "123"]);

    // Execute the request with global config providing the base URL
    let result = execute_request(
        &spec,
        &matches,
        None,
        false,
        None,
        Some(&global_config),
        &OutputFormat::Json,
        None,
        None,  // cache_config
        false, // capture_output
    )
    .await;
    assert!(result.is_ok());
}

#[tokio::test]
async fn test_url_with_json_query_params_not_detected_as_template() {
    let mock_server = MockServer::start().await;

    // URLs with JSON in query parameters should not be detected as template variables
    let spec = CachedSpec {
        cache_format_version: aperture_cli::cache::models::CACHE_FORMAT_VERSION,
        name: "json-api".to_string(),
        version: "1.0.0".to_string(),
        commands: vec![cached_command!(
            "search",
            "search",
            "GET",
            "/search",
            vec![]
        )],
        base_url: Some(r#"https://api.example.com?filter={"type":"user"}"#.to_string()),
        servers: vec![r#"https://api.example.com?filter={"type":"user"}"#.to_string()],
        security_schemes: HashMap::new(),
        skipped_endpoints: vec![],
        server_variables: HashMap::new(),
    };

    // Mock the endpoint
    Mock::given(method("GET"))
        .and(path("/search"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!([])))
        .mount(&mock_server)
        .await;

    let command =
        Command::new("api").subcommand(Command::new("search").subcommand(Command::new("search")));

    let matches = command.get_matches_from(vec!["api", "search", "search"]);

    // Should NOT fail with template error because {"type":"user"} is not a valid template
    let result = execute_request(
        &spec,
        &matches,
        Some(&mock_server.uri()), // Use mock server
        false,
        None,
        None,
        &OutputFormat::Json,
        None,
        None,
        false,
    )
    .await;

    // Should succeed because it's not detected as a template
    assert!(result.is_ok());
}

#[tokio::test]
async fn test_url_with_path_braces_detected_as_template() {
    // Template variables in base URL should be detected and fail with UnresolvedTemplateVariable error
    let spec = CachedSpec {
        cache_format_version: aperture_cli::cache::models::CACHE_FORMAT_VERSION,
        name: "path-api".to_string(),
        version: "1.0.0".to_string(),
        commands: vec![cached_command!("test", "test", "GET", "/test", vec![])],
        base_url: Some("https://api.example.com/{version}".to_string()),
        servers: vec!["https://api.example.com/{version}".to_string()],
        security_schemes: HashMap::new(),
        skipped_endpoints: vec![],
        server_variables: HashMap::new(),
    };

    let command =
        Command::new("api").subcommand(Command::new("test").subcommand(Command::new("test")));

    let matches = command.get_matches_from(vec!["api", "test", "test"]);

    let result = execute_request(
        &spec,
        &matches,
        None,
        false,
        None,
        None,
        &OutputFormat::Json,
        None,
        None,
        false,
    )
    .await;

    assert!(result.is_err());
    if let Err(e) = result {
        match e {
            aperture_cli::error::Error::Internal {
                kind: aperture_cli::error::ErrorKind::ServerVariable,
                message,
                ..
            } => {
                assert!(message.contains("version"));
            }
            _ => panic!("Expected Internal ServerVariable error, got: {:?}", e),
        }
    }
}

#[tokio::test]
async fn test_url_with_multiple_templates_detected() {
    // Multiple template variables should be detected and fail with UnresolvedTemplateVariable error
    let spec = CachedSpec {
        cache_format_version: aperture_cli::cache::models::CACHE_FORMAT_VERSION,
        name: "multi-template-api".to_string(),
        version: "1.0.0".to_string(),
        commands: vec![cached_command!("test", "test", "GET", "/test", vec![])],
        base_url: Some("https://{region}-{env}.api.example.com".to_string()),
        servers: vec!["https://{region}-{env}.api.example.com".to_string()],
        security_schemes: HashMap::new(),
        skipped_endpoints: vec![],
        server_variables: HashMap::new(),
    };

    let command =
        Command::new("api").subcommand(Command::new("test").subcommand(Command::new("test")));

    let matches = command.get_matches_from(vec!["api", "test", "test"]);

    let result = execute_request(
        &spec,
        &matches,
        None,
        false,
        None,
        None,
        &OutputFormat::Json,
        None,
        None,
        false,
    )
    .await;

    assert!(result.is_err());
    if let Err(e) = result {
        match e {
            aperture_cli::error::Error::Internal {
                kind: aperture_cli::error::ErrorKind::ServerVariable,
                message,
                ..
            } => {
                // Should fail on the first template variable encountered
                assert!(message.contains("region"));
            }
            _ => panic!("Expected Internal ServerVariable error, got: {:?}", e),
        }
    }
}

#[tokio::test]
async fn test_url_with_empty_braces_detected_as_invalid_template() {
    // Empty braces should be detected as invalid template with empty variable name
    let spec = CachedSpec {
        cache_format_version: aperture_cli::cache::models::CACHE_FORMAT_VERSION,
        name: "empty-braces-api".to_string(),
        version: "1.0.0".to_string(),
        commands: vec![cached_command!("test", "test", "GET", "/test", vec![])],
        base_url: Some("https://api.example.com/path{}".to_string()),
        servers: vec!["https://api.example.com/path{}".to_string()],
        security_schemes: HashMap::new(),
        skipped_endpoints: vec![],
        server_variables: HashMap::new(),
    };

    let command =
        Command::new("api").subcommand(Command::new("test").subcommand(Command::new("test")));

    let matches = command.get_matches_from(vec!["api", "test", "test"]);

    let result = execute_request(
        &spec,
        &matches,
        None,
        false,
        None,
        None,
        &OutputFormat::Json,
        None,
        None,
        false,
    )
    .await;

    // Should fail with validation error for empty template variable name
    assert!(result.is_err());
    if let Err(e) = result {
        match e {
            aperture_cli::error::Error::Internal {
                kind: aperture_cli::error::ErrorKind::Validation,
                message,
                ..
            } => {
                assert!(message.contains("Missing required path parameter"));
            }
            aperture_cli::error::Error::Internal {
                kind: aperture_cli::error::ErrorKind::ServerVariable,
                message,
                ..
            } => {
                assert!(message.contains("Empty template variable name") || message.contains("{}"));
            }
            _ => panic!(
                "Expected Validation or ServerVariable error for empty template variable, got: {}",
                e
            ),
        }
    }
}
