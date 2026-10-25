//! Scalar schema metadata shared by help, capability manifests and validation.

use serde::{Deserialize, Serialize};
use serde_json::{Number, Value};

/// Optional scalar constraints declared by an `OpenAPI` parameter schema.
#[derive(Debug, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct ParameterConstraints {
    /// Inclusive lower numeric bound unless `exclusive_minimum` is true.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub minimum: Option<Number>,
    /// Inclusive upper numeric bound unless `exclusive_maximum` is true.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub maximum: Option<Number>,
    /// Whether the lower numeric bound is exclusive.
    #[serde(skip_serializing_if = "std::ops::Not::not", alias = "exclusiveMinimum")]
    pub exclusive_minimum: bool,
    /// Whether the upper numeric bound is exclusive.
    #[serde(skip_serializing_if = "std::ops::Not::not", alias = "exclusiveMaximum")]
    pub exclusive_maximum: bool,
    /// Minimum string length.
    #[serde(skip_serializing_if = "Option::is_none", alias = "minLength")]
    pub min_length: Option<usize>,
    /// Maximum string length.
    #[serde(skip_serializing_if = "Option::is_none", alias = "maxLength")]
    pub max_length: Option<usize>,
    /// Declared string pattern (not evaluated by help generation).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pattern: Option<String>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(default)]
pub struct ParameterMetadata {
    #[serde(rename = "type")]
    pub schema_type: Option<String>,
    pub description: Option<String>,
    pub format: Option<String>,
    pub default: Option<Value>,
    #[serde(rename = "enum")]
    pub enum_values: Vec<Value>,
    #[serde(flatten)]
    pub constraints: ParameterConstraints,
}

impl ParameterMetadata {
    pub fn from_schema(schema: Option<&str>) -> Self {
        schema
            .and_then(|schema| serde_json::from_str::<Value>(schema).ok())
            .map(unwrap_schema_wrapper)
            .and_then(|schema| serde_json::from_value(schema).ok())
            .unwrap_or_default()
    }

    pub fn help(&self, fallback: &str) -> String {
        let description = self.description.as_deref().unwrap_or(fallback);
        let mut details = self.constraints.help_details();
        if !self.enum_values.is_empty() {
            let values: Vec<_> = self.enum_values.iter().map(display_value).collect();
            details.push(format!("possible values: {}", values.join(", ")));
        }
        if let Some(default) = &self.default {
            details.push(format!("default: {}", display_value(default)));
        }
        if details.is_empty() {
            description.to_owned()
        } else {
            format!("{description} [{}]", details.join(", "))
        }
    }
}

impl ParameterConstraints {
    fn help_details(&self) -> Vec<String> {
        let mut details = Vec::new();
        append_numeric_bound(
            &mut details,
            "minimum",
            self.minimum.as_ref(),
            self.exclusive_minimum,
        );
        append_numeric_bound(
            &mut details,
            "maximum",
            self.maximum.as_ref(),
            self.exclusive_maximum,
        );
        if let Some(length) = self.min_length {
            details.push(format!("min length: {length}"));
        }
        if let Some(length) = self.max_length {
            details.push(format!("max length: {length}"));
        }
        if let Some(pattern) = &self.pattern {
            details.push(format!("pattern: {pattern}"));
        }
        details
    }
}

fn append_numeric_bound(
    details: &mut Vec<String>,
    name: &str,
    bound: Option<&Number>,
    exclusive: bool,
) {
    if let Some(bound) = bound {
        let qualifier = if exclusive { "exclusive " } else { "" };
        details.push(format!("{qualifier}{name}: {bound}"));
    }
}

fn display_value(value: &Value) -> String {
    value
        .as_str()
        .map_or_else(|| value.to_string(), str::to_owned)
}

/// A single-branch allOf is commonly used to annotate a referenced schema.
/// Unwrap only annotation-only wrappers: merging sibling constraints or multiple
/// branches would require intersection semantics, not last-value-wins merging.
fn unwrap_schema_wrapper(mut schema: Value) -> Value {
    for _ in 0..crate::spec::MAX_REFERENCE_DEPTH {
        if !is_annotation_wrapper(&schema) {
            break;
        }
        let Some(mut branches) = schema["allOf"].as_array_mut().map(std::mem::take) else {
            break;
        };
        let mut inner = branches.remove(0);
        let Some(object) = inner.as_object_mut() else {
            break;
        };
        let Some(annotations) = schema.as_object() else {
            break;
        };
        object.extend(
            annotations
                .iter()
                .filter(|(key, _)| key.as_str() != "allOf")
                .map(|(key, value)| (key.clone(), value.clone())),
        );
        schema = inner;
    }
    schema
}

fn is_annotation_wrapper(schema: &Value) -> bool {
    schema["allOf"]
        .as_array()
        .is_some_and(|v| v.len() == 1 && v[0].is_object())
        && schema.as_object().is_some_and(|object| {
            object.keys().all(|key| {
                matches!(
                    key.as_str(),
                    "allOf"
                        | "description"
                        | "title"
                        | "default"
                        | "example"
                        | "deprecated"
                        | "readOnly"
                        | "writeOnly"
                        | "nullable"
                        | "externalDocs"
                        | "xml"
                ) || key.starts_with("x-")
            })
        })
}

#[cfg(test)]
mod tests {
    use super::ParameterMetadata;
    use serde_json::json;

    #[test]
    fn malformed_metadata_falls_back_without_panicking() {
        for schema in [
            "",
            " ",
            "none",
            "{",
            "null",
            "[]",
            r#"{"enum":"invalid"}"#,
            r#"{"minLength":-1}"#,
            r#"{"exclusiveMinimum":0.5}"#,
        ] {
            assert_eq!(
                ParameterMetadata::from_schema(Some(schema)).help("fallback"),
                "fallback"
            );
        }
        assert_eq!(
            ParameterMetadata::from_schema(None).help("fallback"),
            "fallback"
        );
    }

    #[test]
    fn only_transparent_all_of_wrappers_are_unwrapped() {
        let leaf = json!({"type": "string", "description": "Code.", "minLength": 2});
        let wrapped = json!({"allOf": [{"allOf": [leaf]}]});
        assert_eq!(
            ParameterMetadata::from_schema(Some(&wrapped.to_string())).help("fallback"),
            "Code. [min length: 2]"
        );
        for schema in [
            json!({"anyOf": [leaf]}),
            json!({"oneOf": [leaf]}),
            json!({"allOf": [leaf, {"maxLength": 8}]}),
            json!({"allOf": [leaf], "minLength": 1}),
        ] {
            let metadata = ParameterMetadata::from_schema(Some(&schema.to_string()));
            assert!(metadata.description.is_none());
            assert_ne!(metadata.constraints.min_length, Some(2));
        }
        let schema = json!({"description": "Valid.", "unknown": ["none"], "enum": ["", " ", "none", "none"]});
        let metadata = ParameterMetadata::from_schema(Some(&schema.to_string()));
        assert_eq!(
            metadata.help("fallback"),
            "Valid. [possible values: ,  , none, none]"
        );
    }
}
