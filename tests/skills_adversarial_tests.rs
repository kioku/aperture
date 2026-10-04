use assert_cmd::Command;
use serde_json::Value;
use std::{fs, path::Path};
use tempfile::TempDir;

fn cli(root: &TempDir) -> Command {
    let mut cmd = Command::new(assert_cmd::cargo::cargo_bin!("aperture"));
    cmd.env("APERTURE_CONFIG_DIR", root.path());
    cmd
}
fn install(root: &TempDir, source: &Path) {
    cli(root)
        .args(["skills", "install"])
        .arg(source)
        .assert()
        .success();
}
fn json(root: &TempDir, args: &[&str]) -> Value {
    let output = cli(root)
        .args(args)
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    serde_json::from_slice(&output).unwrap()
}
fn directory() -> TempDir {
    let source = TempDir::new().unwrap();
    fs::write(
        source.path().join("SKILL.md"),
        "---\nname: release\n---\n# Release\n",
    )
    .unwrap();
    source
}

#[test]
fn directory_full_binary_provenance_and_determinism() {
    let root = TempDir::new().unwrap();
    let source = directory();
    fs::create_dir(source.path().join("templates")).unwrap();
    fs::write(source.path().join("templates/a.md"), "reference").unwrap();
    fs::write(source.path().join("templates/b.bin"), [0xff, 0x00]).unwrap();
    install(&root, source.path());
    let result = json(&root, &["skills", "get", "release", "--full", "--json"]);
    assert_eq!(result["files"]["templates/b.bin"]["encoding"], "base64");
    assert_eq!(result["files"]["templates/a.md"]["content"], "reference");
    assert_eq!(result["provenance"]["source_type"], "directory");
    assert_eq!(
        result["provenance"]["content_sha256"]
            .as_str()
            .unwrap()
            .len(),
        64
    );
    assert_eq!(
        result,
        json(&root, &["skills", "get", "release", "--full", "--json"])
    );
    let default = cli(&root)
        .args(["skills"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let list = cli(&root)
        .args(["skills", "list"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    assert_eq!(default, list);
}

#[test]
fn manually_placed_skills_are_read_only_and_selected_corruption_is_explicit() {
    let root = TempDir::new().unwrap();
    let manual = root.path().join("skills/manual");
    fs::create_dir_all(&manual).unwrap();
    fs::write(manual.join("SKILL.md"), "# Manual").unwrap();
    cli(&root)
        .args(["skills", "get", "manual", "--full"])
        .assert()
        .success();
    cli(&root)
        .args(["skills", "uninstall", "manual"])
        .assert()
        .failure();
    cli(&root)
        .args([
            "skills",
            "install",
            "# Replace",
            "--name",
            "manual",
            "--replace",
        ])
        .assert()
        .failure();
    assert_eq!(
        fs::read_to_string(manual.join("SKILL.md")).unwrap(),
        "# Manual"
    );
    fs::write(
        manual.join("SKILL.md"),
        "---\nname: [broken]\n---\n# Manual",
    )
    .unwrap();
    cli(&root)
        .args(["skills", "get", "manual", "--json"])
        .assert()
        .failure();
    cli(&root)
        .args(["skills", "get", "core"])
        .assert()
        .success();
}

#[test]
fn import_failures_preserve_existing_skill() {
    let root = TempDir::new().unwrap();
    let source = directory();
    install(&root, source.path());
    let before = fs::read(root.path().join("skills/release/SKILL.md")).unwrap();
    fs::write(
        source.path().join("oversized.md"),
        vec![b'x'; 1024 * 1024 + 1],
    )
    .unwrap();
    cli(&root)
        .args(["skills", "install"])
        .arg(source.path())
        .arg("--replace")
        .assert()
        .failure();
    assert_eq!(
        fs::read(root.path().join("skills/release/SKILL.md")).unwrap(),
        before
    );
    fs::remove_file(source.path().join("oversized.md")).unwrap();
    fs::write(source.path().join(".aperture-skill.json"), "{}").unwrap();
    cli(&root)
        .args(["skills", "install"])
        .arg(source.path())
        .arg("--replace")
        .assert()
        .failure();
    assert_eq!(
        fs::read(root.path().join("skills/release/SKILL.md")).unwrap(),
        before
    );
}

#[test]
fn malformed_and_empty_known_fields_do_not_fallback() {
    let root = TempDir::new().unwrap();
    for yaml in [
        "name: null",
        "description: null",
        "name: ''",
        "description: ''",
        "aperture: null",
        "aperture: []",
        "aperture:\n  required_apis: [api, api]",
    ] {
        let text = format!("---\n{yaml}\n---\n# Content");
        cli(&root)
            .args(["skills", "install", &text, "--name", "sample"])
            .assert()
            .failure();
    }
    cli(&root)
        .args(["skills", "install", "---\nname: sample\n---\n"])
        .assert()
        .failure();
}

#[test]
fn file_fallback_and_invalid_utf8_and_parser_values() {
    let root = TempDir::new().unwrap();
    let source = TempDir::new().unwrap();
    let path = source.path().join("release.md");
    fs::write(&path, "# Release").unwrap();
    install(&root, &path);
    assert_eq!(
        json(&root, &["skills", "get", "release", "--json"])["description"],
        "Release"
    );
    fs::write(&path, [0xff]).unwrap();
    cli(&root)
        .args(["skills", "install"])
        .arg(&path)
        .arg("--replace")
        .assert()
        .failure();
    for args in [
        vec!["skills", "get"],
        vec!["skills", "get", "core", "--all"],
        vec!["skills", "install", "# X", "--unknown"],
        vec!["skills", "install", "# X", "--name", "a", "--name", "b"],
        vec!["skills", "list", "--full"],
    ] {
        cli(&root).args(args).assert().failure();
    }
}

#[test]
fn concurrent_imports_serialize_without_partial_results() {
    let root = TempDir::new().unwrap();
    let path = root.path().to_path_buf();
    let mut workers = Vec::with_capacity(6);
    // Spawn every writer before joining: serial spawn/join would miss lock races.
    for _ in 0..6 {
        let path = path.clone();
        workers.push(std::thread::spawn(move || {
            std::process::Command::new(assert_cmd::cargo::cargo_bin!("aperture"))
                .env("APERTURE_CONFIG_DIR", path)
                .args(["skills", "install", "# Concurrent", "--name", "same"])
                .output()
                .unwrap()
                .status
                .success()
        }));
    }
    assert_eq!(
        workers
            .into_iter()
            .filter_map(|worker| worker.join().ok())
            .filter(|success| *success)
            .count(),
        1
    );
    cli(&root)
        .args(["skills", "get", "same", "--full", "--json"])
        .assert()
        .success();
    assert!(fs::read_dir(root.path().join("skills"))
        .unwrap()
        .all(|entry| !entry
            .unwrap()
            .file_name()
            .to_string_lossy()
            .starts_with(".skill-stage-")));
}

#[cfg(unix)]
#[test]
fn symlinks_and_non_utf8_paths_are_rejected() {
    use std::{
        ffi::OsString,
        os::unix::{ffi::OsStringExt, fs::symlink},
    };
    let root = TempDir::new().unwrap();
    let source = directory();
    let outside = TempDir::new().unwrap();
    fs::write(outside.path().join("private"), "do not read").unwrap();
    symlink(outside.path().join("private"), source.path().join("escape")).unwrap();
    cli(&root)
        .args(["skills", "install"])
        .arg(source.path())
        .assert()
        .failure();
    fs::remove_file(source.path().join("escape")).unwrap();
    fs::write(source.path().join(OsString::from_vec(vec![0xff])), "x").unwrap();
    cli(&root)
        .args(["skills", "install"])
        .arg(source.path())
        .assert()
        .failure();
    let library = root.path().join("skills");
    fs::create_dir(&library).unwrap();
    symlink(outside.path(), library.join("release")).unwrap();
    cli(&root)
        .args(["skills", "install", "# X", "--name", "release", "--replace"])
        .assert()
        .failure();
    cli(&root)
        .args(["skills", "uninstall", "release"])
        .assert()
        .failure();
    assert_eq!(
        fs::read_to_string(outside.path().join("private")).unwrap(),
        "do not read"
    );
}

#[test]
fn corrupt_installer_metadata_never_selects_deletion_targets() {
    let root = TempDir::new().unwrap();
    cli(&root)
        .args(["skills", "install", "# Original", "--name", "release"])
        .assert()
        .success();
    let target = root.path().join("skills/release");
    let mut metadata: Value =
        serde_json::from_slice(&fs::read(target.join(".aperture-skill.json")).unwrap()).unwrap();
    metadata["name"] = Value::String("../outside".into());
    fs::write(
        target.join(".aperture-skill.json"),
        serde_json::to_vec(&metadata).unwrap(),
    )
    .unwrap();
    cli(&root)
        .args(["skills", "uninstall", "release"])
        .assert()
        .failure();
    cli(&root)
        .args([
            "skills",
            "install",
            "# Replace",
            "--name",
            "release",
            "--replace",
        ])
        .assert()
        .failure();
    assert_eq!(
        fs::read_to_string(target.join("SKILL.md")).unwrap(),
        "# Original"
    );
}

#[test]
fn directory_size_depth_and_case_collisions_are_rejected() {
    let root = TempDir::new().unwrap();
    let source = directory();
    for i in 0..129 {
        fs::write(source.path().join(format!("file{i}.txt")), "x").unwrap();
    }
    cli(&root)
        .args(["skills", "install"])
        .arg(source.path())
        .assert()
        .failure();
    let deep = directory();
    let mut path = deep.path().to_path_buf();
    for _ in 0..17 {
        path.push("nested");
        fs::create_dir(&path).unwrap();
    }
    cli(&root)
        .args(["skills", "install"])
        .arg(deep.path())
        .assert()
        .failure();
    let total = directory();
    for i in 0..9 {
        fs::write(
            total.path().join(format!("file{i}.txt")),
            vec![b'x'; 1024 * 1024],
        )
        .unwrap();
    }
    cli(&root)
        .args(["skills", "install"])
        .arg(total.path())
        .assert()
        .failure();
    let reserved = directory();
    fs::write(reserved.path().join("CON.txt"), "x").unwrap();
    cli(&root)
        .args(["skills", "install"])
        .arg(reserved.path())
        .assert()
        .failure();
    #[cfg(unix)]
    {
        let collision = directory();
        fs::create_dir(collision.path().join("Refs")).unwrap();
        fs::create_dir(collision.path().join("refs")).unwrap();
        fs::write(collision.path().join("Refs/a.md"), "x").unwrap();
        fs::write(collision.path().join("refs/b.md"), "x").unwrap();
        cli(&root)
            .args(["skills", "install"])
            .arg(collision.path())
            .assert()
            .failure();
    }
}
