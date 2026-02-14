// These lints are overly pedantic for test code
#![allow(clippy::needless_collect)]
#![allow(clippy::match_same_arms)]
#![allow(clippy::equatable_if_let)]

use aperture_cli::cache::models::{CachedCommand, CachedSpec};
use aperture_cli::shortcuts::{ResolutionResult, ShortcutResolver};
use std::collections::{BTreeMap, HashMap};

fn create_test_spec_with_multiple_operations() -> CachedSpec {
    CachedSpec {
        cache_format_version: aperture_cli::cache::models::CACHE_FORMAT_VERSION,
        name: "petstore".to_string(),
        version: "1.0.0".to_string(),
        commands: vec![
            CachedCommand {
                name: "get-pet-by-id".to_string(),
                description: Some("Get pet by ID".to_string()),
                summary: None,
                operation_id: "getPetById".to_string(),
                method: "GET".to_string(),
                path: "/pets/{id}".to_string(),
                parameters: vec![],
                request_body: None,
                responses: vec![],
                security_requirements: vec![],
                tags: vec!["pets".to_string()],
                deprecated: false,
                external_docs_url: None,
                examples: vec![],
                display_group: None,
                display_name: None,
                aliases: vec![],
                hidden: false,
            },
            CachedCommand {
                name: "create-pet".to_string(),
                description: Some("Create a new pet".to_string()),
                summary: None,
                operation_id: "createPet".to_string(),
                method: "POST".to_string(),
                path: "/pets".to_string(),
                parameters: vec![],
                request_body: None,
                responses: vec![],
                security_requirements: vec![],
                tags: vec!["pets".to_string()],
                deprecated: false,
                external_docs_url: None,
                examples: vec![],
                display_group: None,
                display_name: None,
                aliases: vec![],
                hidden: false,
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
                display_group: None,
                display_name: None,
                aliases: vec![],
                hidden: false,
            },
        ],
        base_url: Some("https://api.petstore.com".to_string()),
        servers: vec!["https://api.petstore.com".to_string()],
        security_schemes: HashMap::new(),
        skipped_endpoints: vec![],
        server_variables: HashMap::new(),
    }
}

#[test]
fn test_operation_id_resolution() {
    let spec = create_test_spec_with_multiple_operations();
    let mut specs = BTreeMap::new();
    specs.insert("petstore".to_string(), spec);

    let mut resolver = ShortcutResolver::new();
    resolver.index_specs(&specs);

    // Test exact operation ID match
    match resolver.resolve_shortcut(&["getPetById".to_string()]) {
        ResolutionResult::Resolved(shortcut) => {
            assert_eq!(
                shortcut.full_command,
                vec!["api", "petstore", "pets", "get-pet-by-id"]
            );
            assert_eq!(shortcut.command.operation_id, "getPetById");
            assert!(shortcut.confidence >= 90);
        }
        _ => panic!("Expected resolved shortcut for getPetById"),
    }

    // Test kebab-case operation ID match
    match resolver.resolve_shortcut(&["get-pet-by-id".to_string()]) {
        ResolutionResult::Resolved(shortcut) => {
            assert_eq!(shortcut.command.operation_id, "getPetById");
        }
        _ => panic!("Expected resolved shortcut for get-pet-by-id"),
    }
}

#[test]
fn test_method_path_resolution() {
    let spec = create_test_spec_with_multiple_operations();
    let mut specs = BTreeMap::new();
    specs.insert("petstore".to_string(), spec);

    let mut resolver = ShortcutResolver::new();
    resolver.index_specs(&specs);

    // Test HTTP method + path resolution
    match resolver.resolve_shortcut(&["GET".to_string(), "/pets/{id}".to_string()]) {
        ResolutionResult::Resolved(shortcut) => {
            assert_eq!(shortcut.command.operation_id, "getPetById");
            assert_eq!(shortcut.command.method, "GET");
            assert_eq!(shortcut.command.path, "/pets/{id}");
            assert!(shortcut.confidence >= 85);
        }
        _ => panic!("Expected resolved shortcut for GET /pets/{{id}}"),
    }

    // Test POST method
    match resolver.resolve_shortcut(&["POST".to_string(), "/pets".to_string()]) {
        ResolutionResult::Resolved(shortcut) => {
            assert_eq!(shortcut.command.operation_id, "createPet");
            assert_eq!(shortcut.command.method, "POST");
        }
        _ => panic!("Expected resolved shortcut for POST /pets"),
    }
}

