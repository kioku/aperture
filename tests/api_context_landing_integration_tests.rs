#![cfg(feature = "integration")]

mod common;

use common::aperture_cmd;
use std::fs;
use tempfile::TempDir;

fn create_spec(temp_dir: &TempDir) -> std::path::PathBuf {
    let spec_content = r#"
openapi: "3.0.0"
info:
  title: "Landing Test API"
  version: "1.0.0"
paths:
  /users:
    get:
      operationId: listUsers
      summary: List users
      tags:
        - users
      responses:
        "200":
          description: Success
"#;

    let spec_file = temp_dir.path().join("spec.yaml");
    fs::write(&spec_file, spec_content).expect("spec file should be written");
    spec_file
}

#[test]
fn api_context_without_operation_shows_landing_overview() {
    let temp_dir = TempDir::new().expect("temp dir should be created");
    let spec_file = create_spec(&temp_dir);

    aperture_cmd()
        .env("APERTURE_CONFIG_DIR", temp_dir.path())
        .args([
            "--quiet",
            "config",
            "api",
            "add",
            "test-api",
            spec_file.to_str().expect("spec path should be valid UTF-8"),
        ])
        .assert()
        .success();

    let output = aperture_cmd()
        .env("APERTURE_CONFIG_DIR", temp_dir.path())
        .args(["api", "test-api"])
        .assert()
        .success();

    let stdout = String::from_utf8_lossy(&output.get_output().stdout);
    assert!(stdout.contains("# test-api API"));
    assert!(stdout.contains("## Statistics"));
    assert!(stdout.contains("## Next Steps"));
    assert!(stdout.contains("aperture commands test-api"));
    assert!(stdout.contains("aperture docs test-api <tag> <operation>"));
    assert!(stdout.contains("aperture api test-api <tag> <operation> ..."));
}

#[test]
fn api_context_describe_json_remains_machine_oriented() {
    let temp_dir = TempDir::new().expect("temp dir should be created");
    let spec_file = create_spec(&temp_dir);

    aperture_cmd()
        .env("APERTURE_CONFIG_DIR", temp_dir.path())
        .args([
            "--quiet",
            "config",
            "api",
            "add",
            "test-api",
            spec_file.to_str().expect("spec path should be valid UTF-8"),
        ])
        .assert()
        .success();

    let output = aperture_cmd()
        .env("APERTURE_CONFIG_DIR", temp_dir.path())
        .args(["api", "test-api", "--describe-json"])
        .output()
        .expect("describe-json command should execute");

    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);

    let manifest: serde_json::Value =
        serde_json::from_str(&stdout).expect("describe-json output should be valid JSON");
    assert_eq!(manifest["api"]["name"].as_str(), Some("Landing Test API"));
    assert!(manifest["commands"]["users"].is_array());
}
