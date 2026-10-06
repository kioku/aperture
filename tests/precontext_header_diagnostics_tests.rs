//! Header preparation failures must be safe before the runtime secret context exists.
use aperture_cli::engine::executor::execute;
use aperture_cli::error::Error;
use aperture_cli::invocation::{ExecutionContext, OperationCall, ProxyOverride};
use aperture_cli::spec::{parser::parse_openapi, transformer::SpecTransformer};
use std::collections::HashMap;
use wiremock::MockServer;

fn rendered(error: &Error) -> String {
    format!(
        "{error}\n{error:?}\n{}",
        serde_json::to_string(&error.to_json()).unwrap()
    )
}

#[test]
fn public_header_errors_never_retain_caller_input_or_reason() {
    for error in [
        Error::invalid_header_format("Authorization Bearer c4-sdk-secret-264"),
        Error::invalid_header_name("X c4-sdk-secret-264", "462-terces-kds-4c"),
        Error::invalid_header_value("X-462-terces-kds-4c", "c4-sdk-secret-264"),
    ] {
        let output = rendered(&error);
        assert!(!output.contains("c4-sdk-secret-264"), "{output}");
        assert!(!output.contains("462-terces-kds-4c"), "{output}");
        assert_eq!(error.to_json().error_type, "Headers");
        assert!(error.to_json().context.is_some());
    }
}

#[tokio::test]
async fn malformed_custom_and_parameter_headers_are_safe_without_network() {
    let server = MockServer::start().await;
    let doc = serde_json::json!({"openapi":"3.0.3","info":{"title":"SDK","version":"1"},
        "servers":[{"url":server.uri()}],"paths":{"/":{"get":{"operationId":"check",
        "responses":{"200":{"description":"ok"}}}}}});
    let spec = SpecTransformer::new()
        .transform("sdk", &parse_openapi(&doc.to_string()).unwrap())
        .unwrap();
    for malformed in [
        "Authorization Bearer c4-sdk-secret-264",
        "X 462-terces-kds-4c: harmless",
        "X-c4-sdk-secret-264: bad\nvalue",
        "X-462-terces-kds-4c: bad\0value",
        "X c4-sdk-secret-264: harmless",
        "X C4-SDK-SECRET-264: harmless",
        "X YzQtc2RrLXNlY3JldC0yNjQ=: harmless",
        "X \u{262f}secret: harmless",
        "",
        " ",
        "none",
        "unknown",
        ": empty",
        "X: bad\rvalue",
        "X: bad\u{1}value",
    ] {
        assert_safe_header_failure(&spec, malformed, false, true).await;
        assert_safe_header_failure(&spec, malformed, false, false).await;
        if !matches!(malformed, "none" | "unknown") {
            assert_safe_header_failure(&spec, malformed, true, true).await;
        }
    }
    assert!(server.received_requests().await.unwrap().is_empty());
}

async fn assert_safe_header_failure(
    spec: &aperture_cli::cache::models::CachedSpec,
    input: &str,
    parameter: bool,
    active: bool,
) {
    let mut call = OperationCall {
        operation_id: "check".into(),
        pagination_url: None,
        path_params: HashMap::default(),
        query_params: HashMap::default(),
        header_params: HashMap::default(),
        body: None,
        custom_headers: if active {
            vec!["Authorization: Bearer c4-sdk-secret-264".into()]
        } else {
            vec![]
        },
    };
    if parameter {
        let (name, value) = input.split_once(':').unwrap_or((input, "harmless"));
        call.header_params.insert(name.into(), value.into());
    } else {
        call.custom_headers.push(input.into());
    }
    let error = execute(
        spec,
        call,
        ExecutionContext {
            proxy_override: ProxyOverride::Disable,
            ..Default::default()
        },
    )
    .await
    .unwrap_err();
    let output = rendered(&error);
    for forbidden in [
        "c4-sdk-secret-264",
        "462-terces-kds-4c",
        "C4-SDK-SECRET-264",
        "YzQtc2RrLXNlY3JldC0yNjQ=",
        "\u{262f}secret",
    ] {
        assert!(!output.contains(forbidden), "{output}");
    }
    assert_eq!(error.to_json().error_type, "Headers", "{output}");
    assert!(error.to_json().context.is_some());
}

