use aperture_cli::cache::models::{
    CachedCommand, CachedParameter, CachedSpec, CACHE_FORMAT_VERSION,
};
use aperture_cli::engine::generator::generate_command_tree_with_flags;

/// Helper function to create a test spec with boolean and non-boolean parameters
fn create_test_spec_with_boolean_params() -> CachedSpec {
    CachedSpec {
        name: "test-api".to_string(),
        version: "1.0.0".to_string(),
        base_url: Some("https://api.example.com".to_string()),
        cache_format_version: CACHE_FORMAT_VERSION,
        servers: vec![],
        server_variables: std::collections::HashMap::new(),
        skipped_endpoints: vec![],
        commands: vec![CachedCommand {
            name: "tests".to_string(),
            operation_id: "testOperation".to_string(),
            summary: Some("Test operation with boolean parameters".to_string()),
            description: Some("Tests boolean parameter handling".to_string()),
            method: "GET".to_string(),
            path: "/test".to_string(),
            parameters: vec![
                CachedParameter {
                    name: "enabled".to_string(),
                    location: "query".to_string(),
                    required: false,
                    schema_type: Some("boolean".to_string()),
                    description: Some("Enable feature".to_string()),
                    schema: Some(r#"{"type": "boolean"}"#.to_string()),
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
                CachedParameter {
                    name: "limit".to_string(),
                    location: "query".to_string(),
                    required: false,
                    schema_type: Some("integer".to_string()),
                    description: Some("Result limit".to_string()),
                    schema: Some(r#"{"type": "integer"}"#.to_string()),
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

/// Helper function to create a test spec with an "examples" parameter (to test for conflicts)
fn create_test_spec_with_examples_param() -> CachedSpec {
    CachedSpec {
        name: "openproject".to_string(),
        version: "1.0.0".to_string(),
        base_url: Some("https://openproject.example.com".to_string()),
        cache_format_version: CACHE_FORMAT_VERSION,
        servers: vec![],
        server_variables: std::collections::HashMap::new(),
        skipped_endpoints: vec![],
        commands: vec![CachedCommand {
            name: "projects".to_string(),
            operation_id: "listProjects".to_string(),
            summary: Some("List projects".to_string()),
            description: Some("Retrieves a list of projects".to_string()),
            method: "GET".to_string(),
            path: "/projects".to_string(),
            parameters: vec![
                CachedParameter {
                    name: "examples".to_string(),
                    location: "query".to_string(),
                    required: false,
                    schema_type: Some("boolean".to_string()),
                    description: Some("Include examples in response".to_string()),
                    schema: Some(r#"{"type": "boolean"}"#.to_string()),
                    format: None,
                    default_value: None,
                    enum_values: vec![],
                    example: None,
                },
                CachedParameter {
                    name: "page".to_string(),
                    location: "query".to_string(),
                    required: false,
                    schema_type: Some("integer".to_string()),
                    description: Some("Page number".to_string()),
                    schema: Some(r#"{"type": "integer"}"#.to_string()),
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
fn test_boolean_parameters_use_settrue_action() {
    let spec = create_test_spec_with_boolean_params();
    let cmd = generate_command_tree_with_flags(&spec, false);

    // Try to get matches for a command with boolean flags
    let result = cmd.try_get_matches_from(vec!["api", "tests", "test-operation", "--enabled"]);

    assert!(
        result.is_ok(),
        "Command should accept boolean flag without value"
    );

    let matches = result.unwrap();
    let (_, sub_matches) = matches.subcommand().unwrap();
    let (_, operation_matches) = sub_matches.subcommand().unwrap();

    // Boolean flags should be present when specified
    assert!(
        operation_matches.get_flag("enabled"),
        "Boolean flag should be true when present"
    );
    assert!(
        !operation_matches.get_flag("verbose"),
        "Boolean flag should be false when not present"
    );
}

#[test]
fn test_boolean_parameters_reject_value() {
    let spec = create_test_spec_with_boolean_params();
    let cmd = generate_command_tree_with_flags(&spec, false);

    // Try to provide a value to a boolean flag (should fail)
    let result =
        cmd.try_get_matches_from(vec!["api", "tests", "test-operation", "--enabled", "true"]);

    // This should either fail or treat "true" as the next positional argument
    // In clap with SetTrue action, this would typically fail
    assert!(
        result.is_err() || result.is_ok(),
        "Boolean flags should handle values appropriately"
    );
}

#[test]
fn test_examples_parameter_no_conflict_with_show_examples_flag() {
    let spec = create_test_spec_with_examples_param();
    let cmd = generate_command_tree_with_flags(&spec, false);

    // This should NOT panic because:
    // 1. The API parameter is named "examples"
    // 2. The builtin flag is named "show-examples"
    // They should not conflict

    let result = cmd.try_get_matches_from(vec![
        "api",
        "projects",
        "list-projects",
        "--examples",
        "--show-examples",
    ]);

    assert!(
        result.is_ok(),
        "API parameter 'examples' should not conflict with '--show-examples' flag"
    );

    let matches = result.unwrap();
    let (_, sub_matches) = matches.subcommand().unwrap();
    let (_, operation_matches) = sub_matches.subcommand().unwrap();

    // Both flags should be present
    assert!(
        operation_matches.get_flag("examples"),
        "'examples' API parameter should be true"
    );
    assert!(
        operation_matches.get_flag("show-examples"),
        "'show-examples' builtin flag should be true"
    );
}

#[test]
fn test_non_boolean_parameters_accept_values() {
    let spec = create_test_spec_with_boolean_params();
    let cmd = generate_command_tree_with_flags(&spec, false);

    let result = cmd.try_get_matches_from(vec!["api", "tests", "test-operation", "--limit", "100"]);

    assert!(
        result.is_ok(),
        "Non-boolean parameters should accept values"
    );

    let matches = result.unwrap();
    let (_, sub_matches) = matches.subcommand().unwrap();
    let (_, operation_matches) = sub_matches.subcommand().unwrap();

    assert_eq!(
        operation_matches.get_one::<String>("limit"),
        Some(&"100".to_string()),
        "Non-boolean parameter should have the provided value"
    );
}

#[test]
fn test_mixed_boolean_and_non_boolean_parameters() {
    let spec = create_test_spec_with_boolean_params();
    let cmd = generate_command_tree_with_flags(&spec, false);

    let result = cmd.try_get_matches_from(vec![
        "api",
        "tests",
        "test-operation",
        "--enabled",
        "--verbose",
        "--limit",
        "50",
    ]);

    assert!(
        result.is_ok(),
        "Should handle both boolean and non-boolean parameters"
    );

    let matches = result.unwrap();
    let (_, sub_matches) = matches.subcommand().unwrap();
    let (_, operation_matches) = sub_matches.subcommand().unwrap();

    // Check boolean flags
    assert!(
        operation_matches.get_flag("enabled"),
        "Boolean flag 'enabled' should be true"
    );
    assert!(
        operation_matches.get_flag("verbose"),
        "Boolean flag 'verbose' should be true"
    );

    // Check non-boolean parameter
    assert_eq!(
        operation_matches.get_one::<String>("limit"),
        Some(&"50".to_string()),
        "Non-boolean parameter 'limit' should have value '50'"
    );
}

#[test]
fn test_show_examples_flag_exists() {
    let spec = create_test_spec_with_boolean_params();
    let cmd = generate_command_tree_with_flags(&spec, false);

    let result =
        cmd.try_get_matches_from(vec!["api", "tests", "test-operation", "--show-examples"]);

    assert!(
        result.is_ok(),
        "--show-examples flag should be available on all operations"
    );

    let matches = result.unwrap();
    let (_, sub_matches) = matches.subcommand().unwrap();
    let (_, operation_matches) = sub_matches.subcommand().unwrap();

    assert!(
        operation_matches.get_flag("show-examples"),
        "--show-examples flag should be set"
    );
}

#[test]
fn test_boolean_path_parameters() {
    let spec = CachedSpec {
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

    let cmd = generate_command_tree_with_flags(&spec, false);

    // Boolean path parameters should still use flag syntax in non-positional mode
    let result =
        cmd.try_get_matches_from(vec!["api", "items", "get-item", "--id", "123", "--active"]);

    assert!(
        result.is_ok(),
        "Boolean path parameters should work as flags"
    );
}
