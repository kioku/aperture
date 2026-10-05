//! Ordinary API response buffering security boundaries, with local synthetic data.
use aperture_cli::cache::models::CachedSpec;
use aperture_cli::config::models::GlobalConfig;
use aperture_cli::engine::executor::{execute, RetryContext};
use aperture_cli::invocation::{ExecutionContext, ExecutionResult, OperationCall, ProxyOverride};
use aperture_cli::spec::SpecTransformer;
use serde_json::json;
use std::collections::HashMap;
use wiremock::{matchers::path, Mock, MockServer, ResponseTemplate};

fn spec(url: &str, method: &str, binary: bool) -> CachedSpec {
    let schema = if binary {
        json!({"type":"string","format":"binary"})
    } else {
        json!({"type":"string"})
    };
    let media = if binary {
        "application/octet-stream"
    } else {
        "text/plain"
    };
    SpecTransformer::new().transform("bounded", &serde_json::from_value(json!({
        "openapi":"3.0.3", "info":{"title":"test","version":"1"},
        "servers":[{"url":url}], "paths":{"/data":{method:{
            "operationId":"read", "responses":{"200":{"description":"ok", "content":{media:{"schema":schema}}}}}}}
    })).unwrap()).unwrap()
}

fn call() -> OperationCall {
    OperationCall {
        operation_id: "read".into(),
        pagination_url: None,
        path_params: HashMap::new(),
        query_params: HashMap::new(),
        header_params: HashMap::new(),
        body: None,
        custom_headers: vec![],
    }
}

fn context(limit: Option<u64>) -> ExecutionContext {
    ExecutionContext {
        max_response_bytes: limit,
        proxy_override: ProxyOverride::Disable,
        ..Default::default()
    }
}

async fn server(body: Vec<u8>, status: u16) -> MockServer {
    let server = MockServer::start().await;
    Mock::given(path("/data"))
        .respond_with(ResponseTemplate::new(status).set_body_bytes(body))
        .mount(&server)
        .await;
    server
}

#[tokio::test]
async fn default_rejects_oversized_response() {
    let server = server(vec![b'x'; 64 * 1024 * 1024 + 1], 200).await;
    let result = execute(&spec(&server.uri(), "get", false), call(), context(None)).await;
    assert!(result.is_err(), "oversized ordinary response was accepted");
    // A finite explicit override can permit useful data above the default.
    assert!(execute(
        &spec(&server.uri(), "get", false),
        call(),
        context(Some(64 * 1024 * 1024 + 1))
    )
    .await
    .is_ok());
}

#[tokio::test]
async fn boundaries_cover_text_binary_errors_and_invalid_utf8() {
    for (binary, status) in [(false, 200), (true, 200), (false, 500), (true, 500)] {
        check_boundary(binary, status, 8).await;
        check_boundary(binary, status, 9).await;
    }
}

async fn check_boundary(binary: bool, status: u16, size: usize) {
    let server = server(vec![0xff; size], status).await;
    let result = execute(
        &spec(&server.uri(), "get", binary),
        call(),
        context(Some(8)),
    )
    .await;
    if size == 9 {
        assert_safe_size_error(&result.unwrap_err());
    } else if status == 200 {
        assert!(result.is_ok());
    } else {
        assert!(!result
            .unwrap_err()
            .to_string()
            .contains("max_response_bytes"));
    }
}

fn assert_safe_size_error(error: &aperture_cli::error::Error) {
    for text in [
        error.to_string(),
        format!("{error:?}"),
        serde_json::to_string(&error.to_json()).unwrap(),
    ] {
        assert!(text.contains("max_response_bytes"));
        assert!(!text.contains("private-fragment"));
    }
    assert!(!matches!(error, aperture_cli::error::Error::Network(_)));
}

#[tokio::test]
async fn oversize_mutation_is_never_retried_even_with_idempotency_key() {
    let server = server(b"private-fragment".to_vec(), 503).await;
    let mut ctx = context(Some(8));
    ctx.idempotency_key = Some("synthetic-key".into());
    ctx.retry_context = Some(RetryContext {
        max_attempts: 3,
        initial_delay_ms: 1,
        has_idempotency_key: true,
        ..Default::default()
    });
    assert_safe_size_error(
        &execute(&spec(&server.uri(), "post", false), call(), ctx)
            .await
            .unwrap_err(),
    );
    assert_eq!(server.received_requests().await.unwrap().len(), 1);
}

#[tokio::test]
async fn sdk_precedence_and_invalid_values_before_requests() {
    let server = server(vec![b'x'; 9], 200).await;
    let spec = spec(&server.uri(), "get", false);
    let mut ctx = context(None);
    ctx.global_config = Some(GlobalConfig {
        max_response_bytes: 8,
        ..Default::default()
    });
    assert_safe_size_error(&execute(&spec, call(), ctx.clone()).await.unwrap_err());
    ctx.max_response_bytes = Some(9);
    assert!(execute(&spec, call(), ctx.clone()).await.is_ok());
    let before = server.received_requests().await.unwrap().len();
    for limit in [
        0,
        u64::MAX,
        aperture_cli::response_limit::MAX_RESPONSE_BYTES + 1,
    ] {
        ctx.max_response_bytes = Some(limit);
        assert!(execute(&spec, call(), ctx.clone()).await.is_err());
        ctx.max_response_bytes = None;
        ctx.global_config.as_mut().unwrap().max_response_bytes = limit;
        assert!(execute(&spec, call(), ctx.clone()).await.is_err());
    }
    assert_eq!(server.received_requests().await.unwrap().len(), before);
}

