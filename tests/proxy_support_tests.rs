#![cfg(feature = "integration")]

mod test_helpers;

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
        Some("http://proxy.example:8080/")
    );
}
