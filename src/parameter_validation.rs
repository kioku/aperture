//! Request-side parameter constraint validation, shared by every executor frontend.

use std::cmp::Ordering;

use serde_json::{Number, Value};

use crate::cache::models::CachedCommand;
use crate::error::Error;
use crate::invocation::OperationCall;
use crate::parameter_metadata::{ParameterConstraints, ParameterMetadata};

pub fn validate(operation: &CachedCommand, call: &OperationCall) -> Result<(), Error> {
    for parameter in &operation.parameters {
        let values = match parameter.location.as_str() {
            "path" => &call.path_params,
            "query" if call.pagination_url.is_none() => &call.query_params,
            _ => continue,
        };
        let Some(value) = values.get(&parameter.name) else {
            continue;
        };
        let metadata = ParameterMetadata::from_schema(parameter.schema.as_deref());
        validate_value(&parameter.name, value, &metadata)?;
    }
    Ok(())
}

/// Check decoded effective scalar values, including base-URL queries, serializer
/// output and every repeated key. Never collect pairs into a last-wins map.
pub fn validate_query(
    operation: &CachedCommand,
    url: &reqwest::Url,
    link_target: bool,
) -> Result<(), Error> {
    for parameter in &operation.parameters {
        if parameter.location != "query" {
            continue;
        }
        let metadata = ParameterMetadata::from_schema(parameter.schema.as_deref());
        if matches!(metadata.schema_type.as_deref(), Some("array" | "object")) {
            validate_compound_link(&parameter.name, &metadata, link_target)?;
            continue;
        }
        for (name, value) in url
            .query_pairs()
            .filter(|(name, _)| name == &parameter.name)
        {
            validate_value(&name, &value, &metadata)?;
        }
    }
    Ok(())
}

fn validate_compound_link(
    name: &str,
    metadata: &ParameterMetadata,
    link_target: bool,
) -> Result<(), Error> {
    // Original compound inputs are checked before serialization. Link targets
    // have no original value and exploded objects may not retain their name.
    // Do not guess an inverse serialization or silently validate one element as
    // the whole value. Unconstrained compounds remain untouched.
    if link_target && !metadata.enum_values.is_empty() {
        return Err(Error::validation_error(format!(
            "Parameter {name:?}: compound enum validation of a pagination URL is unsupported; use --no-validate-parameters to trust the server"
        )));
    }
    Ok(())
}

/// Validate the actual wire headers after custom, security and batch overlays.
/// `HeaderMap` supplies HTTP's case-insensitive lookup; validating the input map
/// instead would miss overrides and could reject values that are never sent.
pub fn validate_headers(
    operation: &CachedCommand,
    headers: &reqwest::header::HeaderMap,
) -> Result<(), Error> {
    for parameter in &operation.parameters {
        if parameter.location != "header" {
            continue;
        }
        let Some(value) = headers.get(&parameter.name) else {
            continue;
        };
        let raw = std::str::from_utf8(value.as_bytes())
            .map_err(|_| Error::validation_error("Declared header parameter is not valid UTF-8"))?;
        let metadata = ParameterMetadata::from_schema(parameter.schema.as_deref());
        validate_value(&parameter.name, raw, &metadata)?;
    }
    Ok(())
}

fn violation(name: &str, value: &str, constraint: impl std::fmt::Display) -> Error {
    Error::validation_error(format!(
        "Parameter {name:?} value {value:?} violates {constraint}; correct the value or use --no-validate-parameters to trust the server"
    ))
}

fn validate_value(name: &str, raw: &str, metadata: &ParameterMetadata) -> Result<(), Error> {
    let value = typed_value(raw, metadata.schema_type.as_deref());
    validate_enum(name, raw, &value, &metadata.enum_values)?;
    let constraints = &metadata.constraints;
    if constraints.minimum.is_some() || constraints.maximum.is_some() {
        let number = value
            .as_number()
            .ok_or_else(|| violation(name, raw, "numeric bounds (expected a number)"))?;
        validate_bounds(name, raw, number, constraints)?;
    }
    if value.is_string() {
        validate_string(name, raw, constraints)?;
    }
    Ok(())
}

