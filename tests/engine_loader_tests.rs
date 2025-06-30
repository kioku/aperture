use aperture::cache::models::{CachedCommand, CachedParameter, CachedResponse, CachedSpec};
use aperture::engine::loader::load_cached_spec;
use aperture::error::Error;
use std::collections::HashMap;
use std::fs;
use tempfile::TempDir;

fn create_test_cached_spec() -> CachedSpec {
    CachedSpec {
        name: "test-api".to_string(),
        version: "1.0.0".to_string(),
        commands: vec![CachedCommand {
            name: "users".to_string(),
            description: Some("User management operations".to_string()),
            operation_id: "listUsers".to_string(),
            method: "GET".to_string(),
            path: "/users".to_string(),
            parameters: vec![CachedParameter {
                name: "limit".to_string(),
                location: "query".to_string(),
                required: false,
                schema: Some(r#"{"type": "integer"}"#.to_string()),
            }],
            request_body: None,
            responses: vec![CachedResponse {
                status_code: "200".to_string(),
                content: Some(r#"{"type": "array"}"#.to_string()),
            }],
            security_requirements: vec![],
        }],
        base_url: Some("https://api.example.com".to_string()),
        servers: vec!["https://api.example.com".to_string()],
        security_schemes: HashMap::new(),
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
    if let Err(Error::Config(msg)) = result {
        assert!(msg.contains("No cached spec found"));
        assert!(msg.contains("nonexistent-api"));
    } else {
        panic!("Expected Config error");
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
    if let Err(Error::Config(msg)) = result {
        assert!(msg.contains("Failed to deserialize"));
        assert!(msg.contains("corrupted"));
    } else {
        panic!("Expected Config error");
    }
}
