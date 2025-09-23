use aperture_cli::cache::models::{
    CachedApertureSecret, CachedCommand, CachedParameter, CachedSecurityScheme, CachedSpec,
    CACHE_FORMAT_VERSION,
};
use aperture_cli::cli::OutputFormat;
use aperture_cli::config::models::{ApertureSecret, ApiConfig, GlobalConfig, SecretSource};
use aperture_cli::constants;
use aperture_cli::engine::executor::execute_request;
use clap::{Arg, Command};
use serde_json::json;
use std::collections::HashMap;
use wiremock::matchers::{header, method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

/// Clean up all environment variables used in tests to avoid pollution between tests
fn cleanup_env_vars() {
    // Remove all test environment variables
    let test_env_vars = [
        "SPEC_BEARER_TOKEN",
        "CONFIG_BEARER_TOKEN",
        "SPEC_API_KEY",
        "CONFIG_API_KEY",
        "OTHER_API_BEARER_TOKEN",
        "OTHER_API_KEY",
        "MISSING_SPEC_BEARER_TOKEN",
        "MISSING_SPEC_API_KEY",
        "MISSING_CONFIG_BEARER_TOKEN",
        "MISSING_CONFIG_API_KEY",
        "MISSING_CONFIG_SPEC_BEARER_TOKEN",
        "MISSING_CONFIG_SPEC_API_KEY",
    ];

    for var in &test_env_vars {
        std::env::remove_var(var);
    }
}

/// Creates a test API spec with authentication schemes for priority testing
fn create_test_spec_with_auth(bearer_env_var: &str, api_key_env_var: &str) -> CachedSpec {
    let mut security_schemes = HashMap::new();

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
                name: bearer_env_var.to_string(),
            }),
        },
    );

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
                name: api_key_env_var.to_string(),
            }),
        },
    );

    CachedSpec {
        cache_format_version: CACHE_FORMAT_VERSION,
        name: "test-api".to_string(),
        version: "1.0.0".to_string(),
        commands: vec![CachedCommand {
            name: "get-user-by-id".to_string(),
            description: Some("Get user by ID".to_string()),
            summary: None,
            operation_id: "getUserById".to_string(),
            method: "GET".to_string(),
            path: "/users/{id}".to_string(),
            parameters: vec![CachedParameter {
                name: "id".to_string(),
                location: "path".to_string(),
                required: true,
                description: Some("User ID".to_string()),
                schema: Some(r#"{"type": "string"}"#.to_string()),
                schema_type: Some("string".to_string()),
                format: None,
                default_value: None,
                enum_values: vec![],
                example: None,
            }],
            request_body: None,
            responses: vec![],
            security_requirements: vec!["bearerAuth".to_string(), "apiKeyAuth".to_string()],
            tags: vec!["users".to_string()],
            deprecated: false,
            external_docs_url: None,
            examples: vec![],
        }],
        base_url: None,
        servers: vec![],
        security_schemes,
        skipped_endpoints: vec![],
        server_variables: HashMap::new(),
    }
}

/// Creates a global config with secret overrides
fn create_global_config_with_secrets(
    api_name: &str,
    bearer_config_env: &str,
    api_key_config_env: &str,
) -> GlobalConfig {
    let mut secrets = HashMap::new();
    secrets.insert(
        "bearerAuth".to_string(),
        ApertureSecret {
            source: SecretSource::Env,
            name: bearer_config_env.to_string(),
        },
    );
    secrets.insert(
        "apiKeyAuth".to_string(),
        ApertureSecret {
            source: SecretSource::Env,
            name: api_key_config_env.to_string(),
        },
    );

    let mut api_configs = HashMap::new();
    api_configs.insert(
        api_name.to_string(),
        ApiConfig {
            base_url_override: None,
            environment_urls: HashMap::new(),
            strict_mode: false,
            secrets,
        },
    );

    GlobalConfig {
        api_configs,
        default_timeout_secs: 30,
        agent_defaults: aperture_cli::config::models::AgentDefaults::default(),
    }
}

