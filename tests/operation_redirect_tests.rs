//! Exercise operation redirect delivery, not only policy predicates.
use aperture_cli::engine::executor::{execute, RetryContext};
use aperture_cli::invocation::{ExecutionContext, OperationCall, ProxyOverride};
use aperture_cli::spec::SpecTransformer;
use serde_json::json;
use std::collections::HashMap;
use wiremock::{matchers::path, Mock, MockServer, ResponseTemplate};

#[tokio::test]
async fn changed_port_receives_no_custom_key_and_is_not_retried() {
    std::env::set_var("APERTURE_TEST_263_KEY", "synthetic-declared-key");
    let source = MockServer::start().await;
    let target = MockServer::start().await;
    Mock::given(path("/start"))
        .respond_with(ResponseTemplate::new(307).insert_header(
            "Location",
            format!("{}/secret-target?private=redirect-secret", target.uri()),
        ))
        .mount(&source)
        .await;
    Mock::given(path("/secret-target"))
        .respond_with(ResponseTemplate::new(200))
        .mount(&target)
        .await;
    let spec = SpecTransformer::new()
        .transform(
            "redirect",
            &serde_json::from_value(json!({
                "openapi":"3.0.3", "info":{"title":"test","version":"1"},
                "servers":[{"url":source.uri()}], "security":[{"key":[]}],
        "components":{"securitySchemes":{"key":{"type":"apiKey","in":"header","name":"X-Declared-Key","x-aperture-secret":{"source":"env","name":"APERTURE_TEST_263_KEY"}}}}, "paths":{"/start":{"post":{
                    "operationId":"send", "responses":{"200":{"description":"ok"}}}}}
            }))
            .unwrap(),
        )
        .unwrap();
    let result = execute(
        &spec,
        OperationCall {
            operation_id: "send".into(),
            custom_headers: vec!["X-Service-Key: synthetic-secret-key".into()],
            pagination_url: None,
            path_params: HashMap::new(),
            query_params: HashMap::new(),
            header_params: HashMap::new(),
            body: None,
        },
        ExecutionContext {
            proxy_override: ProxyOverride::Disable,
            idempotency_key: Some("synthetic-idempotency-key".into()),
            retry_context: Some(RetryContext {
                max_attempts: 3,
                initial_delay_ms: 1,
                has_idempotency_key: true,
                ..Default::default()
            }),
            ..Default::default()
        },
    )
    .await;
    assert!(
        target.received_requests().await.unwrap().is_empty(),
        "credential reached changed origin"
    );
    let error = result.unwrap_err();
    for text in [
        error.to_string(),
        format!("{error:?}"),
        serde_json::to_string(&error.to_json()).unwrap(),
    ] {
        assert!(!text.contains("redirect-secret"));
        assert!(!text.contains("synthetic-secret-key"));
        assert!(!text.contains("secret-target"));
    }
    assert_eq!(source.received_requests().await.unwrap().len(), 1);
}

#[tokio::test]
async fn same_origin_absolute_redirect_retains_declared_key() {
    std::env::set_var("APERTURE_TEST_263_SAFE_KEY", "synthetic-declared-key");
    let source = MockServer::start().await;
    Mock::given(path("/start"))
        .respond_with(
            ResponseTemplate::new(302)
                .insert_header("Location", format!("{}/finish", source.uri())),
        )
        .mount(&source)
        .await;
    Mock::given(path("/finish"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"ok":true})))
        .mount(&source)
        .await;
    let spec = SpecTransformer::new().transform("redirect", &serde_json::from_value(json!({
        "openapi":"3.0.3", "info":{"title":"test","version":"1"},
        "servers":[{"url":source.uri()}], "security":[{"key":[]}],
        "components":{"securitySchemes":{"key":{"type":"apiKey","in":"header","name":"X-Service-Key","x-aperture-secret":{"source":"env","name":"APERTURE_TEST_263_SAFE_KEY"}}}},
        "paths":{"/start":{"get":{"operationId":"send", "responses":{"200":{"description":"ok"}}}}}
    })).unwrap()).unwrap();
    execute(
        &spec,
        OperationCall {
            operation_id: "send".into(),
            pagination_url: None,
            path_params: HashMap::new(),
            query_params: HashMap::new(),
            header_params: HashMap::new(),
            body: None,
            custom_headers: vec![],
        },
        ExecutionContext {
            proxy_override: ProxyOverride::Disable,
            ..Default::default()
        },
    )
    .await
    .unwrap();
    let requests = source.received_requests().await.unwrap();
    assert_eq!(requests.len(), 2);
    assert_eq!(
        requests[0].headers.get("x-service-key"),
        requests[1].headers.get("x-service-key")
    );
    assert!(requests[1].headers.contains_key("x-service-key"));
}
