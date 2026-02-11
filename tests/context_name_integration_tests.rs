#![cfg(feature = "integration")]

mod common;

use common::aperture_cmd;
use predicates::prelude::*;
use std::path::PathBuf;

/// Helper to set up a temporary config directory for test isolation
fn setup_temp_config_dir(test_name: &str) -> PathBuf {
    let mut path = std::env::temp_dir();
    path.push("aperture_context_name_test");
    path.push(test_name);
    let _ = std::fs::remove_dir_all(&path);
    std::fs::create_dir_all(&path).unwrap();
    path
}

// ---- Tests for `config add` with invalid names ----

#[test]
fn test_config_add_rejects_path_traversal() {
    let config_dir = setup_temp_config_dir("add_traversal");
    aperture_cmd()
        .env("APERTURE_CONFIG_DIR", config_dir.as_os_str())
        .args(["config", "add", "../evil", "/dev/null"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("Invalid API context name"));
}

#[test]
fn test_config_add_rejects_hidden_name() {
    let config_dir = setup_temp_config_dir("add_hidden");
    aperture_cmd()
        .env("APERTURE_CONFIG_DIR", config_dir.as_os_str())
        .args(["config", "add", ".hidden", "/dev/null"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("Invalid API context name"));
}

#[test]
fn test_config_add_rejects_slash_in_name() {
    let config_dir = setup_temp_config_dir("add_slash");
    aperture_cmd()
        .env("APERTURE_CONFIG_DIR", config_dir.as_os_str())
        .args(["config", "add", "foo/bar", "/dev/null"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("Invalid API context name"))
        .stderr(predicate::str::contains("invalid character '/'"));
}

// ---- Tests for `config remove` with invalid names ----

#[test]
fn test_config_remove_rejects_path_traversal() {
    let config_dir = setup_temp_config_dir("remove_traversal");
    aperture_cmd()
        .env("APERTURE_CONFIG_DIR", config_dir.as_os_str())
        .args(["config", "remove", "../traversal"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("Invalid API context name"));
}

// ---- Tests for `api` with invalid context ----

#[test]
fn test_api_rejects_path_traversal_context() {
    let config_dir = setup_temp_config_dir("api_traversal");
    aperture_cmd()
        .env("APERTURE_CONFIG_DIR", config_dir.as_os_str())
        .args(["api", "../../../etc/passwd"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("Invalid API context name"));
}

// ---- Test that valid names pass validation ----

#[test]
fn test_config_add_valid_name_passes_validation() {
    let config_dir = setup_temp_config_dir("add_valid");
    // The name is valid, so it should pass name validation.
    // It will fail later because /dev/null is not a valid OpenAPI spec,
    // but the error should NOT be about the name.
    let result = aperture_cmd()
        .env("APERTURE_CONFIG_DIR", config_dir.as_os_str())
        .args(["config", "add", "my-valid-api", "/dev/null"])
        .assert()
        .failure();
    result.stderr(predicate::str::contains("Invalid API context name").not());
}

// ---- Test error message quality ----

#[test]
fn test_error_includes_hint() {
    let config_dir = setup_temp_config_dir("error_hint");
    aperture_cmd()
        .env("APERTURE_CONFIG_DIR", config_dir.as_os_str())
        .args(["config", "add", "../evil", "/dev/null"])
        .assert()
        .failure()
        .stderr(predicate::str::contains(
            "API names must start with a letter or digit",
        ));
}

// ---- JSON errors mode ----

#[test]
fn test_json_errors_mode_with_invalid_name() {
    let config_dir = setup_temp_config_dir("json_errors");
    aperture_cmd()
        .env("APERTURE_CONFIG_DIR", config_dir.as_os_str())
        .args(["--json-errors", "config", "add", "../evil", "/dev/null"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("Invalid API context name"));
}
