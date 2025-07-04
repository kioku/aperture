use aperture_cli::cache::models::{
    CachedApertureSecret, CachedCommand, CachedParameter, CachedSecurityScheme, CachedSpec,
};
use aperture_cli::engine::executor::execute_request;
use clap::{Arg, Command};
use std::collections::HashMap;
use wiremock::matchers::{header, method, path};
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
    ($name:expr, $op_id:expr, $method:expr, $path:expr, $params:expr, $security:expr) => {
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
            security_requirements: $security,
            tags: vec![$name.to_string()],
            deprecated: false,
            external_docs_url: None,
        }
    };
}

fn create_secure_test_spec(bearer_env_var: &str, api_key_env_var: &str) -> CachedSpec {
    let mut security_schemes = HashMap::new();

    // Add Bearer token authentication
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
                name: bearer_env_var.to_string(),
            }),
        },
    );

    // Add API Key authentication
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
                name: api_key_env_var.to_string(),
            }),
        },
    );

    CachedSpec {
        name: "secure-api".to_string(),
        version: "1.0.0".to_string(),
        commands: vec![
            {
                let mut cmd = cached_command!(
                    "users",
                    "getUserById",
                    "GET",
                    "/users/{id}",
                    vec![cached_parameter!("id", "path", true)],
                    vec!["bearerAuth".to_string()]
                );
                cmd.description = Some("Get user by ID".to_string());
                cmd
            },
            {
                let mut cmd = cached_command!(
                    "data",
                    "getData",
                    "GET",
                    "/data",
                    vec![],
                    vec!["apiKeyAuth".to_string()]
                );
                cmd.description = Some("Get data".to_string());
                cmd
            },
            {
                let mut cmd = cached_command!(
                    "public",
                    "getPublicData",
                    "GET",
                    "/public",
                    vec![],
                    vec![] // No authentication required
                );
                cmd.description = Some("Get public data".to_string());
                cmd
            },
        ],
        base_url: Some("https://api.example.com".to_string()),
        servers: vec!["https://api.example.com".to_string()],
        security_schemes,
    }
}

#[tokio::test]
async fn test_bearer_token_authentication() {
    let mock_server = MockServer::start().await;
    let bearer_env = "BEARER_AUTH_TEST_TOKEN";
    let api_key_env = "BEARER_AUTH_TEST_API_KEY";

    // Clean up any existing env var first
    std::env::remove_var(bearer_env);
    // Set up environment variable
    std::env::set_var(bearer_env, "secret-bearer-token");

    // Configure mock to expect Bearer token
    Mock::given(method("GET"))
        .and(path("/users/123"))
        .and(header("Authorization", "Bearer secret-bearer-token"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "id": "123",
            "name": "Test User"
        })))
        .expect(1)
        .mount(&mock_server)
        .await;

    let spec = create_secure_test_spec(bearer_env, api_key_env);

    let command = Command::new("api").subcommand(
        Command::new("users")
            .subcommand(Command::new("get-user-by-id").arg(Arg::new("id").required(true))),
    );

    let matches = command.get_matches_from(vec!["api", "users", "get-user-by-id", "123"]);

    let result =
        execute_request(&spec, &matches, Some(&mock_server.uri()), false, None, None).await;
    assert!(result.is_ok());

    // Clean up
    std::env::remove_var(bearer_env);
}

#[tokio::test]
async fn test_api_key_authentication() {
    let mock_server = MockServer::start().await;
    let bearer_env = "API_KEY_TEST_BEARER";
    let api_key_env = "API_KEY_TEST_KEY";

    // Set up environment variable
    std::env::set_var(api_key_env, "my-secret-api-key");

    // Configure mock to expect API key header
    Mock::given(method("GET"))
        .and(path("/data"))
        .and(header("X-API-Key", "my-secret-api-key"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "data": "sensitive information"
        })))
        .expect(1)
        .mount(&mock_server)
        .await;

    let spec = create_secure_test_spec(bearer_env, api_key_env);

    let command =
        Command::new("api").subcommand(Command::new("data").subcommand(Command::new("get-data")));

    let matches = command.get_matches_from(vec!["api", "data", "get-data"]);

    let result =
        execute_request(&spec, &matches, Some(&mock_server.uri()), false, None, None).await;
    assert!(result.is_ok());

    // Clean up
    std::env::remove_var(api_key_env);
}

