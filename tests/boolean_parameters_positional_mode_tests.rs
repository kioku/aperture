/// Integration tests for boolean parameter handling in positional args mode
///
/// This test suite verifies that boolean parameters work correctly when --positional-args
/// flag is used. Boolean parameters must remain as flags even in positional mode to avoid
/// clap panic when executor reads them via get_flag().
///
/// Key behaviors tested:
/// 1. Boolean path parameters remain as flags (not positional) even with --positional-args
/// 2. Non-boolean path parameters become positional arguments as expected
/// 3. Boolean query/header parameters remain as flags
/// 4. Mixed boolean and non-boolean parameters work together
/// 5. URL substitution works correctly with boolean flags in positional mode
use aperture_cli::cache::models::{
    CachedCommand, CachedParameter, CachedSpec, CACHE_FORMAT_VERSION,
};
use aperture_cli::cli::OutputFormat;
use aperture_cli::engine::executor::execute_request;
use aperture_cli::engine::generator::generate_command_tree_with_flags;
use wiremock::matchers::{method, path, query_param};
use wiremock::{Mock, MockServer, ResponseTemplate};

/// Create spec with mixed boolean and non-boolean path parameters
fn create_spec_with_mixed_path_params() -> CachedSpec {
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
                    description: Some("Item ID".to_string()),
                    schema: Some(r#"{"type": "string"}"#.to_string()),
                    format: None,
                    default_value: None,
                    enum_values: vec![],
                    example: None,
                },
                CachedParameter {
                    name: "active".to_string(),
                    location: "path".to_string(),
                    required: true,
                    schema_type: Some("boolean".to_string()),
                    description: Some("Active status".to_string()),
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

/// Create spec with boolean query parameter for positional mode testing
fn create_spec_with_boolean_query_param() -> CachedSpec {
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
                    name: "verbose".to_string(),
                    location: "query".to_string(),
                    required: false,
                    schema_type: Some("boolean".to_string()),
                    description: Some("Verbose output".to_string()),
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
fn test_boolean_path_param_remains_flag_in_positional_mode() {
    let spec = create_spec_with_mixed_path_params();
    let cmd = generate_command_tree_with_flags(&spec, true); // use_positional_args = true

    // In positional mode:
    // - Non-boolean path params (id) become positional
    // - Boolean path params (active) remain as flags to avoid clap panic
    let result = cmd.try_get_matches_from(vec![
        "api", "items", "get-item", "123",      // positional: id
        "--active", // flag: boolean path param
    ]);

    assert!(
        result.is_ok(),
        "Boolean path param should work as flag in positional mode: {:?}",
        result.err()
    );

    let matches = result.unwrap();
    let (_, sub_matches) = matches.subcommand().unwrap();
    let (_, operation_matches) = sub_matches.subcommand().unwrap();

    // Verify the boolean flag is set
    assert!(
        operation_matches.get_flag("active"),
        "Boolean path parameter should be readable via get_flag()"
    );

    // Verify the positional arg is set
    assert_eq!(
        operation_matches.get_one::<String>("id").unwrap(),
        "123",
        "Non-boolean path parameter should be positional"
    );
}

#[test]
fn test_boolean_path_param_defaults_to_false_in_positional_mode() {
    let spec = create_spec_with_mixed_path_params();
    let cmd = generate_command_tree_with_flags(&spec, true);

    // Omit the boolean flag
    let result = cmd.try_get_matches_from(vec!["api", "items", "get-item", "123"]);

    assert!(
        result.is_ok(),
        "Should succeed without boolean flag in positional mode: {:?}",
        result.err()
    );

    let matches = result.unwrap();
    let (_, sub_matches) = matches.subcommand().unwrap();
    let (_, operation_matches) = sub_matches.subcommand().unwrap();

    // Boolean flag should default to false
    assert!(
        !operation_matches.get_flag("active"),
        "Boolean path parameter should default to false when omitted"
    );
}

#[tokio::test]
async fn test_boolean_path_param_url_substitution_in_positional_mode() {
    let mock_server = MockServer::start().await;

    // Test Case 1: Boolean flag present → "true" in URL
    Mock::given(method("GET"))
        .and(path("/items/123/true"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "id": "123",
            "active": true
        })))
        .expect(1)
        .mount(&mock_server)
        .await;

    let mut spec = create_spec_with_mixed_path_params();
    spec.base_url = Some(mock_server.uri());

    let cmd = generate_command_tree_with_flags(&spec, true); // positional mode
    let matches = cmd
        .try_get_matches_from(vec!["api", "items", "get-item", "123", "--active"])
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
    )
    .await;

    assert!(
        result.is_ok(),
        "Request should succeed with boolean=true in URL: {:?}",
        result.err()
    );

    // Test Case 2: Boolean flag absent → "false" in URL
    Mock::given(method("GET"))
        .and(path("/items/456/false"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "id": "456",
            "active": false
        })))
        .expect(1)
        .mount(&mock_server)
        .await;

    let cmd2 = generate_command_tree_with_flags(&spec, true);
    let matches2 = cmd2
        .try_get_matches_from(vec!["api", "items", "get-item", "456"])
        .expect("Command should parse without --active");

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
    )
    .await;

    assert!(
        result2.is_ok(),
        "Request should succeed with boolean=false in URL: {:?}",
        result2.err()
    );
}

