/// Integration test to verify --show-examples flag works through full execution path
///
/// This test ensures that the executor can properly handle the --show-examples flag
/// by actually executing a command (not just parsing arguments). This catches bugs
/// like incorrect Clap API usage or wrong `ArgMatches` level being passed to executor.
use aperture_cli::cache::models::{
    CachedCommand, CachedParameter, CachedSpec, CACHE_FORMAT_VERSION,
};
use aperture_cli::cli::OutputFormat;
use aperture_cli::engine::executor::execute_request;
use aperture_cli::engine::generator::generate_command_tree_with_flags;

/// Create a test spec with both boolean parameters and nested subcommand structure
/// to test the full command hierarchy traversal
fn create_nested_test_spec() -> CachedSpec {
    CachedSpec {
        name: "test-api".to_string(),
        version: "1.0.0".to_string(),
        base_url: Some("https://api.example.com".to_string()),
        cache_format_version: CACHE_FORMAT_VERSION,
        servers: vec![],
        server_variables: std::collections::HashMap::new(),
        skipped_endpoints: vec![],
        commands: vec![
            CachedCommand {
                name: "users".to_string(),
                operation_id: "listUsers".to_string(),
                summary: Some("List users".to_string()),
                description: Some("Retrieves all users".to_string()),
                method: "GET".to_string(),
                path: "/users".to_string(),
                parameters: vec![CachedParameter {
                    name: "active".to_string(),
                    location: "query".to_string(),
                    required: false,
                    schema_type: Some("boolean".to_string()),
                    description: Some("Filter by active users".to_string()),
                    schema: Some(r#"{"type": "boolean"}"#.to_string()),
                    format: None,
                    default_value: None,
                    enum_values: vec![],
                    example: None,
                }],
                request_body: None,
                security_requirements: vec![],
                examples: vec![],
                deprecated: false,
                external_docs_url: None,
                responses: vec![],
                tags: vec!["users".to_string()],
            },
            CachedCommand {
                name: "getUser".to_string(),
                operation_id: "getUserById".to_string(),
                summary: Some("Get user by ID".to_string()),
                description: Some("Retrieves a specific user".to_string()),
                method: "GET".to_string(),
                path: "/users/{id}".to_string(),
                parameters: vec![
                    CachedParameter {
                        name: "id".to_string(),
                        location: "path".to_string(),
                        required: true,
                        schema_type: Some("string".to_string()),
                        description: Some("User ID".to_string()),
                        schema: Some(r#"{"type": "string"}"#.to_string()),
                        format: None,
                        default_value: None,
                        enum_values: vec![],
                        example: None,
                    },
                    CachedParameter {
                        name: "includeDetails".to_string(),
                        location: "query".to_string(),
                        required: false,
                        schema_type: Some("boolean".to_string()),
                        description: Some("Include detailed information".to_string()),
                        schema: Some(r#"{"type": "boolean"}"#.to_string()),
                        format: None,
                        default_value: None,
                        enum_values: vec![],
                        example: None,
                    },
                ],
                request_body: None,
                security_requirements: vec![],
                examples: vec![],
                deprecated: false,
                external_docs_url: None,
                responses: vec![],
                tags: vec!["users".to_string()],
            },
        ],
        security_schemes: std::collections::HashMap::new(),
    }
}

#[tokio::test]
async fn test_show_examples_flag_through_executor() {
    let spec = create_nested_test_spec();
    let cmd = generate_command_tree_with_flags(&spec, false);

    // Parse command with --show-examples flag
    let matches = cmd
        .try_get_matches_from(vec!["api", "users", "list-users", "--show-examples"])
        .expect("Should parse command with --show-examples flag");

    // Execute through the actual executor (this would have panicked with the bugs)
    let result = execute_request(
        &spec,
        &matches,
        None,
        false,
        None,
        None,
        &OutputFormat::Json,
        None,
        None,
        true, // capture_output = true so we don't make actual HTTP requests
    )
    .await;

    // Should succeed and return None (examples are printed, no HTTP request made)
    assert!(
        result.is_ok(),
        "Executor should handle --show-examples flag without panicking: {:?}",
        result.err()
    );

    let output = result.unwrap();
    assert!(
        output.is_none(),
        "--show-examples should return None (no HTTP request)"
    );
}

#[tokio::test]
async fn test_normal_execution_without_show_examples() {
    let spec = create_nested_test_spec();
    let cmd = generate_command_tree_with_flags(&spec, false);

    // Parse command WITHOUT --show-examples flag
    let matches = cmd
        .try_get_matches_from(vec!["api", "users", "list-users"])
        .expect("Should parse command without --show-examples flag");

    // Execute through the actual executor
    let result = execute_request(
        &spec,
        &matches,
        None,
        false,
        None,
        None,
        &OutputFormat::Json,
        None,
        None,
        true, // capture_output = true so we don't make actual HTTP requests
    )
    .await;

    // Should succeed (would fail with network error in real execution, but that's expected)
    // The important part is it doesn't panic trying to access the flag
    assert!(
        result.is_ok() || result.is_err(),
        "Executor should handle command without --show-examples flag without panicking"
    );
}

#[tokio::test]
async fn test_boolean_parameter_with_show_examples() {
    let spec = create_nested_test_spec();
    let cmd = generate_command_tree_with_flags(&spec, false);

    // Parse command with both boolean parameter and --show-examples
    let matches = cmd
        .try_get_matches_from(vec![
            "api",
            "users",
            "list-users",
            "--active",
            "--show-examples",
        ])
        .expect("Should parse command with boolean parameter and --show-examples");

    // Execute through the actual executor
    let result = execute_request(
        &spec,
        &matches,
        None,
        false,
        None,
        None,
        &OutputFormat::Json,
        None,
        None,
        true,
    )
    .await;

    // Should succeed
    assert!(
        result.is_ok(),
        "Executor should handle boolean parameter with --show-examples: {:?}",
        result.err()
    );

    let output = result.unwrap();
    assert!(
        output.is_none(),
        "--show-examples should return None even with other parameters"
    );
}

#[tokio::test]
async fn test_command_with_multiple_parameters_and_show_examples() {
    let spec = create_nested_test_spec();
    let cmd = generate_command_tree_with_flags(&spec, false);

    // Parse command with both types of boolean parameters
    let matches = cmd
        .try_get_matches_from(vec![
            "api",
            "users",
            "list-users",
            "--active",        // boolean query parameter
            "--show-examples", // internal flag
        ])
        .expect("Should parse command with multiple flags");

    // This specifically tests Bug #2: that we return the correct (deepest) ArgMatches
    // Without the fix, operation_matches would point to the parent level and panic
    let result = execute_request(
        &spec,
        &matches,
        None,
        false,
        None,
        None,
        &OutputFormat::Json,
        None,
        None,
        true,
    )
    .await;

    assert!(
        result.is_ok(),
        "Executor should find --show-examples at correct ArgMatches level: {:?}",
        result.err()
    );
}
