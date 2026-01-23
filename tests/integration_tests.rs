#![cfg(feature = "integration")]
// These lints are overly pedantic for integration tests
#![allow(clippy::too_many_lines)]

mod common;
mod test_helpers;

use common::aperture_cmd;
use predicates::prelude::*;
use std::fs;
use tempfile::TempDir;
use wiremock::matchers::{header, method, path, path_regex};
use wiremock::{Mock, MockServer, ResponseTemplate};

#[tokio::test]
async fn test_full_pipeline_add_and_execute() {
    let temp_dir = TempDir::new().unwrap();
    let config_dir = temp_dir.path().to_path_buf();
    let spec_file = temp_dir.path().join("test-spec.yaml");

    // Create a minimal OpenAPI spec with security scheme
    let spec_content = "openapi: 3.0.0
info:
  title: Test API
  version: 1.0.0
servers:
  - url: https://api.example.com
components:
  securitySchemes:
    apiKey:
      type: apiKey
      in: header
      name: X-API-Key
      x-aperture-secret:
        source: env
        name: TEST_API_KEY
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
";
    fs::write(&spec_file, spec_content).unwrap();

    // Test adding the spec
    let add_output = aperture_cmd()
        .env("APERTURE_CONFIG_DIR", config_dir.to_str().unwrap())
        .args(["config", "add", "test-api", spec_file.to_str().unwrap()])
        .output()
        .unwrap();

    if !add_output.status.success() {
        eprintln!("Add command failed!");
        eprintln!("stdout: {}", String::from_utf8_lossy(&add_output.stdout));
        eprintln!("stderr: {}", String::from_utf8_lossy(&add_output.stderr));
        panic!("Failed to add spec");
    }

    assert!(
        String::from_utf8_lossy(&add_output.stdout).contains("Spec 'test-api' added successfully")
    );

    // Verify the spec was cached
    let cache_file = config_dir.join(".cache/test-api.bin");
    assert!(cache_file.exists());

    // Start mock server for API execution
    let mock_server = MockServer::start().await;

    Mock::given(method("GET"))
        .and(path("/users/123"))
        .and(header("User-Agent", "aperture/0.1.0"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "id": "123",
            "name": "John Doe",
            "email": "john@example.com"
        })))
        .expect(1)
        .mount(&mock_server)
        .await;

    // Test executing a command against the API
    aperture_cmd()
        .env("APERTURE_CONFIG_DIR", config_dir.to_str().unwrap())
        .env("APERTURE_BASE_URL", mock_server.uri())
        .args(["api", "test-api", "users", "get-user-by-id", "--id", "123"])
        .assert()
        .success()
        .stdout(predicate::str::contains("\"id\": \"123\""))
        .stdout(predicate::str::contains("\"name\": \"John Doe\""));
}

#[tokio::test]
async fn test_config_list_and_remove() {
    let temp_dir = TempDir::new().unwrap();
    let config_dir = temp_dir.path().to_path_buf();
    let spec_file = temp_dir.path().join("spec.yaml");

    // Create a minimal spec with security
    fs::write(
        &spec_file,
        "openapi: 3.0.0
info:
  title: Test
  version: 1.0.0
components:
  securitySchemes:
    apiKey:
      type: apiKey
      in: header
      name: X-API-Key
      x-aperture-secret:
        source: env
        name: TEST_KEY
paths: {}
",
    )
    .unwrap();

    // Add multiple specs
    aperture_cmd()
        .env("APERTURE_CONFIG_DIR", config_dir.to_str().unwrap())
        .args(["config", "add", "api-one", spec_file.to_str().unwrap()])
        .assert()
        .success();

    aperture_cmd()
        .env("APERTURE_CONFIG_DIR", config_dir.to_str().unwrap())
        .args(["config", "add", "api-two", spec_file.to_str().unwrap()])
        .assert()
        .success();

    // Test listing specs
    aperture_cmd()
        .env("APERTURE_CONFIG_DIR", config_dir.to_str().unwrap())
        .args(["config", "list"])
        .assert()
        .success()
        .stdout(predicate::str::contains("api-one"))
        .stdout(predicate::str::contains("api-two"));

    // Test removing a spec
    aperture_cmd()
        .env("APERTURE_CONFIG_DIR", config_dir.to_str().unwrap())
        .args(["config", "remove", "api-one"])
        .assert()
        .success()
        .stdout(predicate::str::contains(
            "Spec 'api-one' removed successfully",
        ));

    // Verify only api-two remains
    aperture_cmd()
        .env("APERTURE_CONFIG_DIR", config_dir.to_str().unwrap())
        .args(["config", "list"])
        .assert()
        .success()
        .stdout(predicate::str::contains("api-two"))
        .stdout(predicate::str::contains("api-one").not());
}

#[tokio::test]
async fn test_api_error_handling() {
    let temp_dir = TempDir::new().unwrap();
    let config_dir = temp_dir.path().to_path_buf();

    // Test executing without any specs
    aperture_cmd()
        .env("APERTURE_CONFIG_DIR", config_dir.to_str().unwrap())
        .args(["api", "nonexistent", "users", "list"])
        .assert()
        .failure()
        .stderr(predicate::str::contains(
            "No cached spec found for 'nonexistent'",
        ));

    // Create and add a spec
    let spec_file = temp_dir.path().join("spec.yaml");
    fs::write(
        &spec_file,
        "openapi: 3.0.0
info:
  title: Test API
  version: 1.0.0
components:
  securitySchemes:
    apiKey:
      type: apiKey
      in: header
      name: X-API-Key
      x-aperture-secret:
        source: env
        name: TEST_KEY
paths:
  /users:
    post:
      tags:
        - users
      operationId: createUser
      requestBody:
        required: true
        content:
          application/json:
            schema:
              type: object
      responses:
        '201':
          description: Created
",
    )
    .unwrap();

    aperture_cmd()
        .env("APERTURE_CONFIG_DIR", config_dir.to_str().unwrap())
        .args(["config", "add", "test-api", spec_file.to_str().unwrap()])
        .assert()
        .success();

    // Test with invalid JSON body
    let mock_server = MockServer::start().await;

    aperture_cmd()
        .env("APERTURE_CONFIG_DIR", config_dir.to_str().unwrap())
        .env("APERTURE_BASE_URL", mock_server.uri())
        .args([
            "api",
            "test-api",
            "users",
            "create-user",
            "--body",
            "invalid-json",
        ])
        .assert()
        .failure()
        .stderr(predicate::str::contains("Invalid JSON body"));
}