fn typed_value(raw: &str, schema_type: Option<&str>) -> Value {
    match schema_type {
        Some("integer" | "number" | "boolean" | "array" | "object") => {
            serde_json::from_str::<Value>(raw).unwrap_or_else(|_| Value::String(raw.to_owned()))
        }
        _ => Value::String(raw.to_owned()),
    }
}

fn validate_enum(name: &str, raw: &str, value: &Value, allowed: &[Value]) -> Result<(), Error> {
    if !allowed.is_empty() && !allowed.iter().any(|allowed| equal(value, allowed)) {
        return Err(violation(
            name,
            raw,
            format_args!("enum {}", Value::Array(allowed.to_vec())),
        ));
    }
    Ok(())
}

fn equal(left: &Value, right: &Value) -> bool {
    match (left, right) {
        (Value::Number(left), Value::Number(right)) => compare(left, right) == Ordering::Equal,
        (Value::Array(left), Value::Array(right)) => {
            left.len() == right.len()
                && left
                    .iter()
                    .zip(right)
                    .all(|(left, right)| equal(left, right))
        }
        (Value::Object(left), Value::Object(right)) => equal_objects(left, right),
        _ => left == right,
    }
}

fn equal_objects(
    left: &serde_json::Map<String, Value>,
    right: &serde_json::Map<String, Value>,
) -> bool {
    left.len() == right.len()
        && left
            .iter()
            .all(|(key, left)| right.get(key).is_some_and(|right| equal(left, right)))
}

// Compare decimal representations exactly: converting an integer bound or enum
// through f64 can silently admit an adjacent integer above 2^53.
fn compare(left: &Number, right: &Number) -> Ordering {
    let (left_negative, left_digits, left_power) = decimal_parts(left);
    let (right_negative, right_digits, right_power) = decimal_parts(right);
    if left_negative != right_negative {
        return right_negative.cmp(&left_negative);
    }
    let width = left_digits.len().max(right_digits.len());
    let order = left_power.cmp(&right_power).then_with(|| {
        left_digits
            .bytes()
            .chain(std::iter::repeat(b'0'))
            .take(width)
            .cmp(
                right_digits
                    .bytes()
                    .chain(std::iter::repeat(b'0'))
                    .take(width),
            )
    });
    if left_negative {
        order.reverse()
    } else {
        order
    }
}

// The power is the decimal position immediately after the leading significant
// digit. Zero uses a sentinel power smaller than any finite JSON number.
fn decimal_parts(number: &Number) -> (bool, String, i32) {
    let text = number.to_string();
    let unsigned = text.strip_prefix('-').unwrap_or(&text);
    let (mantissa, exponent) = unsigned
        .split_once(['e', 'E'])
        .map_or((unsigned, 0), |(mantissa, exponent)| {
            (mantissa, exponent.parse::<i32>().unwrap_or_default())
        });
    let fraction = mantissa
        .split_once('.')
        .map_or(0, |(_, fraction)| fraction.len());
    let digits: String = mantissa.chars().filter(|&c| c != '.').collect();
    let digits = digits.trim_start_matches('0').to_owned();
    if digits.is_empty() {
        return (false, digits, i32::MIN);
    }
    // serde_json::Number renders a finite f64, i64 or u64, so these lengths
    // are bounded by a few hundred characters, far below i32::MAX.
    let power = exponent - i32::try_from(fraction).expect("finite number fraction length")
        + i32::try_from(digits.len()).expect("finite number digit length");
    (text.starts_with('-'), digits, power)
}

fn validate_bounds(
    name: &str,
    raw: &str,
    value: &Number,
    constraints: &ParameterConstraints,
) -> Result<(), Error> {
    for (label, bound, exclusive, invalid_order) in [
        (
            "minimum",
            constraints.minimum.as_ref(),
            constraints.exclusive_minimum,
            Ordering::Less,
        ),
        (
            "maximum",
            constraints.maximum.as_ref(),
            constraints.exclusive_maximum,
            Ordering::Greater,
        ),
    ] {
        let Some(bound) = bound else { continue };
        let order = compare(value, bound);
        if order == invalid_order || (exclusive && order == Ordering::Equal) {
            let qualifier = if exclusive { "exclusive " } else { "" };
            return Err(violation(
                name,
                raw,
                format_args!("{qualifier}{label} {bound}"),
            ));
        }
    }

    Ok(())
}

