// These lints are overly pedantic for test code
#![allow(clippy::default_trait_access)]
#![allow(clippy::field_reassign_with_default)]
#![allow(clippy::too_many_lines)]
#![allow(clippy::significant_drop_tightening)]
#![allow(clippy::used_underscore_binding)]

mod test_helpers;

use aperture_cli::cache::models::CachedApertureSecret;
use aperture_cli::cli::OutputFormat;
use aperture_cli::config::context_name::ApiContextName;
use aperture_cli::config::manager::ConfigManager;

/// Helper to create a validated `ApiContextName` from a string literal in tests
fn name(s: &str) -> ApiContextName {
    ApiContextName::new(s).expect("test name should be valid")
}
use aperture_cli::constants;
use aperture_cli::engine::executor::execute_request;
use aperture_cli::engine::loader::load_cached_spec;
use aperture_cli::fs::OsFileSystem;
use clap::{Arg, Command};
use std::path::Path;
use tempfile::TempDir;
use wiremock::matchers::{header, method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

fn create_temp_config_manager() -> (ConfigManager<OsFileSystem>, TempDir) {
    let temp_dir = TempDir::new().expect("Failed to create temp directory");
    let config_manager = ConfigManager::with_fs(OsFileSystem, temp_dir.path().to_path_buf());
    (config_manager, temp_dir)
}

#[tokio::test]
async fn test_bearer_auth_extension_parsing_from_yaml() {
    let (config_manager, _temp_dir) = create_temp_config_manager();

    // Add spec with x-aperture-secret extension
    let spec_path = Path::new("tests/fixtures/openapi/bearer-auth-with-extension.yaml");
    config_manager
        .add_spec(&name("bearer-test"), spec_path, false, true)
        .unwrap();

    // Load the cached spec and verify extension parsing
    let cache_dir = _temp_dir.path().join(".cache");
    let cached_spec = load_cached_spec(&cache_dir, "bearer-test").unwrap();

    // Verify the security scheme was parsed with the extension
    assert!(cached_spec.security_schemes.contains_key("bearerAuth"));
    let bearer_scheme = &cached_spec.security_schemes["bearerAuth"];

    assert_eq!(bearer_scheme.scheme_type, "http");
    assert_eq!(
        bearer_scheme.scheme,
        Some(constants::AUTH_SCHEME_BEARER.to_string())
    );
    assert_eq!(bearer_scheme.location, Some("header".to_string()));
    assert_eq!(
        bearer_scheme.parameter_name,
        Some(constants::HEADER_AUTHORIZATION.to_string())
    );

    // Most importantly, verify the x-aperture-secret extension was parsed
    assert!(bearer_scheme.aperture_secret.is_some());
    let aperture_secret = bearer_scheme.aperture_secret.as_ref().unwrap();
    assert_eq!(aperture_secret.source, "env");
    assert_eq!(aperture_secret.name, "TEST_BEARER_TOKEN");

    // Verify security requirements are properly extracted
    let user_command = cached_spec
        .commands
        .iter()
        .find(|cmd| cmd.operation_id == "getUserById")
        .expect("getUserById command not found");
    assert_eq!(user_command.security_requirements, vec!["bearerAuth"]);
}

#[tokio::test]
async fn test_api_key_extension_parsing_from_yaml() {
    let (config_manager, _temp_dir) = create_temp_config_manager();

    // Add spec with x-aperture-secret extension
    let spec_path = Path::new("tests/fixtures/openapi/api-key-with-extension.yaml");
    config_manager
        .add_spec(&name("apikey-test"), spec_path, false, true)
        .unwrap();

    // Load the cached spec and verify extension parsing
    let cache_dir = _temp_dir.path().join(".cache");
    let cached_spec = load_cached_spec(&cache_dir, "apikey-test").unwrap();

    // Verify the security scheme was parsed with the extension
    assert!(cached_spec.security_schemes.contains_key("apiKeyAuth"));
    let api_key_scheme = &cached_spec.security_schemes["apiKeyAuth"];

    assert_eq!(api_key_scheme.scheme_type, "apiKey");
    assert_eq!(api_key_scheme.scheme, None);
    assert_eq!(api_key_scheme.location, Some("header".to_string()));
    assert_eq!(api_key_scheme.parameter_name, Some("X-API-Key".to_string()));

    // Most importantly, verify the x-aperture-secret extension was parsed
    assert!(api_key_scheme.aperture_secret.is_some());
    let aperture_secret = api_key_scheme.aperture_secret.as_ref().unwrap();
    assert_eq!(aperture_secret.source, "env");
    assert_eq!(aperture_secret.name, "TEST_API_KEY");

    // Verify security requirements are properly extracted
    let data_command = cached_spec
        .commands
        .iter()
        .find(|cmd| cmd.operation_id == "getData")
        .expect("getData command not found");
    assert_eq!(data_command.security_requirements, vec!["apiKeyAuth"]);
}

#[tokio::test]
async fn test_multiple_schemes_extension_parsing_from_json() {
    let (config_manager, _temp_dir) = create_temp_config_manager();

    // Add spec with multiple x-aperture-secret extensions
    let spec_path = Path::new("tests/fixtures/openapi/multiple-schemes-with-extensions.json");
    config_manager
        .add_spec(&name("multi-test"), spec_path, false, true)
        .unwrap();

    // Load the cached spec and verify extension parsing
    let cache_dir = _temp_dir.path().join(".cache");
    let cached_spec = load_cached_spec(&cache_dir, "multi-test").unwrap();

    // Verify both security schemes were parsed with extensions
    assert_eq!(cached_spec.security_schemes.len(), 2);

    // Check Bearer auth
    let bearer_scheme = &cached_spec.security_schemes["bearerAuth"];
    assert_eq!(bearer_scheme.scheme_type, "http");
    assert_eq!(
        bearer_scheme.scheme,
        Some(constants::AUTH_SCHEME_BEARER.to_string())
    );
    let bearer_secret = bearer_scheme.aperture_secret.as_ref().unwrap();
    assert_eq!(bearer_secret.name, "MULTI_BEARER_TOKEN");

    // Check API key auth
    let api_key_scheme = &cached_spec.security_schemes["apiKeyAuth"];
    assert_eq!(api_key_scheme.scheme_type, "apiKey");
    assert_eq!(api_key_scheme.parameter_name, Some("X-API-Key".to_string()));
    let api_key_secret = api_key_scheme.aperture_secret.as_ref().unwrap();
    assert_eq!(api_key_secret.name, "MULTI_API_KEY");

    // Verify security requirements for different operations
    let bearer_command = cached_spec
        .commands
        .iter()
        .find(|cmd| cmd.operation_id == "getBearerProtected")
        .expect("getBearerProtected command not found");
    assert_eq!(bearer_command.security_requirements, vec!["bearerAuth"]);

    let api_key_command = cached_spec
        .commands
        .iter()
        .find(|cmd| cmd.operation_id == "getApiKeyProtected")
        .expect("getApiKeyProtected command not found");
    assert_eq!(api_key_command.security_requirements, vec!["apiKeyAuth"]);
}

#[tokio::test]
async fn test_end_to_end_authentication_with_parsed_extensions() {
    let mock_server = MockServer::start().await;
    let (config_manager, _temp_dir) = create_temp_config_manager();

    // Set up environment variable for authentication
    let env_var = "E2E_PARSED_BEARER_TOKEN";
    std::env::set_var(env_var, "parsed-extension-token");

    // Add spec with x-aperture-secret extension
    let spec_path = Path::new("tests/fixtures/openapi/bearer-auth-with-extension.yaml");
    config_manager
        .add_spec(&name("e2e-test"), spec_path, false, true)
        .unwrap();

    // Load the cached spec
    let cache_dir = _temp_dir.path().join(".cache");
    let cached_spec = load_cached_spec(&cache_dir, "e2e-test").unwrap();

    // Update the cached spec to use our test environment variable
    let mut updated_spec = cached_spec;
    if let Some(bearer_scheme) = updated_spec.security_schemes.get_mut("bearerAuth") {
        bearer_scheme.aperture_secret = Some(CachedApertureSecret {
            source: "env".to_string(),
            name: env_var.to_string(),
        });
    }

    // Configure mock to expect the Bearer token from environment variable
    Mock::given(method("GET"))
        .and(path("/users/456"))
        .and(header("Authorization", "Bearer parsed-extension-token"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "id": "456",
            "name": "Extension Parsed User"
        })))
        .expect(1)
        .mount(&mock_server)
        .await;

    // Create command structure for testing
    let command = Command::new("api").subcommand(
        Command::new("users")
            .subcommand(Command::new("get-user-by-id").arg(Arg::new("id").required(true))),
    );

    let matches = command.get_matches_from(vec!["api", "users", "get-user-by-id", "456"]);

    // Execute request using parsed extension data
    let result = execute_request(
        &updated_spec,
        &matches,
        Some(&mock_server.uri()),
        false,
        None,
        None,
        &OutputFormat::Json,
        None,
        None,  // cache_config
        false, // capture_output
        None,  // retry_context
    )
    .await;
    assert!(result.is_ok());

    // Clean up
    std::env::remove_var(env_var);
}

