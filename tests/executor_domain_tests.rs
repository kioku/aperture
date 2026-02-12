use aperture_cli::cache::models::{CachedCommand, CachedParameter, CachedSpec};
use aperture_cli::engine::executor::execute;
use aperture_cli::invocation::{ExecutionContext, ExecutionResult, OperationCall};
use aperture_cli::response_cache::CacheConfig;
use std::collections::HashMap;
use std::time::Duration;
use tempfile::tempdir;
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

fn test_spec() -> CachedSpec {
    CachedSpec {
        cache_format_version: aperture_cli::cache::models::CACHE_FORMAT_VERSION,
        name: "test-api".to_string(),
        version: "1.0.0".to_string(),
        commands: vec![CachedCommand {
            name: "users".to_string(),
            description: None,
            summary: Some("Get user by id".to_string()),
            operation_id: "getUserById".to_string(),
            method: "GET".to_string(),
            path: "/users/{id}".to_string(),
            parameters: vec![CachedParameter {
                name: "id".to_string(),
                location: "path".to_string(),
                required: true,
                description: None,
                schema: Some("{\"type\":\"string\"}".to_string()),
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
            examples: vec![],
        }],
        base_url: Some("https://api.example.com".to_string()),
        servers: vec!["https://api.example.com".to_string()],
        security_schemes: HashMap::new(),
        skipped_endpoints: vec![],
        server_variables: HashMap::new(),
    }
}

fn user_by_id_call(id: &str) -> OperationCall {
    let mut path_params = HashMap::new();
    path_params.insert("id".to_string(), id.to_string());

    OperationCall {
        operation_id: "getUserById".to_string(),
        path_params,
        query_params: HashMap::new(),
        header_params: HashMap::new(),
        body: None,
        custom_headers: vec![],
    }
}

#[tokio::test]
async fn execute_returns_dry_run_result_without_network_call() {
    let spec = test_spec();
    let call = user_by_id_call("123");

    let ctx = ExecutionContext {
        dry_run: true,
        base_url: Some("https://example.test".to_string()),
        ..ExecutionContext::default()
    };

    let result = execute(&spec, call, ctx)
        .await
        .expect("dry-run execution should succeed");

    match result {
        ExecutionResult::DryRun { request_info } => {
            assert_eq!(request_info["dry_run"], true);
            assert_eq!(request_info["method"], "GET");
            assert_eq!(request_info["url"], "https://example.test/users/123");
            assert_eq!(request_info["operation_id"], "getUserById");
        }
        _ => panic!("Expected DryRun result"),
    }
}

#[tokio::test]
async fn execute_returns_success_for_http_200() {
    let mock_server = MockServer::start().await;

    Mock::given(method("GET"))
        .and(path("/users/123"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "id": "123",
            "name": "Alice"
        })))
        .expect(1)
        .mount(&mock_server)
        .await;

    let spec = test_spec();
    let call = user_by_id_call("123");
    let ctx = ExecutionContext {
        base_url: Some(mock_server.uri()),
        ..ExecutionContext::default()
    };

    let result = execute(&spec, call, ctx)
        .await
        .expect("request should succeed");

    match result {
        ExecutionResult::Success {
            body,
            status,
            headers,
        } => {
            assert_eq!(status, 200);
            let parsed: serde_json::Value =
                serde_json::from_str(&body).expect("body should be valid JSON");
            assert_eq!(parsed["id"], "123");
            assert!(headers.contains_key("content-type"));
        }
        _ => panic!("Expected Success result"),
    }
}

#[tokio::test]
async fn execute_returns_cached_result_on_repeat_call() {
    let mock_server = MockServer::start().await;

    Mock::given(method("GET"))
        .and(path("/users/123"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "id": "123",
            "cached": true
        })))
        .expect(1)
        .mount(&mock_server)
        .await;

    let cache_dir = tempdir().expect("tempdir should be created");
    let cache_config = CacheConfig {
        cache_dir: cache_dir.path().to_path_buf(),
        default_ttl: Duration::from_secs(60),
        max_entries: 100,
        enabled: true,
        allow_authenticated: false,
    };

    let spec = test_spec();
    let call = user_by_id_call("123");

    let first_ctx = ExecutionContext {
        base_url: Some(mock_server.uri()),
        cache_config: Some(cache_config.clone()),
        ..ExecutionContext::default()
    };

    let first = execute(&spec, call.clone(), first_ctx)
        .await
        .expect("first request should succeed");
    assert!(matches!(first, ExecutionResult::Success { .. }));

    let second_ctx = ExecutionContext {
        base_url: Some(mock_server.uri()),
        cache_config: Some(cache_config),
        ..ExecutionContext::default()
    };

    let second = execute(&spec, call, second_ctx)
        .await
        .expect("second request should succeed");

    match second {
        ExecutionResult::Cached { body } => {
            let parsed: serde_json::Value =
                serde_json::from_str(&body).expect("cached body should be valid JSON");
            assert_eq!(parsed["cached"], true);
        }
        _ => panic!("Expected Cached result on second call"),
    }
}
