//! Compatibility normalization follows `OpenAPI` structure, never arbitrary payloads.
use serde_json::Value;

#[derive(Clone, Copy)]
enum Context {
    Root,
    Components,
    Path,
    Operation,
    Parameter,
    Header,
    Body,
    Response,
    Media,
    Encoding,
    Schema,
}

const fn boolean_fields(context: Context, openapi31: bool) -> &'static [&'static str] {
    match context {
        Context::Operation => &["deprecated"],
        Context::Parameter => &[
            "required",
            "deprecated",
            "allowEmptyValue",
            "explode",
            "allowReserved",
        ],
        Context::Header => &["required", "deprecated", "explode"],
        Context::Body => &["required"],
        Context::Encoding => &["explode", "allowReserved"],
        Context::Schema => schema_boolean_fields(openapi31),
        _ => &[],
    }
}

const fn schema_boolean_fields(openapi31: bool) -> &'static [&'static str] {
    if openapi31 {
        &["deprecated", "readOnly", "writeOnly", "uniqueItems"]
    } else {
        &[
            "deprecated",
            "readOnly",
            "writeOnly",
            "nullable",
            "uniqueItems",
            "exclusiveMinimum",
            "exclusiveMaximum",
        ]
    }
}

fn normalize(value: &mut Value, context: Context, openapi31: bool) {
    let Some(object) = value.as_object_mut() else {
        return;
    };
    for field in boolean_fields(context, openapi31) {
        if let Some(value) = object.get_mut(*field) {
            match value.as_u64() {
                Some(0) => *value = Value::Bool(false),
                Some(1) => *value = Value::Bool(true),
                _ => {}
            }
        }
    }
    descend(value, context, openapi31);
}

fn child(value: &mut Value, key: &str, context: Context, openapi31: bool) {
    if let Some(value) = value.get_mut(key) {
        normalize(value, context, openapi31);
    }
}

fn map(value: &mut Value, key: &str, context: Context, openapi31: bool) {
    let Some(object) = value.get_mut(key).and_then(Value::as_object_mut) else {
        return;
    };
    for (name, value) in object {
        // Paths and Responses objects allow extensions alongside their entries.
        // Other maps use arbitrary names, including x-prefixed schema/header names.
        if matches!(key, "paths" | "responses") && name.starts_with("x-") {
            continue;
        }
        normalize(value, context, openapi31);
    }
}

fn array(value: &mut Value, key: &str, context: Context, openapi31: bool) {
    if let Some(array) = value.get_mut(key).and_then(Value::as_array_mut) {
        for value in array {
            normalize(value, context, openapi31);
        }
    }
}

fn descend(value: &mut Value, context: Context, version: bool) {
    match context {
        Context::Root => root(value, version),
        Context::Components | Context::Schema => descend_definition(value, context, version),
        Context::Path => path(value, version),
        Context::Operation => operation(value, version),
        Context::Parameter | Context::Header => {
            child(value, "schema", Context::Schema, version);
            map(value, "content", Context::Media, version);
        }
        Context::Body | Context::Response => response_or_body(value, context, version),
        Context::Media => {
            child(value, "schema", Context::Schema, version);
            map(value, "encoding", Context::Encoding, version);
        }
        Context::Encoding => map(value, "headers", Context::Header, version),
    }
}

fn root(value: &mut Value, version: bool) {
    child(value, "components", Context::Components, version);
    map(value, "paths", Context::Path, version);
    if version {
        map(value, "webhooks", Context::Path, version);
    }
}

fn response_or_body(value: &mut Value, context: Context, version: bool) {
    map(value, "content", Context::Media, version);
    if matches!(context, Context::Response) {
        map(value, "headers", Context::Header, version);
    }
}

fn descend_definition(value: &mut Value, context: Context, version: bool) {
    match context {
        Context::Components => components(value, version),
        Context::Schema => schema(value, version),
        _ => {}
    }
}

fn path(value: &mut Value, version: bool) {
    for method in [
        "get", "put", "post", "delete", "options", "head", "patch", "trace",
    ] {
        child(value, method, Context::Operation, version);
    }
    array(value, "parameters", Context::Parameter, version);
}

fn operation(value: &mut Value, version: bool) {
    array(value, "parameters", Context::Parameter, version);
    child(value, "requestBody", Context::Body, version);
    map(value, "responses", Context::Response, version);
    callbacks(value, version);
}

fn components(value: &mut Value, version: bool) {
    for (key, context) in [
        ("schemas", Context::Schema),
        ("parameters", Context::Parameter),
        ("headers", Context::Header),
        ("requestBodies", Context::Body),
        ("responses", Context::Response),
    ] {
        map(value, key, context, version);
    }
    if version {
        map(value, "pathItems", Context::Path, version);
    }
    callbacks(value, version);
}

fn callbacks(value: &mut Value, version: bool) {
    let Some(callbacks) = value.get_mut("callbacks").and_then(Value::as_object_mut) else {
        return;
    };
    // Callback names are arbitrary, including x-prefixed names. Extensions
    // inside Callback Objects are excluded by callback_paths itself.
    for callback in callbacks.values_mut() {
        callback_paths(callback, version);
    }
}

fn callback_paths(value: &mut Value, version: bool) {
    let Some(expressions) = value.as_object_mut() else {
        return;
    };
    for (expression, path) in expressions {
        if expression.starts_with('{') {
            normalize(path, Context::Path, version);
        }
    }
}

fn schema(value: &mut Value, version: bool) {
    map(value, "properties", Context::Schema, version);
    for key in ["items", "additionalProperties", "not"] {
        child(value, key, Context::Schema, version);
    }
    for key in ["allOf", "anyOf", "oneOf"] {
        array(value, key, Context::Schema, version);
    }
    if version {
        schema31(value, version);
    }
}

fn schema31(value: &mut Value, version: bool) {
    for key in ["patternProperties", "$defs", "dependentSchemas"] {
        map(value, key, Context::Schema, version);
    }
    for key in [
        "if",
        "then",
        "else",
        "contains",
        "propertyNames",
        "unevaluatedProperties",
        "unevaluatedItems",
        "contentSchema",
    ] {
        child(value, key, Context::Schema, version);
    }
    array(value, "prefixItems", Context::Schema, version);
}

pub(super) fn normalize_document(value: &mut Value) {
    let version = value
        .get("openapi")
        .and_then(Value::as_str)
        .is_some_and(|v| v.starts_with("3.1"));
    normalize(value, Context::Root, version);
}
