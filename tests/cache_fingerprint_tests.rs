#![cfg(feature = "integration")]

mod common;
mod test_helpers;

use common::aperture_cmd;
use predicates::prelude::*;
use std::fs;
use tempfile::TempDir;

/// Minimal `OpenAPI` spec for testing
const fn minimal_spec() -> &'static str {
    "openapi: 3.0.0
info:
  title: Fingerprint Test API
  version: 1.0.0
servers:
  - url: https://api.example.com
paths:
  /users:
    get:
      tags:
        - users
      operationId: listUsers
      summary: List users
      responses:
        '200':
          description: Success
          content:
            application/json:
              schema:
                type: object
"
}

/// Modified spec with an additional endpoint
const fn modified_spec() -> &'static str {
    "openapi: 3.0.0
info:
  title: Fingerprint Test API
  version: 2.0.0
servers:
  - url: https://api.example.com
paths:
  /users:
    get:
      tags:
        - users
      operationId: listUsers
      summary: List users
      responses:
        '200':
          description: Success
          content:
            application/json:
              schema:
                type: object
  /health:
    get:
      tags:
        - health
      operationId: healthCheck
      summary: Health check
      responses:
        '200':
          description: OK
          content:
            application/json:
              schema:
                type: object
"
}

#[test]
fn test_unchanged_spec_loads_successfully() {
    let temp_dir = TempDir::new().unwrap();
    let config_dir = temp_dir.path().to_path_buf();
    let spec_file = temp_dir.path().join("test-spec.yaml");

    // Write and add the spec
    fs::write(&spec_file, minimal_spec()).unwrap();

    let add_output = aperture_cmd()
        .env("APERTURE_CONFIG_DIR", config_dir.to_str().unwrap())
        .args(["config", "add", "fp-test", spec_file.to_str().unwrap()])
        .output()
        .unwrap();
    assert!(add_output.status.success(), "Failed to add spec");

    // Loading the spec (via list-commands) should succeed without modification
    aperture_cmd()
        .env("APERTURE_CONFIG_DIR", config_dir.to_str().unwrap())
        .args(["list-commands", "fp-test"])
        .assert()
        .success();
}

