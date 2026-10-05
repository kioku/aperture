use aperture_cli::cache::models::{
    CachedCommand, CachedParameter, CachedResponse, CachedSpec, PaginationInfo,
    ParameterSerialization, CACHE_FORMAT_VERSION,
};
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
                serialization: ParameterSerialization::default(),
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
            security_scopes: Vec::new(),
            security_requirements: vec![],
            tags: vec!["users".to_string()],
            deprecated: false,
            external_docs_url: None,
            examples: vec![],
            display_group: None,
            display_name: None,
            aliases: vec![],
            hidden: false,
            pagination: PaginationInfo::default(),
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
    let cache_data = postcard::to_allocvec(&test_spec).unwrap();

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
fn prior_v6_collapsed_response_cache_is_rejected() {
    let temp_dir = TempDir::new().unwrap();
    let cache_dir = temp_dir.path();
    let mut prior_cache = create_test_cached_spec();
    prior_cache.cache_format_version = 6;
    prior_cache.commands[0].responses = vec![CachedResponse {
        status_code: "200".to_string(),
        description: Some("Prior transformer retained only JSON".to_string()),
        content_type: Some(constants::CONTENT_TYPE_JSON.to_string()),
        schema: Some(r#"{"type":"object"}"#.to_string()),
        example: None,
    }];
    fs::write(
        cache_dir.join("prior-v6.bin"),
        postcard::to_allocvec(&prior_cache).unwrap(),
    )
    .unwrap();

    let error = load_cached_spec(cache_dir, "prior-v6").unwrap_err();
    assert!(error.to_string().contains("found v6"));
    assert!(error.to_string().contains(&format!(
        "expected v{}",
        aperture_cli::cache::models::CACHE_FORMAT_VERSION
    )));

    let Error::Internal {
        context: Some(context),
        ..
    } = error
    else {
        panic!("expected version mismatch context");
    };
    assert!(context
        .suggestion
        .as_deref()
        .is_some_and(|suggestion| suggestion.contains("config api reinit")));
}

#[test]
fn test_load_cached_spec_version_mismatch() {
    let temp_dir = TempDir::new().unwrap();
    let cache_dir = temp_dir.path();

    // Create a cached spec with old version
    let mut test_spec = create_test_cached_spec();
    test_spec.cache_format_version = 1; // Old version (current is 2)

    let cache_data = postcard::to_allocvec(&test_spec).unwrap();
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

#[test]
fn cache_adversarial_layouts() {
    let cache = TempDir::new().unwrap();
    let mut spec = create_test_cached_spec();
    let current = postcard::to_allocvec(&spec).unwrap();
    fs::write(cache.path().join("current.bin"), &current).unwrap();
    assert_eq!(load_cached_spec(cache.path(), "current").unwrap(), spec);
    for (name, bytes) in [
        ("empty", vec![]),
        ("truncated", current[..current.len() / 2].to_vec()),
        ("invalid", vec![255; 10]),
    ] {
        fs::write(cache.path().join(format!("{name}.bin")), bytes).unwrap();
        assert!(load_cached_spec(cache.path(), name)
            .unwrap_err()
            .to_string()
            .contains("corrupt"));
    }
    for version in [7, 8, 9, CACHE_FORMAT_VERSION + 1] {
        spec.cache_format_version = version;
        fs::write(
            cache.path().join("version.bin"),
            postcard::to_allocvec(&spec).unwrap(),
        )
        .unwrap();
        assert!(load_cached_spec(cache.path(), "version")
            .unwrap_err()
            .to_string()
            .contains("version"));
    }
}

#[test]
fn metadata_does_not_override_embedded_cache_version() {
    let cache = TempDir::new().unwrap();
    let metadata =
        aperture_cli::cache::metadata::CacheMetadataManager::new(&aperture_cli::fs::OsFileSystem);
    metadata
        .update_spec_metadata(cache.path(), "mixed", 0)
        .unwrap();
    let mut spec = create_test_cached_spec();
    spec.cache_format_version = CACHE_FORMAT_VERSION + 1;
    fs::write(
        cache.path().join("mixed.bin"),
        postcard::to_allocvec(&spec).unwrap(),
    )
    .unwrap();
    assert!(load_cached_spec(cache.path(), "mixed")
        .unwrap_err()
        .to_string()
        .contains("version"));
}

/// Fixture emitted by the landed spec slice's binary (layout 8), including
/// a parameter without layout 9's serialization fields.
#[test]
fn actual_prior_spec_layout8_is_rejected_before_field_decoding() {
    use aperture_cli::cache::metadata::CacheMetadataManager;
    use aperture_cli::cache::models::{GlobalCacheMetadata, SpecMetadata, CACHE_FORMAT_VERSION};
    let cache = TempDir::new().unwrap();
    fs::write(
        cache.path().join("prior8.bin"),
        include_bytes!("fixtures/parsed-spec-v8.bin"),
    )
    .unwrap();
    let manager = CacheMetadataManager::new(&aperture_cli::fs::OsFileSystem);
    for metadata_version in [8, CACHE_FORMAT_VERSION] {
        manager
            .save_metadata(
                cache.path(),
                &GlobalCacheMetadata {
                    cache_format_version: metadata_version,
                    specs: HashMap::from([(
                        "prior8".into(),
                        SpecMetadata {
                            updated_at: "2026-10-03T00:00:00Z".into(),
                            file_size: 0,
                            content_hash: None,
                            mtime_secs: None,
                            spec_file_size: None,
                        },
                    )]),
                },
            )
            .unwrap();
        let error = load_cached_spec(cache.path(), "prior8").unwrap_err();
        assert!(error.to_string().contains("found v8"), "{error}");
        assert!(
            error
                .to_string()
                .contains(&format!("expected v{CACHE_FORMAT_VERSION}")),
            "{error}"
        );
    }
}
