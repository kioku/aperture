use aperture_cli::cli::render::{render_result, render_result_to_string};
use aperture_cli::cli::OutputFormat;
use aperture_cli::invocation::ExecutionResult;
use serde_json::json;
use std::collections::HashMap;

#[test]
fn render_result_to_string_formats_json_success() {
    let result = ExecutionResult::Success {
        body: "{\"id\":1,\"name\":\"Alice\"}".to_string(),
        status: 200,
        headers: HashMap::new(),
    };

    let output = render_result_to_string(&result, &OutputFormat::Json, None)
        .expect("rendering should succeed")
        .expect("output should be present");

    let parsed: serde_json::Value =
        serde_json::from_str(&output).expect("output should be valid JSON");
    assert_eq!(parsed, json!({ "id": 1, "name": "Alice" }));
}

#[test]
fn render_result_to_string_formats_yaml_success() {
    let result = ExecutionResult::Success {
        body: "{\"name\":\"Alice\",\"active\":true}".to_string(),
        status: 200,
        headers: HashMap::new(),
    };

    let output = render_result_to_string(&result, &OutputFormat::Yaml, None)
        .expect("rendering should succeed")
        .expect("output should be present");

    assert!(output.contains("name: Alice"));
    assert!(output.contains("active: true"));
}

#[test]
fn render_result_to_string_formats_table_success() {
    let result = ExecutionResult::Success {
        body: "{\"name\":\"Alice\",\"age\":30}".to_string(),
        status: 200,
        headers: HashMap::new(),
    };

    let output = render_result_to_string(&result, &OutputFormat::Table, None)
        .expect("rendering should succeed")
        .expect("output should be present");

    assert!(output.contains("Key"));
    assert!(output.contains("Value"));
    assert!(output.contains("name"));
    assert!(output.contains("Alice"));
}

#[test]
fn render_result_to_string_applies_jq_filter() {
    let result = ExecutionResult::Success {
        body: "{\"name\":\"Alice\",\"age\":30}".to_string(),
        status: 200,
        headers: HashMap::new(),
    };

    let output = render_result_to_string(&result, &OutputFormat::Json, Some(".name"))
        .expect("rendering should succeed")
        .expect("output should be present");

    let parsed: serde_json::Value =
        serde_json::from_str(&output).expect("filtered output should be valid JSON");
    assert_eq!(parsed, json!("Alice"));
}

#[test]
fn render_result_to_string_formats_dry_run_payload() {
    let result = ExecutionResult::DryRun {
        request_info: json!({
            "dry_run": true,
            "method": "GET",
            "url": "https://api.example.com/users/123"
        }),
    };

    let output = render_result_to_string(&result, &OutputFormat::Json, None)
        .expect("rendering should succeed")
        .expect("output should be present");

    let parsed: serde_json::Value =
        serde_json::from_str(&output).expect("dry-run output should be valid JSON");
    assert_eq!(parsed["method"], "GET");
    assert_eq!(parsed["dry_run"], true);
}

#[test]
fn render_result_to_string_returns_none_for_empty_result() {
    let output = render_result_to_string(&ExecutionResult::Empty, &OutputFormat::Json, None)
        .expect("rendering should succeed");
    assert!(output.is_none());
}

#[test]
fn render_result_handles_empty_result_without_error() {
    render_result(&ExecutionResult::Empty, &OutputFormat::Json, None)
        .expect("empty output should not fail");
}
