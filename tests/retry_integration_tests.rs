mod test_helpers;

use aperture_cli::cache::models::{CachedCommand, CachedParameter, CachedSpec};
use aperture_cli::cli::OutputFormat;
use aperture_cli::engine::executor::{execute_request, RetryContext};
use clap::{Arg, Command};
use std::collections::HashMap;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, Respond, ResponseTemplate};

// Helper macros for creating test data
macro_rules! cached_parameter {
    ($name:expr, $location:expr, $required:expr) => {
        CachedParameter {
            name: $name.to_string(),
            location: $location.to_string(),
            required: $required,
            description: None,
            schema: Some(r#"{"type": "string"}"#.to_string()),
            schema_type: Some("string".to_string()),
            format: None,
            default_value: None,
            enum_values: vec![],
            example: None,
        }
    };
}

macro_rules! cached_command {
    ($name:expr, $op_id:expr, $method:expr, $path:expr, $params:expr) => {
        CachedCommand {
            name: $name.to_string(),
            description: None,
            summary: None,
            operation_id: $op_id.to_string(),
            method: $method.to_string(),
            path: $path.to_string(),
            parameters: $params,
            request_body: None,
            responses: vec![],
            security_requirements: vec![],
            tags: vec![$name.to_string()],
            deprecated: false,
            external_docs_url: None,
            examples: vec![],
            display_group: None,
            display_name: None,
            aliases: vec![],
            hidden: false,
        }
    };
}

fn create_test_spec() -> CachedSpec {
    CachedSpec {
        cache_format_version: aperture_cli::cache::models::CACHE_FORMAT_VERSION,
        name: "test-api".to_string(),
        version: "1.0.0".to_string(),
        commands: vec![{
            let mut cmd = cached_command!(
                "users",
                "getUserById",
                "GET",
                "/users/{id}",
                vec![cached_parameter!("id", "path", true)]
            );
            cmd.description = Some("Get user by ID".to_string());
            cmd
        }],
        base_url: Some("https://api.example.com".to_string()),
        servers: vec!["https://api.example.com".to_string()],
        security_schemes: HashMap::new(),
        skipped_endpoints: vec![],
        server_variables: HashMap::new(),
    }
}

/// A responder that fails N times with a given status code, then succeeds
struct FailThenSucceed {
    fail_count: usize,
    fail_status: u16,
    call_count: Arc<AtomicUsize>,
}

impl FailThenSucceed {
    fn new(fail_count: usize, fail_status: u16) -> (Self, Arc<AtomicUsize>) {
        let call_count = Arc::new(AtomicUsize::new(0));
        (
            Self {
                fail_count,
                fail_status,
                call_count: call_count.clone(),
            },
            call_count,
        )
    }
}

impl Respond for FailThenSucceed {
    fn respond(&self, _request: &wiremock::Request) -> ResponseTemplate {
        let count = self.call_count.fetch_add(1, Ordering::SeqCst);
        if count < self.fail_count {
            ResponseTemplate::new(self.fail_status).set_body_json(serde_json::json!({
                "error": "Service temporarily unavailable"
            }))
        } else {
            ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "id": "123",
                "name": "Test User"
            }))
        }
    }
}

#[tokio::test]
async fn test_retry_succeeds_after_transient_503_errors() {
    let mock_server = MockServer::start().await;

    // Fail twice with 503, then succeed on third attempt
    let (responder, call_count) = FailThenSucceed::new(2, 503);

    Mock::given(method("GET"))
        .and(path("/users/123"))
        .respond_with(responder)
        .expect(3) // Should be called 3 times total
        .mount(&mock_server)
        .await;

    let spec = create_test_spec();

    let command = Command::new("api").subcommand(
        Command::new("users")
            .subcommand(Command::new("get-user-by-id").arg(Arg::new("id").required(true))),
    );

    let matches = command.get_matches_from(vec!["api", "users", "get-user-by-id", "123"]);

    let retry_context = RetryContext {
        max_attempts: 3,
        initial_delay_ms: 10, // Use short delays for tests
        max_delay_ms: 100,
        force_retry: false,
        method: Some("GET".to_string()),
        has_idempotency_key: false,
    };

    let result = execute_request(
        &spec,
        &matches,
        Some(&mock_server.uri()),
        false,
        None,
        None,
        &OutputFormat::Json,
        None,
        None,
        true, // capture_output
        Some(&retry_context),
    )
    .await;

    assert!(result.is_ok(), "Request should succeed after retries");
    assert_eq!(
        call_count.load(Ordering::SeqCst),
        3,
        "Should have made exactly 3 requests"
    );
}

