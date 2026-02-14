// These lints are overly pedantic for test code
#![allow(clippy::missing_panics_doc)]
#![allow(clippy::used_underscore_binding)]

use aperture_cli::cache::models::{
    CachedCommand, CachedParameter, CachedRequestBody, CachedResponse, CachedSpec, CommandExample,
};
use aperture_cli::docs::{DocumentationGenerator, HelpFormatter};
use std::collections::{BTreeMap, HashMap};

fn create_test_spec() -> CachedSpec {
    CachedSpec {
        cache_format_version: aperture_cli::cache::models::CACHE_FORMAT_VERSION,
        name: "TestAPI".to_string(),
        version: "1.0.0".to_string(),
        base_url: Some("https://api.test.com".to_string()),
        servers: vec!["https://api.test.com".to_string()],
        security_schemes: HashMap::new(),
        skipped_endpoints: vec![],
        server_variables: HashMap::new(),
        commands: vec![
            CachedCommand {
                name: "get-user".to_string(),
                operation_id: "getUserById".to_string(),
                method: "GET".to_string(),
                path: "/users/{id}".to_string(),
                summary: Some("Get user by ID".to_string()),
                description: Some("Retrieve detailed information about a specific user by their unique identifier.".to_string()),
                parameters: vec![
                    CachedParameter {
                        name: "id".to_string(),
                        location: "path".to_string(),
                        required: true,
                        description: Some("User ID".to_string()),
                        schema: None,
                        schema_type: Some("string".to_string()),
                        format: None,
                        default_value: None,
                        enum_values: vec![],
                        example: None,
                    },
                ],
                request_body: None,
                responses: vec![
                    CachedResponse {
                        status_code: "200".to_string(),
                        description: Some("User found successfully".to_string()),
                        content_type: Some("application/json".to_string()),
                        schema: None,
                        example: None,
                    },
                    CachedResponse {
                        status_code: "404".to_string(),
                        description: Some("User not found".to_string()),
                        content_type: None,
                        schema: None,
                        example: None,
                    },
                ],
                security_requirements: vec![],
                tags: vec!["users".to_string()],
                deprecated: false,
                external_docs_url: Some("https://docs.test.com/users".to_string()),
                examples: vec![
                    CommandExample {
                        description: "Get user 123".to_string(),
                        command_line: "aperture api testapi users get-user --id 123".to_string(),
                        explanation: Some("Example of retrieving user with ID 123".to_string()),
                    },
                ],
                display_group: None,
                display_name: None,
                aliases: vec![],
                hidden: false,
            },
            CachedCommand {
                name: "create-user".to_string(),
                operation_id: "createUser".to_string(),
                method: "POST".to_string(),
                path: "/users".to_string(),
                summary: Some("Create new user".to_string()),
                description: Some("Create a new user account with the provided information.".to_string()),
                parameters: vec![],
                request_body: Some(CachedRequestBody {
                    content_type: "application/json".to_string(),
                    schema: "{}".to_string(),
                    required: true,
                    description: Some("User data".to_string()),
                    example: None,
                }),
                responses: vec![
                    CachedResponse {
                        status_code: "201".to_string(),
                        description: Some("User created successfully".to_string()),
                        content_type: Some("application/json".to_string()),
                        schema: None,
                        example: None,
                    },
                ],
                security_requirements: vec![],
                tags: vec!["users".to_string()],
                deprecated: false,
                external_docs_url: None,
                examples: vec![],
                display_group: None,
                display_name: None,
                aliases: vec![],
                hidden: false,
            },
        ],
    }
}

#[test]
fn test_documentation_generator_creation() {
    let mut specs = BTreeMap::new();
    specs.insert("test".to_string(), create_test_spec());

    let doc_gen = DocumentationGenerator::new(specs);
    // Should not panic and should be able to create - just test that it doesn't crash
    let _test_help = doc_gen.generate_interactive_menu();
    assert!(!_test_help.is_empty());
}

#[test]
fn test_generate_command_help() {
    let mut specs = BTreeMap::new();
    specs.insert("testapi".to_string(), create_test_spec());

    let doc_gen = DocumentationGenerator::new(specs);
    let help = doc_gen
        .generate_command_help("testapi", "users", "get-user-by-id")
        .unwrap();

    // Check that help contains expected sections
    assert!(help.contains("# GET /users/{id}"));
    assert!(help.contains("**Get user by ID**"));
    assert!(help.contains("## Usage"));
    assert!(help.contains("aperture api testapi users get-user-by-id"));
    assert!(help.contains("## Parameters"));
    assert!(help.contains("--id"));
    assert!(help.contains("**(required)**"));
    assert!(help.contains("## Examples"));
    assert!(help.contains("## Responses"));
    assert!(help.contains("**200**: User found successfully"));
    assert!(help.contains("**404**: User not found"));
    assert!(help.contains("📖 **External Documentation**"));
    assert!(help.contains("https://docs.test.com/users"));
}

