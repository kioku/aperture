mod test_helpers;

use aperture_cli::cache::models::{
    CachedCommand, CachedParameter, CachedRequestBody, CachedResponse, CachedSpec, PaginationInfo,
    ParameterSerialization,
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
                serialization: ParameterSerialization::default(),
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
            security_scopes: Vec::new(),
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
        pagination_url: None,
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
        pagination_url: None,
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
async fn path_and_query_parameters_are_observed_as_data() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({})))
        .mount(&server)
        .await;
    let mut spec = test_spec();
    spec.base_url = Some(server.uri());
    spec.servers = vec![server.uri()];
    let mut call = user_by_id_call("a/b?#% space雪");
    call.query_params
        .insert("key&=?雪".into(), "value+& #雪".into());
    execute(&spec, call, ExecutionContext::default())
        .await
        .unwrap();
    let requests = server.received_requests().await.unwrap();
    let url = &requests[0].url;
    assert_eq!(url.path(), "/users/a%2Fb%3F%23%25%20space%E9%9B%AA");
    assert_eq!(url.fragment(), None);
    assert_eq!(
        url.query_pairs().collect::<Vec<_>>(),
        vec![("key&=?雪".into(), "value+& #雪".into())]
    );
}

fn accept_before_deadline(listener: &std::net::TcpListener) -> std::net::TcpStream {
    let deadline = std::time::Instant::now() + Duration::from_secs(5);
    loop {
        match listener.accept() {
            Ok((stream, _)) => {
                // macOS inherits the listener's nonblocking flag. Only accept
                // is polled; request reads must wait under their own deadline.
                stream.set_nonblocking(false).unwrap();
                return stream;
            }
            Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                assert!(
                    std::time::Instant::now() < deadline,
                    "missing retry attempt"
                );
                std::thread::sleep(Duration::from_millis(10));
            }
            Err(error) => panic!("accept failed: {error}"),
        }
    }
}

/// Each accepted connection is a fresh attempt: unsuccessful attempts close
/// before sending headers, reproducing a transient transport failure.
fn disconnect_server(attempts: usize, succeed: bool) -> (String, std::thread::JoinHandle<usize>) {
    let mut responses = vec![None; attempts];
    if succeed {
        responses[attempts - 1] =
            Some(&b"HTTP/1.1 200 OK\r\nContent-Length: 2\r\nConnection: close\r\n\r\n{}"[..]);
    }
    response_attempt_server(responses)
}

fn response_attempt_server(
    responses: Vec<Option<&'static [u8]>>,
) -> (String, std::thread::JoinHandle<usize>) {
    use std::io::{Read, Write};
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let address = listener.local_addr().unwrap();
    listener.set_nonblocking(true).unwrap();
    let worker = std::thread::spawn(move || {
        let attempts = responses.len();
        for response in responses {
            let mut stream = accept_before_deadline(&listener);
            stream
                .set_read_timeout(Some(Duration::from_secs(5)))
                .unwrap();
            let mut request = [0; 4096];
            assert!(stream.read(&mut request).unwrap() > 0);
            if let Some(response) = response {
                stream.write_all(response).unwrap();
            }
        }
        attempts
    });
    (format!("http://{address}"), worker)
}

async fn run_disconnect_request(
    attempts: usize,
    succeed: bool,
    method: &str,
) -> Result<ExecutionResult, aperture_cli::error::Error> {
    let (url, worker) = disconnect_server(attempts, succeed);
    let mut spec = test_spec();
    spec.base_url = Some(url.clone());
    spec.servers = vec![url];
    spec.commands[0].method = method.into();
    let ctx = ExecutionContext {
        retry_context: Some(aperture_cli::engine::executor::RetryContext {
            max_attempts: 3,
            initial_delay_ms: 10,
            max_delay_ms: 10,
            ..Default::default()
        }),
        proxy_override: aperture_cli::invocation::ProxyOverride::Disable,
        ..Default::default()
    };
    let start = std::time::Instant::now();
    let result = execute(&spec, user_by_id_call("1"), ctx).await;
    assert!(start.elapsed() >= Duration::from_millis(10 * u64::try_from(attempts - 1).unwrap()));
    assert_eq!(worker.join().unwrap(), attempts);
    result
}

#[tokio::test]
async fn transient_disconnect_retries_then_succeeds() {
    assert!(run_disconnect_request(2, true, "GET").await.is_ok());
}

#[tokio::test]
async fn exhausted_disconnect_retries_report_failure() {
    let error = run_disconnect_request(3, false, "GET").await.unwrap_err();
    assert!(error.to_string().contains('3'), "{error}");
}

#[tokio::test]
async fn unsafe_request_does_not_retry_disconnect() {
    assert!(run_disconnect_request(1, false, "POST").await.is_err());
}