#[tokio::test]
async fn test_http_error_responses() {
    let temp_dir = TempDir::new().unwrap();
    let config_dir = temp_dir.path().to_path_buf();
    let spec_file = temp_dir.path().join("spec.yaml");

    fs::write(
        &spec_file,
        "openapi: 3.0.0
info:
  title: Test API
  version: 1.0.0
components:
  securitySchemes:
    apiKey:
      type: apiKey
      in: header
      name: X-API-Key
      x-aperture-secret:
        source: env
        name: TEST_KEY
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
        '404':
          description: Not found
",
    )
    .unwrap();

    aperture_cmd()
        .env("APERTURE_CONFIG_DIR", config_dir.to_str().unwrap())
        .args(["config", "add", "test-api", spec_file.to_str().unwrap()])
        .assert()
        .success();

    let mock_server = MockServer::start().await;

    // Mock 404 response
    Mock::given(method("GET"))
        .and(path("/users/999"))
        .respond_with(ResponseTemplate::new(404).set_body_json(serde_json::json!({
            "error": "User not found"
        })))
        .mount(&mock_server)
        .await;

    // Test that 404 is handled
    aperture_cmd()
        .env("APERTURE_CONFIG_DIR", config_dir.to_str().unwrap())
        .env("APERTURE_BASE_URL", mock_server.uri())
        .args(["api", "test-api", "users", "get-user-by-id", "--id", "999"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("404"))
        .stderr(predicate::str::contains("User not found"));
}

#[test]
fn test_help_output() {
    // Test root help
    aperture_cmd()
        .arg("--help")
        .assert()
        .success()
        .stdout(predicate::str::contains("Commands:"))
        .stdout(predicate::str::contains("config"))
        .stdout(predicate::str::contains("api"));

    // Test config help
    aperture_cmd()
        .args(["config", "--help"])
        .assert()
        .success()
        .stdout(predicate::str::contains(
            "Manage your collection of OpenAPI specifications",
        ))
        .stdout(predicate::str::contains("add"))
        .stdout(predicate::str::contains("list"))
        .stdout(predicate::str::contains("remove"))
        .stdout(predicate::str::contains("edit"));
}

#[tokio::test]
async fn test_query_and_header_parameters() {
    let temp_dir = TempDir::new().unwrap();
    let config_dir = temp_dir.path().to_path_buf();
    let spec_file = temp_dir.path().join("spec.yaml");

    fs::write(
        &spec_file,
        "openapi: 3.0.0
info:
  title: Test API
  version: 1.0.0
components:
  securitySchemes:
    apiKey:
      type: apiKey
      in: header
      name: X-API-Key
      x-aperture-secret:
        source: env
        name: TEST_KEY
paths:
  /search:
    get:
      tags:
        - search
      operationId: searchItems
      parameters:
        - name: q
          in: query
          required: true
          schema:
            type: string
        - name: limit
          in: query
          schema:
            type: integer
        - name: X-API-Key
          in: header
          required: true
          schema:
            type: string
      responses:
        '200':
          description: Success
",
    )
    .unwrap();

    aperture_cmd()
        .env("APERTURE_CONFIG_DIR", config_dir.to_str().unwrap())
        .args(["config", "add", "test-api", spec_file.to_str().unwrap()])
        .assert()
        .success();

    let mock_server = MockServer::start().await;

    Mock::given(method("GET"))
        .and(path("/search"))
        .and(header("X-API-Key", "secret123"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "results": ["item1", "item2"]
        })))
        .mount(&mock_server)
        .await;

    aperture_cmd()
        .env("APERTURE_CONFIG_DIR", config_dir.to_str().unwrap())
        .env("APERTURE_BASE_URL", mock_server.uri())
        .args([
            "api",
            "test-api",
            "search",
            "search-items",
            "--q",
            "test",
            "--limit",
            "10",
            "--x-api-key",
            "secret123",
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains("\"results\""));
}

#[tokio::test]
async fn test_describe_json_flag() {
    let temp_dir = TempDir::new().unwrap();
    let config_dir = temp_dir.path().to_path_buf();
    let spec_file = temp_dir.path().join("spec.yaml");

    // Create a minimal spec
    fs::write(
        &spec_file,
        "openapi: 3.0.0
info:
  title: Test API
  version: 1.0.0
components:
  securitySchemes:
    apiKey:
      type: apiKey
      in: header
      name: X-API-Key
      x-aperture-secret:
        source: env
        name: TEST_KEY
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
",
    )
    .unwrap();

    // Add the spec
    aperture_cmd()
        .env("APERTURE_CONFIG_DIR", config_dir.to_str().unwrap())
        .args(["config", "add", "test-api", spec_file.to_str().unwrap()])
        .assert()
        .success();

    // Test --describe-json flag
    let output = aperture_cmd()
        .env("APERTURE_CONFIG_DIR", config_dir.to_str().unwrap())
        .args(["api", "test-api", "--describe-json"])
        .output()
        .unwrap();

    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);

    // Parse the JSON output to verify structure
    let manifest: serde_json::Value = serde_json::from_str(&stdout).unwrap();
    assert_eq!(manifest["api"]["name"].as_str().unwrap(), "Test API");
    assert_eq!(manifest["api"]["version"].as_str().unwrap(), "1.0.0");
    assert!(manifest["commands"]["users"].is_array());

    let users_commands = manifest["commands"]["users"].as_array().unwrap();
    assert_eq!(users_commands.len(), 1);
    assert_eq!(
        users_commands[0]["name"].as_str().unwrap(),
        "get-user-by-id"
    );
    assert_eq!(users_commands[0]["method"].as_str().unwrap(), "GET");
    assert_eq!(
        users_commands[0]["operation_id"].as_str().unwrap(),
        "getUserById"
    );
}

#[tokio::test]
async fn test_describe_json_with_jq_filter() {
    let temp_dir = TempDir::new().unwrap();
    let config_dir = temp_dir.path().to_path_buf();
    let spec_file = temp_dir.path().join("spec.yaml");

    // Create a minimal spec
    fs::write(
        &spec_file,
        "openapi: 3.0.0
info:
  title: Test API
  version: 1.0.0
components:
  securitySchemes:
    apiKey:
      type: apiKey
      in: header
      name: X-API-Key
      x-aperture-secret:
        source: env
        name: TEST_KEY
    bearerAuth:
      type: http
      scheme: bearer
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
  /posts:
    get:
      tags:
        - posts
      operationId: listPosts
      responses:
        '200':
          description: Success
",
    )
    .unwrap();

    // Add the spec
    aperture_cmd()
        .env("APERTURE_CONFIG_DIR", config_dir.to_str().unwrap())
        .args(["config", "add", "test-api", spec_file.to_str().unwrap()])
        .assert()
        .success();

    // Test --describe-json with --jq to get only the users commands
    let output = aperture_cmd()
        .env("APERTURE_CONFIG_DIR", config_dir.to_str().unwrap())
        .args([
            "api",
            "test-api",
            "--describe-json",
            "--jq",
            ".commands.users",
        ])
        .output()
        .unwrap();

    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);

    // Parse the JSON output to verify it's only the users commands
    let users_commands: serde_json::Value = serde_json::from_str(&stdout).unwrap();
    assert!(users_commands.is_array());
    assert_eq!(users_commands.as_array().unwrap().len(), 1);
    assert_eq!(
        users_commands[0]["name"].as_str().unwrap(),
        "get-user-by-id"
    );

    // Test --jq to get security schemes
    let output = aperture_cmd()
        .env("APERTURE_CONFIG_DIR", config_dir.to_str().unwrap())
        .args([
            "api",
            "test-api",
            "--describe-json",
            "--jq",
            ".security_schemes",
        ])
        .output()
        .unwrap();

    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    let security_schemes: serde_json::Value = serde_json::from_str(&stdout).unwrap();
    assert!(security_schemes["apiKey"].is_object());
    assert!(security_schemes["bearerAuth"].is_object());

    // Test --jq to get API version
    let output = aperture_cmd()
        .env("APERTURE_CONFIG_DIR", config_dir.to_str().unwrap())
        .args(["api", "test-api", "--describe-json", "--jq", ".api.version"])
        .output()
        .unwrap();

    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    // The output should be a JSON string
    let version: serde_json::Value = serde_json::from_str(&stdout).unwrap();
    assert_eq!(version, serde_json::Value::String("1.0.0".to_string()));

    // Test invalid JQ filter error handling
    let output = aperture_cmd()
        .env("APERTURE_CONFIG_DIR", config_dir.to_str().unwrap())
        .args([
            "api",
            "test-api",
            "--describe-json",
            "--jq",
            "invalid[filter",
        ])
        .output()
        .unwrap();

    assert!(!output.status.success());
}

