//! Resolve URL-parameter schema locations without visiting example/default payloads.
//! Only local component references are supported; the existing resolver checks
//! chained references, while the active set also catches nested reference cycles.

use crate::error::Error;
use openapiv3::OpenAPI;
use serde_json::Value;
use std::collections::HashSet;

pub(super) fn resolve(spec: &OpenAPI, schema: Value) -> Result<Value, Error> {
    resolve_node(spec, schema, &mut HashSet::new(), 0)
}

fn resolve_node(
    spec: &OpenAPI,
    mut schema: Value,
    active: &mut HashSet<String>,
    depth: usize,
) -> Result<Value, Error> {
    if depth > crate::spec::MAX_REFERENCE_DEPTH {
        return Err(Error::validation_error(
            "Parameter schema reference depth exceeded",
        ));
    }
    let Some(reference) = schema.get("$ref").and_then(Value::as_str) else {
        resolve_children(spec, &mut schema, active, depth)?;
        return Ok(schema);
    };
    let reference = reference.to_owned();
    if !active.insert(reference.clone()) {
        return Err(Error::validation_error(
            "Circular parameter schema reference",
        ));
    }
    let resolved = crate::spec::resolve_schema_reference(spec, &reference)?;
    let resolved = serde_json::to_value(resolved)
        .map_err(|error| Error::serialization_error(error.to_string()))?;
    let result = resolve_node(spec, resolved, active, depth + 1);
    active.remove(&reference);
    result
}

fn resolve_children(
    spec: &OpenAPI,
    schema: &mut Value,
    active: &mut HashSet<String>,
    depth: usize,
) -> Result<(), Error> {
    for key in ["items", "additionalProperties", "not"] {
        if let Some(child) = schema.get_mut(key).filter(|value| value.is_object()) {
            *child = resolve_node(spec, child.take(), active, depth + 1)?;
        }
    }
    if let Some(properties) = schema.get_mut("properties").and_then(Value::as_object_mut) {
        for child in properties.values_mut() {
            *child = resolve_node(spec, child.take(), active, depth + 1)?;
        }
    }
    resolve_compositions(spec, schema, active, depth)
}

fn resolve_compositions(
    spec: &OpenAPI,
    schema: &mut Value,
    active: &mut HashSet<String>,
    depth: usize,
) -> Result<(), Error> {
    for key in ["allOf", "anyOf", "oneOf"] {
        if let Some(children) = schema.get_mut(key).and_then(Value::as_array_mut) {
            for child in children {
                *child = resolve_node(spec, child.take(), active, depth + 1)?;
            }
        }
    }
    Ok(())
}

/// Untyped schemas retain opaque string input. Compositions are supported only
/// when every branch has the same string wire shape; compound ambiguity fails.
/// This establishes serialization shape, not full JSON Schema validation.
pub(super) fn supported_shape(schema: &Value) -> bool {
    if schema.get("not").is_some() {
        return false;
    }
    if let Some(kind) = schema.get("type").and_then(Value::as_str) {
        return matches!(
            kind,
            "string" | "integer" | "number" | "boolean" | "array" | "object"
        );
    }
    for key in ["allOf", "anyOf", "oneOf"] {
        if let Some(branches) = schema.get(key).and_then(Value::as_array) {
            return !branches.is_empty() && branches.iter().all(string_shape);
        }
    }
    // A type-less object/array declaration does not provide an input shape.
    !["properties", "items", "additionalProperties"]
        .iter()
        .any(|key| schema.get(key).is_some())
}

fn string_shape(schema: &Value) -> bool {
    schema
        .get("type")
        .and_then(Value::as_str)
        .is_none_or(|kind| kind == "string")
        && supported_shape(schema)
}