#[test]
fn test_boolean_query_param_remains_flag_in_positional_mode() {
    let spec = create_spec_with_boolean_query_param();
    let cmd = generate_command_tree_with_flags(&spec, true); // positional mode

    // Path param is positional, query param is flag
    let result = cmd.try_get_matches_from(vec![
        "api",
        "users",
        "list-users",
        "123",       // positional: id
        "--verbose", // flag: boolean query param
    ]);

    assert!(
        result.is_ok(),
        "Boolean query param should work as flag in positional mode: {:?}",
        result.err()
    );

    let matches = result.unwrap();
    let (_, sub_matches) = matches.subcommand().unwrap();
    let (_, operation_matches) = sub_matches.subcommand().unwrap();

    assert!(
        operation_matches.get_flag("verbose"),
        "Boolean query parameter should be set"
    );
}

#[tokio::test]
async fn test_boolean_query_param_adds_to_query_string_in_positional_mode() {
    let mock_server = MockServer::start().await;

    Mock::given(method("GET"))
        .and(path("/users/123"))
        .and(query_param("verbose", "true"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "user": {"id": "123"}
        })))
        .expect(1)
        .mount(&mock_server)
        .await;

    let mut spec = create_spec_with_boolean_query_param();
    spec.base_url = Some(mock_server.uri());

    let cmd = generate_command_tree_with_flags(&spec, true); // positional mode
    let matches = cmd
        .try_get_matches_from(vec!["api", "users", "list-users", "123", "--verbose"])
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
    )
    .await;

    assert!(
        result.is_ok(),
        "Request should succeed with verbose=true in query string: {:?}",
        result.err()
    );
}

#[test]
fn test_non_boolean_positional_args_still_work() {
    // Verify that non-boolean parameters still work as positional args
    let spec = create_spec_with_boolean_query_param();
    let cmd = generate_command_tree_with_flags(&spec, true);

    // Just provide the positional arg, no flags
    let result = cmd.try_get_matches_from(vec!["api", "users", "list-users", "999"]);

    assert!(
        result.is_ok(),
        "Non-boolean positional args should work: {:?}",
        result.err()
    );

    let matches = result.unwrap();
    let (_, sub_matches) = matches.subcommand().unwrap();
    let (_, operation_matches) = sub_matches.subcommand().unwrap();

    assert_eq!(
        operation_matches.get_one::<String>("id").unwrap(),
        "999",
        "Positional arg should be parsed correctly"
    );
}

#[test]
fn test_multiple_boolean_path_params_in_positional_mode() {
    // Create spec with multiple boolean path parameters
    let spec = CachedSpec {
        name: "test-api".to_string(),
        version: "1.0.0".to_string(),
        base_url: Some("https://api.example.com".to_string()),
        cache_format_version: CACHE_FORMAT_VERSION,
        servers: vec![],
        server_variables: std::collections::HashMap::new(),
        skipped_endpoints: vec![],
        commands: vec![CachedCommand {
            name: "resources".to_string(),
            operation_id: "getResource".to_string(),
            summary: Some("Get resource".to_string()),
            description: None,
            method: "GET".to_string(),
            path: "/resources/{id}/{active}/{verified}".to_string(),
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
                    required: true,
                    schema_type: Some("boolean".to_string()),
                    description: None,
                    schema: Some(r#"{"type": "boolean"}"#.to_string()),
                    format: None,
                    default_value: None,
                    enum_values: vec![],
                    example: None,
                },
                CachedParameter {
                    name: "verified".to_string(),
                    location: "path".to_string(),
                    required: true,
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
    };

    let cmd = generate_command_tree_with_flags(&spec, true);

    // Test with both boolean flags provided
    let result = cmd.try_get_matches_from(vec![
        "api",
        "resources",
        "get-resource",
        "abc123",     // positional: id
        "--active",   // flag: boolean
        "--verified", // flag: boolean
    ]);

    assert!(
        result.is_ok(),
        "Multiple boolean path params should work in positional mode: {:?}",
        result.err()
    );

    let matches = result.unwrap();
    let (_, sub_matches) = matches.subcommand().unwrap();
    let (_, operation_matches) = sub_matches.subcommand().unwrap();

    assert!(operation_matches.get_flag("active"));
    assert!(operation_matches.get_flag("verified"));
    assert_eq!(operation_matches.get_one::<String>("id").unwrap(), "abc123");
}

#[tokio::test]
async fn test_mixed_boolean_flags_url_substitution_positional_mode() {
    let mock_server = MockServer::start().await;

    // Test different combinations of boolean flags
    Mock::given(method("GET"))
        .and(path("/resources/test1/true/false"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({"ok": true})))
        .expect(1)
        .mount(&mock_server)
        .await;

    let spec = CachedSpec {
        name: "test-api".to_string(),
        version: "1.0.0".to_string(),
        base_url: Some(mock_server.uri()),
        cache_format_version: CACHE_FORMAT_VERSION,
        servers: vec![],
        server_variables: std::collections::HashMap::new(),
        skipped_endpoints: vec![],
        commands: vec![CachedCommand {
            name: "resources".to_string(),
            operation_id: "getResource".to_string(),
            summary: Some("Get resource".to_string()),
            description: None,
            method: "GET".to_string(),
            path: "/resources/{id}/{active}/{verified}".to_string(),
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
                    required: true,
                    schema_type: Some("boolean".to_string()),
                    description: None,
                    schema: Some(r#"{"type": "boolean"}"#.to_string()),
                    format: None,
                    default_value: None,
                    enum_values: vec![],
                    example: None,
                },
                CachedParameter {
                    name: "verified".to_string(),
                    location: "path".to_string(),
                    required: true,
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
    };

    let cmd = generate_command_tree_with_flags(&spec, true);
    let matches = cmd
        .try_get_matches_from(vec![
            "api",
            "resources",
            "get-resource",
            "test1",
            "--active",
            // --verified omitted, should default to false
        ])
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
    )
    .await;

    assert!(
        result.is_ok(),
        "Mixed boolean flags should produce correct URL: {:?}",
        result.err()
    );
}