#[tokio::test]
async fn executor_rejects_cross_origin_pagination_override_before_dry_run() {
    let mut call = user_by_id_call("1");
    call.pagination_url = Some(reqwest::Url::parse("https://untrusted.example/items").unwrap());
    let context = ExecutionContext {
        dry_run: true,
        ..Default::default()
    };
    let error = execute(&test_spec(), call, context).await.unwrap_err();
    assert!(error.to_string().contains("same-origin"));
}

#[tokio::test]
async fn declared_matrix_path_serializes_array_items() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .respond_with(ResponseTemplate::new(200).set_body_string("{}"))
        .mount(&server)
        .await;
    let mut spec = test_spec();
    spec.base_url = Some(server.uri());
    spec.servers = vec![server.uri()];
    spec.commands[0].parameters[0].schema_type = Some("array".into());
    spec.commands[0].parameters[0].serialization.style = Some("matrix".into());
    spec.commands[0].parameters[0].serialization.explode = Some(true);
    execute(
        &spec,
        user_by_id_call(r#"["a/b", "c;=?#% 雪"]"#),
        ExecutionContext::default(),
    )
    .await
    .unwrap();
    let requests = server.received_requests().await.unwrap();
    assert_eq!(
        requests[0].url.path(),
        "/users/;id=a%2Fb;id=c%3B%3D%3F%23%25%20%E9%9B%AA"
    );
}

#[tokio::test]
async fn sdk_override_revalidates_userinfo_fragments_and_redirects() {
    let server = MockServer::start().await;
    let other = MockServer::start().await;
    let mut spec = test_spec();
    spec.base_url = Some(server.uri());
    spec.servers = vec![server.uri()];
    for target in [
        format!("{}/next#fragment", server.uri()),
        server.uri().replacen("http://", "http://user:password@", 1),
    ] {
        let mut call = user_by_id_call("1");
        call.pagination_url = Some(reqwest::Url::parse(&target).unwrap());
        assert!(execute(&spec, call, ExecutionContext::default())
            .await
            .is_err());
        assert!(server.received_requests().await.unwrap().is_empty());
    }
    Mock::given(path("/next"))
        .respond_with(
            ResponseTemplate::new(302).insert_header("location", format!("{}/stolen", other.uri())),
        )
        .mount(&server)
        .await;
    let mut call = user_by_id_call("1");
    call.pagination_url = Some(reqwest::Url::parse(&format!("{}/next", server.uri())).unwrap());
    call.custom_headers
        .push("X-Api-Key: synthetic-sdk-key".into());
    assert!(execute(&spec, call, ExecutionContext::default())
        .await
        .is_err());
    assert_eq!(server.received_requests().await.unwrap().len(), 1);
    assert!(other.received_requests().await.unwrap().is_empty());
}

#[tokio::test]
async fn mixed_http_then_transport_exhaustion_reports_final_failure() {
    let (url, worker) = response_attempt_server(vec![
        Some(b"HTTP/1.1 503 Unavailable\r\nContent-Length: 2\r\nConnection: close\r\n\r\n{}"),
        None,
        None,
    ]);
    let mut spec = test_spec();
    spec.base_url = Some(url.clone());
    spec.servers = vec![url];
    let ctx = ExecutionContext {
        retry_context: Some(aperture_cli::engine::executor::RetryContext {
            max_attempts: 3,
            initial_delay_ms: 1,
            max_delay_ms: 1,
            ..Default::default()
        }),
        proxy_override: aperture_cli::invocation::ProxyOverride::Disable,
        ..Default::default()
    };
    let error = execute(&spec, user_by_id_call("1"), ctx).await.unwrap_err();
    assert_eq!(worker.join().unwrap(), 3);
    assert!(error.to_string().contains("Retry"), "{error}");
}

#[tokio::test]
async fn truncated_response_body_retries_then_succeeds() {
    let (url, worker) = response_attempt_server(vec![
        Some(b"HTTP/1.1 200 OK\r\nContent-Length: 20\r\nConnection: close\r\n\r\n{"),
        Some(b"HTTP/1.1 200 OK\r\nContent-Length: 2\r\nConnection: close\r\n\r\n{}"),
    ]);
    let mut spec = test_spec();
    spec.base_url = Some(url.clone());
    spec.servers = vec![url];
    let ctx = ExecutionContext {
        retry_context: Some(aperture_cli::engine::executor::RetryContext {
            max_attempts: 2,
            initial_delay_ms: 10,
            max_delay_ms: 10,
            ..Default::default()
        }),
        proxy_override: aperture_cli::invocation::ProxyOverride::Disable,
        ..Default::default()
    };
    let start = std::time::Instant::now();
    execute(&spec, user_by_id_call("1"), ctx)
        .await
        .expect("truncated response retry should succeed");
    assert!(start.elapsed() >= Duration::from_millis(10));
    assert_eq!(worker.join().unwrap(), 2);
}

