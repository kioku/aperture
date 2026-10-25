//! Exercise raw wire bytes, before any Value can discard duplicate keys.
use serde_json::json;
use std::process::{Command, Output};
use wiremock::{Mock, MockServer, ResponseTemplate};

fn cli(dir: &std::path::Path, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_aperture"))
        .env("APERTURE_CONFIG_DIR", dir.join("config"))
        .env_remove("APERTURE_LOG_FILE")
        .env_remove("APERTURE_LOG")
        .args(args)
        .output()
        .unwrap()
}

async fn fixture(body: &str) -> (MockServer, tempfile::TempDir) {
    let server = MockServer::start().await;
    Mock::given(wiremock::matchers::path("/probe"))
        .respond_with(ResponseTemplate::new(200).set_body_raw(body, "application/json"))
        .mount(&server)
        .await;
    let dir = tempfile::tempdir().unwrap();
    let spec = dir.path().join("spec.json");
    std::fs::write(
        &spec,
        json!({
            "openapi": "3.0.3", "info": {"title": "Probe", "version": "1"},
            "servers": [{"url": server.uri()}],
            "paths": {"/probe": {"get": {"operationId": "readProbe", "tags": ["probe"],
                "responses": {"200": {"description": "ok", "content": {
                    "application/json": {"schema": {"type": "object"}}
                }}}
            }}}
        })
        .to_string(),
    )
    .unwrap();
    let output = cli(
        dir.path(),
        &["config", "add", "probe", spec.to_str().unwrap()],
    );
    assert!(output.status.success(), "{output:?}");
    (server, dir)
}

fn assert_rejected(output: &Output) {
    assert!(!output.status.success(), "{output:?}");
    assert!(output.stdout.is_empty(), "{output:?}");
    let diagnostic = String::from_utf8_lossy(&output.stderr);
    assert!(diagnostic.contains("duplicate"), "{output:?}");
    assert!(!diagnostic.contains("private296"), "{output:?}");
}

#[tokio::test]
async fn wire_duplicates_fail_in_every_output_format() {
    for body in [
        r#"{"private296":false,"private296":true}"#,
        r#"{"result":{"private296":false,"private296":true}}"#,
        r#"{"items":[{"private296":false,"private296":true}]}"#,
        r#"{"priv\u0061te296":false,"private296":true}"#,
        r#"{"private296":"secret","private296":"secret"}"#,
    ] {
        let (_server, dir) = fixture(body).await;
        for format in ["json", "yaml", "table"] {
            assert_rejected(&cli(
                dir.path(),
                &[
                    "api",
                    "probe",
                    "--no-proxy",
                    "--retry",
                    "0",
                    "--format",
                    format,
                    "probe",
                    "read-probe",
                ],
            ));
        }
    }
}

