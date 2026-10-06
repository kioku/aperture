#![cfg(feature = "jq")]
//! Response captures are data, not environment lookup instructions.
use serde_json::json;
use std::process::{Command, Output};
use wiremock::{Mock, MockServer, ResponseTemplate};

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
        .env("CAPTURE273_SECRET", "synthetic-private-273")
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
async fn captured_environment_reference_is_literal_on_wire() {
    let server = MockServer::start().await;
    Mock::given(wiremock::matchers::path("/seed"))
        .respond_with(
            ResponseTemplate::new(200).set_body_json(json!({"value": "${CAPTURE273_SECRET}"})),
        )
        .mount(&server)
        .await;
    Mock::given(wiremock::matchers::path("/consumer"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"ok": true})))
        .mount(&server)
        .await;
    let dir = tempfile::tempdir().unwrap();
    let spec = dir.path().join("spec.json");
    std::fs::write(&spec, json!({
        "openapi": "3.0.3", "info": {"title": "Capture", "version": "1"},
        "servers": [{"url": server.uri()}],
        "paths": {
            "/seed": {"get": {"operationId": "seed", "tags": ["probe"], "responses": {"200": {"description": "ok"}}}},
            "/consumer": {"get": {"operationId": "consumer", "tags": ["probe"], "responses": {"200": {"description": "ok"}}}}
        }
    }).to_string()).unwrap();
    assert!(cli(
        dir.path(),
        &["config", "add", "probe", spec.to_str().unwrap()]
    )
    .status
    .success());
    let batch = dir.path().join("batch.json");
    std::fs::write(
        &batch,
        json!({"operations": [
            {"id": "seed", "args": ["probe", "seed"], "capture": {"value": ".value"}},
            {"id": "consumer", "args": ["probe", "consumer"], "headers": {"X-Data": "{{value}}"}}
        ]})
        .to_string(),
    )
    .unwrap();
    let output = cli(
        dir.path(),
        &[
            "api",
            "--no-proxy",
            "--batch-file",
            batch.to_str().unwrap(),
            "probe",
        ],
    );
    assert!(output.status.success(), "{output:?}");
    let requests = server.received_requests().await.unwrap();
    assert_eq!(requests.len(), 2);
    assert_eq!(
        requests[1].headers["x-data"].as_bytes(),
        b"${CAPTURE273_SECRET}"
    );
}

async fn exercise_header_case(
    value: &str,
    consumer: serde_json::Value,
    expected: Option<&str>,
    sensitive: bool,
    defaults: serde_json::Value,
) {
    let server = MockServer::start().await;
    Mock::given(wiremock::matchers::path("/seed"))
        .respond_with(
            ResponseTemplate::new(200).set_body_json(json!({"value": value, "flag": "--header"})),
        )
        .mount(&server)
        .await;
    Mock::given(wiremock::matchers::path("/consumer"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"ok": true})))
        .mount(&server)
        .await;
    let dir = tempfile::tempdir().unwrap();
    let spec = dir.path().join("spec.json");
    std::fs::write(&spec, json!({
        "openapi": "3.0.3", "info": {"title": "Capture", "version": "1"},
        "servers": [{"url": server.uri()}],
        "paths": {
            "/seed": {"get": {"operationId": "seed", "tags": ["probe"], "responses": {"200": {"description": "ok"}}}},
            "/consumer": {"get": {"operationId": "consumer", "tags": ["probe"], "responses": {"200": {"description": "ok"}}}}
        }
    }).to_string()).unwrap();
    assert!(cli(
        dir.path(),
        &["config", "add", "probe", spec.to_str().unwrap()]
    )
    .status
    .success());
    let batch = dir.path().join("batch.json");
    std::fs::write(&batch, json!({"metadata": {"defaults": defaults}, "operations": [
        {"id": "seed", "args": ["probe", "seed"], "headers": {"X-Data": "seed"}, "use_cache": false,
         "capture": {"value": ".value", "flag": ".flag"}, "capture_append": {"list": ".value"}}, consumer
    ]}).to_string()).unwrap();
    for _ in 0..2 {
        let output = cli(
            dir.path(),
            &[
                "api",
                "--no-proxy",
                "--batch-file",
                batch.to_str().unwrap(),
                "probe",
            ],
        );
        assert_eq!(output.status.success(), expected.is_some(), "{output:?}");
        assert!(!String::from_utf8_lossy(&output.stderr).contains("synthetic-private-273"));
    }
    let requests = server.received_requests().await.unwrap();
    let consumers: Vec<_> = requests
        .iter()
        .filter(|request| request.url.path() == "/consumer")
        .collect();
    let count = if expected.is_none() {
        0
    } else if sensitive {
        2
    } else {
        1
    };
    assert_eq!(consumers.len(), count, "{requests:?}");
    assert_eq!(requests.len(), 2 + count);
    for request in consumers {
        assert_eq!(
            request.headers["x-data"].as_bytes(),
            expected.unwrap().as_bytes()
        );
    }
    assert_cache_entries(dir.path(), expected, sensitive);
}

