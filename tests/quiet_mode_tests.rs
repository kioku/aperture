//! Integration tests for quiet mode functionality.
//!
//! Tests that `--quiet` and `--json-errors` properly suppress informational output.

#![cfg(feature = "integration")]

mod test_helpers;

use assert_cmd::Command;
use predicates::prelude::*;
use std::fs;
use tempfile::TempDir;

fn aperture_cmd() -> Command {
    Command::cargo_bin("aperture").unwrap()
}

fn create_minimal_spec(temp_dir: &TempDir) -> std::path::PathBuf {
    let spec_content = r#"
openapi: "3.0.0"
info:
  title: "Test API"
  version: "1.0.0"
paths:
  /users:
    get:
      operationId: listUsers
      summary: List all users
      tags:
        - users
      responses:
        "200":
          description: Success
"#;
    let spec_file = temp_dir.path().join("spec.yaml");
    fs::write(&spec_file, spec_content).unwrap();
    spec_file
}

#[test]
fn test_quiet_flag_suppresses_success_message_on_add() {
    let temp_dir = TempDir::new().unwrap();
    let spec_file = create_minimal_spec(&temp_dir);

    // Without --quiet: should see success message
    aperture_cmd()
        .env("APERTURE_CONFIG_DIR", temp_dir.path())
        .args(["config", "add", "test-api", spec_file.to_str().unwrap()])
        .assert()
        .success()
        .stdout(predicate::str::contains("added successfully"));

    // Remove to re-add
    aperture_cmd()
        .env("APERTURE_CONFIG_DIR", temp_dir.path())
        .args(["config", "remove", "test-api"])
        .assert()
        .success();

    // With --quiet: should NOT see success message
    aperture_cmd()
        .env("APERTURE_CONFIG_DIR", temp_dir.path())
        .args([
            "--quiet",
            "config",
            "add",
            "test-api",
            spec_file.to_str().unwrap(),
        ])
        .assert()
        .success()
        .stdout(predicate::str::is_empty());
}

#[test]
fn test_json_errors_implies_quiet() {
    let temp_dir = TempDir::new().unwrap();
    let spec_file = create_minimal_spec(&temp_dir);

    // --json-errors should suppress informational output
    aperture_cmd()
        .env("APERTURE_CONFIG_DIR", temp_dir.path())
        .args([
            "--json-errors",
            "config",
            "add",
            "test-api",
            spec_file.to_str().unwrap(),
        ])
        .assert()
        .success()
        .stdout(predicate::str::is_empty());
}

#[test]
fn test_quiet_flag_suppresses_remove_success_message() {
    let temp_dir = TempDir::new().unwrap();
    let spec_file = create_minimal_spec(&temp_dir);

    // First add the spec
    aperture_cmd()
        .env("APERTURE_CONFIG_DIR", temp_dir.path())
        .args([
            "--quiet",
            "config",
            "add",
            "test-api",
            spec_file.to_str().unwrap(),
        ])
        .assert()
        .success();

    // Remove with --quiet: should NOT see success message
    aperture_cmd()
        .env("APERTURE_CONFIG_DIR", temp_dir.path())
        .args(["--quiet", "config", "remove", "test-api"])
        .assert()
        .success()
        .stdout(predicate::str::is_empty());
}

