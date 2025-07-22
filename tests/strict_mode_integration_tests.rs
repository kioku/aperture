use aperture_cli::config::manager::ConfigManager;
use aperture_cli::engine::loader::load_cached_spec;
use aperture_cli::fs::OsFileSystem;
use assert_cmd::Command;
use predicates::prelude::*;
use std::path::Path;
use tempfile::TempDir;

fn create_temp_config_manager() -> (ConfigManager<OsFileSystem>, TempDir) {
    let temp_dir = TempDir::new().expect("Failed to create temp directory");
    let config_manager = ConfigManager::with_fs(OsFileSystem, temp_dir.path().to_path_buf());
    (config_manager, temp_dir)
}

#[test]
fn test_non_strict_mode_accepts_spec_with_multipart() {
    let (config_manager, _temp_dir) = create_temp_config_manager();

    // Add spec with multipart endpoints (non-strict mode - default)
    let spec_path = Path::new("tests/fixtures/openapi/spec-with-multipart.yaml");
    let result = config_manager.add_spec("test-multipart", spec_path, false, false);

    assert!(
        result.is_ok(),
        "Non-strict mode should accept spec with multipart endpoints"
    );

    // Verify the spec was cached
    let cache_dir = _temp_dir.path().join(".cache");
    let cached_spec = load_cached_spec(&cache_dir, "test-multipart").unwrap();

    // Verify that only JSON endpoints were included
    assert_eq!(
        cached_spec.commands.len(),
        3,
        "Should have 3 commands (excluding multipart endpoints)"
    );

    let operation_ids: Vec<&str> = cached_spec
        .commands
        .iter()
        .map(|cmd| cmd.operation_id.as_str())
        .collect();

    assert!(operation_ids.contains(&"getUsers"));
    assert!(operation_ids.contains(&"getUserById"));
    assert!(operation_ids.contains(&"generateReport"));

    // Multipart endpoints should be excluded
    assert!(!operation_ids.contains(&"uploadUserAvatar"));
    assert!(!operation_ids.contains(&"uploadDocument"));
}

#[test]
fn test_strict_mode_rejects_spec_with_multipart() {
    let (config_manager, _temp_dir) = create_temp_config_manager();

    // Add spec with multipart endpoints (strict mode)
    let spec_path = Path::new("tests/fixtures/openapi/spec-with-multipart.yaml");
    let result = config_manager.add_spec("test-multipart", spec_path, false, true);

    assert!(
        result.is_err(),
        "Strict mode should reject spec with multipart endpoints"
    );

    match result.unwrap_err() {
        aperture_cli::error::Error::Validation(msg) => {
            assert!(
                msg.contains("multipart/form-data"),
                "Error should mention multipart/form-data"
            );
            assert!(
                msg.contains("v1.0"),
                "Error should mention version limitation"
            );
        }
        _ => panic!("Expected Validation error"),
    }
}

#[test]
fn test_cli_non_strict_mode_with_warnings() {
    let temp_dir = TempDir::new().unwrap();
    let config_dir = temp_dir.path().join(".aperture");
    let spec_path = Path::new("tests/fixtures/openapi/spec-with-multipart.yaml")
        .canonicalize()
        .unwrap();

    // Add spec without --strict flag (default non-strict mode)
    let output = Command::cargo_bin("aperture")
        .unwrap()
        .env("APERTURE_CONFIG_DIR", config_dir.to_str().unwrap())
        .args(["config", "add", "test-api", spec_path.to_str().unwrap()])
        .output()
        .unwrap();

    let stderr = String::from_utf8_lossy(&output.stderr);
    let stdout = String::from_utf8_lossy(&output.stdout);

    assert!(
        output.status.success(),
        "Command should succeed in non-strict mode. stderr: {}, stdout: {}",
        stderr,
        stdout
    );

    // Check for warning messages
    assert!(
        stderr.contains("Warning: Skipping"),
        "Should show warning about skipping endpoints"
    );
    assert!(
        stderr.contains("endpoints with unsupported content types"),
        "Should mention unsupported content types"
    );
    assert!(
        stderr.contains("multipart/form-data"),
        "Should mention specific content type"
    );
    assert!(
        stderr.contains("POST /users/{userId}/avatar"),
        "Should list specific endpoints"
    );
    assert!(
        stderr.contains("POST /documents"),
        "Should list all skipped endpoints"
    );
    assert!(
        stderr.contains("Use --strict to reject specs"),
        "Should mention --strict flag"
    );

    // Verify spec was added
    Command::cargo_bin("aperture")
        .unwrap()
        .env("APERTURE_CONFIG_DIR", config_dir.to_str().unwrap())
        .args(["config", "list"])
        .assert()
        .success()
        .stdout(predicate::str::contains("test-api"));
}

