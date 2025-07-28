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
fn test_server_url_template_error_message() {
    let (_temp_dir, home_dir) = setup_test_env();

    // Create a Sentry-like OpenAPI spec with server URL template
    let sentry_spec = r#"
openapi: 3.0.0
info:
  title: Sentry API
  version: 1.0.0
servers:
  - url: https://{region}.sentry.io
    variables:
      region:
        default: us
        enum: [us, eu]
        description: The regional instance of Sentry
paths:
  /api/0/projects/{organization}/{project}/events/:
    get:
      operationId: listEvents
      tags: [events]
      summary: List project events
      parameters:
        - name: organization
          in: path
          required: true
          schema:
            type: string
        - name: project
          in: path
          required: true
          schema:
            type: string
      responses:
        '200':
          description: Success
          content:
            application/json:
              schema:
                type: array
                items:
                  type: object
"#;

    let spec_file = home_dir.path().join("sentry.yaml");
    fs::write(&spec_file, sentry_spec).unwrap();

    // Add the spec
    Command::cargo_bin("aperture")
        .unwrap()
        .env("HOME", home_dir.path())
        .args(&["config", "add", "sentry", spec_file.to_str().unwrap()])
        .assert()
        .success();

    // Try to execute a command - should fail with template error
    Command::cargo_bin("aperture")
        .unwrap()
        .env("HOME", home_dir.path())
        .args(&[
            "api",
            "sentry",
            "events",
            "list-events",
            "--organization",
            "my-org",
            "--project",
            "my-project",
        ])
        .assert()
        .failure()
        .stderr(predicate::str::contains(
            "Server URL contains template variable(s): https://{region}.sentry.io",
        ))
        .stderr(predicate::str::contains("aperture config set-url sentry"));
}

#[test]
fn test_server_url_template_with_override() {
    let (_temp_dir, home_dir) = setup_test_env();

    // Create a spec with server URL template
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

    // First attempt should fail
    Command::cargo_bin("aperture")
        .unwrap()
        .env("HOME", home_dir.path())
        .args(&["api", "regional", "health", "get-status"])
        .assert()
        .failure()
        .stderr(predicate::str::contains(
            "Server URL contains template variable(s)",
        ));

    // Set a base URL override
    Command::cargo_bin("aperture")
        .unwrap()
        .env("HOME", home_dir.path())
        .args(&[
            "config",
            "set-url",
            "regional",
            "https://prod.api.example.com",
        ])
        .assert()
        .success();

    // Now it should work (would fail at network level, but not template error)
    Command::cargo_bin("aperture")
        .unwrap()
        .env("HOME", home_dir.path())
        .args(&["--dry-run", "api", "regional", "health", "get-status"])
        .assert()
        .success()
        .stdout(predicate::str::contains(
            "https://prod.api.example.com/status",
        ));
}

#[test]
fn test_multiple_server_url_templates() {
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

    // Should fail with error mentioning the templated URL
    Command::cargo_bin("aperture")
        .unwrap()
        .env("HOME", home_dir.path())
        .args(&["api", "multi", "health", "ping"])
        .assert()
        .failure()
        .stderr(predicate::str::contains(
            "https://{region}-{env}.api.example.com",
        ))
        .stderr(predicate::str::contains("aperture config set-url multi"));
}