#[test]
fn test_modified_spec_triggers_stale_cache_error() {
    let temp_dir = TempDir::new().unwrap();
    let config_dir = temp_dir.path().to_path_buf();
    let spec_file = temp_dir.path().join("test-spec.yaml");

    // Write and add the spec
    fs::write(&spec_file, minimal_spec()).unwrap();

    let add_output = aperture_cmd()
        .env("APERTURE_CONFIG_DIR", config_dir.to_str().unwrap())
        .args(["config", "add", "fp-test", spec_file.to_str().unwrap()])
        .output()
        .unwrap();
    assert!(add_output.status.success(), "Failed to add spec");

    // Now modify the spec file in the config directory (simulating a manual edit)
    let stored_spec_path = config_dir.join("specs/fp-test.yaml");
    assert!(stored_spec_path.exists(), "Stored spec should exist");

    // Small delay to ensure different mtime
    std::thread::sleep(std::time::Duration::from_millis(1100));
    fs::write(&stored_spec_path, modified_spec()).unwrap();

    // Trying to use the API should detect stale cache
    aperture_cmd()
        .env("APERTURE_CONFIG_DIR", config_dir.to_str().unwrap())
        .args(["list-commands", "fp-test"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("stale").or(predicate::str::contains("modified")));
}

#[test]
fn test_reinit_fixes_stale_cache() {
    let temp_dir = TempDir::new().unwrap();
    let config_dir = temp_dir.path().to_path_buf();
    let spec_file = temp_dir.path().join("test-spec.yaml");

    // Write and add the spec
    fs::write(&spec_file, minimal_spec()).unwrap();

    let add_output = aperture_cmd()
        .env("APERTURE_CONFIG_DIR", config_dir.to_str().unwrap())
        .args(["config", "add", "fp-test", spec_file.to_str().unwrap()])
        .output()
        .unwrap();
    assert!(add_output.status.success(), "Failed to add spec");

    // Modify the stored spec file
    let stored_spec_path = config_dir.join("specs/fp-test.yaml");
    std::thread::sleep(std::time::Duration::from_millis(1100));
    fs::write(&stored_spec_path, modified_spec()).unwrap();

    // Verify it's stale
    aperture_cmd()
        .env("APERTURE_CONFIG_DIR", config_dir.to_str().unwrap())
        .args(["list-commands", "fp-test"])
        .assert()
        .failure();

    // Reinit should fix the cache
    aperture_cmd()
        .env("APERTURE_CONFIG_DIR", config_dir.to_str().unwrap())
        .args(["config", "reinit", "fp-test"])
        .assert()
        .success();

    // Now loading should succeed
    aperture_cmd()
        .env("APERTURE_CONFIG_DIR", config_dir.to_str().unwrap())
        .args(["list-commands", "fp-test"])
        .assert()
        .success();
}

#[test]
fn test_force_add_updates_fingerprint() {
    let temp_dir = TempDir::new().unwrap();
    let config_dir = temp_dir.path().to_path_buf();
    let spec_file = temp_dir.path().join("test-spec.yaml");

    // Write and add the original spec
    fs::write(&spec_file, minimal_spec()).unwrap();

    aperture_cmd()
        .env("APERTURE_CONFIG_DIR", config_dir.to_str().unwrap())
        .args(["config", "add", "fp-test", spec_file.to_str().unwrap()])
        .assert()
        .success();

    // Write the modified spec to the source file
    fs::write(&spec_file, modified_spec()).unwrap();

    // Re-add with --force should update the fingerprint
    aperture_cmd()
        .env("APERTURE_CONFIG_DIR", config_dir.to_str().unwrap())
        .args([
            "config",
            "add",
            "fp-test",
            spec_file.to_str().unwrap(),
            "--force",
        ])
        .assert()
        .success();

    // Loading should succeed with updated spec
    aperture_cmd()
        .env("APERTURE_CONFIG_DIR", config_dir.to_str().unwrap())
        .args(["list-commands", "fp-test"])
        .assert()
        .success();
}

#[test]
fn test_metadata_backward_compatibility() {
    let temp_dir = TempDir::new().unwrap();
    let config_dir = temp_dir.path().to_path_buf();
    let spec_file = temp_dir.path().join("test-spec.yaml");

    // Write and add the spec
    fs::write(&spec_file, minimal_spec()).unwrap();

    aperture_cmd()
        .env("APERTURE_CONFIG_DIR", config_dir.to_str().unwrap())
        .args(["config", "add", "fp-test", spec_file.to_str().unwrap()])
        .assert()
        .success();

    // Manually strip fingerprint fields from metadata to simulate legacy format
    let metadata_path = config_dir.join(".cache/cache_metadata.json");
    let metadata_content = fs::read_to_string(&metadata_path).unwrap();
    let mut metadata: serde_json::Value = serde_json::from_str(&metadata_content).unwrap();

    // Remove fingerprint fields from the spec metadata
    if let Some(spec) = metadata
        .get_mut("specs")
        .and_then(|s| s.get_mut("fp-test"))
        .and_then(|s| s.as_object_mut())
    {
        spec.remove("content_hash");
        spec.remove("mtime_secs");
        spec.remove("spec_file_size");
    }
    fs::write(
        &metadata_path,
        serde_json::to_string_pretty(&metadata).unwrap(),
    )
    .unwrap();

    // Loading should still succeed with legacy metadata (no fingerprint = no opinion)
    aperture_cmd()
        .env("APERTURE_CONFIG_DIR", config_dir.to_str().unwrap())
        .args(["list-commands", "fp-test"])
        .assert()
        .success();
}