#[tokio::test]
async fn test_json_errors_flag() {
    let temp_dir = TempDir::new().unwrap();
    let config_dir = temp_dir.path().to_path_buf();

    // Test with --json-errors for nonexistent spec
    let output = aperture_cmd()
        .env("APERTURE_CONFIG_DIR", config_dir.to_str().unwrap())
        .args(["--json-errors", "api", "nonexistent", "users", "list"])
        .output()
        .unwrap();

    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);

    // Extract JSON from stderr (might have debug output before it)
    let json_start = stderr.find('{').expect("No JSON found in stderr");
    let json_str = &stderr[json_start..];

    // Parse the JSON error output
    let error: serde_json::Value = serde_json::from_str(json_str).unwrap();
    let error_type = error["error_type"].as_str().unwrap();
    // Accept both old and new error types during migration
    assert!(
        error_type == "CachedSpecNotFound" || error_type == "Specification",
        "Expected CachedSpecNotFound or Specification, got: {error_type}"
    );
    assert!(error["message"]
        .as_str()
        .unwrap()
        .contains("No cached spec found"));
}

#[tokio::test]
async fn test_dry_run_flag() {
    let temp_dir = TempDir::new().unwrap();
    let config_dir = temp_dir.path().to_path_buf();
    let spec_file = temp_dir.path().join("spec.yaml");

    // Create a minimal spec
    fs::write(
        &spec_file,
        "openapi: 3.0.0
info:
  title: Test API
  version: 1.0.0
components:
  securitySchemes:
    apiKey:
      type: apiKey
      in: header
      name: X-API-Key
      x-aperture-secret:
        source: env
        name: TEST_KEY
paths:
  /users:
    post:
      tags:
        - users
      operationId: createUser
      requestBody:
        required: true
        content:
          application/json:
            schema:
              type: object
      responses:
        '201':
          description: Created
",
    )
    .unwrap();

    // Add the spec
    aperture_cmd()
        .env("APERTURE_CONFIG_DIR", config_dir.to_str().unwrap())
        .args(["config", "add", "test-api", spec_file.to_str().unwrap()])
        .assert()
        .success();

    // Test --dry-run flag
    let output = aperture_cmd()
        .env("APERTURE_CONFIG_DIR", config_dir.to_str().unwrap())
        .env("TEST_KEY", "secret123")
        .args([
            "api",
            "test-api",
            "--dry-run",
            "users",
            "create-user",
            "--body",
            "{\"name\":\"John\"}",
        ])
        .output()
        .unwrap();

    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);

    // Parse the dry-run JSON output
    let dry_run_info: serde_json::Value = serde_json::from_str(&stdout).unwrap();
    assert!(dry_run_info["dry_run"].as_bool().unwrap());
    assert_eq!(dry_run_info["method"].as_str().unwrap(), "POST");
    assert_eq!(dry_run_info["operation_id"].as_str().unwrap(), "createUser");
    assert!(dry_run_info["url"].as_str().unwrap().contains("/users"));
}

#[tokio::test]
async fn test_idempotency_key_flag() {
    let temp_dir = TempDir::new().unwrap();
    let config_dir = temp_dir.path().to_path_buf();
    let spec_file = temp_dir.path().join("spec.yaml");

    // Create a minimal spec
    fs::write(
        &spec_file,
        "openapi: 3.0.0
info:
  title: Test API
  version: 1.0.0
components:
  securitySchemes:
    apiKey:
      type: apiKey
      in: header
      name: X-API-Key
      x-aperture-secret:
        source: env
        name: TEST_KEY
paths:
  /users:
    post:
      tags:
        - users
      operationId: createUser
      responses:
        '201':
          description: Created
",
    )
    .unwrap();

    // Add the spec
    aperture_cmd()
        .env("APERTURE_CONFIG_DIR", config_dir.to_str().unwrap())
        .args(["config", "add", "test-api", spec_file.to_str().unwrap()])
        .assert()
        .success();

    // Test --idempotency-key with --dry-run to see headers
    let output = aperture_cmd()
        .env("APERTURE_CONFIG_DIR", config_dir.to_str().unwrap())
        .env("TEST_KEY", "secret123")
        .args([
            "api",
            "test-api",
            "--dry-run",
            "--idempotency-key",
            "my-unique-key-123",
            "users",
            "create-user",
        ])
        .output()
        .unwrap();

    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);

    // Parse the dry-run JSON output to check headers
    let dry_run_info: serde_json::Value = serde_json::from_str(&stdout).unwrap();
    let headers = &dry_run_info["headers"];
    assert!(headers["idempotency-key"].as_str().unwrap() == "my-unique-key-123");
}