#[tokio::test]
async fn unsafe_transport_retries_require_key_or_explicit_force() {
    for (key, force) in [(Some("synthetic-key".to_string()), false), (None, true)] {
        let (url, worker) = disconnect_server(2, true);
        let mut spec = test_spec();
        spec.base_url = Some(url.clone());
        spec.servers = vec![url];
        spec.commands[0].method = "POST".into();
        let ctx = ExecutionContext {
            idempotency_key: key,
            retry_context: Some(aperture_cli::engine::executor::RetryContext {
                max_attempts: 2,
                initial_delay_ms: 1,
                max_delay_ms: 1,
                force_retry: force,
                ..Default::default()
            }),
            proxy_override: aperture_cli::invocation::ProxyOverride::Disable,
            ..Default::default()
        };
        execute(&spec, user_by_id_call("1"), ctx)
            .await
            .expect("explicit unsafe retry should succeed");
        assert_eq!(worker.join().unwrap(), 2);
    }
}

#[tokio::test]
async fn sdk_retry_flag_without_an_actual_key_does_not_authorize_post() {
    let (url, worker) = disconnect_server(1, false);
    let mut spec = test_spec();
    spec.base_url = Some(url.clone());
    spec.servers = vec![url];
    spec.commands[0].method = "POST".into();
    let ctx = ExecutionContext {
        retry_context: Some(aperture_cli::engine::executor::RetryContext {
            max_attempts: 3,
            has_idempotency_key: true,
            ..Default::default()
        }),
        proxy_override: aperture_cli::invocation::ProxyOverride::Disable,
        ..Default::default()
    };
    let error = execute(&spec, user_by_id_call("1"), ctx).await.unwrap_err();
    assert!(!error.to_string().contains("Retry"), "{error}");
    assert_eq!(worker.join().unwrap(), 1);
}

#[tokio::test]
async fn empty_idempotency_keys_fail_before_requests() {
    let server = MockServer::start().await;
    let mut spec = test_spec();
    spec.base_url = Some(server.uri());
    spec.servers = vec![server.uri()];
    for key in ["", "  "] {
        let ctx = ExecutionContext {
            idempotency_key: Some(key.into()),
            ..Default::default()
        };
        assert!(execute(&spec, user_by_id_call("1"), ctx).await.is_err());
    }
    assert!(server.received_requests().await.unwrap().is_empty());
}

#[tokio::test]
async fn retry_backoff_is_observed_between_server_attempts() {
    use std::io::Read;
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let url = format!("http://{}", listener.local_addr().unwrap());
    listener.set_nonblocking(true).unwrap();
    let worker = std::thread::spawn(move || {
        let mut accepted = Vec::new();
        for _ in 0..3 {
            let mut stream = accept_before_deadline(&listener);
            accepted.push(std::time::Instant::now());
            stream
                .set_read_timeout(Some(Duration::from_secs(5)))
                .unwrap();
            let mut request = [0; 4096];
            assert!(stream.read(&mut request).unwrap() > 0);
        }
        accepted
    });
    let mut spec = test_spec();
    spec.base_url = Some(url.clone());
    spec.servers = vec![url];
    let ctx = ExecutionContext {
        retry_context: Some(aperture_cli::engine::executor::RetryContext {
            max_attempts: 3,
            initial_delay_ms: 50,
            max_delay_ms: 100,
            ..Default::default()
        }),
        proxy_override: aperture_cli::invocation::ProxyOverride::Disable,
        ..Default::default()
    };
    assert!(execute(&spec, user_by_id_call("1"), ctx).await.is_err());
    let accepted = worker.join().unwrap();
    assert_eq!(accepted.len(), 3);
    assert!(accepted[1].duration_since(accepted[0]) >= Duration::from_millis(50));
    assert!(accepted[2].duration_since(accepted[1]) >= Duration::from_millis(100));
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
            oauth2_flows: None,
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
            oauth2_flows: None,
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

#[test]
fn accepted_retry_socket_waits_for_request_bytes() {
    use std::io::{Read, Write};
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    listener.set_nonblocking(true).unwrap();
    let mut client = std::net::TcpStream::connect(listener.local_addr().unwrap()).unwrap();
    let writer = std::thread::spawn(move || {
        std::thread::sleep(Duration::from_millis(50));
        client.write_all(b"request").unwrap();
    });
    let mut accepted = accept_before_deadline(&listener);
    accepted
        .set_read_timeout(Some(Duration::from_secs(5)))
        .unwrap();
    let mut bytes = [0; 7];
    accepted.read_exact(&mut bytes).unwrap();
    assert_eq!(&bytes, b"request");
    writer.join().unwrap();
}