#[test]
fn test_quiet_flag_suppresses_list_header_but_shows_data() {
    let temp_dir = TempDir::new().unwrap();
    let spec_file = create_minimal_spec(&temp_dir);

    // Add spec first
    aperture_cmd()
        .env("APERTURE_CONFIG_DIR", temp_dir.path())
        .args([
            "--quiet",
            "config",
            "add",
            "test-api",
            spec_file.to_str().unwrap(),
        ])
        .assert()
        .success();

    // Without --quiet: should see header and listing
    let output = aperture_cmd()
        .env("APERTURE_CONFIG_DIR", temp_dir.path())
        .args(["config", "list"])
        .assert()
        .success();

    let stdout = String::from_utf8_lossy(&output.get_output().stdout);
    assert!(stdout.contains("Registered API specifications:"));
    assert!(stdout.contains("test-api"));

    // With --quiet: should see listing (data) but NOT header
    let output = aperture_cmd()
        .env("APERTURE_CONFIG_DIR", temp_dir.path())
        .args(["--quiet", "config", "list"])
        .assert()
        .success();

    let stdout = String::from_utf8_lossy(&output.get_output().stdout);
    assert!(
        !stdout.contains("Registered API specifications:"),
        "Header should be suppressed in quiet mode"
    );
    assert!(
        stdout.contains("test-api"),
        "Data (spec names) should still be shown in quiet mode"
    );
}

#[test]
fn test_quiet_flag_short_form() {
    let temp_dir = TempDir::new().unwrap();
    let spec_file = create_minimal_spec(&temp_dir);

    // -q should work same as --quiet
    aperture_cmd()
        .env("APERTURE_CONFIG_DIR", temp_dir.path())
        .args([
            "-q",
            "config",
            "add",
            "test-api",
            spec_file.to_str().unwrap(),
        ])
        .assert()
        .success()
        .stdout(predicate::str::is_empty());
}