#[test]
fn test_list_commands_feature() {
    let temp_dir = TempDir::new().unwrap();
    let config_dir = temp_dir.path().to_path_buf();
    let spec_file = temp_dir.path().join("spec.yaml");

    // Create a spec with multiple tags and operations
    fs::write(
        &spec_file,
        "openapi: 3.0.0
info:
  title: Test API
  version: 1.0.0
components:
  securitySchemes:
    apiKey:
      type: apiKey
      in: header
      name: X-API-Key
      x-aperture-secret:
        source: env
        name: TEST_KEY
paths:
  /users:
    get:
      tags:
        - users
      operationId: listUsers
      summary: List all users
      responses:
        '200':
          description: Success
    post:
      tags:
        - users
      operationId: createUser
      summary: Create a new user
      responses:
        '201':
          description: Created
  /posts/{id}:
    get:
      tags:
        - posts
      operationId: getPostById
      summary: Get post by ID
      parameters:
        - name: id
          in: path
          required: true
          schema:
            type: string
      responses:
        '200':
          description: Success
  /health:
    get:
      operationId: healthCheck
      summary: Health check endpoint
      responses:
        '200':
          description: Success
",
    )
    .unwrap();

    // Add the spec
    aperture_cmd()
        .env("APERTURE_CONFIG_DIR", config_dir.to_str().unwrap())
        .args(["config", "add", "test-api", spec_file.to_str().unwrap()])
        .assert()
        .success();

    // Test list-commands
    let output = aperture_cmd()
        .env("APERTURE_CONFIG_DIR", config_dir.to_str().unwrap())
        .args(["list-commands", "test-api"])
        .output()
        .unwrap();

    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);

    // Check that commands are grouped by tags
    assert!(stdout.contains("users"));
    assert!(stdout.contains("posts"));
    assert!(stdout.contains("General")); // For healthCheck without tag

    // Check that operations are listed
    assert!(stdout.contains("list-users"));
    assert!(stdout.contains("create-user"));
    assert!(stdout.contains("get-post-by-id"));
    assert!(stdout.contains("health-check"));

    // Check that HTTP methods are shown
    assert!(stdout.contains("GET"));
    assert!(stdout.contains("POST"));
}

#[test]
fn test_config_reinit_feature() {
    let temp_dir = TempDir::new().unwrap();
    let config_dir = temp_dir.path().to_path_buf();
    let spec_file = temp_dir.path().join("spec.yaml");

    // Create a minimal spec
    fs::write(
        &spec_file,
        "openapi: 3.0.0
info:
  title: Test API
  version: 1.0.0
components:
  securitySchemes:
    apiKey:
      type: apiKey
      in: header
      name: X-API-Key
      x-aperture-secret:
        source: env
        name: TEST_KEY
paths:
  /users:
    get:
      tags:
        - users
      operationId: listUsers
      responses:
        '200':
          description: Success
",
    )
    .unwrap();

    // Add the spec
    aperture_cmd()
        .env("APERTURE_CONFIG_DIR", config_dir.to_str().unwrap())
        .args(["config", "add", "test-api", spec_file.to_str().unwrap()])
        .assert()
        .success();

    // Test reinit for specific spec
    aperture_cmd()
        .env("APERTURE_CONFIG_DIR", config_dir.to_str().unwrap())
        .args(["config", "reinit", "test-api"])
        .assert()
        .success()
        .stdout(predicate::str::contains(
            "Successfully reinitialized cache for 'test-api'",
        ));

    // Add another spec
    let spec_file2 = temp_dir.path().join("spec2.yaml");
    fs::write(
        &spec_file2,
        "openapi: 3.0.0
info:
  title: Test API 2
  version: 1.0.0
paths:
  /health:
    get:
      operationId: healthCheck
      responses:
        '200':
          description: Success
",
    )
    .unwrap();

    aperture_cmd()
        .env("APERTURE_CONFIG_DIR", config_dir.to_str().unwrap())
        .args(["config", "add", "test-api-2", spec_file2.to_str().unwrap()])
        .assert()
        .success();

    // Test reinit --all
    aperture_cmd()
        .env("APERTURE_CONFIG_DIR", config_dir.to_str().unwrap())
        .args(["config", "reinit", "--all"])
        .assert()
        .success()
        .stdout(predicate::str::contains("Reinitialization complete"));
}

#[tokio::test]
async fn test_context_aware_error_messages() {
    let temp_dir = TempDir::new().unwrap();
    let config_dir = temp_dir.path().to_path_buf();
    let spec_file = temp_dir.path().join("spec.yaml");

    // Create a spec with authentication
    fs::write(
        &spec_file,
        "openapi: 3.0.0
info:
  title: Test API
  version: 1.0.0
components:
  securitySchemes:
    apiKey:
      type: apiKey
      in: header
      name: X-API-Key
      x-aperture-secret:
        source: env
        name: TEST_API_KEY
    bearerAuth:
      type: http
      scheme: bearer
      x-aperture-secret:
        source: env
        name: BEARER_TOKEN
paths:
  /users:
    get:
      tags:
        - users
      operationId: listUsers
      security:
        - apiKey: []
        - bearerAuth: []
      responses:
        '200':
          description: Success
",
    )
    .unwrap();

    // Add the spec
    aperture_cmd()
        .env("APERTURE_CONFIG_DIR", config_dir.to_str().unwrap())
        .args(["config", "add", "test-api", spec_file.to_str().unwrap()])
        .assert()
        .success();

    let mock_server = MockServer::start().await;

    // Mock 401 response
    Mock::given(method("GET"))
        .and(path("/users"))
        .respond_with(ResponseTemplate::new(401).set_body_json(serde_json::json!({
            "error": "Unauthorized"
        })))
        .mount(&mock_server)
        .await;

    // Test that 401 error shows environment variable names (set auth to get past SecretNotSet)
    let output = aperture_cmd()
        .env("APERTURE_CONFIG_DIR", config_dir.to_str().unwrap())
        .env("APERTURE_BASE_URL", mock_server.uri())
        .env("TEST_API_KEY", "test-key")
        .env("BEARER_TOKEN", "test-token")
        .args(["--json-errors", "api", "test-api", "users", "list-users"])
        .output()
        .unwrap();

    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);

    // Extract JSON from stderr (might have debug output before it)
    let json_start = stderr.find('{').expect("No JSON found in stderr");
    let json_str = &stderr[json_start..];

    // Parse the JSON error output
    let error: serde_json::Value = serde_json::from_str(json_str).unwrap_or_else(|e| {
        panic!("Failed to parse JSON from stderr: {e}\nJSON str was: {json_str}")
    });
    assert_eq!(error["error_type"].as_str().unwrap(), "HttpError");
    assert_eq!(error["details"]["status"], 401);
    assert_eq!(error["details"]["api_name"], "test-api");
    assert_eq!(error["details"]["operation_id"], "listUsers");

    // Check that environment variable names are included in security_schemes
    let security_schemes = error["details"]["security_schemes"].as_array().unwrap();
    assert!(security_schemes.contains(&serde_json::Value::String("TEST_API_KEY".to_string())));
    assert!(security_schemes.contains(&serde_json::Value::String("BEARER_TOKEN".to_string())));
}

// Phase 2.2: Advanced Output Formatting Tests (ignored until implementation)

