//! Authenticated diagnostics must not render untrusted reflected bodies.
use aperture_cli::cache::models::CachedSpec;
use aperture_cli::engine::executor::execute;
use aperture_cli::invocation::{ExecutionContext, ExecutionResult, OperationCall, ProxyOverride};
use aperture_cli::spec::{parser::parse_openapi, transformer::SpecTransformer};
use serde_json::json;
use std::collections::HashMap;
use wiremock::{Mock, MockServer, ResponseTemplate};

fn cached(url: &str, scheme: Option<serde_json::Value>) -> CachedSpec {
    let mut doc = json!({
        "openapi": "3.0.3", "info": {"title": "Diagnostics", "version": "1"},
        "servers": [{"url": url}],
        "paths": {"/records": {"get": {"operationId": "getRecords", "tags": ["records"],
            "requestBody": {"content": {"application/json": {"schema": {"type": "object"}}}},
            "responses": {"200": {"description": "OK"}}}}}
    });
    if let Some(scheme) = scheme {
        doc["components"] = json!({"securitySchemes": {"auth": scheme}});
        doc["security"] = json!([{"auth": []}]);
    }
    let spec = parse_openapi(&doc.to_string()).unwrap();
    SpecTransformer::new()
        .transform("diagnostics", &spec)
        .unwrap()
}

async fn invoke(spec: &CachedSpec) -> Result<ExecutionResult, aperture_cli::error::Error> {
    execute(
        spec,
        OperationCall {
            operation_id: "getRecords".into(),
            pagination_url: None,
            path_params: HashMap::default(),
            query_params: HashMap::default(),
            header_params: HashMap::default(),
            body: None,
            custom_headers: vec![],
        },
        ExecutionContext {
            proxy_override: ProxyOverride::Disable,
            ..Default::default()
        },
    )
    .await
}

#[tokio::test]
async fn authenticated_error_bodies_are_not_rendered() {
    let server = MockServer::start().await;
    std::env::set_var("DIAGNOSTICS264_TOKEN", "tiny1");
    let spec = cached(
        &server.uri(),
        Some(json!({"type": "http", "scheme": "bearer",
        "x-aperture-secret": {"source": "env", "name": "DIAGNOSTICS264_TOKEN"}})),
    );
    for body in [
        "tiny1",
        r#"{"token":"\u0074\u0069\u006e\u0079\u0031"}"#,
        "token: tiny1",
        "reversed: 1ynit",
    ] {
        server.reset().await;
        Mock::given(wiremock::matchers::method("GET"))
            .respond_with(ResponseTemplate::new(401).set_body_string(body))
            .mount(&server)
            .await;
        let error = invoke(&spec).await.unwrap_err();
        let rendered = format!("{error:?}");
        assert!(
            !rendered.contains(body),
            "untrusted body retained: {rendered}"
        );
    }
    std::env::remove_var("DIAGNOSTICS264_TOKEN");
}

#[tokio::test]
async fn successful_and_anonymous_bodies_remain_useful() {
    let server = MockServer::start().await;
    let spec = cached(&server.uri(), None);
    Mock::given(wiremock::matchers::method("GET"))
        .respond_with(ResponseTemplate::new(400).set_body_string("invalid record identifier"))
        .mount(&server)
        .await;
    assert!(format!("{:?}", invoke(&spec).await.unwrap_err()).contains("invalid record identifier"));
    server.reset().await;
    Mock::given(wiremock::matchers::method("GET"))
        .respond_with(ResponseTemplate::new(200).set_body_string("requested content"))
        .mount(&server)
        .await;
    assert!(
        matches!(invoke(&spec).await.unwrap(), ExecutionResult::Success { body, .. } if body == "requested content")
    );
}

/// Shared trace sink, so assertions inspect emitted diagnostics rather than prose.
#[derive(Clone)]
struct TraceSink(std::sync::Arc<std::sync::Mutex<Vec<u8>>>);

