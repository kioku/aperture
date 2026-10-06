#![cfg(feature = "integration")]

mod test_helpers;

use aperture_cli::batch::{BatchConfig, BatchFile, BatchOperation, BatchProcessor};
use aperture_cli::cache::models::{CachedSpec, CACHE_FORMAT_VERSION};
use aperture_cli::config::models::{GlobalConfig, ProxyConfig};
use aperture_cli::engine::executor::execute;
use aperture_cli::invocation::{ExecutionContext, ExecutionResult, OperationCall, ProxyOverride};
use base64::{engine::general_purpose, Engine as _};
use std::collections::HashMap;
use std::env;
use std::sync::OnceLock;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;
use tokio::sync::Mutex;
use tokio::task::JoinHandle;
use tokio::time::{timeout, Duration};
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

const PROXY_ENV_VARS: &[&str] = &[
    "HTTP_PROXY",
    "http_proxy",
    "HTTPS_PROXY",
    "https_proxy",
    "ALL_PROXY",
    "all_proxy",
    "NO_PROXY",
    "no_proxy",
    "APERTURE_PROXY_TEST_PASSWORD",
];

static ENV_LOCK: OnceLock<Mutex<()>> = OnceLock::new();

struct EnvGuard {
    saved: Vec<(&'static str, Option<String>)>,
}

impl EnvGuard {
    fn clear_proxy_env() -> Self {
        let saved = PROXY_ENV_VARS
            .iter()
            .map(|name| (*name, env::var(name).ok()))
            .collect();
        for name in PROXY_ENV_VARS {
            env::remove_var(name);
        }
        Self { saved }
    }
}

impl Drop for EnvGuard {
    fn drop(&mut self) {
        for (name, value) in &self.saved {
            if let Some(value) = value {
                env::set_var(name, value);
            } else {
                env::remove_var(name);
            }
        }
    }
}

#[derive(Debug)]
struct CapturedProxyRequest {
    request_line: String,
    headers: HashMap<String, String>,
}

async fn spawn_proxy(
    status_line: &'static str,
) -> (String, JoinHandle<Option<CapturedProxyRequest>>) {
    let listener = TcpListener::bind("127.0.0.1:0")
        .await
        .expect("proxy listener should bind");
    let addr = listener.local_addr().expect("proxy address should exist");
    let handle = tokio::spawn(async move {
        let Ok(Ok((mut socket, _))) = timeout(Duration::from_secs(2), listener.accept()).await
        else {
            return None;
        };

        let mut buffer = vec![0_u8; 8192];
        let bytes_read = socket
            .read(&mut buffer)
            .await
            .expect("proxy should read request");
        let request = String::from_utf8_lossy(&buffer[..bytes_read]);
        let captured = parse_proxy_request(&request);
        let response = format!(
            "{status_line}\r\nContent-Type: application/json\r\nContent-Length: 11\r\n\r\n{{\"ok\":true}}"
        );
        let _ = socket.write_all(response.as_bytes()).await;
        captured
    });
    (format!("http://{addr}"), handle)
}

fn parse_proxy_request(raw: &str) -> Option<CapturedProxyRequest> {
    let mut lines = raw.lines();
    let request_line = lines.next()?.trim_end_matches('\r').to_string();
    let headers = lines
        .take_while(|line| !line.trim().is_empty())
        .filter_map(|line| {
            let (name, value) = line.trim_end_matches('\r').split_once(':')?;
            Some((name.to_ascii_lowercase(), value.trim().to_string()))
        })
        .collect();
    Some(CapturedProxyRequest {
        request_line,
        headers,
    })
}

fn test_spec(base_url: &str) -> CachedSpec {
    CachedSpec {
        cache_format_version: CACHE_FORMAT_VERSION,
        name: "proxy-test".to_string(),
        version: "1.0.0".to_string(),
        commands: vec![test_helpers::test_command(
            "resource",
            "getResource",
            "GET",
            "/resource",
        )],
        base_url: Some(base_url.to_string()),
        servers: vec![base_url.to_string()],
        security_schemes: HashMap::new(),
        skipped_endpoints: vec![],
        server_variables: HashMap::new(),
    }
}

fn test_call() -> OperationCall {
    OperationCall {
        pagination_url: None,
        operation_id: "getResource".to_string(),
        path_params: HashMap::new(),
        query_params: HashMap::new(),
        header_params: HashMap::new(),
        body: None,
        custom_headers: vec![],
    }
}

fn context_with_config(config: GlobalConfig) -> ExecutionContext {
    ExecutionContext {
        global_config: Some(config),
        ..ExecutionContext::default()
    }
}

async fn execute_ok(spec: CachedSpec, ctx: ExecutionContext) {
    let result = execute(&spec, test_call(), ctx)
        .await
        .expect("request should succeed");
    assert!(matches!(
        result,
        ExecutionResult::Success { status: 200, .. }
    ));
}

#[tokio::test(flavor = "current_thread")]
async fn http_proxy_env_routes_http_requests() {
    let _lock = ENV_LOCK.get_or_init(|| Mutex::new(())).lock().await;
    let _env = EnvGuard::clear_proxy_env();
    let (proxy_url, proxy_handle) = spawn_proxy("HTTP/1.1 200 OK").await;
    env::set_var("HTTP_PROXY", &proxy_url);

    execute_ok(
        test_spec("http://example.test"),
        ExecutionContext::default(),
    )
    .await;

    let captured = proxy_handle
        .await
        .unwrap()
        .expect("proxy should receive request");
    assert_eq!(
        captured.request_line,
        "GET http://example.test/resource HTTP/1.1"
    );
}

#[tokio::test(flavor = "current_thread")]
async fn https_proxy_env_routes_https_connect_requests() {
    let _lock = ENV_LOCK.get_or_init(|| Mutex::new(())).lock().await;
    let _env = EnvGuard::clear_proxy_env();
    let (proxy_url, proxy_handle) = spawn_proxy("HTTP/1.1 502 Bad Gateway").await;
    env::set_var("HTTPS_PROXY", &proxy_url);

    let result = execute(
        &test_spec("https://example.test"),
        test_call(),
        ExecutionContext::default(),
    )
    .await;
    assert!(
        result.is_err(),
        "CONNECT failure should surface as request error"
    );

    let captured = proxy_handle
        .await
        .unwrap()
        .expect("proxy should receive CONNECT");
    assert_eq!(captured.request_line, "CONNECT example.test:443 HTTP/1.1");
}

#[tokio::test(flavor = "current_thread")]
async fn no_proxy_env_bypasses_proxy_for_matching_hosts() {
    let _lock = ENV_LOCK.get_or_init(|| Mutex::new(())).lock().await;
    let _env = EnvGuard::clear_proxy_env();
    let target = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/resource"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({ "ok": true })))
        .expect(1)
        .mount(&target)
        .await;
    let (proxy_url, proxy_handle) = spawn_proxy("HTTP/1.1 200 OK").await;
    env::set_var("HTTP_PROXY", &proxy_url);
    env::set_var("NO_PROXY", "127.0.0.1,localhost");

    execute_ok(test_spec(&target.uri()), ExecutionContext::default()).await;

    assert!(
        proxy_handle.await.unwrap().is_none(),
        "proxy should not receive request"
    );
}

#[tokio::test(flavor = "current_thread")]
async fn config_proxy_is_used_when_env_proxy_is_absent() {
    let _lock = ENV_LOCK.get_or_init(|| Mutex::new(())).lock().await;
    let _env = EnvGuard::clear_proxy_env();
    let (proxy_url, proxy_handle) = spawn_proxy("HTTP/1.1 200 OK").await;
    let config = GlobalConfig {
        proxy: ProxyConfig {
            http: Some(proxy_url),
            ..ProxyConfig::default()
        },
        ..GlobalConfig::default()
    };

    execute_ok(
        test_spec("http://example.test"),
        context_with_config(config),
    )
    .await;

    let captured = proxy_handle
        .await
        .unwrap()
        .expect("config proxy should receive request");
    assert_eq!(
        captured.request_line,
        "GET http://example.test/resource HTTP/1.1"
    );
}

#[tokio::test(flavor = "current_thread")]
async fn environment_proxy_beats_config_proxy() {
    let _lock = ENV_LOCK.get_or_init(|| Mutex::new(())).lock().await;
    let _env = EnvGuard::clear_proxy_env();
    let (proxy_url, proxy_handle) = spawn_proxy("HTTP/1.1 200 OK").await;
    env::set_var("HTTP_PROXY", &proxy_url);
    let config = GlobalConfig {
        proxy: ProxyConfig {
            http: Some("http://127.0.0.1:9".to_string()),
            ..ProxyConfig::default()
        },
        ..GlobalConfig::default()
    };

    execute_ok(
        test_spec("http://example.test"),
        context_with_config(config),
    )
    .await;

    let captured = proxy_handle
        .await
        .unwrap()
        .expect("environment proxy should receive request");
    assert_eq!(
        captured.request_line,
        "GET http://example.test/resource HTTP/1.1"
    );
}

#[tokio::test(flavor = "current_thread")]
async fn empty_environment_proxy_does_not_shadow_config_proxy() {
    let _lock = ENV_LOCK.get_or_init(|| Mutex::new(())).lock().await;
    let _env = EnvGuard::clear_proxy_env();
    let (proxy_url, proxy_handle) = spawn_proxy("HTTP/1.1 200 OK").await;
    env::set_var("HTTP_PROXY", "   ");
    let config = GlobalConfig {
        proxy: ProxyConfig {
            http: Some(proxy_url),
            ..ProxyConfig::default()
        },
        ..GlobalConfig::default()
    };

    execute_ok(
        test_spec("http://example.test"),
        context_with_config(config),
    )
    .await;

    let captured = proxy_handle
        .await
        .unwrap()
        .expect("config proxy should receive request");
    assert_eq!(
        captured.request_line,
        "GET http://example.test/resource HTTP/1.1"
    );
}

#[tokio::test(flavor = "current_thread")]
async fn config_no_proxy_bypasses_config_proxy_for_matching_hosts() {
    let _lock = ENV_LOCK.get_or_init(|| Mutex::new(())).lock().await;
    let _env = EnvGuard::clear_proxy_env();
    let target = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/resource"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({ "ok": true })))
        .expect(1)
        .mount(&target)
        .await;
    let (proxy_url, proxy_handle) = spawn_proxy("HTTP/1.1 200 OK").await;
    let config = GlobalConfig {
        proxy: ProxyConfig {
            http: Some(proxy_url),
            no_proxy: vec!["127.0.0.1".to_string(), "localhost".to_string()],
            ..ProxyConfig::default()
        },
        ..GlobalConfig::default()
    };

    execute_ok(test_spec(&target.uri()), context_with_config(config)).await;

    assert!(
        proxy_handle.await.unwrap().is_none(),
        "config proxy should be bypassed"
    );
}

#[tokio::test(flavor = "current_thread")]
async fn cli_proxy_override_beats_env_and_config_proxy() {
    let _lock = ENV_LOCK.get_or_init(|| Mutex::new(())).lock().await;
    let _env = EnvGuard::clear_proxy_env();
    let (proxy_url, proxy_handle) = spawn_proxy("HTTP/1.1 200 OK").await;
    env::set_var("HTTP_PROXY", "http://127.0.0.1:9");
    let config = GlobalConfig {
        proxy: ProxyConfig {
            http: Some("http://127.0.0.1:9".to_string()),
            ..ProxyConfig::default()
        },
        ..GlobalConfig::default()
    };
    let ctx = ExecutionContext {
        proxy_override: ProxyOverride::Use(proxy_url),
        global_config: Some(config),
        ..ExecutionContext::default()
    };

    execute_ok(test_spec("http://example.test"), ctx).await;

    let captured = proxy_handle
        .await
        .unwrap()
        .expect("CLI proxy should receive request");
    assert_eq!(
        captured.request_line,
        "GET http://example.test/resource HTTP/1.1"
    );
}

#[tokio::test(flavor = "current_thread")]
async fn no_proxy_flag_disables_env_and_config_proxy() {
    let _lock = ENV_LOCK.get_or_init(|| Mutex::new(())).lock().await;
    let _env = EnvGuard::clear_proxy_env();
    let target = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/resource"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({ "ok": true })))
        .expect(1)
        .mount(&target)
        .await;
    env::set_var("HTTP_PROXY", "http://127.0.0.1:9");
    let config = GlobalConfig {
        proxy: ProxyConfig {
            http: Some("http://127.0.0.1:9".to_string()),
            ..ProxyConfig::default()
        },
        ..GlobalConfig::default()
    };
    let ctx = ExecutionContext {
        proxy_override: ProxyOverride::Disable,
        global_config: Some(config),
        ..ExecutionContext::default()
    };

    execute_ok(test_spec(&target.uri()), ctx).await;
}

#[tokio::test(flavor = "current_thread")]
async fn config_proxy_auth_uses_password_env_without_printing_password() {
    let _lock = ENV_LOCK.get_or_init(|| Mutex::new(())).lock().await;
    let _env = EnvGuard::clear_proxy_env();
    let (proxy_url, proxy_handle) = spawn_proxy("HTTP/1.1 200 OK").await;
    env::set_var("APERTURE_PROXY_TEST_PASSWORD", "secret-password");
    let config = GlobalConfig {
        proxy: ProxyConfig {
            http: Some(proxy_url),
            username: Some("proxy-user".to_string()),
            password_env: Some("APERTURE_PROXY_TEST_PASSWORD".to_string()),
            ..ProxyConfig::default()
        },
        ..GlobalConfig::default()
    };

    execute_ok(
        test_spec("http://example.test"),
        context_with_config(config),
    )
    .await;

    let captured = proxy_handle
        .await
        .unwrap()
        .expect("proxy should receive request");
    let expected = format!(
        "Basic {}",
        general_purpose::STANDARD.encode("proxy-user:secret-password")
    );
    assert_eq!(captured.headers.get("proxy-authorization"), Some(&expected));
}

#[tokio::test(flavor = "current_thread")]
async fn dry_run_proxy_diagnostics_redact_credentials() {
    let _lock = ENV_LOCK.get_or_init(|| Mutex::new(())).lock().await;
    let _env = EnvGuard::clear_proxy_env();
    let config = GlobalConfig {
        proxy: ProxyConfig {
            http: Some("http://user:secret@proxy.example:8080".to_string()),
            ..ProxyConfig::default()
        },
        ..GlobalConfig::default()
    };
    let ctx = ExecutionContext {
        dry_run: true,
        global_config: Some(config),
        ..ExecutionContext::default()
    };

    let result = execute(&test_spec("http://example.test"), test_call(), ctx)
        .await
        .expect("dry run should succeed");
    let ExecutionResult::DryRun { request_info } = result else {
        panic!("expected dry-run result");
    };

    let rendered = serde_json::to_string(&request_info).unwrap();
    assert!(!rendered.contains("secret"));
    assert_eq!(
        request_info["proxy"]["http"].as_str(),
        Some("[PROXY URL OMITTED]")
    );
}

#[tokio::test(flavor = "current_thread")]
async fn batch_no_proxy_override_disables_env_and_config_proxy() {
    let _lock = ENV_LOCK.get_or_init(|| Mutex::new(())).lock().await;
    let _env = EnvGuard::clear_proxy_env();
    let target = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/resource"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({ "ok": true })))
        .expect(1)
        .mount(&target)
        .await;
    env::set_var("HTTP_PROXY", "http://127.0.0.1:9");
    let config = GlobalConfig {
        proxy: ProxyConfig {
            http: Some("http://127.0.0.1:9".to_string()),
            ..ProxyConfig::default()
        },
        ..GlobalConfig::default()
    };
    let batch = BatchFile {
        metadata: None,
        operations: vec![BatchOperation {
            args: vec!["resource".to_string(), "get-resource".to_string()],
            ..BatchOperation::default()
        }],
    };
    let processor = BatchProcessor::new_with_proxy_override(
        BatchConfig {
            show_progress: false,
            suppress_output: true,
            ..BatchConfig::default()
        },
        ProxyOverride::Disable,
    );

    let result = processor
        .execute_batch(
            &test_spec(&target.uri()),
            batch,
            Some(&config),
            None,
            false,
            &aperture_cli::cli::OutputFormat::Json,
            None,
        )
        .await
        .expect("batch request should bypass proxy and succeed");

    assert_eq!(result.success_count, 1);
    assert_eq!(result.failure_count, 0);
}

fn cache_context(directory: &tempfile::TempDir) -> ExecutionContext {
    ExecutionContext {
        cache_config: Some(aperture_cli::response_cache::CacheConfig {
            cache_dir: directory.path().join("responses"),
            allow_authenticated: true,
            ..aperture_cli::response_cache::CacheConfig::default()
        }),
        ..ExecutionContext::default()
    }
}

async fn assert_uncached_twice(spec: &CachedSpec, ctx: ExecutionContext) {
    for _ in 0..2 {
        assert!(matches!(
            execute(spec, test_call(), ctx.clone()).await.unwrap(),
            ExecutionResult::Success { .. }
        ));
    }
}

#[tokio::test(flavor = "current_thread")]
async fn authenticated_proxy_cache_isolation_and_selection() {
    let _lock = ENV_LOCK.get_or_init(|| Mutex::new(())).lock().await;
    let _env = EnvGuard::clear_proxy_env();
    let proxy = MockServer::start().await;
    Mock::given(method("GET"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({"ok":true})))
        .mount(&proxy)
        .await;
    let directory = tempfile::TempDir::new().unwrap();
    let spec = test_spec("http://example.test");
    let mut ctx = cache_context(&directory);
    for user in ["alice", "bob"] {
        let url = proxy
            .uri()
            .replace("http://", &format!("http://{user}:synthetic@"));
        ctx.proxy_override = ProxyOverride::Use(url.clone());
        assert_uncached_twice(&spec, ctx.clone()).await;
        ctx.proxy_override = ProxyOverride::Default;
        env::set_var("HTTP_PROXY", &url);
        assert_uncached_twice(&spec, ctx.clone()).await;
        env::remove_var("HTTP_PROXY");
    }
    ctx.global_config = Some(GlobalConfig {
        proxy: ProxyConfig {
            http: Some(proxy.uri()),
            username: Some("alice".into()),
            password_env: Some("APERTURE_PROXY_TEST_PASSWORD".into()),
            ..ProxyConfig::default()
        },
        ..GlobalConfig::default()
    });
    for password in ["first-synthetic", "rotated-synthetic"] {
        env::set_var("APERTURE_PROXY_TEST_PASSWORD", password);
        assert_uncached_twice(&spec, ctx.clone()).await;
    }
    let requests = proxy.received_requests().await.unwrap();
    assert_eq!(requests.len(), 12);
    assert!(requests
        .iter()
        .all(|request| request.headers.contains_key("proxy-authorization")));
    assert!(!directory.path().join("responses").exists());
    // An anonymous environment proxy takes precedence over configured credentials.
    env::set_var("HTTP_PROXY", proxy.uri());
    assert!(matches!(
        execute(&spec, test_call(), ctx.clone()).await.unwrap(),
        ExecutionResult::Success { .. }
    ));
    assert!(matches!(
        execute(&spec, test_call(), ctx).await.unwrap(),
        ExecutionResult::Cached { .. }
    ));
    assert_eq!(proxy.received_requests().await.unwrap().len(), 13);
    // A warmed anonymous entry must never satisfy an authenticated execution.
    env::set_var(
        "HTTP_PROXY",
        proxy.uri().replace("http://", "http://bob:synthetic@"),
    );
    assert_uncached_twice(&spec, cache_context(&directory)).await;
    assert_eq!(proxy.received_requests().await.unwrap().len(), 15);
}

#[tokio::test(flavor = "current_thread")]
async fn authenticated_proxy_no_proxy_boundary_is_conservative() {
    let _lock = ENV_LOCK.get_or_init(|| Mutex::new(())).lock().await;
    let _env = EnvGuard::clear_proxy_env();
    let origin = MockServer::start().await;
    Mock::given(method("GET"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({"ok":true})))
        .expect(6)
        .mount(&origin)
        .await;
    env::set_var("HTTP_PROXY", "http://alice:synthetic@127.0.0.1:1");
    for bypass in ["127.0.0.1", "*"] {
        env::set_var("NO_PROXY", bypass);
        let directory = tempfile::TempDir::new().unwrap();
        // The locked proxy matcher applies `*` to hostnames, not IP literals.
        let origin_url = if bypass == "*" {
            origin.uri().replace("127.0.0.1", "localhost")
        } else {
            origin.uri()
        };
        let spec = test_spec(&origin_url);
        let mut ctx = cache_context(&directory);
        assert_uncached_twice(&spec, ctx.clone()).await;
        ctx.proxy_override = ProxyOverride::Disable;
        assert!(matches!(
            execute(&spec, test_call(), ctx.clone()).await.unwrap(),
            ExecutionResult::Success { .. }
        ));
        assert!(matches!(
            execute(&spec, test_call(), ctx).await.unwrap(),
            ExecutionResult::Cached { .. }
        ));
    }
}

#[tokio::test(flavor = "current_thread")]
async fn prior_proxy_account_cache_entries_are_not_reused() {
    assert_prior_proxy_entry_is_not_reused("getResource:redirects=true").await;
    assert_prior_proxy_entry_is_not_reused("getResource:redirects=true:proxy-auth-bypass=v1").await;
}

async fn assert_prior_proxy_entry_is_not_reused(operation_identity: &str) {
    use aperture_cli::response_cache::{CacheKey, CachedRequestInfo, ResponseCache};
    let _lock = ENV_LOCK.get_or_init(|| Mutex::new(())).lock().await;
    let _env = EnvGuard::clear_proxy_env();
    let origin = MockServer::start().await;
    Mock::given(method("GET"))
        .respond_with(ResponseTemplate::new(200).set_body_string("anonymous"))
        .expect(1)
        .mount(&origin)
        .await;
    let directory = tempfile::TempDir::new().unwrap();
    let mut ctx = cache_context(&directory);
    ctx.proxy_override = ProxyOverride::Disable;
    let url = format!("{}/resource", origin.uri());
    let headers = HashMap::from([
        (
            "user-agent".into(),
            format!("aperture/{}", env!("CARGO_PKG_VERSION")),
        ),
        ("accept".into(), "application/json".into()),
    ]);
    let key = CacheKey::from_request(
        "proxy-test",
        operation_identity,
        "GET",
        &url,
        &headers,
        None,
    )
    .unwrap();
    let cache = ResponseCache::new(ctx.cache_config.clone().unwrap()).unwrap();
    cache
        .store(
            &key,
            "alice-private",
            200,
            &HashMap::new(),
            CachedRequestInfo {
                method: "GET".into(),
                url,
                headers,
                body_hash: None,
            },
            None,
        )
        .await
        .unwrap();
    let spec = test_spec(&origin.uri());
    let result = execute(&spec, test_call(), ctx.clone()).await.unwrap();
    assert!(matches!(result, ExecutionResult::Success { body, .. } if body == "anonymous"));
    assert!(
        matches!(execute(&spec, test_call(), ctx).await.unwrap(), ExecutionResult::Cached { body, .. } if body == "anonymous")
    );
    assert_eq!(
        cache.get(&key).await.unwrap().unwrap().body,
        "alice-private"
    );
}

fn schemeless_proxy_context(
    directory: &tempfile::TempDir,
    mode: &str,
    authority: String,
) -> ExecutionContext {
    let mut ctx = cache_context(directory);
    match mode {
        "override" => ctx.proxy_override = ProxyOverride::Use(authority),
        "config" => {
            ctx.global_config = Some(GlobalConfig {
                proxy: ProxyConfig {
                    http: Some(authority),
                    ..Default::default()
                },
                ..Default::default()
            });
        }
        name => env::set_var(name, authority),
    }
    ctx
}

/// reqwest accepts proxy authorities without a scheme. They must receive the
/// same account isolation as full URLs, including embedded config credentials.
#[tokio::test(flavor = "current_thread")]
async fn schemeless_authenticated_proxy_accounts_never_cache() {
    let _lock = ENV_LOCK.get_or_init(|| Mutex::new(())).lock().await;
    let _env = EnvGuard::clear_proxy_env();
    let proxy = MockServer::start().await;
    Mock::given(method("GET"))
        .respond_with(|request: &wiremock::Request| {
            let account = request.headers.get("proxy-authorization").unwrap();
            ResponseTemplate::new(200).set_body_string(account.to_str().unwrap())
        })
        .mount(&proxy)
        .await;
    let directory = tempfile::TempDir::new().unwrap();
    let spec = test_spec("http://example.test");
    for mode in [
        "override",
        "config",
        "HTTP_PROXY",
        "http_proxy",
        "ALL_PROXY",
        "all_proxy",
    ] {
        for user in ["alice", "bob"] {
            let authority = proxy
                .uri()
                .replace("http://", &format!("{user}:synthetic@"));
            let ctx = schemeless_proxy_context(&directory, mode, authority);
            for _ in 0..2 {
                let result = execute(&spec, test_call(), ctx.clone()).await.unwrap();
                let ExecutionResult::Success { body, .. } = result else {
                    panic!("authenticated {mode} proxy returned a cached account");
                };
                assert_eq!(
                    body,
                    format!(
                        "Basic {}",
                        general_purpose::STANDARD.encode(format!("{user}:synthetic"))
                    )
                );
            }
            if !matches!(mode, "override" | "config") {
                env::remove_var(mode);
            }
        }
    }
    assert_eq!(proxy.received_requests().await.unwrap().len(), 24);
    assert!(!directory.path().join("responses").exists());
}

/// SDK dry-run must suppress proxy routes before any operation auth context exists.
#[tokio::test(flavor = "current_thread")]
async fn proxy_metadata_sdk_sources_omit_reflected_credentials() {
    let _lock = ENV_LOCK.get_or_init(|| Mutex::new(())).lock().await;
    let _env = EnvGuard::clear_proxy_env();
    let url = "http://u:proxy-fresh-264@127.0.0.1:9/proxy-fresh-264?echo=462-hserf-yxorp#c2VjcmV0";
    for mode in ["cli", "environment", "config", "config-basic"] {
        env::remove_var("HTTP_PROXY");
        let mut ctx = ExecutionContext {
            dry_run: true,
            ..Default::default()
        };
        match mode {
            "cli" => ctx.proxy_override = ProxyOverride::Use(url.into()),
            "environment" => {
                env::set_var("HTTP_PROXY", url);
                env::set_var("NO_PROXY", "proxy-fresh-264,462-hserf-yxorp");
            }
            _ => {
                let mut proxy = ProxyConfig {
                    http: Some(url.into()),
                    no_proxy: vec!["proxy-fresh-264".into()],
                    ..Default::default()
                };
                if mode == "config-basic" {
                    proxy.http =
                        Some("http://127.0.0.1:9/proxy-fresh-264?echo=462-hserf-yxorp".into());
                    proxy.username = Some("u".into());
                    proxy.password_env = Some("APERTURE_PROXY_TEST_PASSWORD".into());
                    env::set_var("APERTURE_PROXY_TEST_PASSWORD", "proxy-fresh-264");
                }
                ctx.global_config = Some(GlobalConfig {
                    proxy,
                    ..Default::default()
                });
            }
        }
        let ExecutionResult::DryRun { request_info } =
            execute(&test_spec("http://example.test"), test_call(), ctx)
                .await
                .unwrap()
        else {
            panic!("expected dry-run");
        };
        let output = request_info.to_string();
        assert!(!output.contains("proxy-fresh-264"), "{mode}: {output}");
        assert!(!output.contains("462-hserf-yxorp"), "{mode}: {output}");
        assert!(!output.contains("c2VjcmV0"), "{mode}: {output}");
        assert_eq!(
            request_info["proxy"]["source"],
            if mode == "config-basic" {
                "config"
            } else {
                mode
            }
        );
        env::remove_var("NO_PROXY");
    }
}

/// A shared pool must deliver each rotated account to the same proxy authority.
#[tokio::test(flavor = "current_thread")]
async fn proxy_metadata_omission_preserves_shared_pool_rotation_and_request_bytes() {
    let _lock = ENV_LOCK.get_or_init(|| Mutex::new(())).lock().await;
    let _env = EnvGuard::clear_proxy_env();
    let proxy = MockServer::start().await;
    Mock::given(method("GET"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({"ok":true})))
        .mount(&proxy)
        .await;
    let spec = test_spec("http://example.test");
    let mut ctx = ExecutionContext::default();
    for mode in ["cli", "environment", "config", "config-basic"] {
        for password in ["first-password", "rotated-password"] {
            env::remove_var("HTTP_PROXY");
            ctx.proxy_override = ProxyOverride::Default;
            ctx.global_config = None;
            let authority = proxy
                .uri()
                .replace("http://", &format!("http://u:{password}@"));
            let url = format!("{authority}/{password}?echo={password}#fragment");
            match mode {
                "cli" => ctx.proxy_override = ProxyOverride::Use(url),
                "environment" => env::set_var("HTTP_PROXY", url),
                "config" => {
                    ctx.global_config = Some(GlobalConfig {
                        proxy: ProxyConfig {
                            http: Some(url),
                            ..Default::default()
                        },
                        ..Default::default()
                    });
                }
                _ => {
                    env::set_var("APERTURE_PROXY_TEST_PASSWORD", password);
                    ctx.global_config = Some(GlobalConfig {
                        proxy: ProxyConfig {
                            http: Some(format!("{}/{password}?echo={password}", proxy.uri())),
                            username: Some("u".into()),
                            password_env: Some("APERTURE_PROXY_TEST_PASSWORD".into()),
                            ..Default::default()
                        },
                        ..Default::default()
                    });
                }
            }
            execute_ok(spec.clone(), ctx.clone()).await;
            let requests = proxy.received_requests().await.unwrap();
            let request = requests.last().unwrap();
            let expected = format!(
                "Basic {}",
                general_purpose::STANDARD.encode(format!("u:{password}"))
            );
            assert_eq!(request.headers["proxy-authorization"], expected);
            assert_eq!(request.url.path(), "/resource");
            assert_eq!(request.headers["host"], "example.test");
        }
    }
    assert_eq!(proxy.received_requests().await.unwrap().len(), 8);
}

#[tokio::test(flavor = "current_thread")]
async fn malformed_proxy_sdk_errors_omit_fallback_tails() {
    let _lock = ENV_LOCK.get_or_init(|| Mutex::new(())).lock().await;
    let _env = EnvGuard::clear_proxy_env();
    for url in [
        "http://u:invalid-proxy-secret-264@[bad/invalid-proxy-secret-264",
        "http://invalid-proxy-secret-264@[bad",
    ] {
        for config in [false, true] {
            let mut ctx = ExecutionContext {
                dry_run: true,
                ..Default::default()
            };
            if config {
                ctx.global_config = Some(GlobalConfig {
                    proxy: ProxyConfig {
                        http: Some(url.into()),
                        ..Default::default()
                    },
                    ..Default::default()
                });
            } else {
                ctx.proxy_override = ProxyOverride::Use(url.into());
            }
            let error = execute(&test_spec("http://example.test"), test_call(), ctx)
                .await
                .unwrap_err();
            assert!(!format!("{error:?} {error}").contains("invalid-proxy-secret-264"));
            assert!(error.to_string().contains("proxy URL (value omitted)"));
        }
    }
}