#[test]
fn test_cli_strict_mode_rejection() {
    let temp_dir = TempDir::new().unwrap();
    let config_dir = temp_dir.path().join(".aperture");
    let spec_path = Path::new("tests/fixtures/openapi/spec-with-multipart.yaml")
        .canonicalize()
        .unwrap();

    // Add spec with --strict flag
    let output = Command::cargo_bin("aperture")
        .unwrap()
        .env("APERTURE_CONFIG_DIR", config_dir.to_str().unwrap())
        .args([
            "config",
            "add",
            "--strict",
            "test-api",
            spec_path.to_str().unwrap(),
        ])
        .output()
        .unwrap();

    let stderr = String::from_utf8_lossy(&output.stderr);

    assert!(
        !output.status.success(),
        "Command should fail in strict mode"
    );
    assert!(
        stderr.contains("Unsupported request body content type 'multipart/form-data'"),
        "Should show error about unsupported content type"
    );
    assert!(
        stderr.contains("Only 'application/json' is supported in v1.0"),
        "Should mention version limitation"
    );

    // Verify spec was NOT added
    Command::cargo_bin("aperture")
        .unwrap()
        .env("APERTURE_CONFIG_DIR", config_dir.to_str().unwrap())
        .args(["config", "list"])
        .assert()
        .success()
        .stdout(predicate::str::contains("test-api").not());
}

#[test]
fn test_cli_force_flag_with_non_strict_mode() {
    let temp_dir = TempDir::new().unwrap();
    let config_dir = temp_dir.path().join(".aperture");
    let spec_path = Path::new("tests/fixtures/openapi/spec-with-multipart.yaml")
        .canonicalize()
        .unwrap();

    // Add spec first time
    Command::cargo_bin("aperture")
        .unwrap()
        .env("APERTURE_CONFIG_DIR", config_dir.to_str().unwrap())
        .args(["config", "add", "test-api", spec_path.to_str().unwrap()])
        .assert()
        .success();

    // Try to add again without force - should fail
    Command::cargo_bin("aperture")
        .unwrap()
        .env("APERTURE_CONFIG_DIR", config_dir.to_str().unwrap())
        .args(["config", "add", "test-api", spec_path.to_str().unwrap()])
        .assert()
        .failure()
        .stderr(predicate::str::contains("already exists"));

    // Add with force flag - should succeed with warnings
    let output = Command::cargo_bin("aperture")
        .unwrap()
        .env("APERTURE_CONFIG_DIR", config_dir.to_str().unwrap())
        .args([
            "config",
            "add",
            "--force",
            "test-api",
            spec_path.to_str().unwrap(),
        ])
        .output()
        .unwrap();

    let stderr = String::from_utf8_lossy(&output.stderr);

    assert!(output.status.success(), "Force flag should allow overwrite");
    assert!(
        stderr.contains("Warning: Skipping"),
        "Should still show warnings in non-strict mode"
    );
}

