//! Keep the binary-distributed handbook complete and tied to the native CLI.
use aperture_cli::cli::Cli;
use assert_cmd::Command;
use clap::CommandFactory;
use tempfile::TempDir;

const CORE: &str = include_str!("../src/skills/core.md");

#[test]
fn bundled_core_teaches_the_complete_workflow() {
    for heading in [
        "## What Aperture does",
        "## Start here",
        "## Command map",
        "## Discover the right operation",
        "## Configure APIs and authentication",
        "## Construct and inspect requests",
        "## Read output and errors",
        "## Paginate complete collections",
        "## Compose batches and dependent workflows",
        "## Control retries, timeouts, proxies, and caches",
        "## Use and manage workflow skills",
        "## Troubleshoot and verify outcomes",
    ] {
        assert!(
            CORE.contains(heading),
            "Missing onboarding topic: {heading}"
        );
    }
    for command in Cli::command()
        .get_subcommands()
        .filter(|c| !c.is_hide_set())
    {
        if command.get_name() != "help" {
            let entry = format!("| `aperture {}` |", command.get_name());
            assert!(CORE.contains(&entry), "Missing native command: {entry}");
        }
    }
    for invariant in [
        "Batches are not transactions",
        "not authentication or remote health",
        "dry-run validates request construction, not response captures",
        "--body-file",
        "--output-file",
        "--auto-paginate",
        "--retry 0",
        "--force-retry",
        "APERTURE_CONFIG_DIR",
        "untrusted",
    ] {
        assert!(
            CORE.contains(invariant),
            "Missing operational guidance: {invariant}"
        );
    }
}

#[test]
fn binary_returns_the_entire_embedded_handbook_without_companion_files() {
    let temp = TempDir::new().unwrap();
    let config = temp.path().join("absent-config");
    let output = Command::new(assert_cmd::cargo::cargo_bin!("aperture"))
        .current_dir(temp.path())
        .env("APERTURE_CONFIG_DIR", &config)
        .args(["skills", "get", "core", "--full", "--json"])
        .assert()
        .success();
    let value: serde_json::Value = serde_json::from_slice(&output.get_output().stdout).unwrap();
    assert_eq!(value["origin"], "bundled");
    assert_eq!(value["content"], CORE);
    assert_eq!(value["files"], serde_json::json!({}));
    assert!(value["package_revision"].is_string());
    assert!(!config.join("skills").exists());
}

#[test]
fn handbook_requests_and_batches_match_generated_capabilities() {
    let root = TempDir::new().unwrap();
    let spec = root.path().join("openapi.yaml");
    std::fs::write(&spec, include_str!("fixtures/core_skill_api.yaml")).unwrap();
    let cli = |args: &[&str]| {
        Command::new(assert_cmd::cargo::cargo_bin!("aperture"))
            .current_dir(root.path())
            .env("APERTURE_CONFIG_DIR", root.path())
            .args(args)
            .assert()
            .success()
    };
    cli(&["config", "api", "add", "myapi", spec.to_str().unwrap()]);
    for args in [
        vec!["overview", "myapi", "--format", "json"],
        vec!["commands", "myapi", "--format", "json"],
        vec!["docs", "myapi", "items", "get-item", "--format", "json"],
        vec!["search", "GET item", "--api", "myapi", "--verbose"],
        vec!["search", "regex:(?i)item", "--api", "myapi"],
        vec!["api", "myapi", "--describe-json", "--jq", ".commands"],
        vec![
            "api",
            "--dry-run",
            "myapi",
            "items",
            "get-item",
            "--id",
            "123",
        ],
        vec![
            "run",
            "--api",
            "myapi",
            "--dry-run",
            "getItem",
            "--id",
            "123",
        ],
        vec![
            "api",
            "--dry-run",
            "myapi",
            "items",
            "create-item",
            "--body",
            "{\"name\":\"example\"}",
        ],
    ] {
        cli(&args);
    }
    std::fs::write(root.path().join("payload.json"), "{\"name\":\"example\"}").unwrap();
    cli(&[
        "api",
        "--dry-run",
        "--retry",
        "0",
        "myapi",
        "items",
        "create-item",
        "--body-file",
        "./payload.json",
    ]);
    let independent = CORE
        .split("```json\n")
        .nth(1)
        .unwrap()
        .split("```")
        .next()
        .unwrap();
    let dependent = CORE
        .split("```yaml\n")
        .nth(1)
        .unwrap()
        .split("```")
        .next()
        .unwrap();
    let batch: aperture_cli::batch::BatchFile = serde_yaml::from_str(dependent).unwrap();
    aperture_cli::batch::graph::resolve_execution_order(&batch.operations).unwrap();
    std::fs::write(root.path().join("operations.json"), independent).unwrap();
    let result = cli(&[
        "--json-errors",
        "api",
        "--dry-run",
        "--retry",
        "0",
        "--batch-file",
        "./operations.json",
        "--batch-concurrency",
        "2",
        "--batch-rate-limit",
        "5",
        "myapi",
    ]);
    let summary: serde_json::Value = serde_json::from_slice(&result.get_output().stdout).unwrap();
    assert_eq!(
        summary["batch_execution_summary"]["successful_operations"],
        2
    );
}