#[tokio::test]
async fn authenticated_preparation_omits_url_and_security_reference_inputs() {
    let server = MockServer::start().await;
    let doc = serde_json::json!({"openapi":"3.0.3","info":{"title":"SDK","version":"1"},
        "servers":[{"url":server.uri()}],"paths":{"/":{"get":{"operationId":"check",
        "responses":{"200":{"description":"ok"}}}}}});
    let spec = SpecTransformer::new()
        .transform("sdk", &parse_openapi(&doc.to_string()).unwrap())
        .unwrap();
    let mut bad_path = spec.clone();
    bad_path.commands[0].path = "/{462-terces-kds-4c}".into();
    let mut bad_reference = spec.clone();
    bad_reference.commands[0].security_requirements = vec![vec!["462-terces-kds-4c".into()]];
    for invalid in [bad_path, bad_reference] {
        let error = execute(
            &invalid,
            OperationCall {
                operation_id: "check".into(),
                pagination_url: None,
                path_params: HashMap::default(),
                query_params: HashMap::default(),
                header_params: HashMap::default(),
                body: None,
                custom_headers: vec!["Authorization: Bearer c4-sdk-secret-264".into()],
            },
            ExecutionContext {
                proxy_override: ProxyOverride::Disable,
                ..Default::default()
            },
        )
        .await
        .unwrap_err();
        let output = rendered(&error);
        assert!(!output.contains("462-terces-kds-4c"), "{output}");
        assert!(!output.contains("c4-sdk-secret-264"), "{output}");
    }
    assert!(server.received_requests().await.unwrap().is_empty());
}

#[test]
fn authenticated_translation_omits_body_file_path_but_anonymous_retains_it() {
    let doc = serde_json::json!({"openapi":"3.0.3","info":{"title":"SDK","version":"1"},
        "paths":{"/":{"post":{"operationId":"check","tags":["audit"],
        "requestBody":{"content":{"application/json":{"schema":{"type":"object"}}}},
        "responses":{"200":{"description":"ok"}}}}}});
    let spec = SpecTransformer::new()
        .transform("sdk", &parse_openapi(&doc.to_string()).unwrap())
        .unwrap();
    let missing = tempfile::tempdir().unwrap();
    let path = missing
        .path()
        .join("462-terces-kds-4c")
        .to_string_lossy()
        .into_owned();
    for authenticated in [false, true] {
        let command =
            aperture_cli::engine::generator::generate_command_tree_with_flags(&spec, false);
        let mut args = vec!["sdk", "audit", "check", "--body-file", path.as_str()];
        if authenticated {
            args.extend(["--header", "Authorization: Bearer c4-sdk-secret-264"]);
        }
        let matches = command.try_get_matches_from(args).unwrap();
        let error =
            aperture_cli::cli::translate::matches_to_operation_call(&spec, &matches).unwrap_err();
        assert_eq!(
            rendered(&error).contains("462-terces-kds-4c"),
            !authenticated
        );
    }
}

#[tokio::test]
async fn implicit_url_authentication_omits_preparation_errors() {
    let doc = serde_json::json!({"openapi":"3.0.3","info":{"title":"SDK","version":"1"},
        "paths":{"/":{"get":{"operationId":"check","responses":{"200":{"description":"ok"}}}}}});
    let spec = SpecTransformer::new()
        .transform("sdk", &parse_openapi(&doc.to_string()).unwrap())
        .unwrap();
    let error = execute(
        &spec,
        OperationCall {
            operation_id: "check".into(),
            pagination_url: None,
            path_params: HashMap::default(),
            query_params: HashMap::default(),
            header_params: HashMap::default(),
            body: None,
            custom_headers: vec![],
        },
        ExecutionContext {
            dry_run: true,
            proxy_override: ProxyOverride::Disable,
            base_url: Some("http://user:c4-sdk-secret-264@localhost/{462-terces-kds-4c}".into()),
            ..Default::default()
        },
    )
    .await
    .unwrap_err();
    assert!(!rendered(&error).contains("c4-sdk-secret-264"), "{error:?}");
    assert!(!rendered(&error).contains("462-terces-kds-4c"), "{error:?}");
}

