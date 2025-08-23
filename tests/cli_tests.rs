#![cfg(feature = "integration")]

mod common;

use assert_cmd::Command;
use common::aperture_cmd;
use predicates::prelude::*;
use std::fs;
use std::path::PathBuf;

// Helper to get the path to the test binary
fn get_bin() -> Command {
    aperture_cmd()
}

// Helper to create a temporary config directory for tests
fn setup_temp_config_dir(test_name: &str) -> PathBuf {
    let mut path = std::env::temp_dir();
    path.push("aperture_cli_test");
    path.push(test_name);
    if path.exists() {
        fs::remove_dir_all(&path).unwrap();
    }
    fs::create_dir_all(&path).unwrap();
    path
}

#[test]
fn test_config_add_new_spec() {
    let config_dir = setup_temp_config_dir("test_config_add_new_spec");
    let spec_file = config_dir.join("test_api.yaml");
    fs::write(
        &spec_file,
        "openapi: 3.0.0\ninfo:\n  title: Test API\n  version: 1.0.0\npaths: {}",
    )
    .unwrap();

    get_bin()
        .arg("config")
        .arg("add")
        .arg("my-api")
        .arg(&spec_file)
        .env("APERTURE_CONFIG_DIR", &config_dir)
        .assert()
        .success()
        .stdout(predicate::str::contains(
            "Spec 'my-api' added successfully.",
        ));

    assert!(config_dir.join("specs").join("my-api.yaml").exists());
}

#[test]
fn test_config_add_existing_spec_no_force() {
    let config_dir = setup_temp_config_dir("test_config_add_existing_spec_no_force");
    let existing_spec_path = config_dir.join("specs").join("my-api.yaml");
    fs::create_dir_all(existing_spec_path.parent().unwrap()).unwrap();
    fs::write(
        &existing_spec_path,
        "openapi: 3.0.0\ninfo:\n  title: Original API\n  version: 1.0.0\npaths: {}",
    )
    .unwrap();

    let new_spec_file = config_dir.join("new_api.yaml");
    fs::write(
        &new_spec_file,
        "openapi: 3.0.0\ninfo:\n  title: New API\n  version: 1.0.0\npaths: {}",
    )
    .unwrap();

    get_bin()
        .arg("config")
        .arg("add")
        .arg("my-api")
        .arg(&new_spec_file)
        .env("APERTURE_CONFIG_DIR", &config_dir)
        .assert()
        .failure()
        .stderr(predicate::str::contains(
            "API specification 'my-api' already exists. Use --force to overwrite",
        ));

    assert_eq!(
        fs::read_to_string(&existing_spec_path).unwrap(),
        "openapi: 3.0.0\ninfo:\n  title: Original API\n  version: 1.0.0\npaths: {}"
    );
}

#[test]
fn test_config_add_existing_spec_with_force() {
    let config_dir = setup_temp_config_dir("test_config_add_existing_spec_with_force");
    let existing_spec_path = config_dir.join("specs").join("my-api.yaml");
    fs::create_dir_all(existing_spec_path.parent().unwrap()).unwrap();
    fs::write(
        &existing_spec_path,
        "openapi: 3.0.0\ninfo:\n  title: Original API\n  version: 1.0.0\npaths: {}",
    )
    .unwrap();

    let new_spec_file = config_dir.join("new_api.yaml");
    fs::write(
        &new_spec_file,
        "openapi: 3.0.0\ninfo:\n  title: New API\n  version: 1.0.0\npaths: {}",
    )
    .unwrap();

    get_bin()
        .arg("config")
        .arg("add")
        .arg("my-api")
        .arg(&new_spec_file)
        .arg("--force")
        .env("APERTURE_CONFIG_DIR", &config_dir)
        .assert()
        .success()
        .stdout(predicate::str::contains(
            "Spec 'my-api' added successfully.",
        ));

    assert_eq!(
        fs::read_to_string(&existing_spec_path).unwrap(),
        "openapi: 3.0.0\ninfo:\n  title: New API\n  version: 1.0.0\npaths: {}"
    );
}

#[test]
fn test_config_list_no_specs() {
    let config_dir = setup_temp_config_dir("test_config_list_no_specs");

    get_bin()
        .arg("config")
        .arg("list")
        .env("APERTURE_CONFIG_DIR", &config_dir)
        .assert()
        .success()
        .stdout(predicate::str::contains("No API specifications found."));
}

