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
    assert_eq!(error["error_type"].as_str().unwrap(), "CachedSpecNotFound");
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
            "{\"name\":\"John\"}",
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
    Command::cargo_bin("aperture")
        .unwrap()
        .env("APERTURE_CONFIG_DIR", config_dir.to_str().unwrap())
        .args(&["config", "add", "test-api", spec_file.to_str().unwrap()])
        .assert()
        .success();

    // Test list-commands
    let output = Command::cargo_bin("aperture")
        .unwrap()
        .env("APERTURE_CONFIG_DIR", config_dir.to_str().unwrap())
        .args(&["list-commands", "test-api"])
        .output()
        .unwrap();

    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);

    // Check that commands are grouped by tags
    assert!(stdout.contains("users"));
    assert!(stdout.contains("posts"));
    assert!(stdout.contains("default")); // For healthCheck without tag

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
    Command::cargo_bin("aperture")
        .unwrap()
        .env("APERTURE_CONFIG_DIR", config_dir.to_str().unwrap())
        .args(&["config", "add", "test-api", spec_file.to_str().unwrap()])
        .assert()
        .success();

    // Test reinit for specific spec
    Command::cargo_bin("aperture")
        .unwrap()
        .env("APERTURE_CONFIG_DIR", config_dir.to_str().unwrap())
        .args(&["config", "reinit", "test-api"])
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

    Command::cargo_bin("aperture")
        .unwrap()
        .env("APERTURE_CONFIG_DIR", config_dir.to_str().unwrap())
        .args(&["config", "add", "test-api-2", spec_file2.to_str().unwrap()])
        .assert()
        .success();

    // Test reinit --all
    Command::cargo_bin("aperture")
        .unwrap()
        .env("APERTURE_CONFIG_DIR", config_dir.to_str().unwrap())
        .args(&["config", "reinit", "--all"])
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
    Command::cargo_bin("aperture")
        .unwrap()
        .env("APERTURE_CONFIG_DIR", config_dir.to_str().unwrap())
        .args(&["config", "add", "test-api", spec_file.to_str().unwrap()])
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
    let output = Command::cargo_bin("aperture")
        .unwrap()
        .env("APERTURE_CONFIG_DIR", config_dir.to_str().unwrap())
        .env("APERTURE_BASE_URL", &mock_server.uri())
        .env("TEST_API_KEY", "test-key")
        .env("BEARER_TOKEN", "test-token")
        .args(&["--json-errors", "api", "test-api", "users", "list-users"])
        .output()
        .unwrap();

    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);

    // Parse the JSON error output
    let error: serde_json::Value = serde_json::from_str(&stderr).unwrap();
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
    Command::cargo_bin("aperture")
        .unwrap()
        .env("APERTURE_CONFIG_DIR", config_dir.to_str().unwrap())
        .args(&["config", "add", "test-api", spec_file.to_str().unwrap()])
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
    let output = Command::cargo_bin("aperture")
        .unwrap()
        .env("APERTURE_CONFIG_DIR", config_dir.to_str().unwrap())
        .env("APERTURE_BASE_URL", &mock_server.uri())
        .args(&["api", "test-api", "--format", "json", "users", "list-users"])
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
    Command::cargo_bin("aperture")
        .unwrap()
        .env("APERTURE_CONFIG_DIR", config_dir.to_str().unwrap())
        .args(&["config", "add", "test-api", spec_file.to_str().unwrap()])
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
    let output = Command::cargo_bin("aperture")
        .unwrap()
        .env("APERTURE_CONFIG_DIR", config_dir.to_str().unwrap())
        .env("APERTURE_BASE_URL", &mock_server.uri())
        .args(&["api", "test-api", "--format", "yaml", "users", "list-users"])
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
    Command::cargo_bin("aperture")
        .unwrap()
        .env("APERTURE_CONFIG_DIR", config_dir.to_str().unwrap())
        .args(&["config", "add", "test-api", spec_file.to_str().unwrap()])
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
    let output = Command::cargo_bin("aperture")
        .unwrap()
        .env("APERTURE_CONFIG_DIR", config_dir.to_str().unwrap())
        .env("APERTURE_BASE_URL", &mock_server.uri())
        .args(&[
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
    assert!(stdout.contains("|")); // Table borders
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
    Command::cargo_bin("aperture")
        .unwrap()
        .env("APERTURE_CONFIG_DIR", config_dir.to_str().unwrap())
        .args(&["config", "add", "test-api", spec_file.to_str().unwrap()])
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
    let output = Command::cargo_bin("aperture")
        .unwrap()
        .env("APERTURE_CONFIG_DIR", config_dir.to_str().unwrap())
        .env("APERTURE_BASE_URL", &mock_server.uri())
        .args(&[
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
    assert!(stdout.contains("|"));
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
    Command::cargo_bin("aperture")
        .unwrap()
        .env("APERTURE_CONFIG_DIR", config_dir.to_str().unwrap())
        .args(&["config", "add", "test-api", spec_file.to_str().unwrap()])
        .assert()
        .success();

    // Test invalid format
    let output = Command::cargo_bin("aperture")
        .unwrap()
        .env("APERTURE_CONFIG_DIR", config_dir.to_str().unwrap())
        .args(&[
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
    let output = Command::cargo_bin("aperture")
        .unwrap()
        .args(&["api", "test", "--help"])
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
    Command::cargo_bin("aperture")
        .unwrap()
        .env("APERTURE_CONFIG_DIR", config_dir.to_str().unwrap())
        .env("APERTURE_BASE_URL", &mock_server.uri())
        .args(&[
            "api",
            "test-api",
            "users",
            "get-user-by-id",
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
            "name": "John Doe",
            "metadata": {
                "role": "admin",
                "permissions": ["read", "write", "delete"]
            }
        })))
        .mount(&mock_server)
        .await;

    // Test nested field extraction
    Command::cargo_bin("aperture")
        .unwrap()
        .env("APERTURE_CONFIG_DIR", config_dir.to_str().unwrap())
        .env("APERTURE_BASE_URL", &mock_server.uri())
        .args(&[
            "api",
            "test-api",
            "users",
            "get-user-by-id",
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
    Command::cargo_bin("aperture")
        .unwrap()
        .env("APERTURE_CONFIG_DIR", config_dir.to_str().unwrap())
        .args(&["config", "add", "test-api", spec_file.to_str().unwrap()])
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
    Command::cargo_bin("aperture")
        .unwrap()
        .env("APERTURE_CONFIG_DIR", config_dir.to_str().unwrap())
        .env("APERTURE_BASE_URL", &mock_server.uri())
        .args(&["api", "test-api", "users", "list-users", "--jq", ".0.name"])
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
            "name": "John Doe",
            "email": "john@example.com",
            "scores": [85, 90, 78, 92]
        })))
        .mount(&mock_server)
        .await;

    // Test JQ with YAML output
    Command::cargo_bin("aperture")
        .unwrap()
        .env("APERTURE_CONFIG_DIR", config_dir.to_str().unwrap())
        .env("APERTURE_BASE_URL", &mock_server.uri())
        .args(&[
            "api",
            "test-api",
            "users",
            "get-user-by-id",
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
            "name": "John Doe"
        })))
        .mount(&mock_server)
        .await;

    // Test invalid JQ expression
    Command::cargo_bin("aperture")
        .unwrap()
        .env("APERTURE_CONFIG_DIR", config_dir.to_str().unwrap())
        .env("APERTURE_BASE_URL", &mock_server.uri())
        .args(&[
            "api",
            "test-api",
            "users",
            "get-user-by-id",
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
    let output = Command::cargo_bin("aperture")
        .unwrap()
        .args(&["api", "test", "--help"])
        .output()
        .unwrap();

    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("--jq") || stdout.contains("jq filter"));
}
