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
    let independent = handbook_example(CORE, "json").unwrap();
    let dependent = handbook_example(CORE, "yaml").unwrap();
    let batch: aperture_cli::batch::BatchFile = serde_yaml::from_str(&dependent).unwrap();
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

// Checkouts can use LF or CRLF. Extract by lines rather than matching a
// platform-specific newline sequence, and reject incomplete fenced examples.
fn handbook_example(document: &str, language: &str) -> Option<String> {
    let marker = format!("```{language}");
    let mut lines = document.lines().skip_while(|line| *line != marker);
    lines.next()?;
    let mut body = Vec::new();
    for line in lines {
        if line == "```" {
            return Some(body.join("\n"));
        }
        body.push(line);
    }
    None
}

#[test]
fn handbook_examples_support_lf_crlf_and_reject_incomplete_fences() {
    let lf = CORE.replace("\r\n", "\n");
    let crlf = lf.replace('\n', "\r\n");
    for language in ["json", "yaml"] {
        let expected = handbook_example(&lf, language).unwrap();
        assert_eq!(handbook_example(&crlf, language).unwrap(), expected);
        let batch: aperture_cli::batch::BatchFile = serde_yaml::from_str(&expected).unwrap();
        assert_eq!(batch.operations.len(), 2);
        aperture_cli::batch::graph::resolve_execution_order(&batch.operations).unwrap();
    }
    for malformed in ["", "{}", "```yaml\n{}\n```", "```json\n{}"] {
        assert!(handbook_example(malformed, "json").is_none());
    }
}

#[test]
fn root_help_directs_agents_to_the_bundled_handbook() {
    let root = TempDir::new().unwrap();
    let config = root.path().join("absent-config");
    for flag in ["-h", "--help"] {
        Command::new(assert_cmd::cargo::cargo_bin!("aperture"))
            .env("APERTURE_CONFIG_DIR", &config)
            .args([flag])
            .assert()
            .success()
            .stdout(predicates::str::contains("aperture skills get core --full"))
            .stdout(predicates::str::contains("aperture skills list --json"));
    }
    assert!(!config.exists());
}

#[test]
fn core_description_explains_when_and_why_to_use_it() {
    let frontmatter = CORE.split("---").nth(1).unwrap();
    let metadata: serde_yaml::Value = serde_yaml::from_str(frontmatter).unwrap();
    let description = metadata["description"].as_str().unwrap();
    assert!(description.starts_with("Use when "));
    assert!(description.contains("Aperture"));
    assert!(description.contains("to learn"));
}
