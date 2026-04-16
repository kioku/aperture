mod common;

use aperture_cli::cache::models::{
    CachedCommand, CachedParameter, CachedResponse, CachedSpec, PaginationInfo,
};
use aperture_cli::constants;
use common::aperture_cmd;
use predicates::prelude::*;
use std::collections::HashMap;
use std::fs;
use tempfile::TempDir;

fn write_completion_fixture() -> TempDir {
    let temp_dir = TempDir::new().expect("temp dir should be created");
    let config_dir = temp_dir.path();

    let specs_dir = config_dir.join(constants::DIR_SPECS);
    let cache_dir = config_dir.join(constants::DIR_CACHE);

    fs::create_dir_all(&specs_dir).expect("specs directory should be created");
    fs::create_dir_all(&cache_dir).expect("cache directory should be created");

    fs::write(
        specs_dir.join("petstore.yaml"),
        "openapi: 3.0.0\ninfo:\n  title: Petstore\n  version: 1.0.0\npaths: {}\n",
    )
    .expect("spec placeholder should be written");

    let cached_spec = CachedSpec {
        cache_format_version: aperture_cli::cache::models::CACHE_FORMAT_VERSION,
        name: "petstore".to_string(),
        version: "1.0.0".to_string(),
        commands: vec![CachedCommand {
            name: "users".to_string(),
            description: Some("User operations".to_string()),
            summary: None,
            operation_id: "getUserById".to_string(),
            method: constants::HTTP_METHOD_GET.to_string(),
            path: "/users/{userId}".to_string(),
            parameters: vec![
                CachedParameter {
                    name: "userId".to_string(),
                    location: constants::PARAM_LOCATION_PATH.to_string(),
                    required: true,
                    description: None,
                    schema: Some(r#"{"type":"string"}"#.to_string()),
                    schema_type: Some(constants::SCHEMA_TYPE_STRING.to_string()),
                    format: None,
                    default_value: None,
                    enum_values: vec![],
                    example: None,
                },
                CachedParameter {
                    name: "limit".to_string(),
                    location: constants::PARAM_LOCATION_QUERY.to_string(),
                    required: false,
                    description: None,
                    schema: Some(r#"{"type":"integer"}"#.to_string()),
                    schema_type: Some(constants::SCHEMA_TYPE_INTEGER.to_string()),
                    format: None,
                    default_value: None,
                    enum_values: vec![],
                    example: None,
                },
            ],
            request_body: None,
            responses: vec![CachedResponse {
                status_code: "200".to_string(),
                description: None,
                content_type: Some(constants::CONTENT_TYPE_JSON.to_string()),
                schema: Some(r#"{"type":"object"}"#.to_string()),
                example: None,
            }],
            security_requirements: vec![],
            tags: vec!["users".to_string()],
            deprecated: false,
            external_docs_url: None,
            examples: vec![],
            display_group: None,
            display_name: None,
            aliases: vec![],
            hidden: false,
            pagination: PaginationInfo::default(),
        }],
        base_url: Some("https://api.example.com".to_string()),
        servers: vec!["https://api.example.com".to_string()],
        security_schemes: HashMap::new(),
        skipped_endpoints: vec![],
        server_variables: HashMap::new(),
    };

    let bytes = postcard::to_allocvec(&cached_spec).expect("cache should serialize");
    fs::write(cache_dir.join("petstore.bin"), bytes).expect("cache file should be written");

    temp_dir
}

#[test]
fn completion_command_generates_bash_script() {
    aperture_cmd()
        .args(["completion", "bash"])
        .assert()
        .success()
        .stdout(predicate::str::contains("aperture __complete bash"))
        .stdout(predicate::str::contains(
            "complete -F _aperture_completion aperture",
        ));
}

#[test]
fn completion_command_generates_nu_script() {
    aperture_cmd()
        .args(["completion", "nu"])
        .assert()
        .success()
        .stdout(predicate::str::contains("aperture __complete nu"))
        .stdout(predicate::str::contains(
            "upsert completions.external.completer",
        ));
}

#[test]
fn completion_command_generates_powershell_script_with_documented_shell_name() {
    aperture_cmd()
        .args(["completion", "powershell"])
        .assert()
        .success()
        .stdout(predicate::str::contains(
            "aperture __complete powershell $cword",
        ))
        .stdout(predicate::str::contains("Register-ArgumentCompleter"));
}

#[test]
fn runtime_completion_suggests_contexts_groups_operations_and_flags() {
    let fixture = write_completion_fixture();

    aperture_cmd()
        .env(constants::ENV_APERTURE_CONFIG_DIR, fixture.path())
        .args(["__complete", "bash", "2", "aperture", "api", "p"])
        .assert()
        .success()
        .stdout(predicate::str::contains("petstore"));

    aperture_cmd()
        .env(constants::ENV_APERTURE_CONFIG_DIR, fixture.path())
        .args([
            "__complete",
            "bash",
            "3",
            "aperture",
            "api",
            "petstore",
            "u",
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains("users"));

    aperture_cmd()
        .env(constants::ENV_APERTURE_CONFIG_DIR, fixture.path())
        .args([
            "__complete",
            "bash",
            "4",
            "aperture",
            "api",
            "petstore",
            "users",
            "g",
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains("get-user-by-id"));

    aperture_cmd()
        .env(constants::ENV_APERTURE_CONFIG_DIR, fixture.path())
        .args([
            "__complete",
            "bash",
            "5",
            "aperture",
            "api",
            "petstore",
            "users",
            "get-user-by-id",
            "--",
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains("--user-id"))
        .stdout(predicate::str::contains("--header"));
}

#[test]
fn runtime_completion_accepts_nu_shell_name() {
    let fixture = write_completion_fixture();

    aperture_cmd()
        .env(constants::ENV_APERTURE_CONFIG_DIR, fixture.path())
        .args(["__complete", "nu", "2", "aperture", "api", "p"])
        .assert()
        .success()
        .stdout(predicate::str::contains("petstore"));
}

#[test]
fn runtime_completion_accepts_documented_powershell_shell_name() {
    let fixture = write_completion_fixture();

    aperture_cmd()
        .env(constants::ENV_APERTURE_CONFIG_DIR, fixture.path())
        .args(["__complete", "powershell", "2", "aperture", "api", "p"])
        .assert()
        .success()
        .stdout(predicate::str::contains("petstore"));
}

#[test]
fn runtime_completion_tolerates_missing_execution_flag_value() {
    let fixture = write_completion_fixture();

    aperture_cmd()
        .env(constants::ENV_APERTURE_CONFIG_DIR, fixture.path())
        .args([
            "__complete",
            "bash",
            "4",
            "aperture",
            "api",
            "petstore",
            "--format",
            "",
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains("users"))
        .stdout(predicate::str::contains("--dry-run"));
}