#[test]
fn test_config_list_multiple_specs() {
    let config_dir = setup_temp_config_dir("test_config_list_multiple_specs");
    let specs_dir = config_dir.join("specs");
    fs::create_dir_all(&specs_dir).unwrap();
    fs::write(
        &specs_dir.join("api1.yaml"),
        "openapi: 3.0.0\ninfo:\n  title: API1\n  version: 1.0.0\npaths: {}",
    )
    .unwrap();
    fs::write(
        &specs_dir.join("api2.yaml"),
        "openapi: 3.0.0\ninfo:\n  title: API2\n  version: 1.0.0\npaths: {}",
    )
    .unwrap();

    get_bin()
        .arg("config")
        .arg("list")
        .env("APERTURE_CONFIG_DIR", &config_dir)
        .assert()
        .success()
        .stdout(predicate::str::contains("api1").and(predicate::str::contains("api2")));
}

#[test]
fn test_config_remove_spec_success() {
    let config_dir = setup_temp_config_dir("test_config_remove_spec_success");
    let spec_path = config_dir.join("specs").join("my-api.yaml");
    fs::create_dir_all(spec_path.parent().unwrap()).unwrap();
    fs::write(
        &spec_path,
        "openapi: 3.0.0\ninfo:\n  title: Test API\n  version: 1.0.0\npaths: {}",
    )
    .unwrap();

    get_bin()
        .arg("config")
        .arg("remove")
        .arg("my-api")
        .env("APERTURE_CONFIG_DIR", &config_dir)
        .assert()
        .success()
        .stdout(predicate::str::contains(
            "Spec 'my-api' removed successfully.",
        ));

    assert!(!spec_path.exists());
}

#[test]
fn test_config_remove_spec_not_found() {
    let config_dir = setup_temp_config_dir("test_config_remove_spec_not_found");

    get_bin()
        .arg("config")
        .arg("remove")
        .arg("non-existent-api")
        .env("APERTURE_CONFIG_DIR", &config_dir)
        .assert()
        .failure()
        .stderr(predicate::str::contains(
            "API specification 'non-existent-api' not found",
        ));
}

#[test]
fn test_config_edit_spec_success() {
    let config_dir = setup_temp_config_dir("test_config_edit_spec_success");
    let spec_path = config_dir.join("specs").join("my-api.yaml");
    fs::create_dir_all(spec_path.parent().unwrap()).unwrap();
    fs::write(
        &spec_path,
        "openapi: 3.0.0\ninfo:\n  title: Test API\n  version: 1.0.0\npaths: {}",
    )
    .unwrap();

    // Mock the EDITOR environment variable to a simple command that just succeeds
    // On Unix-like systems, `true` is a command that always exits with 0.
    // On Windows, `cmd /c exit 0` can be used.
    let editor_cmd = if cfg!(windows) {
        "cmd /c exit 0"
    } else {
        "true"
    };

    get_bin()
        .arg("config")
        .arg("edit")
        .arg("my-api")
        .env("APERTURE_CONFIG_DIR", &config_dir)
        .env("EDITOR", editor_cmd)
        .assert()
        .success()
        .stdout(predicate::str::contains("Opened spec 'my-api' in editor."));
}

#[test]
fn test_config_edit_spec_not_found() {
    let config_dir = setup_temp_config_dir("test_config_edit_spec_not_found");

    get_bin()
        .arg("config")
        .arg("edit")
        .arg("non-existent-api")
        .env("APERTURE_CONFIG_DIR", &config_dir)
        .assert()
        .failure()
        .stderr(predicate::str::contains(
            "API specification 'non-existent-api' not found",
        ));
}

#[test]
fn test_config_edit_no_editor_env() {
    let config_dir = setup_temp_config_dir("test_config_edit_no_editor_env");
    let spec_path = config_dir.join("specs").join("my-api.yaml");
    fs::create_dir_all(spec_path.parent().unwrap()).unwrap();
    fs::write(
        &spec_path,
        "openapi: 3.0.0\ninfo:\n  title: Test API\n  version: 1.0.0\npaths: {}",
    )
    .unwrap();

    get_bin()
        .arg("config")
        .arg("edit")
        .arg("my-api")
        .env("APERTURE_CONFIG_DIR", &config_dir)
        .env_remove("EDITOR") // Ensure EDITOR is not set
        .assert()
        .failure()
        .stderr(predicate::str::contains(
            "EDITOR environment variable not set",
        ));
}
