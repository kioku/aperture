use aperture_cli::config::manager::ConfigManager;
use aperture_cli::engine::loader::load_cached_spec;
use aperture_cli::fs::OsFileSystem;
use assert_cmd::Command;
use tempfile::TempDir;

fn create_temp_config_manager() -> (ConfigManager<OsFileSystem>, TempDir) {
    let temp_dir = TempDir::new().expect("Failed to create temp directory");
    let config_manager = ConfigManager::with_fs(OsFileSystem, temp_dir.path().to_path_buf());
    (config_manager, temp_dir)
}

#[test]
fn test_all_unsupported_content_types() {
    let (config_manager, _temp_dir) = create_temp_config_manager();

    // Create a spec with all unsupported content types from issue #11
    let spec_content = r#"
openapi: 3.0.0
info:
  title: All Content Types API
  version: 1.0.0
servers:
  - url: https://api.example.com
paths:
  /json:
    post:
      operationId: postJson
      requestBody:
        content:
          application/json:
            schema:
              type: object
        required: true
      responses:
        '200':
          description: Success
  /multipart:
    post:
      operationId: uploadFile
      requestBody:
        content:
          multipart/form-data:
            schema:
              type: object
        required: true
      responses:
        '200':
          description: Success
  /binary:
    post:
      operationId: uploadBinary
      requestBody:
        content:
          application/octet-stream:
            schema:
              type: string
              format: binary
        required: true
      responses:
        '200':
          description: Success
  /image:
    post:
      operationId: uploadImage
      requestBody:
        content:
          image/png:
            schema:
              type: string
              format: binary
        required: true
      responses:
        '200':
          description: Success
  /pdf:
    post:
      operationId: uploadPdf
      requestBody:
        content:
          application/pdf:
            schema:
              type: string
              format: binary
        required: true
      responses:
        '200':
          description: Success
  /xml:
    post:
      operationId: postXml
      requestBody:
        content:
          application/xml:
            schema:
              type: object
        required: true
      responses:
        '200':
          description: Success
  /text-xml:
    post:
      operationId: postTextXml
      requestBody:
        content:
          text/xml:
            schema:
              type: object
        required: true
      responses:
        '200':
          description: Success
  /form:
    post:
      operationId: postForm
      requestBody:
        content:
          application/x-www-form-urlencoded:
            schema:
              type: object
        required: true
      responses:
        '200':
          description: Success
  /text:
    post:
      operationId: postText
      requestBody:
        content:
          text/plain:
            schema:
              type: string
        required: true
      responses:
        '200':
          description: Success
  /csv:
    post:
      operationId: postCsv
      requestBody:
        content:
          text/csv:
            schema:
              type: string
        required: true
      responses:
        '200':
          description: Success
  /ndjson:
    post:
      operationId: postNdjson
      requestBody:
        content:
          application/x-ndjson:
            schema:
              type: string
        required: true
      responses:
        '200':
          description: Success
  /graphql:
    post:
      operationId: postGraphql
      requestBody:
        content:
          application/graphql:
            schema:
              type: string
        required: true
      responses:
        '200':
          description: Success
  /custom:
    post:
      operationId: postCustom
      requestBody:
        content:
          application/vnd.custom+json:
            schema:
              type: object
        required: true
      responses:
        '200':
          description: Success
"#;

    // Write spec to temp file
    let spec_file = _temp_dir.path().join("all-content-types.yaml");
    std::fs::write(&spec_file, spec_content).unwrap();

    // Add spec in non-strict mode
    let result = config_manager.add_spec("content-test", &spec_file, false, false);
    assert!(result.is_ok(), "Should accept spec in non-strict mode");

    // Load cached spec and verify only JSON endpoints were included
    let cache_dir = _temp_dir.path().join(".cache");
    let cached_spec = load_cached_spec(&cache_dir, "content-test").unwrap();

    // Should have 2 endpoints (standard JSON and the custom+json)
    assert_eq!(cached_spec.commands.len(), 2);

    let operation_ids: Vec<&str> = cached_spec
        .commands
        .iter()
        .map(|cmd| cmd.operation_id.as_str())
        .collect();
    assert!(operation_ids.contains(&"postJson"));
    assert!(operation_ids.contains(&"postCustom")); // application/vnd.custom+json is now accepted

    // Try in strict mode - should fail
    let result_strict = config_manager.add_spec("content-test-strict", &spec_file, false, true);
    assert!(result_strict.is_err(), "Should reject spec in strict mode");
}

