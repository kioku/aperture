use aperture_cli::cache::models::{CachedCommand, CachedParameter, CachedSpec};
use aperture_cli::cli::OutputFormat;
use aperture_cli::engine::executor::execute_request;
use aperture_cli::response_cache::{CacheConfig, ResponseCache};
use clap::{Arg, Command};
use std::collections::HashMap;
use std::time::Duration;
use tempfile::TempDir;
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

fn create_test_spec() -> CachedSpec {
    CachedSpec {
        cache_format_version: aperture_cli::cache::models::CACHE_FORMAT_VERSION,
        name: "test-api".to_string(),
        version: "1.0.0".to_string(),
        commands: vec![CachedCommand {
            name: "users".to_string(),
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
            security_requirements: vec![],
            tags: vec!["users".to_string()],
            deprecated: false,
            external_docs_url: None,
        }],
        base_url: Some("https://api.example.com".to_string()),
        servers: vec!["https://api.example.com".to_string()],
        security_schemes: HashMap::new(),
        skipped_endpoints: vec![],
        server_variables: HashMap::new(),
    }
}

fn create_test_cache_config() -> (CacheConfig, TempDir) {
    let temp_dir = TempDir::new().unwrap();
    let cache_config = CacheConfig {
        cache_dir: temp_dir.path().to_path_buf(),
        default_ttl: Duration::from_secs(60),
        max_entries: 10,
        enabled: true,
    };
    (cache_config, temp_dir)
}

#[tokio::test]
async fn test_response_caching_enabled() {
    let mock_server = MockServer::start().await;
    let (cache_config, _temp_dir) = create_test_cache_config();
    let spec = create_test_spec();

    // Configure mock to be called only once
    Mock::given(method("GET"))
        .and(path("/users/123"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "id": "123",
            "name": "Test User",
            "cached": false
        })))
        .expect(1)
        .mount(&mock_server)
        .await;

    let command = Command::new("api").subcommand(
        Command::new("users")
            .subcommand(Command::new("get-user-by-id").arg(Arg::new("id").required(true))),
    );

    let matches = command.get_matches_from(vec!["api", "users", "get-user-by-id", "123"]);

    // First request should hit the API
    let result1 = execute_request(
        &spec,
        &matches,
        Some(&mock_server.uri()),
        false,
        None,
        None,
        &OutputFormat::Json,
        None,
        Some(&cache_config),
        false, // capture_output
    )
    .await;
    assert!(result1.is_ok());

    // Second request should use cache (mock expects only 1 call)
    let result2 = execute_request(
        &spec,
        &matches,
        Some(&mock_server.uri()),
        false,
        None,
        None,
        &OutputFormat::Json,
        None,
        Some(&cache_config),
        false, // capture_output
    )
    .await;
    assert!(result2.is_ok());

    // Verify cache file was created
    let cache = ResponseCache::new(cache_config).unwrap();
    let stats = cache.get_stats(Some("test-api")).await.unwrap();
    assert_eq!(stats.total_entries, 1);
    assert_eq!(stats.valid_entries, 1);
}

#[tokio::test]
async fn test_response_caching_disabled() {
    let mock_server = MockServer::start().await;
    let (mut cache_config, _temp_dir) = create_test_cache_config();
    cache_config.enabled = false; // Disable caching
    let spec = create_test_spec();

    // Configure mock to be called twice (no caching)
    Mock::given(method("GET"))
        .and(path("/users/123"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "id": "123",
            "name": "Test User"
        })))
        .expect(2)
        .mount(&mock_server)
        .await;

    let command = Command::new("api").subcommand(
        Command::new("users")
            .subcommand(Command::new("get-user-by-id").arg(Arg::new("id").required(true))),
    );

    let matches = command.get_matches_from(vec!["api", "users", "get-user-by-id", "123"]);

    // Both requests should hit the API
    let result1 = execute_request(
        &spec,
        &matches,
        Some(&mock_server.uri()),
        false,
        None,
        None,
        &OutputFormat::Json,
        None,
        Some(&cache_config),
        false, // capture_output
    )
    .await;
    assert!(result1.is_ok());

    let result2 = execute_request(
        &spec,
        &matches,
        Some(&mock_server.uri()),
        false,
        None,
        None,
        &OutputFormat::Json,
        None,
        Some(&cache_config),
        false, // capture_output
    )
    .await;
    assert!(result2.is_ok());

    // Verify no cache file was created
    let cache = ResponseCache::new(cache_config).unwrap();
    let stats = cache.get_stats(Some("test-api")).await.unwrap();
    assert_eq!(stats.total_entries, 0);
}

