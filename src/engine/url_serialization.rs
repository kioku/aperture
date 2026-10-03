//! `OpenAPI` URL parameter expansion. The CLI carries strings; compound inputs
//! use JSON arrays/objects of primitive values, never delimiter-splitting guesses.

use crate::cache::models::CachedParameter;
use crate::error::Error;
use serde_json::Value;

/// Parsed input before style delimiters are introduced or data is encoded.
enum ParameterValue {
    Scalar(String),
    Array(Vec<String>),
    Object(Vec<(String, String)>),
}

fn invalid(parameter: &CachedParameter, reason: &str) -> Error {
    Error::validation_error(format!("Parameter '{}': {reason}", parameter.name))
}

fn primitive(value: &Value, parameter: &CachedParameter) -> Result<String, Error> {
    match value {
        Value::String(value) => Ok(value.clone()),
        Value::Bool(_) | Value::Number(_) => Ok(value.to_string()),
        _ => Err(invalid(
            parameter,
            "expected a non-null primitive; nested arrays/objects are unsupported",
        )),
    }
}

fn cached_schema(parameter: &CachedParameter) -> Result<Option<Value>, Error> {
    parameter
        .schema
        .as_deref()
        .map(|schema| {
            serde_json::from_str(schema)
                .map_err(|_| invalid(parameter, "invalid cached parameter schema"))
        })
        .transpose()
}

fn typed_primitive(
    value: &Value,
    schema: Option<&Value>,
    parameter: &CachedParameter,
) -> Result<String, Error> {
    let Some(schema) = schema else {
        return primitive(value, parameter);
    };
    if ["$ref", "oneOf", "anyOf", "allOf", "not"]
        .iter()
        .any(|key| schema.get(key).is_some())
    {
        return Err(invalid(
            parameter,
            "referenced or composed item/property schemas are unsupported",
        ));
    }
    let valid = primitive_type_matches(value, schema.get("type").and_then(Value::as_str));
    if !valid {
        return Err(invalid(
            parameter,
            "item/property does not match its declared primitive type",
        ));
    }
    primitive(value, parameter)
}

fn primitive_type_matches(value: &Value, kind: Option<&str>) -> bool {
    match kind {
        Some("string") => value.is_string(),
        Some("integer") => value.is_i64() || value.is_u64(),
        Some("number") => value.is_number(),
        Some("boolean") => value.is_boolean(),
        None => true,
        _ => false,
    }
}

fn property_schema<'a>(schema: Option<&'a Value>, key: &str) -> Option<&'a Value> {
    let schema = schema?;
    schema
        .get("properties")
        .and_then(|properties| properties.get(key))
        .or_else(|| {
            schema
                .get("additionalProperties")
                .filter(|value| value.is_object())
        })
}

fn json_input(parameter: &CachedParameter, raw: &str) -> Result<Value, Error> {
    serde_json::from_str(raw)
        .map_err(|_| invalid(parameter, "expected valid JSON for the declared type"))
}

fn parse_value(parameter: &CachedParameter, raw: &str) -> Result<ParameterValue, Error> {
    if parameter.serialization.unsupported_schema {
        return Err(invalid(
            parameter,
            "parameter schema does not provide a supported unambiguous serialization shape",
        ));
    }
    if parameter.serialization.content_based {
        return Err(invalid(
            parameter,
            "content-based parameter serialization is unsupported",
        ));
    }
    match parameter.schema_type.as_deref().unwrap_or("string") {
        "string" => Ok(ParameterValue::Scalar(raw.to_string())),
        "array" => parse_array(parameter, raw),
        "object" => parse_object(parameter, raw),
        "integer" | "number" | "boolean" => parse_typed_scalar(parameter, raw),
        _ => Err(invalid(parameter, "unsupported parameter schema type")),
    }
}

fn parse_array(parameter: &CachedParameter, raw: &str) -> Result<ParameterValue, Error> {
    let value = json_input(parameter, raw)?;
    let values = value
        .as_array()
        .ok_or_else(|| invalid(parameter, "expected a JSON array"))?;
    let schema = cached_schema(parameter)?;
    let item_schema = schema.as_ref().and_then(|schema| schema.get("items"));
    values
        .iter()
        .map(|value| typed_primitive(value, item_schema, parameter))
        .collect::<Result<_, _>>()
        .map(ParameterValue::Array)
}

fn parse_object(parameter: &CachedParameter, raw: &str) -> Result<ParameterValue, Error> {
    let value = json_input(parameter, raw)?;
    let values = value
        .as_object()
        .ok_or_else(|| invalid(parameter, "expected a JSON object"))?;
    let schema = cached_schema(parameter)?;
    // Sort property names so serialization is stable regardless of JSON map backend.
    let mut pairs = values
        .iter()
        .map(|(key, value)| {
            typed_primitive(value, property_schema(schema.as_ref(), key), parameter)
                .map(|value| (key.clone(), value))
        })
        .collect::<Result<Vec<_>, _>>()?;
    pairs.sort_by(|left, right| left.0.cmp(&right.0));
    Ok(ParameterValue::Object(pairs))
}