#[test]
fn test_multiple_content_types_per_endpoint() {
    let (config_manager, _temp_dir) = create_temp_config_manager();

    // Create a spec with endpoints that have multiple content types
    let spec_content = r#"
openapi: 3.0.0
info:
  title: Mixed Content API
  version: 1.0.0
servers:
  - url: https://api.example.com
paths:
  /upload:
    post:
      operationId: uploadMixed
      requestBody:
        content:
          application/json:
            schema:
              type: object
          multipart/form-data:
            schema:
              type: object
          application/xml:
            schema:
              type: object
        required: true
      responses:
        '200':
          description: Success
  /data:
    put:
      operationId: putData
      requestBody:
        content:
          text/plain:
            schema:
              type: string
          application/json:
            schema:
              type: object
        required: true
      responses:
        '200':
          description: Success
"#;

    // Write spec to temp file
    let spec_file = _temp_dir.path().join("mixed-content.yaml");
    std::fs::write(&spec_file, spec_content).unwrap();

    // Add spec in non-strict mode
    let result = config_manager.add_spec("mixed-test", &spec_file, false, false);
    assert!(result.is_ok(), "Should accept spec in non-strict mode");

    // Both endpoints should be INCLUDED because they support JSON
    let cache_dir = _temp_dir.path().join(".cache");
    let cached_spec = load_cached_spec(&cache_dir, "mixed-test").unwrap();
    assert_eq!(
        cached_spec.commands.len(),
        2,
        "Both endpoints should be included because they support JSON"
    );

    // Verify both operations are present
    let operation_ids: Vec<&str> = cached_spec
        .commands
        .iter()
        .map(|cmd| cmd.operation_id.as_str())
        .collect();
    assert!(operation_ids.contains(&"uploadMixed"));
    assert!(operation_ids.contains(&"putData"));
}

#[test]
fn test_malformed_content_types() {
    let (config_manager, _temp_dir) = create_temp_config_manager();

    // Create a spec with malformed/edge case content types
    let spec_content = r#"
openapi: 3.0.0
info:
  title: Malformed Content API
  version: 1.0.0
servers:
  - url: https://api.example.com
paths:
  /empty:
    post:
      operationId: postEmpty
      requestBody:
        content:
          "":
            schema:
              type: string
        required: true
      responses:
        '200':
          description: Success
  /spaces:
    post:
      operationId: postSpaces
      requestBody:
        content:
          "  application/json  ":
            schema:
              type: object
        required: true
      responses:
        '200':
          description: Success
  /case:
    post:
      operationId: postCase
      requestBody:
        content:
          APPLICATION/JSON:
            schema:
              type: object
        required: true
      responses:
        '200':
          description: Success
  /charset:
    post:
      operationId: postCharset
      requestBody:
        content:
          "application/json; charset=utf-8":
            schema:
              type: object
        required: true
      responses:
        '200':
          description: Success
"#;

    // Write spec to temp file
    let spec_file = _temp_dir.path().join("malformed-content.yaml");
    std::fs::write(&spec_file, spec_content).unwrap();

    // Add spec in non-strict mode
    let result = config_manager.add_spec("malformed-test", &spec_file, false, false);
    assert!(result.is_ok(), "Should accept spec in non-strict mode");

    // Check which endpoints were included
    let cache_dir = _temp_dir.path().join(".cache");
    let cached_spec = load_cached_spec(&cache_dir, "malformed-test").unwrap();

    // All valid JSON content types should be accepted (case-insensitive, with/without params)
    assert_eq!(
        cached_spec.commands.len(),
        3,
        "Should accept: spaces, case variations, and charset parameters"
    );

    // Verify the correct endpoints were included
    let operation_ids: Vec<&str> = cached_spec
        .commands
        .iter()
        .map(|cmd| cmd.operation_id.as_str())
        .collect();
    assert!(
        operation_ids.contains(&"postSpaces"),
        "Should include endpoint with spaces around content type"
    );
    assert!(
        operation_ids.contains(&"postCase"),
        "Should include endpoint with uppercase content type"
    );
    assert!(
        operation_ids.contains(&"postCharset"),
        "Should include endpoint with charset parameter"
    );

    // Empty content type should still be skipped
    assert!(
        !operation_ids.contains(&"postEmpty"),
        "Should skip endpoint with empty content type"
    );
}

