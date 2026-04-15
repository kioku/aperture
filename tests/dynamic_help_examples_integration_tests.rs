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

fn add_spec_named(temp_dir: &TempDir, name: &str, spec_file: &std::path::Path) {
    aperture_cmd()
        .env("APERTURE_CONFIG_DIR", temp_dir.path())
        .args(["config", "add", name, spec_file.to_str().unwrap()])
        .assert()
        .success();
}

fn add_spec(temp_dir: &TempDir, spec_file: &std::path::Path) {
    add_spec_named(temp_dir, "test-api", spec_file);
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

fn assert_failure_with_validation_framing(output: &Output) {
    assert!(
        !output.status.success(),
        "expected failure; stdout={} stderr={}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    let combined = combined_output(output);
    assert!(
        combined.contains("Validation: Invalid command"),
        "expected invalid command framing for malformed input; got {combined}"
    );
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
fn api_show_examples_rejects_unknown_extra_flags() {
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
            "--bogus",
        ],
    );

    assert_failure_with_validation_framing(&output);
    assert!(
        combined_output(&output).contains("--bogus"),
        "expected malformed flag details in output"
    );
}

#[test]
fn api_show_examples_rejects_duplicate_flags() {
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
            "--show-examples",
        ],
    );

    assert_failure_with_validation_framing(&output);
    assert!(
        combined_output(&output).contains("cannot be used multiple times"),
        "expected duplicate flag error details in output"
    );
}

#[test]
fn api_show_examples_rejects_invalid_flag_value() {
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
            "--show-examples=foo",
        ],
    );

    assert_failure_with_validation_framing(&output);
    assert!(
        combined_output(&output).contains("unexpected value 'foo'"),
        "expected invalid value details in output"
    );
}

#[test]
fn api_show_examples_rejects_missing_operation_target() {
    let temp_dir = TempDir::new().unwrap();
    let spec_file = create_required_param_spec(&temp_dir);
    add_spec(&temp_dir, &spec_file);

    let output = run_with_config_dir(&temp_dir, &["api", "test-api", "users", "--show-examples"]);

    assert_failure_with_validation_framing(&output);
    assert!(
        combined_output(&output).contains("unexpected argument '--show-examples'"),
        "expected missing operation parse failure details in output"
    );
}

#[test]
fn api_show_examples_rejects_whitespace_operation_name() {
    let temp_dir = TempDir::new().unwrap();
    let spec_file = create_required_param_spec(&temp_dir);
    add_spec(&temp_dir, &spec_file);

    let output = run_with_config_dir(
        &temp_dir,
        &["api", "test-api", "users", " ", "--show-examples"],
    );

    assert_failure_with_validation_framing(&output);
    assert!(
        combined_output(&output).contains("unrecognized subcommand ' '"),
        "expected whitespace operation parse failure details in output"
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

#[test]
fn exec_api_filter_disambiguates_multi_api_shortcuts() {
    let temp_dir = TempDir::new().unwrap();
    let spec_file = create_required_param_spec(&temp_dir);
    add_spec_named(&temp_dir, "users-a", &spec_file);
    add_spec_named(&temp_dir, "users-b", &spec_file);

    let output = run_with_config_dir(
        &temp_dir,
        &["exec", "--api", "users-a", "get-user-by-id", "--help"],
    );

    assert_success_without_validation_framing(&output);
    let combined = combined_output(&output);
    assert!(
        combined.contains("Resolved shortcut to: aperture api users-a users get-user-by-id"),
        "expected API-scoped resolution output; got {combined}"
    );
}

#[test]
fn exec_ambiguity_output_explains_api_disambiguation() {
    let temp_dir = TempDir::new().unwrap();
    let spec_file = create_required_param_spec(&temp_dir);
    add_spec_named(&temp_dir, "users-a", &spec_file);
    add_spec_named(&temp_dir, "users-b", &spec_file);

    let output = run_with_config_dir(&temp_dir, &["exec", "get-user-by-id", "--help"]);

    assert!(
        !output.status.success(),
        "expected ambiguity failure; stdout={} stderr={}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    let combined = combined_output(&output);
    assert!(
        combined.contains("Multiple commands match this shortcut"),
        "expected explicit ambiguity header; got {combined}"
    );
    assert!(
        combined.contains("--api <name>"),
        "expected API narrowing guidance; got {combined}"
    );
    assert!(
        combined.contains("[api: users-a]") && combined.contains("[api: users-b]"),
        "expected per-API suggestions; got {combined}"
    );
}
