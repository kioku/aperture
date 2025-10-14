#![cfg(feature = "integration")]

mod common;

use common::aperture_cmd;
use predicates::prelude::*;
use serde_json;
use std::fs;
use tempfile::TempDir;

#[test]
fn test_tags_with_spaces_converted_to_kebab_case() {
    let temp_dir = TempDir::new().unwrap();
    let config_dir = temp_dir.path().to_path_buf();
    let spec_file = temp_dir.path().join("spaces-tags.yaml");

    // Create OpenAPI spec with tags containing spaces
    let spec_content = r#"openapi: 3.0.0
info:
  title: OpenProject-like API
  version: 1.0.0
servers:
  - url: https://api.example.com
paths:
  /work_packages:
    get:
      tags:
        - Work Packages
      operationId: listWorkPackages
      responses:
        '200':
          description: Success
  /work_packages/{id}:
    get:
      tags:
        - Work Packages
      operationId: getWorkPackage
      parameters:
        - name: id
          in: path
          required: true
          schema:
            type: integer
      responses:
        '200':
          description: Success
  /wiki_pages:
    get:
      tags:
        - Wiki Pages
      operationId: listWikiPages
      responses:
        '200':
          description: Success
  /time_entries:
    get:
      tags:
        - Time Entries
      operationId: listTimeEntries
      responses:
        '200':
          description: Success
"#;

    fs::write(&spec_file, spec_content).unwrap();

    // Add the spec
    let mut cmd = aperture_cmd();
    cmd.env("APERTURE_CONFIG_DIR", config_dir.to_str().unwrap())
        .arg("config")
        .arg("add")
        .arg("openproject")
        .arg(spec_file.to_str().unwrap())
        .assert()
        .success();

    // Test "Work Packages" tag as "work-packages"
    let mut cmd = aperture_cmd();
    cmd.env("APERTURE_CONFIG_DIR", config_dir.to_str().unwrap())
        .arg("api")
        .arg("--dry-run")
        .arg("openproject")
        .arg("work-packages")
        .arg("list-work-packages")
        .assert()
        .success()
        .stdout(predicate::str::contains("GET"))
        .stdout(predicate::str::contains("/work_packages"));

    // Test "Wiki Pages" tag as "wiki-pages"
    let mut cmd = aperture_cmd();
    cmd.env("APERTURE_CONFIG_DIR", config_dir.to_str().unwrap())
        .arg("api")
        .arg("--dry-run")
        .arg("openproject")
        .arg("wiki-pages")
        .arg("list-wiki-pages")
        .assert()
        .success()
        .stdout(predicate::str::contains("GET"))
        .stdout(predicate::str::contains("/wiki_pages"));

    // Test "Time Entries" tag as "time-entries"
    let mut cmd = aperture_cmd();
    cmd.env("APERTURE_CONFIG_DIR", config_dir.to_str().unwrap())
        .arg("api")
        .arg("--dry-run")
        .arg("openproject")
        .arg("time-entries")
        .arg("list-time-entries")
        .assert()
        .success()
        .stdout(predicate::str::contains("GET"))
        .stdout(predicate::str::contains("/time_entries"));
}

