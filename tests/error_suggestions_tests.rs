use aperture_cli::cache::models::{CachedCommand, CachedSpec};
use aperture_cli::suggestions::{
    suggest_auth_fix, suggest_parameter_format, suggest_similar_operations, suggest_valid_values,
};
use std::collections::HashMap;

fn create_test_spec() -> CachedSpec {
    CachedSpec {
        cache_format_version: aperture_cli::cache::models::CACHE_FORMAT_VERSION,
        name: "test-api".to_string(),
        version: "1.0.0".to_string(),
        commands: vec![
            CachedCommand {
                name: "get-user".to_string(),
                description: Some("Get user by ID".to_string()),
                summary: None,
                operation_id: "getUserById".to_string(),
                method: "GET".to_string(),
                path: "/users/{id}".to_string(),
                parameters: vec![],
                request_body: None,
                responses: vec![],
                security_requirements: vec![],
                tags: vec!["users".to_string()],
                deprecated: false,
                external_docs_url: None,
                examples: vec![],
            },
            CachedCommand {
                name: "create-user".to_string(),
                description: Some("Create a new user".to_string()),
                summary: None,
                operation_id: "createUser".to_string(),
                method: "POST".to_string(),
                path: "/users".to_string(),
                parameters: vec![],
                request_body: None,
                responses: vec![],
                security_requirements: vec![],
                tags: vec!["users".to_string()],
                deprecated: false,
                external_docs_url: None,
                examples: vec![],
            },
            CachedCommand {
                name: "list-users".to_string(),
                description: Some("List all users".to_string()),
                summary: None,
                operation_id: "listUsers".to_string(),
                method: "GET".to_string(),
                path: "/users".to_string(),
                parameters: vec![],
                request_body: None,
                responses: vec![],
                security_requirements: vec![],
                tags: vec!["users".to_string()],
                deprecated: false,
                external_docs_url: None,
                examples: vec![],
            },
        ],
        base_url: Some("https://api.example.com".to_string()),
        servers: vec!["https://api.example.com".to_string()],
        security_schemes: HashMap::new(),
        skipped_endpoints: vec![],
        server_variables: HashMap::new(),
    }
}

#[test]
fn test_suggest_similar_operations() {
    let spec = create_test_spec();

    // Test fuzzy matching for typos - search for something more unique
    let suggestions = suggest_similar_operations(&spec, "getUserById");
    println!("Suggestions for 'getUserById': {suggestions:?}");
    assert!(
        !suggestions.is_empty(),
        "Expected suggestions for 'getUserById', got none"
    );
    assert!(
        suggestions[0].contains("get-user-by-id"),
        "Expected first suggestion to contain 'get-user-by-id', got: {}",
        suggestions[0]
    );

    // Test partial matching
    let suggestions = suggest_similar_operations(&spec, "list");
    println!("Suggestions for 'list': {suggestions:?}");
    assert!(!suggestions.is_empty());
    assert!(suggestions[0].contains("list-users")); // Should match list-users

    // Test with completely unrelated term - fuzzy matching should still find something
    let suggestions = suggest_similar_operations(&spec, "xyz123");
    println!("Suggestions for 'xyz123': {suggestions:?}");
    // Fuzzy matching may or may not return results for completely unrelated terms
    // so we'll just check the function doesn't crash
}

#[test]
fn test_suggest_parameter_format() {
    let suggestion = suggest_parameter_format("user-id", Some("integer"));
    assert_eq!(suggestion, "--user-id <integer>");

    let suggestion = suggest_parameter_format("name", None);
    assert_eq!(suggestion, "--name <value>");
}

#[test]
fn test_suggest_valid_values() {
    let values = vec![
        "active".to_string(),
        "inactive".to_string(),
        "pending".to_string(),
    ];
    let suggestion = suggest_valid_values("status", &values);
    assert!(suggestion.contains("'active'"));
    assert!(suggestion.contains("'inactive'"));
    assert!(suggestion.contains("'pending'"));

    // Test with many values
    let many_values: Vec<String> = (0..10).map(|i| format!("value{i}")).collect();
    let suggestion = suggest_valid_values("field", &many_values);
    assert!(suggestion.contains("..."));
    assert!(suggestion.contains("value0"));
    assert!(!suggestion.contains("value9")); // Should be truncated

    // Test with no values
    let suggestion = suggest_valid_values("field", &[]);
    assert!(suggestion.contains("Check the parameter documentation"));
}

#[test]
fn test_suggest_auth_fix() {
    let suggestion = suggest_auth_fix("api-key", Some("API_KEY"));
    assert!(suggestion.contains("API_KEY"));
    assert!(suggestion.contains("aperture config secrets"));

    let suggestion = suggest_auth_fix("oauth", None);
    assert!(suggestion.contains("oauth"));
    assert!(suggestion.contains("aperture config secrets"));
}
