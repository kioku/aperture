//! Caller-level batch diagnostics must not reflect arbitrary argument data.
use serde_json::json;
use std::process::{Command, Output};
use wiremock::{Mock, MockServer, ResponseTemplate};

fn cli(dir: &std::path::Path, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_aperture"))
        .env("APERTURE_CONFIG_DIR", dir.join("config"))
        .env("APERTURE_LOG", "trace")
        .env("APERTURE_LOG_FILE", dir.join("trace.jsonl"))
        .env("BATCH264_AUTH", "Bearer batch-secret-264")
        .args(args)
        .output()
        .unwrap()
}

#[tokio::test(flavor = "multi_thread")]
async fn batch_summaries_and_progress_omit_argument_and_error_reflections() {
    let server = MockServer::start().await;
    Mock::given(wiremock::matchers::path("/fail"))
        .respond_with(
            ResponseTemplate::new(401)
                .insert_header("x-reflection", "462-terces-hctab")
                .set_body_string("batch-secret-264"),
        )
        .mount(&server)
        .await;
    Mock::given(wiremock::matchers::path("/seed"))
        .respond_with(
            ResponseTemplate::new(200).set_body_json(json!({"token": "batch-secret-264"})),
        )
        .mount(&server)
        .await;
    let dir = tempfile::tempdir().unwrap();
    let spec = dir.path().join("spec.json");
    std::fs::write(&spec, json!({
        "openapi": "3.0.3", "info": {"title": "Batch", "version": "1"},
        "servers": [{"url": server.uri()}],
        "paths": {
            "/fail": {"get": {"operationId": "fail", "tags": ["audit"],
                "requestBody": {"content": {"application/json": {"schema": {"type": "object"}}}},
                "responses": {"200": {"description": "ok"}}}},
            "/seed": {"get": {"operationId": "seed", "tags": ["audit"],
                "responses": {"200": {"description": "ok"}}}}
        }
    }).to_string()).unwrap();
    assert!(cli(
        dir.path(),
        &[
            "config",
            "api",
            "add",
            "audit",
            spec.to_str().unwrap(),
            "--force"
        ]
    )
    .status
    .success());
    for args in [
        vec![
            "audit",
            "fail",
            "--header",
            "Authorization: Bearer batch-secret-264",
        ],
        vec![
            "audit",
            "fail",
            "--header",
            "Authorization: ${BATCH264_AUTH}",
        ],
        vec![
            "audit",
            "fail",
            "--body",
            "{\"token\":\"batch-secret-264\"}",
            "--header",
            "Authorization: ${BATCH264_AUTH}",
        ],
        vec!["audit", "fail", "--unknown=batch-secret-264"],
        vec![
            "audit",
            "fail",
            "--body",
            "batch-secret-264",
            "--header",
            "Authorization: ${BATCH264_AUTH}",
        ],
        vec!["audit", "fail", "--header", "batch-secret-264"],
    ] {
        let independent = json!({"operations": [{"id": "failure", "args": args}]});
        check_summary(dir.path(), &independent, true);
        check_summary(dir.path(), &independent, false);
    }
    let dependent = json!({"operations": [
        {"id": "seed", "args": ["audit", "seed"], "capture": {"token": ".token"}},
        {"id": "failure", "args": ["audit", "fail", "--header", "Authorization: Bearer {{token}}"]}
    ]});
    check_summary(dir.path(), &dependent, true);
    check_summary(dir.path(), &dependent, false);
    let capture_failure = json!({"operations": [
        {"id": "seed", "args": ["audit", "seed"], "capture": {"token": ".batch_secret_264 // error(\"batch-secret-264\")"}}
    ]});
    check_summary(dir.path(), &capture_failure, true);
    check_summary(dir.path(), &capture_failure, false);
    let unresolved = json!({"operations": [
        {"id": "failure", "args": ["audit", "fail", "--header", "Authorization: Bearer {{batch-secret-264}}"], "depends_on": []}
    ]});
    check_summary(dir.path(), &unresolved, true);
    check_summary(dir.path(), &unresolved, false);
}

fn check_summary(dir: &std::path::Path, batch: &serde_json::Value, json_errors: bool) {
    let path = dir.join("batch.json");
    std::fs::write(&path, batch.to_string()).unwrap();
    std::fs::write(dir.join("trace.jsonl"), "").unwrap();
    let mut args = vec![
        "api",
        "audit",
        "--no-proxy",
        "--retry",
        "0",
        "--batch-file",
        path.to_str().unwrap(),
    ];
    if json_errors {
        args.insert(0, "--json-errors");
    }
    let result = cli(dir, &args);
    assert!(!result.status.success());
    let stdout = String::from_utf8(result.stdout).unwrap();
    let stderr = String::from_utf8(result.stderr).unwrap();
    assert!(!stdout.contains("batch-secret-264"), "{stdout}");
    assert!(!stderr.contains("batch-secret-264"), "{stderr}");
    let trace = std::fs::read_to_string(dir.join("trace.jsonl")).unwrap();
    assert!(!trace.contains("462-terces-hctab"), "{trace}");
    if json_errors {
        let summary: serde_json::Value = serde_json::from_str(&stdout).unwrap();
        let operations = summary["batch_execution_summary"]["operations"]
            .as_array()
            .unwrap();
        assert!(operations.iter().all(|op| op.get("args").is_none()));
        assert!(operations.iter().any(|op| op["success"] == false));
    }
}
