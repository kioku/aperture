mod test_helpers;

use aperture_cli::cache::models::{CachedCommand, CachedParameter, CachedSpec};
use aperture_cli::cli::OutputFormat;
use aperture_cli::constants;
use aperture_cli::engine::executor::execute_request;
use aperture_cli::engine::generator::{generate_command_tree, generate_command_tree_with_flags};
use aperture_cli::response_cache::{CacheConfig, ResponseCache};
use std::collections::HashMap;
use std::time::Duration;
use tempfile::TempDir;
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

fn create_comprehensive_test_spec() -> CachedSpec {
    CachedSpec {
        cache_format_version: aperture_cli::cache::models::CACHE_FORMAT_VERSION,
        name: "comprehensive-api".to_string(),
        version: "1.0.0".to_string(),
        commands: vec![
            CachedCommand {
                name: "users".to_string(),
                description: Some("Get user by ID".to_string()),
                summary: None,
                operation_id: "getUserById".to_string(),
                method: "GET".to_string(),
                path: "/users/{id}".to_string(),
                parameters: vec![
                    CachedParameter {
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
                    },
                    CachedParameter {
                        name: "include_profile".to_string(),
                        location: "query".to_string(),
                        required: false,
                        description: Some("Include profile information".to_string()),
                        schema: Some(r#"{"type": "boolean"}"#.to_string()),
                        schema_type: Some("boolean".to_string()),
                        format: None,
                        default_value: None,
                        enum_values: vec![],
                        example: None,
                    },
                    CachedParameter {
                        name: "x-request-id".to_string(),
                        location: "header".to_string(),
                        required: false,
                        description: Some("Request ID for tracking".to_string()),
                        schema: Some(r#"{"type": "string"}"#.to_string()),
                        schema_type: Some("string".to_string()),
                        format: None,
                        default_value: None,
                        enum_values: vec![],
                        example: None,
                    },
                ],
                request_body: None,
                responses: vec![],
                security_requirements: vec![],
                tags: vec!["users".to_string()],
                deprecated: false,
                external_docs_url: None,
                examples: vec![],
                display_group: None,
                display_name: None,
                aliases: vec![],
                hidden: false,
            },
            CachedCommand {
                name: "posts".to_string(),
                description: Some("Create a new post".to_string()),
                summary: None,
                operation_id: "createPost".to_string(),
                method: "POST".to_string(),
                path: "/posts".to_string(),
                parameters: vec![],
                request_body: Some(aperture_cli::cache::models::CachedRequestBody {
                    description: Some("Post data".to_string()),
                    required: true,
                    content_type: constants::CONTENT_TYPE_JSON.to_string(),
                    schema: r#"{"type": "object"}"#.to_string(),
                    example: Some(
                        r#"{"title": "Test Post", "content": "This is a test post"}"#.to_string(),
                    ),
                }),
                responses: vec![],
                security_requirements: vec![],
                tags: vec!["posts".to_string()],
                deprecated: false,
                external_docs_url: None,
                examples: vec![],
                display_group: None,
                display_name: None,
                aliases: vec![],
                hidden: false,
            },
        ],
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
        max_entries: 100,
        enabled: true,
        allow_authenticated: false,
    };
    (cache_config, temp_dir)
}

#[tokio::test]
async fn test_flag_based_syntax_with_caching() {
    let mock_server = MockServer::start().await;
    let (cache_config, _temp_dir) = create_test_cache_config();
    let spec = create_comprehensive_test_spec();

    // Configure mock to be called only once (caching should work)
    Mock::given(method("GET"))
        .and(path("/users/123"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "id": "123",
            "name": "Test User",
            "profile": {"age": 30}
        })))
        .expect(1)
        .mount(&mock_server)
        .await;

    // Test with flag-based syntax (now default)
    let command = generate_command_tree_with_flags(&spec, false);
    let users_cmd = command.find_subcommand("users").unwrap();
    let get_user_cmd = users_cmd.find_subcommand("get-user-by-id").unwrap();

    // Verify that path parameter is now a flag
    let id_arg = get_user_cmd
        .get_arguments()
        .find(|arg| arg.get_id() == "id")
        .unwrap();
    assert_eq!(id_arg.get_long(), Some("id"));

    // Create matches using flag-based syntax
    let matches = command
        .try_get_matches_from(vec![
            "api",
            "users",
            "get-user-by-id",
            "--id",
            "123",
            "--include-profile",
            "--x-request-id",
            "req-123",
        ])
        .unwrap();

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
        None,  // retry_context
    )
    .await;
    assert!(result1.is_ok());

    // Second request should use cache
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
        None,  // retry_context
    )
    .await;
    assert!(result2.is_ok());

    // Verify cache was used
    let cache = ResponseCache::new(cache_config).unwrap();
    let stats = cache.get_stats(Some("comprehensive-api")).await.unwrap();
    assert_eq!(stats.total_entries, 1);
    assert_eq!(stats.valid_entries, 1);
}

