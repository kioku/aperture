use assert_cmd::Command;
use tempfile::TempDir;

fn run(root: &std::path::Path, args: &[&str]) -> assert_cmd::assert::Assert {
    Command::new(assert_cmd::cargo::cargo_bin!("aperture"))
        .env("APERTURE_CONFIG_DIR", root)
        .args(args)
        .assert()
}

#[test]
fn mutation_json_and_parser_guards() {
    let root = TempDir::new().unwrap();
    let source = root.path().join("input.md");
    std::fs::write(&source, "# Private content").unwrap();
    let result = run(
        root.path(),
        &[
            "skills",
            "install",
            source.to_str().unwrap(),
            "--name",
            "release",
            "--json",
        ],
    )
    .success();
    let value: serde_json::Value = serde_json::from_slice(&result.get_output().stdout).unwrap();
    assert_eq!(
        value,
        serde_json::json!({"status":"installed","name":"release"})
    );
    for args in [
        vec!["skills", "install", "# New", "--name", "release", "--json"],
        vec!["skills", "uninstall", "core", "--json"],
        vec!["skills", "uninstall", "release", "--json", "--json"],
        vec!["skills", "uninstall", "release", "--unknown"],
        vec!["skills", "uninstall", "release", "--json=true"],
        vec![
            "skills", "install", "# New", "--name", "other", "--json", "--json",
        ],
        vec![
            "skills",
            "install",
            "# New",
            "--name",
            "other",
            "--json=true",
        ],
        vec!["skills", "install", "# New", "--name", "other", "--unknown"],
        vec!["skills", "install", "# New", "--name", "core", "--json"],
    ] {
        run(root.path(), &args).failure();
    }
    let result = run(root.path(), &["skills", "uninstall", "release", "--json"]).success();
    assert_eq!(
        serde_json::from_slice::<serde_json::Value>(&result.get_output().stdout).unwrap(),
        serde_json::json!({"status":"uninstalled","name":"release"})
    );
}

#[cfg(unix)]
#[test]
fn trusted_alias_roots_and_untrusted_entries() {
    use std::os::unix::fs::symlink;
    let temp = TempDir::new().unwrap();
    let real = temp.path().join("real");
    std::fs::create_dir(&real).unwrap();
    let alias = temp.path().join("alias");
    symlink(&real, &alias).unwrap();
    let config = alias.join("config");
    std::fs::create_dir(real.join("config")).unwrap();
    run(&config, &["skills", "get", "core", "--json"]).success();
    assert!(!real.join("config/skills").exists());
    let file = alias.join("input.md");
    std::fs::write(&file, "# Release").unwrap();
    let directory = alias.join("source");
    std::fs::create_dir(&directory).unwrap();
    std::fs::write(directory.join("SKILL.md"), "# Release").unwrap();
    for source in [&file, &directory] {
        run(
            &config,
            &[
                "skills",
                "install",
                source.to_str().unwrap(),
                "--name",
                "release",
            ],
        )
        .success();
        run(&config, &["skills", "get", "release", "--full"]).success();
        run(
            &config,
            &[
                "skills",
                "install",
                source.to_str().unwrap(),
                "--name",
                "release",
                "--replace",
            ],
        )
        .success();
        run(&config, &["skills", "uninstall", "release"]).success();
    }
    let library = real.join("config/skills");
    let selected = real.join("selected-library");
    symlink(&library, &selected).unwrap();
    run(
        &config,
        &[
            "config",
            "set",
            "skills.directory",
            selected.to_str().unwrap(),
        ],
    )
    .success();
    run(
        &config,
        &["skills", "install", "# Release", "--name", "release"],
    )
    .success();
    let canary = real.join("canary");
    std::fs::write(&canary, "unchanged").unwrap();
    for entry in ["reference.md", ".aperture-skill.json"] {
        let path = library.join("release").join(entry);
        if path.exists() {
            std::fs::remove_file(&path).unwrap();
        }
        symlink(&canary, &path).unwrap();
        run(&config, &["skills", "get", "release", "--full"]).failure();
        run(&config, &["skills", "uninstall", "release"]).failure();
        std::fs::remove_file(path).unwrap();
    }
    std::fs::remove_file(library.join(".aperture.lock")).unwrap();
    symlink(&canary, library.join(".aperture.lock")).unwrap();
    run(
        &config,
        &["skills", "install", "# Other", "--name", "other"],
    )
    .failure();
    assert_unsafe_sources(&config, &alias, &real, &file, &directory, &canary);
    assert_eq!(std::fs::read_to_string(canary).unwrap(), "unchanged");
}

