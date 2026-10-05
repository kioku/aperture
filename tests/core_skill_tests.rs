//! Verify availability and retrieval of the bundled skill, not its prose.
use assert_cmd::Command;
use tempfile::TempDir;

#[test]
fn bundled_core_is_available_without_companion_files() {
    let root = TempDir::new().unwrap();
    let config = root.path().join("absent-config");
    let output = Command::new(assert_cmd::cargo::cargo_bin!("aperture"))
        .current_dir(root.path())
        .env("APERTURE_CONFIG_DIR", &config)
        .args(["skills", "get", "core", "--full", "--json"])
        .assert()
        .success();
    let value: serde_json::Value = serde_json::from_slice(&output.get_output().stdout).unwrap();
    assert_eq!(value["name"], "core");
    assert_eq!(value["origin"], "bundled");
    assert!(value["content"].is_string());
    assert!(value["package_revision"].is_string());
    assert_eq!(value["files"], serde_json::json!({}));
    assert!(!config.join("skills").exists());
}