#[tokio::test]
async fn declared_authentication_errors_omit_names_types_and_mapping_inputs() {
    let server = MockServer::start().await;
    let doc = serde_json::json!({"openapi":"3.0.3","info":{"title":"SDK","version":"1"},
        "servers":[{"url":server.uri()}],"components":{"securitySchemes":{"auth":{
        "type":"apiKey","in":"header","name":"X-Key","x-aperture-secret":{
        "source":"env","name":"C4_HEADER_DECLARED_TOKEN"}}}},"security":[{"auth":[]}],
        "paths":{"/":{"get":{"operationId":"check","responses":{"200":{"description":"ok"}}}}}});
    let spec = SpecTransformer::new()
        .transform("sdk", &parse_openapi(&doc.to_string()).unwrap())
        .unwrap();
    std::env::set_var("C4_HEADER_DECLARED_TOKEN", "c4-sdk-secret-264");
    let mut bad_name = spec.clone();
    bad_name
        .security_schemes
        .get_mut("auth")
        .unwrap()
        .parameter_name = Some("X 462-terces-kds-4c".into());
    let mut bad_type = spec.clone();
    bad_type
        .security_schemes
        .get_mut("auth")
        .unwrap()
        .scheme_type = "462-terces-kds-4c".into();
    let mut bad_mapping = spec.clone();
    bad_mapping
        .security_schemes
        .get_mut("auth")
        .unwrap()
        .aperture_secret
        .as_mut()
        .unwrap()
        .name = "C4_MISSING_462-terces-kds-4c".into();
    for invalid in [bad_name, bad_type, bad_mapping] {
        let error = execute(
            &invalid,
            OperationCall {
                operation_id: "check".into(),
                pagination_url: None,
                path_params: HashMap::default(),
                query_params: HashMap::default(),
                header_params: HashMap::default(),
                body: None,
                custom_headers: vec!["Authorization: Bearer c4-sdk-secret-264".into()],
            },
            ExecutionContext {
                proxy_override: ProxyOverride::Disable,
                ..Default::default()
            },
        )
        .await
        .unwrap_err();
        let output = rendered(&error);
        assert!(!output.contains("c4-sdk-secret-264"), "{output}");
        assert!(!output.contains("462-terces-kds-4c"), "{output}");
    }
    std::env::remove_var("C4_HEADER_DECLARED_TOKEN");
    assert!(server.received_requests().await.unwrap().is_empty());
}

#[cfg(feature = "jq")]
#[tokio::test]
async fn authenticated_jq_errors_omit_input_but_success_and_anonymous_errors_remain() {
    let server = MockServer::start().await;
    let doc = serde_json::json!({"openapi":"3.0.3","info":{"title":"SDK","version":"1"},
        "servers":[{"url":server.uri()}],"paths":{"/":{"get":{"operationId":"check",
        "responses":{"200":{"description":"ok"}}}}}});
    let spec = SpecTransformer::new()
        .transform("sdk", &parse_openapi(&doc.to_string()).unwrap())
        .unwrap();
    for active in [true, false] {
        server.reset().await;
        wiremock::Mock::given(wiremock::matchers::method("GET"))
            .respond_with(wiremock::ResponseTemplate::new(200).set_body_json("c4-sdk-secret-264"))
            .mount(&server)
            .await;
        let result = execute(
            &spec,
            OperationCall {
                operation_id: "check".into(),
                pagination_url: None,
                path_params: HashMap::default(),
                query_params: HashMap::default(),
                header_params: HashMap::default(),
                body: None,
                custom_headers: if active {
                    vec!["Authorization: Bearer c4-sdk-secret-264".into()]
                } else {
                    vec![]
                },
            },
            ExecutionContext {
                proxy_override: ProxyOverride::Disable,
                ..Default::default()
            },
        )
        .await
        .unwrap();
        let mut failures = Vec::new();
        for filter in [".foo", ". + 1", ".[0]", "error(.)"] {
            let error = aperture_cli::cli::render::render_result_to_string(
                &result,
                &aperture_cli::cli::OutputFormat::Json,
                Some(filter),
            )
            .unwrap_err();
            if rendered(&error).contains("c4-sdk-secret-264") == active {
                failures.push(rendered(&error));
            }
            assert_eq!(error.to_json().error_type, "Validation");
        }
        assert!(failures.is_empty(), "{failures:?}");
        let output = aperture_cli::cli::render::render_result_to_string(
            &result,
            &aperture_cli::cli::OutputFormat::Json,
            Some("."),
        )
        .unwrap()
        .unwrap();
        assert_eq!(
            serde_json::from_str::<serde_json::Value>(&output).unwrap(),
            "c4-sdk-secret-264"
        );
    }
}