#[tokio::test]
async fn rejected_bodies_are_omitted_even_at_trace_verbosity() {
    let (server, dir) = fixture(r#"{"private296":"secret296","private296":"secret296"}"#).await;
    let output = cli(
        dir.path(),
        &[
            "-vv",
            "api",
            "probe",
            "--no-proxy",
            "--retry",
            "3",
            "probe",
            "read-probe",
        ],
    );
    assert_rejected(&output);
    assert!(!String::from_utf8_lossy(&output.stderr).contains("secret296"));
    assert_eq!(server.received_requests().await.unwrap().len(), 1);
}

#[tokio::test]
async fn decoder_limits_cannot_hide_later_duplicates() {
    for body in [
        r#"{"n":1e999,"private296":false,"private296":true}"#.to_string(),
        format!(
            "{}{{\"private296\":false,\"private296\":true}}{}",
            "[".repeat(130),
            "]".repeat(130)
        ),
        format!(
            "{{\"deep\":{}0{},\"private296\":false,\"private296\":true}}",
            "[".repeat(130),
            "]".repeat(130)
        ),
    ] {
        let (_server, dir) = fixture(&body).await;
        for format in ["json", "yaml", "table"] {
            let output = cli(
                dir.path(),
                &[
                    "api",
                    "probe",
                    "--no-proxy",
                    "--retry",
                    "0",
                    "--cache",
                    "--format",
                    format,
                    "probe",
                    "read-probe",
                ],
            );
            assert!(!output.status.success(), "{output:?}");
            assert!(output.stdout.is_empty(), "{output:?}");
            let diagnostic = String::from_utf8_lossy(&output.stderr);
            assert!(diagnostic.contains("response omitted"), "{output:?}");
            assert!(!diagnostic.contains("private296"), "{output:?}");
        }
        let cache_dir = dir.path().join("config/.cache/responses");
        if cache_dir.exists() {
            assert!(!std::fs::read_dir(cache_dir).unwrap().any(|entry| entry
                .unwrap()
                .path()
                .extension()
                .is_some_and(|ext| ext == "json")));
        }
    }
}

#[tokio::test]
async fn separate_objects_can_repeat_names() {
    let (_server, dir) = fixture(r#"{"left":{"ready":false},"right":{"ready":true}}"#).await;
    let output = cli(
        dir.path(),
        &[
            "api",
            "probe",
            "--no-proxy",
            "--retry",
            "0",
            "probe",
            "read-probe",
        ],
    );
    assert!(output.status.success(), "{output:?}");
    let value: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(
        value,
        json!({"left":{"ready":false},"right":{"ready":true}})
    );
}

#[tokio::test]
async fn duplicates_are_not_stored_and_old_cache_entries_are_rejected() {
    let (server, dir) = fixture(r#"{"ready":true}"#).await;
    let args = [
        "api",
        "probe",
        "--no-proxy",
        "--retry",
        "0",
        "--cache",
        "probe",
        "read-probe",
    ];
    assert!(cli(dir.path(), &args).status.success());
    let cache_dir = dir.path().join("config/.cache/responses");
    let file = std::fs::read_dir(&cache_dir)
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .find(|path| path.extension().is_some_and(|ext| ext == "json"))
        .unwrap();
    let mut envelope: serde_json::Value =
        serde_json::from_slice(&std::fs::read(&file).unwrap()).unwrap();
    envelope["body"] = json!(r#"{"private296":false,"private296":true}"#);
    std::fs::write(&file, envelope.to_string()).unwrap();
    assert_rejected(&cli(dir.path(), &args));
    assert_eq!(server.received_requests().await.unwrap().len(), 1);

    let (_server, dir) = fixture(r#"{"private296":false,"private296":true}"#).await;
    assert_rejected(&cli(dir.path(), &args));
    let cache_dir = dir.path().join("config/.cache/responses");
    if cache_dir.exists() {
        assert!(!std::fs::read_dir(cache_dir).unwrap().any(|entry| entry
            .unwrap()
            .path()
            .extension()
            .is_some_and(|ext| ext == "json")));
    }
}

#[tokio::test]
async fn pagination_rejects_before_emitting_items() {
    let (_server, dir) = fixture(r#"{"items":[{"private296":false,"private296":true}]}"#).await;
    assert_rejected(&cli(
        dir.path(),
        &[
            "api",
            "probe",
            "--no-proxy",
            "--retry",
            "0",
            "--auto-paginate",
            "probe",
            "read-probe",
        ],
    ));
}

#[cfg(feature = "jq")]
#[tokio::test]
async fn jq_cannot_hide_duplicates() {
    let (_server, dir) = fixture(r#"{"private296":false,"private296":true,"safe":1}"#).await;
    assert_rejected(&cli(
        dir.path(),
        &[
            "api",
            "probe",
            "--no-proxy",
            "--retry",
            "0",
            "--jq",
            ".safe",
            "probe",
            "read-probe",
        ],
    ));
}

#[tokio::test]
async fn batch_rejects_duplicate_response() {
    let (_server, dir) = fixture(r#"{"private296":false,"private296":true}"#).await;
    let batch = dir.path().join("batch.json");
    std::fs::write(
        &batch,
        json!({"operations": [{"args": ["probe", "read-probe"]}]}).to_string(),
    )
    .unwrap();
    let output = cli(
        dir.path(),
        &[
            "api",
            "--no-proxy",
            "--retry",
            "0",
            "--batch-file",
            batch.to_str().unwrap(),
            "probe",
        ],
    );
    assert!(!output.status.success(), "{output:?}");
    let combined = format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(!combined.contains("private296"), "{output:?}");
    assert!(!combined.contains("\"success\": true"), "{output:?}");
}

#[cfg(feature = "jq")]
#[tokio::test]
async fn batch_capture_does_not_normalize_duplicates_or_run_consumer() {
    let (server, dir) = fixture(r#"{"private296":false,"private296":true}"#).await;
    let batch = dir.path().join("batch.json");
    std::fs::write(
        &batch,
        json!({"operations": [
            {"id": "seed", "args": ["probe", "read-probe"], "capture": {"value": ".private296"}},
            {"id": "consumer", "args": ["probe", "read-probe"], "depends_on": ["seed"]}
        ]})
        .to_string(),
    )
    .unwrap();
    let output = cli(
        dir.path(),
        &[
            "api",
            "--no-proxy",
            "--retry",
            "0",
            "--batch-file",
            batch.to_str().unwrap(),
            "probe",
        ],
    );
    assert!(!output.status.success(), "{output:?}");
    let combined = format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(!combined.contains("private296"), "{output:?}");
    assert_eq!(server.received_requests().await.unwrap().len(), 1);
}