/// Test that config-based secrets override x-aperture-secret extensions
#[tokio::test]
async fn test_config_secret_overrides_aperture_secret() {
    let mock_server = MockServer::start().await;

    // Use unique environment variable names for this test
    let test_id = "CONFIG_OVERRIDE_TEST";
    let spec_bearer_env = format!("{}_SPEC_BEARER_TOKEN", test_id);
    let config_bearer_env = format!("{}_CONFIG_BEARER_TOKEN", test_id);
    let spec_api_key_env = format!("{}_SPEC_API_KEY", test_id);
    let config_api_key_env = format!("{}_CONFIG_API_KEY", test_id);

    // Clean up any existing environment variables first
    cleanup_env_vars();
    // Clean up test-specific variables
    std::env::remove_var(&spec_bearer_env);
    std::env::remove_var(&config_bearer_env);
    std::env::remove_var(&spec_api_key_env);
    std::env::remove_var(&config_api_key_env);

    // Set up environment variables
    std::env::set_var(&spec_bearer_env, "spec-bearer-value");
    std::env::set_var(&config_bearer_env, "config-bearer-value");
    std::env::set_var(&spec_api_key_env, "spec-api-key-value");
    std::env::set_var(&config_api_key_env, "config-api-key-value");

    // Create spec with x-aperture-secret extensions
    let spec = create_test_spec_with_auth(&spec_bearer_env, &spec_api_key_env);

    // Create global config that overrides the secrets
    let global_config =
        create_global_config_with_secrets("test-api", &config_bearer_env, &config_api_key_env);

    // Mock should expect config values, not spec values
    Mock::given(method("GET"))
        .and(path("/users/123"))
        .and(header("Authorization", "Bearer config-bearer-value"))
        .and(header("X-API-Key", "config-api-key-value"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "id": "123",
            "name": "Test User"
        })))
        .expect(1)
        .mount(&mock_server)
        .await;

    // Create clap matches for the command
    let command = Command::new("api").subcommand(
        Command::new("users")
            .subcommand(Command::new("get-user-by-id").arg(Arg::new("id").required(true))),
    );
    let matches = command.get_matches_from(vec!["api", "users", "get-user-by-id", "123"]);

    // Execute request with global config
    let result = execute_request(
        &spec,
        &matches,
        Some(&mock_server.uri()),
        false,
        None,
        Some(&global_config),
        &OutputFormat::Json,
        None,
        None,
        false,
    )
    .await;

    assert!(result.is_ok(), "Request should succeed with config secrets");

    // Clean up environment variables
    cleanup_env_vars();
    std::env::remove_var(&spec_bearer_env);
    std::env::remove_var(&config_bearer_env);
    std::env::remove_var(&spec_api_key_env);
    std::env::remove_var(&config_api_key_env);
}

/// Test that x-aperture-secret extensions are used when no config secret exists
#[tokio::test]
async fn test_aperture_secret_used_when_no_config() {
    let mock_server = MockServer::start().await;

    // Use unique environment variable names for this test
    let test_id = "APERTURE_SECRET_TEST";
    let spec_bearer_env = format!("{}_SPEC_BEARER_TOKEN", test_id);
    let spec_api_key_env = format!("{}_SPEC_API_KEY", test_id);

    // Clean up any existing environment variables first
    cleanup_env_vars();
    std::env::remove_var(&spec_bearer_env);
    std::env::remove_var(&spec_api_key_env);

    // Set up only spec environment variables
    std::env::set_var(&spec_bearer_env, "spec-bearer-value");
    std::env::set_var(&spec_api_key_env, "spec-api-key-value");

    // Create spec with x-aperture-secret extensions
    let spec = create_test_spec_with_auth(&spec_bearer_env, &spec_api_key_env);

    // No global config (empty)
    let global_config = GlobalConfig {
        api_configs: HashMap::new(),
        default_timeout_secs: 30,
        agent_defaults: aperture_cli::config::models::AgentDefaults::default(),
    };

    // Mock should expect spec values
    Mock::given(method("GET"))
        .and(path("/users/123"))
        .and(header("Authorization", "Bearer spec-bearer-value"))
        .and(header("X-API-Key", "spec-api-key-value"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "id": "123",
            "name": "Test User"
        })))
        .expect(1)
        .mount(&mock_server)
        .await;

    // Create clap matches for the command
    let command = Command::new("api").subcommand(
        Command::new("users")
            .subcommand(Command::new("get-user-by-id").arg(Arg::new("id").required(true))),
    );
    let matches = command.get_matches_from(vec!["api", "users", "get-user-by-id", "123"]);

    // Execute request with empty global config
    let result = execute_request(
        &spec,
        &matches,
        Some(&mock_server.uri()),
        false,
        None,
        Some(&global_config),
        &OutputFormat::Json,
        None,
        None,
        false,
    )
    .await;

    assert!(result.is_ok(), "Request should succeed with spec secrets");

    // Clean up environment variables
    cleanup_env_vars();
    std::env::remove_var(&spec_bearer_env);
    std::env::remove_var(&spec_api_key_env);
}

