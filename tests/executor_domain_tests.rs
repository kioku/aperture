mod test_helpers;

use aperture_cli::cache::models::{
    CachedCommand, CachedParameter, CachedRequestBody, CachedResponse, CachedSpec, PaginationInfo,
};
use aperture_cli::engine::executor::execute;
use aperture_cli::invocation::{ExecutionContext, ExecutionResult, OperationCall, RequestBody};
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
            display_group: None,
            display_name: None,
            aliases: vec![],
            hidden: false,
            pagination: PaginationInfo::default(),
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

fn body_call(body: RequestBody) -> OperationCall {
    OperationCall {
        operation_id: "upload".to_string(),
        path_params: HashMap::new(),
        query_params: HashMap::new(),
        header_params: HashMap::new(),
        body: Some(body),
        custom_headers: vec![],
    }
}

fn body_spec(content_type: &str, schema: &str) -> CachedSpec {
    let mut spec = test_spec();
    let command = &mut spec.commands[0];
    command.name = "upload".to_string();
    command.operation_id = "upload".to_string();
    command.method = "POST".to_string();
    command.path = "/upload".to_string();
    command.parameters.clear();
    command.request_body = Some(CachedRequestBody {
        content_type: content_type.to_string(),
        schema: schema.to_string(),
        required: true,
        description: None,
        example: None,
    });
    spec
}

