use assert_cmd::Command;
use predicates::prelude::*;
use std::fs;
use tempfile::TempDir;
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

/// Creates a test OpenAPI spec with servers defined
fn create_test_spec_with_servers() -> &'static str {
    r#"openapi: 3.0.0
info:
  title: Test API
  version: 1.0.0
servers:
  - url: https://api.example.com
  - url: https://staging.example.com
components:
  securitySchemes:
    apiKey:
      type: apiKey
      in: header
      name: X-API-Key
      x-aperture-secret:
        env: TEST_API_KEY
paths:
  /users/{id}:
    get:
      tags:
        - users
      operationId: getUserById
      summary: Get user by ID
      parameters:
        - name: id
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
                type: object
"#
}

/// Creates a test OpenAPI spec without servers (for fallback testing)
fn create_test_spec_without_servers() -> &'static str {
    r#"openapi: 3.0.0
info:
  title: Test API No Servers
  version: 1.0.0
components:
  securitySchemes:
    apiKey:
      type: apiKey
      in: header
      name: X-API-Key
      x-aperture-secret:
        env: TEST_API_KEY
paths:
  /users/{id}:
    get:
      tags:
        - users
      operationId: getUserById
      parameters:
        - name: id
          in: path
          required: true
          schema:
            type: string
      responses:
        '200':
          description: Success
"#
}

#[tokio::test]
async fn test_base_url_priority_hierarchy() {
    let temp_dir = TempDir::new().unwrap();
    let config_dir = temp_dir.path().to_path_buf();
    let spec_file = temp_dir.path().join("test-spec.yaml");

    // Create spec with default server URL
    fs::write(&spec_file, create_test_spec_with_servers()).unwrap();

    // Add the spec
    Command::cargo_bin("aperture")
        .unwrap()
        .env("APERTURE_CONFIG_DIR", config_dir.to_str().unwrap())
        .args(&["config", "add", "test-api", spec_file.to_str().unwrap()])
        .assert()
        .success();

    let mock_server = MockServer::start().await;

    // Set up mock response (not used in this test since all requests are --dry-run)
    Mock::given(method("GET"))
        .and(path("/users/123"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "id": "123",
            "name": "Test User"
        })))
        .expect(0) // No actual requests in dry-run mode
        .mount(&mock_server)
        .await;

    // Test 1: Default (spec base URL) - should use https://api.example.com from spec
    let output = Command::cargo_bin("aperture")
        .unwrap()
        .env("APERTURE_CONFIG_DIR", config_dir.to_str().unwrap())
        .args(&[
            "api",
            "test-api",
            "--dry-run",
            "users",
            "get-user-by-id",
            "123",
        ])
        .output()
        .unwrap();

    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    let dry_run: serde_json::Value = serde_json::from_str(&stdout).unwrap();
    assert!(dry_run["url"]
        .as_str()
        .unwrap()
        .starts_with("https://api.example.com"));

    // Test 2: Environment variable override (higher priority)
    let output = Command::cargo_bin("aperture")
        .unwrap()
        .env("APERTURE_CONFIG_DIR", config_dir.to_str().unwrap())
        .env("APERTURE_BASE_URL", &mock_server.uri())
        .args(&[
            "api",
            "test-api",
            "--dry-run",
            "users",
            "get-user-by-id",
            "123",
        ])
        .output()
        .unwrap();

    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    let dry_run: serde_json::Value = serde_json::from_str(&stdout).unwrap();
    assert!(dry_run["url"]
        .as_str()
        .unwrap()
        .starts_with(&mock_server.uri()));

    // Test 3: Config override (should override env var)
    Command::cargo_bin("aperture")
        .unwrap()
        .env("APERTURE_CONFIG_DIR", config_dir.to_str().unwrap())
        .args(&[
            "config",
            "set-url",
            "test-api",
            "https://config-override.example.com",
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains(
            "Set base URL for 'test-api': https://config-override.example.com",
        ));

    // Verify config override takes precedence over env var
    let output = Command::cargo_bin("aperture")
        .unwrap()
        .env("APERTURE_CONFIG_DIR", config_dir.to_str().unwrap())
        .env("APERTURE_BASE_URL", &mock_server.uri())
        .args(&[
            "api",
            "test-api",
            "--dry-run",
            "users",
            "get-user-by-id",
            "123",
        ])
        .output()
        .unwrap();

    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    let dry_run: serde_json::Value = serde_json::from_str(&stdout).unwrap();
    assert!(dry_run["url"]
        .as_str()
        .unwrap()
        .starts_with("https://config-override.example.com"));

    // Test 4: Environment-specific config (highest priority after explicit)
    Command::cargo_bin("aperture")
        .unwrap()
        .env("APERTURE_CONFIG_DIR", config_dir.to_str().unwrap())
        .args(&[
            "config",
            "set-url",
            "test-api",
            "--env",
            "staging",
            "https://staging-env.example.com",
        ])
        .assert()
        .success();

    // With APERTURE_ENV set, environment-specific URL should win
    let output = Command::cargo_bin("aperture")
        .unwrap()
        .env("APERTURE_CONFIG_DIR", config_dir.to_str().unwrap())
        .env("APERTURE_BASE_URL", &mock_server.uri())
        .env("APERTURE_ENV", "staging")
        .args(&[
            "api",
            "test-api",
            "--dry-run",
            "users",
            "get-user-by-id",
            "123",
        ])
        .output()
        .unwrap();

    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    let dry_run: serde_json::Value = serde_json::from_str(&stdout).unwrap();
    assert!(dry_run["url"]
        .as_str()
        .unwrap()
        .starts_with("https://staging-env.example.com"));
}

