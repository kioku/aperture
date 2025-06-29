use aperture::cache::models::{
    CachedCommand, CachedParameter, CachedRequestBody, CachedResponse, CachedSpec,
};
use aperture::engine::generator::generate_command_tree;

fn create_test_spec() -> CachedSpec {
    CachedSpec {
        name: "test-api".to_string(),
        version: "1.0.0".to_string(),
        commands: vec![
            CachedCommand {
                name: "users".to_string(), // This is the tag/group name
                description: Some("Get user by ID".to_string()),
                operation_id: "getUserById".to_string(),
                method: "GET".to_string(),
                path: "/users/{id}".to_string(),
                parameters: vec![
                    CachedParameter {
                        name: "id".to_string(),
                        location: "path".to_string(),
                        required: true,
                        schema: Some(r#"{"type": "string"}"#.to_string()),
                    },
                    CachedParameter {
                        name: "include".to_string(),
                        location: "query".to_string(),
                        required: false,
                        schema: Some(r#"{"type": "string"}"#.to_string()),
                    },
                    CachedParameter {
                        name: "x-request-id".to_string(),
                        location: "header".to_string(),
                        required: false,
                        schema: None,
                    },
                ],
                request_body: None,
                responses: vec![CachedResponse {
                    status_code: "200".to_string(),
                    content: Some(r#"{"type": "object"}"#.to_string()),
                }],
            },
            CachedCommand {
                name: "users".to_string(), // Same tag/group
                description: None,
                operation_id: "createUser".to_string(),
                method: "POST".to_string(),
                path: "/users".to_string(),
                parameters: vec![],
                request_body: Some(CachedRequestBody {
                    content: "application/json".to_string(),
                    required: true,
                }),
                responses: vec![CachedResponse {
                    status_code: "201".to_string(),
                    content: None,
                }],
            },
        ],
        base_url: Some("https://api.example.com".to_string()),
        servers: vec!["https://api.example.com".to_string()],
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
fn test_kebab_case_conversion() {
    let spec = CachedSpec {
        name: "test-api".to_string(),
        version: "1.0.0".to_string(),
        commands: vec![CachedCommand {
            name: "users".to_string(),
            description: Some("User operations".to_string()),
            operation_id: "getUserProfile".to_string(),
            method: "GET".to_string(),
            path: "/users/{id}/profile".to_string(),
            parameters: vec![],
            request_body: None,
            responses: vec![],
        }],
        base_url: Some("https://api.example.com".to_string()),
        servers: vec!["https://api.example.com".to_string()],
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
        name: "test-api".to_string(),
        version: "1.0.0".to_string(),
        commands: vec![CachedCommand {
            name: "".to_string(), // Empty tag should fallback to "default"
            description: Some("Test operation".to_string()),
            operation_id: "testOp".to_string(),
            method: "GET".to_string(),
            path: "/test".to_string(),
            parameters: vec![],
            request_body: None,
            responses: vec![],
        }],
        base_url: Some("https://api.example.com".to_string()),
        servers: vec!["https://api.example.com".to_string()],
    };

    let command = generate_command_tree(&spec);
    let subcommands: Vec<_> = command.get_subcommands().collect();

    assert_eq!(subcommands.len(), 1);
    assert_eq!(subcommands[0].get_name(), "default");
}

#[test]
fn test_fallback_to_http_method() {
    let spec = CachedSpec {
        name: "test-api".to_string(),
        version: "1.0.0".to_string(),
        commands: vec![CachedCommand {
            name: "ops".to_string(),
            description: Some("Test operation".to_string()),
            operation_id: "".to_string(), // Empty operationId should fallback to method
            method: "POST".to_string(),
            path: "/test".to_string(),
            parameters: vec![],
            request_body: None,
            responses: vec![],
        }],
        base_url: Some("https://api.example.com".to_string()),
        servers: vec!["https://api.example.com".to_string()],
    };

    let command = generate_command_tree(&spec);
    let ops_group = command.get_subcommands().next().unwrap();
    let operation = ops_group.get_subcommands().next().unwrap();

    // Should use lowercase HTTP method as fallback
    assert_eq!(operation.get_name(), "post");
}