#[tokio::test]
async fn pagination_and_shared_pool_keep_per_invocation_limits() {
    let server = server(vec![b'x'; 9], 200).await;
    let spec = spec(&server.uri(), "get", false);
    let ctx = context(Some(9));
    assert!(execute(&spec, call(), ctx.clone()).await.is_ok());
    let mut smaller = ctx;
    smaller.max_response_bytes = Some(8);
    smaller.auto_paginate = true;
    let mut page = call();
    page.pagination_url = Some(format!("{}/data", server.uri()).parse().unwrap());
    assert_safe_size_error(&execute(&spec, page, smaller).await.unwrap_err());
}

#[tokio::test]
async fn modest_original_scenario_still_succeeds_under_default() {
    let server = server(vec![b'A'; 16 * 1024 * 1024], 200).await;
    let result = execute(&spec(&server.uri(), "get", false), call(), context(None))
        .await
        .unwrap();
    let ExecutionResult::Success { body, .. } = result else {
        panic!("expected success")
    };
    assert_eq!(body.len(), 16 * 1024 * 1024);
}

/// A bounded raw fixture exercises framing wiremock normalizes away. The worker
/// has bounded socket I/O and is joined even when the client rejects early.
async fn raw_response(
    framing: &str,
    body: &[u8],
    limit: u64,
) -> Result<ExecutionResult, aperture_cli::error::Error> {
    use std::io::{Read, Write};
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let url = format!("http://{}", listener.local_addr().unwrap());
    let response = format!("HTTP/1.1 200 OK\r\nConnection: close\r\n{framing}\r\n").into_bytes();
    let body = body.to_vec();
    let worker = std::thread::spawn(move || {
        let (mut stream, _) = listener.accept().unwrap();
        stream
            .set_read_timeout(Some(std::time::Duration::from_secs(5)))
            .unwrap();
        stream
            .set_write_timeout(Some(std::time::Duration::from_secs(5)))
            .unwrap();
        let mut request = [0; 4096];
        let _ = stream.read(&mut request);
        let _ = stream.write_all(&response);
        let _ = stream.write_all(&body);
    });
    let result = execute(&spec(&url, "get", false), call(), context(Some(limit))).await;
    worker.join().unwrap();
    result
}

#[tokio::test]
async fn missing_chunked_and_misleading_lengths_are_bounded() {
    for body in [b"12345678".as_slice(), b"123456789".as_slice()] {
        let result = raw_response("", body, 8).await;
        assert_eq!(result.is_ok(), body.len() == 8);
        let chunk = format!(
            "{:x}\r\n{}\r\n0\r\n\r\n",
            body.len(),
            String::from_utf8_lossy(body)
        );
        let result = raw_response("Transfer-Encoding: chunked\r\n", chunk.as_bytes(), 8).await;
        assert_eq!(result.is_ok(), body.len() == 8);
    }
    // Chunked framing wins over misleading Content-Length; actual chunks enforce the bound.
    assert_safe_size_error(
        &raw_response(
            "Transfer-Encoding: chunked\r\nContent-Length: 1\r\n",
            b"9\r\n123456789\r\n0\r\n\r\n",
            8,
        )
        .await
        .unwrap_err(),
    );
    // A falsely large advertised length is rejected before waiting for the body.
    assert_safe_size_error(
        &raw_response("Content-Length: 999\r\n", b"x", 8)
            .await
            .unwrap_err(),
    );
}

#[tokio::test]
async fn batch_uses_global_per_response_limit() {
    use aperture_cli::batch::{BatchConfig, BatchFile, BatchOperation, BatchProcessor};
    let server = server(b"private-fragment".to_vec(), 200).await;
    let processor = BatchProcessor::new_with_proxy_override(
        BatchConfig {
            show_progress: false,
            suppress_output: true,
            ..Default::default()
        },
        ProxyOverride::Disable,
    );
    let batch = BatchFile {
        metadata: None,
        operations: vec![
            BatchOperation {
                args: vec!["default".into(), "read".into()],
                ..Default::default()
            };
            2
        ],
    };
    let config = GlobalConfig {
        max_response_bytes: 8,
        ..Default::default()
    };
    let result = processor
        .execute_batch(
            &spec(&server.uri(), "get", false),
            batch,
            Some(&config),
            None,
            false,
            &aperture_cli::cli::OutputFormat::Json,
            None,
        )
        .await
        .unwrap();
    assert_eq!(result.failure_count, 2);
    for entry in result.results {
        let error = entry.error.unwrap();
        assert!(error.contains("max_response_bytes"), "{error}");
        assert!(!error.contains("private-fragment"));
    }
    assert_eq!(server.received_requests().await.unwrap().len(), 2);
}

