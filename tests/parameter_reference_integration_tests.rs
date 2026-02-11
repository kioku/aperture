#![cfg(feature = "integration")]
// These lints are overly pedantic for test code
#![allow(clippy::too_many_lines)]

use aperture_cli::config::context_name::ApiContextName;
use aperture_cli::config::manager::ConfigManager;

/// Helper to create a validated ApiContextName from a string literal in tests
fn name(s: &str) -> ApiContextName {
    ApiContextName::new(s).expect("test name should be valid")
}
use aperture_cli::fs::FileSystem;
mod common;

use common::aperture_cmd;
use predicates::prelude::*;
use std::fs;
use std::path::PathBuf;
use tempfile::TempDir;

struct TestFS {
    temp_dir: TempDir,
}

impl TestFS {
    fn new() -> Self {
        Self {
            temp_dir: TempDir::new().unwrap(),
        }
    }

    fn write_spec(&self, content: &str) -> PathBuf {
        let spec_path = self.temp_dir.path().join("test-spec.yaml");
        fs::write(&spec_path, content).unwrap();
        spec_path
    }

    fn config_dir(&self) -> PathBuf {
        self.temp_dir.path().join(".config")
    }
}

impl FileSystem for TestFS {
    fn read_to_string(&self, path: &std::path::Path) -> std::io::Result<String> {
        fs::read_to_string(path)
    }

    fn write_all(&self, path: &std::path::Path, contents: &[u8]) -> std::io::Result<()> {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        fs::write(path, contents)
    }

    fn exists(&self, path: &std::path::Path) -> bool {
        path.exists()
    }

    fn create_dir_all(&self, path: &std::path::Path) -> std::io::Result<()> {
        fs::create_dir_all(path)
    }

    fn remove_file(&self, path: &std::path::Path) -> std::io::Result<()> {
        fs::remove_file(path)
    }

    fn remove_dir_all(&self, path: &std::path::Path) -> std::io::Result<()> {
        fs::remove_dir_all(path)
    }

    fn is_dir(&self, path: &std::path::Path) -> bool {
        path.is_dir()
    }

    fn is_file(&self, path: &std::path::Path) -> bool {
        path.is_file()
    }

    fn canonicalize(&self, path: &std::path::Path) -> std::io::Result<PathBuf> {
        path.canonicalize()
    }

    fn read_dir(&self, path: &std::path::Path) -> std::io::Result<Vec<PathBuf>> {
        Ok(fs::read_dir(path)?
            .filter_map(std::result::Result::ok)
            .map(|entry| entry.path())
            .collect())
    }
}

