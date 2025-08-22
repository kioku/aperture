#![cfg(feature = "integration")]

use assert_cmd::Command;
use predicates::prelude::*;
use std::fs;
use tempfile::TempDir;

/// Helper to create test directory with config
fn setup_test_env() -> (TempDir, TempDir) {
    let temp_dir = TempDir::new().unwrap();
    let home_dir = TempDir::new().unwrap();
    let config_dir = home_dir.path().join(".config").join("aperture");
    let specs_dir = config_dir.join("specs");
    let cache_dir = config_dir.join(".cache");

    fs::create_dir_all(&specs_dir).unwrap();
    fs::create_dir_all(&cache_dir).unwrap();

    // Create empty config.toml
    let config_content = r#"
[general]
default_output_format = "json"

[api_configs]
"#;
    fs::write(config_dir.join("config.toml"), config_content).unwrap();

    (temp_dir, home_dir)
}

#[test]
fn test_server_url_template_invalid_default_enum_value() {
    let (_temp_dir, home_dir) = setup_test_env();

    // Create an API spec where the default value is not in the enum
    // This tests that enum validation is applied even to default values
    let api_spec = r#"
openapi: 3.0.0
info:
  title: Test API
  version: 1.0.0
servers:
  - url: https://{region}.api.example.com
    variables:
      region:
        default: "invalid-region"
        enum: [us, eu]
        description: The regional instance
paths:
  /test:
    get:
      operationId: test
      tags: [test]
      summary: Test endpoint
      responses:
        '200':
          description: Success
"#;

    let spec_file = home_dir.path().join("test-api.yaml");
    fs::write(&spec_file, api_spec).unwrap();

    // Add the spec
    Command::cargo_bin("aperture")
        .unwrap()
        .env("HOME", home_dir.path())
        .args(&["config", "add", "test-api", spec_file.to_str().unwrap()])
        .assert()
        .success();

    // Try to execute without providing server variable - should fail with invalid enum value
    Command::cargo_bin("aperture")
        .unwrap()
        .env("HOME", home_dir.path())
        .args(&["api", "test-api", "test", "test"])
        .assert()
        .failure()
        .stderr(predicate::str::contains(
            "Invalid value 'invalid-region' for server variable 'region'",
        ))
        .stderr(predicate::str::contains("Allowed values: us, eu"));
}

#[test]
fn test_server_url_template_with_defaults_and_override() {
    let (_temp_dir, home_dir) = setup_test_env();

    // Create a spec with server URL template with default values
    let spec_content = r#"
openapi: 3.0.0
info:
  title: Regional API
  version: 1.0.0
servers:
  - url: https://{env}.api.example.com
    variables:
      env:
        default: prod
        enum: [dev, staging, prod]
paths:
  /status:
    get:
      operationId: getStatus
      tags: [health]
      summary: Get API status
      responses:
        '200':
          description: Success
"#;

    let spec_file = home_dir.path().join("regional.yaml");
    fs::write(&spec_file, spec_content).unwrap();

    // Add the spec
    Command::cargo_bin("aperture")
        .unwrap()
        .env("HOME", home_dir.path())
        .args(&["config", "add", "regional", spec_file.to_str().unwrap()])
        .assert()
        .success();

    // Should work with default values using dry-run to avoid network calls
    Command::cargo_bin("aperture")
        .unwrap()
        .env("HOME", home_dir.path())
        .args(&["--dry-run", "api", "regional", "health", "get-status"])
        .assert()
        .success()
        .stdout(predicate::str::contains(
            "https://prod.api.example.com/status",
        ));

    // Should work with explicit server variable
    Command::cargo_bin("aperture")
        .unwrap()
        .env("HOME", home_dir.path())
        .args(&[
            "--dry-run",
            "api",
            "regional",
            "health",
            "get-status",
            "--server-var",
            "env=staging",
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains(
            "https://staging.api.example.com/status",
        ));

    // Test with config override
    Command::cargo_bin("aperture")
        .unwrap()
        .env("HOME", home_dir.path())
        .args(&[
            "config",
            "set-url",
            "regional",
            "https://override.api.example.com",
        ])
        .assert()
        .success();

    // Config override should take precedence (non-template URL)
    Command::cargo_bin("aperture")
        .unwrap()
        .env("HOME", home_dir.path())
        .args(&["--dry-run", "api", "regional", "health", "get-status"])
        .assert()
        .success()
        .stdout(predicate::str::contains(
            "https://override.api.example.com/status",
        ));
}

#[test]
fn test_multiple_server_url_template_variables() {
    let (_temp_dir, home_dir) = setup_test_env();

    // Create a spec with multiple template variables
    let spec_content = r#"
openapi: 3.0.0
info:
  title: Multi-Region API
  version: 1.0.0
servers:
  - url: https://{region}-{env}.api.example.com
    variables:
      region:
        default: us
        enum: [us, eu, ap]
      env:
        default: prod
        enum: [dev, prod]
paths:
  /ping:
    get:
      operationId: ping
      tags: [health]
      summary: Ping endpoint
      responses:
        '200':
          description: Success
"#;

    let spec_file = home_dir.path().join("multi.yaml");
    fs::write(&spec_file, spec_content).unwrap();

    // Add the spec
    Command::cargo_bin("aperture")
        .unwrap()
        .env("HOME", home_dir.path())
        .args(&["config", "add", "multi", spec_file.to_str().unwrap()])
        .assert()
        .success();

    // Should work with default values
    Command::cargo_bin("aperture")
        .unwrap()
        .env("HOME", home_dir.path())
        .args(&["--dry-run", "api", "multi", "health", "ping"])
        .assert()
        .success()
        .stdout(predicate::str::contains(
            "https://us-prod.api.example.com/ping",
        ));

    // Should work with explicit server variables
    Command::cargo_bin("aperture")
        .unwrap()
        .env("HOME", home_dir.path())
        .args(&[
            "--dry-run",
            "api",
            "multi",
            "health",
            "ping",
            "--server-var",
            "region=eu",
            "--server-var",
            "env=dev",
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains(
            "https://eu-dev.api.example.com/ping",
        ));

    // Should fail with invalid enum value
    Command::cargo_bin("aperture")
        .unwrap()
        .env("HOME", home_dir.path())
        .args(&[
            "api",
            "multi",
            "health",
            "ping",
            "--server-var",
            "region=invalid",
        ])
        .assert()
        .failure()
        .stderr(predicate::str::contains(
            "Invalid value 'invalid' for server variable 'region'",
        ))
        .stderr(predicate::str::contains("Allowed values: us, eu, ap"));
}
