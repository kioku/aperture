use assert_cmd::Command;
use predicates::prelude::*;
use std::fs;
use tempfile::TempDir;
use wiremock::matchers::{header, method, path};
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
";
    fs::write(&spec_file, spec_content).unwrap();

    // Test adding the spec
    let add_output = Command::cargo_bin("aperture")
        .unwrap()
        .env("APERTURE_CONFIG_DIR", config_dir.to_str().unwrap())
        .args(&["config", "add", "test-api", spec_file.to_str().unwrap()])
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
    Command::cargo_bin("aperture")
        .unwrap()
        .env("APERTURE_CONFIG_DIR", config_dir.to_str().unwrap())
        .env("APERTURE_BASE_URL", &mock_server.uri())
        .args(&["api", "test-api", "users", "get-user-by-id", "123"])
        .assert()
        .success()
        .stdout(predicate::str::contains("Executing GET"))
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
        env: TEST_KEY
paths: {}
",
    )
    .unwrap();

    // Add multiple specs
    Command::cargo_bin("aperture")
        .unwrap()
        .env("APERTURE_CONFIG_DIR", config_dir.to_str().unwrap())
        .args(&["config", "add", "api-one", spec_file.to_str().unwrap()])
        .assert()
        .success();

    Command::cargo_bin("aperture")
        .unwrap()
        .env("APERTURE_CONFIG_DIR", config_dir.to_str().unwrap())
        .args(&["config", "add", "api-two", spec_file.to_str().unwrap()])
        .assert()
        .success();

    // Test listing specs
    Command::cargo_bin("aperture")
        .unwrap()
        .env("APERTURE_CONFIG_DIR", config_dir.to_str().unwrap())
        .args(&["config", "list"])
        .assert()
        .success()
        .stdout(predicate::str::contains("api-one"))
        .stdout(predicate::str::contains("api-two"));

    // Test removing a spec
    Command::cargo_bin("aperture")
        .unwrap()
        .env("APERTURE_CONFIG_DIR", config_dir.to_str().unwrap())
        .args(&["config", "remove", "api-one"])
        .assert()
        .success()
        .stdout(predicate::str::contains(
            "Spec 'api-one' removed successfully",
        ));

    // Verify only api-two remains
    Command::cargo_bin("aperture")
        .unwrap()
        .env("APERTURE_CONFIG_DIR", config_dir.to_str().unwrap())
        .args(&["config", "list"])
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
    Command::cargo_bin("aperture")
        .unwrap()
        .env("APERTURE_CONFIG_DIR", config_dir.to_str().unwrap())
        .args(&["api", "nonexistent", "users", "list"])
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
        env: TEST_KEY
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

    Command::cargo_bin("aperture")
        .unwrap()
        .env("APERTURE_CONFIG_DIR", config_dir.to_str().unwrap())
        .args(&["config", "add", "test-api", spec_file.to_str().unwrap()])
        .assert()
        .success();

    // Test with invalid JSON body
    let mock_server = MockServer::start().await;

    Command::cargo_bin("aperture")
        .unwrap()
        .env("APERTURE_CONFIG_DIR", config_dir.to_str().unwrap())
        .env("APERTURE_BASE_URL", &mock_server.uri())
        .args(&[
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
        env: TEST_KEY
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

    Command::cargo_bin("aperture")
        .unwrap()
        .env("APERTURE_CONFIG_DIR", config_dir.to_str().unwrap())
        .args(&["config", "add", "test-api", spec_file.to_str().unwrap()])
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
    Command::cargo_bin("aperture")
        .unwrap()
        .env("APERTURE_CONFIG_DIR", config_dir.to_str().unwrap())
        .env("APERTURE_BASE_URL", &mock_server.uri())
        .args(&["api", "test-api", "users", "get-user-by-id", "999"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("404"))
        .stderr(predicate::str::contains("User not found"));
}

#[test]
fn test_help_output() {
    // Test root help
    Command::cargo_bin("aperture")
        .unwrap()
        .arg("--help")
        .assert()
        .success()
        .stdout(predicate::str::contains("Commands:"))
        .stdout(predicate::str::contains("config"))
        .stdout(predicate::str::contains("api"));

    // Test config help
    Command::cargo_bin("aperture")
        .unwrap()
        .args(&["config", "--help"])
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
        env: TEST_KEY
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

    Command::cargo_bin("aperture")
        .unwrap()
        .env("APERTURE_CONFIG_DIR", config_dir.to_str().unwrap())
        .args(&["config", "add", "test-api", spec_file.to_str().unwrap()])
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

    Command::cargo_bin("aperture")
        .unwrap()
        .env("APERTURE_CONFIG_DIR", config_dir.to_str().unwrap())
        .env("APERTURE_BASE_URL", &mock_server.uri())
        .args(&[
            "api",
            "test-api",
            "search",
            "search-items",
            "--q",
            "test",
            "--limit",
            "10",
            "--X-API-Key",
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
        env: TEST_KEY
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
    Command::cargo_bin("aperture")
        .unwrap()
        .env("APERTURE_CONFIG_DIR", config_dir.to_str().unwrap())
        .args(&["config", "add", "test-api", spec_file.to_str().unwrap()])
        .assert()
        .success();

    // Test --describe-json flag
    let output = Command::cargo_bin("aperture")
        .unwrap()
        .env("APERTURE_CONFIG_DIR", config_dir.to_str().unwrap())
        .args(&["api", "test-api", "--describe-json"])
        .output()
        .unwrap();

    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    
    // Parse the JSON output to verify structure
    let manifest: serde_json::Value = serde_json::from_str(&stdout).unwrap();
    assert_eq!(manifest["api"]["name"].as_str().unwrap(), "test-api");
    assert_eq!(manifest["api"]["version"].as_str().unwrap(), "1.0.0");
    assert!(manifest["commands"]["users"].is_array());
    
    let users_commands = manifest["commands"]["users"].as_array().unwrap();
    assert_eq!(users_commands.len(), 1);
    assert_eq!(users_commands[0]["name"].as_str().unwrap(), "get-user-by-id");
    assert_eq!(users_commands[0]["method"].as_str().unwrap(), "GET");
    assert_eq!(users_commands[0]["operation_id"].as_str().unwrap(), "getUserById");
}

#[tokio::test]
async fn test_json_errors_flag() {
    let temp_dir = TempDir::new().unwrap();
    let config_dir = temp_dir.path().to_path_buf();

    // Test with --json-errors for nonexistent spec
    let output = Command::cargo_bin("aperture")
        .unwrap()
        .env("APERTURE_CONFIG_DIR", config_dir.to_str().unwrap())
        .args(&["--json-errors", "api", "nonexistent", "users", "list"])
        .output()
        .unwrap();

    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    
    // Parse the JSON error output
    let error: serde_json::Value = serde_json::from_str(&stderr).unwrap();
    assert_eq!(error["error_type"].as_str().unwrap(), "Configuration");
    assert!(error["message"].as_str().unwrap().contains("No cached spec found"));
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
        env: TEST_KEY
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
    Command::cargo_bin("aperture")
        .unwrap()
        .env("APERTURE_CONFIG_DIR", config_dir.to_str().unwrap())
        .args(&["config", "add", "test-api", spec_file.to_str().unwrap()])
        .assert()
        .success();

    // Test --dry-run flag
    let output = Command::cargo_bin("aperture")
        .unwrap()
        .env("APERTURE_CONFIG_DIR", config_dir.to_str().unwrap())
        .env("TEST_KEY", "secret123")
        .args(&[
            "api",
            "test-api",
            "--dry-run",
            "users",
            "create-user",
            "--body",
            "{\"name\":\"John\"}"
        ])
        .output()
        .unwrap();

    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    
    // Parse the dry-run JSON output
    let dry_run_info: serde_json::Value = serde_json::from_str(&stdout).unwrap();
    assert_eq!(dry_run_info["dry_run"].as_bool().unwrap(), true);
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
        env: TEST_KEY
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
    Command::cargo_bin("aperture")
        .unwrap()
        .env("APERTURE_CONFIG_DIR", config_dir.to_str().unwrap())
        .args(&["config", "add", "test-api", spec_file.to_str().unwrap()])
        .assert()
        .success();

    // Test --idempotency-key with --dry-run to see headers
    let output = Command::cargo_bin("aperture")
        .unwrap()
        .env("APERTURE_CONFIG_DIR", config_dir.to_str().unwrap())
        .env("TEST_KEY", "secret123")
        .args(&[
            "api",
            "test-api",
            "--dry-run",
            "--idempotency-key",
            "my-unique-key-123",
            "users",
            "create-user"
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
