use assert_cmd::Command;
use predicates::prelude::*;
use std::fs;
use tempfile::TempDir;

#[test]
fn test_uppercase_tag_conversion() {
    let temp_dir = TempDir::new().unwrap();
    let config_dir = temp_dir.path().to_path_buf();
    let spec_file = temp_dir.path().join("uppercase-tags.yaml");

    // Create OpenAPI spec with uppercase tags
    let spec_content = r#"openapi: 3.0.0
info:
  title: Test API
  version: 1.0.0
servers:
  - url: https://api.example.com
paths:
  /users:
    get:
      tags:
        - Users
      operationId: getUsers
      responses:
        '200':
          description: Success
  /events:
    get:
      tags:
        - EVENTS
      operationId: getEvents
      responses:
        '200':
          description: Success
  /mixed-case:
    get:
      tags:
        - MixedCaseTag
      operationId: getMixed
      responses:
        '200':
          description: Success
"#;

    fs::write(&spec_file, spec_content).unwrap();

    // Add the spec
    let mut cmd = Command::cargo_bin("aperture").unwrap();
    cmd.env("APERTURE_CONFIG_DIR", config_dir.to_str().unwrap())
        .arg("config")
        .arg("add")
        .arg("uppercase-test")
        .arg(spec_file.to_str().unwrap())
        .assert()
        .success();

    // The describe-json output shows the raw tags, but the CLI accepts lowercase versions
    // Let's verify the CLI accepts lowercase tags by using dry-run
    let mut cmd = Command::cargo_bin("aperture").unwrap();
    cmd.env("APERTURE_CONFIG_DIR", config_dir.to_str().unwrap())
        .arg("api")
        .arg("--dry-run")
        .arg("uppercase-test")
        .arg("users")  // lowercase tag
        .arg("get-users")
        .assert()
        .success()
        .stdout(predicate::str::contains("GET"))
        .stdout(predicate::str::contains("/users"));

    // Test EVENTS tag as lowercase
    let mut cmd = Command::cargo_bin("aperture").unwrap();
    cmd.env("APERTURE_CONFIG_DIR", config_dir.to_str().unwrap())
        .arg("api")
        .arg("--dry-run")
        .arg("uppercase-test")
        .arg("events")  // lowercase tag  
        .arg("get-events")
        .assert()
        .success()
        .stdout(predicate::str::contains("GET"))
        .stdout(predicate::str::contains("/events"));

    // Test MixedCaseTag as lowercase
    let mut cmd = Command::cargo_bin("aperture").unwrap();
    cmd.env("APERTURE_CONFIG_DIR", config_dir.to_str().unwrap())
        .arg("api")
        .arg("--dry-run")
        .arg("uppercase-test")
        .arg("mixedcasetag")  // lowercase tag
        .arg("get-mixed")
        .assert()
        .success()
        .stdout(predicate::str::contains("GET"))
        .stdout(predicate::str::contains("/mixed-case"));
}

#[test]
fn test_uppercase_tag_error_suggestions() {
    // This test can be removed as the new architecture doesn't use subcommands for tags
    // The tags are now part of the dynamically parsed arguments
}

#[test]
fn test_unicode_tag_names() {
    let temp_dir = TempDir::new().unwrap();
    let config_dir = temp_dir.path().to_path_buf();
    let spec_file = temp_dir.path().join("unicode-tags.yaml");

    // Create OpenAPI spec with Unicode tags
    let spec_content = r#"openapi: 3.0.0
info:
  title: Unicode Test API
  version: 1.0.0
servers:
  - url: https://api.example.com
paths:
  /cafe:
    get:
      tags:
        - CAFÉ
      operationId: getCafe
      responses:
        '200':
          description: Success
  /spanish:
    get:
      tags:
        - ÑOÑO
      operationId: getSpanish
      responses:
        '200':
          description: Success
"#;

    fs::write(&spec_file, spec_content).unwrap();

    // Add the spec
    let mut cmd = Command::cargo_bin("aperture").unwrap();
    cmd.env("APERTURE_CONFIG_DIR", config_dir.to_str().unwrap())
        .arg("config")
        .arg("add")
        .arg("unicode-test")
        .arg(spec_file.to_str().unwrap())
        .assert()
        .success();

    // Test Unicode tags work with lowercase in CLI
    let mut cmd = Command::cargo_bin("aperture").unwrap();
    cmd.env("APERTURE_CONFIG_DIR", config_dir.to_str().unwrap())
        .arg("api")
        .arg("--dry-run")
        .arg("unicode-test")
        .arg("café")  // lowercase Unicode tag
        .arg("get-cafe")
        .assert()
        .success()
        .stdout(predicate::str::contains("GET"))
        .stdout(predicate::str::contains("/cafe"));

    // Test Spanish characters
    let mut cmd = Command::cargo_bin("aperture").unwrap();
    cmd.env("APERTURE_CONFIG_DIR", config_dir.to_str().unwrap())
        .arg("api")
        .arg("--dry-run")
        .arg("unicode-test")
        .arg("ñoño")  // lowercase Unicode tag
        .arg("get-spanish")
        .assert()
        .success()
        .stdout(predicate::str::contains("GET"))
        .stdout(predicate::str::contains("/spanish"));
}

