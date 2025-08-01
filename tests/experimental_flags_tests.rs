use aperture_cli::cache::models::{CachedCommand, CachedParameter, CachedSpec};
use aperture_cli::engine::generator::{generate_command_tree, generate_command_tree_with_flags};
use std::collections::HashMap;

fn create_test_spec() -> CachedSpec {
    CachedSpec {
        cache_format_version: aperture_cli::cache::models::CACHE_FORMAT_VERSION,
        name: "test-api".to_string(),
        version: "1.0.0".to_string(),
        commands: vec![CachedCommand {
            name: "users".to_string(),
            description: Some("Get user by ID".to_string()),
            summary: None,
            operation_id: "getUserById".to_string(),
            method: "GET".to_string(),
            path: "/users/{id}".to_string(),
            parameters: vec![
                CachedParameter {
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
                },
                CachedParameter {
                    name: "include_profile".to_string(),
                    location: "query".to_string(),
                    required: false,
                    description: Some("Include profile information".to_string()),
                    schema: Some(r#"{"type": "boolean"}"#.to_string()),
                    schema_type: Some("boolean".to_string()),
                    format: None,
                    default_value: None,
                    enum_values: vec![],
                    example: None,
                },
            ],
            request_body: None,
            responses: vec![],
            security_requirements: vec![],
            tags: vec!["users".to_string()],
            deprecated: false,
            external_docs_url: None,
        }],
        base_url: Some("https://api.example.com".to_string()),
        servers: vec!["https://api.example.com".to_string()],
        security_schemes: HashMap::new(),
        skipped_endpoints: vec![],
        server_variables: HashMap::new(),
    }
}

#[test]
fn test_default_flag_based_command_generation() {
    let spec = create_test_spec();
    let command = generate_command_tree(&spec);

    // Test that the command tree is generated correctly
    assert_eq!(command.get_name(), "api");

    // Get the users subcommand
    let users_cmd = command
        .find_subcommand("users")
        .expect("users subcommand should exist");

    // Get the get-user-by-id operation
    let get_user_cmd = users_cmd
        .find_subcommand("get-user-by-id")
        .expect("get-user-by-id subcommand should exist");

    // Check that path parameter is positional (no long flag)
    let id_arg = get_user_cmd
        .get_arguments()
        .find(|arg| arg.get_id() == "id")
        .expect("id argument should exist");
    assert_eq!(
        id_arg.get_long(),
        Some("id"),
        "Path parameter should have long flag in default mode"
    );

    // Check that query parameter has long flag
    let include_profile_arg = get_user_cmd
        .get_arguments()
        .find(|arg| arg.get_id() == "include_profile")
        .expect("include_profile argument should exist");
    assert_eq!(
        include_profile_arg.get_long(),
        Some("include_profile"),
        "Query parameter should have long flag"
    );
}

#[test]
fn test_legacy_positional_command_generation() {
    let spec = create_test_spec();
    let command = generate_command_tree_with_flags(&spec, true);

    // Test that the command tree is generated correctly
    assert_eq!(command.get_name(), "api");

    // Get the users subcommand
    let users_cmd = command
        .find_subcommand("users")
        .expect("users subcommand should exist");

    // Get the get-user-by-id operation
    let get_user_cmd = users_cmd
        .find_subcommand("get-user-by-id")
        .expect("get-user-by-id subcommand should exist");

    // Check that path parameter is positional in legacy mode
    let id_arg = get_user_cmd
        .get_arguments()
        .find(|arg| arg.get_id() == "id")
        .expect("id argument should exist");
    assert!(
        id_arg.get_long().is_none(),
        "Path parameter should not have long flag in legacy positional mode"
    );

    // Check that query parameter still has long flag
    let include_profile_arg = get_user_cmd
        .get_arguments()
        .find(|arg| arg.get_id() == "include_profile")
        .expect("include_profile argument should exist");
    assert_eq!(
        include_profile_arg.get_long(),
        Some("include_profile"),
        "Query parameter should still have long flag"
    );
}

#[test]
fn test_legacy_positional_help_text() {
    let spec = create_test_spec();
    let command = generate_command_tree_with_flags(&spec, true);

    // Get the users subcommand
    let users_cmd = command
        .find_subcommand("users")
        .expect("users subcommand should exist");

    // Get the get-user-by-id operation
    let get_user_cmd = users_cmd
        .find_subcommand("get-user-by-id")
        .expect("get-user-by-id subcommand should exist");

    // Check that path parameter has appropriate help text
    let id_arg = get_user_cmd
        .get_arguments()
        .find(|arg| arg.get_id() == "id")
        .expect("id argument should exist");
    let help_text = id_arg
        .get_help()
        .expect("id argument should have help text");
    assert!(
        help_text.to_string().contains("id parameter"),
        "Path parameter should have descriptive help text"
    );
}

#[test]
fn test_backwards_compatibility() {
    let spec = create_test_spec();

    // Generate command tree without experimental flags
    let normal_command = generate_command_tree(&spec);
    let explicit_normal_command = generate_command_tree_with_flags(&spec, false);

    // Both should produce the same structure
    assert_eq!(
        normal_command.get_name(),
        explicit_normal_command.get_name()
    );

    // Both should have users subcommand
    assert!(normal_command.find_subcommand("users").is_some());
    assert!(explicit_normal_command.find_subcommand("users").is_some());
}
