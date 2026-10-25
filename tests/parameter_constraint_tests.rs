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

#[tokio::test]
async fn effective_link_queries_are_checked_before_dispatch() {
    use aperture_cli::cache::models::PaginationStrategy;
    use aperture_cli::pagination::execute_paginated;
    for (query, disabled, accepted) in [
        ("limit=500", false, false),
        ("%6cimit=%35%30%30", false, false),
        ("limit=500&limit=10", false, false),
        ("limit=10&limit=500", false, false),
        ("limit=10&limit=20&cursor=a%2Bb&cursor=c", false, true),
        ("limit=500", true, true),
    ] {
        let server = MockServer::start().await;
        Mock::given(wiremock::matchers::query_param("limit", "10"))
            .respond_with(
                ResponseTemplate::new(200)
                    .insert_header(
                        "Link",
                        format!("<{}/items?{query}>; rel=\"next\"", server.uri()),
                    )
                    .set_body_json(json!({"items":[{"id":1}]})),
            )
            .up_to_n_times(1)
            .mount(&server)
            .await;
        Mock::given(wiremock::matchers::method("GET"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({"items":[{"id":2}]})))
            .with_priority(10)
            .mount(&server)
            .await;
        let mut spec = SpecTransformer::new()
            .transform("constraints", &source())
            .unwrap();
        spec.commands[0].pagination.strategy = PaginationStrategy::LinkHeader;
        let mut output = Vec::new();
        let result = execute_paginated(
            &spec,
            call("10", "aa"),
            ExecutionContext {
                base_url: Some(server.uri()),
                auto_paginate: true,
                no_validate_parameters: disabled,
                ..Default::default()
            },
            &mut output,
        )
        .await;
        assert_eq!(result.is_ok(), accepted, "{query}: {result:?}");
        let requests = server.received_requests().await.unwrap();
        assert_eq!(requests.len(), if accepted { 2 } else { 1 }, "{query}");
        assert_eq!(
            output
                .split(|&b| b == b'\n')
                .filter(|line| !line.is_empty())
                .count(),
            if accepted { 2 } else { 1 }
        );
        if accepted {
            assert_eq!(requests[1].url.query(), Some(query));
        } else {
            assert_eq!(result.unwrap_err().to_json().error_type, "Validation");
        }
    }
}

