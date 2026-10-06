//! Retry duration preparation must fail normally without delivery or waiting.
use serde_json::json;
use std::process::{Command, Output};
use wiremock::MockServer;

fn copy_config(source: &std::path::Path, destination: &std::path::Path) {
    if !source.exists() {
        return;
    }
    std::fs::create_dir_all(destination).unwrap();
    for entry in std::fs::read_dir(source).unwrap() {
        let entry = entry.unwrap();
        let target = destination.join(entry.file_name());
        if entry.file_type().unwrap().is_dir() {
            copy_config(&entry.path(), &target);
        } else {
            std::fs::copy(entry.path(), &target).unwrap();
            // Cached spec validity includes its source mtime. Keep that identity
            // while making an independent copy for the next child.
            std::fs::File::options()
                .write(true)
                .open(target)
                .unwrap()
                .set_times(
                    std::fs::FileTimes::new()
                        .set_modified(entry.metadata().unwrap().modified().unwrap()),
                )
                .unwrap();
        }
    }
}

fn cli(dir: &std::path::Path, args: &[&str]) -> Output {
    // Each child gets independent files; carry task-only fixture/cache state
    // forward by copying it, never by linking to another config directory.
    let child = tempfile::tempdir().unwrap();
    let config = child.path().join("config");
    copy_config(&dir.join("config"), &config);
    let output = Command::new(env!("CARGO_BIN_EXE_aperture"))
        .env("APERTURE_CONFIG_DIR", &config)
        .env_remove("APERTURE_LOG_FILE")
        .env_remove("APERTURE_LOG")
        .env_remove("APERTURE_LOG_FORMAT")
        .args(args)
        .output()
        .unwrap();
    copy_config(&config, &dir.join("config"));
    output
}

#[tokio::test]
async fn cli_and_batch_retry_boundaries_never_deliver_or_panic() {
    let server = MockServer::start().await;
    let spec = json!({
        "openapi": "3.0.3", "info": {"title": "Duration", "version": "1"},
        "servers": [{"url": server.uri()}],
        "paths": {"/": {"get": {"operationId": "get", "tags": ["probe"],
            "responses": {"200": {"description": "ok"}}}}}
    });
    for token in [
        "18446744073709551615m",
        "18446744073709552s",
        "307445734561826m",
        "",
        "   ",
        "bad",
        "1h",
        "none",
        "-1ms",
    ] {
        for flag in ["--retry-delay", "--retry-max-delay"] {
            let dir = tempfile::tempdir().unwrap();
            let path = dir.path().join("spec.json");
            std::fs::write(&path, spec.to_string()).unwrap();
            assert!(cli(
                dir.path(),
                &["config", "add", "probe", path.to_str().unwrap()]
            )
            .status
            .success());
            let result = cli(
                dir.path(),
                &[
                    "api",
                    "--no-proxy",
                    "--retry",
                    "1",
                    &format!("{flag}={token}"),
                    "--dry-run",
                    "--json-errors",
                    "probe",
                    "probe",
                    "get",
                ],
            );
            assert_eq!(result.status.code(), Some(1), "{flag} {token}: {result:?}");
            assert!(!String::from_utf8_lossy(&result.stderr).contains("panicked"));
            let error: serde_json::Value = serde_json::from_slice(&result.stderr).unwrap();
            assert!(error.is_object());
            let field = if flag == "--retry-delay" {
                "retry_delay"
            } else {
                "retry_max_delay"
            };
            let mut first = json!({"id": "invalid", "args": ["probe", "get"], "retry": 1});
            first[field] = json!(token);
            let batch = dir.path().join("batch.json");
            std::fs::write(
                &batch,
                json!({"operations": [first, {"id": "valid", "args": ["probe", "get"]}]})
                    .to_string(),
            )
            .unwrap();
            let result = cli(
                dir.path(),
                &[
                    "api",
                    "--no-proxy",
                    "--batch-file",
                    batch.to_str().unwrap(),
                    "--dry-run",
                    "--json-errors",
                    "probe",
                ],
            );
            assert_eq!(result.status.code(), Some(1), "{field} {token}: {result:?}");
            assert!(!String::from_utf8_lossy(&result.stderr).contains("panicked"));
            let summary: serde_json::Value = serde_json::from_slice(&result.stdout).unwrap();
            assert_eq!(
                summary["batch_execution_summary"]["operations"][0]["success"], false,
                "{summary}"
            );
            assert_eq!(
                summary["batch_execution_summary"]["operations"][1]["success"], true,
                "{summary}"
            );
        }
    }
    assert!(server.received_requests().await.unwrap().is_empty());
}
