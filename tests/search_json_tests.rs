mod common;
use common::aperture_cmd;
use serde_json::{json, Value};
use std::path::Path;
use tempfile::TempDir;

fn search(config: &Path, query: &str, extra: &[&str]) -> std::process::Output {
    aperture_cmd()
        .env("APERTURE_CONFIG_DIR", config)
        .env_remove("RUST_LOG")
        .args(["search", query])
        .args(extra)
        .output()
        .unwrap()
}

fn setup() -> TempDir {
    let temp = TempDir::new().unwrap();
    let spec = temp.path().join("spec.json");
    std::fs::write(&spec, serde_json::to_vec(&json!({
        "openapi":"3.0.0", "info":{"title":"Search fixture","version":"1"},
        "paths": {
            "/catalog":{"get":{"tags":["discovery"],"operationId":"getPublicModelCatalog","summary":"Public model catalog café \"quoted\"\nline","responses":{"200":{"description":"ok"}}}},
            "/bare":{"get":{"operationId":"bareOperation","responses":{"200":{"description":"ok"}}}}
        }
    })).unwrap()).unwrap();
    for name in ["alpha", "none", "zeta"] {
        aperture_cmd()
            .env("APERTURE_CONFIG_DIR", temp.path())
            .args(["config", "add", name, spec.to_str().unwrap()])
            .assert()
            .success();
    }
    temp
}

#[test]
fn json_search_serializes_actual_ranked_results() {
    let temp = setup();
    for query in [
        "public model catalog",
        "GET public model catalog",
        "regex:catalog",
        "bareOperation",
    ] {
        let output = search(temp.path(), query, &["--format", "json"]);
        assert!(output.status.success(), "{output:?}");
        let value: Value = serde_json::from_slice(&output.stdout).unwrap();
        assert_eq!(value["query"], query);
        assert_eq!(value["api_filter"], Value::Null);
        assert_eq!(value.as_object().unwrap().len(), 3);
        let results = value["results"].as_array().unwrap();
        assert_eq!(results.len(), 3);
        let specs = ["alpha", "none", "zeta"]
            .into_iter()
            .map(|name| {
                (
                    name.to_string(),
                    aperture_cli::engine::loader::load_cached_spec(
                        temp.path().join(aperture_cli::constants::DIR_CACHE),
                        name,
                    )
                    .unwrap(),
                )
            })
            .collect();
        let ranked = aperture_cli::search::CommandSearcher::new()
            .search(&specs, query, None)
            .unwrap();
        let expected: Vec<Value> = ranked
            .iter()
            .map(|result| {
                json!({
                    "api_context":result.api_context, "operation_id":result.command.operation_id,
                    "command_path":result.command_path, "method":result.command.method,
                    "path":result.command.path, "summary":result.command.summary,
                    "score":result.score, "highlights":result.highlights
                })
            })
            .collect();
        assert_eq!(results, &expected);
        assert_eq!(results[0]["api_context"], "alpha");
        assert_eq!(results[1]["api_context"], "none");
        assert_eq!(results[2]["api_context"], "zeta");
        assert_eq!(results[0].as_object().unwrap().len(), 8);
        assert!(results[0]["score"].is_i64());
        assert!(results[0]["highlights"].is_array());
        let scoped = search(
            temp.path(),
            query,
            &["--format", "json", "--api", "none", "--verbose"],
        );
        assert!(scoped.status.success());
        let scoped: Value = serde_json::from_slice(&scoped.stdout).unwrap();
        assert_eq!(scoped["api_filter"], "none");
        assert_eq!(scoped["results"], json!([results[1]]));
        if query == "bareOperation" {
            assert_eq!(results[0]["summary"], Value::Null);
        } else {
            assert_eq!(results[0]["operation_id"], "getPublicModelCatalog");
            assert_eq!(
                results[0]["command_path"],
                "discovery get-public-model-catalog"
            );
            assert_eq!(results[0]["method"], "GET");
            assert_eq!(results[0]["path"], "/catalog");
            assert_eq!(
                results[0]["summary"],
                "Public model catalog café \"quoted\"\nline"
            );
        }
    }
}