#[test]
fn test_generated_commands_exclude_multipart_endpoints() {
    let temp_dir = TempDir::new().unwrap();
    let config_dir = temp_dir.path().join(".aperture");
    let spec_path = Path::new("tests/fixtures/openapi/spec-with-multipart.yaml")
        .canonicalize()
        .unwrap();

    // Add spec
    Command::cargo_bin("aperture")
        .unwrap()
        .env("APERTURE_CONFIG_DIR", config_dir.to_str().unwrap())
        .args(["config", "add", "test-api", spec_path.to_str().unwrap()])
        .assert()
        .success();

    // Check available commands - multipart endpoints should not be available
    let output = Command::cargo_bin("aperture")
        .unwrap()
        .env("APERTURE_CONFIG_DIR", config_dir.to_str().unwrap())
        .args(["api", "test-api", "users", "--help"])
        .output()
        .unwrap();

    let help_text = format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    // Should have get-users and get-user-by-id
    assert!(
        help_text.contains("get-users") || help_text.contains("Get all users"),
        "Should have get-users command"
    );
    assert!(
        help_text.contains("get-user-by-id") || help_text.contains("Get user by ID"),
        "Should have get-user-by-id command"
    );

    // Should NOT have upload-user-avatar
    assert!(
        !help_text.contains("upload-user-avatar") && !help_text.contains("Upload user avatar"),
        "Should NOT have upload-user-avatar command"
    );

    // Check documents namespace should not exist at all
    Command::cargo_bin("aperture")
        .unwrap()
        .env("APERTURE_CONFIG_DIR", config_dir.to_str().unwrap())
        .args(["api", "test-api", "documents", "--help"])
        .assert()
        .failure();
}

// Note: --describe-json shows the original OpenAPI spec, not the filtered cached spec
// This is by design, as it's used by agents to understand the full API capabilities

#[test]
fn test_path_case_sensitivity_in_filtering() {
    let (config_manager, _temp_dir) = create_temp_config_manager();

    // Create a spec with case-sensitive paths
    let spec_content = r#"
openapi: 3.0.0
info:
  title: Case Sensitive API
  version: 1.0.0
servers:
  - url: https://api.example.com
paths:
  /Users:
    get:
      operationId: getUppercaseUsers
      responses:
        '200':
          description: Success
  /users:
    get:
      operationId: getLowercaseUsers
      responses:
        '200':
          description: Success
    post:
      operationId: createUser
      requestBody:
        content:
          multipart/form-data:
            schema:
              type: object
        required: true
      responses:
        '201':
          description: Created
  /USERS:
    get:
      operationId: getAllCapsUsers
      responses:
        '200':
          description: Success
"#;

    // Write spec to temp file
    let spec_file = _temp_dir.path().join("case-sensitive.yaml");
    std::fs::write(&spec_file, spec_content).unwrap();

    // Add spec in non-strict mode
    let result = config_manager.add_spec("case-test", &spec_file, false, false);
    assert!(result.is_ok(), "Should accept spec in non-strict mode");

    // Load cached spec and verify correct endpoints were filtered
    let cache_dir = _temp_dir.path().join(".cache");
    let cached_spec = load_cached_spec(&cache_dir, "case-test").unwrap();

    // Should have 3 endpoints total (excluding only the multipart one)
    assert_eq!(cached_spec.commands.len(), 3);

    let operation_ids: Vec<&str> = cached_spec
        .commands
        .iter()
        .map(|cmd| cmd.operation_id.as_str())
        .collect();

    // All three case variants should be present
    assert!(operation_ids.contains(&"getUppercaseUsers"));
    assert!(operation_ids.contains(&"getLowercaseUsers"));
    assert!(operation_ids.contains(&"getAllCapsUsers"));

    // The multipart endpoint should be excluded
    assert!(!operation_ids.contains(&"createUser"));
}

#[test]
fn test_cli_list_verbose_shows_skipped_endpoints() {
    let temp_dir = TempDir::new().unwrap();
    let config_dir = temp_dir.path().join(".aperture");
    let spec_path = Path::new("tests/fixtures/openapi/spec-with-multipart.yaml")
        .canonicalize()
        .unwrap();

    // Add spec
    Command::cargo_bin("aperture")
        .unwrap()
        .env("APERTURE_CONFIG_DIR", config_dir.to_str().unwrap())
        .args(["config", "add", "test-api", spec_path.to_str().unwrap()])
        .assert()
        .success();

    // Run config list --verbose
    let output = Command::cargo_bin("aperture")
        .unwrap()
        .env("APERTURE_CONFIG_DIR", config_dir.to_str().unwrap())
        .args(["config", "list", "--verbose"])
        .output()
        .unwrap();

    let stdout = String::from_utf8_lossy(&output.stdout);

    assert!(output.status.success());
    assert!(stdout.contains("test-api"));
    assert!(stdout.contains("Skipped endpoints:"));
    // Check for the new format where multiple unsupported types are shown together
    assert!(stdout.contains("POST /users/{userId}/avatar"));
    assert!(stdout.contains("POST /documents"));
    assert!(stdout.contains("multipart/form-data (file uploads are not supported)"));
    assert!(stdout.contains("endpoint has no supported content types"));
}

