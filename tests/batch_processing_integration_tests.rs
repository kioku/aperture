use aperture_cli::batch::{BatchConfig, BatchFile, BatchOperation, BatchProcessor};
use aperture_cli::cache::models::{CachedCommand, CachedParameter, CachedSpec};
use aperture_cli::cli::OutputFormat;
use std::collections::HashMap;
use std::io::Write;
use tempfile::NamedTempFile;

fn create_test_spec() -> CachedSpec {
    CachedSpec {
        cache_format_version: aperture_cli::cache::models::CACHE_FORMAT_VERSION,
        name: "test-api".to_string(),
        version: "1.0.0".to_string(),
        commands: vec![
            CachedCommand {
                name: "users".to_string(),
                description: Some("Get user by ID".to_string()),
                summary: None,
                operation_id: "getUserById".to_string(),
                method: "GET".to_string(),
                path: "/users/{id}".to_string(),
                parameters: vec![CachedParameter {
                    name: "id".to_string(),
                    location: "path".to_string(),
                    required: true,
                    description: Some("User ID".to_string()),
                    schema: Some(r#"{"type": "string"}"#.to_string()),
                    schema_type: Some("string".to_string()),
                    format: None,
                    default_value: None,
                    enum_values: vec![],
                    example: None,
                }],
                request_body: None,
                responses: vec![],
                security_requirements: vec![],
                tags: vec!["users".to_string()],
                deprecated: false,
                external_docs_url: None,
            },
            CachedCommand {
                name: "users".to_string(),
                description: Some("Create a new user".to_string()),
                summary: None,
                operation_id: "createUser".to_string(),
                method: "POST".to_string(),
                path: "/users".to_string(),
                parameters: vec![],
                request_body: Some(aperture_cli::cache::models::CachedRequestBody {
                    description: Some("User data".to_string()),
                    required: true,
                    content_type: "application/json".to_string(),
                    schema: r#"{"type": "object"}"#.to_string(),
                    example: Some(
                        r#"{"name": "John Doe", "email": "john@example.com"}"#.to_string(),
                    ),
                }),
                responses: vec![],
                security_requirements: vec![],
                tags: vec!["users".to_string()],
                deprecated: false,
                external_docs_url: None,
            },
        ],
        base_url: Some("https://api.example.com".to_string()),
        servers: vec!["https://api.example.com".to_string()],
        security_schemes: HashMap::new(),
    }
}

#[tokio::test]
async fn test_batch_file_parsing_json() {
    let batch_content = r#"{
        "operations": [
            {
                "id": "get-user-1",
                "args": ["users", "get-user-by-id", "123"]
            },
            {
                "id": "get-user-2",
                "args": ["users", "get-user-by-id", "456"]
            }
        ]
    }"#;

    let mut temp_file = NamedTempFile::new().unwrap();
    temp_file.write_all(batch_content.as_bytes()).unwrap();
    temp_file.flush().unwrap();

    let batch_file = BatchProcessor::parse_batch_file(temp_file.path())
        .await
        .unwrap();

    assert_eq!(batch_file.operations.len(), 2);
    assert_eq!(batch_file.operations[0].id, Some("get-user-1".to_string()));
    assert_eq!(
        batch_file.operations[0].args,
        vec!["users", "get-user-by-id", "123"]
    );
    assert_eq!(batch_file.operations[1].id, Some("get-user-2".to_string()));
    assert_eq!(
        batch_file.operations[1].args,
        vec!["users", "get-user-by-id", "456"]
    );
}

#[tokio::test]
async fn test_batch_file_parsing_yaml() {
    let batch_content = r#"
operations:
  - id: create-user-1
    args: [users, create-user, --body, '{"name": "Alice", "email": "alice@example.com"}']
  - id: create-user-2
    args: [users, create-user, --body, '{"name": "Bob", "email": "bob@example.com"}']
"#;

    let mut temp_file = NamedTempFile::new().unwrap();
    temp_file.write_all(batch_content.as_bytes()).unwrap();
    temp_file.flush().unwrap();

    let batch_file = BatchProcessor::parse_batch_file(temp_file.path())
        .await
        .unwrap();

    assert_eq!(batch_file.operations.len(), 2);
    assert_eq!(
        batch_file.operations[0].id,
        Some("create-user-1".to_string())
    );
    assert_eq!(batch_file.operations[0].args[0], "users");
    assert_eq!(batch_file.operations[0].args[1], "create-user");
    assert_eq!(
        batch_file.operations[1].id,
        Some("create-user-2".to_string())
    );
}

#[tokio::test]
async fn test_batch_file_parsing_invalid_format() {
    let batch_content = "invalid json content {";

    let mut temp_file = NamedTempFile::new().unwrap();
    temp_file.write_all(batch_content.as_bytes()).unwrap();
    temp_file.flush().unwrap();

    let result = BatchProcessor::parse_batch_file(temp_file.path()).await;
    assert!(result.is_err());
}

#[tokio::test]
async fn test_batch_file_parsing_empty_operations() {
    let batch_content = r#"{
        "operations": []
    }"#;

    let mut temp_file = NamedTempFile::new().unwrap();
    temp_file.write_all(batch_content.as_bytes()).unwrap();
    temp_file.flush().unwrap();

    let batch_file = BatchProcessor::parse_batch_file(temp_file.path())
        .await
        .unwrap();
    assert_eq!(batch_file.operations.len(), 0);
}