#[test]
fn test_config_url_management_commands() {
    let temp_dir = TempDir::new().unwrap();
    let config_dir = temp_dir.path().to_path_buf();
    let spec_file = temp_dir.path().join("test-spec.yaml");

    // Create and add spec
    fs::write(&spec_file, create_test_spec_with_servers()).unwrap();

    Command::cargo_bin("aperture")
        .unwrap()
        .env("APERTURE_CONFIG_DIR", config_dir.to_str().unwrap())
        .args(&["config", "add", "test-api", spec_file.to_str().unwrap()])
        .assert()
        .success();

    // Test set-url command
    Command::cargo_bin("aperture")
        .unwrap()
        .env("APERTURE_CONFIG_DIR", config_dir.to_str().unwrap())
        .args(&[
            "config",
            "set-url",
            "test-api",
            "https://custom.example.com",
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains(
            "Set base URL for 'test-api': https://custom.example.com",
        ));

    // Test set-url with environment
    Command::cargo_bin("aperture")
        .unwrap()
        .env("APERTURE_CONFIG_DIR", config_dir.to_str().unwrap())
        .args(&[
            "config",
            "set-url",
            "test-api",
            "--env",
            "prod",
            "https://prod.example.com",
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains(
            "Set base URL for 'test-api' in environment 'prod': https://prod.example.com",
        ));

    Command::cargo_bin("aperture")
        .unwrap()
        .env("APERTURE_CONFIG_DIR", config_dir.to_str().unwrap())
        .args(&[
            "config",
            "set-url",
            "test-api",
            "--env",
            "dev",
            "https://dev.example.com",
        ])
        .assert()
        .success();

    // Test get-url command
    Command::cargo_bin("aperture")
        .unwrap()
        .env("APERTURE_CONFIG_DIR", config_dir.to_str().unwrap())
        .args(&["config", "get-url", "test-api"])
        .assert()
        .success()
        .stdout(predicate::str::contains(
            "Base URL configuration for 'test-api':",
        ))
        .stdout(predicate::str::contains(
            "Base override: https://custom.example.com",
        ))
        .stdout(predicate::str::contains("prod: https://prod.example.com"))
        .stdout(predicate::str::contains("dev: https://dev.example.com"))
        .stdout(predicate::str::contains(
            "Resolved URL (current): https://custom.example.com",
        ));

    // Test get-url with environment set
    Command::cargo_bin("aperture")
        .unwrap()
        .env("APERTURE_CONFIG_DIR", config_dir.to_str().unwrap())
        .env("APERTURE_ENV", "prod")
        .args(&["config", "get-url", "test-api"])
        .assert()
        .success()
        .stdout(predicate::str::contains("(Using APERTURE_ENV=prod)"));

    // Test list-urls command
    Command::cargo_bin("aperture")
        .unwrap()
        .env("APERTURE_CONFIG_DIR", config_dir.to_str().unwrap())
        .args(&["config", "list-urls"])
        .assert()
        .success()
        .stdout(predicate::str::contains("Configured base URLs:"))
        .stdout(predicate::str::contains("test-api:"))
        .stdout(predicate::str::contains(
            "Base override: https://custom.example.com",
        ))
        .stdout(predicate::str::contains("prod: https://prod.example.com"))
        .stdout(predicate::str::contains("dev: https://dev.example.com"));
}

#[test]
fn test_config_url_error_handling() {
    let temp_dir = TempDir::new().unwrap();
    let config_dir = temp_dir.path().to_path_buf();

    // Test set-url on nonexistent spec
    Command::cargo_bin("aperture")
        .unwrap()
        .env("APERTURE_CONFIG_DIR", config_dir.to_str().unwrap())
        .args(&["config", "set-url", "nonexistent", "https://example.com"])
        .assert()
        .failure()
        .stderr(predicate::str::contains(
            "API specification 'nonexistent' not found",
        ));

    // Test get-url on nonexistent spec
    Command::cargo_bin("aperture")
        .unwrap()
        .env("APERTURE_CONFIG_DIR", config_dir.to_str().unwrap())
        .args(&["config", "get-url", "nonexistent"])
        .assert()
        .failure()
        .stderr(predicate::str::contains(
            "API specification 'nonexistent' not found",
        ));

    // Test list-urls with no configurations
    Command::cargo_bin("aperture")
        .unwrap()
        .env("APERTURE_CONFIG_DIR", config_dir.to_str().unwrap())
        .args(&["config", "list-urls"])
        .assert()
        .success()
        .stdout(predicate::str::contains("No base URLs configured."));
}

#[tokio::test]
async fn test_base_url_fallback_behavior() {
    let temp_dir = TempDir::new().unwrap();
    let config_dir = temp_dir.path().to_path_buf();
    let spec_file = temp_dir.path().join("test-spec.yaml");

    // Create spec WITHOUT servers (to test fallback)
    fs::write(&spec_file, create_test_spec_without_servers()).unwrap();

    Command::cargo_bin("aperture")
        .unwrap()
        .env("APERTURE_CONFIG_DIR", config_dir.to_str().unwrap())
        .args(&[
            "config",
            "add",
            "no-servers-api",
            spec_file.to_str().unwrap(),
        ])
        .assert()
        .success();

    // Test fallback URL when no configuration exists
    let output = Command::cargo_bin("aperture")
        .unwrap()
        .env("APERTURE_CONFIG_DIR", config_dir.to_str().unwrap())
        .args(&[
            "api",
            "no-servers-api",
            "--dry-run",
            "users",
            "get-user-by-id",
            "123",
        ])
        .output()
        .unwrap();

    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    let dry_run: serde_json::Value = serde_json::from_str(&stdout).unwrap();
    assert!(dry_run["url"]
        .as_str()
        .unwrap()
        .starts_with("https://api.example.com"));
}

#[tokio::test]
async fn test_describe_json_with_base_url_resolution() {
    let temp_dir = TempDir::new().unwrap();
    let config_dir = temp_dir.path().to_path_buf();
    let spec_file = temp_dir.path().join("test-spec.yaml");

    // Create spec with servers
    fs::write(&spec_file, create_test_spec_with_servers()).unwrap();

    Command::cargo_bin("aperture")
        .unwrap()
        .env("APERTURE_CONFIG_DIR", config_dir.to_str().unwrap())
        .args(&["config", "add", "test-api", spec_file.to_str().unwrap()])
        .assert()
        .success();

    // Test describe-json shows spec default URL
    let output = Command::cargo_bin("aperture")
        .unwrap()
        .env("APERTURE_CONFIG_DIR", config_dir.to_str().unwrap())
        .args(&["api", "test-api", "--describe-json"])
        .output()
        .unwrap();

    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    let manifest: serde_json::Value = serde_json::from_str(&stdout).unwrap();
    assert_eq!(
        manifest["api"]["base_url"].as_str().unwrap(),
        "https://api.example.com"
    );

    // Set custom URL and verify describe-json reflects it
    Command::cargo_bin("aperture")
        .unwrap()
        .env("APERTURE_CONFIG_DIR", config_dir.to_str().unwrap())
        .args(&[
            "config",
            "set-url",
            "test-api",
            "https://custom.example.com",
        ])
        .assert()
        .success();

    let output = Command::cargo_bin("aperture")
        .unwrap()
        .env("APERTURE_CONFIG_DIR", config_dir.to_str().unwrap())
        .args(&["api", "test-api", "--describe-json"])
        .output()
        .unwrap();

    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    let manifest: serde_json::Value = serde_json::from_str(&stdout).unwrap();
    assert_eq!(
        manifest["api"]["base_url"].as_str().unwrap(),
        "https://custom.example.com"
    );

    // Test environment-specific URL in describe-json
    Command::cargo_bin("aperture")
        .unwrap()
        .env("APERTURE_CONFIG_DIR", config_dir.to_str().unwrap())
        .args(&[
            "config",
            "set-url",
            "test-api",
            "--env",
            "staging",
            "https://staging.example.com",
        ])
        .assert()
        .success();

    let output = Command::cargo_bin("aperture")
        .unwrap()
        .env("APERTURE_CONFIG_DIR", config_dir.to_str().unwrap())
        .env("APERTURE_ENV", "staging")
        .args(&["api", "test-api", "--describe-json"])
        .output()
        .unwrap();

    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    let manifest: serde_json::Value = serde_json::from_str(&stdout).unwrap();
    assert_eq!(
        manifest["api"]["base_url"].as_str().unwrap(),
        "https://staging.example.com"
    );
}

#[tokio::test]
async fn test_backward_compatibility() {
    let temp_dir = TempDir::new().unwrap();
    let config_dir = temp_dir.path().to_path_buf();
    let spec_file = temp_dir.path().join("test-spec.yaml");

    // Create spec with servers
    fs::write(&spec_file, create_test_spec_with_servers()).unwrap();

    Command::cargo_bin("aperture")
        .unwrap()
        .env("APERTURE_CONFIG_DIR", config_dir.to_str().unwrap())
        .args(&["config", "add", "test-api", spec_file.to_str().unwrap()])
        .assert()
        .success();

    let mock_server = MockServer::start().await;

    Mock::given(method("GET"))
        .and(path("/users/123"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "id": "123",
            "name": "Test User"
        })))
        .expect(1)
        .mount(&mock_server)
        .await;

    // Test that existing APERTURE_BASE_URL environment variable still works
    // (this is how users configured base URLs before the new system)
    Command::cargo_bin("aperture")
        .unwrap()
        .env("APERTURE_CONFIG_DIR", config_dir.to_str().unwrap())
        .env("APERTURE_BASE_URL", &mock_server.uri())
        .args(&["api", "test-api", "users", "get-user-by-id", "123"])
        .assert()
        .success()
        .stdout(predicate::str::contains("Executing GET"))
        .stdout(predicate::str::contains("\"id\": \"123\""));
}