#[test]
fn test_reinit_preserves_strict_mode_preference() {
    let temp_dir = TempDir::new().unwrap();
    let config_dir = temp_dir.path().join(".aperture");

    // Create a spec that passes non-strict but fails strict
    let spec_content = r#"
openapi: 3.0.0
info:
  title: Test API
  version: 1.0.0
servers:
  - url: https://api.example.com
paths:
  /users:
    get:
      operationId: getUsers
      responses:
        '200':
          description: Success
  /upload:
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
"#;

    let spec_file = temp_dir.path().join("test-spec.yaml");
    std::fs::write(&spec_file, spec_content).unwrap();

    // Add spec with --strict flag
    Command::cargo_bin("aperture")
        .unwrap()
        .env("APERTURE_CONFIG_DIR", config_dir.to_str().unwrap())
        .args([
            "config",
            "add",
            "--strict",
            "strict-api",
            spec_file.to_str().unwrap(),
        ])
        .assert()
        .failure()
        .stderr(predicate::str::contains(
            "Unsupported request body content type",
        ));

    // Add spec without --strict flag (should succeed)
    Command::cargo_bin("aperture")
        .unwrap()
        .env("APERTURE_CONFIG_DIR", config_dir.to_str().unwrap())
        .args([
            "config",
            "add",
            "non-strict-api",
            spec_file.to_str().unwrap(),
        ])
        .assert()
        .success();

    // Now test reinit - it should preserve the non-strict preference
    // First, corrupt the cache to force a reinit
    let cache_file = config_dir.join(".cache/non-strict-api.bin");
    std::fs::write(&cache_file, b"corrupted data").unwrap();

    // Reinit should succeed because it uses the saved non-strict preference
    Command::cargo_bin("aperture")
        .unwrap()
        .env("APERTURE_CONFIG_DIR", config_dir.to_str().unwrap())
        .args(["config", "reinit", "non-strict-api"])
        .assert()
        .success()
        .stdout(predicate::str::contains("Successfully reinitialized"));
}

#[test]
fn test_strict_mode_with_url_spec() {
    use wiremock::matchers::{method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    // Start a mock server
    let runtime = tokio::runtime::Runtime::new().unwrap();
    let mock_server = runtime.block_on(async { MockServer::start().await });

    // Create a spec with multipart content
    let spec_content = r#"
openapi: 3.0.0
info:
  title: Remote API with Multipart
  version: 1.0.0
servers:
  - url: https://api.example.com
paths:
  /users:
    get:
      operationId: getUsers
      responses:
        '200':
          description: Success
  /upload:
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
"#;

    // Mock the GET request for the spec
    runtime.block_on(async {
        Mock::given(method("GET"))
            .and(path("/spec.yaml"))
            .respond_with(ResponseTemplate::new(200).set_body_string(spec_content))
            .mount(&mock_server)
            .await;
    });

    let temp_dir = TempDir::new().unwrap();
    let config_dir = temp_dir.path().join(".aperture");
    let spec_url = format!("{}/spec.yaml", mock_server.uri());

    // Test with --strict flag (should fail)
    Command::cargo_bin("aperture")
        .unwrap()
        .env("APERTURE_CONFIG_DIR", config_dir.to_str().unwrap())
        .args(["config", "add", "--strict", "url-strict-api", &spec_url])
        .assert()
        .failure()
        .stderr(predicate::str::contains(
            "Unsupported request body content type",
        ));

    // Test without --strict flag (should succeed)
    Command::cargo_bin("aperture")
        .unwrap()
        .env("APERTURE_CONFIG_DIR", config_dir.to_str().unwrap())
        .args(["config", "add", "url-non-strict-api", &spec_url])
        .assert()
        .success()
        .stderr(predicate::str::contains("Warning: Skipping 1 endpoints"));
}