#[test]
fn test_generate_command_help_with_request_body() {
    let mut specs = BTreeMap::new();
    specs.insert("testapi".to_string(), create_test_spec());

    let doc_gen = DocumentationGenerator::new(specs);
    let help = doc_gen
        .generate_command_help("testapi", "users", "create-user")
        .unwrap();

    assert!(help.contains("# POST /users"));
    assert!(help.contains("## Request Body"));
    assert!(help.contains("User data"));
    assert!(help.contains("Required: true"));
    assert!(help.contains("--body"));
}

#[test]
fn test_generate_api_overview() {
    let mut specs = BTreeMap::new();
    specs.insert("testapi".to_string(), create_test_spec());

    let doc_gen = DocumentationGenerator::new(specs);
    let overview = doc_gen.generate_api_overview("testapi").unwrap();

    assert!(overview.contains("# TestAPI API"));
    assert!(overview.contains("**Version**: 1.0.0"));
    assert!(overview.contains("**Base URL**: https://api.test.com"));
    assert!(overview.contains("## Statistics"));
    assert!(overview.contains("**Total Operations**: 2"));
    assert!(overview.contains("GET: 1"));
    assert!(overview.contains("POST: 1"));
    assert!(overview.contains("users: 2"));
    assert!(overview.contains("## Quick Start"));
    assert!(overview.contains("aperture list-commands testapi"));
    assert!(overview.contains("## Sample Operations"));
}

#[test]
fn test_generate_interactive_menu() {
    let mut specs = BTreeMap::new();
    specs.insert("testapi".to_string(), create_test_spec());

    let doc_gen = DocumentationGenerator::new(specs);
    let menu = doc_gen.generate_interactive_menu();

    assert!(menu.contains("# Aperture Interactive Help"));
    assert!(menu.contains("## Your APIs"));
    assert!(menu.contains("**testapi** (2 operations)"));
    assert!(menu.contains("## Common Commands"));
    assert!(menu.contains("aperture config list"));
    assert!(menu.contains("aperture search"));
    assert!(menu.contains("aperture exec"));
    assert!(menu.contains("## Tips"));
    assert!(menu.contains("--describe-json"));
    assert!(menu.contains("--dry-run"));
}

#[test]
fn test_generate_interactive_menu_no_apis() {
    let specs = BTreeMap::new();
    let doc_gen = DocumentationGenerator::new(specs);
    let menu = doc_gen.generate_interactive_menu();

    assert!(menu.contains("## No APIs Configured"));
    assert!(menu.contains("aperture config add myapi ./openapi.yaml"));
}

#[test]
fn test_command_help_nonexistent_api() {
    let specs = BTreeMap::new();
    let doc_gen = DocumentationGenerator::new(specs);

    let result = doc_gen.generate_command_help("nonexistent", "users", "get-user");
    assert!(result.is_err());
}

#[test]
fn test_command_help_nonexistent_operation() {
    let mut specs = BTreeMap::new();
    specs.insert("testapi".to_string(), create_test_spec());

    let doc_gen = DocumentationGenerator::new(specs);

    let result = doc_gen.generate_command_help("testapi", "users", "nonexistent");
    assert!(result.is_err());
}

#[test]
fn test_help_formatter_command_list() {
    let spec = create_test_spec();
    let formatted = HelpFormatter::format_command_list(&spec);

    assert!(formatted.contains("📋 TestAPI API Commands"));
    assert!(formatted.contains("Version: 1.0.0"));
    assert!(formatted.contains("Operations: 2"));
    assert!(formatted.contains("Base URL: https://api.test.com"));
    assert!(formatted.contains("📁 users"));
    assert!(formatted.contains("🔍 GET"));
    assert!(formatted.contains("📝 POST"));
    assert!(formatted.contains("get-user"));
    assert!(formatted.contains("create-user"));
    assert!(formatted.contains("Path: /users/{id}"));
    assert!(formatted.contains("Path: /users"));
}

#[test]
fn test_help_formatter_method_badges() {
    let spec = create_test_spec();
    let formatted = HelpFormatter::format_command_list(&spec);

    // Check that different HTTP methods get different badges
    assert!(formatted.contains("🔍 GET"));
    assert!(formatted.contains("📝 POST"));
}

#[test]
fn test_deprecated_command_indication() {
    let mut spec = create_test_spec();
    spec.commands[0].deprecated = true;

    let formatted = HelpFormatter::format_command_list(&spec);
    assert!(formatted.contains("⚠️"));
}

#[test]
fn test_command_help_contains_basic_example() {
    let mut specs = BTreeMap::new();
    specs.insert("testapi".to_string(), create_test_spec());

    let doc_gen = DocumentationGenerator::new(specs);
    let help = doc_gen
        .generate_command_help("testapi", "users", "create-user")
        .unwrap();

    // Should generate a basic example since this command has no predefined examples
    assert!(help.contains("## Example"));
    assert!(help.contains("aperture api testapi users create-user"));
}