#[test]
fn test_add_spec_with_parameter_references() {
    let fs = TestFS::new();
    let spec_with_refs = r"
openapi: 3.0.0
info:
  title: Test API with Parameter References
  version: 1.0.0
servers:
  - url: https://api.example.com
components:
  parameters:
    userId:
      name: userId
      in: path
      required: true
      description: Unique identifier of the user
      schema:
        type: string
        format: uuid
    limit:
      name: limit
      in: query
      required: false
      description: Maximum number of items to return
      schema:
        type: integer
        default: 10
        minimum: 1
        maximum: 100
paths:
  /users/{userId}:
    get:
      operationId: getUserById
      summary: Get user by ID
      tags:
        - users
      parameters:
        - $ref: '#/components/parameters/userId'
      responses:
        '200':
          description: User found
          content:
            application/json:
              schema:
                type: object
                properties:
                  id:
                    type: string
                  name:
                    type: string
  /users:
    get:
      operationId: getUsers
      summary: List all users
      tags:
        - users
      parameters:
        - $ref: '#/components/parameters/limit'
      responses:
        '200':
          description: List of users
          content:
            application/json:
              schema:
                type: array
                items:
                  type: object
                  properties:
                    id:
                      type: string
                    name:
                      type: string
";

    let spec_path = fs.write_spec(spec_with_refs);
    let config_dir = fs.config_dir();
    let cache_file = config_dir.join(".cache").join("test-api.bin");
    let config_manager = ConfigManager::with_fs(fs, config_dir);

    // Should successfully add the spec with parameter references
    let result = config_manager.add_spec(&name("test-api"), &spec_path, false, true);
    assert!(
        result.is_ok(),
        "Should successfully add spec with parameter references: {:?}",
        result.err()
    );

    // Verify the spec was cached
    assert!(cache_file.exists(), "Cache file should exist");

    // Load and verify the cached spec
    let cached_content = std::fs::read(&cache_file).unwrap();
    let cached_spec: aperture_cli::cache::models::CachedSpec =
        bincode::deserialize(&cached_content).unwrap();

    // Verify commands were created with resolved parameters
    assert_eq!(cached_spec.commands.len(), 2);

    // Check getUserById command
    let get_user_cmd = cached_spec
        .commands
        .iter()
        .find(|c| c.operation_id == "getUserById")
        .expect("getUserById command should exist");

    assert_eq!(get_user_cmd.parameters.len(), 1);
    let user_id_param = &get_user_cmd.parameters[0];
    assert_eq!(user_id_param.name, "userId");
    assert_eq!(user_id_param.location, "path");
    assert!(user_id_param.required);
    assert_eq!(
        user_id_param.description,
        Some("Unique identifier of the user".to_string())
    );

    // Check getUsers command
    let get_users_cmd = cached_spec
        .commands
        .iter()
        .find(|c| c.operation_id == "getUsers")
        .expect("getUsers command should exist");

    assert_eq!(get_users_cmd.parameters.len(), 1);
    let limit_param = &get_users_cmd.parameters[0];
    assert_eq!(limit_param.name, "limit");
    assert_eq!(limit_param.location, "query");
    assert!(!limit_param.required);
    assert_eq!(
        limit_param.description,
        Some("Maximum number of items to return".to_string())
    );
    assert_eq!(limit_param.default_value, Some("10".to_string()));
}

#[test]
fn test_parameter_references_with_special_characters() {
    let fs = TestFS::new();
    let spec_with_special_chars = r"
openapi: 3.0.0
info:
  title: Test API with Special Parameter Names
  version: 1.0.0
servers:
  - url: https://api.example.com
components:
  parameters:
    user-id:
      name: user-id
      in: path
      required: true
      description: User identifier with hyphen
      schema:
        type: string
    user.name:
      name: user.name
      in: query
      required: false
      description: User name with dot
      schema:
        type: string
    user_email:
      name: user_email
      in: query
      required: false
      description: User email with underscore
      schema:
        type: string
        format: email
    search%20query:
      name: search query
      in: query
      required: false
      description: Search query with URL encoded name
      schema:
        type: string
paths:
  /users/{user-id}:
    get:
      operationId: getUserWithSpecialParams
      tags:
        - users
      parameters:
        - $ref: '#/components/parameters/user-id'
        - $ref: '#/components/parameters/user.name'
        - $ref: '#/components/parameters/user_email'
        - $ref: '#/components/parameters/search%20query'
      responses:
        '200':
          description: User details
          content:
            application/json:
              schema:
                type: object
";

    let spec_path = fs.write_spec(spec_with_special_chars);
    let config_dir = fs.config_dir();
    let cache_file = config_dir.join(".cache").join("test-special-api.bin");
    let config_manager = ConfigManager::with_fs(fs, config_dir);

    // Should successfully add the spec with special parameter names
    let result = config_manager.add_spec(&name("test-special-api"), &spec_path, false, true);
    assert!(
        result.is_ok(),
        "Should successfully add spec with special parameter names: {:?}",
        result.err()
    );

    // Verify the spec was cached
    assert!(cache_file.exists(), "Cache file should exist");

    // Load and verify the cached spec
    let cached_content = std::fs::read(&cache_file).unwrap();
    let cached_spec: aperture_cli::cache::models::CachedSpec =
        bincode::deserialize(&cached_content).unwrap();

    // Verify command was created with all special parameters
    assert_eq!(cached_spec.commands.len(), 1);
    let cmd = &cached_spec.commands[0];
    assert_eq!(cmd.operation_id, "getUserWithSpecialParams");
    assert_eq!(cmd.parameters.len(), 4);

    // Check parameters were resolved correctly
    let param_names: Vec<&str> = cmd.parameters.iter().map(|p| p.name.as_str()).collect();

    assert!(
        param_names.contains(&"user-id"),
        "Should contain user-id parameter"
    );
    assert!(
        param_names.contains(&"user.name"),
        "Should contain user.name parameter"
    );
    assert!(
        param_names.contains(&"user_email"),
        "Should contain user_email parameter"
    );
    assert!(
        param_names.contains(&"search query"),
        "Should contain search query parameter"
    );

    // Verify parameter details
    let user_id_param = cmd.parameters.iter().find(|p| p.name == "user-id").unwrap();
    assert_eq!(user_id_param.location, "path");
    assert!(user_id_param.required);
    assert_eq!(
        user_id_param.description,
        Some("User identifier with hyphen".to_string())
    );

    let search_param = cmd
        .parameters
        .iter()
        .find(|p| p.name == "search query")
        .unwrap();
    assert_eq!(search_param.location, "query");
    assert!(!search_param.required);
    assert_eq!(
        search_param.description,
        Some("Search query with URL encoded name".to_string())
    );
}

