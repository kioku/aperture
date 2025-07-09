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
    )
    .await;
    assert!(result.is_ok());
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
    )
    .await;
    assert!(result.is_err());

    if let Err(e) = result {
        let error_msg = e.to_string();
        assert!(error_msg.contains("404"));
        assert!(error_msg.contains(r#""error":"User not found"#));
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
    };

    // Create global config with API override
    let mut api_configs = HashMap::new();
    api_configs.insert(
        "test-api".to_string(),
        ApiConfig {
            base_url_override: Some(mock_server.uri()),
            environment_urls: HashMap::new(),
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
    )
    .await;
    assert!(result.is_ok());
}