#[tokio::test]
async fn execute_rejects_direct_body_mismatches_before_dry_run_or_cache_work() {
    let cache_dir = tempdir().unwrap();
    let ctx = ExecutionContext {
        dry_run: true,
        base_url: Some("not a valid URL".to_string()),
        cache_config: Some(CacheConfig {
            cache_dir: cache_dir.path().to_path_buf(),
            default_ttl: Duration::from_mins(1),
            max_entries: 1,
            enabled: true,
            allow_authenticated: false,
        }),
        ..ExecutionContext::default()
    };
    let binary_spec = body_spec("image/png", r#"{"type":"string","format":"binary"}"#);
    let error = execute(
        &binary_spec,
        body_call(RequestBody::Json("{}".to_string())),
        ctx.clone(),
    )
    .await
    .unwrap_err();
    assert!(error
        .to_string()
        .contains("JSON request text does not match"));

    let json_spec = body_spec("application/json", r#"{"type":"object"}"#);
    for body in [
        RequestBody::Binary(vec![0xff]),
        RequestBody::Json("{invalid".to_string()),
    ] {
        assert!(execute(&json_spec, body_call(body), ctx.clone())
            .await
            .is_err());
    }
    assert!(!cache_dir.path().join("cache_metadata.json").exists());
}

#[tokio::test]
async fn execute_rejects_ambiguous_binary_responses_before_client_work() {
    let mut spec = test_spec();
    spec.commands[0].responses = vec![
        CachedResponse {
            status_code: "200".to_string(),
            description: None,
            content_type: Some("application/pdf".to_string()),
            schema: Some(r#"{"type":"string","format":"binary"}"#.to_string()),
            example: None,
        },
        CachedResponse {
            status_code: "201".to_string(),
            description: None,
            content_type: Some("application/json".to_string()),
            schema: Some(r#"{"type":"object"}"#.to_string()),
            example: None,
        },
    ];
    let error = execute(
        &spec,
        user_by_id_call("123"),
        ExecutionContext {
            dry_run: true,
            base_url: Some("not a valid URL".to_string()),
            ..ExecutionContext::default()
        },
    )
    .await
    .unwrap_err();
    assert!(error
        .to_string()
        .contains("mixes binary and non-binary successful responses"));
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
        default_ttl: Duration::from_mins(1),
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
        ExecutionResult::Cached { body, .. } => {
            let parsed: serde_json::Value =
                serde_json::from_str(&body).expect("cached body should be valid JSON");
            assert_eq!(parsed["cached"], true);
        }
        _ => panic!("Expected Cached result on second call"),
    }
}

#[tokio::test]
async fn cached_dry_run_does_not_read_or_create_cache() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .respond_with(
            ResponseTemplate::new(203)
                .insert_header("link", "</next>; rel=\"next\"")
                .set_body_string("cached"),
        )
        .expect(1)
        .mount(&server)
        .await;
    let dir = tempdir().unwrap();
    let mut ctx = ExecutionContext {
        base_url: Some(server.uri()),
        cache_config: Some(CacheConfig {
            cache_dir: dir.path().join("cache"),
            ..Default::default()
        }),
        ..Default::default()
    };
    let spec = test_spec();
    execute(&spec, user_by_id_call("123"), ctx.clone())
        .await
        .unwrap();
    let cached = execute(&spec, user_by_id_call("123"), ctx.clone())
        .await
        .unwrap();
    assert!(
        matches!(cached, ExecutionResult::Cached { status: 203, headers, .. } if headers.contains_key("link"))
    );
    ctx.dry_run = true;
    assert!(matches!(
        execute(&spec, user_by_id_call("123"), ctx.clone())
            .await
            .unwrap(),
        ExecutionResult::DryRun { .. }
    ));
    ctx.cache_config.as_mut().unwrap().cache_dir = dir.path().join("absent");
    execute(&spec, user_by_id_call("123"), ctx).await.unwrap();
    assert!(!dir.path().join("absent").exists());
}

#[tokio::test]
async fn repeated_posts_are_never_cached() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .respond_with(ResponseTemplate::new(200).set_body_string("ok"))
        .expect(2)
        .mount(&server)
        .await;
    let dir = tempdir().unwrap();
    let ctx = ExecutionContext {
        base_url: Some(server.uri()),
        cache_config: Some(CacheConfig {
            cache_dir: dir.path().join("cache"),
            ..Default::default()
        }),
        ..Default::default()
    };
    let mut spec = test_spec();
    spec.commands[0].method = "POST".into();
    for _ in 0..2 {
        assert!(matches!(
            execute(&spec, user_by_id_call("123"), ctx.clone())
                .await
                .unwrap(),
            ExecutionResult::Success { .. }
        ));
    }
    assert!(!dir.path().join("cache").exists());
}

/// Keep declared credential locations in the fixture even when injection is unsupported.
fn tenant_security_spec(location: &str) -> CachedSpec {
    use aperture_cli::cache::models::CachedSecurityScheme;
    let mut spec = test_spec();
    spec.commands[0].security_requirements = vec![vec!["tenant".into()]];
    spec.security_schemes.insert(
        "tenant".into(),
        CachedSecurityScheme {
            name: "tenant".into(),
            scheme_type: "apiKey".into(),
            scheme: None,
            location: Some(location.into()),
            parameter_name: Some("X-Tenant-Secret".into()),
            description: None,
            bearer_format: None,
            aperture_secret: None,
        },
    );
    spec
}

#[tokio::test]
async fn unsupported_security_locations_fail_before_network_or_cache() {
    let server = MockServer::start().await;
    let dir = tempdir().unwrap();
    let ctx = ExecutionContext {
        base_url: Some(server.uri()),
        cache_config: Some(CacheConfig {
            cache_dir: dir.path().join("cache"),
            allow_authenticated: true,
            ..Default::default()
        }),
        ..Default::default()
    };
    for location in ["query", "cookie"] {
        let spec = tenant_security_spec(location);
        for account in ["alice", "bob"] {
            let mut call = user_by_id_call("123");
            // Manual credentials must not turn unsupported declared injection into success.
            call.query_params
                .insert("X-Tenant-Secret".into(), account.into());
            call.custom_headers
                .push(format!("Cookie: X-Tenant-Secret={account}"));
            let error = execute(&spec, call, ctx.clone()).await.unwrap_err();
            assert!(error.to_string().contains("apiKey outside headers"));
        }
    }
    assert!(server.received_requests().await.unwrap().is_empty());
    assert!(!dir.path().join("cache").exists());
}

#[tokio::test]
async fn anonymous_alternative_with_query_or_cookie_security_bypasses_cache() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        // No Set-Cookie response: only declared security can explain the cache bypass.
        .respond_with(ResponseTemplate::new(200).set_body_string("public response"))
        .expect(4)
        .mount(&server)
        .await;
    let dir = tempdir().unwrap();
    let ctx = ExecutionContext {
        base_url: Some(server.uri()),
        cache_config: Some(CacheConfig {
            cache_dir: dir.path().join("cache"),
            allow_authenticated: true,
            ..Default::default()
        }),
        ..Default::default()
    };
    for location in ["query", "cookie"] {
        let mut spec = tenant_security_spec(location);
        // Anonymous access is genuinely satisfiable; unsupported injection is never selected.
        spec.commands[0].security_requirements.insert(0, vec![]);
        for _ in 0..2 {
            assert!(matches!(
                execute(&spec, user_by_id_call("123"), ctx.clone())
                    .await
                    .unwrap(),
                ExecutionResult::Success { .. }
            ));
        }
    }
    for request in server.received_requests().await.unwrap() {
        assert!(!request.headers.contains_key("X-Tenant-Secret"));
        assert!(!request.headers.contains_key("Cookie"));
        assert!(request.url.query().is_none());
    }
    assert!(!dir.path().join("cache").exists());
}