#[test]
fn test_cli_warnings_for_different_content_types() {
    let temp_dir = TempDir::new().unwrap();
    let config_dir = temp_dir.path().join(".aperture");

    // Create a spec with various content types
    let spec_content = r#"
openapi: 3.0.0
info:
  title: Various Content Types
  version: 1.0.0
servers:
  - url: https://api.example.com
paths:
  /file:
    post:
      operationId: uploadFile
      requestBody:
        content:
          multipart/form-data:
            schema:
              type: object
        required: true
      responses:
        '200':
          description: Success
  /image:
    post:
      operationId: uploadImage
      requestBody:
        content:
          image/jpeg:
            schema:
              type: string
              format: binary
        required: true
      responses:
        '200':
          description: Success
  /xml:
    post:
      operationId: postXml
      requestBody:
        content:
          application/xml:
            schema:
              type: object
        required: true
      responses:
        '200':
          description: Success
  /form:
    post:
      operationId: submitForm
      requestBody:
        content:
          application/x-www-form-urlencoded:
            schema:
              type: object
        required: true
      responses:
        '200':
          description: Success
"#;

    let spec_file = temp_dir.path().join("various-content.yaml");
    std::fs::write(&spec_file, spec_content).unwrap();

    // Add spec without --strict flag
    let output = Command::cargo_bin("aperture")
        .unwrap()
        .env("APERTURE_CONFIG_DIR", config_dir.to_str().unwrap())
        .args(["config", "add", "test-api", spec_file.to_str().unwrap()])
        .output()
        .unwrap();

    let stderr = String::from_utf8_lossy(&output.stderr);

    assert!(
        output.status.success(),
        "Command should succeed in non-strict mode"
    );

    // Check for specific warning messages
    assert!(
        stderr.contains("Warning: Skipping 4 endpoints with unsupported content types (0 of 4 endpoints will be available)"),
        "Should show correct count of skipped endpoints with available count. Actual stderr: {}", stderr
    );
    assert!(
        stderr.contains("file uploads are not supported"),
        "Should show specific message for multipart/form-data"
    );
    assert!(
        stderr.contains("image uploads are not supported"),
        "Should show specific message for image types"
    );
    assert!(
        stderr.contains("XML content is not supported"),
        "Should show specific message for XML"
    );
    assert!(
        stderr.contains("form-encoded data is not supported"),
        "Should show specific message for form-urlencoded"
    );
}

#[test]
fn test_wildcard_content_types() {
    let (config_manager, _temp_dir) = create_temp_config_manager();

    // Create a spec with various image types
    let spec_content = r#"
openapi: 3.0.0
info:
  title: Image API
  version: 1.0.0
servers:
  - url: https://api.example.com
paths:
  /png:
    post:
      operationId: uploadPng
      requestBody:
        content:
          image/png:
            schema:
              type: string
              format: binary
        required: true
      responses:
        '200':
          description: Success
  /jpeg:
    post:
      operationId: uploadJpeg
      requestBody:
        content:
          image/jpeg:
            schema:
              type: string
              format: binary
        required: true
      responses:
        '200':
          description: Success
  /gif:
    post:
      operationId: uploadGif
      requestBody:
        content:
          image/gif:
            schema:
              type: string
              format: binary
        required: true
      responses:
        '200':
          description: Success
  /svg:
    post:
      operationId: uploadSvg
      requestBody:
        content:
          image/svg+xml:
            schema:
              type: string
        required: true
      responses:
        '200':
          description: Success
"#;

    // Write spec to temp file
    let spec_file = _temp_dir.path().join("image-types.yaml");
    std::fs::write(&spec_file, spec_content).unwrap();

    // Add spec in non-strict mode
    let result = config_manager.add_spec("image-test", &spec_file, false, false);
    assert!(result.is_ok(), "Should accept spec in non-strict mode");

    // All image endpoints should be skipped
    let cache_dir = _temp_dir.path().join(".cache");
    let cached_spec = load_cached_spec(&cache_dir, "image-test").unwrap();
    assert_eq!(
        cached_spec.commands.len(),
        0,
        "All image endpoints should be skipped"
    );
}