#[tokio::test]
async fn test_legacy_positional_syntax_with_caching() {
    let mock_server = MockServer::start().await;
    let (cache_config, _temp_dir) = create_test_cache_config();
    let spec = create_comprehensive_test_spec();

    // Configure mock to be called only once (caching should work)
    Mock::given(method("GET"))
        .and(path("/users/456"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "id": "456",
            "name": "Another User"
        })))
        .expect(1)
        .mount(&mock_server)
        .await;

    // Test with legacy positional syntax
    let command = generate_command_tree_with_flags(&spec, true);
    let users_cmd = command.find_subcommand("users").unwrap();
    let get_user_cmd = users_cmd.find_subcommand("get-user-by-id").unwrap();

    // Verify that path parameter is positional
    let id_arg = get_user_cmd
        .get_arguments()
        .find(|arg| arg.get_id() == "id")
        .unwrap();
    assert!(id_arg.get_long().is_none());

    // Create matches using positional syntax (without include-profile flag)
    let matches = command
        .try_get_matches_from(vec!["api", "users", "get-user-by-id", "456"])
        .unwrap();

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
        None,  // retry_context
    )
    .await;
    assert!(result1.is_ok());

    // Second request should use cache
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
        None,  // retry_context
    )
    .await;
    assert!(result2.is_ok());

    // Verify cache was used
    let cache = ResponseCache::new(cache_config).unwrap();
    let stats = cache.get_stats(Some("comprehensive-api")).await.unwrap();
    assert_eq!(stats.total_entries, 1);
    assert_eq!(stats.valid_entries, 1);
}

#[tokio::test]
async fn test_different_parameter_combinations_cache_separately() {
    let mock_server = MockServer::start().await;
    let (cache_config, _temp_dir) = create_test_cache_config();
    let spec = create_comprehensive_test_spec();

    // Configure mocks for different parameter combinations
    Mock::given(method("GET"))
        .and(path("/users/123"))
        .and(wiremock::matchers::query_param("include_profile", "true"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "id": "123",
            "name": "User with profile",
            "profile": {"age": 30}
        })))
        .expect(1)
        .mount(&mock_server)
        .await;

    Mock::given(method("GET"))
        .and(path("/users/123"))
        .and(wiremock::matchers::query_param_is_missing(
            "include_profile",
        ))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "id": "123",
            "name": "User without profile"
        })))
        .expect(1)
        .mount(&mock_server)
        .await;

    // Request with include_profile=true (flag present)
    let command1 = generate_command_tree_with_flags(&spec, false);
    let matches1 = command1
        .try_get_matches_from(vec![
            "api",
            "users",
            "get-user-by-id",
            "--id",
            "123",
            "--include-profile",
        ])
        .unwrap();

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
        None,  // retry_context
    )
    .await;
    assert!(result1.is_ok());

    // Request without include_profile (flag absent, should be cached separately)
    let command2 = generate_command_tree_with_flags(&spec, false);
    let matches2 = command2
        .try_get_matches_from(vec!["api", "users", "get-user-by-id", "--id", "123"])
        .unwrap();

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
        None,  // retry_context
    )
    .await;
    assert!(result2.is_ok());

    // Verify both combinations were cached separately
    let cache = ResponseCache::new(cache_config).unwrap();
    let stats = cache.get_stats(Some("comprehensive-api")).await.unwrap();
    assert_eq!(stats.total_entries, 2);
    assert_eq!(stats.valid_entries, 2);
}