#[test]
fn test_multiple_apis_url_management() {
    let temp_dir = TempDir::new().unwrap();
    let config_dir = temp_dir.path().to_path_buf();
    let spec_file1 = temp_dir.path().join("api1-spec.yaml");
    let spec_file2 = temp_dir.path().join("api2-spec.yaml");

    // Create two different specs
    fs::write(&spec_file1, create_test_spec_with_servers()).unwrap();
    fs::write(&spec_file2, create_test_spec_without_servers()).unwrap();

    // Add both specs
    Command::cargo_bin("aperture")
        .unwrap()
        .env("APERTURE_CONFIG_DIR", config_dir.to_str().unwrap())
        .args(&["config", "add", "api1", spec_file1.to_str().unwrap()])
        .assert()
        .success();

    Command::cargo_bin("aperture")
        .unwrap()
        .env("APERTURE_CONFIG_DIR", config_dir.to_str().unwrap())
        .args(&["config", "add", "api2", spec_file2.to_str().unwrap()])
        .assert()
        .success();

    // Configure URLs for both APIs
    Command::cargo_bin("aperture")
        .unwrap()
        .env("APERTURE_CONFIG_DIR", config_dir.to_str().unwrap())
        .args(&[
            "config",
            "set-url",
            "api1",
            "https://api1-custom.example.com",
        ])
        .assert()
        .success();

    Command::cargo_bin("aperture")
        .unwrap()
        .env("APERTURE_CONFIG_DIR", config_dir.to_str().unwrap())
        .args(&[
            "config",
            "set-url",
            "api1",
            "--env",
            "prod",
            "https://api1-prod.example.com",
        ])
        .assert()
        .success();

    Command::cargo_bin("aperture")
        .unwrap()
        .env("APERTURE_CONFIG_DIR", config_dir.to_str().unwrap())
        .args(&[
            "config",
            "set-url",
            "api2",
            "https://api2-custom.example.com",
        ])
        .assert()
        .success();

    // Test list-urls shows both APIs
    Command::cargo_bin("aperture")
        .unwrap()
        .env("APERTURE_CONFIG_DIR", config_dir.to_str().unwrap())
        .args(&["config", "list-urls"])
        .assert()
        .success()
        .stdout(predicate::str::contains("api1:"))
        .stdout(predicate::str::contains("api2:"))
        .stdout(predicate::str::contains("https://api1-custom.example.com"))
        .stdout(predicate::str::contains("https://api1-prod.example.com"))
        .stdout(predicate::str::contains("https://api2-custom.example.com"));

    // Test that each API resolves its own URL correctly
    Command::cargo_bin("aperture")
        .unwrap()
        .env("APERTURE_CONFIG_DIR", config_dir.to_str().unwrap())
        .args(&["config", "get-url", "api1"])
        .assert()
        .success()
        .stdout(predicate::str::contains(
            "Resolved URL (current): https://api1-custom.example.com",
        ));

    Command::cargo_bin("aperture")
        .unwrap()
        .env("APERTURE_CONFIG_DIR", config_dir.to_str().unwrap())
        .args(&["config", "get-url", "api2"])
        .assert()
        .success()
        .stdout(predicate::str::contains(
            "Resolved URL (current): https://api2-custom.example.com",
        ));
}

#[test]
fn test_help_includes_new_commands() {
    // Test that help shows the new URL management commands
    Command::cargo_bin("aperture")
        .unwrap()
        .args(&["config", "--help"])
        .assert()
        .success()
        .stdout(predicate::str::contains("set-url"))
        .stdout(predicate::str::contains("get-url"))
        .stdout(predicate::str::contains("list-urls"));

    // Test individual command help
    Command::cargo_bin("aperture")
        .unwrap()
        .args(&["config", "set-url", "--help"])
        .assert()
        .success()
        .stdout(predicate::str::contains(
            "Set the base URL for an API specification",
        ))
        .stdout(predicate::str::contains("--env"));

    Command::cargo_bin("aperture")
        .unwrap()
        .args(&["config", "get-url", "--help"])
        .assert()
        .success()
        .stdout(predicate::str::contains(
            "Display the base URL configuration",
        ));

    Command::cargo_bin("aperture")
        .unwrap()
        .args(&["config", "list-urls", "--help"])
        .assert()
        .success()
        .stdout(predicate::str::contains("Display all configured base URLs"));
}