#[test]
fn config_and_cli_reject_invalid_limit_values() {
    use aperture_cli::config::settings::{SettingKey, SettingValue};
    use clap::Parser;
    assert_eq!(GlobalConfig::default().max_response_bytes, 64 * 1024 * 1024);
    assert_eq!(
        serde_json::from_str::<GlobalConfig>("{}")
            .unwrap()
            .max_response_bytes,
        64 * 1024 * 1024
    );
    assert_eq!(
        "max_response_bytes".parse::<SettingKey>().unwrap(),
        SettingKey::MaxResponseBytes
    );
    for value in [
        "0",
        "-1",
        "none",
        "unlimited",
        "1.5",
        "18446744073709551616",
        "18446744073709551615",
        "",
    ] {
        assert!(SettingValue::parse_for_key(SettingKey::MaxResponseBytes, value).is_err());
        assert!(aperture_cli::cli::Cli::try_parse_from([
            "aperture",
            "api",
            "test",
            "--max-response-bytes",
            value
        ])
        .is_err());
    }
    assert!(SettingValue::parse_for_key(SettingKey::MaxResponseBytes, "1").is_ok());
    assert!(aperture_cli::cli::Cli::try_parse_from([
        "aperture",
        "api",
        "test",
        "--max-response-bytes",
        "8"
    ])
    .is_ok());
    for value in [json!(0), json!(-1), json!("none"), json!(1.5)] {
        // Typed decoding rejects non-integers; executor rejects directly decoded zero.
        let decoded = serde_json::from_value::<GlobalConfig>(json!({"max_response_bytes":value}));
        assert!(
            decoded.is_err()
                || aperture_cli::response_limit::validate(decoded.unwrap().max_response_bytes)
                    .is_err()
        );
    }
    assert!(aperture_cli::response_limit::validate(
        aperture_cli::response_limit::MAX_RESPONSE_BYTES
    )
    .is_ok());
}

#[tokio::test]
async fn cache_envelope_and_decoded_body_are_bounded_and_inspection_is_safe() {
    use aperture_cli::response_cache::{CacheConfig, CacheKey, CachedRequestInfo, ResponseCache};
    let dir = tempfile::tempdir().unwrap();
    let cache = ResponseCache::new(CacheConfig {
        cache_dir: dir.path().into(),
        ..Default::default()
    })
    .unwrap();
    let key = CacheKey::from_request(
        "bounded",
        "read",
        "GET",
        "http://local/data",
        &HashMap::new(),
        None,
    )
    .unwrap();
    let info = CachedRequestInfo {
        method: "GET".into(),
        url: "http://local/data".into(),
        headers: HashMap::new(),
        body_hash: None,
    };
    // Six-byte JSON escapes exercise the envelope allowance at the exact body boundary.
    cache
        .store(
            &key,
            "\0\0\0\0\0\0\0\0",
            200,
            &HashMap::new(),
            info.clone(),
            None,
        )
        .await
        .unwrap();
    assert_eq!(
        cache
            .get_with_limit(&key, 8)
            .await
            .unwrap()
            .unwrap()
            .body
            .len(),
        8
    );
    cache
        .store(&key, "123456789", 200, &HashMap::new(), info, None)
        .await
        .unwrap();
    assert!(cache.get_with_limit(&key, 8).await.unwrap().is_none());
    assert!(cache.get_with_limit(&key, 9).await.unwrap().is_some());
    assert!(cache.get_with_limit(&key, 0).await.is_err());
    assert!(cache.get_with_limit(&key, u64::MAX).await.is_err());
    // A sparse legacy file proves inspection never performs an unbounded file read.
    let file = std::fs::File::create(dir.path().join(key.to_filename())).unwrap();
    file.set_len(1024 * 1024 * 1024).unwrap();
    assert!(cache.get_with_limit(&key, 8).await.unwrap().is_none());
    assert!(cache.get(&key).await.unwrap().is_none());
    assert!(!cache.is_cached(&key).await.unwrap());
    let stats = cache.get_stats(None).await.unwrap();
    assert_eq!(stats.valid_entries, 0);
}

#[tokio::test]
async fn cached_success_cannot_bypass_a_smaller_invocation_limit() {
    use aperture_cli::response_cache::CacheConfig;
    let server = server(b"123456789".to_vec(), 200).await;
    let spec = spec(&server.uri(), "get", false);
    let dir = tempfile::tempdir().unwrap();
    let mut ctx = context(Some(9));
    ctx.cache_config = Some(CacheConfig {
        cache_dir: dir.path().into(),
        ..Default::default()
    });
    assert!(execute(&spec, call(), ctx.clone()).await.is_ok());
    assert!(matches!(
        execute(&spec, call(), ctx.clone()).await.unwrap(),
        ExecutionResult::Cached { .. }
    ));
    ctx.max_response_bytes = Some(8);
    assert_safe_size_error(&execute(&spec, call(), ctx).await.unwrap_err());
    assert_eq!(server.received_requests().await.unwrap().len(), 2);
}