fn parse_typed_scalar(parameter: &CachedParameter, raw: &str) -> Result<ParameterValue, Error> {
    let value = json_input(parameter, raw)?;
    let valid = primitive_type_matches(&value, parameter.schema_type.as_deref());
    if !valid {
        return Err(invalid(
            parameter,
            "input does not match the declared primitive type",
        ));
    }
    primitive(&value, parameter).map(ParameterValue::Scalar)
}

fn encode(value: &str) -> String {
    urlencoding::encode(value).into_owned()
}

fn encode_component(value: &str, dots: bool) -> String {
    let encoded = encode(value);
    if dots {
        encoded.replace('.', "%2E")
    } else {
        encoded
    }
}

fn encode_value(value: ParameterValue, dots: bool) -> ParameterValue {
    match value {
        ParameterValue::Scalar(value) => ParameterValue::Scalar(encode_component(&value, dots)),
        ParameterValue::Array(values) => ParameterValue::Array(
            values
                .iter()
                .map(|value| encode_component(value, dots))
                .collect(),
        ),
        ParameterValue::Object(values) => ParameterValue::Object(
            values
                .iter()
                .map(|(key, value)| (encode_component(key, dots), encode_component(value, dots)))
                .collect(),
        ),
    }
}

fn object_text(values: &[(String, String)], explode: bool, separator: &str) -> String {
    if explode {
        values
            .iter()
            .map(|(key, value)| format!("{key}={value}"))
            .collect::<Vec<_>>()
            .join(separator)
    } else {
        values
            .iter()
            .flat_map(|(key, value)| [key.as_str(), value.as_str()])
            .collect::<Vec<_>>()
            .join(",")
    }
}

fn delimited_path(value: &ParameterValue, explode: bool, label: bool) -> String {
    let separator = if label && explode { "." } else { "," };
    let text = match value {
        ParameterValue::Scalar(value) => value.clone(),
        ParameterValue::Array(values) => values.join(separator),
        ParameterValue::Object(values) => object_text(values, explode, separator),
    };
    if label {
        format!(".{text}")
    } else {
        text
    }
}

fn matrix_pair(name: &str, value: &str) -> String {
    if value.is_empty() {
        format!(";{name}")
    } else {
        format!(";{name}={value}")
    }
}

fn matrix_path(name: &str, value: &ParameterValue, explode: bool) -> String {
    if explode {
        return exploded_matrix(name, value);
    }
    let text = match value {
        ParameterValue::Scalar(value) => value.clone(),
        ParameterValue::Array(values) => values.join(","),
        ParameterValue::Object(values) => object_text(values, false, ","),
    };
    matrix_pair(name, &text)
}

fn exploded_matrix(name: &str, value: &ParameterValue) -> String {
    match value {
        ParameterValue::Scalar(value) => matrix_pair(name, value),
        ParameterValue::Array(values) => values
            .iter()
            .map(|value| matrix_pair(name, value))
            .collect(),
        ParameterValue::Object(values) => values
            .iter()
            .map(|(key, value)| matrix_pair(key, value))
            .collect(),
    }
}

const fn empty_collection(value: &ParameterValue) -> bool {
    match value {
        ParameterValue::Scalar(_) => false,
        ParameterValue::Array(values) => values.is_empty(),
        ParameterValue::Object(values) => values.is_empty(),
    }
}

/// Encode components before inserting style punctuation; escaped user data cannot
/// create extra segments, query strings, matrix properties, or fragments.
pub(super) fn path_value(parameter: &CachedParameter, raw: &str) -> Result<String, Error> {
    let style = parameter.serialization.style.as_deref().unwrap_or("simple");
    let explode = parameter.serialization.explode.unwrap_or(false);
    let value = encode_value(parse_value(parameter, raw)?, style == "label" && explode);
    let text = match style {
        "simple" => delimited_path(&value, explode, false),
        "label" => delimited_path(&value, explode, true),
        "matrix" => matrix_path(&encode(&parameter.name), &value, explode),
        _ => return Err(invalid(parameter, "unsupported path serialization style")),
    };
    if empty_collection(&value) {
        Ok(String::new())
    } else {
        Ok(text)
    }
}

fn form_query(name: &str, value: ParameterValue, explode: bool) -> Vec<(String, String)> {
    match value {
        ParameterValue::Scalar(value) => vec![(name.to_string(), value)],
        ParameterValue::Array(values) if explode => values
            .into_iter()
            .map(|value| (name.to_string(), value))
            .collect(),
        ParameterValue::Array(values) => vec![(name.to_string(), values.join(","))],
        ParameterValue::Object(values) if explode => values,
        ParameterValue::Object(values) => {
            vec![(name.to_string(), object_text(&values, false, ","))]
        }
    }
}

fn checked_form_query(
    parameter: &CachedParameter,
    value: ParameterValue,
    explode: bool,
) -> Result<Vec<(String, String)>, Error> {
    if !explode && contains_comma(&value) {
        return Err(invalid(
            parameter,
            "unexploded query data contains a comma; use exploded form instead",
        ));
    }
    if empty_collection(&value) {
        return Ok(Vec::new());
    }
    Ok(form_query(&parameter.name, value, explode))
}

