use aperture_cli::cache::models::CachedSpec;
use aperture_cli::engine::executor::execute;
use aperture_cli::invocation::{ExecutionContext, ExecutionResult, OperationCall, ProxyOverride};
use aperture_cli::response_cache::{CacheConfig, CacheKey, CachedRequestInfo, ResponseCache};
use std::collections::HashMap;
use wiremock::matchers::method;
use wiremock::{Mock, MockServer, ResponseTemplate};

fn spec(base: &str) -> CachedSpec {
    let document = serde_json::from_value(serde_json::json!({
        "openapi":"3.0.3", "info":{"title":"literal", "version":"1"},
        "servers":[{"url":base}],
        "paths":{"/items":{"get":{"operationId":"items","responses":{"200":{"description":"ok"}}}}}
    }))
    .unwrap();
    aperture_cli::spec::SpecTransformer::new()
        .transform("literal", &document)
        .unwrap()
}

fn call(value: &str, parameter: bool) -> OperationCall {
    let mut call = OperationCall {
        pagination_url: None,
        operation_id: "items".into(),
        path_params: HashMap::new(),
        query_params: HashMap::new(),
        header_params: HashMap::new(),
        body: None,
        custom_headers: Vec::new(),
    };
    if parameter {
        call.header_params.insert("X-Unknown".into(), value.into());
    } else {
        call.custom_headers.push(format!("X-Unknown: {value}"));
    }
    call
}

/// Recreate the old empty projection, not the real byte identity, to test old hits.
async fn seed_legacy_empty_identity(
    spec: &CachedSpec,
    ctx: &ExecutionContext,
    base: &str,
) -> (CacheKey, Vec<u8>) {
    let result = execute(
        spec,
        call("", false),
        ExecutionContext {
            dry_run: true,
            ..ctx.clone()
        },
    )
    .await
    .unwrap();
    let ExecutionResult::DryRun { request_info } = result else {
        panic!("expected preview")
    };
    let headers = serde_json::from_value(request_info["headers"].clone()).unwrap();
    let key = CacheKey::from_request(
        "literal",
        "items:redirects=true:origin-bound=v1:proxy-auth-bypass=v2",
        "GET",
        &format!("{base}/items"),
        &headers,
        None,
    )
    .unwrap();
    let config = ctx.cache_config.clone().unwrap();
    let cache = ResponseCache::new(config.clone()).unwrap();
    cache
        .store(
            &key,
            "old-wrong-account",
            200,
            &HashMap::new(),
            CachedRequestInfo {
                method: "GET".into(),
                url: format!("{base}/items"),
                headers,
                body_hash: None,
            },
            None,
        )
        .await
        .unwrap();
    let before = std::fs::read(config.cache_dir.join(key.to_filename())).unwrap();
    (key, before)
}

fn assert_cache_unchanged(directory: &std::path::Path, key: &CacheKey, seed: &[u8]) {
    assert_eq!(
        std::fs::read(directory.join(key.to_filename())).unwrap(),
        seed
    );
    let files = std::fs::read_dir(directory)
        .unwrap()
        .filter(|entry| {
            entry
                .as_ref()
                .unwrap()
                .path()
                .extension()
                .is_some_and(|ext| ext == "json")
        })
        .count();
    assert_eq!(files, 1, "must not store Unicode results");
}

#[tokio::test]
async fn literal_unicode_headers_bypass_old_cache_and_preserve_wire_bytes() {
    for parameter in [false, true] {
        let server = MockServer::start().await;
        let spec = spec(&server.uri());
        let directory = tempfile::tempdir().unwrap();
        let ctx = ExecutionContext {
            cache_config: Some(CacheConfig {
                cache_dir: directory.path().into(),
                allow_authenticated: true,
                ..Default::default()
            }),
            proxy_override: ProxyOverride::Disable,
            ..Default::default()
        };
        let (key, seed) = seed_legacy_empty_identity(&spec, &ctx, &server.uri()).await;
        for value in ["é", "ø", "é"] {
            Mock::given(method("GET"))
                .respond_with(ResponseTemplate::new(200).set_body_string(value))
                .expect(1)
                .mount(&server)
                .await;
            let result = execute(&spec, call(value, parameter), ctx.clone())
                .await
                .unwrap();
            let ExecutionResult::Success { body, .. } = result else {
                panic!("must not read cache")
            };
            assert_eq!(body, value);
            let requests = server.received_requests().await.unwrap();
            assert_eq!(
                requests[0].headers["x-unknown"].as_bytes(),
                value.as_bytes()
            );
            assert_cache_unchanged(directory.path(), &key, &seed);
            server.verify().await;
            server.reset().await;
        }
        // Empty ASCII headers could also read the legacy Unicode projection.
        Mock::given(method("GET"))
            .respond_with(ResponseTemplate::new(200).set_body_string("empty-account"))
            .expect(1)
            .mount(&server)
            .await;
        let result = execute(&spec, call("", parameter), ctx.clone())
            .await
            .unwrap();
        let ExecutionResult::Success { body, .. } = result else {
            panic!("legacy projection must not be read by an empty literal either")
        };
        assert_eq!(body, "empty-account");
        server.verify().await;
        server.reset().await;
        let mut public = call("é", parameter);
        public.custom_headers.push("x-unknown: public".into());
        Mock::given(method("GET"))
            .respond_with(ResponseTemplate::new(200).set_body_string("public-data"))
            .expect(1)
            .mount(&server)
            .await;
        assert!(matches!(
            execute(&spec, public.clone(), ctx.clone()).await.unwrap(),
            ExecutionResult::Success { .. }
        ));
        assert!(matches!(
            execute(&spec, public, ctx).await.unwrap(),
            ExecutionResult::Cached { .. }
        ));
    }
}
