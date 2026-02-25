//! JQ-based value extraction from operation responses.
//!
//! Applies JQ queries from `capture` and `capture_append` fields to an
//! operation's response body and stores results in the [`VariableStore`].

use crate::batch::interpolation::VariableStore;
use crate::batch::BatchOperation;
use crate::engine::executor::apply_jq_filter;
use crate::error::Error;

/// Extracts captured values from a response and stores them in the variable store.
///
/// For `capture` entries, the JQ result is stored as a scalar string.
/// For `capture_append` entries, the JQ result is appended to a list.
///
/// # Errors
///
/// Returns an error if JQ evaluation fails or produces no output.
pub fn extract_captures(
    operation: &BatchOperation,
    response_body: &str,
    store: &mut VariableStore,
) -> Result<(), Error> {
    let op_id = operation.id.as_deref().unwrap_or("<unnamed>");

    if let Some(captures) = &operation.capture {
        for (var_name, jq_query) in captures {
            let value = run_jq_capture(op_id, var_name, jq_query, response_body)?;
            store.scalars.insert(var_name.clone(), value);
        }
    }

    if let Some(appends) = &operation.capture_append {
        for (list_name, jq_query) in appends {
            let value = run_jq_capture(op_id, list_name, jq_query, response_body)?;
            store
                .lists
                .entry(list_name.clone())
                .or_default()
                .push(value);
        }
    }

    Ok(())
}

/// Runs a single JQ query and returns the extracted string value.
///
/// Strips surrounding quotes from JSON string results so that
/// interpolation produces clean values (e.g. `abc-123` not `"abc-123"`).
fn run_jq_capture(
    operation_id: &str,
    var_name: &str,
    jq_query: &str,
    response_body: &str,
) -> Result<String, Error> {
    let raw = apply_jq_filter(response_body, jq_query)
        .map_err(|e| Error::batch_capture_failed(operation_id, var_name, e.to_string()))?;

    let trimmed = raw.trim();
    if trimmed == "null" || trimmed.is_empty() {
        return Err(Error::batch_capture_failed(
            operation_id,
            var_name,
            format!("JQ query '{jq_query}' returned null or empty"),
        ));
    }

    // Strip surrounding quotes from JSON string values
    Ok(strip_json_quotes(trimmed))
}

/// Converts JQ output into the scalar representation used by interpolation.
///
/// If the output is a JSON string literal, decode it so escape sequences are
/// interpreted (`"a\\\"b"` → `a"b`). Non-string JSON values are preserved
/// as their textual representation (`42` → `42`, `true` → `true`).
fn strip_json_quotes(s: &str) -> String {
    if s.len() >= 2 && s.starts_with('"') && s.ends_with('"') {
        return serde_json::from_str::<String>(s).unwrap_or_else(|_| s[1..s.len() - 1].to_string());
    }
    s.to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    fn op_with_capture(id: &str, captures: &[(&str, &str)]) -> BatchOperation {
        BatchOperation {
            id: Some(id.into()),
            capture: Some(
                captures
                    .iter()
                    .map(|(k, v)| ((*k).into(), (*v).into()))
                    .collect(),
            ),
            ..Default::default()
        }
    }

    fn op_with_capture_append(id: &str, appends: &[(&str, &str)]) -> BatchOperation {
        BatchOperation {
            id: Some(id.into()),
            capture_append: Some(
                appends
                    .iter()
                    .map(|(k, v)| ((*k).into(), (*v).into()))
                    .collect(),
            ),
            ..Default::default()
        }
    }

    #[test]
    fn extract_scalar_from_json_object() {
        let op = op_with_capture("create-user", &[("user_id", ".id")]);
        let response = r#"{"id": "abc-123", "name": "Alice"}"#;
        let mut store = VariableStore::default();
        extract_captures(&op, response, &mut store).unwrap();
        assert_eq!(store.scalars.get("user_id").unwrap(), "abc-123");
    }

    #[test]
    fn extract_numeric_scalar() {
        let op = op_with_capture("get-count", &[("count", ".total")]);
        let response = r#"{"total": 42}"#;
        let mut store = VariableStore::default();
        extract_captures(&op, response, &mut store).unwrap();
        assert_eq!(store.scalars.get("count").unwrap(), "42");
    }

    #[test]
    fn extract_string_scalar_unescapes_json_string() {
        let op = op_with_capture("create-user", &[("user_id", ".id")]);
        let response = r#"{"id": "a\"b"}"#;
        let mut store = VariableStore::default();

        extract_captures(&op, response, &mut store).unwrap();

        assert_eq!(store.scalars.get("user_id").unwrap(), "a\"b");
    }

    #[test]
    fn extract_nested_field() {
        let op = op_with_capture("deep", &[("val", ".data.nested.value")]);
        let response = r#"{"data": {"nested": {"value": "deep-val"}}}"#;
        let mut store = VariableStore::default();
        extract_captures(&op, response, &mut store).unwrap();
        assert_eq!(store.scalars.get("val").unwrap(), "deep-val");
    }

    #[test]
    fn capture_append_accumulates_values() {
        let op1 = op_with_capture_append("beat-1", &[("ids", ".id")]);
        let op2 = op_with_capture_append("beat-2", &[("ids", ".id")]);
        let mut store = VariableStore::default();

        extract_captures(&op1, r#"{"id": "first"}"#, &mut store).unwrap();
        extract_captures(&op2, r#"{"id": "second"}"#, &mut store).unwrap();

        let list = store.lists.get("ids").unwrap();
        assert_eq!(list, &["first", "second"]);
    }

    #[test]
    fn null_capture_returns_error() {
        let op = op_with_capture("test-op", &[("val", ".missing_field")]);
        let response = r#"{"other": "data"}"#;
        let mut store = VariableStore::default();
        let result = extract_captures(&op, response, &mut store);
        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(
            err.contains("null or empty"),
            "expected null error, got: {err}"
        );
    }

    #[test]
    fn invalid_jq_returns_error() {
        let op = op_with_capture("test-op", &[("val", "invalid..query")]);
        let response = r#"{"id": "test"}"#;
        let mut store = VariableStore::default();
        let result = extract_captures(&op, response, &mut store);
        // Should error — either from jq parsing or from our validation
        assert!(result.is_err());
    }

    #[test]
    fn mixed_capture_and_append() {
        let op = BatchOperation {
            id: Some("mixed".into()),
            capture: Some(HashMap::from([("scalar_id".into(), ".id".into())])),
            capture_append: Some(HashMap::from([("list_ids".into(), ".id".into())])),
            ..Default::default()
        };
        let mut store = VariableStore::default();
        extract_captures(&op, r#"{"id": "val-1"}"#, &mut store).unwrap();
        assert_eq!(store.scalars.get("scalar_id").unwrap(), "val-1");
        assert_eq!(store.lists.get("list_ids").unwrap(), &["val-1"]);
    }

    #[test]
    fn no_captures_is_noop() {
        let op = BatchOperation {
            id: Some("plain".into()),
            ..Default::default()
        };
        let mut store = VariableStore::default();
        extract_captures(&op, r#"{"id": "test"}"#, &mut store).unwrap();
        assert!(store.scalars.is_empty());
        assert!(store.lists.is_empty());
    }
}