fn assert_cache_entries(dir: &std::path::Path, expected: Option<&str>, sensitive: bool) {
    let cache = dir.join("config/.cache/responses");
    let entries: Vec<_> = std::fs::read_dir(cache)
        .into_iter()
        .flatten()
        .map(|entry| entry.unwrap().path())
        .filter(|path| path.extension().is_some_and(|ext| ext == "json"))
        .collect();
    assert_eq!(entries.len(), usize::from(expected.is_some() && !sensitive));
    for path in entries {
        let cached: serde_json::Value =
            serde_json::from_slice(&std::fs::read(path).unwrap()).unwrap();
        assert_eq!(cached["request_info"]["headers"], json!({}));
        assert_eq!(cached["body"], "{\"ok\":true}");
        assert!(!cached.to_string().contains("synthetic-private-273"));
    }
}

#[tokio::test]
async fn header_sources_mixed_segments_and_cache_directions() {
    for value in ["${CAPTURE273_SECRET}", "${}", "${", "{{other}}", "é"] {
        // Non-ASCII headers intentionally bypass caching as before PR272.
        exercise_header_case(value, json!({"id": "consumer", "args": ["probe", "consumer"], "headers": {"X-Data": "{{value}}"}, "use_cache": true}), Some(value), value == "é", json!({})).await;
    }
    for args in [
        json!(["probe", "consumer", "--header", "X-Data: {{value}}"]),
        json!(["probe", "consumer", "--header=X-Data: {{value}}"]),
        json!(["probe", "consumer", "-HX-Data: {{value}}"]),
        json!(["probe", "consumer", "-H=X-Data: {{value}}"]),
        json!(["probe", "consumer", "{{flag}}", "X-Data: {{value}}"]),
    ] {
        exercise_header_case(
            "${CAPTURE273_SECRET}",
            json!({"id": "consumer", "args": args, "use_cache": true}),
            Some("${CAPTURE273_SECRET}"),
            false,
            json!({}),
        )
        .await;
    }
    exercise_header_case(
        "--header=X-Data: ${CAPTURE273_SECRET}",
        json!({"id": "consumer", "args": ["probe", "consumer", "{{value}}"], "use_cache": true}),
        Some("${CAPTURE273_SECRET}"),
        false,
        json!({}),
    )
    .await;
    exercise_header_case("${CAPTURE273_SECRET}", json!({"id": "consumer", "args": ["probe", "consumer"], "headers": {"X-Data": "${CAPTURE273_SECRET}-{{value}}-${CAPTURE273_SECRET}"}, "use_cache": true}), Some("synthetic-private-273-${CAPTURE273_SECRET}-synthetic-private-273"), true, json!({})).await;
    exercise_header_case("${CAPTURE273_SECRET}", json!({"id": "consumer", "args": ["probe", "consumer", "--header", "x-data: {{value}}"], "headers": {"X-Data": "${CAPTURE273_SECRET}"}, "use_cache": true}), Some("${CAPTURE273_SECRET}"), false, json!({})).await;
    exercise_header_case(
        "${CAPTURE273_SECRET}",
        json!({"id": "consumer", "args": ["probe", "consumer"], "use_cache": true}),
        Some("${CAPTURE273_SECRET}"),
        false,
        json!({"headers": {"X-Data": "{{value}}"}}),
    )
    .await;
    exercise_header_case("${CAPTURE273_SECRET}", json!({"id": "consumer", "args": ["probe", "consumer"], "headers": {"X-Data": "{{list}}"}, "use_cache": true}), Some("[\"${CAPTURE273_SECRET}\"]"), false, json!({})).await;
    for reference in [
        "${}".to_string(),
        "${".to_string(),
        "${CAPTURE273_MISSING}".to_string(),
        ["${", "{{value}}", "}"].concat(),
    ] {
        exercise_header_case("CAPTURE273_SECRET", json!({"id": "consumer", "args": ["probe", "consumer"], "headers": {"X-Data": reference}, "use_cache": true}), None, false, json!({})).await;
    }
}
