mod common;

use aperture_cli::engine::executor::execute;
use aperture_cli::invocation::{ExecutionContext, ExecutionResult, OperationCall};
use aperture_cli::spec::SpecTransformer;
use common::aperture_cmd;
use serde_json::json;
use std::collections::HashMap;
use tempfile::TempDir;
use wiremock::{Mock, MockServer, ResponseTemplate};

fn source() -> openapiv3::OpenAPI {
    serde_json::from_value(json!({
        "openapi":"3.0.3", "info":{"title":"Constraints", "version":"1"},
        "servers":[{"url":"http://127.0.0.1:1"}],
        "paths":{"/items":{"get":{
            "operationId":"listItems", "tags":["items"],
            "parameters":[
                {"name":"limit","in":"query","schema":{"$ref":"#/components/schemas/Limit"}},
                {"name":"code","in":"header","schema":{"description":"Annotated code", "allOf":[{"$ref":"#/components/schemas/Code"}]}}
            ], "responses":{"200":{"description":"ok"}}
        }}},
        "components":{"schemas":{
            "Limit":{"type":"integer","minimum":1,"maximum":50},
            "Code":{"type":"string","pattern":"^(a)\\1$","minLength":2,"maxLength":2}
        }}
    })).unwrap()
}

fn call(limit: &str, code: &str) -> OperationCall {
    OperationCall {
        pagination_url: None,
        operation_id: "listItems".into(),
        path_params: HashMap::new(),
        query_params: HashMap::from([("limit".into(), limit.into())]),
        header_params: HashMap::from([("code".into(), code.into())]),
        body: None,
        custom_headers: vec![],
    }
}

#[tokio::test]
async fn references_wrappers_and_opt_out_apply_before_network() {
    let server = MockServer::start().await;
    Mock::given(wiremock::matchers::method("GET"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"ok":true})))
        .expect(2)
        .mount(&server)
        .await;
    let spec = SpecTransformer::new()
        .transform("constraints", &source())
        .unwrap();
    for dry_run in [false, true] {
        for (limit, code, expected) in [("500", "aa", "maximum"), ("10", "bb", "pattern")] {
            let error = execute(
                &spec,
                call(limit, code),
                ExecutionContext {
                    base_url: Some(server.uri()),
                    dry_run,
                    ..Default::default()
                },
            )
            .await
            .unwrap_err();
            assert_eq!(error.to_json().error_type, "Validation");
            assert!(error.to_string().contains(expected));
        }
    }
    assert!(server.received_requests().await.unwrap().is_empty());
    for (limit, code, no_validate_parameters) in [("10", "aa", false), ("500", "bb", true)] {
        for dry_run in [false, true] {
            let result = execute(
                &spec,
                call(limit, code),
                ExecutionContext {
                    base_url: Some(server.uri()),
                    dry_run,
                    no_validate_parameters,
                    ..Default::default()
                },
            )
            .await
            .unwrap();
            assert_eq!(matches!(result, ExecutionResult::DryRun { .. }), dry_run);
        }
    }
}

fn configure() -> TempDir {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("spec.json");
    std::fs::write(&path, serde_json::to_vec(&source()).unwrap()).unwrap();
    aperture_cmd()
        .env("APERTURE_CONFIG_DIR", temp.path())
        .args(["config", "add", "constraints", path.to_str().unwrap()])
        .assert()
        .success();
    temp
}

#[test]
fn cli_json_error_and_opt_out_work_before_or_after_operation() {
    let temp = configure();
    let output = aperture_cmd()
        .env("APERTURE_CONFIG_DIR", temp.path())
        .args([
            "--json-errors",
            "api",
            "constraints",
            "--dry-run",
            "items",
            "list-items",
            "--limit",
            "500",
        ])
        .output()
        .unwrap();
    assert!(!output.status.success());
    let error: serde_json::Value = serde_json::from_slice(&output.stderr).unwrap();
    assert_eq!(error["error_type"], "Validation");
    assert!(error.to_string().contains("maximum"));
    for args in [
        vec![
            "api",
            "constraints",
            "--dry-run",
            "--no-validate-parameters",
            "items",
            "list-items",
            "--limit",
            "500",
        ],
        vec![
            "api",
            "constraints",
            "--dry-run",
            "items",
            "list-items",
            "--limit",
            "500",
            "--no-validate-parameters",
        ],
        vec![
            "run",
            "--api",
            "constraints",
            "--dry-run",
            "--no-validate-parameters",
            "listItems",
            "--limit",
            "500",
        ],
    ] {
        aperture_cmd()
            .env("APERTURE_CONFIG_DIR", temp.path())
            .args(args)
            .assert()
            .success();
    }
}

#[test]
fn batch_opt_out_applies_to_concurrent_and_dependent_execution() {
    let temp = configure();
    for dependent in [false, true] {
        let mut operations =
            vec![json!({"id":"first","args":["items","list-items","--limit","500"]})];
        if dependent {
            operations.push(json!({"id":"second","args":["items","list-items","--limit","500"], "depends_on":["first"]}));
        }
        let batch = temp.path().join("batch.json");
        std::fs::write(&batch, json!({"operations":operations}).to_string()).unwrap();
        aperture_cmd()
            .env("APERTURE_CONFIG_DIR", temp.path())
            .args([
                "api",
                "constraints",
                "--dry-run",
                "--no-validate-parameters",
                "--batch-file",
                batch.to_str().unwrap(),
            ])
            .assert()
            .success();
    }
}
