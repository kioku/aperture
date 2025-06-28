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
                name: "get-user".to_string(),
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
                name: "create-user".to_string(),
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
    }
}

#[test]
fn test_generate_command_tree_structure() {
    let spec = create_test_spec();
    let command = generate_command_tree(&spec);

    // Check root command properties
    assert_eq!(command.get_name(), "api");
    assert_eq!(command.get_version(), Some("1.0.0"));

    // Check that subcommands exist
    let subcommands: Vec<_> = command.get_subcommands().collect();
    assert_eq!(subcommands.len(), 1);

    let default_group = subcommands.first().unwrap();
    assert_eq!(default_group.get_name(), "default");

    // Check operation subcommands
    let operations: Vec<_> = default_group.get_subcommands().collect();
    assert_eq!(operations.len(), 2);

    let operation_names: Vec<&str> = operations.iter().map(|cmd| cmd.get_name()).collect();
    assert!(operation_names.contains(&"get-user"));
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
