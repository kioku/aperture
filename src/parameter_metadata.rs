//! Display-only metadata shared by generated help and capability manifests.

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
            .and_then(|schema| serde_json::from_str(schema).ok())
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