/// Test that missing config secret environment variable produces appropriate error
#[tokio::test]
async fn test_missing_config_secret_env_var_error() {
    let mock_server = MockServer::start().await;

    // Clean up any existing environment variables first
    cleanup_env_vars();

    // Set up unique spec environment variables for this test
    std::env::set_var("MISSING_CONFIG_SPEC_BEARER_TOKEN", "spec-bearer-value");
    std::env::set_var("MISSING_CONFIG_SPEC_API_KEY", "spec-api-key-value");
    // MISSING_CONFIG_BEARER_TOKEN is intentionally not set

    // Create spec with x-aperture-secret extensions
    let spec = create_test_spec_with_auth(
        "MISSING_CONFIG_SPEC_BEARER_TOKEN",
        "MISSING_CONFIG_SPEC_API_KEY",
    );

    // Create global config with missing environment variable
    let global_config = create_global_config_with_secrets(
        "test-api",
        "MISSING_CONFIG_BEARER_TOKEN", // This env var doesn't exist
        "MISSING_CONFIG_SPEC_API_KEY",
    );

    // No mock expectations - the request should fail before reaching the server

    // Create clap matches for the command
    let command = Command::new("api").subcommand(
        Command::new("users")
            .subcommand(Command::new("get-user-by-id").arg(Arg::new("id").required(true))),
    );
    let matches = command.get_matches_from(vec!["api", "users", "get-user-by-id", "123"]);

    // Execute request - should fail with config env var error
    let result = execute_request(
        &spec,
        &matches,
        Some(&mock_server.uri()),
        false,
        None,
        Some(&global_config),
        &OutputFormat::Json,
        None,
        None,
        false,
    )
    .await;

    assert!(
        result.is_err(),
        "Request should fail with missing config secret"
    );

    // Verify error message mentions config env var name or secret not set
    let error_message = result.unwrap_err().to_string();
    assert!(
        error_message.contains("MISSING_CONFIG_BEARER_TOKEN")
            || error_message.contains("secret not set")
            || error_message.contains("SecretNotSet"),
        "Error should mention config env var name or secret error, got: {error_message}"
    );

    // Clean up environment variables
    cleanup_env_vars();
}

/// Test that missing spec secret environment variable produces appropriate error
#[tokio::test]
async fn test_missing_spec_secret_env_var_error() {
    let mock_server = MockServer::start().await;

    // Clean up any existing environment variables first
    cleanup_env_vars();

    // Use unique env vars to avoid conflicts with other tests
    // MISSING_SPEC_BEARER_TOKEN is not set, which will cause the auth to fail

    // Create spec with x-aperture-secret extensions
    let spec = create_test_spec_with_auth("MISSING_SPEC_BEARER_TOKEN", "MISSING_SPEC_API_KEY");

    // No global config (empty) - so it should fall back to spec extension
    let global_config = GlobalConfig {
        api_configs: HashMap::new(),
        default_timeout_secs: 30,
        agent_defaults: aperture_cli::config::models::AgentDefaults::default(),
    };

    // No mock expectations - the request should fail before reaching the server

    // Create clap matches for the command
    let command = Command::new("api").subcommand(
        Command::new("users")
            .subcommand(Command::new("get-user-by-id").arg(Arg::new("id").required(true))),
    );
    let matches = command.get_matches_from(vec!["api", "users", "get-user-by-id", "123"]);

    // Execute request - should fail with spec env var error
    let result = execute_request(
        &spec,
        &matches,
        Some(&mock_server.uri()),
        false,
        None,
        Some(&global_config),
        &OutputFormat::Json,
        None,
        None,
        false,
    )
    .await;

    assert!(
        result.is_err(),
        "Request should fail with missing spec secret"
    );

    // Verify error message mentions spec env var name or secret not set
    let error_message = result.unwrap_err().to_string();
    assert!(
        error_message.contains("MISSING_SPEC_BEARER_TOKEN")
            || error_message.contains("secret not set")
            || error_message.contains("SecretNotSet"),
        "Error should mention spec env var name or secret error, got: {error_message}"
    );
}