fn validate_string(name: &str, raw: &str, constraints: &ParameterConstraints) -> Result<(), Error> {
    // JSON Schema length counts Unicode characters, not bytes or UTF-16 code units.
    let length = raw.chars().count();
    if let Some(minimum) = constraints.min_length.filter(|&minimum| length < minimum) {
        return Err(violation(name, raw, format_args!("minLength {minimum}")));
    }
    if let Some(maximum) = constraints.max_length.filter(|&maximum| length > maximum) {
        return Err(violation(name, raw, format_args!("maxLength {maximum}")));
    }
    let Some(pattern) = &constraints.pattern else {
        return Ok(());
    };
    let regex = regress::Regex::with_flags(pattern, "u").map_err(|error| {
        violation(
            name,
            raw,
            format_args!("pattern {pattern:?} (invalid ECMAScript regex: {error})"),
        )
    })?;
    // Use unanchored Unicode-aware ECMAScript matching, as recommended by
    // JSON Schema. UTF-16 input preserves surrogate escape semantics.
    let text: Vec<u16> = raw.encode_utf16().collect();
    if regex.find_from_utf16(&text, 0).next().is_none() {
        return Err(violation(name, raw, format_args!("pattern {pattern:?}")));
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn check(schema: &Value, value: &str) -> Result<(), Error> {
        validate_value(
            "test",
            value,
            &ParameterMetadata::from_schema(Some(&schema.to_string())),
        )
    }

    #[test]
    fn ecmascript_patterns_not_rust_regex() {
        for (pattern, valid, invalid) in [
            (r"^(?=a)[a-z]+$", "abc", "bbb"),
            (r"^(a)\1$", "aa", "ab"),
            (r"(?<=a)b", "ab", "bb"),
            (r"^\d+$", "123", "١٢٣"),
            (r"^.$", "😀", "ab"),
            (r"^\uD83D\uDE00$", "😀", "x"),
            (r"abc", "xabcx", "xyz"),
        ] {
            let schema = json!({"type":"string", "pattern":pattern});
            assert!(check(&schema, valid).is_ok(), "{pattern}: {valid}");
            assert!(check(&schema, invalid).is_err(), "{pattern}: {invalid}");
        }
        assert!(check(&json!({"type":"string", "pattern":"["}), "x").is_err());
    }

    #[test]
    fn mixed_numeric_representations_preserve_large_integer_precision() {
        let schema = json!({"type":"integer", "maximum":9_007_199_254_740_992.0});
        assert!(check(&schema, "9007199254740992").is_ok());
        assert!(check(&schema, "9007199254740993").is_err());
        assert!(check(
            &json!({"type":"number", "enum":[9_007_199_254_740_993_u64]}),
            "9007199254740992.0"
        )
        .is_err());
    }

    #[test]
    fn scalar_types_unicode_and_numeric_boundaries() {
        for (schema, valid, invalid) in [
            (json!({"type":"string","enum":["1"]}), "1", "1.0"),
            (
                json!({"type":"array","enum":[[1,"a"]]}),
                "[1.0,\"a\"]",
                "[2,\"a\"]",
            ),
            (
                json!({"type":"object","enum":[{"a":1}]}),
                "{\"a\":1.0}",
                "{\"a\":2}",
            ),
            (json!({"type":"number","enum":[1,2.5]}), "1.0", "3"),
            (json!({"type":"boolean","enum":[false]}), "false", "true"),
            (
                json!({"type":"boolean","nullable":true,"enum":[false,null]}),
                "false",
                "garbage",
            ),
            (
                json!({"type":"integer","maximum":9_007_199_254_740_992_u64}),
                "9007199254740992",
                "9007199254740993",
            ),
            (
                json!({"type":"string","minLength":1,"maxLength":1}),
                "😀",
                "😀a",
            ),
            (
                json!({"type":"number","minimum":0,"maximum":0}),
                "-0.0",
                "1",
            ),
        ] {
            assert!(check(&schema, valid).is_ok(), "{schema}: {valid}");
            assert!(check(&schema, invalid).is_err());
        }
        assert!(check(&json!({"type":"string"}), "anything").is_ok());
        assert!(check(&json!({}), "anything").is_ok());
        assert!(check(&json!({"type":"number","minimum":1}), "bad").is_err());
    }
}
