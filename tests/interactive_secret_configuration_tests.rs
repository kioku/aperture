use assert_cmd::Command;
use predicates::prelude::*;
use std::fs;
use tempfile::TempDir;

#[test]
fn test_interactive_mode_flag_exists() {
    let mut cmd = Command::cargo_bin("aperture").unwrap();
    cmd.args(&["config", "set-secret", "--help"])
        .assert()
        .success()
        .stdout(predicate::str::contains("--interactive"))
        .stdout(predicate::str::contains("Configure secrets interactively"));
}

#[test]
fn test_interactive_mode_conflicts_with_direct_mode() {
    let temp_dir = TempDir::new().unwrap();
    let config_dir = temp_dir.path().join(".config").join("aperture");

    let mut cmd = Command::cargo_bin("aperture").unwrap();
    cmd.env("APERTURE_CONFIG_DIR", config_dir.to_str().unwrap())
        .args(&[
            "config",
            "set-secret",
            "test-api",
            "bearerAuth",
            "--env",
            "TOKEN",
            "--interactive",
        ])
        .assert()
        .failure()
        .stderr(predicate::str::contains(
            "cannot be used with '--interactive'",
        ));
}

#[test]
fn test_interactive_mode_with_nonexistent_api() {
    let temp_dir = TempDir::new().unwrap();
    let config_dir = temp_dir.path().join(".config").join("aperture");

    let mut cmd = Command::cargo_bin("aperture").unwrap();
    cmd.env("APERTURE_CONFIG_DIR", config_dir.to_str().unwrap())
        .args(&["config", "set-secret", "nonexistent-api", "--interactive"])
        .assert()
        .failure()
        .stderr(predicate::str::contains(
            "API specification 'nonexistent-api' not found",
        ));
}

#[test]
fn test_interactive_mode_with_api_without_security_schemes() {
    let temp_dir = TempDir::new().unwrap();
    let config_dir = temp_dir.path().join(".config").join("aperture");

    // Create a simple OpenAPI spec without security schemes
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

    // Try interactive mode - should handle gracefully and exit successfully
    let mut cmd = Command::cargo_bin("aperture").unwrap();
    cmd.env("APERTURE_CONFIG_DIR", config_dir.to_str().unwrap())
        .args(&["config", "set-secret", "test-api", "--interactive"])
        .assert()
        .success()
        .stdout(predicate::str::contains(
            "No security schemes found in API 'test-api'",
        ));
}

#[test]
fn test_help_text_describes_interactive_workflow() {
    let mut cmd = Command::cargo_bin("aperture").unwrap();
    cmd.args(&["config", "set-secret", "--help"])
        .assert()
        .success()
        .stdout(predicate::str::contains("Configure secrets interactively"))
        .stdout(predicate::str::contains("--interactive"))
        .stdout(predicate::str::contains("--env"))
        .stdout(predicate::str::contains("Environment variable name"));
}

#[test]
fn test_validation_requires_either_direct_or_interactive_mode() {
    let temp_dir = TempDir::new().unwrap();
    let config_dir = temp_dir.path().join(".config").join("aperture");

    // Test missing both modes
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
fn test_direct_mode_still_works_alongside_interactive_option() {
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
        .success();

    // Test direct mode still works
    let mut cmd = Command::cargo_bin("aperture").unwrap();
    cmd.env("APERTURE_CONFIG_DIR", config_dir.to_str().unwrap())
        .args(&[
            "config",
            "set-secret",
            "test-api",
            "bearerAuth",
            "--env",
            "TEST_TOKEN",
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains(
            "Set secret for scheme 'bearerAuth' in API 'test-api' to use environment variable 'TEST_TOKEN'",
        ));

    // Verify it was set
    let mut cmd = Command::cargo_bin("aperture").unwrap();
    cmd.env("APERTURE_CONFIG_DIR", config_dir.to_str().unwrap())
        .args(&["config", "list-secrets", "test-api"])
        .assert()
        .success()
        .stdout(predicate::str::contains(
            "bearerAuth: environment variable 'TEST_TOKEN'",
        ));
}

#[test]
fn test_cli_help_shows_both_modes() {
    let mut cmd = Command::cargo_bin("aperture").unwrap();
    cmd.args(&["config", "set-secret", "--help"])
        .assert()
        .success()
        .stdout(predicate::str::contains("Examples:"))
        .stdout(predicate::str::contains(
            "aperture config set-secret myapi bearerAuth --env API_TOKEN",
        ))
        .stdout(predicate::str::contains(
            "aperture config set-secret myapi --interactive",
        ));
}
