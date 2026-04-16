mod common;

use common::aperture_cmd;
use std::fs;
use tempfile::TempDir;

fn write_primary_spec(temp_dir: &TempDir) -> std::path::PathBuf {
    let spec_file = temp_dir.path().join("spec.yaml");
    fs::write(
        &spec_file,
        r"openapi: 3.0.0
info:
  title: Test API
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
  /users/{id}:
    get:
      tags:
        - users
      operationId: getUserById
      summary: Get user by id
      parameters:
        - name: id
          in: path
          required: true
          schema:
            type: string
      responses:
        '200':
          description: Success
  /health:
    get:
      operationId: healthCheck
      summary: Health check
      responses:
        '200':
          description: Success
",
    )
    .unwrap();
    spec_file
}

fn write_secondary_spec(temp_dir: &TempDir) -> std::path::PathBuf {
    let spec_file = temp_dir.path().join("spec-2.yaml");
    fs::write(
        &spec_file,
        r"openapi: 3.0.0
info:
  title: Billing API
  version: 2.1.0
paths:
  /invoices:
    get:
      tags:
        - billing
      operationId: listInvoices
      responses:
        '200':
          description: Success
",
    )
    .unwrap();
    spec_file
}

fn add_spec(config_dir: &std::path::Path, name: &str, spec_file: &std::path::Path) {
    aperture_cmd()
        .env("APERTURE_CONFIG_DIR", config_dir)
        .args(["config", "api", "add", name, spec_file.to_str().unwrap()])
        .assert()
        .success();
}

#[test]
fn commands_support_json_format() {
    let temp_dir = TempDir::new().unwrap();
    let spec_file = write_primary_spec(&temp_dir);
    add_spec(temp_dir.path(), "test-api", &spec_file);

    let output = aperture_cmd()
        .env("APERTURE_CONFIG_DIR", temp_dir.path())
        .args(["commands", "test-api", "--format", "json"])
        .output()
        .unwrap();

    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    let parsed: serde_json::Value = serde_json::from_str(&stdout).unwrap();

    assert_eq!(parsed["api"]["context"], "test-api");
    assert_eq!(parsed["api"]["operation_count"], 3);
    assert!(parsed["groups"].is_array());
    assert!(parsed["groups"]
        .as_array()
        .unwrap()
        .iter()
        .any(|group| group["name"] == "users"));
}

#[test]
fn overview_supports_json_for_single_and_all() {
    let temp_dir = TempDir::new().unwrap();
    let primary = write_primary_spec(&temp_dir);
    let secondary = write_secondary_spec(&temp_dir);
    add_spec(temp_dir.path(), "test-api", &primary);
    add_spec(temp_dir.path(), "billing", &secondary);

    let single = aperture_cmd()
        .env("APERTURE_CONFIG_DIR", temp_dir.path())
        .args(["overview", "test-api", "--format", "json"])
        .output()
        .unwrap();

    assert!(single.status.success());
    let single_json: serde_json::Value =
        serde_json::from_slice(&single.stdout).expect("single overview should be valid JSON");
    assert_eq!(single_json["api"]["context"], "test-api");
    assert_eq!(single_json["statistics"]["total_operations"], 3);
    assert!(single_json["statistics"]["methods"].is_array());

    let all = aperture_cmd()
        .env("APERTURE_CONFIG_DIR", temp_dir.path())
        .args(["overview", "--all", "--format", "json"])
        .output()
        .unwrap();

    assert!(all.status.success());
    let all_json: serde_json::Value =
        serde_json::from_slice(&all.stdout).expect("all overview should be valid JSON");
    let apis = all_json["apis"].as_array().unwrap();
    assert_eq!(apis.len(), 2);
    assert!(apis.iter().any(|api| api["context"] == "test-api"));
    assert!(apis.iter().any(|api| api["context"] == "billing"));
}

