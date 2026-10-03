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
        ExecutionResult::Cached { body } => {
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
            Ok((stream, _)) => return stream,
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
    use std::io::{Read, Write};
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let address = listener.local_addr().unwrap();
    listener.set_nonblocking(true).unwrap();
    let worker = std::thread::spawn(move || {
        for attempt in 0..attempts {
            let mut stream = accept_before_deadline(&listener);
            stream
                .set_read_timeout(Some(Duration::from_secs(5)))
                .unwrap();
            let mut request = [0; 4096];
            // Any request bytes establish that this connection is an attempt.
            assert!(stream.read(&mut request).unwrap() > 0);
            if succeed && attempt + 1 == attempts {
                stream
                    .write_all(
                        b"HTTP/1.1 200 OK\r\nContent-Length: 2\r\nConnection: close\r\n\r\n{}",
                    )
                    .unwrap();
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