impl std::io::Write for TraceSink {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        self.0.lock().unwrap().extend_from_slice(bytes);
        Ok(bytes.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

#[test]
fn final_authorization_forms_and_body_logs_are_safe() {
    use aperture_cli::logging::{log_operation_request, log_operation_response, SecretContext};
    let spec = cached("http://127.0.0.1:1", None);
    let operation = &spec.commands[0];
    for (authorization, forms) in [
        ("Bearer tiny1", vec!["tiny1"]),
        (
            "Basic dXNlcjpwYXNz",
            vec!["dXNlcjpwYXNz", "user:pass", "pass"],
        ),
    ] {
        let mut headers = reqwest::header::HeaderMap::new();
        headers.insert("authorization", authorization.parse().unwrap());
        let ctx = SecretContext::empty().with_active_operation_headers(&spec, operation, &headers);
        for form in forms {
            assert!(ctx.is_secret(form));
            assert!(!ctx
                .redact_secrets_in_text(&format!("echo={form}"))
                .contains(form));
        }
        let sink = TraceSink(std::sync::Arc::default());
        let writer = sink.clone();
        let subscriber = tracing_subscriber::fmt()
            .with_max_level(tracing::Level::TRACE)
            .without_time()
            .with_ansi(false)
            .with_writer(move || writer.clone())
            .finish();
        tracing::subscriber::with_default(subscriber, || {
            log_operation_request(
                "POST",
                "http://127.0.0.1:1",
                Some(&headers),
                Some("arbitrary transformed request credential"),
                Some(&ctx),
                &spec,
                operation,
            );
            log_operation_response(
                401,
                1,
                Some(&headers),
                Some("arbitrary transformed response credential"),
                1000,
                Some(&ctx),
                (&spec, operation),
            );
        });
        let logs = String::from_utf8(sink.0.lock().unwrap().clone()).unwrap();
        assert!(logs.contains("401"));
        assert!(!logs.contains(authorization));
        assert!(!logs.contains("transformed"));
        assert!(!logs.contains("Request body:"));
        assert!(!logs.contains("Response body:"));
    }
}

#[tokio::test]
async fn final_override_suppresses_errors_and_dry_run_body_but_preserves_success() {
    let server = MockServer::start().await;
    let spec = cached(&server.uri(), None);
    let call = OperationCall {
        operation_id: "getRecords".into(),
        pagination_url: None,
        path_params: HashMap::default(),
        query_params: HashMap::default(),
        header_params: HashMap::default(),
        body: Some(aperture_cli::invocation::RequestBody::Json(
            r#"{"token":"tiny1"}"#.into(),
        )),
        custom_headers: vec![
            "Authorization: Bearer tiny1".into(),
            "X-Echo: reflected tiny1".into(),
        ],
    };
    let context = ExecutionContext {
        proxy_override: ProxyOverride::Disable,
        ..Default::default()
    };
    let dry = execute(
        &spec,
        call.clone(),
        ExecutionContext {
            dry_run: true,
            ..context.clone()
        },
    )
    .await
    .unwrap();
    assert!(!format!("{dry:?}").contains("tiny1"));
    for status in [400, 401, 403, 429, 500] {
        server.reset().await;
        Mock::given(wiremock::matchers::method("GET"))
            .respond_with(ResponseTemplate::new(status).set_body_string("encoded arbitrary secret"))
            .mount(&server)
            .await;
        let error = execute(&spec, call.clone(), context.clone())
            .await
            .unwrap_err();
        let rendered = format!("{error:?}");
        assert!(!rendered.contains("encoded arbitrary secret"));
        assert!(rendered.contains(&status.to_string()));
        assert!(rendered.contains("getRecords"));
    }
    server.reset().await;
    Mock::given(wiremock::matchers::method("GET"))
        .respond_with(ResponseTemplate::new(200).set_body_string("tiny1"))
        .mount(&server)
        .await;
    assert!(
        matches!(execute(&spec, call, context).await.unwrap(), ExecutionResult::Success {body, ..} if body == "tiny1")
    );
}

#[tokio::test]
async fn implicit_url_and_proxy_credentials_suppress_diagnostic_bodies() {
    let server = MockServer::start().await;
    let mut url = reqwest::Url::parse(&server.uri()).unwrap();
    url.set_username("alice").unwrap();
    url.set_password(Some("transport-secret")).unwrap();
    for (spec, context, status) in [
        (
            cached(url.as_str(), None),
            ExecutionContext {
                proxy_override: ProxyOverride::Disable,
                ..Default::default()
            },
            401,
        ),
        (
            cached("http://example.invalid", None),
            ExecutionContext {
                proxy_override: ProxyOverride::Use(url.to_string()),
                ..Default::default()
            },
            407,
        ),
    ] {
        server.reset().await;
        Mock::given(wiremock::matchers::method("GET"))
            .respond_with(ResponseTemplate::new(status).set_body_string("transport-secret"))
            .mount(&server)
            .await;
        let call = OperationCall {
            operation_id: "getRecords".into(),
            pagination_url: None,
            path_params: HashMap::default(),
            query_params: HashMap::default(),
            header_params: HashMap::default(),
            custom_headers: vec![],
            body: Some(aperture_cli::invocation::RequestBody::Json(
                r#"{"secret":"transport-secret"}"#.into(),
            )),
        };
        let dry = execute(
            &spec,
            call.clone(),
            ExecutionContext {
                dry_run: true,
                ..context.clone()
            },
        )
        .await
        .unwrap();
        assert!(!format!("{dry:?}").contains("transport-secret"));
        let error = execute(&spec, call, context).await.unwrap_err();
        assert!(!format!("{error:?}").contains("transport-secret"));
        assert!(format!("{error:?}").contains(&status.to_string()));
        assert_eq!(server.received_requests().await.unwrap().len(), 1);
    }
}
