mod common;

use common::aperture_cmd;
use std::{fs, process::Output};
use tempfile::TempDir;

fn fixture() -> TempDir {
    let dir = TempDir::new().unwrap();
    let source = dir.path().join("input.yaml");
    fs::write(&source, "openapi: 3.0.0\ninfo:\n  title: Selected API\n  version: 1.0.0\npaths:\n  /users:\n    get:\n      tags: [users]\n      operationId: listUsers\n      summary: List users\n      responses:\n        '200':\n          description: OK\n").unwrap();
    for name in ["selected", "unrelated"] {
        aperture_cmd()
            .env("APERTURE_CONFIG_DIR", dir.path())
            .args(["config", "api", "add", name, source.to_str().unwrap()])
            .assert()
            .success();
    }
    dir
}

fn run(dir: &TempDir, args: &[&str], format: &str) -> Output {
    aperture_cmd()
        .env("APERTURE_CONFIG_DIR", dir.path())
        .env("RUST_LOG", "warn")
        .env("NO_COLOR", "1")
        .args(args)
        .args(["--format", format])
        .output()
        .unwrap()
}

#[test]
fn scoped_discovery_ignores_broken_unrelated_cache_and_preserves_output() {
    let dir = fixture();
    let paths = [
        vec!["overview", "selected"],
        vec!["docs", "selected"],
        vec!["docs", "selected", "users", "list-users"],
        vec!["commands", "selected"],
        vec!["list-commands", "selected"],
    ];
    let unrelated = dir.path().join(".cache/unrelated.bin");
    let healthy = fs::read(&unrelated).unwrap();
    for format in ["text", "json"] {
        for args in &paths {
            fs::write(&unrelated, &healthy).unwrap();
            let before = run(&dir, args, format);
            assert!(before.status.success(), "{args:?}: {before:?}");
            fs::write(dir.path().join(".cache/unrelated.bin"), b"broken cache").unwrap();
            let after = run(&dir, args, format);
            assert!(after.status.success(), "{args:?}: {after:?}");
            assert_eq!(before.stdout, after.stdout);
            assert!(
                !String::from_utf8_lossy(&after.stderr).contains("unrelated"),
                "{args:?}: {after:?}"
            );
            assert_eq!(after.stdout, run(&dir, args, format).stdout);
            if format == "json" {
                serde_json::from_slice::<serde_json::Value>(&after.stdout).unwrap();
            }
        }
    }
}

#[test]
fn scoped_discovery_reports_selected_cache_errors_directly() {
    let dir = fixture();
    fs::write(dir.path().join(".cache/unrelated.bin"), b"broken cache").unwrap();
    for corrupt in [true, false] {
        let cache = dir.path().join(".cache/selected.bin");
        if corrupt {
            fs::write(&cache, b"broken selected cache").unwrap();
        } else {
            fs::remove_file(&cache).unwrap();
        }
        for format in ["text", "json"] {
            for args in [
                vec!["overview", "selected"],
                vec!["docs", "selected"],
                vec!["docs", "selected", "users", "list-users"],
                vec!["commands", "selected"],
            ] {
                let output = run(&dir, &args, format);
                assert!(!output.status.success());
                let stderr = String::from_utf8_lossy(&output.stderr);
                assert!(stderr.contains("selected"), "{output:?}");
                assert!(!stderr.contains("unrelated"), "{output:?}");
                assert!(!corrupt || stderr.contains("deserialize"), "{output:?}");
            }
        }
    }
}

#[test]
fn cross_api_discovery_still_reports_unrelated_failures() {
    let dir = fixture();
    fs::write(dir.path().join(".cache/unrelated.bin"), b"broken cache").unwrap();
    for format in ["text", "json"] {
        for args in [vec!["overview", "--all"], vec!["docs"]] {
            let output = run(&dir, &args, format);
            assert!(output.status.success());
            assert!(
                String::from_utf8_lossy(&output.stderr).contains("unrelated"),
                "{output:?}"
            );
            assert!(String::from_utf8_lossy(&output.stdout).contains("selected"));
        }
    }
}

#[test]
fn scoped_docs_preserves_mapped_aliases_and_hidden_operation_lookup() {
    let dir = fixture();
    let cache = dir.path().join(".cache/selected.bin");
    let mut spec: aperture_cli::cache::models::CachedSpec =
        postcard::from_bytes(&fs::read(&cache).unwrap()).unwrap();
    spec.commands[0].display_group = Some("people".into());
    spec.commands[0].display_name = Some("browse".into());
    spec.commands[0].aliases = vec!["fetch".into()];
    let mut hidden = spec.commands[0].clone();
    hidden.operation_id = "hiddenUsers".into();
    hidden.display_name = Some("hidden-users".into());
    hidden.aliases = vec!["secret-fetch".into()];
    hidden.hidden = true;
    spec.commands.push(hidden);
    fs::write(cache, postcard::to_allocvec(&spec).unwrap()).unwrap();
    fs::write(dir.path().join(".cache/unrelated.bin"), b"broken cache").unwrap();
    for format in ["text", "json"] {
        for operation in ["browse", "fetch", "hidden-users", "secret-fetch"] {
            let output = run(&dir, &["docs", "selected", "people", operation], format);
            assert!(output.status.success(), "{output:?}");
            assert!(!String::from_utf8_lossy(&output.stderr).contains("unrelated"));
        }
        for command in ["overview", "docs", "commands"] {
            let output = run(&dir, &[command, "selected"], format);
            assert!(output.status.success(), "{output:?}");
            let stdout = String::from_utf8_lossy(&output.stdout);
            assert!(!stdout.contains("hidden-users"), "{output:?}");
            if format == "json" {
                let value: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
                assert_eq!(value["api"]["operation_count"], 1);
            }
        }
    }
}
