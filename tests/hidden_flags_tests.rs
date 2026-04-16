//! Integration tests for hidden dynamic flags functionality.
//!
//! Tests that dynamic-operation flags (--jq, --format, --server-var) remain hidden from
//! generated operation help while staying functional where supported.

#![cfg(feature = "integration")]

mod test_helpers;

use assert_cmd::Command;
use std::fs;
use tempfile::TempDir;

#[allow(deprecated)] // TODO: Migrate to cargo_bin! macro
fn aperture_cmd() -> Command {
    Command::cargo_bin("aperture").unwrap()
}

fn create_minimal_spec(temp_dir: &TempDir) -> std::path::PathBuf {
    let spec_content = r#"
openapi: "3.0.0"
info:
  title: "Test API"
  version: "1.0.0"
servers:
  - url: https://{region}.api.example.com
    variables:
      region:
        default: us
        enum: [us, eu]
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
"#;
    let spec_file = temp_dir.path().join("spec.yaml");
    fs::write(&spec_file, spec_content).unwrap();
    spec_file
}

#[test]
fn test_execution_only_flags_hidden_in_main_help() {
    // Main help should stay focused on universal flags only.
    let output = aperture_cmd().args(["--help"]).assert().success();

    let stdout = String::from_utf8_lossy(&output.get_output().stdout);

    assert!(
        !stdout.contains("--jq"),
        "Expected --jq to be hidden from main help"
    );
    assert!(
        !stdout.contains("--format"),
        "Expected --format to be hidden from main help"
    );
    assert!(
        stdout.contains("--json-errors"),
        "Expected universal flags to remain visible"
    );
}

#[test]
fn test_hidden_flags_still_functional() {
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

    // Test that --format is still functional (even though hidden in dynamic tree)
    aperture_cmd()
        .env("APERTURE_CONFIG_DIR", temp_dir.path())
        .args([
            "api",
            "test-api",
            "--describe-json",
            "--format",
            "json", // This should be accepted
        ])
        .assert()
        .success();
}

#[test]
fn test_global_flags_hidden_from_dynamic_command_tree() {
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

    // Check help at the operation level - this is the dynamic command tree help.
    // Help is a successful control-flow path and should not be wrapped as a validation error.
    let output = aperture_cmd()
        .env("APERTURE_CONFIG_DIR", temp_dir.path())
        .args(["api", "test-api", "users", "list-users", "--help"])
        .output()
        .expect("Failed to execute command");

    assert!(
        output.status.success(),
        "Expected --help to exit successfully; stdout={} stderr={}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    let combined = format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    // Verify we got some help output (contains Options section)
    assert!(
        combined.contains("Options:") || combined.contains("-h, --help"),
        "Expected help output to contain Options section"
    );

    assert!(
        !combined.contains("Validation: Invalid command"),
        "Expected help output to avoid validation framing"
    );

    // The hidden flags should NOT appear in the dynamic command help
    assert!(
        !combined.contains("--jq"),
        "Expected --jq to be hidden from dynamic command help"
    );
    assert!(
        !combined.contains("--format ") && !combined.contains("--format\n"),
        "Expected --format to be hidden from dynamic command help"
    );
    assert!(
        !combined.contains("--server-var"),
        "Expected --server-var to be hidden from dynamic command help"
    );
}

#[test]
fn test_hidden_flags_work_at_operation_level() {
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

    // Verify the command structure via describe-json
    let output = aperture_cmd()
        .env("APERTURE_CONFIG_DIR", temp_dir.path())
        .args(["api", "test-api", "--describe-json"])
        .assert()
        .success();

    let stdout = String::from_utf8_lossy(&output.get_output().stdout);

    // The describe-json should work - proving the command structure is correct
    assert!(stdout.contains("listUsers") || stdout.contains("list-users"));
    assert!(stdout.contains("commands"));
}
