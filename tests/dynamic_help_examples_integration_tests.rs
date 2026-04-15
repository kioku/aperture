#![cfg(feature = "integration")]

mod common;

use common::aperture_cmd;
use std::fs;
use std::process::Output;
use tempfile::TempDir;

fn create_required_param_spec(temp_dir: &TempDir) -> std::path::PathBuf {
    let spec_content = r"
openapi: 3.0.0
info:
  title: Test API
  version: 1.0.0
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
";

    let spec_file = temp_dir.path().join("spec.yaml");
    fs::write(&spec_file, spec_content).expect("failed to write test spec");
    spec_file
}

fn add_spec(temp_dir: &TempDir, spec_file: &std::path::Path) {
    aperture_cmd()
        .env("APERTURE_CONFIG_DIR", temp_dir.path())
        .args(["config", "add", "test-api", spec_file.to_str().unwrap()])
        .assert()
        .success();
}

fn run_with_config_dir(temp_dir: &TempDir, args: &[&str]) -> Output {
    aperture_cmd()
        .env("APERTURE_CONFIG_DIR", temp_dir.path())
        .args(args)
        .output()
        .expect("failed to execute aperture command")
}

fn assert_success_without_validation_framing(output: &Output) {
    assert!(
        output.status.success(),
        "expected success; stdout={} stderr={}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);

    assert!(
        !stdout.contains("Validation: Invalid command")
            && !stderr.contains("Validation: Invalid command"),
        "help/examples should not be wrapped as validation failure; stdout={stdout} stderr={stderr}"
    );
}

fn combined_output(output: &Output) -> String {
    format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    )
}

#[test]
fn api_group_help_is_successful_control_flow() {
    let temp_dir = TempDir::new().unwrap();
    let spec_file = create_required_param_spec(&temp_dir);
    add_spec(&temp_dir, &spec_file);

    let output = run_with_config_dir(&temp_dir, &["api", "test-api", "users", "--help"]);
    assert_success_without_validation_framing(&output);

    let combined = combined_output(&output);
    assert!(
        combined.contains("Usage: api users") || combined.contains("users operations"),
        "expected group help output; got {combined}"
    );
}

#[test]
fn api_operation_help_succeeds_without_required_runtime_arguments() {
    let temp_dir = TempDir::new().unwrap();
    let spec_file = create_required_param_spec(&temp_dir);
    add_spec(&temp_dir, &spec_file);

    let output = run_with_config_dir(
        &temp_dir,
        &["api", "test-api", "users", "get-user-by-id", "--help"],
    );
    assert_success_without_validation_framing(&output);

    let combined = combined_output(&output);
    assert!(
        combined.contains("--id <ID>") || combined.contains("Path parameter: id"),
        "expected operation help output; got {combined}"
    );
}

#[test]
fn api_show_examples_succeeds_without_required_runtime_arguments() {
    let temp_dir = TempDir::new().unwrap();
    let spec_file = create_required_param_spec(&temp_dir);
    add_spec(&temp_dir, &spec_file);

    let output = run_with_config_dir(
        &temp_dir,
        &[
            "api",
            "test-api",
            "users",
            "get-user-by-id",
            "--show-examples",
        ],
    );
    assert_success_without_validation_framing(&output);

    let combined = combined_output(&output);
    assert!(
        combined.contains("Command: get-user-by-id")
            && (combined.contains("No examples available") || combined.contains("Examples:")),
        "expected example output; got {combined}"
    );
}

#[test]
fn exec_help_resolves_shortcut_and_succeeds_without_required_runtime_arguments() {
    let temp_dir = TempDir::new().unwrap();
    let spec_file = create_required_param_spec(&temp_dir);
    add_spec(&temp_dir, &spec_file);

    let output = run_with_config_dir(&temp_dir, &["exec", "get-user-by-id", "--help"]);
    assert_success_without_validation_framing(&output);

    let combined = combined_output(&output);
    assert!(
        combined.contains("Usage: api users get-user-by-id")
            || combined.contains("Resolved shortcut to:"),
        "expected resolved help output; got {combined}"
    );
}

#[test]
fn exec_show_examples_succeeds_without_required_runtime_arguments() {
    let temp_dir = TempDir::new().unwrap();
    let spec_file = create_required_param_spec(&temp_dir);
    add_spec(&temp_dir, &spec_file);

    let output = run_with_config_dir(&temp_dir, &["exec", "get-user-by-id", "--show-examples"]);
    assert_success_without_validation_framing(&output);

    let combined = combined_output(&output);
    assert!(
        combined.contains("Resolved shortcut to:") && combined.contains("Command: get-user-by-id"),
        "expected resolved examples output; got {combined}"
    );
}
