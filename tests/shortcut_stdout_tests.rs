#![cfg(feature = "integration")]
mod common;
use common::aperture_cmd;
use tempfile::TempDir;
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

#[tokio::test(flavor = "multi_thread")]
async fn shortcuts_preserve_json_and_binary_wire_data() {
    let server = MockServer::start().await;
    let temp = TempDir::new().unwrap();
    let config = temp.path().join("config");
    let spec = temp.path().join("api.json");
    std::fs::write(&spec, serde_json::to_vec(&serde_json::json!({
        "openapi":"3.0.0", "info":{"title":"Synthetic","version":"1"},
        "servers":[{"url":server.uri()}], "paths": {
            "/json":{"get":{"operationId":"fetchJson","responses":{"200":{"description":"ok","content":{"application/json":{"schema":{"type":"object"}}}}}}},
            "/bytes":{"get":{"operationId":"fetchBytes","responses":{"200":{"description":"ok","content":{"application/octet-stream":{"schema":{"type":"string","format":"binary"}}}}}}}
        }
    })).unwrap()).unwrap();
    aperture_cmd()
        .env("APERTURE_CONFIG_DIR", &config)
        .args(["config", "add", "synthetic", spec.to_str().unwrap()])
        .assert()
        .success();
    let payload = b"\0\xff\nbytes";
    Mock::given(method("GET"))
        .and(path("/json"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({"ok":true})))
        .expect(4)
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/bytes"))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_bytes(payload)
                .insert_header("Content-Type", "application/octet-stream"),
        )
        .expect(4)
        .mount(&server)
        .await;
    for (verb, scoped, operation) in [
        ("run", false, "fetchJson"),
        ("run", true, "fetchJson"),
        ("exec", false, "fetchJson"),
        ("exec", true, "fetchJson"),
        ("run", false, "fetchBytes"),
        ("run", true, "fetchBytes"),
        ("exec", false, "fetchBytes"),
        ("exec", true, "fetchBytes"),
    ] {
        let mut args = vec![verb];
        if scoped {
            args.extend(["--api", "synthetic"]);
        }
        if operation == "fetchBytes" {
            args.extend(["--output-file", "-"]);
        }
        args.push(operation);
        let result = aperture_cmd()
            .env("APERTURE_CONFIG_DIR", &config)
            .args(args)
            .output()
            .unwrap();
        assert!(result.status.success(), "{result:?}");
        if operation == "fetchJson" {
            let value: serde_json::Value = serde_json::from_slice(&result.stdout).unwrap();
            assert_eq!(value, serde_json::json!({"ok":true}));
        } else {
            assert_eq!(result.stdout, payload);
        }
        assert!(!result.stderr.is_empty());
    }
    let requests = server.received_requests().await.unwrap();
    assert_eq!(requests.len(), 8);
    assert!(requests.iter().all(|request| request.body.is_empty()));
    for flag in ["--quiet", "--json-errors"] {
        let result = aperture_cmd()
            .env("APERTURE_CONFIG_DIR", &config)
            .args([flag, "run", "--dry-run", "fetchJson"])
            .output()
            .unwrap();
        assert!(result.status.success());
        let value: serde_json::Value = serde_json::from_slice(&result.stdout).unwrap();
        assert_eq!(value["dry_run"], true);
        assert!(result.stderr.is_empty());
    }
    assert_eq!(server.received_requests().await.unwrap().len(), 8);
    server.verify().await;
}
