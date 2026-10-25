//! Decode response JSON without ever collapsing duplicate decoded object names.
//!
//! Text responses retain their historical fallback. Duplicate-key failures never
//! include keys, values, or serde diagnostics, which may contain private data.
use crate::error::Error;
use serde::de::{self, MapAccess, SeqAccess, Visitor};
use serde::{Deserialize, Deserializer};
use serde_json::{Map, Value};
use std::fmt;

struct StrictValue(Value);

impl<'de> Deserialize<'de> for StrictValue {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        deserializer.deserialize_any(StrictVisitor)
    }
}

struct StrictVisitor;

impl<'de> Visitor<'de> for StrictVisitor {
    type Value = StrictValue;

    fn expecting(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
        formatter.write_str("JSON response")
    }

    fn visit_bool<E: de::Error>(self, value: bool) -> Result<Self::Value, E> {
        Ok(StrictValue(Value::Bool(value)))
    }

    fn visit_i64<E: de::Error>(self, value: i64) -> Result<Self::Value, E> {
        Ok(StrictValue(value.into()))
    }

    fn visit_u64<E: de::Error>(self, value: u64) -> Result<Self::Value, E> {
        Ok(StrictValue(value.into()))
    }

    fn visit_f64<E: de::Error>(self, value: f64) -> Result<Self::Value, E> {
        Ok(StrictValue(Value::from(value)))
    }

    fn visit_str<E: de::Error>(self, value: &str) -> Result<Self::Value, E> {
        Ok(StrictValue(Value::String(value.to_owned())))
    }

    fn visit_unit<E: de::Error>(self) -> Result<Self::Value, E> {
        Ok(StrictValue(Value::Null))
    }

    fn visit_seq<A: SeqAccess<'de>>(self, mut sequence: A) -> Result<Self::Value, A::Error> {
        let mut values = Vec::new();
        while let Some(StrictValue(value)) = sequence.next_element()? {
            values.push(value);
        }
        Ok(StrictValue(Value::Array(values)))
    }

    fn visit_map<A: MapAccess<'de>>(self, mut object: A) -> Result<Self::Value, A::Error> {
        let mut values = Map::new();
        while let Some(key) = object.next_key::<String>()? {
            // Check decoded names before reading/inserting the next value.
            if values.contains_key(&key) {
                return Err(de::Error::custom("duplicate JSON response key"));
            }
            let StrictValue(value) = object.next_value()?;
            values.insert(key, value);
        }
        Ok(StrictValue(Value::Object(values)))
    }
}

/// Parse JSON when present, preserving non-JSON text fallback.
///
/// # Errors
/// Returns a static, input-free error if an object repeats a decoded name.
pub fn parse_optional(source: &str) -> Result<Option<Value>, Error> {
    match serde_json::from_str::<StrictValue>(source) {
        Ok(StrictValue(value)) => Ok(Some(value)),
        // The visitor's only data error is a duplicate name. Do not expose the
        // parser message: even ordinary syntax diagnostics can quote input.
        Err(error) if error.is_data() => Err(Error::invalid_json_body(
            "Response contains a duplicate JSON object key (response omitted)",
        )),
        Err(_) => Ok(None),
    }
}

/// Parse a response that must be JSON.
///
/// # Errors
/// Returns input-free diagnostics for duplicate keys or invalid JSON.
pub fn parse(source: &str) -> Result<Value, Error> {
    parse_optional(source)?.ok_or_else(|| Error::invalid_json_body("Response is not valid JSON"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recursive_decoded_duplicates_are_rejected() {
        for source in [
            r#"{"a":1,"a":1}"#,
            r#"{"x":{"a":1,"a":2}}"#,
            r#"[{"a":1,"\u0061":2}]"#,
            r#"{"a":1,"a": invalid}"#,
        ] {
            assert!(parse_optional(source).is_err(), "{source}");
        }
    }

    #[test]
    fn valid_json_and_text_are_preserved() {
        for source in [
            r#"{"left":{"a":1},"right":{"a":2}}"#,
            r#"[null,true,false,-1,18446744073709551615,1.5,"text"]"#,
        ] {
            assert_eq!(
                parse(source).unwrap(),
                serde_json::from_str::<Value>(source).unwrap()
            );
        }
        assert!(parse_optional("plain text").unwrap().is_none());
        assert!(parse_optional("{}").unwrap().is_some());
        assert!(parse("{} trailing").is_err());
    }
}
