//! Tests for command search functionality

use aperture_cli::cache::models::{
    CachedCommand, CachedParameter, CachedSpec, CACHE_FORMAT_VERSION,
};
use aperture_cli::search::{format_search_results, CommandSearcher};
use std::collections::{BTreeMap, HashMap};

fn create_test_spec(name: &str) -> CachedSpec {
    CachedSpec {
        cache_format_version: CACHE_FORMAT_VERSION,
        name: name.to_string(),
        version: "1.0.0".to_string(),
        base_url: Some("https://api.example.com".to_string()),
        commands: vec![
            CachedCommand {
                operation_id: "getUser".to_string(),
                method: "GET".to_string(),
                path: "/users/{id}".to_string(),
                summary: Some("Get a user by ID".to_string()),
                description: Some("Retrieves detailed user information".to_string()),
                tags: vec!["users".to_string()],
                name: "users".to_string(),
                parameters: vec![CachedParameter {
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
                }],
                request_body: None,
                responses: vec![],
                security_requirements: vec![],
                deprecated: false,
                external_docs_url: None,
            },
            CachedCommand {
                operation_id: "listUsers".to_string(),
                method: "GET".to_string(),
                path: "/users".to_string(),
                summary: Some("List all users".to_string()),
                description: Some("Returns a paginated list of users".to_string()),
                tags: vec!["users".to_string()],
                name: "users".to_string(),
                parameters: vec![],
                request_body: None,
                responses: vec![],
                security_requirements: vec![],
                deprecated: false,
                external_docs_url: None,
            },
            CachedCommand {
                operation_id: "createUser".to_string(),
                method: "POST".to_string(),
                path: "/users".to_string(),
                summary: Some("Create a new user".to_string()),
                description: Some("Creates a new user account".to_string()),
                tags: vec!["users".to_string()],
                name: "users".to_string(),
                parameters: vec![],
                request_body: None,
                responses: vec![],
                security_requirements: vec![],
                deprecated: false,
                external_docs_url: None,
            },
            CachedCommand {
                operation_id: "getIssue".to_string(),
                method: "GET".to_string(),
                path: "/issues/{id}".to_string(),
                summary: Some("Get an issue by ID".to_string()),
                description: Some("Retrieves issue details".to_string()),
                tags: vec!["issues".to_string()],
                name: "issues".to_string(),
                parameters: vec![],
                request_body: None,
                responses: vec![],
                security_requirements: vec![],
                deprecated: false,
                external_docs_url: None,
            },
        ],
        servers: vec![],
        security_schemes: HashMap::new(),
        skipped_endpoints: vec![],
        server_variables: HashMap::new(),
    }
}

#[test]
fn test_search_by_operation_id() {
    let searcher = CommandSearcher::new();
    let mut specs = BTreeMap::new();
    let spec = create_test_spec("test-api");

    // Debug: Print command operation IDs to verify the test data
    for cmd in &spec.commands {
        eprintln!(
            "Command: operation_id={}, method={}, path={}",
            cmd.operation_id, cmd.method, cmd.path
        );
    }

    specs.insert("test-api".to_string(), spec);

    let results = searcher.search(&specs, "getUser", None).unwrap();

    eprintln!("Search for 'getUser' found {} results", results.len());
    for result in &results {
        eprintln!(
            "  - {} (score: {})",
            result.command.operation_id, result.score
        );
    }

    assert_eq!(results.len(), 1);
    assert_eq!(results[0].command.operation_id, "getUser");
    assert_eq!(results[0].api_context, "test-api");
}

#[test]
fn test_search_by_method() {
    let searcher = CommandSearcher::new();
    let mut specs = BTreeMap::new();
    specs.insert("test-api".to_string(), create_test_spec("test-api"));

    let results = searcher.search(&specs, "POST", None).unwrap();

    assert_eq!(results.len(), 1);
    assert_eq!(results[0].command.method, "POST");
}

#[test]
fn test_search_by_keyword_in_summary() {
    let searcher = CommandSearcher::new();
    let mut specs = BTreeMap::new();
    specs.insert("test-api".to_string(), create_test_spec("test-api"));

    let results = searcher.search(&specs, "paginated", None).unwrap();

    assert_eq!(results.len(), 1);
    assert_eq!(results[0].command.operation_id, "listUsers");
}

