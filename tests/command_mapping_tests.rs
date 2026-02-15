//! Integration tests for custom command mapping (issue #73).
//!
//! Verifies that group renames, operation renames, aliases, and hidden flags
//! applied via `CommandMapping` correctly flow through to the generated CLI
//! command tree.

use aperture_cli::cache::models::{CachedCommand, CachedResponse, CachedSpec};
use aperture_cli::config::mapping::apply_command_mapping;
use aperture_cli::config::models::{CommandMapping, OperationMapping};
use aperture_cli::constants;
use aperture_cli::engine::generator::generate_command_tree;
use std::collections::HashMap;

// HashMap used for make_spec's security_schemes

fn make_command(tag: &str, operation_id: &str, method: &str, path: &str) -> CachedCommand {
    CachedCommand {
        name: tag.to_string(),
        description: Some(format!("Operation {operation_id}")),
        summary: None,
        operation_id: operation_id.to_string(),
        method: method.to_string(),
        path: path.to_string(),
        parameters: vec![],
        request_body: None,
        responses: vec![CachedResponse {
            status_code: "200".to_string(),
            description: None,
            content_type: Some(constants::CONTENT_TYPE_JSON.to_string()),
            schema: Some(r#"{"type": "object"}"#.to_string()),
            example: None,
        }],
        security_requirements: vec![],
        tags: vec![tag.to_string()],
        deprecated: false,
        external_docs_url: None,
        examples: vec![],
        display_group: None,
        display_name: None,
        aliases: vec![],
        hidden: false,
    }
}

fn make_spec(commands: Vec<CachedCommand>) -> CachedSpec {
    CachedSpec {
        cache_format_version: aperture_cli::cache::models::CACHE_FORMAT_VERSION,
        name: "test-api".to_string(),
        base_url: Some("https://api.example.com".to_string()),
        version: "1.0.0".to_string(),
        security_schemes: HashMap::new(),
        servers: vec!["https://api.example.com".to_string()],
        skipped_endpoints: vec![],
        server_variables: HashMap::new(),
        commands,
    }
}

#[test]
fn test_group_rename_flows_to_command_tree() {
    let mut commands = vec![
        make_command("User Management", "getUser", "GET", "/users/{id}"),
        make_command("User Management", "createUser", "POST", "/users"),
    ];

    let mapping = CommandMapping {
        groups: HashMap::from([("User Management".to_string(), "users".to_string())]),
        operations: HashMap::new(),
    };

    let result = apply_command_mapping(&mut commands, &mapping).unwrap();
    assert!(result.warnings.is_empty());

    let spec = make_spec(commands);
    let cmd = generate_command_tree(&spec);

    // The group should be "users" not "user-management"
    let subcommands: Vec<String> = cmd
        .get_subcommands()
        .map(|s| s.get_name().to_string())
        .collect();
    assert!(
        subcommands.contains(&"users".to_string()),
        "Expected 'users' group, found: {subcommands:?}"
    );
    assert!(
        !subcommands.contains(&"user-management".to_string()),
        "Should not contain old 'user-management' group"
    );
}

#[test]
fn test_operation_rename_flows_to_command_tree() {
    let mut commands = vec![make_command("users", "getUserById", "GET", "/users/{id}")];

    let mapping = CommandMapping {
        groups: HashMap::new(),
        operations: HashMap::from([(
            "getUserById".to_string(),
            OperationMapping {
                name: Some("fetch".to_string()),
                ..Default::default()
            },
        )]),
    };

    apply_command_mapping(&mut commands, &mapping).unwrap();

    let spec = make_spec(commands);
    let cmd = generate_command_tree(&spec);

    // Find the "users" group and check for "fetch" subcommand
    let users_group = cmd
        .get_subcommands()
        .find(|s| s.get_name() == "users")
        .expect("should have 'users' group");

    let sub_names: Vec<String> = users_group
        .get_subcommands()
        .map(|s| s.get_name().to_string())
        .collect();
    assert!(
        sub_names.contains(&"fetch".to_string()),
        "Expected 'fetch' subcommand, found: {sub_names:?}"
    );
    assert!(
        !sub_names.contains(&"get-user-by-id".to_string()),
        "Should not contain old 'get-user-by-id'"
    );
}

#[test]
fn test_aliases_registered_on_command() {
    let mut commands = vec![make_command("users", "getUser", "GET", "/users/{id}")];

    let mapping = CommandMapping {
        groups: HashMap::new(),
        operations: HashMap::from([(
            "getUser".to_string(),
            OperationMapping {
                aliases: vec!["fetch".to_string(), "show".to_string()],
                ..Default::default()
            },
        )]),
    };

    apply_command_mapping(&mut commands, &mapping).unwrap();

    let spec = make_spec(commands);
    let cmd = generate_command_tree(&spec);

    let users_group = cmd
        .get_subcommands()
        .find(|s| s.get_name() == "users")
        .expect("should have 'users' group");

    let get_user_cmd = users_group
        .get_subcommands()
        .find(|s| s.get_name() == "get-user")
        .expect("should have 'get-user' subcommand");

    let visible_aliases: Vec<&str> = get_user_cmd.get_visible_aliases().collect();
    assert!(
        visible_aliases.contains(&"fetch"),
        "Expected 'fetch' alias, found: {visible_aliases:?}"
    );
    assert!(
        visible_aliases.contains(&"show"),
        "Expected 'show' alias, found: {visible_aliases:?}"
    );
}

#[test]
fn test_hidden_command_not_visible() {
    let mut commands = vec![
        make_command("users", "getUser", "GET", "/users/{id}"),
        make_command("users", "deleteUser", "DELETE", "/users/{id}"),
    ];

    let mapping = CommandMapping {
        groups: HashMap::new(),
        operations: HashMap::from([(
            "deleteUser".to_string(),
            OperationMapping {
                hidden: true,
                ..Default::default()
            },
        )]),
    };

    apply_command_mapping(&mut commands, &mapping).unwrap();

    let spec = make_spec(commands);
    let cmd = generate_command_tree(&spec);

    let users_group = cmd
        .get_subcommands()
        .find(|s| s.get_name() == "users")
        .expect("should have 'users' group");

    // The hidden command should still exist but be marked hidden
    let delete_cmd = users_group
        .get_subcommands()
        .find(|s| s.get_name() == "delete-user")
        .expect("hidden command should still exist");

    assert!(
        delete_cmd.is_hide_set(),
        "delete-user should be hidden from help"
    );

    // The visible command should not be hidden
    let get_cmd = users_group
        .get_subcommands()
        .find(|s| s.get_name() == "get-user")
        .expect("visible command should exist");

    assert!(!get_cmd.is_hide_set(), "get-user should not be hidden");
}

#[test]
fn test_operation_group_override_moves_command() {
    let mut commands = vec![
        make_command("users", "getUser", "GET", "/users/{id}"),
        make_command("users", "getUserSettings", "GET", "/users/{id}/settings"),
    ];

    let mapping = CommandMapping {
        groups: HashMap::new(),
        operations: HashMap::from([(
            "getUserSettings".to_string(),
            OperationMapping {
                group: Some("settings".to_string()),
                ..Default::default()
            },
        )]),
    };

    apply_command_mapping(&mut commands, &mapping).unwrap();

    let spec = make_spec(commands);
    let cmd = generate_command_tree(&spec);

    let group_names: Vec<String> = cmd
        .get_subcommands()
        .map(|s| s.get_name().to_string())
        .collect();

    assert!(
        group_names.contains(&"settings".to_string()),
        "Expected 'settings' group, found: {group_names:?}"
    );

    // The settings group should contain getUserSettings
    let settings_group = cmd
        .get_subcommands()
        .find(|s| s.get_name() == "settings")
        .expect("should have 'settings' group");

    let sub_names: Vec<String> = settings_group
        .get_subcommands()
        .map(|s| s.get_name().to_string())
        .collect();
    assert!(
        sub_names.contains(&"get-user-settings".to_string()),
        "Expected 'get-user-settings' in settings group, found: {sub_names:?}"
    );
}

#[test]
fn test_combined_group_and_operation_mapping() {
    let mut commands = vec![
        make_command("User Management", "getUserById", "GET", "/users/{id}"),
        make_command("User Management", "createUser", "POST", "/users"),
        make_command("Organization", "getOrg", "GET", "/orgs/{id}"),
    ];

    let mapping = CommandMapping {
        groups: HashMap::from([
            ("User Management".to_string(), "users".to_string()),
            ("Organization".to_string(), "orgs".to_string()),
        ]),
        operations: HashMap::from([(
            "getUserById".to_string(),
            OperationMapping {
                name: Some("fetch".to_string()),
                aliases: vec!["get".to_string()],
                ..Default::default()
            },
        )]),
    };

    let result = apply_command_mapping(&mut commands, &mapping).unwrap();
    assert!(result.warnings.is_empty());

    let spec = make_spec(commands);
    let cmd = generate_command_tree(&spec);

    // Check group names
    let group_names: Vec<String> = cmd
        .get_subcommands()
        .map(|s| s.get_name().to_string())
        .collect();
    assert!(group_names.contains(&"users".to_string()));
    assert!(group_names.contains(&"orgs".to_string()));

    // Check renamed operation under renamed group
    let users_group = cmd
        .get_subcommands()
        .find(|s| s.get_name() == "users")
        .expect("should have 'users' group");

    let sub_names: Vec<String> = users_group
        .get_subcommands()
        .map(|s| s.get_name().to_string())
        .collect();
    assert!(
        sub_names.contains(&"fetch".to_string()),
        "Expected 'fetch', found: {sub_names:?}"
    );
    assert!(
        sub_names.contains(&"create-user".to_string()),
        "Expected 'create-user', found: {sub_names:?}"
    );
}

#[test]
fn test_collision_detection_blocks_duplicate_names() {
    let mut commands = vec![
        make_command("users", "getUser", "GET", "/users/{id}"),
        make_command("users", "fetchUser", "GET", "/users/fetch/{id}"),
    ];

    let mapping = CommandMapping {
        groups: HashMap::new(),
        operations: HashMap::from([(
            "fetchUser".to_string(),
            OperationMapping {
                name: Some("get-user".to_string()),
                ..Default::default()
            },
        )]),
    };

    let result = apply_command_mapping(&mut commands, &mapping);
    assert!(
        result.is_err(),
        "Should reject duplicate (group, name) pair"
    );
    let err = result.unwrap_err().to_string();
    assert!(
        err.contains("collision"),
        "Error should mention collision: {err}"
    );
}

#[test]
fn test_collision_detection_blocks_alias_vs_name() {
    let mut commands = vec![
        make_command("users", "getUser", "GET", "/users/{id}"),
        make_command("users", "fetchUser", "GET", "/users/fetch/{id}"),
    ];

    let mapping = CommandMapping {
        groups: HashMap::new(),
        operations: HashMap::from([(
            "fetchUser".to_string(),
            OperationMapping {
                aliases: vec!["get-user".to_string()],
                ..Default::default()
            },
        )]),
    };

    let result = apply_command_mapping(&mut commands, &mapping);
    assert!(result.is_err(), "Should reject alias colliding with name");
}

#[test]
fn test_reserved_group_name_rejected() {
    let mut commands = vec![make_command("myapi", "doSomething", "GET", "/something")];

    let mapping = CommandMapping {
        groups: HashMap::from([("myapi".to_string(), "config".to_string())]),
        operations: HashMap::new(),
    };

    let result = apply_command_mapping(&mut commands, &mapping);
    assert!(
        result.is_err(),
        "Should reject reserved group name 'config'"
    );
    let err = result.unwrap_err().to_string();
    assert!(
        err.contains("config"),
        "Error should mention 'config': {err}"
    );
}

#[test]
fn test_stale_mappings_produce_warnings_not_errors() {
    let mut commands = vec![make_command("users", "getUser", "GET", "/users/{id}")];

    let mapping = CommandMapping {
        groups: HashMap::from([("NonExistentTag".to_string(), "nope".to_string())]),
        operations: HashMap::from([(
            "nonExistentOp".to_string(),
            OperationMapping {
                name: Some("gone".to_string()),
                ..Default::default()
            },
        )]),
    };

    let result = apply_command_mapping(&mut commands, &mapping).unwrap();
    assert_eq!(
        result.warnings.len(),
        2,
        "Expected 2 stale warnings, got: {:?}",
        result.warnings
    );
}
