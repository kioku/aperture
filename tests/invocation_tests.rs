use aperture_cli::invocation::{ExecutionContext, ExecutionResult, OperationCall};
use std::collections::HashMap;

#[test]
fn operation_call_preserves_pre_extracted_parameters() {
    let mut path_params = HashMap::new();
    path_params.insert("id".to_string(), "123".to_string());

    let mut query_params = HashMap::new();
    query_params.insert("limit".to_string(), "10".to_string());

    let mut header_params = HashMap::new();
    header_params.insert("x-request-id".to_string(), "req-1".to_string());

    let call = OperationCall {
        operation_id: "getUserById".to_string(),
        path_params,
        query_params,
        header_params,
        body: Some(r#"{"name":"Alice"}"#.to_string()),
        custom_headers: vec!["X-Custom: value".to_string()],
    };

    assert_eq!(call.operation_id, "getUserById");
    assert_eq!(call.path_params.get("id"), Some(&"123".to_string()));
    assert_eq!(call.query_params.get("limit"), Some(&"10".to_string()));
    assert_eq!(
        call.header_params.get("x-request-id"),
        Some(&"req-1".to_string())
    );
    assert_eq!(call.body.as_deref(), Some(r#"{"name":"Alice"}"#));
    assert_eq!(call.custom_headers, vec!["X-Custom: value".to_string()]);
}

#[test]
fn execution_context_default_has_execution_only_concerns() {
    let ctx = ExecutionContext::default();

    assert!(!ctx.dry_run);
    assert!(ctx.idempotency_key.is_none());
    assert!(ctx.cache_config.is_none());
    assert!(ctx.retry_context.is_none());
    assert!(ctx.base_url.is_none());
    assert!(ctx.global_config.is_none());
    assert!(ctx.server_var_args.is_empty());
}

#[test]
fn execution_result_success_preserves_status_and_headers() {
    let mut headers = HashMap::new();
    headers.insert("content-type".to_string(), "application/json".to_string());

    let result = ExecutionResult::Success {
        body: "{\"ok\":true}".to_string(),
        status: 201,
        headers,
    };

    match result {
        ExecutionResult::Success {
            body,
            status,
            headers,
        } => {
            assert_eq!(status, 201);
            assert_eq!(body, "{\"ok\":true}");
            assert_eq!(
                headers.get("content-type"),
                Some(&"application/json".to_string())
            );
        }
        _ => panic!("Expected Success variant"),
    }
}