#[test]
fn test_json_content_type_variations() {
    let (config_manager, _temp_dir) = create_temp_config_manager();

    // Create a spec with various valid JSON content type variations
    let spec_content = r#"
openapi: 3.0.0
info:
  title: JSON Content Type Variations
  version: 1.0.0
servers:
  - url: https://api.example.com
paths:
  /standard:
    post:
      operationId: postStandard
      requestBody:
        content:
          application/json:
            schema:
              type: object
        required: true
      responses:
        '200':
          description: Success
  /uppercase:
    post:
      operationId: postUppercase
      requestBody:
        content:
          APPLICATION/JSON:
            schema:
              type: object
        required: true
      responses:
        '200':
          description: Success
  /mixedcase:
    post:
      operationId: postMixedCase
      requestBody:
        content:
          Application/Json:
            schema:
              type: object
        required: true
      responses:
        '200':
          description: Success
  /charset:
    post:
      operationId: postCharset
      requestBody:
        content:
          "application/json; charset=utf-8":
            schema:
              type: object
        required: true
      responses:
        '200':
          description: Success
  /boundary:
    post:
      operationId: postBoundary
      requestBody:
        content:
          "application/json; boundary=something":
            schema:
              type: object
        required: true
      responses:
        '200':
          description: Success
  /spaces:
    post:
      operationId: postSpaces
      requestBody:
        content:
          "  application/json  ":
            schema:
              type: object
        required: true
      responses:
        '200':
          description: Success
  /tabs:
    post:
      operationId: postTabs
      requestBody:
        content:
          "\tapplication/json\t":
            schema:
              type: object
        required: true
      responses:
        '200':
          description: Success
  /jsonapi:
    post:
      operationId: postJsonApi
      requestBody:
        content:
          application/vnd.api+json:
            schema:
              type: object
        required: true
      responses:
        '200':
          description: Success
  /jsonld:
    post:
      operationId: postJsonLd
      requestBody:
        content:
          application/ld+json:
            schema:
              type: object
        required: true
      responses:
        '200':
          description: Success
  /hal:
    post:
      operationId: postHal
      requestBody:
        content:
          application/hal+json:
            schema:
              type: object
        required: true
      responses:
        '200':
          description: Success
  /problem:
    post:
      operationId: postProblem
      requestBody:
        content:
          application/problem+json:
            schema:
              type: object
        required: true
      responses:
        '200':
          description: Success
  /custom:
    post:
      operationId: postCustom
      requestBody:
        content:
          application/vnd.custom+json:
            schema:
              type: object
        required: true
      responses:
        '200':
          description: Success
"#;

    // Write spec to temp file
    let spec_file = _temp_dir.path().join("json-variations.yaml");
    std::fs::write(&spec_file, spec_content).unwrap();

    // Add spec in non-strict mode
    let result = config_manager.add_spec("json-test", &spec_file, false, false);
    assert!(
        result.is_ok(),
        "Should accept spec with JSON content type variations"
    );

    // All endpoints should be accepted
    let cache_dir = _temp_dir.path().join(".cache");
    let cached_spec = load_cached_spec(&cache_dir, "json-test").unwrap();
    assert_eq!(
        cached_spec.commands.len(),
        12,
        "All JSON content type variations should be accepted"
    );

    // Verify all operations are included
    let operation_ids: Vec<&str> = cached_spec
        .commands
        .iter()
        .map(|cmd| cmd.operation_id.as_str())
        .collect();

    assert!(operation_ids.contains(&"postStandard"));
    assert!(operation_ids.contains(&"postUppercase"));
    assert!(operation_ids.contains(&"postMixedCase"));
    assert!(operation_ids.contains(&"postCharset"));
    assert!(operation_ids.contains(&"postBoundary"));
    assert!(operation_ids.contains(&"postSpaces"));
    assert!(operation_ids.contains(&"postTabs"));
    assert!(operation_ids.contains(&"postJsonApi"));
    assert!(operation_ids.contains(&"postJsonLd"));
    assert!(operation_ids.contains(&"postHal"));
    assert!(operation_ids.contains(&"postProblem"));
    assert!(operation_ids.contains(&"postCustom"));
}

