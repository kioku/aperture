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
fn test_quiet_flag_suppresses_list_output() {
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

    // Without --quiet: should see listing
    aperture_cmd()
        .env("APERTURE_CONFIG_DIR", temp_dir.path())
        .args(["config", "list"])
        .assert()
        .success()
        .stdout(predicate::str::contains("test-api"));

    // With --quiet: should NOT see listing
    aperture_cmd()
        .env("APERTURE_CONFIG_DIR", temp_dir.path())
        .args(["--quiet", "config", "list"])
        .assert()
        .success()
        .stdout(predicate::str::is_empty());
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
