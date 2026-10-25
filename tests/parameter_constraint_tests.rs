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

#[tokio::test]
async fn effective_header_values_cannot_bypass_constraints() {
    let server = MockServer::start().await;
    let spec = SpecTransformer::new()
        .transform("constraints", &source())
        .unwrap();
    for dry_run in [false, true] {
        for custom in [false, true] {
            let mut invocation = call("10", "aa");
            if custom {
                invocation.custom_headers.push("CoDe: bb".into());
            } else {
                invocation.header_params.clear();
                invocation.header_params.insert("CoDe".into(), "bb".into());
            }
            let error = execute(
                &spec,
                invocation,
                ExecutionContext {
                    base_url: Some(server.uri()),
                    dry_run,
                    ..Default::default()
                },
            )
            .await
            .expect_err("effective header must be validated");
            assert_eq!(error.to_json().error_type, "Validation");
            assert!(error.to_string().contains("pattern"));
        }
    }
    // Validate what will be sent, not a parameter value replaced by an overlay.
    let mut invocation = call("10", "bb");
    invocation.custom_headers.push("CoDe: aa".into());
    execute(
        &spec,
        invocation,
        ExecutionContext {
            base_url: Some(server.uri()),
            dry_run: true,
            ..Default::default()
        },
    )
    .await
    .unwrap();
    let mut invocation = call("10", "bb");
    invocation.custom_headers.push("CoDe: bb".into());
    execute(
        &spec,
        invocation,
        ExecutionContext {
            base_url: Some(server.uri()),
            dry_run: true,
            no_validate_parameters: true,
            ..Default::default()
        },
    )
    .await
    .unwrap();
    assert!(server.received_requests().await.unwrap().is_empty());
}

#[tokio::test]
async fn constraint_errors_follow_preparation_sensitivity_policy() {
    let server = MockServer::start().await;
    let mut spec = SpecTransformer::new()
        .transform("constraints", &source())
        .unwrap();
    let parameter = &mut spec.commands[0].parameters[0];
    parameter.schema = Some(json!({"type":"string","maxLength":1}).to_string());
    for location in ["path", "query", "header"] {
        spec.commands[0].parameters[0].location = location.into();
        for dry_run in [false, true] {
            let mut invocation = call("10", "aa");
            invocation.path_params.clear();
            invocation.query_params.clear();
            invocation.header_params.clear();
            let values = match location {
                "path" => &mut invocation.path_params,
                "query" => &mut invocation.query_params,
                _ => &mut invocation.header_params,
            };
            values.insert("limit".into(), "review-secret-291".into());
            invocation
                .custom_headers
                .push("Authorization: Bearer private".into());
            let error = execute(
                &spec,
                invocation,
                ExecutionContext {
                    base_url: Some(server.uri()),
                    dry_run,
                    ..Default::default()
                },
            )
            .await
            .unwrap_err();
            let rendered = format!(
                "{error}\n{error:?}\n{}",
                serde_json::to_string(&error.to_json()).unwrap()
            );
            assert!(!rendered.contains("review-secret-291"), "{rendered}");
            assert_eq!(error.to_json().error_type, "Validation");
        }
    }
    assert!(server.received_requests().await.unwrap().is_empty());
}

#[tokio::test]
async fn declared_credential_headers_omit_constraint_inputs() {
    let server = MockServer::start().await;
    let mut spec = SpecTransformer::new()
        .transform("constraints", &source())
        .unwrap();
    let parameter = &mut spec.commands[0].parameters[1];
    parameter.name = "Authorization".into();
    parameter.schema = Some(json!({"type":"string","maxLength":1}).to_string());
    let mut invocation = call("10", "aa");
    invocation.header_params.clear();
    invocation
        .header_params
        .insert("authorization".into(), "review-secret-291".into());
    let error = execute(
        &spec,
        invocation,
        ExecutionContext {
            base_url: Some(server.uri()),
            dry_run: true,
            ..Default::default()
        },
    )
    .await
    .unwrap_err();
    let rendered = format!(
        "{error}\n{error:?}\n{}",
        serde_json::to_string(&error.to_json()).unwrap()
    );
    assert!(!rendered.contains("review-secret-291"), "{rendered}");
    assert_eq!(error.to_json().error_type, "Validation");
    assert!(server.received_requests().await.unwrap().is_empty());
}

#[tokio::test]
async fn batch_header_overlays_validate_in_both_execution_paths() {
    use aperture_cli::batch::{BatchConfig, BatchFile, BatchProcessor};
    let server = MockServer::start().await;
    let spec = SpecTransformer::new()
        .transform("constraints", &source())
        .unwrap();
    for dependent in [false, true] {
        for disabled in [false, true] {
            let mut second = json!({"id":"second","args":["items","list-items","--limit","10"],"headers":{"CoDe":"bb"}});
            if dependent {
                second["depends_on"] = json!(["first"]);
            }
            let batch: BatchFile = serde_json::from_value(json!({"operations":[
                {"id":"first","args":["items","list-items","--limit","10"],"headers":{"CoDe":"aa"}},
                second
            ]}))
            .unwrap();
            let result = BatchProcessor::new(BatchConfig {
                show_progress: false,
                suppress_output: true,
                ..Default::default()
            })
            .with_no_validate_parameters(disabled)
            .execute_batch(
                &spec,
                batch,
                None,
                Some(&server.uri()),
                true,
                &aperture_cli::cli::OutputFormat::Json,
                None,
            )
            .await
            .unwrap();
            assert!(result.results[0].success);
            assert_eq!(result.results[1].success, disabled);
            if !disabled {
                assert!(result.results[1]
                    .error
                    .as_ref()
                    .unwrap()
                    .contains("Validation"));
            }
        }
    }
    assert!(server.received_requests().await.unwrap().is_empty());
}
