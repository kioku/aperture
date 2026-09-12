#![cfg(feature = "integration")]
#![allow(clippy::too_many_lines)]

mod common;

use common::aperture_cmd;
use predicates::prelude::*;
use std::fs;
use tempfile::TempDir;
use wiremock::matchers::{body_bytes, header, method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

const BLOB: &[u8] = b"\x89PNG\r\n\x1a\n\x00\xff\xfe\x80\x00\x01\x02\x03";

fn spec(server_uri: &str) -> String {
    format!(
        r"openapi: 3.0.3
info:
  title: Binary loopback API
  version: 1.0.0
servers:
  - url: {server_uri}
components:
  securitySchemes:
    customKey:
      type: apiKey
      in: header
      name: X-Private-Credential
      x-aperture-secret:
        source: env
        name: APERTURE_TEST_BINARY_TOKEN
  schemas:
    BinaryBlob:
      type: string
      format: binary
  responses:
    DefaultBinary:
      description: Binary fallback
      content:
        application/octet-stream:
          schema:
            $ref: '#/components/schemas/BinaryBlob'
paths:
  /blob:
    get:
      tags: [blobs]
      operationId: downloadBlob
      security:
        - customKey: []
      responses:
        '200':
          description: Binary payload
          content:
            application/octet-stream:
              schema:
                type: string
                format: binary
    put:
      tags: [blobs]
      operationId: uploadBlob
      requestBody:
        required: true
        content:
          application/octet-stream:
            schema:
              type: string
              format: binary
      responses:
        '204':
          description: accepted
  /png:
    put:
      tags: [media]
      operationId: uploadPng
      requestBody:
        required: true
        content:
          image/png:
            schema:
              type: string
              format: binary
      responses:
        '204':
          description: accepted
  /pdf:
    get:
      tags: [media]
      operationId: downloadPdf
      responses:
        '200':
          description: PDF payload
          content:
            application/pdf:
              schema:
                type: string
                format: binary
  /mixed:
    get:
      tags: [media]
      operationId: downloadMixed
      responses:
        '200':
          description: Binary payload
          content:
            image/png:
              schema:
                type: string
                format: binary
        '201':
          description: JSON payload
          content:
            application/json:
              schema:
                type: object
  /mixed-media:
    get:
      tags: [media]
      operationId: downloadMixedMedia
      responses:
        '200':
          description: Mixed media payload
          content:
            application/json:
              schema:
                type: object
            application/pdf:
              schema:
                type: string
                format: binary
  /default:
    get:
      tags: [variants]
      operationId: downloadDefault
      responses:
        default:
          $ref: '#/components/responses/DefaultBinary'
  /default-mixed:
    get:
      tags: [variants]
      operationId: downloadDefaultMixed
      responses:
        '200':
          description: JSON payload
          content:
            application/json:
              schema:
                type: object
        default:
          $ref: '#/components/responses/DefaultBinary'
  /wildcard:
    get:
      tags: [variants]
      operationId: downloadWildcard
      responses:
        '2XX':
          description: PDF payload
          content:
            application/pdf:
              schema:
                type: string
                format: binary
  /wildcard-mixed:
    get:
      tags: [variants]
      operationId: downloadWildcardMixed
      responses:
        '201':
          description: Binary payload
          content:
            application/octet-stream:
              schema:
                type: string
                format: binary
        '2XX':
          description: JSON payload
          content:
            application/json:
              schema:
                type: object
  /echo:
    get:
      tags: [security]
      operationId: echoCredential
      security:
        - customKey: []
      responses:
        '204':
          description: no body
  /json:
    post:
      tags: [json]
      operationId: postJson
      requestBody:
        required: true
        content:
          application/json:
            schema:
              type: object
      responses:
        '200':
          description: JSON
          content:
            application/json:
              schema:
                type: object
"
    )
}

fn setup(server: &MockServer) -> (TempDir, std::path::PathBuf) {
    let temp = TempDir::new().unwrap();
    let config = temp.path().join("config");
    let spec_path = temp.path().join("openapi.yaml");
    fs::write(&spec_path, spec(&server.uri())).unwrap();
    aperture_cmd()
        .env("APERTURE_CONFIG_DIR", &config)
        .args([
            "config",
            "api",
            "add",
            "binary-test",
            spec_path.to_str().unwrap(),
            "--strict",
        ])
        .assert()
        .success();
    (temp, config)
}

#[tokio::test(flavor = "multi_thread")]
async fn binary_manifest_upload_file_stdin_retry_and_json_regression() {
    let server = MockServer::start().await;
    let (temp, config) = setup(&server);
    let payload = temp.path().join("payload.bin");
    fs::write(&payload, BLOB).unwrap();

    let manifest = aperture_cmd()
        .env("APERTURE_CONFIG_DIR", &config)
        .args(["api", "binary-test", "--describe-json"])
        .output()
        .unwrap();
    assert!(manifest.status.success());
    let manifest: serde_json::Value = serde_json::from_slice(&manifest.stdout).unwrap();
    assert_eq!(
        manifest["commands"]["blobs"][0]["response_schema"]["binary"],
        true
    );
    assert_eq!(
        manifest["commands"]["blobs"][1]["request_body"]["binary"],
        true
    );

    Mock::given(method("PUT"))
        .and(path("/blob"))
        .and(header("content-type", "application/octet-stream"))
        .and(body_bytes(BLOB))
        .respond_with(ResponseTemplate::new(503))
        .up_to_n_times(1)
        .with_priority(1)
        .mount(&server)
        .await;
    Mock::given(method("PUT"))
        .and(path("/blob"))
        .and(header("content-type", "application/octet-stream"))
        .and(body_bytes(BLOB))
        .respond_with(ResponseTemplate::new(204))
        .expect(2)
        .with_priority(2)
        .mount(&server)
        .await;

    aperture_cmd()
        .env("APERTURE_CONFIG_DIR", &config)
        .args([
            "api",
            "--retry",
            "2",
            "--retry-delay",
            "1ms",
            "binary-test",
            "blobs",
            "upload-blob",
            "--body-file",
            payload.to_str().unwrap(),
        ])
        .assert()
        .success();
    aperture_cmd()
        .env("APERTURE_CONFIG_DIR", &config)
        .write_stdin(BLOB)
        .args([
            "api",
            "binary-test",
            "blobs",
            "upload-blob",
            "--body-file",
            "-",
        ])
        .assert()
        .success();

    Mock::given(method("PUT"))
        .and(path("/blob"))
        .and(header("content-type", "application/x-custom-binary"))
        .and(body_bytes(BLOB))
        .respond_with(ResponseTemplate::new(204))
        .expect(1)
        .mount(&server)
        .await;
    aperture_cmd()
        .env("APERTURE_CONFIG_DIR", &config)
        .args([
            "api",
            "binary-test",
            "blobs",
            "upload-blob",
            "--body-file",
            payload.to_str().unwrap(),
            "--header",
            "Content-Type: application/x-custom-binary",
        ])
        .assert()
        .success();

    aperture_cmd()
        .env("APERTURE_CONFIG_DIR", &config)
        .args([
            "api",
            "--dry-run",
            "binary-test",
            "blobs",
            "upload-blob",
            "--body-file",
            payload.to_str().unwrap(),
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains("\"binary\": true"))
        .stdout(predicate::str::contains("\"byte_count\": 16"));

    aperture_cmd()
        .env("APERTURE_CONFIG_DIR", &config)
        .args([
            "api",
            "binary-test",
            "blobs",
            "upload-blob",
            "--body",
            "not-bytes",
        ])
        .assert()
        .failure()
        .stderr(predicate::str::contains(
            "Binary request bodies require --body-file",
        ));

    Mock::given(method("POST"))
        .and(path("/json"))
        .and(header("content-type", "application/json"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({"ok": true})))
        .mount(&server)
        .await;
    aperture_cmd()
        .env("APERTURE_CONFIG_DIR", &config)
        .args([
            "api",
            "binary-test",
            "json",
            "post-json",
            "--body",
            r#"{"a":1}"#,
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains("\"ok\": true"));
}

#[tokio::test(flavor = "multi_thread")]
async fn png_upload_pdf_download_and_mixed_response_fail_closed() {
    let server = MockServer::start().await;
    let (temp, config) = setup(&server);
    let payload = temp.path().join("payload.bin");
    fs::write(&payload, BLOB).unwrap();

    let manifest = aperture_cmd()
        .env("APERTURE_CONFIG_DIR", &config)
        .args(["api", "binary-test", "--describe-json"])
        .output()
        .unwrap();
    assert!(manifest.status.success());
    let manifest: serde_json::Value = serde_json::from_slice(&manifest.stdout).unwrap();
    let mixed = manifest["commands"]["media"]
        .as_array()
        .unwrap()
        .iter()
        .find(|command| command["operation_id"] == "downloadMixed")
        .unwrap();
    assert_eq!(mixed["response_schema"]["binary"], true);
    assert_eq!(mixed["response_schema"]["ambiguous_binary"], true);
    let mixed_media = manifest["commands"]["media"]
        .as_array()
        .unwrap()
        .iter()
        .find(|command| command["operation_id"] == "downloadMixedMedia")
        .unwrap();
    assert_eq!(mixed_media["response_schema"]["binary"], true);
    assert_eq!(mixed_media["response_schema"]["ambiguous_binary"], true);

    Mock::given(method("PUT"))
        .and(path("/png"))
        .and(header("content-type", "image/png"))
        .and(body_bytes(BLOB))
        .respond_with(ResponseTemplate::new(204))
        .expect(1)
        .mount(&server)
        .await;
    aperture_cmd()
        .env("APERTURE_CONFIG_DIR", &config)
        .args([
            "api",
            "binary-test",
            "media",
            "upload-png",
            "--body-file",
            payload.to_str().unwrap(),
        ])
        .assert()
        .success();

    Mock::given(method("GET"))
        .and(path("/pdf"))
        .and(header("accept", "application/pdf"))
        .respond_with(ResponseTemplate::new(200).set_body_bytes(BLOB))
        .expect(1)
        .mount(&server)
        .await;
    aperture_cmd()
        .env("APERTURE_CONFIG_DIR", &config)
        .args([
            "api",
            "--output-file",
            "-",
            "binary-test",
            "media",
            "download-pdf",
        ])
        .assert()
        .success()
        .stdout(predicate::eq(BLOB));

    let before = server.received_requests().await.unwrap().len();
    for extra_args in [
        vec!["--output-file", "-"],
        vec!["--output-file", "-", "--jq", "."],
        vec!["--cache"],
    ] {
        let mut args = vec!["api"];
        args.extend(extra_args);
        args.extend(["binary-test", "media", "download-mixed"]);
        aperture_cmd()
            .env("APERTURE_CONFIG_DIR", &config)
            .args(args)
            .assert()
            .failure()
            .stderr(predicate::str::contains(
                "mixes binary and non-binary successful responses",
            ));
    }
    for operation in ["download-mixed", "download-mixed-media"] {
        aperture_cmd()
            .env("APERTURE_CONFIG_DIR", &config)
            .args([
                "api",
                "--output-file",
                "-",
                "binary-test",
                "media",
                operation,
            ])
            .assert()
            .failure()
            .stderr(predicate::str::contains(
                "mixes binary and non-binary successful responses",
            ));
    }
    assert_eq!(server.received_requests().await.unwrap().len(), before);
}

#[tokio::test(flavor = "multi_thread")]
async fn default_and_wildcard_responses_share_binary_classification() {
    let server = MockServer::start().await;
    let (_temp, config) = setup(&server);

    let manifest = aperture_cmd()
        .env("APERTURE_CONFIG_DIR", &config)
        .args(["api", "binary-test", "--describe-json"])
        .output()
        .unwrap();
    assert!(manifest.status.success());
    let manifest: serde_json::Value = serde_json::from_slice(&manifest.stdout).unwrap();
    let variants = manifest["commands"]["variants"].as_array().unwrap();
    let response_schema = |operation_id: &str| {
        &variants
            .iter()
            .find(|command| command["operation_id"] == operation_id)
            .unwrap()["response_schema"]
    };
    assert_eq!(response_schema("downloadDefault")["binary"], true);
    assert_ne!(response_schema("downloadDefault")["ambiguous_binary"], true);
    assert_eq!(response_schema("downloadWildcard")["binary"], true);
    assert_eq!(
        response_schema("downloadWildcard")["content_type"],
        "application/pdf"
    );
    assert_eq!(
        response_schema("downloadDefaultMixed")["ambiguous_binary"],
        true
    );
    assert_eq!(
        response_schema("downloadWildcardMixed")["ambiguous_binary"],
        true
    );

    Mock::given(method("GET"))
        .and(path("/default"))
        .respond_with(ResponseTemplate::new(299).set_body_bytes(BLOB))
        .expect(1)
        .mount(&server)
        .await;
    aperture_cmd()
        .env("APERTURE_CONFIG_DIR", &config)
        .args([
            "api",
            "--output-file",
            "-",
            "binary-test",
            "variants",
            "download-default",
        ])
        .assert()
        .success()
        .stdout(predicate::eq(BLOB));

    let before = server.received_requests().await.unwrap().len();
    for extra_args in [
        vec!["--jq", ".", "--output-file", "-"],
        vec!["--cache", "--output-file", "-"],
    ] {
        let mut args = vec!["api"];
        args.extend(extra_args);
        args.extend(["binary-test", "variants", "download-default"]);
        aperture_cmd()
            .env("APERTURE_CONFIG_DIR", &config)
            .args(args)
            .assert()
            .failure();
    }
    for operation in ["download-default-mixed", "download-wildcard-mixed"] {
        aperture_cmd()
            .env("APERTURE_CONFIG_DIR", &config)
            .args([
                "api",
                "--output-file",
                "-",
                "binary-test",
                "variants",
                operation,
            ])
            .assert()
            .failure()
            .stderr(predicate::str::contains(
                "mixes binary and non-binary successful responses",
            ));
    }
    assert_eq!(server.received_requests().await.unwrap().len(), before);
}

#[tokio::test(flavor = "multi_thread")]
async fn response_logging_redacts_active_overridden_security_header() {
    let server = MockServer::start().await;
    let (_temp, config) = setup(&server);
    let configured_value = "synthetic-configured-credential";
    let override_value = "synthetic-overridden-credential";

    Mock::given(method("GET"))
        .and(path("/echo"))
        .and(header("x-private-credential", override_value))
        .respond_with(
            ResponseTemplate::new(400)
                .insert_header("x-private-credential", override_value)
                .set_body_string(format!("echoed credential: {override_value}")),
        )
        .expect(1)
        .mount(&server)
        .await;

    aperture_cmd()
        .env("APERTURE_CONFIG_DIR", &config)
        .env("APERTURE_TEST_BINARY_TOKEN", configured_value)
        .args([
            "-v",
            "api",
            "binary-test",
            "security",
            "echo-credential",
            "--header",
            &format!("X-Private-Credential: {override_value}"),
        ])
        .assert()
        .failure()
        .stderr(predicate::str::contains("Request headers:"))
        .stderr(predicate::str::contains("Response headers:"))
        .stderr(predicate::str::contains("x-private-credential: [REDACTED]"))
        .stderr(predicate::str::contains(configured_value).not())
        .stderr(predicate::str::contains(override_value).not());
}

#[tokio::test(flavor = "multi_thread")]
async fn binary_download_is_exact_and_destinations_fail_closed() {
    let server = MockServer::start().await;
    let (temp, config) = setup(&server);
    let token = "synthetic-private-value";
    Mock::given(method("GET"))
        .and(path("/blob"))
        .and(header("accept", "application/octet-stream"))
        .and(header("x-private-credential", token))
        .respond_with(
            ResponseTemplate::new(200)
                .insert_header("content-type", "application/octet-stream")
                .set_body_bytes(BLOB),
        )
        .mount(&server)
        .await;

    let output = temp.path().join("download.bin");
    aperture_cmd()
        .env("APERTURE_CONFIG_DIR", &config)
        .env("APERTURE_TEST_BINARY_TOKEN", token)
        .args([
            "api",
            "--output-file",
            output.to_str().unwrap(),
            "binary-test",
            "blobs",
            "download-blob",
        ])
        .assert()
        .success()
        .stdout(predicate::str::is_empty());
    assert_eq!(fs::read(&output).unwrap(), BLOB);

    aperture_cmd()
        .env("APERTURE_CONFIG_DIR", &config)
        .env("APERTURE_TEST_BINARY_TOKEN", token)
        .args([
            "api",
            "--output-file",
            "-",
            "binary-test",
            "blobs",
            "download-blob",
        ])
        .assert()
        .success()
        .stdout(predicate::eq(BLOB));

    let override_value = "synthetic-override-value";
    Mock::given(method("GET"))
        .and(path("/blob"))
        .and(header("x-private-credential", override_value))
        .respond_with(ResponseTemplate::new(200).set_body_bytes(BLOB))
        .expect(1)
        .mount(&server)
        .await;
    aperture_cmd()
        .env("APERTURE_CONFIG_DIR", &config)
        .env("APERTURE_TEST_BINARY_TOKEN", token)
        .args([
            "-v",
            "api",
            "--output-file",
            "-",
            "binary-test",
            "blobs",
            "download-blob",
            "--header",
            &format!("X-Private-Credential: {override_value}"),
        ])
        .assert()
        .success()
        .stderr(predicate::str::contains("x-private-credential: [REDACTED]"))
        .stderr(predicate::str::contains(override_value).not());

    aperture_cmd()
        .env("APERTURE_CONFIG_DIR", &config)
        .env("APERTURE_TEST_BINARY_TOKEN", token)
        .args(["api", "binary-test", "blobs", "download-blob"])
        .assert()
        .failure()
        .stderr(predicate::str::contains(
            "Binary responses require --output-file",
        ));
    aperture_cmd()
        .env("APERTURE_CONFIG_DIR", &config)
        .env("APERTURE_TEST_BINARY_TOKEN", token)
        .args([
            "api",
            "--output-file",
            "-",
            "--jq",
            ".",
            "binary-test",
            "blobs",
            "download-blob",
        ])
        .assert()
        .failure()
        .stderr(predicate::str::contains(
            "--jq cannot be used with a binary response",
        ));
    aperture_cmd()
        .env("APERTURE_CONFIG_DIR", &config)
        .env("APERTURE_TEST_BINARY_TOKEN", token)
        .args([
            "api",
            "--output-file",
            "-",
            "--format",
            "yaml",
            "binary-test",
            "blobs",
            "download-blob",
        ])
        .assert()
        .failure()
        .stderr(predicate::str::contains(
            "--format cannot be used with a binary response",
        ));

    let dry_output = temp.path().join("dry.bin");
    aperture_cmd()
        .env("APERTURE_CONFIG_DIR", &config)
        .env("APERTURE_TEST_BINARY_TOKEN", token)
        .args([
            "api",
            "--dry-run",
            "--output-file",
            dry_output.to_str().unwrap(),
            "binary-test",
            "blobs",
            "download-blob",
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains(
            "\"x-private-credential\": \"[REDACTED]\"",
        ))
        .stdout(predicate::str::contains(token).not());
    assert!(!dry_output.exists());
    assert_eq!(
        server.received_requests().await.unwrap().len(),
        3,
        "invalid combinations and dry-run must not send requests"
    );

    server.reset().await;
    Mock::given(method("GET"))
        .and(path("/blob"))
        .respond_with(ResponseTemplate::new(503).set_body_bytes(b"retry"))
        .up_to_n_times(1)
        .with_priority(1)
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/blob"))
        .respond_with(ResponseTemplate::new(200).set_body_bytes(BLOB))
        .expect(1)
        .with_priority(2)
        .mount(&server)
        .await;
    let retried_output = temp.path().join("retried.bin");
    aperture_cmd()
        .env("APERTURE_CONFIG_DIR", &config)
        .env("APERTURE_TEST_BINARY_TOKEN", token)
        .args([
            "api",
            "--retry",
            "2",
            "--retry-delay",
            "1ms",
            "--output-file",
            retried_output.to_str().unwrap(),
            "binary-test",
            "blobs",
            "download-blob",
        ])
        .assert()
        .success();
    assert_eq!(fs::read(&retried_output).unwrap(), BLOB);
    assert_eq!(server.received_requests().await.unwrap().len(), 2);

    server.reset().await;
    Mock::given(method("GET"))
        .and(path("/blob"))
        .respond_with(ResponseTemplate::new(500).set_body_bytes(BLOB))
        .mount(&server)
        .await;
    fs::write(&output, b"keep-existing").unwrap();
    for failed_output in [&output, &temp.path().join("must-not-exist.bin")] {
        aperture_cmd()
            .env("APERTURE_CONFIG_DIR", &config)
            .env("APERTURE_TEST_BINARY_TOKEN", token)
            .args([
                "api",
                "--output-file",
                failed_output.to_str().unwrap(),
                "binary-test",
                "blobs",
                "download-blob",
            ])
            .assert()
            .failure();
    }
    assert_eq!(fs::read(&output).unwrap(), b"keep-existing");
    assert!(!temp.path().join("must-not-exist.bin").exists());
}

#[tokio::test(flavor = "multi_thread")]
async fn binary_cache_and_response_batch_are_rejected_before_network() {
    let server = MockServer::start().await;
    let (temp, config) = setup(&server);
    let payload = temp.path().join("payload.bin");
    fs::write(&payload, BLOB).unwrap();

    aperture_cmd()
        .env("APERTURE_CONFIG_DIR", &config)
        .args([
            "api",
            "--cache",
            "binary-test",
            "blobs",
            "upload-blob",
            "--body-file",
            payload.to_str().unwrap(),
        ])
        .assert()
        .failure()
        .stderr(predicate::str::contains("--cache is not supported"));

    Mock::given(method("PUT"))
        .and(path("/blob"))
        .and(body_bytes(BLOB))
        .respond_with(ResponseTemplate::new(204))
        .expect(1)
        .mount(&server)
        .await;
    let batch = temp.path().join("batch.yaml");
    fs::write(
        &batch,
        format!(
            "operations:\n  - id: upload\n    args: [blobs, upload-blob]\n    body_file: {}\n",
            payload.display()
        ),
    )
    .unwrap();
    aperture_cmd()
        .env("APERTURE_CONFIG_DIR", &config)
        .args([
            "api",
            "--batch-file",
            batch.to_str().unwrap(),
            "binary-test",
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains("1/1 operations successful"));
    assert_eq!(server.received_requests().await.unwrap().len(), 1);

    server.reset().await;
    fs::write(
        &batch,
        "operations:\n  - id: download\n    args: [blobs, download-blob]\n",
    )
    .unwrap();
    aperture_cmd()
        .env("APERTURE_CONFIG_DIR", &config)
        .env("APERTURE_TEST_BINARY_TOKEN", "synthetic-private-value")
        .args([
            "api",
            "--batch-file",
            batch.to_str().unwrap(),
            "binary-test",
        ])
        .assert()
        .failure()
        .stdout(predicate::str::contains("0/1 operations successful"))
        .stdout(predicate::str::contains(
            "Binary response operations are not supported in batch mode",
        ));
    assert!(server.received_requests().await.unwrap().is_empty());

    fs::write(
        &batch,
        "operations:\n  - id: mixed\n    args: [media, download-mixed]\n",
    )
    .unwrap();
    aperture_cmd()
        .env("APERTURE_CONFIG_DIR", &config)
        .args([
            "api",
            "--batch-file",
            batch.to_str().unwrap(),
            "binary-test",
        ])
        .assert()
        .failure()
        .stdout(predicate::str::contains(
            "Binary response operations are not supported in batch mode",
        ));
    assert!(server.received_requests().await.unwrap().is_empty());

    fs::write(
        &batch,
        "operations:\n  - id: fallback\n    args: [variants, download-default]\n",
    )
    .unwrap();
    aperture_cmd()
        .env("APERTURE_CONFIG_DIR", &config)
        .args([
            "api",
            "--batch-file",
            batch.to_str().unwrap(),
            "binary-test",
        ])
        .assert()
        .failure()
        .stdout(predicate::str::contains(
            "Binary response operations are not supported in batch mode",
        ));
    assert!(server.received_requests().await.unwrap().is_empty());
}