/// Test partial config override - only some schemes have config overrides
#[tokio::test]
async fn test_partial_config_override() {
    let mock_server = MockServer::start().await;

    // Use unique environment variable names for this test
    let test_id = "PARTIAL_CONFIG_TEST";
    let spec_bearer_env = format!("{}_SPEC_BEARER_TOKEN", test_id);
    let config_bearer_env = format!("{}_CONFIG_BEARER_TOKEN", test_id);
    let spec_api_key_env = format!("{}_SPEC_API_KEY", test_id);

    // Clean up any existing environment variables first
    cleanup_env_vars();
    std::env::remove_var(&spec_bearer_env);
    std::env::remove_var(&config_bearer_env);
    std::env::remove_var(&spec_api_key_env);

    // Set up environment variables
    std::env::set_var(&spec_bearer_env, "spec-bearer-value");
    std::env::set_var(&config_bearer_env, "config-bearer-value");
    std::env::set_var(&spec_api_key_env, "spec-api-key-value");
    // Note: No CONFIG_API_KEY set

    // Create spec with x-aperture-secret extensions
    let spec = create_test_spec_with_auth(&spec_bearer_env, &spec_api_key_env);

    // Create global config that only overrides bearer auth
    let mut secrets = HashMap::new();
    secrets.insert(
        "bearerAuth".to_string(),
        ApertureSecret {
            source: SecretSource::Env,
            name: config_bearer_env.clone(),
        },
    );
    // Note: No override for apiKeyAuth

    let mut api_configs = HashMap::new();
    api_configs.insert(
        "test-api".to_string(),
        ApiConfig {
            base_url_override: None,
            environment_urls: HashMap::new(),
            strict_mode: false,
            secrets,
        },
    );

    let global_config = GlobalConfig {
        api_configs,
        default_timeout_secs: 30,
        agent_defaults: aperture_cli::config::models::AgentDefaults::default(),
    };

    // Mock should expect config bearer token but spec api key
    Mock::given(method("GET"))
        .and(path("/users/123"))
        .and(header("Authorization", "Bearer config-bearer-value"))
        .and(header("X-API-Key", "spec-api-key-value"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "id": "123",
            "name": "Test User"
        })))
        .expect(1)
        .mount(&mock_server)
        .await;

    // Create clap matches for the command
    let command = Command::new("api").subcommand(
        Command::new("users")
            .subcommand(Command::new("get-user-by-id").arg(Arg::new("id").required(true))),
    );
    let matches = command.get_matches_from(vec!["api", "users", "get-user-by-id", "123"]);

    // Execute request
    let result = execute_request(
        &spec,
        &matches,
        Some(&mock_server.uri()),
        false,
        None,
        Some(&global_config),
        &OutputFormat::Json,
        None,
        None,
        false,
    )
    .await;

    assert!(
        result.is_ok(),
        "Request should succeed with mixed auth sources"
    );

    // Clean up environment variables
    cleanup_env_vars();
    std::env::remove_var(&spec_bearer_env);
    std::env::remove_var(&config_bearer_env);
    std::env::remove_var(&spec_api_key_env);
}