#[tokio::test]
async fn test_batch_config_default() {
    let config = BatchConfig::default();
    assert_eq!(config.max_concurrency, 5);
    assert_eq!(config.rate_limit, None);
    assert_eq!(config.continue_on_error, true);
    assert_eq!(config.show_progress, true);
}

#[tokio::test]
async fn test_batch_config_custom() {
    let config = BatchConfig {
        max_concurrency: 10,
        rate_limit: Some(100),
        continue_on_error: true,
        show_progress: true,
    };

    assert_eq!(config.max_concurrency, 10);
    assert_eq!(config.rate_limit, Some(100));
    assert_eq!(config.continue_on_error, true);
    assert_eq!(config.show_progress, true);
}

#[tokio::test]
async fn test_batch_processor_creation() {
    let config = BatchConfig {
        max_concurrency: 3,
        rate_limit: Some(50),
        continue_on_error: false,
        show_progress: false,
    };

    let _processor = BatchProcessor::new(config);
    // Verify the processor was created successfully
    // We can't directly access config fields as they are private
}

#[tokio::test]
async fn test_batch_operation_serialization() {
    let operation = BatchOperation {
        id: Some("test-op".to_string()),
        args: vec![
            "users".to_string(),
            "get-user-by-id".to_string(),
            "123".to_string(),
        ],
        description: None,
        headers: std::collections::HashMap::new(),
        use_cache: None,
    };

    let serialized = serde_json::to_string(&operation).unwrap();
    let deserialized: BatchOperation = serde_json::from_str(&serialized).unwrap();

    assert_eq!(deserialized.id, Some("test-op".to_string()));
    assert_eq!(deserialized.args, vec!["users", "get-user-by-id", "123"]);
}

#[tokio::test]
async fn test_batch_file_serialization() {
    let batch_file = BatchFile {
        metadata: None,
        operations: vec![
            BatchOperation {
                id: Some("op1".to_string()),
                args: vec![
                    "users".to_string(),
                    "get-user-by-id".to_string(),
                    "123".to_string(),
                ],
                description: None,
                headers: std::collections::HashMap::new(),
                use_cache: None,
            },
            BatchOperation {
                id: Some("op2".to_string()),
                args: vec![
                    "users".to_string(),
                    "get-user-by-id".to_string(),
                    "456".to_string(),
                ],
                description: None,
                headers: std::collections::HashMap::new(),
                use_cache: None,
            },
        ],
    };

    let serialized = serde_json::to_string_pretty(&batch_file).unwrap();
    let deserialized: BatchFile = serde_json::from_str(&serialized).unwrap();

    assert_eq!(deserialized.operations.len(), 2);
    assert_eq!(deserialized.operations[0].id, Some("op1".to_string()));
    assert_eq!(deserialized.operations[1].id, Some("op2".to_string()));
}

#[tokio::test]
async fn test_batch_dry_run_execution() {
    let spec = create_test_spec();
    let config = BatchConfig::default();
    let processor = BatchProcessor::new(config);

    let batch_file = BatchFile {
        metadata: None,
        operations: vec![BatchOperation {
            id: Some("test-op".to_string()),
            args: vec![
                "users".to_string(),
                "get-user-by-id".to_string(),
                "123".to_string(),
            ],
            description: None,
            headers: std::collections::HashMap::new(),
            use_cache: None,
        }],
    };

    // Note: This test only verifies the batch processor can be created and called
    // The actual execution is not fully implemented in the current batch processor
    let result = processor
        .execute_batch(
            &spec,
            batch_file,
            None,
            None,
            true, // dry_run
            &OutputFormat::Json,
            None,
        )
        .await;

    // The current implementation returns a placeholder for dry run
    assert!(result.is_ok());
    let batch_result = result.unwrap();
    assert_eq!(batch_result.results.len(), 1);
    assert_eq!(batch_result.success_count, 1);
    assert_eq!(batch_result.failure_count, 0);
}

#[tokio::test]
async fn test_batch_complex_operations() {
    let batch_content = r#"{
        "operations": [
            {
                "id": "create-user",
                "args": ["users", "create-user", "--body", "{\"name\": \"John\", \"email\": \"john@example.com\"}"]
            },
            {
                "id": "get-user",
                "args": ["users", "get-user-by-id", "123"]
            },
            {
                "id": "update-user",
                "args": ["users", "update-user", "123", "--body", "{\"name\": \"John Updated\"}"]
            }
        ]
    }"#;

    let mut temp_file = NamedTempFile::new().unwrap();
    temp_file.write_all(batch_content.as_bytes()).unwrap();
    temp_file.flush().unwrap();

    let batch_file = BatchProcessor::parse_batch_file(temp_file.path())
        .await
        .unwrap();

    assert_eq!(batch_file.operations.len(), 3);

    // Verify create operation
    assert_eq!(batch_file.operations[0].id, Some("create-user".to_string()));
    assert!(batch_file.operations[0]
        .args
        .contains(&"--body".to_string()));

    // Verify get operation
    assert_eq!(batch_file.operations[1].id, Some("get-user".to_string()));
    assert!(batch_file.operations[1]
        .args
        .contains(&"get-user-by-id".to_string()));

    // Verify update operation
    assert_eq!(batch_file.operations[2].id, Some("update-user".to_string()));
    assert!(batch_file.operations[2]
        .args
        .contains(&"update-user".to_string()));
}
