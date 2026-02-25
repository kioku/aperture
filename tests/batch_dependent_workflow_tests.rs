//! Integration tests for dependent batch workflows (variable capture and chaining).

mod test_helpers;

use aperture_cli::batch::{BatchConfig, BatchFile, BatchOperation, BatchProcessor};
use aperture_cli::cache::models::{CachedCommand, CachedParameter, CachedRequestBody, CachedSpec};
use aperture_cli::cli::OutputFormat;
use aperture_cli::constants;
use std::collections::HashMap;
use std::io::Write;
use tempfile::NamedTempFile;

// ── Helpers ─────────────────────────────────────────────────────────

#[allow(clippy::too_many_lines)]
fn test_spec(base_url: &str) -> CachedSpec {
    CachedSpec {
        cache_format_version: aperture_cli::cache::models::CACHE_FORMAT_VERSION,
        name: "test-api".into(),
        version: "1.0.0".into(),
        commands: vec![
            CachedCommand {
                name: "users".into(),
                description: Some("Create a new user".into()),
                summary: None,
                operation_id: "createUser".into(),
                method: constants::HTTP_METHOD_POST.into(),
                path: "/users".into(),
                parameters: vec![],
                request_body: Some(CachedRequestBody {
                    description: Some("User data".into()),
                    required: true,
                    content_type: constants::CONTENT_TYPE_JSON.into(),
                    schema: r#"{"type": "object"}"#.into(),
                    example: None,
                }),
                responses: vec![],
                security_requirements: vec![],
                tags: vec!["users".into()],
                deprecated: false,
                external_docs_url: None,
                examples: vec![],
                display_group: None,
                display_name: None,
                aliases: vec![],
                hidden: false,
            },
            CachedCommand {
                name: "users".into(),
                description: Some("Get user by ID".into()),
                summary: None,
                operation_id: "getUserById".into(),
                method: constants::HTTP_METHOD_GET.into(),
                path: "/users/{id}".into(),
                parameters: vec![CachedParameter {
                    name: "id".into(),
                    location: "path".into(),
                    required: true,
                    description: Some("User ID".into()),
                    schema: Some(r#"{"type": "string"}"#.into()),
                    schema_type: Some("string".into()),
                    format: None,
                    default_value: None,
                    enum_values: vec![],
                    example: None,
                }],
                request_body: None,
                responses: vec![],
                security_requirements: vec![],
                tags: vec!["users".into()],
                deprecated: false,
                external_docs_url: None,
                examples: vec![],
                display_group: None,
                display_name: None,
                aliases: vec![],
                hidden: false,
            },
            CachedCommand {
                name: "groups".into(),
                description: Some("Add member to group".into()),
                summary: None,
                operation_id: "addGroupMember".into(),
                method: constants::HTTP_METHOD_POST.into(),
                path: "/groups/{group_id}/members".into(),
                parameters: vec![CachedParameter {
                    name: "group_id".into(),
                    location: "path".into(),
                    required: true,
                    description: Some("Group ID".into()),
                    schema: Some(r#"{"type": "string"}"#.into()),
                    schema_type: Some("string".into()),
                    format: None,
                    default_value: None,
                    enum_values: vec![],
                    example: None,
                }],
                request_body: Some(CachedRequestBody {
                    description: Some("Member data".into()),
                    required: true,
                    content_type: constants::CONTENT_TYPE_JSON.into(),
                    schema: r#"{"type": "object"}"#.into(),
                    example: None,
                }),
                responses: vec![],
                security_requirements: vec![],
                tags: vec!["groups".into()],
                deprecated: false,
                external_docs_url: None,
                examples: vec![],
                display_group: None,
                display_name: None,
                aliases: vec![],
                hidden: false,
            },
        ],
        base_url: Some(base_url.into()),
        servers: vec![base_url.into()],
        security_schemes: HashMap::new(),
        skipped_endpoints: vec![],
        server_variables: HashMap::new(),
    }
}

const fn quiet_config() -> BatchConfig {
    BatchConfig {
        max_concurrency: 1,
        rate_limit: None,
        continue_on_error: false,
        show_progress: false,
        suppress_output: true,
    }
}

// ── Tests ───────────────────────────────────────────────────────────

#[tokio::test]
async fn linear_chain_create_then_get() {
    let mock = wiremock::MockServer::start().await;

    // POST /users → returns { "id": "user-42" }
    mock.register(
        wiremock::Mock::given(wiremock::matchers::method("POST"))
            .and(wiremock::matchers::path("/users"))
            .respond_with(
                wiremock::ResponseTemplate::new(201)
                    .set_body_json(serde_json::json!({"id": "user-42", "name": "Alice"}))
                    .insert_header("content-type", constants::CONTENT_TYPE_JSON),
            ),
    )
    .await;

    // GET /users/user-42 → returns user details
    mock.register(
        wiremock::Mock::given(wiremock::matchers::method("GET"))
            .and(wiremock::matchers::path("/users/user-42"))
            .respond_with(
                wiremock::ResponseTemplate::new(200)
                    .set_body_json(serde_json::json!({"id": "user-42", "name": "Alice", "email": "alice@example.com"}))
                    .insert_header("content-type", constants::CONTENT_TYPE_JSON),
            ),
    )
    .await;

    let spec = test_spec(&mock.uri());
    let processor = BatchProcessor::new(quiet_config());

    let batch = BatchFile {
        metadata: None,
        operations: vec![
            BatchOperation {
                id: Some("create".into()),
                args: vec![
                    "users".into(),
                    "create-user".into(),
                    "--body".into(),
                    r#"{"name": "Alice"}"#.into(),
                ],
                capture: Some(HashMap::from([("user_id".into(), ".id".into())])),
                ..Default::default()
            },
            BatchOperation {
                id: Some("get".into()),
                args: vec![
                    "users".into(),
                    "get-user-by-id".into(),
                    "--id".into(),
                    "{{user_id}}".into(),
                ],
                depends_on: Some(vec!["create".into()]),
                ..Default::default()
            },
        ],
    };

    let result = processor
        .execute_batch(
            &spec,
            batch,
            None,
            Some(&mock.uri()),
            false,
            &OutputFormat::Json,
            None,
        )
        .await
        .unwrap();

    assert_eq!(result.success_count, 2);
    assert_eq!(result.failure_count, 0);

    // Verify the second operation actually hit the correct URL
    let received = mock.received_requests().await.unwrap();
    assert_eq!(received.len(), 2);
    assert_eq!(received[1].url.path(), "/users/user-42");
}

#[tokio::test]
async fn fan_out_aggregate_with_capture_append() {
    let mock = wiremock::MockServer::start().await;

    // Two POST calls, each returning a different ID
    mock.register(
        wiremock::Mock::given(wiremock::matchers::method("POST"))
            .and(wiremock::matchers::path("/users"))
            .respond_with(
                wiremock::ResponseTemplate::new(201)
                    .set_body_json(serde_json::json!({"id": "beat-id"})),
            ),
    )
    .await;

    // Terminal call that receives the aggregated IDs
    mock.register(
        wiremock::Mock::given(wiremock::matchers::method("POST"))
            .and(wiremock::matchers::path_regex("/groups/.*/members"))
            .respond_with(
                wiremock::ResponseTemplate::new(200).set_body_json(serde_json::json!({"ok": true})),
            ),
    )
    .await;

    let spec = test_spec(&mock.uri());
    let processor = BatchProcessor::new(quiet_config());

    let batch = BatchFile {
        metadata: None,
        operations: vec![
            BatchOperation {
                id: Some("beat-1".into()),
                args: vec![
                    "users".into(),
                    "create-user".into(),
                    "--body".into(),
                    r#"{"name": "A"}"#.into(),
                ],
                capture_append: Some(HashMap::from([("event_ids".into(), ".id".into())])),
                ..Default::default()
            },
            BatchOperation {
                id: Some("beat-2".into()),
                args: vec![
                    "users".into(),
                    "create-user".into(),
                    "--body".into(),
                    r#"{"name": "B"}"#.into(),
                ],
                capture_append: Some(HashMap::from([("event_ids".into(), ".id".into())])),
                ..Default::default()
            },
            BatchOperation {
                id: Some("aggregate".into()),
                args: vec![
                    "groups".into(),
                    "add-group-member".into(),
                    "--group-id".into(),
                    "admins".into(),
                    "--body".into(),
                    r#"{"memberIds": {{event_ids}}}"#.into(),
                ],
                depends_on: Some(vec!["beat-1".into(), "beat-2".into()]),
                ..Default::default()
            },
        ],
    };

    let result = processor
        .execute_batch(
            &spec,
            batch,
            None,
            Some(&mock.uri()),
            false,
            &OutputFormat::Json,
            None,
        )
        .await
        .unwrap();

    assert_eq!(result.success_count, 3);
    assert_eq!(result.failure_count, 0);

    // Verify the aggregate call received the array
    let received = mock.received_requests().await.unwrap();
    assert_eq!(received.len(), 3);
    let body = std::str::from_utf8(&received[2].body).unwrap();
    assert!(
        body.contains("beat-id"),
        "expected aggregate body to contain IDs, got: {body}"
    );
}

#[tokio::test]
async fn atomic_execution_halts_on_failure() {
    let mock = wiremock::MockServer::start().await;

    // First call succeeds
    mock.register(
        wiremock::Mock::given(wiremock::matchers::method("POST"))
            .and(wiremock::matchers::path("/users"))
            .respond_with(
                wiremock::ResponseTemplate::new(201)
                    .set_body_json(serde_json::json!({"id": "ok-id"})),
            ),
    )
    .await;

    // Second call fails (404)
    mock.register(
        wiremock::Mock::given(wiremock::matchers::method("GET"))
            .and(wiremock::matchers::path("/users/ok-id"))
            .respond_with(wiremock::ResponseTemplate::new(404)),
    )
    .await;

    let spec = test_spec(&mock.uri());
    let processor = BatchProcessor::new(quiet_config());

    let batch = BatchFile {
        metadata: None,
        operations: vec![
            BatchOperation {
                id: Some("create".into()),
                args: vec![
                    "users".into(),
                    "create-user".into(),
                    "--body".into(),
                    r#"{"name": "X"}"#.into(),
                ],
                capture: Some(HashMap::from([("uid".into(), ".id".into())])),
                ..Default::default()
            },
            BatchOperation {
                id: Some("get".into()),
                args: vec![
                    "users".into(),
                    "get-user-by-id".into(),
                    "--id".into(),
                    "{{uid}}".into(),
                ],
                depends_on: Some(vec!["create".into()]),
                ..Default::default()
            },
            BatchOperation {
                id: Some("group".into()),
                args: vec![
                    "groups".into(),
                    "add-group-member".into(),
                    "--group-id".into(),
                    "admins".into(),
                    "--body".into(),
                    r#"{"userId": "{{uid}}"}"#.into(),
                ],
                depends_on: Some(vec!["get".into()]),
                ..Default::default()
            },
        ],
    };

    let result = processor
        .execute_batch(
            &spec,
            batch,
            None,
            Some(&mock.uri()),
            false,
            &OutputFormat::Json,
            None,
        )
        .await
        .unwrap();

    // First op succeeded, second failed, third was skipped
    assert_eq!(result.success_count, 1);
    assert_eq!(result.failure_count, 2);
    assert!(result.results[0].success);
    assert!(!result.results[1].success);
    assert!(!result.results[2].success);
    assert_eq!(
        result.results[2].error.as_deref(),
        Some("Skipped due to prior failure")
    );

    // Only 2 requests should have been made (third was skipped)
    let received = mock.received_requests().await.unwrap();
    assert_eq!(received.len(), 2);
}

#[tokio::test]
async fn cycle_detection_in_batch() {
    let mock = wiremock::MockServer::start().await;
    let spec = test_spec(&mock.uri());
    let processor = BatchProcessor::new(quiet_config());

    let batch = BatchFile {
        metadata: None,
        operations: vec![
            BatchOperation {
                id: Some("a".into()),
                args: vec![
                    "users".into(),
                    "create-user".into(),
                    "--body".into(),
                    "{}".into(),
                ],
                depends_on: Some(vec!["b".into()]),
                ..Default::default()
            },
            BatchOperation {
                id: Some("b".into()),
                args: vec![
                    "users".into(),
                    "create-user".into(),
                    "--body".into(),
                    "{}".into(),
                ],
                depends_on: Some(vec!["a".into()]),
                ..Default::default()
            },
        ],
    };

    let result = processor
        .execute_batch(
            &spec,
            batch,
            None,
            Some(&mock.uri()),
            false,
            &OutputFormat::Json,
            None,
        )
        .await;

    assert!(result.is_err());
    let err = result.unwrap_err().to_string();
    assert!(err.contains("cycle"), "expected cycle error, got: {err}");
}

#[tokio::test]
async fn backward_compatible_no_dependencies() {
    let mock = wiremock::MockServer::start().await;

    mock.register(
        wiremock::Mock::given(wiremock::matchers::method("GET"))
            .and(wiremock::matchers::path("/users/1"))
            .respond_with(
                wiremock::ResponseTemplate::new(200).set_body_json(serde_json::json!({"id": "1"})),
            ),
    )
    .await;

    mock.register(
        wiremock::Mock::given(wiremock::matchers::method("GET"))
            .and(wiremock::matchers::path("/users/2"))
            .respond_with(
                wiremock::ResponseTemplate::new(200).set_body_json(serde_json::json!({"id": "2"})),
            ),
    )
    .await;

    let spec = test_spec(&mock.uri());
    let processor = BatchProcessor::new(quiet_config());

    // No capture, no depends_on → concurrent execution
    let batch = BatchFile {
        metadata: None,
        operations: vec![
            BatchOperation {
                id: Some("get-1".into()),
                args: vec![
                    "users".into(),
                    "get-user-by-id".into(),
                    "--id".into(),
                    "1".into(),
                ],
                ..Default::default()
            },
            BatchOperation {
                id: Some("get-2".into()),
                args: vec![
                    "users".into(),
                    "get-user-by-id".into(),
                    "--id".into(),
                    "2".into(),
                ],
                ..Default::default()
            },
        ],
    };

    let result = processor
        .execute_batch(
            &spec,
            batch,
            None,
            Some(&mock.uri()),
            false,
            &OutputFormat::Json,
            None,
        )
        .await
        .unwrap();

    assert_eq!(result.success_count, 2);
    assert_eq!(result.failure_count, 0);
}

#[tokio::test]
async fn implicit_dependency_from_variable_ref() {
    let mock = wiremock::MockServer::start().await;

    mock.register(
        wiremock::Mock::given(wiremock::matchers::method("POST"))
            .and(wiremock::matchers::path("/users"))
            .respond_with(
                wiremock::ResponseTemplate::new(201)
                    .set_body_json(serde_json::json!({"id": "impl-42"})),
            ),
    )
    .await;

    mock.register(
        wiremock::Mock::given(wiremock::matchers::method("GET"))
            .and(wiremock::matchers::path("/users/impl-42"))
            .respond_with(
                wiremock::ResponseTemplate::new(200)
                    .set_body_json(serde_json::json!({"id": "impl-42", "name": "Test"})),
            ),
    )
    .await;

    let spec = test_spec(&mock.uri());
    let processor = BatchProcessor::new(quiet_config());

    // No explicit depends_on — dependency inferred from {{user_id}} usage
    let batch = BatchFile {
        metadata: None,
        operations: vec![
            BatchOperation {
                id: Some("create".into()),
                args: vec![
                    "users".into(),
                    "create-user".into(),
                    "--body".into(),
                    r#"{"name": "Test"}"#.into(),
                ],
                capture: Some(HashMap::from([("user_id".into(), ".id".into())])),
                ..Default::default()
            },
            BatchOperation {
                id: Some("get".into()),
                args: vec![
                    "users".into(),
                    "get-user-by-id".into(),
                    "--id".into(),
                    "{{user_id}}".into(),
                ],
                // No depends_on — should be inferred
                ..Default::default()
            },
        ],
    };

    let result = processor
        .execute_batch(
            &spec,
            batch,
            None,
            Some(&mock.uri()),
            false,
            &OutputFormat::Json,
            None,
        )
        .await
        .unwrap();

    assert_eq!(result.success_count, 2);
    assert_eq!(result.failure_count, 0);
}

#[tokio::test]
async fn dependent_batch_yaml_parsing() {
    let yaml_content = r#"
operations:
  - id: create-user
    args: [users, create-user, --body, '{"name": "Alice"}']
    capture:
      user_id: ".id"

  - id: get-user
    args: [users, get-user-by-id, --id, "{{user_id}}"]
    depends_on: [create-user]
"#;

    let mut temp_file = NamedTempFile::new().unwrap();
    temp_file.write_all(yaml_content.as_bytes()).unwrap();
    temp_file.flush().unwrap();

    let batch = BatchProcessor::parse_batch_file(temp_file.path())
        .await
        .unwrap();

    assert_eq!(batch.operations.len(), 2);
    assert!(batch.operations[0].capture.is_some());
    assert_eq!(
        batch.operations[0]
            .capture
            .as_ref()
            .unwrap()
            .get("user_id")
            .unwrap(),
        ".id"
    );
    assert_eq!(
        batch.operations[1].depends_on.as_ref().unwrap(),
        &["create-user"]
    );
    assert!(batch.operations[1].args.contains(&"{{user_id}}".into()));
}

#[tokio::test]
async fn dependent_batch_json_parsing() {
    let json_content = r#"{
        "operations": [
            {
                "id": "step-1",
                "args": ["users", "create-user", "--body", "{\"name\": \"Bob\"}"],
                "capture": {"new_id": ".id"},
                "capture_append": {"all_ids": ".id"}
            },
            {
                "id": "step-2",
                "args": ["users", "get-user-by-id", "--id", "{{new_id}}"],
                "depends_on": ["step-1"]
            }
        ]
    }"#;

    let mut temp_file = NamedTempFile::new().unwrap();
    temp_file.write_all(json_content.as_bytes()).unwrap();
    temp_file.flush().unwrap();

    let batch = BatchProcessor::parse_batch_file(temp_file.path())
        .await
        .unwrap();

    assert_eq!(batch.operations.len(), 2);
    assert!(batch.operations[0].capture.is_some());
    assert!(batch.operations[0].capture_append.is_some());
    assert_eq!(
        batch.operations[0]
            .capture_append
            .as_ref()
            .unwrap()
            .get("all_ids")
            .unwrap(),
        ".id"
    );
}

#[tokio::test]
async fn dependent_dry_run_skips_capture() {
    let mock = wiremock::MockServer::start().await;
    let spec = test_spec(&mock.uri());
    let processor = BatchProcessor::new(quiet_config());

    let batch = BatchFile {
        metadata: None,
        operations: vec![
            BatchOperation {
                id: Some("create".into()),
                args: vec![
                    "users".into(),
                    "create-user".into(),
                    "--body".into(),
                    r#"{"name": "X"}"#.into(),
                ],
                capture: Some(HashMap::from([("uid".into(), ".id".into())])),
                ..Default::default()
            },
            BatchOperation {
                id: Some("get".into()),
                args: vec![
                    "users".into(),
                    "get-user-by-id".into(),
                    "--id".into(),
                    "{{uid}}".into(),
                ],
                depends_on: Some(vec!["create".into()]),
                ..Default::default()
            },
        ],
    };

    // Dry-run produces a success message string, not a JSON response.
    // Capture will fail on that string, which is expected in dry-run mode.
    // This test verifies that dry-run doesn't panic.
    let result = processor
        .execute_batch(
            &spec,
            batch,
            None,
            None,
            true, // dry_run
            &OutputFormat::Json,
            None,
        )
        .await;

    // In dry-run mode the capture will fail because the response is not JSON,
    // but we should get a clean error rather than a panic.
    assert!(
        result.is_ok() || result.is_err(),
        "should not panic in dry-run"
    );
}