#[cfg(unix)]
fn assert_unsafe_sources(
    config: &std::path::Path,
    alias: &std::path::Path,
    real: &std::path::Path,
    file: &std::path::Path,
    directory: &std::path::Path,
    canary: &std::path::Path,
) {
    use std::os::unix::fs::symlink;
    let final_alias = alias.join("final.md");
    symlink(file, &final_alias).unwrap();
    run(
        config,
        &[
            "skills",
            "install",
            final_alias.to_str().unwrap(),
            "--name",
            "other",
        ],
    )
    .failure();
    let directory_alias = alias.join("final-directory");
    symlink(directory, &directory_alias).unwrap();
    for spelling in [
        directory_alias.to_string_lossy().into_owned(),
        format!("{}/", directory_alias.display()),
    ] {
        run(config, &["skills", "install", &spelling, "--name", "other"]).failure();
    }
    let dangling = alias.join("dangling");
    symlink(real.join("missing"), &dangling).unwrap();
    run(
        config,
        &[
            "skills",
            "install",
            dangling.to_str().unwrap(),
            "--name",
            "other",
        ],
    )
    .failure();
    symlink(canary, directory.join("nested.md")).unwrap();
    run(
        config,
        &[
            "skills",
            "install",
            directory.to_str().unwrap(),
            "--name",
            "other",
        ],
    )
    .failure();
    run(
        config,
        &[
            "config",
            "set",
            "skills.directory",
            dangling.to_str().unwrap(),
        ],
    )
    .success();
    run(config, &["skills", "get", "core"]).failure();
}

#[cfg(unix)]
#[test]
fn nested_skill_and_dangling_reference_cannot_escape() {
    use std::os::unix::fs::symlink;
    let root = TempDir::new().unwrap();
    let outside = TempDir::new().unwrap();
    let canary = outside.path().join("SKILL.md");
    std::fs::write(&canary, "# Unchanged").unwrap();
    run(
        root.path(),
        &["skills", "install", "# Local", "--name", "local"],
    )
    .success();
    let library = root.path().join("skills");
    symlink(outside.path(), library.join("escape")).unwrap();
    run(root.path(), &["skills", "get", "escape", "--full"]).failure();
    run(root.path(), &["skills", "uninstall", "escape", "--json"]).failure();
    run(
        root.path(),
        &[
            "skills",
            "install",
            "# New",
            "--name",
            "escape",
            "--replace",
            "--json",
        ],
    )
    .failure();
    symlink(
        outside.path().join("missing"),
        library.join("local/dangling.md"),
    )
    .unwrap();
    run(root.path(), &["skills", "get", "local", "--full"]).failure();
    run(root.path(), &["skills", "uninstall", "local"]).failure();
    run(root.path(), &["skills", "get", "core"]).success();
    assert_eq!(std::fs::read_to_string(canary).unwrap(), "# Unchanged");
    assert!(!outside.path().join("missing").exists());
}

#[test]
fn every_embedded_example_command_executes() {
    let root = TempDir::new().unwrap();
    let spec = root.path().join("fixture.yaml");
    std::fs::write(
        &spec,
        "openapi: 3.0.0\ninfo:\n  title: Local\n  version: '1'\npaths: {}\n",
    )
    .unwrap();
    for api in ["source", "tracker"] {
        run(
            root.path(),
            &["config", "api", "add", api, spec.to_str().unwrap()],
        )
        .success();
    }
    run(
        root.path(),
        &["skills", "install", "# Workflow", "--name", "workflow"],
    )
    .success();
    for document in [
        include_str!("../src/skills/core.md"),
        include_str!("../skills/aperture/SKILL.md"),
        include_str!("../skills/examples/release.md"),
    ] {
        for example in document.split('`').skip(1).step_by(2) {
            if let Some(command) = example.strip_prefix("aperture ") {
                let command = command
                    .replace("<api>", "source")
                    .replace("<name>", "workflow");
                run(root.path(), &command.split_whitespace().collect::<Vec<_>>()).success();
            }
        }
    }
}

#[test]
fn bundled_guidance_and_completion_match_cli() {
    let root = TempDir::new().unwrap();
    let core = include_str!("../src/skills/core.md");
    assert!(core.contains("`aperture config api list`"));
    assert!(core.contains("`aperture commands <api>`"));
    assert!(!core.contains("`aperture list-commands`"));
    for args in [
        vec!["config", "api", "list"],
        vec!["build-info", "--json"],
        vec!["--help"],
        vec!["skills", "list", "--json"],
        vec!["skills", "get", "core", "--full"],
    ] {
        run(root.path(), &args).success();
    }
    for command in ["api", "run"] {
        assert!(core.contains(&format!("`aperture {command} --help`")));
        run(root.path(), &[command, "--help"])
            .success()
            .stdout(predicates::str::contains("--batch-file"));
    }
    std::fs::write(root.path().join("specs"), "corrupt").unwrap();
    for (args, expected) in [
        (vec!["__complete", "bash", "1", "aperture", "sk"], "skills"),
        (
            vec!["__complete", "bash", "2", "aperture", "skills", "in"],
            "install",
        ),
        (
            vec![
                "__complete",
                "bash",
                "3",
                "aperture",
                "skills",
                "get",
                "--f",
            ],
            "--full",
        ),
        (
            vec![
                "__complete",
                "bash",
                "3",
                "aperture",
                "skills",
                "install",
                "--r",
            ],
            "--replace",
        ),
        (
            vec![
                "__complete",
                "bash",
                "3",
                "aperture",
                "skills",
                "uninstall",
                "--j",
            ],
            "--json",
        ),
    ] {
        run(root.path(), &args)
            .success()
            .stdout(predicates::str::contains(expected));
    }
    run(root.path(), &["completion", "bash"]).success();
    assert!(!root.path().join("skills").exists());
}