#[test]
fn test_quiet_mode_still_outputs_errors() {
    let temp_dir = TempDir::new().unwrap();

    // With --quiet: errors should still appear on stderr
    aperture_cmd()
        .env("APERTURE_CONFIG_DIR", temp_dir.path())
        .args(["--quiet", "config", "add", "test-api", "/nonexistent/path"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("Error").or(predicate::str::contains("error")));
}

#[test]
fn test_quiet_flag_suppresses_tips_in_list_commands() {
    let temp_dir = TempDir::new().unwrap();
    let spec_file = create_minimal_spec(&temp_dir);

    // Add spec first
    aperture_cmd()
        .env("APERTURE_CONFIG_DIR", temp_dir.path())
        .args([
            "--quiet",
            "config",
            "add",
            "test-api",
            spec_file.to_str().unwrap(),
        ])
        .assert()
        .success();

    // Without --quiet: should see tips
    aperture_cmd()
        .env("APERTURE_CONFIG_DIR", temp_dir.path())
        .args(["list-commands", "test-api"])
        .assert()
        .success()
        .stdout(predicate::str::contains("aperture docs"));

    // With --quiet: should NOT see tips but still see command list
    let output = aperture_cmd()
        .env("APERTURE_CONFIG_DIR", temp_dir.path())
        .args(["--quiet", "list-commands", "test-api"])
        .assert()
        .success();

    // Command list should still appear (it's the requested data)
    let stdout = String::from_utf8_lossy(&output.get_output().stdout);
    assert!(stdout.contains("users") || stdout.contains("list-users"));
    // But tips should not appear
    assert!(!stdout.contains("aperture docs"));
}

#[test]
fn test_quiet_flag_suppresses_header_in_cache_stats() {
    let temp_dir = TempDir::new().unwrap();
    let spec_file = create_minimal_spec(&temp_dir);

    // Add spec first
    aperture_cmd()
        .env("APERTURE_CONFIG_DIR", temp_dir.path())
        .args([
            "--quiet",
            "config",
            "add",
            "test-api",
            spec_file.to_str().unwrap(),
        ])
        .assert()
        .success();

    // Without --quiet: should see header and stats
    let output = aperture_cmd()
        .env("APERTURE_CONFIG_DIR", temp_dir.path())
        .args(["config", "cache-stats", "test-api"])
        .assert()
        .success();

    let stdout = String::from_utf8_lossy(&output.get_output().stdout);
    assert!(stdout.contains("Cache statistics for API"));
    assert!(stdout.contains("Total entries:"));

    // With --quiet: should see stats (data) but NOT header
    let output = aperture_cmd()
        .env("APERTURE_CONFIG_DIR", temp_dir.path())
        .args(["--quiet", "config", "cache-stats", "test-api"])
        .assert()
        .success();

    let stdout = String::from_utf8_lossy(&output.get_output().stdout);
    assert!(
        !stdout.contains("Cache statistics for API"),
        "Header should be suppressed in quiet mode"
    );
    assert!(
        stdout.contains("Total entries:"),
        "Stats data should still be shown in quiet mode"
    );
}

#[test]
fn test_quiet_flag_suppresses_tips_in_overview() {
    let temp_dir = TempDir::new().unwrap();
    let spec_file = create_minimal_spec(&temp_dir);

    // Add spec first
    aperture_cmd()
        .env("APERTURE_CONFIG_DIR", temp_dir.path())
        .args([
            "--quiet",
            "config",
            "add",
            "test-api",
            spec_file.to_str().unwrap(),
        ])
        .assert()
        .success();

    // Without --quiet: should see tip
    let output = aperture_cmd()
        .env("APERTURE_CONFIG_DIR", temp_dir.path())
        .args(["overview", "--all"])
        .assert()
        .success();

    let stdout = String::from_utf8_lossy(&output.get_output().stdout);
    assert!(stdout.contains("Use 'aperture overview"));
    assert!(stdout.contains("All APIs Overview"));

    // With --quiet: should NOT see tip but still see overview data
    let output = aperture_cmd()
        .env("APERTURE_CONFIG_DIR", temp_dir.path())
        .args(["--quiet", "overview", "--all"])
        .assert()
        .success();

    let stdout = String::from_utf8_lossy(&output.get_output().stdout);
    assert!(
        !stdout.contains("Use 'aperture overview"),
        "Tip should be suppressed in quiet mode"
    );
    assert!(
        stdout.contains("All APIs Overview"),
        "Overview data should still be shown in quiet mode"
    );
}

#[test]
fn test_quiet_flag_shows_data_in_config_get_url() {
    let temp_dir = TempDir::new().unwrap();
    let spec_file = create_minimal_spec(&temp_dir);

    // Add spec first
    aperture_cmd()
        .env("APERTURE_CONFIG_DIR", temp_dir.path())
        .args([
            "--quiet",
            "config",
            "add",
            "test-api",
            spec_file.to_str().unwrap(),
        ])
        .assert()
        .success();

    // Without --quiet: should see header and URL info
    let output = aperture_cmd()
        .env("APERTURE_CONFIG_DIR", temp_dir.path())
        .args(["config", "get-url", "test-api"])
        .assert()
        .success();

    let stdout = String::from_utf8_lossy(&output.get_output().stdout);
    assert!(stdout.contains("Base URL configuration for"));
    assert!(stdout.contains("Resolved URL"));

    // With --quiet: should see URL data but NOT header
    let output = aperture_cmd()
        .env("APERTURE_CONFIG_DIR", temp_dir.path())
        .args(["--quiet", "config", "get-url", "test-api"])
        .assert()
        .success();

    let stdout = String::from_utf8_lossy(&output.get_output().stdout);
    assert!(
        !stdout.contains("Base URL configuration for"),
        "Header should be suppressed in quiet mode"
    );
    assert!(
        stdout.contains("Resolved URL"),
        "URL data should still be shown in quiet mode"
    );
}

fn create_spec_with_security(temp_dir: &TempDir) -> std::path::PathBuf {
    let spec_content = r#"
openapi: "3.0.0"
info:
  title: "Test API"
  version: "1.0.0"
components:
  securitySchemes:
    api_key:
      type: apiKey
      in: header
      name: X-API-Key
paths:
  /users:
    get:
      operationId: listUsers
      summary: List all users
      tags:
        - users
      security:
        - api_key: []
      responses:
        "200":
          description: Success
"#;
    let spec_file = temp_dir.path().join("spec.yaml");
    fs::write(&spec_file, spec_content).unwrap();
    spec_file
}

#[test]
fn test_quiet_flag_shows_data_in_list_secrets() {
    let temp_dir = TempDir::new().unwrap();
    let spec_file = create_spec_with_security(&temp_dir);

    // Add spec first
    aperture_cmd()
        .env("APERTURE_CONFIG_DIR", temp_dir.path())
        .args([
            "--quiet",
            "config",
            "add",
            "test-api",
            spec_file.to_str().unwrap(),
        ])
        .assert()
        .success();

    // Configure a secret
    aperture_cmd()
        .env("APERTURE_CONFIG_DIR", temp_dir.path())
        .args([
            "--quiet",
            "config",
            "set-secret",
            "test-api",
            "api_key",
            "--env",
            "MY_API_KEY",
        ])
        .assert()
        .success();

    // Without --quiet: should see header and secret info
    let output = aperture_cmd()
        .env("APERTURE_CONFIG_DIR", temp_dir.path())
        .args(["config", "list-secrets", "test-api"])
        .assert()
        .success();

    let stdout = String::from_utf8_lossy(&output.get_output().stdout);
    assert!(stdout.contains("Configured secrets for API"));
    assert!(stdout.contains("api_key"));
    assert!(stdout.contains("MY_API_KEY"));

    // With --quiet: should see secret data but NOT header
    let output = aperture_cmd()
        .env("APERTURE_CONFIG_DIR", temp_dir.path())
        .args(["--quiet", "config", "list-secrets", "test-api"])
        .assert()
        .success();

    let stdout = String::from_utf8_lossy(&output.get_output().stdout);
    assert!(
        !stdout.contains("Configured secrets for API"),
        "Header should be suppressed in quiet mode"
    );
    assert!(
        stdout.contains("api_key"),
        "Secret scheme name should still be shown in quiet mode"
    );
    assert!(
        stdout.contains("MY_API_KEY"),
        "Secret env var should still be shown in quiet mode"
    );
}

#[test]
fn test_quiet_flag_shows_data_in_list_urls() {
    let temp_dir = TempDir::new().unwrap();
    let spec_file = create_minimal_spec(&temp_dir);

    // Add spec first
    aperture_cmd()
        .env("APERTURE_CONFIG_DIR", temp_dir.path())
        .args([
            "--quiet",
            "config",
            "add",
            "test-api",
            spec_file.to_str().unwrap(),
        ])
        .assert()
        .success();

    // Set a URL override
    aperture_cmd()
        .env("APERTURE_CONFIG_DIR", temp_dir.path())
        .args([
            "--quiet",
            "config",
            "set-url",
            "test-api",
            "https://api.example.com",
        ])
        .assert()
        .success();

    // Without --quiet: should see header and URL info
    let output = aperture_cmd()
        .env("APERTURE_CONFIG_DIR", temp_dir.path())
        .args(["config", "list-urls"])
        .assert()
        .success();

    let stdout = String::from_utf8_lossy(&output.get_output().stdout);
    assert!(stdout.contains("Configured base URLs:"));
    assert!(stdout.contains("test-api"));
    assert!(stdout.contains("https://api.example.com"));

    // With --quiet: should see URL data but NOT header
    let output = aperture_cmd()
        .env("APERTURE_CONFIG_DIR", temp_dir.path())
        .args(["--quiet", "config", "list-urls"])
        .assert()
        .success();

    let stdout = String::from_utf8_lossy(&output.get_output().stdout);
    assert!(
        !stdout.contains("Configured base URLs:"),
        "Header should be suppressed in quiet mode"
    );
    assert!(
        stdout.contains("test-api"),
        "API name should still be shown in quiet mode"
    );
    assert!(
        stdout.contains("https://api.example.com"),
        "URL should still be shown in quiet mode"
    );
}