#[test]
fn test_operation_names_with_spaces_in_tags() {
    let temp_dir = TempDir::new().unwrap();
    let config_dir = temp_dir.path().to_path_buf();
    let spec_file = temp_dir.path().join("spaces-tags.yaml");

    // Create OpenAPI spec with operation IDs containing spaces
    let spec_content = r#"openapi: 3.0.0
info:
  title: Spaces Test API
  version: 1.0.0
servers:
  - url: https://api.example.com
paths:
  /issues:
    get:
      tags:
        - Events
      operationId: List an Organization's Issues
      responses:
        '200':
          description: Success
  /projects:
    get:
      tags:
        - PROJECTS
      operationId: List Organization's Projects
      responses:
        '200':
          description: Success
"#;

    fs::write(&spec_file, spec_content).unwrap();

    // Add the spec
    let mut cmd = Command::cargo_bin("aperture").unwrap();
    cmd.env("APERTURE_CONFIG_DIR", config_dir.to_str().unwrap())
        .arg("config")
        .arg("add")
        .arg("spaces-test")
        .arg(spec_file.to_str().unwrap())
        .assert()
        .success();

    // Test tags work with lowercase and operations are kebab-cased
    let mut cmd = Command::cargo_bin("aperture").unwrap();
    cmd.env("APERTURE_CONFIG_DIR", config_dir.to_str().unwrap())
        .arg("api")
        .arg("--dry-run")
        .arg("spaces-test")
        .arg("events")  // lowercase tag
        .arg("list-an-organizations-issues")  // kebab-case operation
        .assert()
        .success()
        .stdout(predicate::str::contains("GET"))
        .stdout(predicate::str::contains("/issues"));

    // Test projects tag
    let mut cmd = Command::cargo_bin("aperture").unwrap();
    cmd.env("APERTURE_CONFIG_DIR", config_dir.to_str().unwrap())
        .arg("api")
        .arg("--dry-run")
        .arg("spaces-test")
        .arg("projects")  // lowercase tag
        .arg("list-organizations-projects")  // kebab-case operation
        .assert()
        .success()
        .stdout(predicate::str::contains("GET"))
        .stdout(predicate::str::contains("/projects"));
}

#[test]
fn test_tag_case_insensitive_cli() {
    let temp_dir = TempDir::new().unwrap();
    let config_dir = temp_dir.path().to_path_buf();
    let spec_file = temp_dir.path().join("case-test.yaml");

    // Create OpenAPI spec with various case tags
    let spec_content = r#"openapi: 3.0.0
info:
  title: Case Test API
  version: 1.0.0
servers:
  - url: https://api.example.com
paths:
  /test1:
    get:
      tags:
        - UPPERCASE
      operationId: test1
      responses:
        '200':
          description: Success
  /test2:
    get:
      tags:
        - MixedCase
      operationId: test2
      responses:
        '200':
          description: Success
"#;

    fs::write(&spec_file, spec_content).unwrap();

    // Add the spec
    let mut cmd = Command::cargo_bin("aperture").unwrap();
    cmd.env("APERTURE_CONFIG_DIR", config_dir.to_str().unwrap())
        .arg("config")
        .arg("add")
        .arg("case-test")
        .arg(spec_file.to_str().unwrap())
        .assert()
        .success();

    // Test that CLI accepts lowercase versions of tags
    let mut cmd = Command::cargo_bin("aperture").unwrap();
    cmd.env("APERTURE_CONFIG_DIR", config_dir.to_str().unwrap())
        .arg("api")
        .arg("--dry-run")
        .arg("case-test")
        .arg("uppercase")  // lowercase version
        .arg("test1")
        .assert()
        .success();

    let mut cmd = Command::cargo_bin("aperture").unwrap();
    cmd.env("APERTURE_CONFIG_DIR", config_dir.to_str().unwrap())
        .arg("api")
        .arg("--dry-run")
        .arg("case-test")
        .arg("mixedcase")  // lowercase version
        .arg("test2")
        .assert()
        .success();
}