#[test]
fn json_empty_searches_and_errors_keep_data_contract() {
    let empty = TempDir::new().unwrap();
    let loaded = setup();
    for config in [empty.path(), loaded.path()] {
        for query in ["", " \t ", "unmatchable98765"] {
            let output = search(config, query, &["--format", "json"]);
            assert!(output.status.success(), "{output:?}");
            let value: Value = serde_json::from_slice(&output.stdout).unwrap();
            assert_eq!(value, json!({"query":query,"api_filter":null,"results":[]}));
        }
    }
    let missing = search(
        loaded.path(),
        "catalog",
        &["--format", "json", "--api", "missing"],
    );
    assert!(missing.status.success());
    assert_eq!(
        serde_json::from_slice::<Value>(&missing.stdout).unwrap(),
        json!({"query":"catalog","api_filter":"missing","results":[]})
    );
    for config in [empty.path(), loaded.path()] {
        let invalid = search(config, "regex:[", &["--format", "json"]);
        assert!(!invalid.status.success());
        assert!(invalid.stdout.is_empty());
    }
}

#[test]
fn format_parser_and_requested_data_flags() {
    use aperture_cli::cli::Cli;
    use clap::Parser;
    for args in [
        vec!["search", "catalog", "--format", "yaml"],
        vec!["search", "catalog", "--format", ""],
        vec!["search", "catalog", "--format", " "],
        vec!["search", "catalog", "--format", "json", "--format", "text"],
        vec!["search", "catalog", "--unknown"],
    ] {
        assert!(Cli::try_parse_from(std::iter::once("aperture").chain(args)).is_err());
    }
    let temp = setup();
    for flag in ["--quiet", "--json-errors"] {
        let result = search(temp.path(), "catalog", &["--format", "json", flag]);
        assert!(result.status.success());
        let value: Value = serde_json::from_slice(&result.stdout).unwrap();
        assert_eq!(value["results"].as_array().unwrap().len(), 3);
    }
    let invalid = search(
        temp.path(),
        "catalog",
        &["--format", "json", "--api", "bad/name"],
    );
    assert!(!invalid.status.success());
    assert!(invalid.stdout.is_empty());
    let default = search(temp.path(), "catalog", &[]);
    let text = search(temp.path(), "catalog", &["--format", "text"]);
    assert!(default.status.success());
    assert_eq!(default.stdout, text.stdout);
}

#[test]
fn unloadable_api_warns_on_stderr_and_returns_empty_json() {
    let temp = setup();
    let cache = temp
        .path()
        .join(aperture_cli::constants::DIR_CACHE)
        .join("alpha.bin");
    std::fs::write(cache, b"invalid cache").unwrap();
    let result = search(
        temp.path(),
        "catalog",
        &["--format", "json", "--api", "alpha"],
    );
    assert!(result.status.success());
    assert_eq!(
        serde_json::from_slice::<Value>(&result.stdout).unwrap(),
        json!({"query":"catalog","api_filter":"alpha","results":[]})
    );
    assert!(!result.stderr.is_empty());
}

#[cfg(unix)]
#[test]
fn json_search_closed_stdout_is_successful() {
    use std::os::fd::OwnedFd;
    use std::os::unix::net::UnixStream;
    use std::process::{Command, Stdio};
    let temp = setup();
    let (writer, reader) = UnixStream::pair().unwrap();
    drop(reader);
    let stdout = Stdio::from(OwnedFd::from(writer));
    let output = Command::new(env!("CARGO_BIN_EXE_aperture"))
        .env("APERTURE_CONFIG_DIR", temp.path())
        .args(["search", "catalog", "--format", "json"])
        .stdout(stdout)
        .output()
        .unwrap();
    assert!(output.status.success());
    assert!(output.stderr.is_empty());
}
