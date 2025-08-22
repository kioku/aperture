#![cfg(feature = "integration")]

mod common;

use common::aperture_cmd;
use predicates::prelude::*;
use std::fs;
use tempfile::TempDir;
use wiremock::{Mock, MockServer, ResponseTemplate};

fn setup_test_env() -> (TempDir, String) {
    let temp_dir = TempDir::new().unwrap();
    let config_dir = temp_dir.path().join(".config").join("aperture");
    fs::create_dir_all(&config_dir).unwrap();
    (temp_dir, config_dir.to_string_lossy().to_string())
}

#[tokio::test]
async fn test_custom_http_scheme_token_execution() {
    let (temp_dir, config_dir) = setup_test_env();
    let mock_server = MockServer::start().await;

    // Create OpenAPI spec with Token auth scheme
    let spec_content = format!(
        r#"
openapi: 3.0.0
info:
  title: Token Auth Test API
  version: 1.0.0
servers:
  - url: {}
components:
  securitySchemes:
    tokenAuth:
      type: http
      scheme: Token
      x-aperture-secret:
        source: env
        name: TEST_TOKEN
paths:
  /protected:
    get:
      operationId: getProtected
      security:
        - tokenAuth: []
      responses:
        '200':
          description: Success
          content:
            application/json:
              schema:
                type: object
"#,
        mock_server.uri()
    );

    let spec_file = temp_dir.path().join("token-api.yaml");
    fs::write(&spec_file, spec_content).unwrap();

    // Add the spec
    aperture_cmd()
        .env("APERTURE_CONFIG_DIR", &config_dir)
        .args(&["config", "add", "token-api", spec_file.to_str().unwrap()])
        .assert()
        .success();

    // Set up mock to verify the correct Authorization header
    Mock::given(wiremock::matchers::method("GET"))
        .and(wiremock::matchers::path("/protected"))
        .and(wiremock::matchers::header(
            "Authorization",
            "Token test-token-123",
        ))
        .respond_with(ResponseTemplate::new(200).set_body_string(r#"{"status": "success"}"#))
        .expect(1)
        .mount(&mock_server)
        .await;

    // Execute the command with Token auth
    aperture_cmd()
        .env("APERTURE_CONFIG_DIR", &config_dir)
        .env("TEST_TOKEN", "test-token-123")
        .args(&["api", "token-api", "default", "get-protected"])
        .assert()
        .success()
        .stdout(predicate::str::contains("\"status\": \"success\""));
}

#[tokio::test]
async fn test_custom_http_scheme_dsn_execution() {
    let (temp_dir, config_dir) = setup_test_env();
    let mock_server = MockServer::start().await;

    // Create OpenAPI spec with DSN auth scheme
    let spec_content = format!(
        r#"
openapi: 3.0.0
info:
  title: DSN Auth Test API
  version: 1.0.0
servers:
  - url: {}
components:
  securitySchemes:
    dsnAuth:
      type: http
      scheme: DSN
      x-aperture-secret:
        source: env
        name: SENTRY_DSN
paths:
  /api/events:
    post:
      operationId: sendEvent
      security:
        - dsnAuth: []
      requestBody:
        required: true
        content:
          application/json:
            schema:
              type: object
      responses:
        '200':
          description: Event accepted
"#,
        mock_server.uri()
    );

    let spec_file = temp_dir.path().join("dsn-api.yaml");
    fs::write(&spec_file, spec_content).unwrap();

    // Add the spec
    aperture_cmd()
        .env("APERTURE_CONFIG_DIR", &config_dir)
        .args(&["config", "add", "dsn-api", spec_file.to_str().unwrap()])
        .assert()
        .success();

    // Set up mock to verify the correct DSN Authorization header
    Mock::given(wiremock::matchers::method("POST"))
        .and(wiremock::matchers::path("/api/events"))
        .and(wiremock::matchers::header(
            "Authorization",
            "DSN https://key@sentry.io/123",
        ))
        .respond_with(ResponseTemplate::new(200).set_body_string(r#"{"id": "event123"}"#))
        .expect(1)
        .mount(&mock_server)
        .await;

    // Execute the command with DSN auth
    aperture_cmd()
        .env("APERTURE_CONFIG_DIR", &config_dir)
        .env("SENTRY_DSN", "https://key@sentry.io/123")
        .args(&[
            "api",
            "dsn-api",
            "default",
            "send-event",
            "--body",
            r#"{"type": "error"}"#,
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains("\"id\": \"event123\""));
}

#[tokio::test]
async fn test_custom_http_scheme_proprietary() {
    let (temp_dir, config_dir) = setup_test_env();
    let mock_server = MockServer::start().await;

    // Create OpenAPI spec with completely custom auth scheme
    let spec_content = format!(
        r#"
openapi: 3.0.0
info:
  title: Custom Auth Test API
  version: 1.0.0
servers:
  - url: {}
components:
  securitySchemes:
    customAuth:
      type: http
      scheme: X-Custom-Auth
      x-aperture-secret:
        source: env
        name: CUSTOM_AUTH_KEY
paths:
  /api/data:
    get:
      operationId: getData
      security:
        - customAuth: []
      responses:
        '200':
          description: Success
"#,
        mock_server.uri()
    );

    let spec_file = temp_dir.path().join("custom-api.yaml");
    fs::write(&spec_file, spec_content).unwrap();

    // Add the spec
    aperture_cmd()
        .env("APERTURE_CONFIG_DIR", &config_dir)
        .args(&["config", "add", "custom-api", spec_file.to_str().unwrap()])
        .assert()
        .success();

    // Set up mock to verify the correct custom Authorization header
    Mock::given(wiremock::matchers::method("GET"))
        .and(wiremock::matchers::path("/api/data"))
        .and(wiremock::matchers::header(
            "Authorization",
            "X-Custom-Auth secret-key-789",
        ))
        .respond_with(ResponseTemplate::new(200).set_body_string(r#"{"data": "test"}"#))
        .expect(1)
        .mount(&mock_server)
        .await;

    // Execute the command with custom auth
    aperture_cmd()
        .env("APERTURE_CONFIG_DIR", &config_dir)
        .env("CUSTOM_AUTH_KEY", "secret-key-789")
        .args(&["api", "custom-api", "default", "get-data"])
        .assert()
        .success()
        .stdout(predicate::str::contains("\"data\": \"test\""));
}

#[tokio::test]
async fn test_dry_run_shows_custom_auth_header() {
    let (temp_dir, config_dir) = setup_test_env();

    // Create OpenAPI spec with Token auth
    let spec_content = r#"
openapi: 3.0.0
info:
  title: Token Auth Dry Run Test
  version: 1.0.0
servers:
  - url: https://api.example.com
components:
  securitySchemes:
    tokenAuth:
      type: http
      scheme: Token
      x-aperture-secret:
        source: env
        name: API_TOKEN
paths:
  /users:
    get:
      operationId: getUsers
      security:
        - tokenAuth: []
      responses:
        '200':
          description: Success
"#;

    let spec_file = temp_dir.path().join("dry-run-api.yaml");
    fs::write(&spec_file, spec_content).unwrap();

    // Add the spec
    aperture_cmd()
        .env("APERTURE_CONFIG_DIR", &config_dir)
        .args(&["config", "add", "dry-run-api", spec_file.to_str().unwrap()])
        .assert()
        .success();

    // Run with --dry-run to see the request details
    aperture_cmd()
        .env("APERTURE_CONFIG_DIR", &config_dir)
        .env("API_TOKEN", "my-token-value")
        .args(&["api", "--dry-run", "dry-run-api", "default", "get-users"])
        .assert()
        .success()
        .stdout(predicate::str::contains(
            "\"authorization\": \"<REDACTED>\"",
        ))
        .stdout(predicate::str::contains(
            "\"url\": \"https://api.example.com/users\"",
        ))
        .stdout(predicate::str::contains("my-token-value").not());
}

#[tokio::test]
async fn test_basic_auth_base64_encoding() {
    let (temp_dir, config_dir) = setup_test_env();
    let mock_server = MockServer::start().await;

    // Create OpenAPI spec with Basic auth
    let spec_content = format!(
        r#"
openapi: 3.0.0
info:
  title: Basic Auth Test API
  version: 1.0.0
servers:
  - url: {}
components:
  securitySchemes:
    basicAuth:
      type: http
      scheme: basic
      x-aperture-secret:
        source: env
        name: BASIC_CREDS
paths:
  /api/secure:
    get:
      operationId: getSecure
      security:
        - basicAuth: []
      responses:
        '200':
          description: Success
"#,
        mock_server.uri()
    );

    let spec_file = temp_dir.path().join("basic-api.yaml");
    fs::write(&spec_file, spec_content).unwrap();

    // Add the spec
    aperture_cmd()
        .env("APERTURE_CONFIG_DIR", &config_dir)
        .args(&["config", "add", "basic-api", spec_file.to_str().unwrap()])
        .assert()
        .success();

    // Set up mock to verify the correct Basic Authorization header
    // "testuser:testpass" base64 encoded is "dGVzdHVzZXI6dGVzdHBhc3M="
    Mock::given(wiremock::matchers::method("GET"))
        .and(wiremock::matchers::path("/api/secure"))
        .and(wiremock::matchers::header(
            "Authorization",
            "Basic dGVzdHVzZXI6dGVzdHBhc3M=",
        ))
        .respond_with(ResponseTemplate::new(200).set_body_string(r#"{"status": "authenticated"}"#))
        .expect(1)
        .mount(&mock_server)
        .await;

    // Execute the command with Basic auth
    aperture_cmd()
        .env("APERTURE_CONFIG_DIR", &config_dir)
        .env("BASIC_CREDS", "testuser:testpass")
        .args(&["api", "basic-api", "default", "get-secure"])
        .assert()
        .success()
        .stdout(predicate::str::contains("\"status\": \"authenticated\""));
}

#[tokio::test]
async fn test_header_injection_protection() {
    let (temp_dir, config_dir) = setup_test_env();

    // Create a simple OpenAPI spec
    let spec_content = r#"
openapi: 3.0.0
info:
  title: Header Injection Test API
  version: 1.0.0
servers:
  - url: https://api.example.com
components:
  securitySchemes:
    bearerAuth:
      type: http
      scheme: bearer
      x-aperture-secret:
        source: env
        name: API_TOKEN
paths:
  /test:
    get:
      operationId: getTest
      security:
        - bearerAuth: []
      parameters:
        - name: custom-header
          in: header
          required: false
          schema:
            type: string
      responses:
        '200':
          description: Success
"#;

    let spec_file = temp_dir.path().join("injection-api.yaml");
    fs::write(&spec_file, spec_content).unwrap();

    // Add the spec
    aperture_cmd()
        .env("APERTURE_CONFIG_DIR", &config_dir)
        .args(&[
            "config",
            "add",
            "injection-api",
            spec_file.to_str().unwrap(),
        ])
        .assert()
        .success();

    // Test 1: Newline in custom header value
    aperture_cmd()
        .env("APERTURE_CONFIG_DIR", &config_dir)
        .env("API_TOKEN", "valid-token")
        .args(&[
            "api",
            "injection-api",
            "default",
            "get-test",
            "--header",
            "X-Custom: value\nX-Injected: malicious",
        ])
        .assert()
        .failure()
        .stderr(predicate::str::contains("invalid control characters"));

    // Test 2: Carriage return in custom header
    aperture_cmd()
        .env("APERTURE_CONFIG_DIR", &config_dir)
        .env("API_TOKEN", "valid-token")
        .args(&[
            "api",
            "injection-api",
            "default",
            "get-test",
            "--header",
            "X-Custom: value\rX-Injected: malicious",
        ])
        .assert()
        .failure()
        .stderr(predicate::str::contains("invalid control characters"));

    // Test 3: Valid header should work
    aperture_cmd()
        .env("APERTURE_CONFIG_DIR", &config_dir)
        .env("API_TOKEN", "valid-token")
        .args(&[
            "api",
            "--dry-run",
            "injection-api",
            "default",
            "get-test",
            "--header",
            "X-Custom: valid-value",
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains("\"x-custom\": \"valid-value\""));

    // Test 4: Environment variable expansion with newline
    aperture_cmd()
        .env("APERTURE_CONFIG_DIR", &config_dir)
        .env("API_TOKEN", "valid-token")
        .env("MALICIOUS_VAR", "value\nX-Injected: bad")
        .args(&[
            "api",
            "injection-api",
            "default",
            "get-test",
            "--header",
            "X-Custom: ${MALICIOUS_VAR}",
        ])
        .assert()
        .failure()
        .stderr(predicate::str::contains("invalid control characters"));
}

#[tokio::test]
async fn test_auth_token_injection_protection() {
    let (temp_dir, config_dir) = setup_test_env();

    // Create OpenAPI spec
    let spec_content = r#"
openapi: 3.0.0
info:
  title: Token Injection Test API
  version: 1.0.0
servers:
  - url: https://api.example.com
components:
  securitySchemes:
    bearerAuth:
      type: http
      scheme: bearer
      x-aperture-secret:
        source: env
        name: BEARER_TOKEN
paths:
  /protected:
    get:
      operationId: getProtected
      security:
        - bearerAuth: []
      responses:
        '200':
          description: Success
"#;

    let spec_file = temp_dir.path().join("token-injection-api.yaml");
    fs::write(&spec_file, spec_content).unwrap();

    // Add the spec
    aperture_cmd()
        .env("APERTURE_CONFIG_DIR", &config_dir)
        .args(&[
            "config",
            "add",
            "token-injection-api",
            spec_file.to_str().unwrap(),
        ])
        .assert()
        .success();

    // Test auth token with newline
    aperture_cmd()
        .env("APERTURE_CONFIG_DIR", &config_dir)
        .env("BEARER_TOKEN", "token\nX-Injected: malicious")
        .args(&["api", "token-injection-api", "default", "get-protected"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("invalid control characters"));

    // Test auth token with carriage return
    aperture_cmd()
        .env("APERTURE_CONFIG_DIR", &config_dir)
        .env("BEARER_TOKEN", "token\rmalicious")
        .args(&["api", "token-injection-api", "default", "get-protected"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("invalid control characters"));
}

#[tokio::test]
async fn test_dry_run_redacts_bearer_auth_header() {
    let (temp_dir, config_dir) = setup_test_env();

    // Create a spec with Bearer authentication
    let spec_content = r#"
openapi: 3.0.3
info:
  title: Bearer Auth API
  version: 1.0.0
servers:
  - url: https://api.example.com
components:
  securitySchemes:
    bearerAuth:
      type: http
      scheme: bearer
      x-aperture-secret:
        source: env
        name: BEARER_TOKEN
paths:
  /protected:
    get:
      operationId: getProtected
      security:
        - bearerAuth: []
      responses:
        '200':
          description: Success
"#;

    let spec_file = temp_dir.path().join("bearer-api.yaml");
    fs::write(&spec_file, spec_content).unwrap();

    // Add the spec
    aperture_cmd()
        .env("APERTURE_CONFIG_DIR", &config_dir)
        .args(&["config", "add", "bearer-api", spec_file.to_str().unwrap()])
        .assert()
        .success();

    // Run with --dry-run and verify Bearer token is redacted
    aperture_cmd()
        .env("APERTURE_CONFIG_DIR", &config_dir)
        .env("BEARER_TOKEN", "super-secret-bearer-token")
        .args(&["api", "--dry-run", "bearer-api", "default", "get-protected"])
        .assert()
        .success()
        .stdout(predicate::str::contains(
            "\"authorization\": \"<REDACTED>\"",
        ))
        .stdout(predicate::str::contains("super-secret-bearer-token").not());
}

#[tokio::test]
async fn test_dry_run_redacts_api_key_headers() {
    let (temp_dir, config_dir) = setup_test_env();

    // Create a spec with API Key authentication in header
    let spec_content = r#"
openapi: 3.0.3
info:
  title: API Key Auth API
  version: 1.0.0
servers:
  - url: https://api.example.com
components:
  securitySchemes:
    apiKeyAuth:
      type: apiKey
      in: header
      name: X-API-Key
      x-aperture-secret:
        source: env
        name: API_KEY
paths:
  /data:
    get:
      operationId: getData
      security:
        - apiKeyAuth: []
      responses:
        '200':
          description: Success
"#;

    let spec_file = temp_dir.path().join("api-key-api.yaml");
    fs::write(&spec_file, spec_content).unwrap();

    // Add the spec
    aperture_cmd()
        .env("APERTURE_CONFIG_DIR", &config_dir)
        .args(&["config", "add", "api-key-api", spec_file.to_str().unwrap()])
        .assert()
        .success();

    // Run with --dry-run and verify API key is redacted
    aperture_cmd()
        .env("APERTURE_CONFIG_DIR", &config_dir)
        .env("API_KEY", "my-secret-api-key-12345")
        .args(&["api", "--dry-run", "api-key-api", "default", "get-data"])
        .assert()
        .success()
        .stdout(predicate::str::contains("\"x-api-key\": \"<REDACTED>\""))
        .stdout(predicate::str::contains("my-secret-api-key-12345").not());
}

#[tokio::test]
async fn test_dry_run_shows_non_sensitive_headers() {
    let (temp_dir, config_dir) = setup_test_env();

    // Create a simple spec without auth to test non-sensitive headers
    let spec_content = r#"
openapi: 3.0.3
info:
  title: Simple API
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
"#;

    let spec_file = temp_dir.path().join("simple-api.yaml");
    fs::write(&spec_file, spec_content).unwrap();

    // Add the spec
    aperture_cmd()
        .env("APERTURE_CONFIG_DIR", &config_dir)
        .args(&["config", "add", "simple-api", spec_file.to_str().unwrap()])
        .assert()
        .success();

    // Run with --dry-run and verify non-sensitive headers are shown
    aperture_cmd()
        .env("APERTURE_CONFIG_DIR", &config_dir)
        .args(&["api", "--dry-run", "simple-api", "default", "get-users"])
        .assert()
        .success()
        .stdout(predicate::str::contains(
            "\"user-agent\": \"aperture/0.1.0\"",
        ))
        .stdout(predicate::str::contains("\"accept\": \"application/json\""));
}