#[tokio::test]
async fn test_output_format_json() {
    let temp_dir = TempDir::new().unwrap();
    let config_dir = temp_dir.path().to_path_buf();
    let spec_file = temp_dir.path().join("spec.yaml");

    // Create a spec with a response that has structured data
    fs::write(
        &spec_file,
        "openapi: 3.0.0
info:
  title: Test API
  version: 1.0.0
paths:
  /users:
    get:
      tags:
        - users
      operationId: listUsers
      responses:
        '200':
          description: Success
          content:
            application/json:
              schema:
                type: array
                items:
                  type: object
                  properties:
                    id:
                      type: integer
                    name:
                      type: string
                    email:
                      type: string
",
    )
    .unwrap();

    // Add the spec
    aperture_cmd()
        .env("APERTURE_CONFIG_DIR", config_dir.to_str().unwrap())
        .args(["config", "add", "test-api", spec_file.to_str().unwrap()])
        .assert()
        .success();

    let mock_server = MockServer::start().await;

    // Mock successful response with JSON data
    Mock::given(method("GET"))
        .and(path("/users"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!([
            {"id": 1, "name": "Alice", "email": "alice@example.com"},
            {"id": 2, "name": "Bob", "email": "bob@example.com"}
        ])))
        .mount(&mock_server)
        .await;

    // Test --format json (default behavior should be preserved)
    let output = aperture_cmd()
        .env("APERTURE_CONFIG_DIR", config_dir.to_str().unwrap())
        .env("APERTURE_BASE_URL", mock_server.uri())
        .args(["api", "test-api", "--format", "json", "users", "list-users"])
        .output()
        .unwrap();

    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);

    // Should be valid JSON
    let parsed: serde_json::Value = serde_json::from_str(&stdout).unwrap();
    assert!(parsed.is_array());
    assert_eq!(parsed.as_array().unwrap().len(), 2);
    assert_eq!(parsed[0]["name"].as_str().unwrap(), "Alice");
}

#[tokio::test]
async fn test_output_format_yaml() {
    let temp_dir = TempDir::new().unwrap();
    let config_dir = temp_dir.path().to_path_buf();
    let spec_file = temp_dir.path().join("spec.yaml");

    // Create a spec with a response that has structured data
    fs::write(
        &spec_file,
        "openapi: 3.0.0
info:
  title: Test API
  version: 1.0.0
paths:
  /users:
    get:
      tags:
        - users
      operationId: listUsers
      responses:
        '200':
          description: Success
          content:
            application/json:
              schema:
                type: object
                properties:
                  users:
                    type: array
                    items:
                      type: object
                      properties:
                        id:
                          type: integer
                        name:
                          type: string
",
    )
    .unwrap();

    // Add the spec
    aperture_cmd()
        .env("APERTURE_CONFIG_DIR", config_dir.to_str().unwrap())
        .args(["config", "add", "test-api", spec_file.to_str().unwrap()])
        .assert()
        .success();

    let mock_server = MockServer::start().await;

    // Mock successful response with JSON data
    Mock::given(method("GET"))
        .and(path("/users"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "users": [
                {"id": 1, "name": "Alice"},
                {"id": 2, "name": "Bob"}
            ]
        })))
        .mount(&mock_server)
        .await;

    // Test --format yaml
    let output = aperture_cmd()
        .env("APERTURE_CONFIG_DIR", config_dir.to_str().unwrap())
        .env("APERTURE_BASE_URL", mock_server.uri())
        .args(["api", "test-api", "--format", "yaml", "users", "list-users"])
        .output()
        .unwrap();

    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);

    // Should be valid YAML
    let parsed: serde_yaml::Value = serde_yaml::from_str(&stdout).unwrap();
    assert!(parsed["users"].is_sequence());
    let users = parsed["users"].as_sequence().unwrap();
    assert_eq!(users.len(), 2);
    assert_eq!(users[0]["name"].as_str().unwrap(), "Alice");

    // Should contain YAML syntax markers
    assert!(stdout.contains("users:"));
    assert!(stdout.contains("- id: 1"));
    assert!(stdout.contains("  name: Alice"));
}

#[tokio::test]
async fn test_output_format_table() {
    let temp_dir = TempDir::new().unwrap();
    let config_dir = temp_dir.path().to_path_buf();
    let spec_file = temp_dir.path().join("spec.yaml");

    // Create a spec with a response that has structured data
    fs::write(
        &spec_file,
        "openapi: 3.0.0
info:
  title: Test API
  version: 1.0.0
paths:
  /users:
    get:
      tags:
        - users
      operationId: listUsers
      responses:
        '200':
          description: Success
          content:
            application/json:
              schema:
                type: array
                items:
                  type: object
                  properties:
                    id:
                      type: integer
                    name:
                      type: string
                    email:
                      type: string
                    active:
                      type: boolean
",
    )
    .unwrap();

    // Add the spec
    aperture_cmd()
        .env("APERTURE_CONFIG_DIR", config_dir.to_str().unwrap())
        .args(["config", "add", "test-api", spec_file.to_str().unwrap()])
        .assert()
        .success();

    let mock_server = MockServer::start().await;

    // Mock successful response with JSON data suitable for table format
    Mock::given(method("GET"))
        .and(path("/users"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!([
            {"id": 1, "name": "Alice", "email": "alice@example.com", "active": true},
            {"id": 2, "name": "Bob", "email": "bob@example.com", "active": false},
            {"id": 3, "name": "Charlie", "email": "charlie@example.com", "active": true}
        ])))
        .mount(&mock_server)
        .await;

    // Test --format table
    let output = aperture_cmd()
        .env("APERTURE_CONFIG_DIR", config_dir.to_str().unwrap())
        .env("APERTURE_BASE_URL", mock_server.uri())
        .args([
            "api",
            "test-api",
            "--format",
            "table",
            "users",
            "list-users",
        ])
        .output()
        .unwrap();

    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);

    // Should contain table structure
    assert!(stdout.contains('|')); // Table borders
    assert!(stdout.contains("id")); // Header columns
    assert!(stdout.contains("name"));
    assert!(stdout.contains("email"));
    assert!(stdout.contains("active"));

    // Should contain data rows
    assert!(stdout.contains("Alice"));
    assert!(stdout.contains("Bob"));
    assert!(stdout.contains("Charlie"));
    assert!(stdout.contains("alice@example.com"));

    // Should have table formatting with proper alignment
    assert!(stdout.lines().count() >= 5); // Header, separator, at least 3 data rows
}