#[tokio::test]
async fn custom_api_key_from_secret_mapping_never_reaches_cache_disk() {
    use aperture_cli::cache::models::{CachedApertureSecret, CachedSecurityScheme};
    let server = MockServer::start().await;
    for account in ["synthetic-tenant-alice", "synthetic-tenant-bob"] {
        Mock::given(method("GET"))
            .and(wiremock::matchers::header("X-Tenant-Secret", account))
            .respond_with(ResponseTemplate::new(200).set_body_string("ok"))
            .expect(2)
            .mount(&server)
            .await;
    }
    let dir = tempdir().unwrap();
    let mut spec = test_spec();
    spec.commands[0].security_requirements = vec![vec!["tenant".into()]];
    spec.security_schemes.insert(
        "tenant".into(),
        CachedSecurityScheme {
            name: "tenant".into(),
            scheme_type: "apiKey".into(),
            scheme: None,
            location: Some("header".into()),
            parameter_name: Some("X-Tenant-Secret".into()),
            description: None,
            bearer_format: None,
            aperture_secret: Some(CachedApertureSecret {
                source: "env".into(),
                name: "APERTURE_CACHE_TEST_TENANT_SECRET".into(),
            }),
        },
    );
    let ctx = ExecutionContext {
        base_url: Some(server.uri()),
        cache_config: Some(CacheConfig {
            cache_dir: dir.path().join("cache"),
            ..Default::default()
        }),
        ..Default::default()
    };
    for account in ["synthetic-tenant-alice", "synthetic-tenant-bob"] {
        std::env::set_var("APERTURE_CACHE_TEST_TENANT_SECRET", account);
        for _ in 0..2 {
            assert!(matches!(
                execute(&spec, user_by_id_call("123"), ctx.clone())
                    .await
                    .unwrap(),
                ExecutionResult::Success { .. }
            ));
        }
    }
    std::env::remove_var("APERTURE_CACHE_TEST_TENANT_SECRET");
    assert!(!dir.path().join("cache").exists());
}

#[tokio::test]
async fn session_creating_responses_are_not_cached() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .respond_with(
            ResponseTemplate::new(200)
                .insert_header("set-cookie", "session=synthetic-secret")
                .set_body_string("ok"),
        )
        .expect(2)
        .mount(&server)
        .await;
    let dir = tempdir().unwrap();
    let ctx = ExecutionContext {
        base_url: Some(server.uri()),
        cache_config: Some(CacheConfig {
            cache_dir: dir.path().to_path_buf(),
            ..Default::default()
        }),
        ..Default::default()
    };
    for _ in 0..2 {
        assert!(matches!(
            execute(&test_spec(), user_by_id_call("123"), ctx.clone())
                .await
                .unwrap(),
            ExecutionResult::Success { .. }
        ));
    }
    assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 0);
}