#[test]
fn test_mixed_content_type_warnings() {
    let (_config_manager, _temp_dir) = create_temp_config_manager();

    // Create a spec with endpoints that have mixed content types
    let spec_content = r#"
openapi: 3.0.0
info:
  title: Mixed Content API
  version: 1.0.0
servers:
  - url: https://api.example.com
paths:
  /upload:
    post:
      operationId: uploadMixed
      requestBody:
        content:
          application/json:
            schema:
              type: object
          multipart/form-data:
            schema:
              type: object
          application/xml:
            schema:
              type: object
        required: true
      responses:
        '200':
          description: Success
  /data:
    put:
      operationId: putData
      requestBody:
        content:
          text/plain:
            schema:
              type: string
          application/json:
            schema:
              type: object
        required: true
      responses:
        '200':
          description: Success
  /json-only:
    post:
      operationId: postJsonOnly
      requestBody:
        content:
          application/json:
            schema:
              type: object
        required: true
      responses:
        '200':
          description: Success
  /no-json:
    post:
      operationId: postNoJson
      requestBody:
        content:
          multipart/form-data:
            schema:
              type: object
        required: true
      responses:
        '200':
          description: Success
"#;

    // Write spec to temp file
    let spec_file = _temp_dir.path().join("mixed-warnings.yaml");
    std::fs::write(&spec_file, spec_content).unwrap();

    // Use a custom test that captures stderr to check warnings
    use std::process::Command;
    let output = Command::new(env!("CARGO_BIN_EXE_aperture"))
        .env("APERTURE_CONFIG_DIR", _temp_dir.path().to_str().unwrap())
        .args(["config", "add", "mixed-test", spec_file.to_str().unwrap()])
        .output()
        .unwrap();

    assert!(
        output.status.success(),
        "Should accept spec in non-strict mode"
    );

    let stderr = String::from_utf8_lossy(&output.stderr);

    // Check for skipped endpoint warning
    assert!(stderr.contains("Warning: Skipping 1 endpoints with unsupported content types"));
    assert!(stderr.contains("POST /no-json"));
    assert!(stderr.contains("endpoint has no supported content types"));

    // Check for mixed content warnings
    assert!(stderr.contains("Warning: 2 endpoints have partial content type support:"));
    assert!(stderr.contains("POST /upload supports JSON but not:"));
    assert!(stderr.contains("multipart/form-data"));
    assert!(stderr.contains("application/xml"));
    assert!(stderr.contains("PUT /data supports JSON but not: text/plain"));

    // Verify the correct endpoints were included in cache
    let cache_dir = _temp_dir.path().join(".cache");
    let cached_spec = load_cached_spec(&cache_dir, "mixed-test").unwrap();

    // Should have 3 endpoints (excluding /no-json)
    assert_eq!(cached_spec.commands.len(), 3);

    let operation_ids: Vec<&str> = cached_spec
        .commands
        .iter()
        .map(|cmd| cmd.operation_id.as_str())
        .collect();

    assert!(operation_ids.contains(&"uploadMixed"));
    assert!(operation_ids.contains(&"putData"));
    assert!(operation_ids.contains(&"postJsonOnly"));
    assert!(!operation_ids.contains(&"postNoJson"));
}

#[test]
fn test_empty_request_body_content() {
    let (config_manager, _temp_dir) = create_temp_config_manager();

    // Create a spec with endpoints that have empty request body content
    let spec_content = r#"
openapi: 3.0.0
info:
  title: Empty Content API
  version: 1.0.0
servers:
  - url: https://api.example.com
paths:
  /users:
    post:
      operationId: createUser
      requestBody:
        content: {}
        required: true
      responses:
        '200':
          description: Success
  /data:
    put:
      operationId: updateData
      requestBody:
        content: {}
        required: false
      responses:
        '200':
          description: Success
"#;

    // Write spec to temp file
    let spec_file = _temp_dir.path().join("empty-content.yaml");
    std::fs::write(&spec_file, spec_content).unwrap();

    // Add spec - should succeed even with empty content
    let result = config_manager.add_spec("empty-test", &spec_file, false, false);
    assert!(
        result.is_ok(),
        "Should accept spec with empty request body content"
    );

    // Verify endpoints were included
    let cache_dir = _temp_dir.path().join(".cache");
    let cached_spec = load_cached_spec(&cache_dir, "empty-test").unwrap();

    // Both endpoints should be included
    assert_eq!(cached_spec.commands.len(), 2);

    let operation_ids: Vec<&str> = cached_spec
        .commands
        .iter()
        .map(|cmd| cmd.operation_id.as_str())
        .collect();

    assert!(operation_ids.contains(&"createUser"));
    assert!(operation_ids.contains(&"updateData"));
}