#[tokio::test]
async fn test_output_format_table_with_nested_objects() {
    let temp_dir = TempDir::new().unwrap();
    let config_dir = temp_dir.path().to_path_buf();
    let spec_file = temp_dir.path().join("spec.yaml");

    // Create a spec with nested object response
    fs::write(
        &spec_file,
        "openapi: 3.0.0
info:
  title: Test API
  version: 1.0.0
paths:
  /users:
    get:
      tags:
        - users
      operationId: listUsers
      responses:
        '200':
          description: Success
          content:
            application/json:
              schema:
                type: array
                items:
                  type: object
                  properties:
                    id:
                      type: integer
                    name:
                      type: string
                    profile:
                      type: object
                      properties:
                        age:
                          type: integer
                        city:
                          type: string
",
    )
    .unwrap();

    // Add the spec
    aperture_cmd()
        .env("APERTURE_CONFIG_DIR", config_dir.to_str().unwrap())
        .args(["config", "add", "test-api", spec_file.to_str().unwrap()])
        .assert()
        .success();

    let mock_server = MockServer::start().await;

    // Mock response with nested objects
    Mock::given(method("GET"))
        .and(path("/users"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!([
            {"id": 1, "name": "Alice", "profile": {"age": 30, "city": "New York"}},
            {"id": 2, "name": "Bob", "profile": {"age": 25, "city": "San Francisco"}}
        ])))
        .mount(&mock_server)
        .await;

    // Test --format table with nested objects (should flatten or stringify)
    let output = aperture_cmd()
        .env("APERTURE_CONFIG_DIR", config_dir.to_str().unwrap())
        .env("APERTURE_BASE_URL", mock_server.uri())
        .args([
            "api",
            "test-api",
            "--format",
            "table",
            "users",
            "list-users",
        ])
        .output()
        .unwrap();

    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);

    // Should contain table structure
    assert!(stdout.contains('|'));
    assert!(stdout.contains("id"));
    assert!(stdout.contains("name"));
    assert!(stdout.contains("profile"));

    // Should contain the flattened or stringified profile data
    assert!(stdout.contains("Alice"));
    assert!(stdout.contains("Bob"));
    // Profile should be either flattened (profile.age, profile.city) or stringified
    assert!(stdout.contains("30") || stdout.contains("age"));
}

#[tokio::test]
async fn test_output_format_invalid() {
    let temp_dir = TempDir::new().unwrap();
    let config_dir = temp_dir.path().to_path_buf();
    let spec_file = temp_dir.path().join("spec.yaml");

    // Create a minimal spec
    fs::write(
        &spec_file,
        "openapi: 3.0.0
info:
  title: Test API
  version: 1.0.0
paths:
  /users:
    get:
      tags:
        - users
      operationId: listUsers
      responses:
        '200':
          description: Success
",
    )
    .unwrap();

    // Add the spec
    aperture_cmd()
        .env("APERTURE_CONFIG_DIR", config_dir.to_str().unwrap())
        .args(["config", "add", "test-api", spec_file.to_str().unwrap()])
        .assert()
        .success();

    // Test invalid format
    let output = aperture_cmd()
        .env("APERTURE_CONFIG_DIR", config_dir.to_str().unwrap())
        .args([
            "api",
            "test-api",
            "--format",
            "invalid",
            "users",
            "list-users",
        ])
        .output()
        .unwrap();

    // Should fail with helpful error message
    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("invalid") || stderr.contains("format"));
}

#[test]
fn test_output_format_help_text() {
    // Test that --help shows the format option
    let output = aperture_cmd()
        .args(["api", "test", "--help"])
        .output()
        .unwrap();

    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("--format") || stdout.contains("format"));
    assert!(stdout.contains("json") || stdout.contains("yaml") || stdout.contains("table"));
}

#[tokio::test]
async fn test_jq_filter_basic() {
    let temp_dir = TempDir::new().unwrap();
    let config_dir = temp_dir.path().to_path_buf();
    let spec_file = temp_dir.path().join("test-spec.yaml");

    // Create spec and add it
    let spec_content = "openapi: 3.0.0
info:
  title: Test API
  version: 1.0.0
servers:
  - url: https://api.example.com
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
          content:
            application/json:
              schema:
                type: object
";
    fs::write(&spec_file, spec_content).unwrap();
    aperture_cmd()
        .env("APERTURE_CONFIG_DIR", config_dir.to_str().unwrap())
        .args(["config", "add", "test-api", spec_file.to_str().unwrap()])
        .assert()
        .success();

    let mock_server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/users/123"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "id": "123",
            "name": "John Doe",
            "email": "john@example.com",
            "metadata": {
                "created": "2024-01-01",
                "role": "admin"
            }
        })))
        .mount(&mock_server)
        .await;

    // Test basic field extraction
    aperture_cmd()
        .env("APERTURE_CONFIG_DIR", config_dir.to_str().unwrap())
        .env("APERTURE_BASE_URL", mock_server.uri())
        .args([
            "api",
            "test-api",
            "users",
            "get-user-by-id",
            "--id",
            "123",
            "--jq",
            ".name",
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains("\"John Doe\""));
}

#[tokio::test]
async fn test_jq_filter_nested_fields() {
    let temp_dir = TempDir::new().unwrap();
    let config_dir = temp_dir.path().to_path_buf();
    let spec_file = temp_dir.path().join("test-spec.yaml");

    let spec_content = "openapi: 3.0.0
info:
  title: Test API
  version: 1.0.0
servers:
  - url: https://api.example.com
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
          content:
            application/json:
              schema:
                type: object
";
    fs::write(&spec_file, spec_content).unwrap();
    aperture_cmd()
        .env("APERTURE_CONFIG_DIR", config_dir.to_str().unwrap())
        .args(["config", "add", "test-api", spec_file.to_str().unwrap()])
        .assert()
        .success();

    let mock_server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/users/123"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "id": "123",
            "name": "John Doe",
            "metadata": {
                "role": "admin",
                "permissions": ["read", "write", "delete"]
            }
        })))
        .mount(&mock_server)
        .await;

    // Test nested field extraction
    aperture_cmd()
        .env("APERTURE_CONFIG_DIR", config_dir.to_str().unwrap())
        .env("APERTURE_BASE_URL", mock_server.uri())
        .args([
            "api",
            "test-api",
            "users",
            "get-user-by-id",
            "--id",
            "123",
            "--jq",
            ".metadata.role",
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains("\"admin\""));
}

#[tokio::test]
async fn test_jq_filter_array_operations() {
    let temp_dir = TempDir::new().unwrap();
    let config_dir = temp_dir.path().to_path_buf();
    let spec_file = temp_dir.path().join("test-spec.yaml");

    let spec_content = "openapi: 3.0.0
info:
  title: Test API
  version: 1.0.0
servers:
  - url: https://api.example.com
paths:
  /users:
    get:
      tags:
        - users
      operationId: listUsers
      responses:
        '200':
          description: Success
          content:
            application/json:
              schema:
                type: array
                items:
                  type: object
";
    fs::write(&spec_file, spec_content).unwrap();
    aperture_cmd()
        .env("APERTURE_CONFIG_DIR", config_dir.to_str().unwrap())
        .args(["config", "add", "test-api", spec_file.to_str().unwrap()])
        .assert()
        .success();

    let mock_server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/users"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!([
            {"id": "1", "name": "Alice", "active": true},
            {"id": "2", "name": "Bob", "active": false},
            {"id": "3", "name": "Charlie", "active": true}
        ])))
        .mount(&mock_server)
        .await;

    // Test array filtering
    aperture_cmd()
        .env("APERTURE_CONFIG_DIR", config_dir.to_str().unwrap())
        .env("APERTURE_BASE_URL", mock_server.uri())
        .args([
            "api",
            "test-api",
            "users",
            "list-users",
            "--jq",
            ".[0].name",
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains("\"Alice\""));
}