#[test]
fn test_search_with_api_filter() {
    let searcher = CommandSearcher::new();
    let mut specs = BTreeMap::new();
    specs.insert("api1".to_string(), create_test_spec("api1"));
    specs.insert("api2".to_string(), create_test_spec("api2"));

    // Search without filter should return results from both APIs
    let results = searcher.search(&specs, "user", None).unwrap();
    assert!(results.len() > 3); // Should find user operations in both APIs

    // Search with filter should only return from specified API
    let results = searcher.search(&specs, "user", Some("api1")).unwrap();
    assert!(results.iter().all(|r| r.api_context == "api1"));
}

#[test]
fn test_fuzzy_search() {
    let searcher = CommandSearcher::new();
    let mut specs = BTreeMap::new();
    specs.insert("test-api".to_string(), create_test_spec("test-api"));

    // Test partial match - searching for "user" should find user-related operations
    let results = searcher.search(&specs, "user", None).unwrap();
    assert!(
        !results.is_empty(),
        "Fuzzy search should find results for partial matches"
    );

    // Should find multiple user operations
    let user_ops: Vec<_> = results
        .iter()
        .filter(|r| r.command.operation_id.to_lowercase().contains("user"))
        .collect();
    assert!(
        user_ops.len() >= 2,
        "Should find at least 2 user operations"
    );

    // Test that exact matches score higher than partial matches
    let exact_results = searcher.search(&specs, "getUser", None).unwrap();
    assert!(!exact_results.is_empty());
    assert_eq!(exact_results[0].command.operation_id, "getUser");

    // Verify fuzzy matching works with similar terms
    let list_results = searcher.search(&specs, "list", None).unwrap();
    assert!(!list_results.is_empty(), "Should find list operations");
}

#[test]
fn test_regex_search() {
    let searcher = CommandSearcher::new();
    let mut specs = BTreeMap::new();
    specs.insert("test-api".to_string(), create_test_spec("test-api"));

    // Regex pattern to find all "get" operations
    let results = searcher.search(&specs, r"get\w+", None).unwrap();

    assert_eq!(results.len(), 2); // getUser and getIssue
    assert!(results
        .iter()
        .all(|r| r.command.operation_id.starts_with("get")));
}

#[test]
fn test_find_similar_commands() {
    let searcher = CommandSearcher::new();
    let spec = create_test_spec("test");

    // Find similar to a typo
    let suggestions = searcher.find_similar_commands(&spec, "usr get-usr", 3);

    assert!(!suggestions.is_empty());
    // Should suggest "users get-user" as the closest match
    assert!(suggestions[0].0.contains("get-user"));
}

#[test]
fn test_format_search_results() {
    let searcher = CommandSearcher::new();
    let mut specs = BTreeMap::new();
    specs.insert("test-api".to_string(), create_test_spec("test-api"));

    let results = searcher.search(&specs, "getUser", None).unwrap();

    // Test non-verbose output
    let output = format_search_results(&results, false);
    assert!(output
        .iter()
        .any(|line| line.contains("aperture api test-api")));
    assert!(output.iter().any(|line| line.contains("GET /users/{id}")));

    // Test verbose output
    let output = format_search_results(&results, true);
    assert!(output.iter().any(|line| line.contains("Parameters:")));
}

#[test]
fn test_empty_search_results() {
    let results = vec![];
    let output = format_search_results(&results, false);

    assert_eq!(output.len(), 1);
    assert_eq!(output[0], "No matching operations found.");
}

#[test]
fn test_search_scoring() {
    let searcher = CommandSearcher::new();
    let mut specs = BTreeMap::new();
    specs.insert("test-api".to_string(), create_test_spec("test-api"));

    // Search for "user" should rank operations with "user" in different places
    let results = searcher.search(&specs, "user", None).unwrap();

    assert!(!results.is_empty());
    // Results should be sorted by score (highest first)
    for i in 1..results.len() {
        assert!(results[i - 1].score >= results[i].score);
    }
}