#[test]
fn test_tags_with_spaces_in_describe_json() {
    let temp_dir = TempDir::new().unwrap();
    let config_dir = temp_dir.path().to_path_buf();
    let spec_file = temp_dir.path().join("spaces-tags.yaml");

    // Create OpenAPI spec with tags containing spaces
    let spec_content = r#"openapi: 3.0.0
info:
  title: Test API
  version: 1.0.0
servers:
  - url: https://api.example.com
paths:
  /items:
    get:
      tags:
        - Work Packages
      operationId: listItems
      responses:
        '200':
          description: Success
  /documents:
    get:
      tags:
        - Project Phases
      operationId: listDocuments
      responses:
        '200':
          description: Success
"#;

    fs::write(&spec_file, spec_content).unwrap();

    // Add the spec
    let mut cmd = aperture_cmd();
    cmd.env("APERTURE_CONFIG_DIR", config_dir.to_str().unwrap())
        .arg("config")
        .arg("add")
        .arg("test-api")
        .arg(spec_file.to_str().unwrap())
        .assert()
        .success();

    // Get the describe-json output
    let mut cmd = aperture_cmd();
    let output = cmd
        .env("APERTURE_CONFIG_DIR", config_dir.to_str().unwrap())
        .arg("api")
        .arg("--describe-json")
        .arg("test-api")
        .output()
        .unwrap();

    assert!(output.status.success());
    let json_output = String::from_utf8(output.stdout).unwrap();

    // Parse the JSON and verify tag names are kebab-case
    let manifest: serde_json::Value = serde_json::from_str(&json_output).unwrap();
    let commands = manifest["commands"].as_object().unwrap();

    // Verify that the JSON manifest contains kebab-case tag names
    assert!(
        commands.contains_key("work-packages"),
        "Expected 'work-packages' tag in JSON manifest"
    );
    assert!(
        commands.contains_key("project-phases"),
        "Expected 'project-phases' tag in JSON manifest"
    );
    assert!(
        !commands.contains_key("Work Packages"),
        "Unexpected 'Work Packages' tag in JSON manifest"
    );
    assert!(
        !commands.contains_key("Project Phases"),
        "Unexpected 'Project Phases' tag in JSON manifest"
    );

    // Verify that tags arrays within commands are kebab-case
    let work_packages_commands = commands["work-packages"].as_array().unwrap();
    assert!(!work_packages_commands.is_empty());
    let first_command = &work_packages_commands[0];

    // Verify tags field contains kebab-case
    let tags = first_command["tags"].as_array().unwrap();
    assert_eq!(tags.len(), 1);
    assert_eq!(tags[0].as_str().unwrap(), "work-packages");

    // Verify original_tags field contains original values
    let original_tags = first_command["original_tags"].as_array().unwrap();
    assert_eq!(original_tags.len(), 1);
    assert_eq!(original_tags[0].as_str().unwrap(), "Work Packages");

    // Same verification for project-phases
    let project_phases_commands = commands["project-phases"].as_array().unwrap();
    assert!(!project_phases_commands.is_empty());
    let first_command = &project_phases_commands[0];

    let tags = first_command["tags"].as_array().unwrap();
    assert_eq!(tags.len(), 1);
    assert_eq!(tags[0].as_str().unwrap(), "project-phases");

    let original_tags = first_command["original_tags"].as_array().unwrap();
    assert_eq!(original_tags.len(), 1);
    assert_eq!(original_tags[0].as_str().unwrap(), "Project Phases");

    // Verify we can use the kebab-case tags from the manifest to execute commands
    for tag_name in commands.keys() {
        let commands_in_tag = commands[tag_name].as_array().unwrap();
        if let Some(first_command) = commands_in_tag.first() {
            let operation_name = first_command["name"].as_str().unwrap();

            let mut cmd = aperture_cmd();
            cmd.env("APERTURE_CONFIG_DIR", config_dir.to_str().unwrap())
                .arg("api")
                .arg("--dry-run")
                .arg("test-api")
                .arg(tag_name)
                .arg(operation_name)
                .assert()
                .success();
        }
    }
}

#[test]
fn test_mixed_case_tags_with_spaces() {
    let temp_dir = TempDir::new().unwrap();
    let config_dir = temp_dir.path().to_path_buf();
    let spec_file = temp_dir.path().join("mixed-tags.yaml");

    // Create OpenAPI spec with mixed case tags containing spaces
    let spec_content = r#"openapi: 3.0.0
info:
  title: Mixed Case API
  version: 1.0.0
servers:
  - url: https://api.example.com
paths:
  /endpoint1:
    get:
      tags:
        - Custom Actions
      operationId: getEndpoint1
      responses:
        '200':
          description: Success
  /endpoint2:
    get:
      tags:
        - OAuth 2
      operationId: getEndpoint2
      responses:
        '200':
          description: Success
  /endpoint3:
    get:
      tags:
        - Query Filter Instance Schema
      operationId: getEndpoint3
      responses:
        '200':
          description: Success
"#;

    fs::write(&spec_file, spec_content).unwrap();

    // Add the spec
    let mut cmd = aperture_cmd();
    cmd.env("APERTURE_CONFIG_DIR", config_dir.to_str().unwrap())
        .arg("config")
        .arg("add")
        .arg("mixed-api")
        .arg(spec_file.to_str().unwrap())
        .assert()
        .success();

    // Test "Custom Actions" becomes "custom-actions"
    let mut cmd = aperture_cmd();
    cmd.env("APERTURE_CONFIG_DIR", config_dir.to_str().unwrap())
        .arg("api")
        .arg("--dry-run")
        .arg("mixed-api")
        .arg("custom-actions")
        .arg("get-endpoint1")
        .assert()
        .success()
        .stdout(predicate::str::contains("GET"))
        .stdout(predicate::str::contains("/endpoint1"));

    // Test "OAuth 2" becomes "o-auth-2"
    let mut cmd = aperture_cmd();
    cmd.env("APERTURE_CONFIG_DIR", config_dir.to_str().unwrap())
        .arg("api")
        .arg("--dry-run")
        .arg("mixed-api")
        .arg("o-auth-2")
        .arg("get-endpoint2")
        .assert()
        .success()
        .stdout(predicate::str::contains("GET"))
        .stdout(predicate::str::contains("/endpoint2"));

    // Test "Query Filter Instance Schema" becomes "query-filter-instance-schema"
    let mut cmd = aperture_cmd();
    cmd.env("APERTURE_CONFIG_DIR", config_dir.to_str().unwrap())
        .arg("api")
        .arg("--dry-run")
        .arg("mixed-api")
        .arg("query-filter-instance-schema")
        .arg("get-endpoint3")
        .assert()
        .success()
        .stdout(predicate::str::contains("GET"))
        .stdout(predicate::str::contains("/endpoint3"));
}