#[test]
fn test_cli_with_parameter_references() {
    let temp_dir = TempDir::new().unwrap();
    let config_dir = temp_dir.path().join(".aperture");
    let spec_path = temp_dir.path().join("api-with-refs.yaml");

    // Write OpenAPI spec with parameter references
    fs::write(
        &spec_path,
        r"
openapi: 3.0.0
info:
  title: Test API
  version: 1.0.0
servers:
  - url: https://api.example.com
components:
  parameters:
    petId:
      name: petId
      in: path
      required: true
      description: ID of the pet
      schema:
        type: integer
paths:
  /pets/{petId}:
    get:
      operationId: getPetById
      tags:
        - pets
      parameters:
        - $ref: '#/components/parameters/petId'
      responses:
        '200':
          description: Pet details
",
    )
    .unwrap();

    // Add the spec using the CLI
    let output = aperture_cmd()
        .env("APERTURE_CONFIG_DIR", config_dir.to_str().unwrap())
        .args(["config", "add", "test-api", spec_path.to_str().unwrap()])
        .output()
        .unwrap();

    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);

    assert!(
        output.status.success(),
        "Command should succeed. stdout: {stdout}, stderr: {stderr}"
    );

    // Verify the spec was added
    aperture_cmd()
        .env("APERTURE_CONFIG_DIR", config_dir.to_str().unwrap())
        .args(["config", "list"])
        .assert()
        .success()
        .stdout(predicate::str::contains("test-api"));

    // Verify we can use the generated command with the resolved parameter
    // Check if the command structure is correct by looking at the error output
    let output = aperture_cmd()
        .env("APERTURE_CONFIG_DIR", config_dir.to_str().unwrap())
        .args(["api", "test-api", "pets", "get-pet-by-id", "--help"])
        .output()
        .unwrap();

    // The command might be showing help on stderr with exit code 1
    let stderr = String::from_utf8_lossy(&output.stderr);
    let stdout = String::from_utf8_lossy(&output.stdout);

    // Check that parameter was resolved correctly
    assert!(
        stdout.contains("--pet-id") || stderr.contains("--pet-id"),
        "Output should contain --pet-id parameter. stdout: {stdout}, stderr: {stderr}"
    );
    assert!(
        stdout.contains("ID of the pet")
            || stderr.contains("ID of the pet")
            || stdout.contains("Path parameter: petId")
            || stderr.contains("Path parameter: petId"),
        "Output should contain parameter description. stdout: {stdout}, stderr: {stderr}"
    );
}

