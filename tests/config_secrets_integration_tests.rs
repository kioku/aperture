#![cfg(feature = "integration")]

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

#[test]
fn test_config_remove_secret_success() {
    let temp_dir = TempDir::new().unwrap();
    let config_dir = temp_dir.path().join(".config").join("aperture");

    // Create a simple OpenAPI spec with security schemes
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
    apiKey:
      type: apiKey
      in: header
      name: X-API-Key
paths:
  /test:
    get:
      security:
        - bearerAuth: []
        - apiKey: []
      responses:
        '200':
          description: Success
servers:
  - url: https://api.example.com
"#;

    let spec_file = temp_dir.path().join("test-spec.yaml");
    fs::write(&spec_file, spec_content).unwrap();

    // Add the spec
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

    // Set two secrets
    let mut cmd = Command::cargo_bin("aperture").unwrap();
    cmd.env("APERTURE_CONFIG_DIR", config_dir.to_str().unwrap())
        .args(&[
            "config",
            "set-secret",
            "test-api",
            "bearerAuth",
            "--env",
            "BEARER_TOKEN",
        ])
        .assert()
        .success();

    let mut cmd = Command::cargo_bin("aperture").unwrap();
    cmd.env("APERTURE_CONFIG_DIR", config_dir.to_str().unwrap())
        .args(&[
            "config",
            "set-secret",
            "test-api",
            "apiKey",
            "--env",
            "API_KEY",
        ])
        .assert()
        .success();

    // Verify both secrets are configured
    let mut cmd = Command::cargo_bin("aperture").unwrap();
    cmd.env("APERTURE_CONFIG_DIR", config_dir.to_str().unwrap())
        .args(&["config", "list-secrets", "test-api"])
        .assert()
        .success()
        .stdout(predicate::str::contains("bearerAuth"))
        .stdout(predicate::str::contains("apiKey"));

    // Remove one secret
    let mut cmd = Command::cargo_bin("aperture").unwrap();
    cmd.env("APERTURE_CONFIG_DIR", config_dir.to_str().unwrap())
        .args(&["config", "remove-secret", "test-api", "bearerAuth"])
        .assert()
        .success()
        .stdout(predicate::str::contains(
            "Removed secret configuration for scheme 'bearerAuth'",
        ));

    // Verify only one secret remains
    let mut cmd = Command::cargo_bin("aperture").unwrap();
    cmd.env("APERTURE_CONFIG_DIR", config_dir.to_str().unwrap())
        .args(&["config", "list-secrets", "test-api"])
        .assert()
        .success()
        .stdout(predicate::str::contains("apiKey"))
        .stdout(predicate::str::contains("bearerAuth").not());
}

#[test]
fn test_config_remove_secret_nonexistent_api() {
    let temp_dir = TempDir::new().unwrap();
    let config_dir = temp_dir.path().join(".config").join("aperture");

    // Try to remove secret from nonexistent API
    let mut cmd = Command::cargo_bin("aperture").unwrap();
    cmd.env("APERTURE_CONFIG_DIR", config_dir.to_str().unwrap())
        .args(&["config", "remove-secret", "nonexistent-api", "bearerAuth"])
        .assert()
        .failure()
        .stderr(predicate::str::contains(
            "API specification 'nonexistent-api' not found",
        ));
}

#[test]
fn test_config_remove_secret_nonexistent_scheme() {
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

    // Add the spec
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

    // Try to remove a secret that was never configured
    let mut cmd = Command::cargo_bin("aperture").unwrap();
    cmd.env("APERTURE_CONFIG_DIR", config_dir.to_str().unwrap())
        .args(&["config", "remove-secret", "test-api", "bearerAuth"])
        .assert()
        .failure()
        .stderr(predicate::str::contains(
            "No secrets configured for API 'test-api'",
        ));
}

