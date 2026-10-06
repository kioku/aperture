mod test_helpers;

use aperture_cli::cache::models::CachedSpec;
use aperture_cli::engine::executor::execute;
use aperture_cli::invocation::{ExecutionContext, ExecutionResult, OperationCall, ProxyOverride};
use aperture_cli::response_cache::{CacheConfig, CacheKey, CachedRequestInfo, ResponseCache};
use std::collections::HashMap;
use wiremock::matchers::{header, method};
use wiremock::{Mock, MockServer, ResponseTemplate};

fn spec(base: &str) -> CachedSpec {
    let openapi = serde_json::from_value(serde_json::json!({
        "openapi": "3.0.3", "info": {"title": "env", "version": "1"},
        "servers": [{"url": base}],
        "paths": {"/items": {"get": {"operationId": "items", "responses": {"200": {"description": "ok"}}}}}
    })).unwrap();
    aperture_cli::spec::SpecTransformer::new()
        .transform("env", &openapi)
        .unwrap()
}

fn call(value: &str) -> OperationCall {
    OperationCall {
        pagination_url: None,
        operation_id: "items".into(),
        path_params: HashMap::new(),
        query_params: HashMap::new(),
        header_params: HashMap::new(),
        body: None,
        custom_headers: vec![format!("X-Unknown: {value}")],
    }
}

/// Seed the literal identity to prove provenance bypasses a byte-identical hit.
async fn seed_literal_cache(
    spec: &CachedSpec,
    ctx: &ExecutionContext,
    config: &CacheConfig,
    directory: &std::path::Path,
    base: &str,
) -> (CacheKey, Vec<u8>) {
    // Seed the exact anonymous/literal request identity. The env invocation has
    // identical wire bytes, but must neither read nor replace this older entry.
    let literal = execute(
        spec,
        call("synthetic-first"),
        ExecutionContext {
            dry_run: true,
            ..ctx.clone()
        },
    )
    .await
    .unwrap();
    let ExecutionResult::DryRun { request_info } = literal else {
        panic!("expected preview")
    };
    let headers: HashMap<String, String> =
        serde_json::from_value(request_info["headers"].clone()).unwrap();
    let url = format!("{base}/items");
    let key = CacheKey::from_request(
        "env",
        "items:redirects=true:origin-bound=v1:proxy-auth-bypass=v2",
        "GET",
        &url,
        &headers,
        None,
    )
    .unwrap();
    let cache = ResponseCache::new(config.clone()).unwrap();
    cache
        .store(
            &key,
            "seeded-public",
            200,
            &HashMap::new(),
            CachedRequestInfo {
                method: "GET".into(),
                url: url.clone(),
                headers: headers.clone(),
                body_hash: None,
            },
            None,
        )
        .await
        .unwrap();
    let before = std::fs::read(directory.join(key.to_filename())).unwrap();
    (key, before)
}

