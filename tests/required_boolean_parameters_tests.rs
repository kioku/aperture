mod test_helpers;

/// Tests for boolean parameter handling with required/optional semantics
///
/// Boolean parameters have special handling:
/// - **Path parameters**: Always optional (flag presence = true, absence = false)
/// - **Query/Header parameters**: Respect `OpenAPI` required field
///
/// This test suite verifies that:
/// 1. Boolean path parameters default to false when flag omitted (even if `OpenAPI` marks as required)
/// 2. Boolean path parameters substitute "true"/"false" correctly in URLs
/// 3. Required boolean query parameters error when missing
/// 4. Required booleans work correctly when provided
/// 5. Optional booleans continue to work as before (default to false when absent)
use aperture_cli::cache::models::{
    CachedCommand, CachedParameter, CachedSpec, CACHE_FORMAT_VERSION,
};
use aperture_cli::cli::OutputFormat;
use aperture_cli::engine::executor::execute_request;
use aperture_cli::engine::generator::generate_command_tree_with_flags;
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

fn create_spec_with_required_boolean_path_param() -> CachedSpec {
    CachedSpec {
        name: "test-api".to_string(),
        version: "1.0.0".to_string(),
        base_url: Some("https://api.example.com".to_string()),
        cache_format_version: CACHE_FORMAT_VERSION,
        servers: vec![],
        server_variables: std::collections::HashMap::new(),
        skipped_endpoints: vec![],
        commands: vec![CachedCommand {
            name: "items".to_string(),
            operation_id: "getItem".to_string(),
            summary: Some("Get item".to_string()),
            description: None,
            method: "GET".to_string(),
            path: "/items/{id}/{active}".to_string(),
            parameters: vec![
                CachedParameter {
                    name: "id".to_string(),
                    location: "path".to_string(),
                    required: true,
                    schema_type: Some("string".to_string()),
                    description: None,
                    schema: Some(r#"{"type": "string"}"#.to_string()),
                    format: None,
                    default_value: None,
                    enum_values: vec![],
                    example: None,
                },
                CachedParameter {
                    name: "active".to_string(),
                    location: "path".to_string(),
                    required: true, // REQUIRED boolean
                    schema_type: Some("boolean".to_string()),
                    description: None,
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
            tags: vec![],
        }],
        security_schemes: std::collections::HashMap::new(),
    }
}

fn create_spec_with_required_boolean_query_param() -> CachedSpec {
    CachedSpec {
        name: "test-api".to_string(),
        version: "1.0.0".to_string(),
        base_url: Some("https://api.example.com".to_string()),
        cache_format_version: CACHE_FORMAT_VERSION,
        servers: vec![],
        server_variables: std::collections::HashMap::new(),
        skipped_endpoints: vec![],
        commands: vec![CachedCommand {
            name: "users".to_string(),
            operation_id: "listUsers".to_string(),
            summary: Some("List users".to_string()),
            description: None,
            method: "GET".to_string(),
            path: "/users".to_string(),
            parameters: vec![CachedParameter {
                name: "includeInactive".to_string(),
                location: "query".to_string(),
                required: true, // REQUIRED boolean
                schema_type: Some("boolean".to_string()),
                description: Some("Must specify whether to include inactive users".to_string()),
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
            tags: vec![],
        }],
        security_schemes: std::collections::HashMap::new(),
    }
}

fn create_spec_with_mixed_boolean_params() -> CachedSpec {
    CachedSpec {
        name: "test-api".to_string(),
        version: "1.0.0".to_string(),
        base_url: Some("https://api.example.com".to_string()),
        cache_format_version: CACHE_FORMAT_VERSION,
        servers: vec![],
        server_variables: std::collections::HashMap::new(),
        skipped_endpoints: vec![],
        commands: vec![CachedCommand {
            name: "search".to_string(),
            operation_id: "search".to_string(),
            summary: Some("Search with filters".to_string()),
            description: None,
            method: "GET".to_string(),
            path: "/search".to_string(),
            parameters: vec![
                CachedParameter {
                    name: "required-flag".to_string(),
                    location: "query".to_string(),
                    required: true, // REQUIRED
                    schema_type: Some("boolean".to_string()),
                    description: Some("Must be specified".to_string()),
                    schema: Some(r#"{"type": "boolean"}"#.to_string()),
                    format: None,
                    default_value: None,
                    enum_values: vec![],
                    example: None,
                },
                CachedParameter {
                    name: "optional-flag".to_string(),
                    location: "query".to_string(),
                    required: false, // OPTIONAL
                    schema_type: Some("boolean".to_string()),
                    description: Some("Optional filter".to_string()),
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
            tags: vec![],
        }],
        security_schemes: std::collections::HashMap::new(),
    }
}

#[test]
fn test_required_boolean_path_parameter_missing_defaults_to_false() {
    let spec = create_spec_with_required_boolean_path_param();
    let cmd = generate_command_tree_with_flags(&spec, false);

    // Boolean path parameters are always optional, even if OpenAPI spec marks them as required
    // When flag is omitted, it defaults to false
    let result = cmd.try_get_matches_from(vec!["api", "items", "get-item", "--id", "123"]);

    assert!(
        result.is_ok(),
        "Boolean path parameters should be optional regardless of OpenAPI required field: {:?}",
        result.err()
    );

    let matches = result.unwrap();
    let (_, sub_matches) = matches.subcommand().unwrap();
    let (_, operation_matches) = sub_matches.subcommand().unwrap();

    // When flag is not provided, get_flag returns false
    assert!(
        !operation_matches.get_flag("active"),
        "Boolean path parameter should be false when flag not provided"
    );
}

#[test]
fn test_required_boolean_path_parameter_with_flag_succeeds() {
    let spec = create_spec_with_required_boolean_path_param();
    let cmd = generate_command_tree_with_flags(&spec, false);

    // Provide the required boolean flag - should SUCCEED
    let result =
        cmd.try_get_matches_from(vec!["api", "items", "get-item", "--id", "123", "--active"]);

    assert!(
        result.is_ok(),
        "Required boolean path parameter should succeed when provided: {:?}",
        result.err()
    );

    let matches = result.unwrap();
    let (_, sub_matches) = matches.subcommand().unwrap();
    let (_, operation_matches) = sub_matches.subcommand().unwrap();

    assert!(
        operation_matches.get_flag("active"),
        "Boolean flag should be true when provided"
    );
}

#[test]
fn test_required_boolean_query_parameter_missing_errors() {
    let spec = create_spec_with_required_boolean_query_param();
    let cmd = generate_command_tree_with_flags(&spec, false);

    // Try WITHOUT the required boolean flag - should ERROR
    let result = cmd.try_get_matches_from(vec!["api", "users", "list-users"]);

    assert!(
        result.is_err(),
        "Required boolean query parameter should error when missing"
    );

    let err = result.unwrap_err();
    let err_str = err.to_string();
    assert!(
        err_str.contains("required") || err_str.contains("include-inactive"),
        "Error message should mention the required parameter: {err_str}"
    );
}

#[test]
fn test_required_boolean_query_parameter_with_flag_succeeds() {
    let spec = create_spec_with_required_boolean_query_param();
    let cmd = generate_command_tree_with_flags(&spec, false);

    // Provide the required boolean flag - should SUCCEED
    let result = cmd.try_get_matches_from(vec!["api", "users", "list-users", "--include-inactive"]);

    assert!(
        result.is_ok(),
        "Required boolean query parameter should succeed when provided: {:?}",
        result.err()
    );

    let matches = result.unwrap();
    let (_, sub_matches) = matches.subcommand().unwrap();
    let (_, operation_matches) = sub_matches.subcommand().unwrap();

    assert!(
        operation_matches.get_flag("includeInactive"),
        "Boolean flag should be true when provided"
    );
}

#[test]
fn test_mixed_required_and_optional_booleans() {
    let spec = create_spec_with_mixed_boolean_params();
    let cmd = generate_command_tree_with_flags(&spec, false);

    // Missing required flag should error
    let result =
        cmd.clone()
            .try_get_matches_from(vec!["api", "search", "search", "--optional-flag"]);
    assert!(
        result.is_err(),
        "Should error when required boolean is missing"
    );

    // With required flag, optional can be omitted
    let result =
        cmd.clone()
            .try_get_matches_from(vec!["api", "search", "search", "--required-flag"]);
    assert!(
        result.is_ok(),
        "Should succeed with required flag even if optional is omitted"
    );

    // Both flags provided should work
    let result = cmd.try_get_matches_from(vec![
        "api",
        "search",
        "search",
        "--required-flag",
        "--optional-flag",
    ]);
    assert!(result.is_ok(), "Should succeed with both flags");

    let matches = result.unwrap();
    let (_, sub_matches) = matches.subcommand().unwrap();
    let (_, operation_matches) = sub_matches.subcommand().unwrap();

    assert!(
        operation_matches.get_flag("required-flag"),
        "Required flag should be true"
    );
    assert!(
        operation_matches.get_flag("optional-flag"),
        "Optional flag should be true"
    );
}

#[test]
fn test_optional_boolean_defaults_to_false_when_absent() {
    let spec = create_spec_with_mixed_boolean_params();
    let cmd = generate_command_tree_with_flags(&spec, false);

    // Only provide required flag, omit optional
    let result = cmd.try_get_matches_from(vec!["api", "search", "search", "--required-flag"]);
    assert!(result.is_ok());

    let matches = result.unwrap();
    let (_, sub_matches) = matches.subcommand().unwrap();
    let (_, operation_matches) = sub_matches.subcommand().unwrap();

    assert!(
        operation_matches.get_flag("required-flag"),
        "Required flag should be true"
    );
    assert!(
        !operation_matches.get_flag("optional-flag"),
        "Optional flag should default to false when not provided"
    );
}

#[tokio::test]
async fn test_required_boolean_path_param_url_substitution() {
    let mock_server = MockServer::start().await;

    // Test Case 1: When the flag is provided, expect "true" in the URL
    Mock::given(method("GET"))
        .and(path("/items/123/true"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "id": "123",
            "active": true
        })))
        .expect(1)
        .mount(&mock_server)
        .await;

    let mut spec = create_spec_with_required_boolean_path_param();
    spec.base_url = Some(mock_server.uri());

    let cmd = generate_command_tree_with_flags(&spec, false);
    let matches = cmd
        .try_get_matches_from(vec!["api", "items", "get-item", "--id", "123", "--active"])
        .expect("Command should parse");

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
        false,
        None, // retry_context
    )
    .await;

    assert!(
        result.is_ok(),
        "Request should succeed with boolean=true in URL: {:?}",
        result.err()
    );

    // Test Case 2: When the flag is NOT provided, expect "false" in the URL
    Mock::given(method("GET"))
        .and(path("/items/456/false"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "id": "456",
            "active": false
        })))
        .expect(1)
        .mount(&mock_server)
        .await;

    let cmd2 = generate_command_tree_with_flags(&spec, false);
    let matches2 = cmd2
        .try_get_matches_from(vec!["api", "items", "get-item", "--id", "456"])
        .expect("Command should parse without --active flag");

    let result2 = execute_request(
        &spec,
        &matches2,
        None,
        false,
        None,
        None,
        &OutputFormat::Json,
        None,
        None,
        false,
        None, // retry_context
    )
    .await;

    assert!(
        result2.is_ok(),
        "Request should succeed with boolean=false in URL: {:?}",
        result2.err()
    );
}

#[tokio::test]
async fn test_required_boolean_query_param_adds_to_query_string() {
    let mock_server = MockServer::start().await;

    // When the flag is provided, expect ?includeInactive=true
    Mock::given(method("GET"))
        .and(path("/users"))
        .and(wiremock::matchers::query_param("includeInactive", "true"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "users": []
        })))
        .expect(1)
        .mount(&mock_server)
        .await;

    let mut spec = create_spec_with_required_boolean_query_param();
    spec.base_url = Some(mock_server.uri());

    let cmd = generate_command_tree_with_flags(&spec, false);
    let matches = cmd
        .try_get_matches_from(vec!["api", "users", "list-users", "--include-inactive"])
        .expect("Command should parse");

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
        false,
        None, // retry_context
    )
    .await;

    assert!(
        result.is_ok(),
        "Request should succeed with includeInactive=true in query string: {:?}",
        result.err()
    );
}
