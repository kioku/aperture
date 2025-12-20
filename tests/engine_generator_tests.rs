use aperture_cli::cache::models::{
    CachedCommand, CachedParameter, CachedRequestBody, CachedResponse, CachedSpec,
};
use aperture_cli::constants;
use aperture_cli::engine::generator::generate_command_tree;
use std::collections::HashMap;

// Helper macros for creating test data with default values
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
    ($name:expr, $location:expr, $required:expr, $schema:expr) => {
        CachedParameter {
            name: $name.to_string(),
            location: $location.to_string(),
            required: $required,
            description: None,
            schema: Some($schema.to_string()),
            schema_type: Some("string".to_string()),
            format: None,
            default_value: None,
            enum_values: vec![],
            example: None,
        }
    };
}

macro_rules! cached_response {
    ($status:expr) => {
        CachedResponse {
            status_code: $status.to_string(),
            description: None,
            content_type: Some(constants::CONTENT_TYPE_JSON.to_string()),
            schema: Some(r#"{"type": "object"}"#.to_string()),
            example: None,
        }
    };
}

macro_rules! cached_request_body {
    () => {
        CachedRequestBody {
            content_type: constants::CONTENT_TYPE_JSON.to_string(),
            schema: r#"{"type": "object"}"#.to_string(),
            required: true,
            description: None,
            example: None,
        }
    };
    ($required:expr) => {
        CachedRequestBody {
            content_type: constants::CONTENT_TYPE_JSON.to_string(),
            schema: r#"{"type": "object"}"#.to_string(),
            required: $required,
            description: None,
            example: None,
        }
    };
}

macro_rules! cached_command {
    ($name:expr, $op_id:expr, $method:expr, $path:expr, $params:expr, $body:expr, $responses:expr) => {
        CachedCommand {
            name: $name.to_string(),
            description: None,
            summary: None,
            operation_id: $op_id.to_string(),
            method: $method.to_string(),
            path: $path.to_string(),
            parameters: $params,
            request_body: $body,
            responses: $responses,
            security_requirements: vec![],
            tags: vec![$name.to_string()],
            deprecated: false,
            external_docs_url: None,
            examples: vec![],
        }
    };
}

fn create_test_spec() -> CachedSpec {
    CachedSpec {
        cache_format_version: aperture_cli::cache::models::CACHE_FORMAT_VERSION,
        name: "test-api".to_string(),
        version: "1.0.0".to_string(),
        commands: vec![
            {
                let mut cmd = cached_command!(
                    "users",
                    "getUserById",
                    "GET",
                    "/users/{id}",
                    vec![
                        cached_parameter!("id", "path", true),
                        cached_parameter!("include", "query", false),
                        cached_parameter!("x-request-id", "header", false),
                    ],
                    None,
                    vec![cached_response!("200")]
                );
                cmd.description = Some("Get user by ID".to_string());
                cmd
            },
            cached_command!(
                "users",
                "createUser",
                "POST",
                "/users",
                vec![],
                Some(cached_request_body!()),
                vec![cached_response!("201")]
            ),
        ],
        base_url: Some("https://api.example.com".to_string()),
        servers: vec!["https://api.example.com".to_string()],
        security_schemes: HashMap::new(),
        skipped_endpoints: vec![],
        server_variables: HashMap::new(),
    }
}

#[test]
fn test_generate_command_tree_structure() {
    let spec = create_test_spec();
    let command = generate_command_tree(&spec);

    // Check root command properties
    assert_eq!(command.get_name(), "api");
    assert_eq!(command.get_version(), Some("1.0.0"));

    // Check that subcommands exist based on tags
    let subcommands: Vec<_> = command.get_subcommands().collect();
    assert_eq!(subcommands.len(), 1); // Only one group: "users"

    // Find the users group
    let users_group = subcommands
        .iter()
        .find(|cmd| cmd.get_name() == "users")
        .unwrap();

    // Check the users group has both operations
    let user_operations: Vec<_> = users_group.get_subcommands().collect();
    assert_eq!(user_operations.len(), 2);

    let operation_names: Vec<&str> = user_operations.iter().map(|cmd| cmd.get_name()).collect();
    assert!(operation_names.contains(&"get-user-by-id"));
    assert!(operation_names.contains(&"create-user"));
}

#[test]
fn test_generate_command_basic_functionality() {
    let spec = create_test_spec();
    let command = generate_command_tree(&spec);

    // Basic smoke test to ensure the command can be built
    assert!(!command.get_name().is_empty());
    assert!(command.get_subcommands().count() > 0);
}

#[test]
fn test_server_var_flag_present() {
    let spec = create_test_spec();
    let command = generate_command_tree(&spec);

    // Check that the --server-var global flag is present
    // Try to parse with the --server-var flag to see if it's accepted
    let result = command.try_get_matches_from(vec!["api", "--server-var", "region=us", "--help"]);

    // Should not fail due to unknown argument
    match result {
        Err(e) if e.kind() == clap::error::ErrorKind::UnknownArgument => {
            panic!("--server-var flag not recognized: {e}");
        }
        _ => {
            // Expected: either success or help display, but not unknown argument error
        }
    }
}

#[test]
fn test_kebab_case_conversion() {
    let spec = CachedSpec {
        cache_format_version: aperture_cli::cache::models::CACHE_FORMAT_VERSION,
        name: "test-api".to_string(),
        version: "1.0.0".to_string(),
        commands: vec![{
            let mut cmd = cached_command!(
                "users",
                "getUserProfile",
                "GET",
                "/users/{id}/profile",
                vec![],
                None,
                vec![]
            );
            cmd.description = Some("User operations".to_string());
            cmd
        }],
        base_url: Some("https://api.example.com".to_string()),
        servers: vec!["https://api.example.com".to_string()],
        security_schemes: HashMap::new(),
        skipped_endpoints: vec![],
        server_variables: HashMap::new(),
    };

    let command = generate_command_tree(&spec);
    let users_group = command.get_subcommands().next().unwrap();
    let operation = users_group.get_subcommands().next().unwrap();

    // Should convert getUserProfile to get-user-profile
    assert_eq!(operation.get_name(), "get-user-profile");
}

#[test]
fn test_parameter_generation() {
    let spec = create_test_spec();
    let command = generate_command_tree(&spec);

    // Find the get-user operation
    let users_group = command
        .get_subcommands()
        .find(|cmd| cmd.get_name() == "users")
        .unwrap();
    let get_user_op = users_group
        .get_subcommands()
        .find(|cmd| cmd.get_name() == "get-user-by-id")
        .unwrap();

    // Check arguments were created
    let args: Vec<_> = get_user_op.get_arguments().collect();
    assert!(args.len() >= 3); // id (path), include (query), x-request-id (header)

    // Check specific arguments
    assert!(args.iter().any(|arg| arg.get_id() == "id"));
    assert!(args.iter().any(|arg| arg.get_id() == "include"));
    assert!(args.iter().any(|arg| arg.get_id() == "x-request-id"));
}

#[test]
fn test_fallback_to_default_tag() {
    let spec = CachedSpec {
        cache_format_version: aperture_cli::cache::models::CACHE_FORMAT_VERSION,
        name: "test-api".to_string(),
        version: "1.0.0".to_string(),
        commands: vec![{
            let mut cmd = cached_command!(
                "", // Empty tag should fallback to "default"
                "testOp",
                "GET",
                "/test",
                vec![],
                None,
                vec![]
            );
            cmd.description = Some("Test operation".to_string());
            cmd
        }],
        base_url: Some("https://api.example.com".to_string()),
        servers: vec!["https://api.example.com".to_string()],
        security_schemes: HashMap::new(),
        skipped_endpoints: vec![],
        server_variables: HashMap::new(),
    };

    let command = generate_command_tree(&spec);
    let subcommands: Vec<_> = command.get_subcommands().collect();

    assert_eq!(subcommands.len(), 1);
    assert_eq!(subcommands[0].get_name(), "default");
}

#[test]
fn test_fallback_to_http_method() {
    let spec = CachedSpec {
        cache_format_version: aperture_cli::cache::models::CACHE_FORMAT_VERSION,
        name: "test-api".to_string(),
        version: "1.0.0".to_string(),
        commands: vec![{
            let mut cmd = cached_command!(
                "ops",
                "", // Empty operationId should fallback to method
                "POST",
                "/test",
                vec![],
                None,
                vec![]
            );
            cmd.description = Some("Test operation".to_string());
            cmd
        }],
        base_url: Some("https://api.example.com".to_string()),
        servers: vec!["https://api.example.com".to_string()],
        security_schemes: HashMap::new(),
        skipped_endpoints: vec![],
        server_variables: HashMap::new(),
    };

    let command = generate_command_tree(&spec);
    let ops_group = command.get_subcommands().next().unwrap();
    let operation = ops_group.get_subcommands().next().unwrap();

    // Should use lowercase HTTP method as fallback
    assert_eq!(operation.get_name(), "post");
}