/// Test that requests proceed when no authentication is configured
#[tokio::test]
async fn test_no_authentication_configured() {
    let mock_server = MockServer::start().await;

    // Clean up any existing environment variables first
    cleanup_env_vars();

    // Create spec without any authentication
    let spec = CachedSpec {
        cache_format_version: CACHE_FORMAT_VERSION,
        name: "test-api".to_string(),
        version: "1.0.0".to_string(),
        commands: vec![CachedCommand {
            name: "get-user-by-id".to_string(),
            description: Some("Get user by ID".to_string()),
            summary: None,
            operation_id: "getUserById".to_string(),
            method: "GET".to_string(),
            path: "/users/{id}".to_string(),
            parameters: vec![CachedParameter {
                name: "id".to_string(),
                location: "path".to_string(),
                required: true,
                description: Some("User ID".to_string()),
                schema: Some(r#"{"type": "string"}"#.to_string()),
                schema_type: Some("string".to_string()),
                format: None,
                default_value: None,
                enum_values: vec![],
                example: None,
            }],
            request_body: None,
            responses: vec![],
            security_requirements: vec![], // No security requirements
            tags: vec!["users".to_string()],
            deprecated: false,
            external_docs_url: None,
            examples: vec![],
        }],
        base_url: None,
        servers: vec![],
        security_schemes: HashMap::new(), // No security schemes defined
        skipped_endpoints: vec![],
        server_variables: HashMap::new(),
    };

    // Mock should not expect any authentication headers
    Mock::given(method("GET"))
        .and(path("/users/123"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "id": "123",
            "name": "Test User"
        })))
        .expect(1)
        .mount(&mock_server)
        .await;

    // Execute request with empty global config
    let global_config = GlobalConfig {
        api_configs: HashMap::new(),
        default_timeout_secs: 30,
        agent_defaults: aperture_cli::config::models::AgentDefaults::default(),
    };

    // Create clap matches for the command
    let command = Command::new("api").subcommand(
        Command::new("users")
            .subcommand(Command::new("get-user-by-id").arg(Arg::new("id").required(true))),
    );
    let matches = command.get_matches_from(vec!["api", "users", "get-user-by-id", "123"]);

    let result = execute_request(
        &spec,
        &matches,
        Some(&mock_server.uri()),
        false,
        None,
        Some(&global_config),
        &OutputFormat::Json,
        None,
        None,
        false,
    )
    .await;

    assert!(
        result.is_ok(),
        "Request should succeed without authentication"
    );
}

/// Test authentication priority with different API configurations
#[tokio::test]
async fn test_different_api_configs() {
    let mock_server = MockServer::start().await;

    // Use unique environment variable names for this test
    let test_id = "DIFFERENT_API_TEST";
    let spec_bearer_env = format!("{}_SPEC_BEARER_TOKEN", test_id);
    let spec_api_key_env = format!("{}_SPEC_API_KEY", test_id);
    let other_bearer_env = format!("{}_OTHER_API_BEARER_TOKEN", test_id);
    let other_api_key_env = format!("{}_OTHER_API_KEY", test_id);

    // Clean up any existing environment variables first
    cleanup_env_vars();
    std::env::remove_var(&spec_bearer_env);
    std::env::remove_var(&spec_api_key_env);
    std::env::remove_var(&other_bearer_env);
    std::env::remove_var(&other_api_key_env);

    // Set up environment variables
    std::env::set_var(&spec_bearer_env, "spec-bearer-value");
    std::env::set_var(&spec_api_key_env, "spec-api-key-value"); // Both auth schemes need env vars
    std::env::set_var(&other_bearer_env, "other-api-bearer-value");

    // Create spec with x-aperture-secret extensions
    let spec = create_test_spec_with_auth(&spec_bearer_env, &spec_api_key_env);

    // Create global config for a different API (not "test-api")
    let global_config = create_global_config_with_secrets(
        "other-api", // Different API name
        &other_bearer_env,
        &other_api_key_env,
    );

    // Mock should expect spec values since config is for different API
    Mock::given(method("GET"))
        .and(path("/users/123"))
        .and(header("Authorization", "Bearer spec-bearer-value"))
        .and(header("X-API-Key", "spec-api-key-value"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "id": "123",
            "name": "Test User"
        })))
        .expect(1)
        .mount(&mock_server)
        .await;

    // Create clap matches for the command
    let command = Command::new("api").subcommand(
        Command::new("users")
            .subcommand(Command::new("get-user-by-id").arg(Arg::new("id").required(true))),
    );
    let matches = command.get_matches_from(vec!["api", "users", "get-user-by-id", "123"]);

    let result = execute_request(
        &spec,
        &matches,
        Some(&mock_server.uri()),
        false,
        None,
        Some(&global_config),
        &OutputFormat::Json,
        None,
        None,
        false,
    )
    .await;

    assert!(
        result.is_ok(),
        "Request should succeed with spec auth for unmatched API"
    );

    // Clean up environment variables
    cleanup_env_vars();
    std::env::remove_var(&spec_bearer_env);
    std::env::remove_var(&spec_api_key_env);
    std::env::remove_var(&other_bearer_env);
    std::env::remove_var(&other_api_key_env);
}