#[cfg(feature = "jq")]
#[test]
fn cached_and_sdk_filter_errors_follow_explicit_sensitivity() {
    for sensitive in [true, false] {
        let result = aperture_cli::invocation::ExecutionResult::Cached {
            body: "\"c4-sdk-secret-264\"".into(),
            status: 200,
            headers: HashMap::default(),
            diagnostics_sensitive: sensitive,
        };
        let error = aperture_cli::cli::render::render_result_to_string(
            &result,
            &aperture_cli::cli::OutputFormat::Json,
            Some(".foo"),
        )
        .unwrap_err();
        assert_eq!(rendered(&error).contains("c4-sdk-secret-264"), !sensitive);
        let output = aperture_cli::cli::render::render_result_to_string(
            &result,
            &aperture_cli::cli::OutputFormat::Json,
            Some("."),
        )
        .unwrap()
        .unwrap();
        assert_eq!(
            serde_json::from_str::<serde_json::Value>(&output).unwrap(),
            "c4-sdk-secret-264"
        );
    }
    for (input, filter) in [
        ("\"462-terces-kds-4c\"", ".foo"),
        ("{\"token\":\"c4-sdk-secret-264\",", "."),
        ("{}", "c4-sdk-secret-264("),
        ("{}", "462-terces-kds-4c("),
    ] {
        let error =
            aperture_cli::engine::executor::apply_jq_filter_with_diagnostics(input, filter, true)
                .unwrap_err();
        let output = rendered(&error);
        assert!(!output.contains("c4-sdk-secret-264"), "{output}");
        assert!(!output.contains("462-terces-kds-4c"), "{output}");
        assert_eq!(error.to_json().error_type, "Validation");
        assert!(error.to_json().context.is_some());
    }
}

#[test]
fn binary_output_errors_omit_untrusted_destination_without_changing_bytes() {
    let directory = tempfile::tempdir().unwrap();
    let result = aperture_cli::invocation::ExecutionResult::Binary {
        body: b"\x00\xffbinary".to_vec(),
        status: 200,
        headers: HashMap::default(),
    };
    for name in ["c4-sdk-secret-264", "462-terces-kds-4c"] {
        let destination = directory.path().join(name);
        std::fs::create_dir(&destination).unwrap();
        let error = aperture_cli::cli::render::render_result_with_binary_destination(
            &result,
            &aperture_cli::cli::OutputFormat::Json,
            None,
            destination.to_str(),
        )
        .unwrap_err();
        let output = rendered(&error);
        assert!(!output.contains(name), "{output}");
        assert_eq!(error.to_json().error_type, "Runtime");
        assert!(error.to_json().context.is_some());
    }
    let destination = directory.path().join("success.bin");
    aperture_cli::cli::render::render_result_with_binary_destination(
        &result,
        &aperture_cli::cli::OutputFormat::Json,
        None,
        destination.to_str(),
    )
    .unwrap();
    assert_eq!(std::fs::read(destination).unwrap(), b"\x00\xffbinary");
}
