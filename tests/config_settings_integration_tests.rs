#![cfg(feature = "integration")]
// These lints are overly pedantic for integration tests
#![allow(clippy::too_many_lines)]

mod common;

use common::aperture_cmd;
use predicates::prelude::*;
use tempfile::TempDir;

/// Test `aperture config settings` lists all available settings
#[test]
fn test_config_settings_lists_all() {
    let temp_dir = TempDir::new().unwrap();

    aperture_cmd()
        .env("APERTURE_CONFIG_DIR", temp_dir.path())
        .args(["config", "settings"])
        .assert()
        .success()
        .stdout(predicate::str::contains("default_timeout_secs"))
        .stdout(predicate::str::contains("agent_defaults.json_errors"))
        .stdout(predicate::str::contains("Type: integer"))
        .stdout(predicate::str::contains("Type: boolean"));
}

/// Test `aperture config settings --json` outputs valid JSON
#[test]
fn test_config_settings_json_output() {
    let temp_dir = TempDir::new().unwrap();

    let output = aperture_cmd()
        .env("APERTURE_CONFIG_DIR", temp_dir.path())
        .args(["config", "settings", "--json"])
        .assert()
        .success();

    let stdout = String::from_utf8_lossy(&output.get_output().stdout);
    let parsed: serde_json::Value = serde_json::from_str(&stdout).expect("Should be valid JSON");

    // Should be an array
    assert!(parsed.is_array());
    let settings = parsed.as_array().unwrap();

    // Should have 2 settings
    assert_eq!(settings.len(), 2);

    // Check structure of first setting
    let first = &settings[0];
    assert!(first.get("key").is_some());
    assert!(first.get("value").is_some());
    assert!(first.get("type").is_some());
    assert!(first.get("description").is_some());
    assert!(first.get("default").is_some());
}

/// Test `aperture config get` returns default value
#[test]
fn test_config_get_default_value() {
    let temp_dir = TempDir::new().unwrap();

    aperture_cmd()
        .env("APERTURE_CONFIG_DIR", temp_dir.path())
        .args(["config", "get", "default_timeout_secs"])
        .assert()
        .success()
        .stdout(predicate::str::contains("30"));
}

/// Test `aperture config get --json` outputs JSON
#[test]
fn test_config_get_json_output() {
    let temp_dir = TempDir::new().unwrap();

    let output = aperture_cmd()
        .env("APERTURE_CONFIG_DIR", temp_dir.path())
        .args(["config", "get", "default_timeout_secs", "--json"])
        .assert()
        .success();

    let stdout = String::from_utf8_lossy(&output.get_output().stdout);
    let parsed: serde_json::Value = serde_json::from_str(&stdout).expect("Should be valid JSON");

    assert_eq!(parsed["key"], "default_timeout_secs");
    assert_eq!(parsed["value"], "30");
}

/// Test `aperture config set` and `config get` roundtrip
#[test]
fn test_config_set_get_roundtrip() {
    let temp_dir = TempDir::new().unwrap();

    // Set a new timeout value
    aperture_cmd()
        .env("APERTURE_CONFIG_DIR", temp_dir.path())
        .args(["config", "set", "default_timeout_secs", "120"])
        .assert()
        .success()
        .stdout(predicate::str::contains("Set default_timeout_secs = 120"));

    // Verify the value was set
    aperture_cmd()
        .env("APERTURE_CONFIG_DIR", temp_dir.path())
        .args(["config", "get", "default_timeout_secs"])
        .assert()
        .success()
        .stdout(predicate::str::contains("120"));
}

/// Test `aperture config set` with nested key
#[test]
fn test_config_set_nested_key() {
    let temp_dir = TempDir::new().unwrap();

    // Set json_errors to true
    aperture_cmd()
        .env("APERTURE_CONFIG_DIR", temp_dir.path())
        .args(["config", "set", "agent_defaults.json_errors", "true"])
        .assert()
        .success()
        .stdout(predicate::str::contains(
            "Set agent_defaults.json_errors = true",
        ));

    // Verify the value was set
    aperture_cmd()
        .env("APERTURE_CONFIG_DIR", temp_dir.path())
        .args(["config", "get", "agent_defaults.json_errors"])
        .assert()
        .success()
        .stdout(predicate::str::contains("true"));
}

/// Test `aperture config get` with invalid key shows helpful error
#[test]
fn test_config_get_invalid_key_error() {
    let temp_dir = TempDir::new().unwrap();

    aperture_cmd()
        .env("APERTURE_CONFIG_DIR", temp_dir.path())
        .args(["config", "get", "nonexistent_key"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("Unknown setting key"))
        .stderr(predicate::str::contains("aperture config settings"));
}

/// Test `aperture config set` with invalid value type shows helpful error
#[test]
fn test_config_set_invalid_value_error() {
    let temp_dir = TempDir::new().unwrap();

    aperture_cmd()
        .env("APERTURE_CONFIG_DIR", temp_dir.path())
        .args(["config", "set", "default_timeout_secs", "not_a_number"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("Invalid value"))
        .stderr(predicate::str::contains("expected integer"));
}

/// Test that settings persist across multiple invocations
#[test]
fn test_settings_persistence() {
    let temp_dir = TempDir::new().unwrap();

    // Set multiple values
    aperture_cmd()
        .env("APERTURE_CONFIG_DIR", temp_dir.path())
        .args(["config", "set", "default_timeout_secs", "45"])
        .assert()
        .success();

    aperture_cmd()
        .env("APERTURE_CONFIG_DIR", temp_dir.path())
        .args(["config", "set", "agent_defaults.json_errors", "true"])
        .assert()
        .success();

    // Verify both values are preserved in config settings list
    let output = aperture_cmd()
        .env("APERTURE_CONFIG_DIR", temp_dir.path())
        .args(["config", "settings", "--json"])
        .assert()
        .success();

    let stdout = String::from_utf8_lossy(&output.get_output().stdout);
    let parsed: serde_json::Value = serde_json::from_str(&stdout).expect("Should be valid JSON");

    let settings = parsed.as_array().unwrap();

    // Find the timeout setting
    let timeout = settings
        .iter()
        .find(|s| s["key"] == "default_timeout_secs")
        .unwrap();
    assert_eq!(timeout["value"], "45");

    // Find the json_errors setting
    let json_errors = settings
        .iter()
        .find(|s| s["key"] == "agent_defaults.json_errors")
        .unwrap();
    assert_eq!(json_errors["value"], "true");
}