#[tokio::test]
async fn test_jq_filter_with_output_formats() {
    let temp_dir = TempDir::new().unwrap();
    let config_dir = temp_dir.path().to_path_buf();
    let spec_file = temp_dir.path().join("test-spec.yaml");

    let spec_content = "openapi: 3.0.0
info:
  title: Test API
  version: 1.0.0
servers:
  - url: https://api.example.com
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
          content:
            application/json:
              schema:
                type: object
";
    fs::write(&spec_file, spec_content).unwrap();
    aperture_cmd()
        .env("APERTURE_CONFIG_DIR", config_dir.to_str().unwrap())
        .args(["config", "add", "test-api", spec_file.to_str().unwrap()])
        .assert()
        .success();

    let mock_server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/users/123"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "id": "123",
            "name": "John Doe",
            "email": "john@example.com",
            "scores": [85, 90, 78, 92]
        })))
        .mount(&mock_server)
        .await;

    // Test JQ with YAML output
    aperture_cmd()
        .env("APERTURE_CONFIG_DIR", config_dir.to_str().unwrap())
        .env("APERTURE_BASE_URL", mock_server.uri())
        .args([
            "api",
            "test-api",
            "users",
            "get-user-by-id",
            "--id",
            "123",
            "--jq",
            ".name",
            "--format",
            "yaml",
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains("John Doe"));
}

#[tokio::test]
async fn test_jq_filter_error_handling() {
    let temp_dir = TempDir::new().unwrap();
    let config_dir = temp_dir.path().to_path_buf();
    let spec_file = temp_dir.path().join("test-spec.yaml");

    let spec_content = "openapi: 3.0.0
info:
  title: Test API
  version: 1.0.0
servers:
  - url: https://api.example.com
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
          content:
            application/json:
              schema:
                type: object
";
    fs::write(&spec_file, spec_content).unwrap();
    aperture_cmd()
        .env("APERTURE_CONFIG_DIR", config_dir.to_str().unwrap())
        .args(["config", "add", "test-api", spec_file.to_str().unwrap()])
        .assert()
        .success();

    let mock_server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/users/123"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "id": "123",
            "name": "John Doe"
        })))
        .mount(&mock_server)
        .await;

    // Test invalid JQ expression
    aperture_cmd()
        .env("APERTURE_CONFIG_DIR", config_dir.to_str().unwrap())
        .env("APERTURE_BASE_URL", mock_server.uri())
        .args([
            "api",
            "test-api",
            "users",
            "get-user-by-id",
            "--id",
            "123",
            "--jq",
            "unsupported complex filter",
        ])
        .assert()
        .failure()
        .stderr(
            predicate::str::contains("Unsupported JQ filter")
                .or(predicate::str::contains("jq error"))
                .or(predicate::str::contains("JQ filter error")),
        );
}

#[test]
fn test_jq_filter_help_text() {
    // Test that --help shows the jq option
    let output = aperture_cmd()
        .args(["api", "test", "--help"])
        .output()
        .unwrap();

    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("--jq") || stdout.contains("jq filter"));
}