#[tokio::test]
async fn test_missing_authentication_environment_variable() {
    let mock_server = MockServer::start().await;
    let bearer_env = "MISSING_AUTH_TEST_TOKEN";
    let api_key_env = "MISSING_AUTH_TEST_KEY";

    // Ensure the environment variable is not set (multiple remove attempts for safety)
    std::env::remove_var(bearer_env);
    std::env::remove_var(bearer_env);

    let spec = create_secure_test_spec(bearer_env, api_key_env);

    let command = Command::new("api").subcommand(
        Command::new("users")
            .subcommand(Command::new("get-user-by-id").arg(Arg::new("id").required(true))),
    );

    let matches = command.get_matches_from(vec!["api", "users", "get-user-by-id", "123"]);

    let result =
        execute_request(&spec, &matches, Some(&mock_server.uri()), false, None, None).await;

    match result {
        Ok(_) => panic!("Expected error but got success"),
        Err(e) => {
            let error_msg = e.to_string();
            assert!(error_msg.contains(&format!("Environment variable '{}'", bearer_env)));
            assert!(error_msg.contains("is not set"));
        }
    }
}

#[tokio::test]
async fn test_custom_headers_with_literal_values() {
    let mock_server = MockServer::start().await;
    let bearer_env = "LITERAL_HEADERS_TEST_TOKEN";
    let api_key_env = "LITERAL_HEADERS_TEST_KEY";

    // Configure mock to expect custom headers
    Mock::given(method("GET"))
        .and(path("/public"))
        .and(header("X-Request-ID", "12345"))
        .and(header("X-Client-Version", "1.0.0"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "data": "response"
        })))
        .expect(1)
        .mount(&mock_server)
        .await;

    let spec = create_secure_test_spec(bearer_env, api_key_env);

    let command = Command::new("api").subcommand(
        Command::new("public").subcommand(
            Command::new("get-public-data").arg(
                Arg::new("header")
                    .long("header")
                    .short('H')
                    .action(clap::ArgAction::Append),
            ),
        ),
    );

    let matches = command.get_matches_from(vec![
        "api",
        "public",
        "get-public-data",
        "--header",
        "X-Request-ID: 12345",
        "-H",
        "X-Client-Version: 1.0.0",
    ]);

    let result =
        execute_request(&spec, &matches, Some(&mock_server.uri()), false, None, None).await;
    assert!(result.is_ok());
}

#[tokio::test]
async fn test_custom_headers_with_environment_variable_expansion() {
    let mock_server = MockServer::start().await;
    let bearer_env = "ENV_HEADERS_TEST_TOKEN";
    let api_key_env = "ENV_HEADERS_TEST_KEY";
    let request_id_env = "ENV_HEADERS_REQUEST_ID";
    let client_version_env = "ENV_HEADERS_CLIENT_VERSION";

    // Set up environment variables
    std::env::set_var(request_id_env, "env-request-id-123");
    std::env::set_var(client_version_env, "2.1.0");

    // Configure mock to expect headers with expanded values
    Mock::given(method("GET"))
        .and(path("/public"))
        .and(header("X-Request-ID", "env-request-id-123"))
        .and(header("X-Client-Version", "2.1.0"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "data": "response"
        })))
        .expect(1)
        .mount(&mock_server)
        .await;

    let spec = create_secure_test_spec(bearer_env, api_key_env);

    let command = Command::new("api").subcommand(
        Command::new("public").subcommand(
            Command::new("get-public-data").arg(
                Arg::new("header")
                    .long("header")
                    .short('H')
                    .action(clap::ArgAction::Append),
            ),
        ),
    );

    let matches = command.get_matches_from(vec![
        "api",
        "public",
        "get-public-data",
        "--header",
        &format!("X-Request-ID: ${{{}}}", request_id_env),
        "-H",
        &format!("X-Client-Version: ${{{}}}", client_version_env),
    ]);

    let result =
        execute_request(&spec, &matches, Some(&mock_server.uri()), false, None, None).await;
    assert!(result.is_ok());

    // Clean up
    std::env::remove_var(request_id_env);
    std::env::remove_var(client_version_env);
}