#[tokio::test]
async fn test_response_cache_expiration() {
    let mock_server = MockServer::start().await;
    let (mut cache_config, _temp_dir) = create_test_cache_config();
    cache_config.default_ttl = Duration::from_secs(1); // Short TTL
    let spec = create_test_spec();

    // Configure mock to be called twice (initial + after expiration)
    Mock::given(method("GET"))
        .and(path("/users/123"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "id": "123",
            "name": "Test User"
        })))
        .expect(2)
        .mount(&mock_server)
        .await;

    let command = Command::new("api").subcommand(
        Command::new("users")
            .subcommand(Command::new("get-user-by-id").arg(Arg::new("id").required(true))),
    );

    let matches = command.get_matches_from(vec!["api", "users", "get-user-by-id", "123"]);

    // First request
    let result1 = execute_request(
        &spec,
        &matches,
        Some(&mock_server.uri()),
        false,
        None,
        None,
        &OutputFormat::Json,
        None,
        Some(&cache_config),
        false, // capture_output
    )
    .await;
    assert!(result1.is_ok());

    // Wait for cache to expire
    tokio::time::sleep(Duration::from_secs(2)).await;

    // Second request should hit API again due to expiration
    let result2 = execute_request(
        &spec,
        &matches,
        Some(&mock_server.uri()),
        false,
        None,
        None,
        &OutputFormat::Json,
        None,
        Some(&cache_config),
        false, // capture_output
    )
    .await;
    assert!(result2.is_ok());
}

#[tokio::test]
async fn test_response_cache_different_parameters() {
    let mock_server = MockServer::start().await;
    let (cache_config, _temp_dir) = create_test_cache_config();
    let spec = create_test_spec();

    // Configure mocks for different user IDs
    Mock::given(method("GET"))
        .and(path("/users/123"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "id": "123",
            "name": "User 123"
        })))
        .expect(1)
        .mount(&mock_server)
        .await;

    Mock::given(method("GET"))
        .and(path("/users/456"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "id": "456",
            "name": "User 456"
        })))
        .expect(1)
        .mount(&mock_server)
        .await;

    // Request for user 123
    let command1 = Command::new("api").subcommand(
        Command::new("users")
            .subcommand(Command::new("get-user-by-id").arg(Arg::new("id").required(true))),
    );
    let matches1 = command1.get_matches_from(vec!["api", "users", "get-user-by-id", "123"]);
    let result1 = execute_request(
        &spec,
        &matches1,
        Some(&mock_server.uri()),
        false,
        None,
        None,
        &OutputFormat::Json,
        None,
        Some(&cache_config),
        false, // capture_output
    )
    .await;
    assert!(result1.is_ok());

    // Request for user 456
    let command2 = Command::new("api").subcommand(
        Command::new("users")
            .subcommand(Command::new("get-user-by-id").arg(Arg::new("id").required(true))),
    );
    let matches2 = command2.get_matches_from(vec!["api", "users", "get-user-by-id", "456"]);
    let result2 = execute_request(
        &spec,
        &matches2,
        Some(&mock_server.uri()),
        false,
        None,
        None,
        &OutputFormat::Json,
        None,
        Some(&cache_config),
        false, // capture_output
    )
    .await;
    assert!(result2.is_ok());

    // Verify both requests were cached separately
    let cache = ResponseCache::new(cache_config).unwrap();
    let stats = cache.get_stats(Some("test-api")).await.unwrap();
    assert_eq!(stats.total_entries, 2);
    assert_eq!(stats.valid_entries, 2);
}

#[tokio::test]
async fn test_response_cache_no_config() {
    let mock_server = MockServer::start().await;
    let spec = create_test_spec();

    // Configure mock to be called twice (no caching)
    Mock::given(method("GET"))
        .and(path("/users/123"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "id": "123",
            "name": "Test User"
        })))
        .expect(2)
        .mount(&mock_server)
        .await;

    let command = Command::new("api").subcommand(
        Command::new("users")
            .subcommand(Command::new("get-user-by-id").arg(Arg::new("id").required(true))),
    );

    let matches = command.get_matches_from(vec!["api", "users", "get-user-by-id", "123"]);

    // Both requests should hit the API (no cache config provided)
    let result1 = execute_request(
        &spec,
        &matches,
        Some(&mock_server.uri()),
        false,
        None,
        None,
        &OutputFormat::Json,
        None,
        None,  // No cache config
        false, // capture_output
    )
    .await;
    assert!(result1.is_ok());

    let result2 = execute_request(
        &spec,
        &matches,
        Some(&mock_server.uri()),
        false,
        None,
        None,
        &OutputFormat::Json,
        None,
        None,  // No cache config
        false, // capture_output
    )
    .await;
    assert!(result2.is_ok());
}
