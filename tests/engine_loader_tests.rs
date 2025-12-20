use aperture_cli::cache::models::{CachedCommand, CachedParameter, CachedResponse, CachedSpec};
use aperture_cli::constants;
use aperture_cli::engine::loader::load_cached_spec;
use aperture_cli::error::{Error, ErrorKind};
use std::collections::HashMap;
use std::fs;
use tempfile::TempDir;

fn create_test_cached_spec() -> CachedSpec {
    CachedSpec {
        cache_format_version: aperture_cli::cache::models::CACHE_FORMAT_VERSION,
        name: "test-api".to_string(),
        version: "1.0.0".to_string(),
        commands: vec![CachedCommand {
            name: "users".to_string(),
            description: Some("User management operations".to_string()),
            summary: None,
            operation_id: "listUsers".to_string(),
            method: "GET".to_string(),
            path: "/users".to_string(),
            parameters: vec![CachedParameter {
                name: "limit".to_string(),
                location: "query".to_string(),
                required: false,
                description: None,
                schema: Some(r#"{"type": "integer"}"#.to_string()),
                schema_type: Some("integer".to_string()),
                format: None,
                default_value: None,
                enum_values: vec![],
                example: None,
            }],
            request_body: None,
            responses: vec![CachedResponse {
                status_code: "200".to_string(),
                description: None,
                content_type: Some(constants::CONTENT_TYPE_JSON.to_string()),
                schema: Some(r#"{"type": "array"}"#.to_string()),
                example: None,
            }],
            security_requirements: vec![],
            tags: vec!["users".to_string()],
            deprecated: false,
            external_docs_url: None,
            examples: vec![],
        }],
        base_url: Some("https://api.example.com".to_string()),
        servers: vec!["https://api.example.com".to_string()],
        security_schemes: HashMap::new(),
        skipped_endpoints: vec![],
        server_variables: HashMap::new(),
    }
}

#[test]
fn test_load_cached_spec_success() {
    let temp_dir = TempDir::new().unwrap();
    let cache_dir = temp_dir.path();

    // Create a test cached spec
    let test_spec = create_test_cached_spec();
    let cache_data = bincode::serialize(&test_spec).unwrap();

    let cache_file = cache_dir.join("test-api.bin");
    fs::write(&cache_file, cache_data).unwrap();

    // Load the cached spec
    let loaded_spec = load_cached_spec(cache_dir, "test-api").unwrap();

    // Verify the loaded spec matches
    assert_eq!(loaded_spec, test_spec);
    assert_eq!(loaded_spec.name, "test-api");
    assert_eq!(loaded_spec.version, "1.0.0");
    assert_eq!(loaded_spec.commands.len(), 1);
    assert_eq!(loaded_spec.commands[0].operation_id, "listUsers");
}

#[test]
fn test_load_cached_spec_file_not_found() {
    let temp_dir = TempDir::new().unwrap();
    let cache_dir = temp_dir.path();

    let result = load_cached_spec(cache_dir, "nonexistent-api");

    assert!(result.is_err());
    match result {
        Err(Error::Internal {
            kind,
            message,
            context,
        }) => {
            assert_eq!(kind, ErrorKind::Specification);
            assert!(message.contains("No cached spec found"));
            assert!(message.contains("nonexistent-api"));
            let Some(ctx) = context else { return };
            let Some(details) = &ctx.details else { return };
            assert_eq!(details["spec_name"], "nonexistent-api");
        }
        _ => panic!("Expected CachedSpecNotFound error, got: {result:?}"),
    }
}

#[test]
fn test_load_cached_spec_corrupted_data() {
    let temp_dir = TempDir::new().unwrap();
    let cache_dir = temp_dir.path();

    // Write invalid binary data
    let cache_file = cache_dir.join("corrupted-api.bin");
    fs::write(&cache_file, b"invalid binary data").unwrap();

    let result = load_cached_spec(cache_dir, "corrupted-api");

    assert!(result.is_err());
    match result {
        Err(Error::Internal {
            kind,
            message,
            context,
        }) => {
            assert_eq!(kind, ErrorKind::Specification);
            assert!(message.contains("Failed to deserialize cached spec"));
            assert!(message.contains("corrupted-api"));
            let Some(ctx) = context else { return };
            let Some(details) = &ctx.details else { return };
            assert_eq!(details["spec_name"], "corrupted-api");
            assert!(details["corruption_reason"].is_string());
        }
        _ => panic!("Expected CachedSpecCorrupted error, got: {result:?}"),
    }
}

#[test]
fn test_load_cached_spec_version_mismatch() {
    let temp_dir = TempDir::new().unwrap();
    let cache_dir = temp_dir.path();

    // Create a cached spec with old version
    let mut test_spec = create_test_cached_spec();
    test_spec.cache_format_version = 1; // Old version (current is 2)

    let cache_data = bincode::serialize(&test_spec).unwrap();
    let cache_file = cache_dir.join("old-version-api.bin");
    fs::write(&cache_file, cache_data).unwrap();

    // Attempt to load the cached spec with old version
    let result = load_cached_spec(cache_dir, "old-version-api");

    // Should fail with version mismatch error
    assert!(result.is_err());
    match result {
        Err(Error::Internal {
            kind,
            message,
            context,
        }) => {
            assert_eq!(kind, ErrorKind::Specification);
            assert!(message.contains("Cache format version mismatch"));
            assert!(message.contains("old-version-api"));
            assert!(message.contains("found v1"));
            let Some(ctx) = context else { return };
            let Some(details) = &ctx.details else { return };
            assert_eq!(details["spec_name"], "old-version-api");
            assert_eq!(details["found_version"], 1);
            assert_eq!(
                details["expected_version"],
                aperture_cli::cache::models::CACHE_FORMAT_VERSION
            );
        }
        _ => panic!("Expected CacheVersionMismatch error, got: {result:?}"),
    }
}