#[tokio::test]
async fn test_post_request_with_body_and_caching() {
    let mock_server = MockServer::start().await;
    let (cache_config, _temp_dir) = create_test_cache_config();
    let spec = create_comprehensive_test_spec();

    // Configure mock for POST request
    Mock::given(method("POST"))
        .and(path("/posts"))
        .respond_with(ResponseTemplate::new(201).set_body_json(serde_json::json!({
            "id": "post-123",
            "title": "Test Post",
            "content": "This is a test post",
            "created_at": "2023-01-01T00:00:00Z"
        })))
        .expect(1)
        .mount(&mock_server)
        .await;

    let command = generate_command_tree_with_flags(&spec, false);

    // Create matches for POST request with body
    let matches = command
        .try_get_matches_from(vec![
            "api",
            "posts",
            "create-post",
            "--body",
            r#"{"title": "Test Post", "content": "This is a test post"}"#,
        ])
        .unwrap();

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
        None,  // retry_context
    )
    .await;
    assert!(result1.is_ok());

    // Second identical request should use cache
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
        None,  // retry_context
    )
    .await;
    assert!(result2.is_ok());

    // Verify cache was used
    let cache = ResponseCache::new(cache_config).unwrap();
    let stats = cache.get_stats(Some("comprehensive-api")).await.unwrap();
    assert_eq!(stats.total_entries, 1);
    assert_eq!(stats.valid_entries, 1);
}

#[tokio::test]
async fn test_dry_run_with_flag_based_syntax() {
    let mock_server = MockServer::start().await;
    let (cache_config, _temp_dir) = create_test_cache_config();
    let spec = create_comprehensive_test_spec();

    // Mock should not be called in dry run mode
    Mock::given(method("GET"))
        .and(path("/users/123"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({})))
        .expect(0)
        .mount(&mock_server)
        .await;

    let command = generate_command_tree_with_flags(&spec, false);

    let matches = command
        .try_get_matches_from(vec![
            "api",
            "users",
            "get-user-by-id",
            "--id",
            "123",
            "--include-profile",
        ])
        .unwrap();

    // Dry run should not hit the API or cache
    let result = execute_request(
        &spec,
        &matches,
        Some(&mock_server.uri()),
        true, // dry_run = true
        None,
        None,
        &OutputFormat::Json,
        None,
        Some(&cache_config),
        false, // capture_output
        None,  // retry_context
    )
    .await;
    assert!(result.is_ok());

    // Verify no cache entries were created
    let cache = ResponseCache::new(cache_config).unwrap();
    let stats = cache.get_stats(Some("comprehensive-api")).await.unwrap();
    assert_eq!(stats.total_entries, 0);
}

#[tokio::test]
async fn test_cache_with_custom_ttl() {
    let mock_server = MockServer::start().await;
    let (mut cache_config, _temp_dir) = create_test_cache_config();
    cache_config.default_ttl = Duration::from_millis(800); // Short TTL for fast testing
    let spec = create_comprehensive_test_spec();

    // Configure mock to be called twice (initial + after expiration)
    Mock::given(method("GET"))
        .and(path("/users/789"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "id": "789",
            "name": "TTL Test User"
        })))
        .expect(2)
        .mount(&mock_server)
        .await;

    let command = generate_command_tree(&spec);

    let matches = command
        .try_get_matches_from(vec!["api", "users", "get-user-by-id", "--id", "789"])
        .unwrap();

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
        None,  // retry_context
    )
    .await;
    assert!(result1.is_ok());

    // Verify cache entry exists
    let cache = ResponseCache::new(cache_config.clone()).unwrap();
    let stats = cache.get_stats(Some("comprehensive-api")).await.unwrap();
    assert_eq!(stats.total_entries, 1);
    assert_eq!(stats.valid_entries, 1);

    // Wait for TTL to expire (800ms TTL + buffer)
    tokio::time::sleep(Duration::from_millis(1000)).await;

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
        None,  // retry_context
    )
    .await;
    assert!(result2.is_ok());

    // Verify cache entry was refreshed
    let stats = cache.get_stats(Some("comprehensive-api")).await.unwrap();
    assert_eq!(stats.total_entries, 1);
    assert_eq!(stats.valid_entries, 1);
}