#[test]
fn test_config_remove_secret_unconfigured_scheme() {
    let temp_dir = TempDir::new().unwrap();
    let config_dir = temp_dir.path().join(".config").join("aperture");

    // Create a simple OpenAPI spec with multiple schemes
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
    apiKey:
      type: apiKey
      in: header
      name: X-API-Key
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

    // Add the spec
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

    // Configure only one secret
    let mut cmd = Command::cargo_bin("aperture").unwrap();
    cmd.env("APERTURE_CONFIG_DIR", config_dir.to_str().unwrap())
        .args(&[
            "config",
            "set-secret",
            "test-api",
            "bearerAuth",
            "--env",
            "BEARER_TOKEN",
        ])
        .assert()
        .success();

    // Try to remove the unconfigured scheme
    let mut cmd = Command::cargo_bin("aperture").unwrap();
    cmd.env("APERTURE_CONFIG_DIR", config_dir.to_str().unwrap())
        .args(&["config", "remove-secret", "test-api", "apiKey"])
        .assert()
        .failure()
        .stderr(predicate::str::contains(
            "Secret for scheme 'apiKey' is not configured for API 'test-api'",
        ));
}

#[test]
fn test_config_clear_secrets_success() {
    let temp_dir = TempDir::new().unwrap();
    let config_dir = temp_dir.path().join(".config").join("aperture");

    // Create a simple OpenAPI spec with security schemes
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
    apiKey:
      type: apiKey
      in: header
      name: X-API-Key
paths:
  /test:
    get:
      security:
        - bearerAuth: []
        - apiKey: []
      responses:
        '200':
          description: Success
servers:
  - url: https://api.example.com
"#;

    let spec_file = temp_dir.path().join("test-spec.yaml");
    fs::write(&spec_file, spec_content).unwrap();

    // Add the spec
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

    // Set multiple secrets
    let mut cmd = Command::cargo_bin("aperture").unwrap();
    cmd.env("APERTURE_CONFIG_DIR", config_dir.to_str().unwrap())
        .args(&[
            "config",
            "set-secret",
            "test-api",
            "bearerAuth",
            "--env",
            "BEARER_TOKEN",
        ])
        .assert()
        .success();

    let mut cmd = Command::cargo_bin("aperture").unwrap();
    cmd.env("APERTURE_CONFIG_DIR", config_dir.to_str().unwrap())
        .args(&[
            "config",
            "set-secret",
            "test-api",
            "apiKey",
            "--env",
            "API_KEY",
        ])
        .assert()
        .success();

    // Clear all secrets with --force to skip confirmation
    let mut cmd = Command::cargo_bin("aperture").unwrap();
    cmd.env("APERTURE_CONFIG_DIR", config_dir.to_str().unwrap())
        .args(&["config", "clear-secrets", "test-api", "--force"])
        .assert()
        .success()
        .stdout(predicate::str::contains(
            "Cleared all secret configurations for API 'test-api'",
        ));

    // Verify no secrets remain
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
fn test_config_clear_secrets_nonexistent_api() {
    let temp_dir = TempDir::new().unwrap();
    let config_dir = temp_dir.path().join(".config").join("aperture");

    // Try to clear secrets from nonexistent API
    let mut cmd = Command::cargo_bin("aperture").unwrap();
    cmd.env("APERTURE_CONFIG_DIR", config_dir.to_str().unwrap())
        .args(&["config", "clear-secrets", "nonexistent-api", "--force"])
        .assert()
        .failure()
        .stderr(predicate::str::contains(
            "API specification 'nonexistent-api' not found",
        ));
}

#[test]
fn test_config_clear_secrets_no_secrets_configured() {
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

    // Add the spec
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

    // Try to clear secrets when none are configured
    let mut cmd = Command::cargo_bin("aperture").unwrap();
    cmd.env("APERTURE_CONFIG_DIR", config_dir.to_str().unwrap())
        .args(&["config", "clear-secrets", "test-api", "--force"])
        .assert()
        .success()
        .stdout(predicate::str::contains(
            "No secrets configured for API 'test-api'",
        ));
}