#[tokio::test]
async fn test_batch_operations_with_jq_filter() {
    let temp_dir = TempDir::new().unwrap();
    let config_dir = temp_dir.path().to_path_buf();
    let spec_file = temp_dir.path().join("spec.yaml");
    let batch_file = temp_dir.path().join("batch.json");

    // Create a minimal spec
    fs::write(
        &spec_file,
        "openapi: 3.0.0
info:
  title: Test API
  version: 1.0.0
servers:
  - url: https://api.example.com
paths:
  /users/{id}:
    get:
      tags:
        - users
      operationId: getUser
      parameters:
        - name: id
          in: path
          required: true
          schema:
            type: string
      responses:
        '200':
          description: Success
  /posts/{id}:
    get:
      tags:
        - posts
      operationId: getPost
      parameters:
        - name: id
          in: path
          required: true
          schema:
            type: string
      responses:
        '200':
          description: Success
",
    )
    .unwrap();

    // Add the spec
    aperture_cmd()
        .env("APERTURE_CONFIG_DIR", config_dir.to_str().unwrap())
        .args(["config", "add", "test-api", spec_file.to_str().unwrap()])
        .assert()
        .success();

    // Create a batch file with multiple operations
    let batch_ops = serde_json::json!({
        "operations": [
            {
                "id": "op1",
                "args": ["users", "get-user", "--id", "123"]
            },
            {
                "id": "op2",
                "args": ["posts", "get-post", "--id", "456"]
            },
            {
                "id": "op3",
                "args": ["users", "get-user", "--id", "789"]
            }
        ]
    });
    fs::write(
        &batch_file,
        serde_json::to_string_pretty(&batch_ops).unwrap(),
    )
    .unwrap();

    // Mock server setup
    let mock_server = MockServer::start().await;

    // Mock successful user requests
    Mock::given(method("GET"))
        .and(path("/users/123"))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_json(serde_json::json!({"id": "123", "name": "Alice"})),
        )
        .mount(&mock_server)
        .await;

    Mock::given(method("GET"))
        .and(path("/users/789"))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_json(serde_json::json!({"id": "789", "name": "Charlie"})),
        )
        .mount(&mock_server)
        .await;

    // Mock failed post request
    Mock::given(method("GET"))
        .and(path("/posts/456"))
        .respond_with(ResponseTemplate::new(404).set_body_string("Post not found"))
        .mount(&mock_server)
        .await;

    // Test JQ filter to get only failed operations
    let output = aperture_cmd()
        .env("APERTURE_CONFIG_DIR", config_dir.to_str().unwrap())
        .env("APERTURE_BASE_URL", mock_server.uri())
        .args([
            "api",
            "test-api",
            "--batch-file",
            batch_file.to_str().unwrap(),
            "--json-errors",
            "--jq",
            ".batch_execution_summary.failed_operations",
        ])
        .output()
        .unwrap();

    // The command should exit with code 1 since one operation fails
    assert!(
        !output.status.success(),
        "Command should fail when operations fail"
    );
    let stdout = String::from_utf8_lossy(&output.stdout);

    // With --json-errors, only the final JSON summary should be printed
    // Individual operation outputs are suppressed
    let failed_count: serde_json::Value = serde_json::from_str(stdout.trim()).unwrap();
    assert_eq!(failed_count, 1);

    // Test JQ filter to get summary statistics only
    let output = aperture_cmd()
        .env("APERTURE_CONFIG_DIR", config_dir.to_str().unwrap())
        .env("APERTURE_BASE_URL", mock_server.uri())
        .args([
            "api",
            "test-api",
            "--batch-file",
            batch_file.to_str().unwrap(),
            "--json-errors",
            "--jq",
            ".batch_execution_summary.total_operations",
        ])
        .output()
        .unwrap();

    // Exit code 1 because of failed operations
    assert!(!output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    let total_count: serde_json::Value = serde_json::from_str(stdout.trim()).unwrap();
    assert_eq!(total_count, 3);

    // Test JQ filter to get success count
    let output = aperture_cmd()
        .env("APERTURE_CONFIG_DIR", config_dir.to_str().unwrap())
        .env("APERTURE_BASE_URL", mock_server.uri())
        .args([
            "api",
            "test-api",
            "--batch-file",
            batch_file.to_str().unwrap(),
            "--json-errors",
            "--jq",
            ".batch_execution_summary.successful_operations",
        ])
        .output()
        .unwrap();

    // Exit code 1 because of failed operations
    assert!(!output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    let success_count: serde_json::Value = serde_json::from_str(stdout.trim()).unwrap();
    assert_eq!(success_count, 2); // Should have 2 successful operations
}

#[tokio::test]
async fn test_batch_empty_operations_with_jq() {
    let temp_dir = TempDir::new().unwrap();
    let config_dir = temp_dir.path().to_path_buf();
    let spec_file = temp_dir.path().join("spec.yaml");
    let batch_file = temp_dir.path().join("batch.json");

    // Create a minimal spec
    fs::write(
        &spec_file,
        "openapi: 3.0.0
info:
  title: Test API
  version: 1.0.0
servers:
  - url: https://api.example.com
paths:
  /users/{id}:
    get:
      tags:
        - users
      operationId: getUser
      parameters:
        - name: id
          in: path
          required: true
          schema:
            type: string
      responses:
        '200':
          description: Success
",
    )
    .unwrap();

    // Add the spec
    aperture_cmd()
        .env("APERTURE_CONFIG_DIR", config_dir.to_str().unwrap())
        .args(["config", "add", "test-api", spec_file.to_str().unwrap()])
        .assert()
        .success();

    // Create a batch file with no operations
    let batch_ops = serde_json::json!({
        "operations": []
    });
    fs::write(
        &batch_file,
        serde_json::to_string_pretty(&batch_ops).unwrap(),
    )
    .unwrap();

    // Test JQ filter on empty batch
    let output = aperture_cmd()
        .env("APERTURE_CONFIG_DIR", config_dir.to_str().unwrap())
        .args([
            "api",
            "test-api",
            "--batch-file",
            batch_file.to_str().unwrap(),
            "--json-errors",
            "--jq",
            ".batch_execution_summary.total_operations",
        ])
        .output()
        .unwrap();

    // Should succeed with empty batch
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert_eq!(stdout.trim(), "0");
}

#[tokio::test]
async fn test_batch_all_fail_with_jq() {
    let temp_dir = TempDir::new().unwrap();
    let config_dir = temp_dir.path().to_path_buf();
    let spec_file = temp_dir.path().join("spec.yaml");
    let batch_file = temp_dir.path().join("batch.json");

    // Create a minimal spec
    fs::write(
        &spec_file,
        "openapi: 3.0.0
info:
  title: Test API
  version: 1.0.0
servers:
  - url: https://api.example.com
paths:
  /users/{id}:
    get:
      tags:
        - users
      operationId: getUser
      parameters:
        - name: id
          in: path
          required: true
          schema:
            type: string
      responses:
        '200':
          description: Success
",
    )
    .unwrap();

    // Add the spec
    aperture_cmd()
        .env("APERTURE_CONFIG_DIR", config_dir.to_str().unwrap())
        .args(["config", "add", "test-api", spec_file.to_str().unwrap()])
        .assert()
        .success();

    // Create a batch file with operations that will all fail
    let batch_ops = serde_json::json!({
        "operations": [
            {
                "id": "op1",
                "args": ["users", "get-user", "--id", "fail1"]
            },
            {
                "id": "op2",
                "args": ["users", "get-user", "--id", "fail2"]
            }
        ]
    });
    fs::write(
        &batch_file,
        serde_json::to_string_pretty(&batch_ops).unwrap(),
    )
    .unwrap();

    // Mock server setup
    let mock_server = MockServer::start().await;

    // All requests return 404
    Mock::given(method("GET"))
        .and(path_regex("/users/.*"))
        .respond_with(ResponseTemplate::new(404).set_body_string("Not found"))
        .mount(&mock_server)
        .await;

    // Test JQ filter when all operations fail
    let output = aperture_cmd()
        .env("APERTURE_CONFIG_DIR", config_dir.to_str().unwrap())
        .env("APERTURE_BASE_URL", mock_server.uri())
        .args([
            "api",
            "test-api",
            "--batch-file",
            batch_file.to_str().unwrap(),
            "--json-errors",
            "--jq",
            ".batch_execution_summary.failed_operations",
        ])
        .output()
        .unwrap();

    // Should exit with code 1 when all operations fail
    assert!(!output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    let failed_count: serde_json::Value = serde_json::from_str(stdout.trim()).unwrap();
    assert_eq!(failed_count, 2);
}

#[tokio::test]
async fn test_batch_jq_empty_result() {
    let temp_dir = TempDir::new().unwrap();
    let config_dir = temp_dir.path().to_path_buf();
    let spec_file = temp_dir.path().join("spec.yaml");
    let batch_file = temp_dir.path().join("batch.json");

    // Create a minimal spec
    fs::write(
        &spec_file,
        "openapi: 3.0.0
info:
  title: Test API
  version: 1.0.0
servers:
  - url: https://api.example.com
paths:
  /users/{id}:
    get:
      tags:
        - users
      operationId: getUser
      parameters:
        - name: id
          in: path
          required: true
          schema:
            type: string
      responses:
        '200':
          description: Success
",
    )
    .unwrap();

    // Add the spec
    aperture_cmd()
        .env("APERTURE_CONFIG_DIR", config_dir.to_str().unwrap())
        .args(["config", "add", "test-api", spec_file.to_str().unwrap()])
        .assert()
        .success();

    // Create a simple batch file
    let batch_ops = serde_json::json!({
        "operations": [
            {
                "id": "op1",
                "args": ["users", "get-user", "--id", "123"]
            }
        ]
    });
    fs::write(
        &batch_file,
        serde_json::to_string_pretty(&batch_ops).unwrap(),
    )
    .unwrap();

    // Mock server setup
    let mock_server = MockServer::start().await;

    Mock::given(method("GET"))
        .and(path("/users/123"))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_json(serde_json::json!({"id": "123", "name": "Test"})),
        )
        .mount(&mock_server)
        .await;

    // Test JQ filter that returns empty/null
    let output = aperture_cmd()
        .env("APERTURE_CONFIG_DIR", config_dir.to_str().unwrap())
        .env("APERTURE_BASE_URL", mock_server.uri())
        .args([
            "api",
            "test-api",
            "--batch-file",
            batch_file.to_str().unwrap(),
            "--json-errors",
            "--jq",
            ".batch_execution_summary.nonexistent_field",
        ])
        .output()
        .unwrap();

    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert_eq!(stdout.trim(), "null");
}