#[tokio::test]
async fn recognized_query_secrets_never_enter_sdk_errors() {
    for name in [
        "access_token",
        "API_KEY",
        "client_secret",
        "password",
        "signature",
        "session_id",
    ] {
        let server = MockServer::start().await;
        let mut spec = SpecTransformer::new()
            .transform("constraints", &source())
            .unwrap();
        spec.commands[0].parameters[0].name = name.into();
        spec.commands[0].parameters[0].schema =
            Some(json!({"type":"string","maxLength":1}).to_string());
        for (dry_run, link) in [(false, false), (false, true), (true, false), (true, true)] {
            let mut invocation = call("10", "aa");
            invocation.query_params.clear();
            if link {
                invocation.pagination_url = Some(
                    format!("{}/items?{}=syntheticQuerySecret291", server.uri(), name)
                        .parse()
                        .unwrap(),
                );
            } else {
                invocation
                    .query_params
                    .insert(name.into(), "syntheticQuerySecret291".into());
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
            .unwrap_err();
            let rendered = format!(
                "{error}\n{error:?}\n{}",
                serde_json::to_string(&error.to_json()).unwrap()
            );
            assert!(!rendered.contains("syntheticQuerySecret291"), "{rendered}");
            assert_eq!(error.to_json().error_type, "Validation");
        }
        assert!(server.received_requests().await.unwrap().is_empty());
    }
}

#[tokio::test]
async fn link_validation_uses_effective_not_stale_query_in_dry_and_live_calls() {
    let server = MockServer::start().await;
    Mock::given(wiremock::matchers::method("GET"))
        .respond_with(ResponseTemplate::new(200))
        .expect(1)
        .mount(&server)
        .await;
    let spec = SpecTransformer::new()
        .transform("constraints", &source())
        .unwrap();
    for dry_run in [false, true] {
        for (original, target, accepted) in [("500", "10", true), ("10", "500", false)] {
            let mut invocation = call(original, "aa");
            invocation.pagination_url = Some(
                format!("{}/items?limit={target}", server.uri())
                    .parse()
                    .unwrap(),
            );
            let result = execute(
                &spec,
                invocation,
                ExecutionContext {
                    base_url: Some(server.uri()),
                    dry_run,
                    ..Default::default()
                },
            )
            .await;
            assert_eq!(result.is_ok(), accepted, "{result:?}");
        }
    }
}

#[tokio::test]
async fn serialized_compound_properties_cannot_bypass_scalar_constraints() {
    let server = MockServer::start().await;
    let mut spec = SpecTransformer::new()
        .transform("constraints", &source())
        .unwrap();
    let mut object = spec.commands[0].parameters[0].clone();
    object.name = "filter".into();
    object.schema_type = Some("object".into());
    object.schema =
        Some(json!({"type":"object","additionalProperties":{"type":"integer"}}).to_string());
    spec.commands[0].parameters.push(object);
    for dry_run in [false, true] {
        let mut invocation = call("10", "aa");
        invocation
            .query_params
            .insert("filter".into(), r#"{"limit":500}"#.into());
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
        assert!(error.to_string().contains("maximum 50"), "{error}");
    }
    assert!(server.received_requests().await.unwrap().is_empty());
}

#[tokio::test]
async fn compound_enum_links_fail_closed_without_guessing_serialization() {
    let server = MockServer::start().await;
    let mut spec = SpecTransformer::new()
        .transform("constraints", &source())
        .unwrap();
    let parameter = &mut spec.commands[0].parameters[0];
    parameter.schema_type = Some("array".into());
    parameter.schema =
        Some(json!({"type":"array","items":{"type":"integer"},"enum":[[1,2]]}).to_string());
    for (link, disabled, accepted) in [
        (false, false, true),
        (true, false, false),
        (true, true, true),
    ] {
        let mut invocation = call("[1,2]", "aa");
        if link {
            invocation.pagination_url = Some(
                format!("{}/items?limit=1&limit=2", server.uri())
                    .parse()
                    .unwrap(),
            );
        }
        let result = execute(
            &spec,
            invocation,
            ExecutionContext {
                base_url: Some(server.uri()),
                dry_run: true,
                no_validate_parameters: disabled,
                ..Default::default()
            },
        )
        .await;
        assert_eq!(result.is_ok(), accepted, "{result:?}");
        if !accepted {
            assert!(result.unwrap_err().to_string().contains("compound enum"));
        }
    }
    assert!(server.received_requests().await.unwrap().is_empty());
}

#[tokio::test]
async fn anonymous_query_secret_errors_are_private_in_cli_and_batch() {
    let server = MockServer::start().await;
    let temp = tempfile::tempdir().unwrap();
    let mut api = serde_json::to_value(source()).unwrap();
    api["servers"] = json!([{"url":server.uri()}]);
    api["paths"]["/items"]["get"]["parameters"][0] =
        json!({"name":"access_token","in":"query","schema":{"type":"string","maxLength":1}});
    let path = temp.path().join("private.json");
    std::fs::write(&path, api.to_string()).unwrap();
    aperture_cmd()
        .env("APERTURE_CONFIG_DIR", temp.path())
        .args(["config", "add", "private", path.to_str().unwrap()])
        .assert()
        .success();
    let batch = temp.path().join("batch.json");
    std::fs::write(&batch, json!({"operations":[{"args":["items","list-items","--access-token","syntheticQuerySecret291"]}]}).to_string()).unwrap();
    for dry_run in [false, true] {
        for batched in [false, true] {
            let mut cmd = aperture_cmd();
            cmd.env("APERTURE_CONFIG_DIR", temp.path()).args([
                "--json-errors",
                "api",
                "private",
                "--no-proxy",
                "--retry",
                "0",
            ]);
            if dry_run {
                cmd.arg("--dry-run");
            }
            if batched {
                cmd.args(["--batch-file", batch.to_str().unwrap()]);
            } else {
                cmd.args([
                    "items",
                    "list-items",
                    "--access-token",
                    "syntheticQuerySecret291",
                ]);
            }
            let output = cmd.output().unwrap();
            assert!(!output.status.success());
            let rendered = format!(
                "{}{}",
                String::from_utf8_lossy(&output.stdout),
                String::from_utf8_lossy(&output.stderr)
            );
            assert!(!rendered.contains("syntheticQuerySecret291"), "{rendered}");
            assert!(rendered.contains("Validation"), "{rendered}");
        }
    }
    assert!(server.received_requests().await.unwrap().is_empty());
}