fn contains_comma(value: &ParameterValue) -> bool {
    match value {
        ParameterValue::Scalar(_) => false,
        ParameterValue::Array(values) => values.iter().any(|value| value.contains(',')),
        ParameterValue::Object(values) => values
            .iter()
            .any(|(key, value)| key.contains(',') || value.contains(',')),
    }
}

fn array_query(
    parameter: &CachedParameter,
    value: ParameterValue,
    explode: bool,
    separator: &str,
) -> Result<Vec<(String, String)>, Error> {
    if explode {
        return Err(invalid(
            parameter,
            "delimited query arrays require explode=false",
        ));
    }
    let ParameterValue::Array(values) = value else {
        return Err(invalid(
            parameter,
            "delimited query style requires an array",
        ));
    };
    if values.iter().any(|value| value.contains(separator)) {
        return Err(invalid(
            parameter,
            "query array data contains its style delimiter; use exploded form instead",
        ));
    }
    if values.is_empty() {
        return Ok(Vec::new());
    }
    Ok(vec![(parameter.name.clone(), values.join(separator))])
}

fn deep_query(
    parameter: &CachedParameter,
    value: ParameterValue,
    explode: bool,
) -> Result<Vec<(String, String)>, Error> {
    if !explode {
        return Err(invalid(parameter, "deepObject requires explode=true"));
    }
    let ParameterValue::Object(values) = value else {
        return Err(invalid(parameter, "deepObject requires an object"));
    };
    if values.iter().any(|(key, _)| key.contains(['[', ']'])) {
        return Err(invalid(
            parameter,
            "deepObject property names containing brackets are unsupported",
        ));
    }
    Ok(values
        .into_iter()
        .map(|(key, value)| (format!("{}[{key}]", parameter.name), value))
        .collect())
}

/// Return decoded pairs, preserving repeated keys; `Url::query_pairs_mut` handles
/// escaping keys and values exactly once, including style separators.
pub(super) fn query_values(
    parameter: &CachedParameter,
    raw: &str,
) -> Result<Vec<(String, String)>, Error> {
    if parameter.serialization.allow_reserved {
        return Err(invalid(
            parameter,
            "allowReserved=true is unsupported; reserved query data must be encoded",
        ));
    }
    let value = parse_value(parameter, raw)?;
    let style = parameter.serialization.style.as_deref().unwrap_or("form");
    let explode = parameter.serialization.explode.unwrap_or(style == "form");
    match style {
        "form" => checked_form_query(parameter, value, explode),
        "spaceDelimited" => array_query(parameter, value, explode, " "),
        "pipeDelimited" => array_query(parameter, value, explode, "|"),
        "deepObject" => deep_query(parameter, value, explode),
        _ => Err(invalid(parameter, "unsupported query serialization style")),
    }
}

/// Undeclared SDK parameters retain the historical scalar-string representation.
pub(super) fn path_parameter(
    parameters: &[CachedParameter],
    name: &str,
    raw: &str,
) -> Result<String, Error> {
    parameters
        .iter()
        .find(|parameter| parameter.location == "path" && parameter.name == name)
        .map_or_else(|| Ok(encode(raw)), |parameter| path_value(parameter, raw))
}

pub(super) fn query_parameters(
    parameters: &[CachedParameter],
    values: &std::collections::HashMap<String, String>,
) -> Result<Vec<(String, String)>, Error> {
    let mut entries: Vec<_> = values.iter().collect();
    entries.sort_by_key(|(key, _)| *key);
    let mut pairs = Vec::new();
    for (key, raw) in entries {
        let parameter = parameters
            .iter()
            .find(|parameter| parameter.location == "query" && parameter.name == *key);
        let next = parameter.map_or_else(
            || Ok(vec![(key.clone(), raw.clone())]),
            |parameter| query_values(parameter, raw),
        )?;
        pairs.extend(next);
    }
    Ok(pairs)
}

/// URL parsers normalize dot-only segments even if the dots are percent encoded.
/// Fail explicitly rather than silently changing the operation's path.
pub(super) fn validate_path_segments(url: &str) -> Result<(), Error> {
    if url
        .split('/')
        .any(|segment| matches!(urlencoding::decode(segment).as_deref(), Ok("." | "..")))
    {
        return Err(Error::validation_error(
            "Dot-only path segments cannot be represented without URL normalization",
        ));
    }
    Ok(())
}

pub(super) fn expand_path_template(
    template: &str,
    values: &std::collections::HashMap<String, String>,
    parameters: &[CachedParameter],
) -> Result<String, Error> {
    let mut path = template.to_string();
    let mut start = 0;
    while let Some(open) = path[start..].find('{') {
        let open = start + open;
        let close = path[open..]
            .find('}')
            .ok_or_else(|| Error::validation_error("Unclosed path parameter placeholder"))?
            + open;
        let name = &path[open + 1..close];
        let raw = values
            .get(name)
            .ok_or_else(|| Error::missing_path_parameter(name))?;
        let encoded = path_parameter(parameters, name, raw)?;
        path.replace_range(open..=close, &encoded);
        start = open + encoded.len();
    }
    Ok(path)
}