#[tokio::test]
async fn test_missing_extension_graceful_handling() {
    let (config_manager, _temp_dir) = create_temp_config_manager();

    // Create a temporary spec file without x-aperture-secret extension
    let temp_spec = r"
openapi: 3.0.0
info:
  title: No Extension API
  version: 1.0.0
components:
  securitySchemes:
    bearerAuth:
      type: http
      scheme: bearer
      description: Bearer token without extension
paths:
  /test:
    get:
      operationId: getTest
      tags: [test]
      security:
        - bearerAuth: []
      responses:
        '200':
          description: Success
servers:
  - url: https://api.example.com
";

    let temp_file = tempfile::NamedTempFile::new().unwrap();
    std::fs::write(temp_file.path(), temp_spec).unwrap();

    // Add spec without extension
    config_manager
        .add_spec(&name("no-extension"), temp_file.path(), false, true)
        .unwrap();

    // Load and verify the spec handles missing extensions gracefully
    let cache_dir = _temp_dir.path().join(".cache");
    let cached_spec = load_cached_spec(&cache_dir, "no-extension").unwrap();

    assert!(cached_spec.security_schemes.contains_key("bearerAuth"));
    let bearer_scheme = &cached_spec.security_schemes["bearerAuth"];

    // Verify scheme details are correct
    assert_eq!(bearer_scheme.scheme_type, "http");
    assert_eq!(
        bearer_scheme.scheme,
        Some(constants::AUTH_SCHEME_BEARER.to_string())
    );

    // Verify no extension was parsed (graceful handling)
    assert!(bearer_scheme.aperture_secret.is_none());
}