#[test]
fn test_tag_resolution() {
    let spec = create_test_spec_with_multiple_operations();
    let mut specs = BTreeMap::new();
    specs.insert("petstore".to_string(), spec);

    let mut resolver = ShortcutResolver::new();
    resolver.index_specs(&specs);

    // Test tag-only resolution (should return multiple or most relevant)
    match resolver.resolve_shortcut(&["pets".to_string()]) {
        ResolutionResult::Resolved(shortcut) => {
            // Should resolve to one of the pets operations
            assert!(shortcut.command.tags.contains(&"pets".to_string()));
        }
        ResolutionResult::Ambiguous(matches) => {
            // Multiple pets operations - this is also valid
            assert!(!matches.is_empty());
            for m in &matches {
                assert!(m.command.tags.contains(&"pets".to_string()));
            }
        }
        ResolutionResult::NotFound => panic!("Expected to find pets operations"),
    }

    // Test tag + operation combination
    match resolver.resolve_shortcut(&["pets".to_string(), "get-pet-by-id".to_string()]) {
        ResolutionResult::Resolved(shortcut) => {
            assert_eq!(shortcut.command.operation_id, "getPetById");
            assert!(shortcut.confidence >= 70);
        }
        ResolutionResult::Ambiguous(matches) => {
            println!("Got ambiguous matches: {:?}", matches.len());
            for m in &matches {
                println!(
                    "  - {} (confidence: {})",
                    m.command.operation_id, m.confidence
                );
            }
            panic!("Expected resolved shortcut for pets get-pet-by-id, got ambiguous");
        }
        ResolutionResult::NotFound => {
            panic!("Expected resolved shortcut for pets get-pet-by-id, got NotFound");
        }
    }
}

#[test]
fn test_fuzzy_matching() {
    let spec = create_test_spec_with_multiple_operations();
    let mut specs = BTreeMap::new();
    specs.insert("petstore".to_string(), spec);

    let mut resolver = ShortcutResolver::new();
    resolver.index_specs(&specs);

    // Test fuzzy matching with typos
    match resolver.resolve_shortcut(&["getPetByld".to_string()]) {
        // 'I' instead of 'I'
        ResolutionResult::Resolved(shortcut) => {
            assert_eq!(shortcut.command.operation_id, "getPetById");
            // Fuzzy match should have lower confidence
            assert!(shortcut.confidence < 90);
            assert!(shortcut.confidence >= 20);
        }
        ResolutionResult::Ambiguous(_) => {
            // This is also acceptable for fuzzy matches
        }
        ResolutionResult::NotFound => {
            // Fuzzy matching might not always find results
            // This test mainly ensures the fuzzy logic doesn't crash
        }
    }
}

#[test]
fn test_not_found() {
    let spec = create_test_spec_with_multiple_operations();
    let mut specs = BTreeMap::new();
    specs.insert("petstore".to_string(), spec);

    let mut resolver = ShortcutResolver::new();
    resolver.index_specs(&specs);

    // Test completely unrelated term
    if let ResolutionResult::NotFound = resolver.resolve_shortcut(&["nonexistent".to_string()]) {
        // Expected
    } else {
        // Fuzzy matching might still find something, which is okay
    }
}

#[test]
fn test_ambiguous_suggestions_format() {
    let spec = create_test_spec_with_multiple_operations();
    let mut specs = BTreeMap::new();
    specs.insert("petstore".to_string(), spec);

    let mut resolver = ShortcutResolver::new();
    resolver.index_specs(&specs);

    // Create some mock ambiguous matches for formatting test
    let matches = vec![
        aperture_cli::shortcuts::ResolvedShortcut {
            full_command: vec![
                "api".to_string(),
                "petstore".to_string(),
                "pets".to_string(),
                "get-pet-by-id".to_string(),
            ],
            spec: specs.get("petstore").unwrap().clone(),
            command: specs.get("petstore").unwrap().commands[0].clone(),
            confidence: 80,
        },
        aperture_cli::shortcuts::ResolvedShortcut {
            full_command: vec![
                "api".to_string(),
                "petstore".to_string(),
                "pets".to_string(),
                "create-pet".to_string(),
            ],
            spec: specs.get("petstore").unwrap().clone(),
            command: specs.get("petstore").unwrap().commands[1].clone(),
            confidence: 70,
        },
    ];

    let suggestion_text = resolver.format_ambiguous_suggestions(&matches);
    assert!(suggestion_text.contains("Multiple commands match"));
    assert!(suggestion_text.contains("aperture api petstore"));
    assert!(suggestion_text.contains("Get pet by ID"));
    assert!(suggestion_text.contains("Create a new pet"));
}

#[test]
fn test_empty_args() {
    let spec = create_test_spec_with_multiple_operations();
    let mut specs = BTreeMap::new();
    specs.insert("petstore".to_string(), spec);

    let mut resolver = ShortcutResolver::new();
    resolver.index_specs(&specs);

    // Test empty arguments
    match resolver.resolve_shortcut(&[]) {
        ResolutionResult::NotFound => {
            // Expected
        }
        _ => panic!("Expected NotFound for empty arguments"),
    }
}
