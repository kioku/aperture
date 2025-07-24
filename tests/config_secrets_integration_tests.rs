use assert_cmd::Command;
use predicates::prelude::*;
use std::fs;
use tempfile::TempDir;

#[test]
fn test_config_set_secret_basic_functionality() {
    let temp_dir = TempDir::new().unwrap();
    let config_dir = temp_dir.path().join(".config").join("aperture");

    // Create a simple OpenAPI spec
    let spec_content = r#"
openapi: 3.0.0
info:
  title: Test API
  version: 1.0.0
components:
  securitySchemes:
    bearerAuth:
      type: http
      scheme: bearer
paths:
  /test:
    get:
      security:
        - bearerAuth: []
      responses:
        '200':
          description: Success
servers:
  - url: https://api.example.com
"#;

    let spec_file = temp_dir.path().join("test-spec.yaml");
    fs::write(&spec_file, spec_content).unwrap();

    // Add the spec first
    let mut cmd = Command::cargo_bin("aperture").unwrap();
    cmd.env("APERTURE_CONFIG_DIR", config_dir.to_str().unwrap())
        .args(&[
            "config",
            "add",
            "test-api",
            spec_file.to_string_lossy().as_ref(),
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains(
            "Spec 'test-api' added successfully",
        ));

    // Set a secret for the bearerAuth scheme
    let mut cmd = Command::cargo_bin("aperture").unwrap();
    cmd.env("APERTURE_CONFIG_DIR", config_dir.to_str().unwrap())
        .args(&["config", "set-secret", "test-api", "bearerAuth", "--env", "TEST_BEARER_TOKEN"])
        .assert()
        .success()
        .stdout(predicate::str::contains("Set secret for scheme 'bearerAuth' in API 'test-api' to use environment variable 'TEST_BEARER_TOKEN'"));

    // List secrets to verify it was set
    let mut cmd = Command::cargo_bin("aperture").unwrap();
    cmd.env("APERTURE_CONFIG_DIR", config_dir.to_str().unwrap())
        .args(&["config", "list-secrets", "test-api"])
        .assert()
        .success()
        .stdout(predicate::str::contains(
            "Configured secrets for API 'test-api':",
        ))
        .stdout(predicate::str::contains(
            "bearerAuth: environment variable 'TEST_BEARER_TOKEN'",
        ));
}

#[test]
fn test_config_set_secret_nonexistent_api() {
    let temp_dir = TempDir::new().unwrap();
    let config_dir = temp_dir.path().join(".config").join("aperture");
    // Try to set secret for non-existent API
    let mut cmd = Command::cargo_bin("aperture").unwrap();
    cmd.env("APERTURE_CONFIG_DIR", config_dir.to_str().unwrap())
        .args(&[
            "config",
            "set-secret",
            "nonexistent-api",
            "bearerAuth",
            "--env",
            "TOKEN",
        ])
        .assert()
        .failure()
        .stderr(predicate::str::contains(
            "API specification 'nonexistent-api' not found",
        ));
}

#[test]
fn test_config_list_secrets_empty() {
    let temp_dir = TempDir::new().unwrap();
    let config_dir = temp_dir.path().join(".config").join("aperture");

    // Create a simple OpenAPI spec
    let spec_content = r#"
openapi: 3.0.0
info:
  title: Test API
  version: 1.0.0
paths:
  /test:
    get:
      responses:
        '200':
          description: Success
servers:
  - url: https://api.example.com
"#;

    let spec_file = temp_dir.path().join("test-spec.yaml");
    fs::write(&spec_file, spec_content).unwrap();

    // Add the spec first
    let mut cmd = Command::cargo_bin("aperture").unwrap();
    cmd.env("APERTURE_CONFIG_DIR", config_dir.to_str().unwrap())
        .args(&[
            "config",
            "add",
            "test-api",
            spec_file.to_string_lossy().as_ref(),
        ])
        .assert()
        .success();

    // List secrets for API with no configured secrets
    let mut cmd = Command::cargo_bin("aperture").unwrap();
    cmd.env("APERTURE_CONFIG_DIR", config_dir.to_str().unwrap())
        .args(&["config", "list-secrets", "test-api"])
        .assert()
        .success()
        .stdout(predicate::str::contains(
            "No secrets configured for API 'test-api'",
        ));
}

#[test]
fn test_config_set_secret_invalid_arguments() {
    let temp_dir = TempDir::new().unwrap();
    let config_dir = temp_dir.path().join(".config").join("aperture");
    // Try to set secret without providing both scheme and env
    let mut cmd = Command::cargo_bin("aperture").unwrap();
    cmd.env("APERTURE_CONFIG_DIR", config_dir.to_str().unwrap())
        .args(&["config", "set-secret", "test-api", "bearerAuth"])
        .assert()
        .failure()
        .stderr(predicate::str::contains(
            "Either provide --scheme and --env, or use --interactive",
        ));
}
