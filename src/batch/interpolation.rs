//! Variable interpolation engine for batch operation arguments.
//!
//! Replaces `{{variable}}` placeholders in operation argument strings with
//! values from the variable store. Scalar variables produce their string
//! value; list variables (from `capture_append`) produce a JSON array literal.

use crate::error::Error;
use std::collections::HashMap;

/// Combined variable store holding both scalar and list captures.
#[derive(Debug, Default)]
pub struct VariableStore {
    /// Scalar variables captured via `capture`.
    pub scalars: HashMap<String, String>,
    /// List variables accumulated via `capture_append`.
    pub lists: HashMap<String, Vec<String>>,
}

impl VariableStore {
    /// Resolves a variable name to its interpolation value.
    ///
    /// - Scalar variables return their string value directly.
    /// - List variables return a JSON array literal (e.g. `["a","b"]`).
    /// - Returns `None` if the variable is not defined.
    fn resolve(&self, name: &str) -> Option<String> {
        if let Some(scalar) = self.scalars.get(name) {
            return Some(scalar.clone());
        }
        if let Some(list) = self.lists.get(name) {
            let json_array = serde_json::to_string(list)
                .expect("serializing Vec<String> to JSON should never fail");
            return Some(json_array);
        }
        None
    }
}

/// Interpolates `{{variable}}` references in an argument string.
///
/// Returns the string with all placeholders replaced, or an error if any
/// referenced variable is undefined.
fn interpolate_arg(arg: &str, store: &VariableStore, operation_id: &str) -> Result<String, Error> {
    let mut result = String::with_capacity(arg.len());
    let mut remaining = arg;

    while let Some(start) = remaining.find("{{") {
        result.push_str(&remaining[..start]);
        let after_open = &remaining[start + 2..];

        let Some(end) = after_open.find("}}") else {
            // Unclosed brace — treat as literal
            result.push_str("{{");
            remaining = after_open;
            continue;
        };

        let var_name = &after_open[..end];
        let value = store
            .resolve(var_name)
            .ok_or_else(|| Error::batch_undefined_variable(operation_id, var_name))?;

        result.push_str(&value);
        remaining = &after_open[end + 2..];
    }

    result.push_str(remaining);
    Ok(result)
}

/// Interpolates all `{{variable}}` references in a list of arguments.
///
/// # Errors
///
/// Returns an error if any argument references an undefined variable.
pub fn interpolate_args(
    args: &[String],
    store: &VariableStore,
    operation_id: &str,
) -> Result<Vec<String>, Error> {
    args.iter()
        .map(|arg| interpolate_arg(arg, store, operation_id))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn store_with_scalar(name: &str, value: &str) -> VariableStore {
        let mut store = VariableStore::default();
        store.scalars.insert(name.into(), value.into());
        store
    }

    fn store_with_list(name: &str, values: &[&str]) -> VariableStore {
        let mut store = VariableStore::default();
        store.lists.insert(
            name.into(),
            values.iter().map(|s| (*s).to_string()).collect(),
        );
        store
    }

    #[test]
    fn scalar_interpolation() {
        let store = store_with_scalar("user_id", "abc-123");
        let args = vec!["--user-id".into(), "{{user_id}}".into()];
        let result = interpolate_args(&args, &store, "test-op").unwrap();
        assert_eq!(result, vec!["--user-id", "abc-123"]);
    }

    #[test]
    fn scalar_embedded_in_string() {
        let store = store_with_scalar("id", "42");
        let args = vec!["prefix-{{id}}-suffix".into()];
        let result = interpolate_args(&args, &store, "test-op").unwrap();
        assert_eq!(result, vec!["prefix-42-suffix"]);
    }

    #[test]
    fn multiple_variables_in_single_arg() {
        let mut store = VariableStore::default();
        store.scalars.insert("a".into(), "1".into());
        store.scalars.insert("b".into(), "2".into());
        let args = vec!["{{a}}-{{b}}".into()];
        let result = interpolate_args(&args, &store, "test-op").unwrap();
        assert_eq!(result, vec!["1-2"]);
    }

    #[test]
    fn list_interpolation_as_json_array() {
        let store = store_with_list("ids", &["id-a", "id-b", "id-c"]);
        let args = vec!["{\"eventIds\": {{ids}}}".into()];
        let result = interpolate_args(&args, &store, "test-op").unwrap();
        assert_eq!(result, vec![r#"{"eventIds": ["id-a","id-b","id-c"]}"#]);
    }

    #[test]
    fn list_interpolation_escapes_json_elements() {
        let store = store_with_list("ids", &["a\"b", "line\nbreak"]);
        let args = vec!["{\"eventIds\": {{ids}}}".into()];

        let result = interpolate_args(&args, &store, "test-op").unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&result[0]).unwrap();

        assert_eq!(parsed["eventIds"][0], "a\"b");
        assert_eq!(parsed["eventIds"][1], "line\nbreak");
    }

    #[test]
    fn empty_list_interpolates_as_empty_array() {
        let store = store_with_list("ids", &[]);
        let args = vec!["{{ids}}".into()];
        let result = interpolate_args(&args, &store, "test-op").unwrap();
        assert_eq!(result, vec!["[]"]);
    }

    #[test]
    fn undefined_variable_produces_error() {
        let store = VariableStore::default();
        let args = vec!["{{missing}}".into()];
        let result = interpolate_args(&args, &store, "my-op");
        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(err.contains("missing"), "expected var name, got: {err}");
        assert!(err.contains("my-op"), "expected op id, got: {err}");
    }

    #[test]
    fn no_variables_passthrough() {
        let store = VariableStore::default();
        let args = vec!["--flag".into(), "value".into()];
        let result = interpolate_args(&args, &store, "test-op").unwrap();
        assert_eq!(result, vec!["--flag", "value"]);
    }

    #[test]
    fn unclosed_brace_treated_as_literal() {
        let store = VariableStore::default();
        let args = vec!["{{unclosed".into()];
        let result = interpolate_args(&args, &store, "test-op").unwrap();
        assert_eq!(result, vec!["{{unclosed"]);
    }

    #[test]
    fn scalar_takes_precedence_over_list() {
        let mut store = VariableStore::default();
        store.scalars.insert("x".into(), "scalar-val".into());
        store.lists.insert("x".into(), vec!["list-val".into()]);
        let args = vec!["{{x}}".into()];
        let result = interpolate_args(&args, &store, "test-op").unwrap();
        assert_eq!(result, vec!["scalar-val"]);
    }
}
