use assert_cmd::Command;
use tempfile::TempDir;

fn cli(root: &TempDir) -> Command {
    let mut cmd = Command::new(assert_cmd::cargo::cargo_bin!("aperture"));
    cmd.env("APERTURE_CONFIG_DIR", root.path());
    cmd
}

#[test]
fn skills_lifecycle_and_protection() {
    let root = TempDir::new().unwrap();
    cli(&root)
        .args([
            "skills",
            "install",
            "# Release\nSafe instructions",
            "--name",
            "release",
        ])
        .assert()
        .success();
    cli(&root)
        .args(["skills", "get", "release", "--json"])
        .assert()
        .success()
        .stdout(predicates::str::contains("installed"));
    cli(&root)
        .args(["skills", "install", "# Replacement", "--name", "release"])
        .assert()
        .failure();
    cli(&root)
        .args([
            "skills",
            "install",
            "# Replacement",
            "--name",
            "release",
            "--replace",
        ])
        .assert()
        .success();
    cli(&root)
        .args(["skills", "get", "--all", "--full", "--json"])
        .assert()
        .success();
    cli(&root)
        .args(["skills", "uninstall", "release"])
        .assert()
        .success();
    for name in ["core", "CORE", "../escape", "CON", "nul", "com1"] {
        cli(&root)
            .args(["skills", "install", "# Unsafe", "--name", name])
            .assert()
            .failure();
    }
    cli(&root)
        .args(["skills", "uninstall", "core"])
        .assert()
        .failure();
}

#[test]
fn missing_paths_and_malformed_metadata_fail() {
    let root = TempDir::new().unwrap();
    for source in [
        "./missing.md",
        "missing/SKILL.md",
        "ftp://example.com/a",
        "https://user:secret@example.com/a",
        "---\nname: [invalid]\n---\n# Content",
        "---\nname: one\nname: two\n---\n# Content",
        "---\naperture:\n  required_apis: wrong\n---\n# Content",
        "",
    ] {
        cli(&root)
            .args(["skills", "install", source, "--name", "test"])
            .assert()
            .failure();
    }
    assert!(!root.path().join("skills/test").exists());
}

#[test]
fn config_and_stdin() {
    let root = TempDir::new().unwrap();
    cli(&root)
        .args(["config", "set", "skills.directory", "workflow-library"])
        .assert()
        .success();
    cli(&root)
        .args(["config", "get", "skills.directory"])
        .assert()
        .success()
        .stdout(predicates::str::contains("workflow-library"));
    cli(&root).args(["skills", "install", "-"]).write_stdin("---\nname: release\ndescription: Release safely\nunknown: accepted\naperture:\n  required_apis: [example]\n---\n# Release\n").assert().success();
    assert!(root
        .path()
        .join("workflow-library/release/SKILL.md")
        .exists());
    cli(&root)
        .args(["skills", "list", "--json"])
        .assert()
        .success()
        .stdout(predicates::str::contains("missing_apis"));
    assert!(!root.path().join("specs").exists());
}

#[test]
fn literal_markdown_with_urls_and_required_api_names() {
    let root = TempDir::new().unwrap();
    let markdown = "---\nname: workflow\naperture:\n  required_apis: [API.v2]\n---\n# Workflow\nRead https://example.com/docs";
    cli(&root)
        .args(["skills", "install", markdown])
        .assert()
        .success();
    cli(&root)
        .args(["skills", "list", "--json"])
        .assert()
        .success()
        .stdout(predicates::str::contains("API.v2"));
}

#[test]
fn required_apis_report_only_local_configuration() {
    let root = TempDir::new().unwrap();
    std::fs::create_dir(root.path().join("specs")).unwrap();
    std::fs::write(
        root.path().join("specs/local.yaml"),
        "openapi: 3.0.0\ninfo:\n  title: Local\n  version: '1'\npaths: {}\n",
    )
    .unwrap();
    cli(&root)
        .args([
            "skills",
            "install",
            "---\nname: workflow\naperture:\n  required_apis: [missing, local]\n---\n# Workflow",
        ])
        .assert()
        .success();
    let output = cli(&root)
        .args(["skills", "get", "workflow", "--json"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let value: serde_json::Value = serde_json::from_slice(&output).unwrap();
    assert_eq!(value["configured_apis"], serde_json::json!(["local"]));
    assert_eq!(value["missing_apis"], serde_json::json!(["missing"]));
    assert_eq!(value["readiness"], "local_configuration_only");
    assert!(!root.path().join("cache").exists());
    assert!(!root.path().join("config.toml").exists());
}

#[test]
fn invalid_library_directory_is_rejected_before_persistence() {
    let root = TempDir::new().unwrap();
    cli(&root)
        .args(["config", "set", "skills.directory", "valid-library"])
        .assert()
        .success();
    let original = std::fs::read(root.path().join("config.toml")).unwrap();
    for invalid in ["../outside", "nested/../../outside", "", " ", "~/skills"] {
        cli(&root)
            .args(["config", "set", "skills.directory", invalid])
            .assert()
            .failure();
        assert_eq!(
            std::fs::read(root.path().join("config.toml")).unwrap(),
            original
        );
    }
    cli(&root)
        .args(["skills", "get", "core", "--json"])
        .assert()
        .success();
}