#[test]
fn test_invalid_parameter_reference_rejected() {
    let fs = TestFS::new();
    let invalid_spec = r"
openapi: 3.0.0
info:
  title: Test API
  version: 1.0.0
servers:
  - url: https://api.example.com
paths:
  /users/{userId}:
    get:
      operationId: getUserById
      parameters:
        - $ref: '#/components/parameters/nonExistentParam'
      responses:
        '200':
          description: Success
";

    let spec_path = fs.write_spec(invalid_spec);
    let config_dir = fs.config_dir();
    let config_manager = ConfigManager::with_fs(fs, config_dir);

    // Should fail to add the spec with invalid reference
    let result = config_manager.add_spec(&name("test-api"), &spec_path, false, true);
    assert!(
        result.is_err(),
        "Should fail to add spec with invalid parameter reference"
    );

    match result.unwrap_err() {
        aperture_cli::error::Error::Internal {
            kind: aperture_cli::error::ErrorKind::Validation,
            message: msg,
            ..
        } => {
            assert!(
                msg.contains("not found in components") || msg.contains("no components section"),
                "Error should mention missing parameter. Actual error: {msg}"
            );
        }
        _ => panic!("Expected Validation error"),
    }
}

#[test]
fn test_cli_with_special_character_parameters() {
    let temp_dir = TempDir::new().unwrap();
    let config_dir = temp_dir.path().join(".aperture");
    let spec_path = temp_dir.path().join("api-special-chars.yaml");

    // Write OpenAPI spec with special character parameters
    fs::write(
        &spec_path,
        r"
openapi: 3.0.0
info:
  title: Test API Special Chars
  version: 1.0.0
servers:
  - url: https://api.example.com
components:
  parameters:
    user-id:
      name: user-id
      in: path
      required: true
      schema:
        type: string
    include.fields:
      name: include.fields
      in: query
      required: false
      schema:
        type: string
paths:
  /users/{user-id}:
    get:
      operationId: getUser
      tags:
        - users
      parameters:
        - $ref: '#/components/parameters/user-id'
        - $ref: '#/components/parameters/include.fields'
      responses:
        '200':
          description: User details
",
    )
    .unwrap();

    // Add the spec
    let mut cmd = aperture_cmd();
    cmd.env("APERTURE_CONFIG_DIR", config_dir.to_str().unwrap())
        .args(["config", "add", "special-api", spec_path.to_str().unwrap()])
        .assert()
        .success();

    // Test the help output shows the special parameters correctly
    let mut cmd = aperture_cmd();
    let output = cmd
        .env("APERTURE_CONFIG_DIR", config_dir.to_str().unwrap())
        .args(["api", "special-api", "users", "get-user", "--help"])
        .output()
        .unwrap();

    // Help might be in stdout or stderr
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    let combined = format!("{stdout}{stderr}");

    assert!(
        combined.contains("--user-id") && combined.contains("--include-fields"),
        "Help should show both special character parameters"
    );

    // Test dry-run with special character parameters
    let mut cmd = aperture_cmd();
    let output = cmd
        .env("APERTURE_CONFIG_DIR", config_dir.to_str().unwrap())
        .args([
            "api",
            "special-api",
            "--dry-run",
            "users",
            "get-user",
            "--user-id",
            "123",
            "--include-fields",
            "name,email",
        ])
        .output()
        .unwrap();

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        panic!("Command failed with stderr: {stderr}");
    }

    let stdout = String::from_utf8(output.stdout).unwrap();

    // The URL might have the parameter in the path
    assert!(
        stdout.contains("https://api.example.com/users/123"),
        "Should contain the URL with path parameter. Actual output: {stdout}"
    );

    // Query parameters might be shown separately or URL-encoded
    assert!(
        stdout.contains("include.fields") || stdout.contains("include%2Efields"),
        "Should contain the query parameter. Actual output: {stdout}"
    );
}