#[tokio::test]
async fn test_authentication_and_custom_headers_combined() {
    let mock_server = MockServer::start().await;
    let bearer_env = "COMBINED_TEST_BEARER_TOKEN";
    let api_key_env = "COMBINED_TEST_API_KEY";
    let trace_id_env = "COMBINED_TEST_TRACE_ID";

    // Set up environment variables
    std::env::set_var(bearer_env, "combined-test-token");
    std::env::set_var(trace_id_env, "trace-abc-123");

    // Configure mock to expect both authentication and custom headers
    Mock::given(method("GET"))
        .and(path("/users/999"))
        .and(header("Authorization", "Bearer combined-test-token"))
        .and(header("X-Trace-ID", "trace-abc-123"))
        .and(header("X-Custom", "custom-value"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "id": "999",
            "name": "Combined Test User"
        })))
        .expect(1)
        .mount(&mock_server)
        .await;

    let spec = create_secure_test_spec(bearer_env, api_key_env);

    let command = Command::new("api").subcommand(
        Command::new("users").subcommand(
            Command::new("get-user-by-id")
                .arg(Arg::new("id").required(true))
                .arg(
                    Arg::new("header")
                        .long("header")
                        .short('H')
                        .action(clap::ArgAction::Append),
                ),
        ),
    );

    let matches = command.get_matches_from(vec![
        "api",
        "users",
        "get-user-by-id",
        "999",
        "--header",
        &format!("X-Trace-ID: ${{{}}}", trace_id_env),
        "-H",
        "X-Custom: custom-value",
    ]);

    let result =
        execute_request(&spec, &matches, Some(&mock_server.uri()), false, None, None).await;
    assert!(result.is_ok());

    // Clean up
    std::env::remove_var(bearer_env);
    std::env::remove_var(trace_id_env);
}

#[tokio::test]
async fn test_invalid_custom_header_format() {
    let bearer_env = "INVALID_HEADER_TEST_TOKEN";
    let api_key_env = "INVALID_HEADER_TEST_KEY";
    let spec = create_secure_test_spec(bearer_env, api_key_env);

    let command = Command::new("api").subcommand(
        Command::new("public").subcommand(
            Command::new("get-public-data").arg(
                Arg::new("header")
                    .long("header")
                    .action(clap::ArgAction::Append),
            ),
        ),
    );

    let matches = command.get_matches_from(vec![
        "api",
        "public",
        "get-public-data",
        "--header",
        "InvalidHeaderWithoutColon",
    ]);

    let result =
        execute_request(&spec, &matches, Some("http://localhost"), false, None, None).await;

    assert!(result.is_err());
    let error_msg = result.unwrap_err().to_string();
    assert!(error_msg.contains("Invalid header format"));
    assert!(error_msg.contains("Expected 'Name: Value'"));
}

#[tokio::test]
async fn test_empty_header_name() {
    let bearer_env = "EMPTY_HEADER_TEST_TOKEN";
    let api_key_env = "EMPTY_HEADER_TEST_KEY";
    let spec = create_secure_test_spec(bearer_env, api_key_env);

    let command = Command::new("api").subcommand(
        Command::new("public").subcommand(
            Command::new("get-public-data").arg(
                Arg::new("header")
                    .long("header")
                    .action(clap::ArgAction::Append),
            ),
        ),
    );

    let matches = command.get_matches_from(vec![
        "api",
        "public",
        "get-public-data",
        "--header",
        ": value-without-name",
    ]);

    let result =
        execute_request(&spec, &matches, Some("http://localhost"), false, None, None).await;

    assert!(result.is_err());
    let error_msg = result.unwrap_err().to_string();
    assert!(error_msg.contains("Header name cannot be empty"));
}
