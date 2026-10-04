//! Synthetic failed transport requests must not reveal URL credentials.
use aperture_cli::cache::models::CachedSpec;
use aperture_cli::engine::executor::{execute, RetryContext};
use aperture_cli::invocation::{ExecutionContext, OperationCall, ProxyOverride};
use aperture_cli::spec::SpecTransformer;
use serde_json::{json, Value};
use std::collections::HashMap;

fn api(base: &str) -> Value {
    json!({"openapi":"3.0.3", "info":{"title":"redaction","version":"1"},
        "servers":[{"url":base}], "paths":{"/items":{"get":{"operationId":"listItems",
            "tags":["items"], "parameters":[
                {"name":"token","in":"query","schema":{"type":"string"}},
                {"name":"tenant-secret","in":"query","schema":{"type":"string"}}
            ], "responses":{"200":{"description":"ok"}}}}}})
}

fn cached(base: &str) -> CachedSpec {
    SpecTransformer::new()
        .transform("redaction", &serde_json::from_value(api(base)).unwrap())
        .unwrap()
}

fn call() -> OperationCall {
    OperationCall {
        operation_id: "listItems".into(),
        pagination_url: None,
        path_params: HashMap::new(),
        query_params: HashMap::from([
            ("token".into(), "synthetic-query-key".into()),
            ("tenant-secret".into(), "synthetic-custom-key".into()),
        ]),
        header_params: HashMap::new(),
        body: None,
        custom_headers: vec![],
    }
}

fn refused_base() -> String {
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    format!(
        "http://synthetic-user:synthetic-password@{}",
        listener.local_addr().unwrap()
    )
}

fn assert_safe(text: &str) {
    for credential in [
        "synthetic-query-key",
        "synthetic-custom-key",
        "synthetic-user",
        "synthetic-password",
    ] {
        assert!(
            !text.contains(credential),
            "credential appeared in public error"
        );
    }
}

#[tokio::test]
async fn sdk_transport_errors_and_retry_exhaustion_do_not_expose_url_credentials() {
    let spec = cached(&refused_base());
    for retry in [
        None,
        Some(RetryContext {
            max_attempts: 2,
            initial_delay_ms: 1,
            max_delay_ms: 1,
            ..Default::default()
        }),
    ] {
        let error = execute(
            &spec,
            call(),
            ExecutionContext {
                proxy_override: ProxyOverride::Disable,
                retry_context: retry,
                ..Default::default()
            },
        )
        .await
        .unwrap_err();
        assert_eq!(error.to_json().error_type, "Network");
        assert_safe(&error.to_string());
        assert_safe(&format!("{error:?}"));
        assert_safe(&serde_json::to_string(&error.to_json()).unwrap());
    }
}

#[cfg(feature = "integration")]
#[test]
fn cli_json_transport_errors_keep_network_kind_without_url_credentials() {
    let directory = tempfile::tempdir().unwrap();
    let spec_file = directory.path().join("spec.json");
    std::fs::write(
        &spec_file,
        serde_json::to_vec(&api(&refused_base())).unwrap(),
    )
    .unwrap();
    let command = || {
        let mut cmd = std::process::Command::new(env!("CARGO_BIN_EXE_aperture"));
        cmd.env("APERTURE_CONFIG_DIR", directory.path());
        cmd
    };
    let added = command()
        .args(["config", "add", "redaction", spec_file.to_str().unwrap()])
        .output()
        .unwrap();
    assert!(
        added.status.success(),
        "{}",
        String::from_utf8_lossy(&added.stderr)
    );
    let output = command()
        .args([
            "--json-errors",
            "api",
            "redaction",
            "--no-proxy",
            "items",
            "list-items",
            "--token",
            "synthetic-query-key",
            "--tenant-secret",
            "synthetic-custom-key",
        ])
        .output()
        .unwrap();
    assert!(!output.status.success());
    let stderr = String::from_utf8(output.stderr).unwrap();
    let stdout = String::from_utf8(output.stdout).unwrap();
    assert_safe(&stderr);
    assert_safe(&stdout);
    let error: Value = serde_json::from_str(&stderr).unwrap();
    assert_eq!(error["error_type"], "Network", "{error}");
}