#[test]
fn docs_support_json_for_all_modes() {
    let temp_dir = TempDir::new().unwrap();
    let spec_file = write_primary_spec(&temp_dir);
    add_spec(temp_dir.path(), "test-api", &spec_file);

    let interactive = aperture_cmd()
        .env("APERTURE_CONFIG_DIR", temp_dir.path())
        .args(["docs", "--format", "json"])
        .output()
        .unwrap();
    assert!(interactive.status.success());
    let interactive_json: serde_json::Value = serde_json::from_slice(&interactive.stdout).unwrap();
    assert_eq!(interactive_json["mode"], "interactive");
    assert!(interactive_json["apis"]
        .as_array()
        .unwrap()
        .iter()
        .any(|api| api["context"] == "test-api"));

    let index = aperture_cmd()
        .env("APERTURE_CONFIG_DIR", temp_dir.path())
        .args(["docs", "test-api", "--format", "json"])
        .output()
        .unwrap();
    assert!(index.status.success());
    let index_json: serde_json::Value = serde_json::from_slice(&index.stdout).unwrap();
    assert_eq!(index_json["mode"], "api-reference");
    assert_eq!(index_json["api"]["context"], "test-api");
    assert!(index_json["categories"].is_array());

    let operation = aperture_cmd()
        .env("APERTURE_CONFIG_DIR", temp_dir.path())
        .args([
            "docs",
            "test-api",
            "users",
            "get-user-by-id",
            "--format",
            "json",
        ])
        .output()
        .unwrap();
    assert!(operation.status.success());
    let op_json: serde_json::Value = serde_json::from_slice(&operation.stdout).unwrap();
    assert_eq!(op_json["mode"], "operation");
    assert_eq!(op_json["operation"]["name"], "get-user-by-id");
    assert_eq!(
        op_json["operation"]["usage"],
        "aperture api test-api users get-user-by-id --id <ID>"
    );
    assert!(op_json["operation"]["parameters"]
        .as_array()
        .unwrap()
        .iter()
        .any(|param| param["cli_name"] == "id"));
}

#[test]
fn config_api_list_supports_json_output() {
    let temp_dir = TempDir::new().unwrap();
    let spec_file = write_primary_spec(&temp_dir);
    add_spec(temp_dir.path(), "test-api", &spec_file);

    let list = aperture_cmd()
        .env("APERTURE_CONFIG_DIR", temp_dir.path())
        .args(["config", "api", "list", "--json"])
        .output()
        .unwrap();
    assert!(list.status.success());
    let list_json: serde_json::Value = serde_json::from_slice(&list.stdout).unwrap();
    assert!(list_json.is_array());
    assert_eq!(list_json[0]["name"], "test-api");

    let verbose = aperture_cmd()
        .env("APERTURE_CONFIG_DIR", temp_dir.path())
        .args(["config", "api", "list", "--verbose", "--json"])
        .output()
        .unwrap();
    assert!(verbose.status.success());
    let verbose_json: serde_json::Value = serde_json::from_slice(&verbose.stdout).unwrap();
    assert_eq!(verbose_json[0]["name"], "test-api");
    assert_eq!(verbose_json[0]["version"], "1.0.0");
    assert_eq!(verbose_json[0]["endpoints"]["available"], 3);

    let legacy = aperture_cmd()
        .env("APERTURE_CONFIG_DIR", temp_dir.path())
        .args(["config", "list", "--json"])
        .output()
        .unwrap();
    assert!(legacy.status.success());
    let legacy_json: serde_json::Value = serde_json::from_slice(&legacy.stdout).unwrap();
    assert_eq!(legacy_json[0]["name"], "test-api");
}

#[test]
fn help_advertises_structured_output_only_on_supported_commands() {
    let commands_help = aperture_cmd()
        .args(["commands", "--help"])
        .output()
        .unwrap();
    let commands_help = String::from_utf8_lossy(&commands_help.stdout);
    assert!(commands_help.contains("Output format for discovery data"));

    let overview_help = aperture_cmd()
        .args(["overview", "--help"])
        .output()
        .unwrap();
    let overview_help = String::from_utf8_lossy(&overview_help.stdout);
    assert!(overview_help.contains("Output format for discovery data"));

    let docs_help = aperture_cmd().args(["docs", "--help"]).output().unwrap();
    let docs_help = String::from_utf8_lossy(&docs_help.stdout);
    assert!(docs_help.contains("Output format for discovery data"));

    let config_list_help = aperture_cmd()
        .args(["config", "api", "list", "--help"])
        .output()
        .unwrap();
    let config_list_help = String::from_utf8_lossy(&config_list_help.stdout);
    assert!(config_list_help.contains("--json"));

    let search_help = aperture_cmd().args(["search", "--help"]).output().unwrap();
    let search_help = String::from_utf8_lossy(&search_help.stdout);
    assert!(!search_help.contains("Output format for discovery data"));
}
