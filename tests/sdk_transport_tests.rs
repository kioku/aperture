//! No `test_helpers` module or TLS constructor: exercise the public SDK in a fresh process.
use aperture_cli::cache::models::{CachedSpec, PaginationInfo, PaginationStrategy};
use aperture_cli::engine::executor::execute;
use aperture_cli::invocation::{ExecutionContext, ExecutionResult, OperationCall, ProxyOverride};
use std::collections::HashMap;
use std::io::{Read, Write};
use std::net::TcpListener;
use std::sync::{
    atomic::{AtomicUsize, Ordering},
    Arc,
};

fn spec(base: &str) -> CachedSpec {
    serde_json::from_value(serde_json::json!({
        "cache_format_version": aperture_cli::cache::models::CACHE_FORMAT_VERSION,
        "name": "sdk", "version": "1", "base_url": base, "servers": [base],
        "security_schemes": {}, "skipped_endpoints": [], "server_variables": {},
        "commands": [{ "name": "items", "operation_id": "listItems", "method": "GET", "path": "/items",
            "parameters": [], "responses": [], "security_requirements": [], "tags": [], "deprecated": false,
            "examples": [], "aliases": [], "hidden": false, "pagination": {"strategy": "none"} }]
    })).unwrap()
}

fn call() -> OperationCall {
    OperationCall {
        operation_id: "listItems".into(),
        path_params: HashMap::new(),
        query_params: HashMap::new(),
        header_params: HashMap::new(),
        body: None,
        custom_headers: vec![],
    }
}

/// Local HTTP/1.1 keep-alive server counts accepted sockets, not requests.
fn keep_alive_server() -> (String, Arc<AtomicUsize>, std::thread::JoinHandle<()>) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let base = format!("http://{}", listener.local_addr().unwrap());
    let count = Arc::new(AtomicUsize::new(0));
    let accepts = count.clone();
    let server = std::thread::spawn(move || {
        for stream in listener.incoming() {
            let stream = stream.unwrap();
            accepts.fetch_add(1, Ordering::SeqCst);
            std::thread::spawn(move || serve_connection(stream));
        }
    });
    (base, count, server)
}

fn read_request(stream: &mut std::net::TcpStream) -> Option<String> {
    let mut request = Vec::new();
    let mut byte = [0];
    while !request.ends_with(b"\r\n\r\n") {
        if stream.read(&mut byte).unwrap_or(0) == 0 {
            return None;
        }
        request.push(byte[0]);
    }
    Some(String::from_utf8(request).unwrap())
}

fn serve_connection(mut stream: std::net::TcpStream) {
    stream
        .set_read_timeout(Some(std::time::Duration::from_secs(10)))
        .unwrap();
    while let Some(request) = read_request(&mut stream) {
        let link = if request.starts_with("GET /items HTTP") {
            "Link: </items?page=2>; rel=\"next\"\r\n"
        } else {
            ""
        };
        let response = format!("HTTP/1.1 200 OK\r\nContent-Length: 10\r\nContent-Type: application/json\r\n{link}\r\n[{{\"id\":1}}]");
        if stream.write_all(response.as_bytes()).is_err() {
            return;
        }
    }
}

#[test]
fn standalone_sdk_dry_run_and_network() {
    let (base, count, _server) = keep_alive_server();
    let output = std::process::Command::new(std::env::current_exe().unwrap())
        .args(["--exact", "sdk_child", "--ignored", "--nocapture"])
        .env("APERTURE_SDK_TEST_BASE", base)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        count.load(Ordering::SeqCst),
        1,
        "SDK pagination must reuse one TCP connection"
    );
}

#[test]
#[ignore = "launched in a fresh process by standalone_sdk_dry_run_and_network"]
fn sdk_child() {
    assert!(rustls::crypto::CryptoProvider::get_default().is_none());
    let base = std::env::var("APERTURE_SDK_TEST_BASE").unwrap();
    let runtime = tokio::runtime::Runtime::new().unwrap();
    runtime.block_on(async {
        let mut spec = spec(&base);
        let mut ctx = ExecutionContext {
            dry_run: true,
            proxy_override: ProxyOverride::Disable,
            ..Default::default()
        };
        assert!(matches!(
            execute(&spec, call(), ctx.clone()).await.unwrap(),
            ExecutionResult::DryRun { .. }
        ));
        assert!(
            rustls::crypto::CryptoProvider::get_default().is_none(),
            "dry run must not initialize TLS or build a client"
        );
        ctx.dry_run = false;
        spec.commands[0].pagination = PaginationInfo {
            strategy: PaginationStrategy::LinkHeader,
            ..Default::default()
        };
        let mut output = Vec::new();
        assert_eq!(
            aperture_cli::pagination::execute_paginated(&spec, call(), ctx, &mut output)
                .await
                .unwrap(),
            2
        );
        assert!(rustls::crypto::CryptoProvider::get_default().is_some());
    });
}

#[tokio::test]
async fn sequential_batch_reuses_one_connection() {
    use aperture_cli::batch::{BatchConfig, BatchFile, BatchOperation, BatchProcessor};
    let (base, accepts, _server) = keep_alive_server();
    let processor = BatchProcessor::new_with_proxy_override(
        BatchConfig {
            max_concurrency: 1,
            show_progress: false,
            suppress_output: true,
            ..Default::default()
        },
        ProxyOverride::Disable,
    );
    let operations = (0..3)
        .map(|_| BatchOperation {
            args: vec!["items".into(), "list-items".into()],
            ..Default::default()
        })
        .collect();
    let result = processor
        .execute_batch(
            &spec(&base),
            BatchFile {
                metadata: None,
                operations,
            },
            None,
            None,
            false,
            &aperture_cli::cli::OutputFormat::Json,
            None,
        )
        .await
        .unwrap();
    assert_eq!(result.success_count, 3, "{:?}", result.results);
    assert_eq!(accepts.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn cloned_context_honors_changed_proxy_route() {
    let (base, direct_accepts, _direct) = keep_alive_server();
    let (proxy, proxy_accepts, _proxy) = keep_alive_server();
    let mut ctx = ExecutionContext {
        proxy_override: ProxyOverride::Disable,
        ..Default::default()
    };
    execute(&spec(&base), call(), ctx.clone()).await.unwrap();
    ctx.proxy_override = ProxyOverride::Use(proxy);
    execute(&spec(&base), call(), ctx).await.unwrap();
    assert_eq!(direct_accepts.load(Ordering::SeqCst), 1);
    assert_eq!(proxy_accepts.load(Ordering::SeqCst), 1);
}