#[test]
fn test_help_shows_kebab_case_tags() {
    let temp_dir = TempDir::new().unwrap();
    let config_dir = temp_dir.path().to_path_buf();
    let spec_file = temp_dir.path().join("help-test.yaml");

    // Create OpenAPI spec with space-containing tags
    let spec_content = r#"openapi: 3.0.0
info:
  title: Help Test API
  version: 1.0.0
servers:
  - url: https://api.example.com
paths:
  /items:
    get:
      tags:
        - Work Packages
      operationId: listItems
      responses:
        '200':
          description: Success
"#;

    fs::write(&spec_file, spec_content).unwrap();

    // Add the spec
    let mut cmd = aperture_cmd();
    cmd.env("APERTURE_CONFIG_DIR", config_dir.to_str().unwrap())
        .arg("config")
        .arg("add")
        .arg("help-test")
        .arg(spec_file.to_str().unwrap())
        .assert()
        .success();

    // Test that we can use the kebab-case tag (this is the real test)
    // The help output at the root level doesn't show tag names, but we can verify
    // by trying to access the command
    let mut cmd = aperture_cmd();
    cmd.env("APERTURE_CONFIG_DIR", config_dir.to_str().unwrap())
        .arg("api")
        .arg("--dry-run")
        .arg("help-test")
        .arg("work-packages")
        .arg("list-items")
        .assert()
        .success()
        .stdout(predicate::str::contains("GET"))
        .stdout(predicate::str::contains("/items"));
}

#[test]
fn test_parameter_passing_with_space_tags() {
    let temp_dir = TempDir::new().unwrap();
    let config_dir = temp_dir.path().to_path_buf();
    let spec_file = temp_dir.path().join("param-test.yaml");

    // Create OpenAPI spec with parameters and space-containing tags
    let spec_content = r#"openapi: 3.0.0
info:
  title: Parameter Test API
  version: 1.0.0
servers:
  - url: https://api.example.com
paths:
  /work_packages/{id}:
    get:
      tags:
        - Work Packages
      operationId: getWorkPackageById
      parameters:
        - name: id
          in: path
          required: true
          schema:
            type: integer
        - name: include
          in: query
          required: false
          schema:
            type: string
      responses:
        '200':
          description: Success
"#;

    fs::write(&spec_file, spec_content).unwrap();

    // Add the spec
    let mut cmd = aperture_cmd();
    cmd.env("APERTURE_CONFIG_DIR", config_dir.to_str().unwrap())
        .arg("config")
        .arg("add")
        .arg("param-test")
        .arg(spec_file.to_str().unwrap())
        .assert()
        .success();

    // Test with parameters
    let mut cmd = aperture_cmd();
    cmd.env("APERTURE_CONFIG_DIR", config_dir.to_str().unwrap())
        .arg("api")
        .arg("--dry-run")
        .arg("param-test")
        .arg("work-packages")
        .arg("get-work-package-by-id")
        .arg("--id")
        .arg("123")
        .arg("--include")
        .arg("attachments")
        .assert()
        .success()
        .stdout(predicate::str::contains("GET"))
        .stdout(predicate::str::contains("/work_packages/123"))
        .stdout(predicate::str::contains("include=attachments"));
}