#[tokio::test]
async fn test_retry_exhausted_returns_error_status() {
    let mock_server = MockServer::start().await;

    // Always fail with 503
    Mock::given(method("GET"))
        .and(path("/users/123"))
        .respond_with(ResponseTemplate::new(503).set_body_json(serde_json::json!({
            "error": "Service temporarily unavailable"
        })))
        .expect(3) // Should be called 3 times total
        .mount(&mock_server)
        .await;

    let spec = create_test_spec();

    let command = Command::new("api").subcommand(
        Command::new("users")
            .subcommand(Command::new("get-user-by-id").arg(Arg::new("id").required(true))),
    );

    let matches = command.get_matches_from(vec!["api", "users", "get-user-by-id", "123"]);

    let retry_context = RetryContext {
        max_attempts: 3,
        initial_delay_ms: 10,
        max_delay_ms: 100,
        force_retry: false,
        method: Some("GET".to_string()),
        has_idempotency_key: false,
    };

    let result = execute_request(
        &spec,
        &matches,
        Some(&mock_server.uri()),
        false,
        None,
        None,
        &OutputFormat::Json,
        None,
        None,
        true,
        Some(&retry_context),
    )
    .await;

    // After all retries exhausted, returns the last error status as HTTP error
    assert!(
        result.is_err(),
        "Request should fail after all retries exhausted"
    );
    let err = result.unwrap_err();
    assert!(
        err.to_string().contains("503") || err.to_string().contains("Service"),
        "Error should indicate 503 status"
    );
}

#[tokio::test]
async fn test_retry_respects_retry_after_header() {
    let mock_server = MockServer::start().await;

    // First request returns 429 with Retry-After header
    Mock::given(method("GET"))
        .and(path("/users/123"))
        .respond_with(
            ResponseTemplate::new(429)
                .insert_header("Retry-After", "1")
                .set_body_json(serde_json::json!({
                    "error": "Too many requests"
                })),
        )
        .up_to_n_times(1)
        .mount(&mock_server)
        .await;

    // Second request succeeds
    Mock::given(method("GET"))
        .and(path("/users/123"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "id": "123",
            "name": "Test User"
        })))
        .mount(&mock_server)
        .await;

    let spec = create_test_spec();

    let command = Command::new("api").subcommand(
        Command::new("users")
            .subcommand(Command::new("get-user-by-id").arg(Arg::new("id").required(true))),
    );

    let matches = command.get_matches_from(vec!["api", "users", "get-user-by-id", "123"]);

    let retry_context = RetryContext {
        max_attempts: 3,
        initial_delay_ms: 10,
        max_delay_ms: 5000, // High enough to respect the Retry-After
        force_retry: false,
        method: Some("GET".to_string()),
        has_idempotency_key: false,
    };

    let result = execute_request(
        &spec,
        &matches,
        Some(&mock_server.uri()),
        false,
        None,
        None,
        &OutputFormat::Json,
        None,
        None,
        true,
        Some(&retry_context),
    )
    .await;

    assert!(
        result.is_ok(),
        "Request should succeed after retry following 429"
    );
}

#[tokio::test]
async fn test_no_retry_on_4xx_client_errors() {
    let mock_server = MockServer::start().await;

    // Return 404 - should not be retried
    Mock::given(method("GET"))
        .and(path("/users/123"))
        .respond_with(ResponseTemplate::new(404).set_body_json(serde_json::json!({
            "error": "User not found"
        })))
        .expect(1) // Should only be called once - no retries
        .mount(&mock_server)
        .await;

    let spec = create_test_spec();

    let command = Command::new("api").subcommand(
        Command::new("users")
            .subcommand(Command::new("get-user-by-id").arg(Arg::new("id").required(true))),
    );

    let matches = command.get_matches_from(vec!["api", "users", "get-user-by-id", "123"]);

    let retry_context = RetryContext {
        max_attempts: 3,
        initial_delay_ms: 10,
        max_delay_ms: 100,
        force_retry: false,
        method: Some("GET".to_string()),
        has_idempotency_key: false,
    };

    let result = execute_request(
        &spec,
        &matches,
        Some(&mock_server.uri()),
        false,
        None,
        None,
        &OutputFormat::Json,
        None,
        None,
        true,
        Some(&retry_context),
    )
    .await;

    // 404 is not retryable, so it should fail immediately
    assert!(result.is_err(), "Request should fail on 404");
}

#[tokio::test]
async fn test_retry_disabled_when_max_attempts_zero() {
    let mock_server = MockServer::start().await;

    // Return 503 - would be retried if retries were enabled
    Mock::given(method("GET"))
        .and(path("/users/123"))
        .respond_with(ResponseTemplate::new(503).set_body_json(serde_json::json!({
            "error": "Service temporarily unavailable"
        })))
        .expect(1) // Should only be called once - retries disabled
        .mount(&mock_server)
        .await;

    let spec = create_test_spec();

    let command = Command::new("api").subcommand(
        Command::new("users")
            .subcommand(Command::new("get-user-by-id").arg(Arg::new("id").required(true))),
    );

    let matches = command.get_matches_from(vec!["api", "users", "get-user-by-id", "123"]);

    let retry_context = RetryContext {
        max_attempts: 0, // Disabled
        initial_delay_ms: 10,
        max_delay_ms: 100,
        force_retry: false,
        method: Some("GET".to_string()),
        has_idempotency_key: false,
    };

    let result = execute_request(
        &spec,
        &matches,
        Some(&mock_server.uri()),
        false,
        None,
        None,
        &OutputFormat::Json,
        None,
        None,
        true,
        Some(&retry_context),
    )
    .await;

    assert!(
        result.is_err(),
        "Request should fail on 503 without retries"
    );
}