#[tokio::test]
async fn environment_headers_bypass_seeded_cache_and_rotate_with_reused_context() {
    let variable = "APERTURE_265_SDK_ROTATION";
    let server = MockServer::start().await;
    let spec = spec(&server.uri());
    let directory = tempfile::tempdir().unwrap();
    let config = CacheConfig {
        cache_dir: directory.path().into(),
        allow_authenticated: true,
        ..Default::default()
    };
    let ctx = ExecutionContext {
        cache_config: Some(config.clone()),
        proxy_override: ProxyOverride::Disable,
        ..Default::default()
    };
    let (key, before) =
        seed_literal_cache(&spec, &ctx, &config, directory.path(), &server.uri()).await;
    for value in ["synthetic-first", "synthetic-second"] {
        Mock::given(method("GET"))
            .and(header("X-Unknown", value))
            .respond_with(
                ResponseTemplate::new(200)
                    .set_body_string(value)
                    .insert_header("X-Echo", value),
            )
            .expect(1)
            .mount(&server)
            .await;
        std::env::set_var(variable, value);
        let result = execute(&spec, call(&format!("${{{variable}}}")), ctx.clone())
            .await
            .unwrap();
        let ExecutionResult::Success {
            body,
            headers,
            diagnostics_sensitive,
            ..
        } = result
        else {
            panic!("cache must be bypassed")
        };
        assert_eq!(body, value);
        assert_eq!(headers["x-echo"], value);
        assert!(diagnostics_sensitive);
        let preview = execute(
            &spec,
            call(&format!("${{{variable}}}")),
            ExecutionContext {
                dry_run: true,
                ..ctx.clone()
            },
        )
        .await
        .unwrap();
        assert!(!format!("{preview:?}").contains(value));
        let entries: Vec<_> = std::fs::read_dir(directory.path())
            .unwrap()
            .map(|entry| entry.unwrap().path())
            .filter(|path| {
                path.to_string_lossy()
                    .ends_with(aperture_cli::constants::CACHE_FILE_SUFFIX)
            })
            .collect();
        assert_eq!(
            entries.len(),
            1,
            "rotated sensitive responses must not be written"
        );
        assert!(!std::fs::read_to_string(&entries[0])
            .unwrap()
            .contains(value));
        assert_eq!(
            std::fs::read(directory.path().join(key.to_filename())).unwrap(),
            before
        );
    }
    std::env::remove_var(variable);
    assert_eq!(server.received_requests().await.unwrap().len(), 2);
    assert!(matches!(
        execute(&spec, call("synthetic-first"), ctx).await.unwrap(),
        ExecutionResult::Cached {
            diagnostics_sensitive: false,
            ..
        }
    ));
}

#[tokio::test]
async fn invalid_environment_headers_make_zero_deliveries() {
    let server = MockServer::start().await;
    let spec = spec(&server.uri());
    let ctx = ExecutionContext {
        proxy_override: ProxyOverride::Disable,
        ..Default::default()
    };
    for input in [
        "${}",
        "${APERTURE_265_SDK_UNSET}",
        "prefix ${BAD NAME}",
        "${UNCLOSED",
        "${9BAD}",
    ] {
        let error = execute(&spec, call(input), ctx.clone()).await.unwrap_err();
        assert!(!format!("{error:?} {error}").contains(input));
    }
    assert!(server.received_requests().await.unwrap().is_empty());
}

#[tokio::test]
async fn environment_header_unicode_delivery_and_safe_http_errors() {
    let variable = "APERTURE_265_SDK_UNICODE_DELIVERY";
    let server = MockServer::start().await;
    let spec = spec(&server.uri());
    let ctx = ExecutionContext {
        proxy_override: ProxyOverride::Disable,
        ..Default::default()
    };
    std::env::set_var(variable, "synthetic-é");
    Mock::given(method("GET"))
        .respond_with(ResponseTemplate::new(200).set_body_string("synthetic-é"))
        .expect(1)
        .mount(&server)
        .await;
    let result = execute(&spec, call(&format!("${{{variable}}}")), ctx.clone())
        .await
        .unwrap();
    assert!(
        matches!(result, ExecutionResult::Success { body, diagnostics_sensitive: true, .. } if body == "synthetic-é")
    );
    let requests = server.received_requests().await.unwrap();
    assert_eq!(requests.len(), 1);
    assert_eq!(
        requests[0].headers["x-unknown"].as_bytes(),
        "synthetic-é".as_bytes()
    );
    server.verify().await;
    server.reset().await;
    std::env::set_var(variable, "synthetic-error-private");
    Mock::given(method("GET"))
        .and(header("X-Unknown", "synthetic-error-private"))
        .respond_with(
            ResponseTemplate::new(401)
                .set_body_string("arbitrary server transformation of private data"),
        )
        .expect(1)
        .mount(&server)
        .await;
    let error = execute(&spec, call(&format!("${{{variable}}}")), ctx)
        .await
        .unwrap_err();
    std::env::remove_var(variable);
    let diagnostics = format!(
        "{error:?} {error} {}",
        serde_json::to_string(&error.to_json()).unwrap()
    );
    assert!(!diagnostics.contains("arbitrary server transformation"));
    assert!(!diagnostics.contains("synthetic-error-private"));
    assert_eq!(server.received_requests().await.unwrap().len(), 1);
}