#[tokio::test]
async fn test_malformed_extension_graceful_handling() {
    let (config_manager, _temp_dir) = create_temp_config_manager();

    // Create a temporary spec file with malformed x-aperture-secret extension
    let temp_spec = r#"
openapi: 3.0.0
info:
  title: Malformed Extension API
  version: 1.0.0
components:
  securitySchemes:
    bearerAuth:
      type: http
      scheme: bearer
      description: Bearer token with malformed extension
      x-aperture-secret: "this should be an object, not a string"
paths:
  /test:
    get:
      operationId: getTest
      tags: [test]
      security:
        - bearerAuth: []
      responses:
        '200':
          description: Success
servers:
  - url: https://api.example.com
"#;

    let temp_file = tempfile::NamedTempFile::new().unwrap();
    std::fs::write(temp_file.path(), temp_spec).unwrap();

    // Add spec with malformed extension - should now fail with validation error
    let result =
        config_manager.add_spec(&name("malformed-extension"), temp_file.path(), false, true);

    // Verify that the malformed extension is caught during validation
    assert!(result.is_err());
    let error = result.unwrap_err();
    let error_msg = error.to_string();
    assert!(
        error_msg.contains("must be an object"),
        "Expected error about x-aperture-secret needing to be an object, got: {error_msg}"
    );
}
