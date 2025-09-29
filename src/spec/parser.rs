use crate::constants;
use crate::error::Error;
use openapiv3::OpenAPI;
use regex::Regex;

/// Preprocesses `OpenAPI` content to fix common compatibility issues
///
/// This function handles:
/// - Converting numeric boolean values (0/1) to proper booleans (false/true)
/// - Works with both YAML and JSON formats
/// - Preserves multi-digit numbers (e.g., 10, 18, 100)
fn preprocess_for_compatibility(content: &str) -> String {
    // Properties that should be boolean in OpenAPI 3.0 but sometimes use 0/1
    // Note: exclusiveMinimum/Maximum are boolean in 3.0 but numeric in 3.1
    const BOOLEAN_PROPERTIES: &[&str] = &[
        constants::FIELD_DEPRECATED,
        constants::FIELD_REQUIRED,
        constants::FIELD_READ_ONLY,
        constants::FIELD_WRITE_ONLY,
        constants::FIELD_NULLABLE,
        constants::FIELD_UNIQUE_ITEMS,
        constants::FIELD_ALLOW_EMPTY_VALUE,
        constants::FIELD_EXPLODE,
        constants::FIELD_ALLOW_RESERVED,
        constants::FIELD_EXCLUSIVE_MINIMUM,
        constants::FIELD_EXCLUSIVE_MAXIMUM,
    ];

    // Detect format to optimize processing
    let is_json = content.trim_start().starts_with('{');
    let mut result = content.to_string();

    // Apply appropriate replacements based on format
    if is_json {
        return fix_json_boolean_values(result, BOOLEAN_PROPERTIES);
    }

    // Process as YAML
    result = fix_yaml_boolean_values(result, BOOLEAN_PROPERTIES);

    // JSON might be embedded in YAML comments or examples, so also check JSON patterns
    if result.contains('"') {
        result = fix_json_boolean_values(result, BOOLEAN_PROPERTIES);
    }

    result
}

/// Fix boolean values in YAML format
fn fix_yaml_boolean_values(mut content: String, properties: &[&str]) -> String {
    for property in properties {
        let pattern_0 = Regex::new(&format!(r"\b{property}: 0\b"))
            .expect("Regex pattern is hardcoded and valid");
        let pattern_1 = Regex::new(&format!(r"\b{property}: 1\b"))
            .expect("Regex pattern is hardcoded and valid");

        content = pattern_0
            .replace_all(&content, &format!("{property}: false"))
            .to_string();
        content = pattern_1
            .replace_all(&content, &format!("{property}: true"))
            .to_string();
    }
    content
}

/// Fix boolean values in JSON format
fn fix_json_boolean_values(mut content: String, properties: &[&str]) -> String {
    for property in properties {
        let pattern_0 = Regex::new(&format!(r#""{property}"\s*:\s*0\b"#)).unwrap();
        let pattern_1 = Regex::new(&format!(r#""{property}"\s*:\s*1\b"#)).unwrap();

        content = pattern_0
            .replace_all(&content, &format!(r#""{property}":false"#))
            .to_string();
        content = pattern_1
            .replace_all(&content, &format!(r#""{property}":true"#))
            .to_string();
    }
    content
}

/// Fixes common indentation issues in components section for malformed specs
/// This is only applied to `OpenAPI` 3.1 specs where we've seen such issues
fn fix_component_indentation(content: &str) -> String {
    let mut result = content.to_string();

    // Some 3.1 specs (like OpenProject) have component subsections at 2 spaces instead of 4
    // Only fix these specific sections when they appear at the wrong indentation level
    let component_sections = [
        constants::COMPONENT_SCHEMAS,
        constants::COMPONENT_RESPONSES,
        constants::COMPONENT_EXAMPLES,
        constants::COMPONENT_PARAMETERS,
        constants::COMPONENT_REQUEST_BODIES,
        constants::COMPONENT_HEADERS,
        constants::COMPONENT_SECURITY_SCHEMES,
        constants::COMPONENT_LINKS,
        constants::COMPONENT_CALLBACKS,
    ];

    for section in &component_sections {
        // Only replace if it's at 2-space indentation (wrong for components subsections)
        result = result.replace(&format!("\n  {section}:"), &format!("\n    {section}:"));
    }

    result
}

/// Parses `OpenAPI` content, supporting both 3.0.x (directly) and 3.1.x (via oas3 fallback).
///
/// This function first attempts to parse the content as `OpenAPI` 3.0.x using the `openapiv3` crate.
/// If that fails, it falls back to parsing as `OpenAPI` 3.1.x using the `oas3` crate, then attempts
/// to convert the result to `OpenAPI` 3.0.x format.
///
/// # Arguments
///
/// * `content` - The YAML or JSON content of an `OpenAPI` specification
///
/// # Returns
///
/// An `OpenAPI` 3.0.x structure, or an error if parsing fails
///
/// # Errors
///
/// Returns an error if:
/// - The content is not valid YAML
/// - The content is not a valid `OpenAPI` specification
/// - `OpenAPI` 3.1 features cannot be converted to 3.0 format
///
/// # Limitations
///
/// When parsing `OpenAPI` 3.1.x specifications:
/// - Some 3.1-specific features may be lost or downgraded
/// - Type arrays become single types
/// - Webhooks are not supported
/// - JSON Schema 2020-12 features may not be preserved
pub fn parse_openapi(content: &str) -> Result<OpenAPI, Error> {
    // Always preprocess for compatibility issues
    let mut preprocessed = preprocess_for_compatibility(content);

    // Check if this looks like OpenAPI 3.1.x (both YAML and JSON formats)
    if content.contains("openapi: 3.1")
        || content.contains("openapi: \"3.1")
        || content.contains("openapi: '3.1")
        || content.contains(r#""openapi":"3.1"#)
        || content.contains(r#""openapi": "3.1"#)
    {
        // For OpenAPI 3.1 specs, also fix potential indentation issues
        // (some 3.1 specs like OpenProject have malformed indentation)
        preprocessed = fix_component_indentation(&preprocessed);

        // Try oas3 first for 3.1 specs - pass original content for security scheme extraction
        match parse_with_oas3_direct_with_original(&preprocessed, content) {
            Ok(spec) => return Ok(spec),
            #[cfg(not(feature = "openapi31"))]
            Err(e) => return Err(e), // Return the "not enabled" error immediately
            #[cfg(feature = "openapi31")]
            Err(_) => {} // Fall through to try regular parsing
        }
    }

    // Try parsing as OpenAPI 3.0.x (most common case)
    // Detect format based on content structure
    let trimmed = content.trim();
    if trimmed.starts_with('{') {
        parse_json_with_fallback(&preprocessed)
    } else {
        parse_yaml_with_fallback(&preprocessed)
    }
}

/// Parse JSON content with YAML fallback
fn parse_json_with_fallback(content: &str) -> Result<OpenAPI, Error> {
    // Try JSON first since content looks like JSON
    match serde_json::from_str::<OpenAPI>(content) {
        Ok(spec) => Ok(spec),
        Err(json_err) => {
            // Try YAML as fallback
            if let Ok(spec) = serde_yaml::from_str::<OpenAPI>(content) {
                return Ok(spec);
            }

            // Return JSON error since content looked like JSON
            Err(Error::serialization_error(format!(
                "Failed to parse OpenAPI spec as JSON: {json_err}"
            )))
        }
    }
}

/// Parse YAML content with JSON fallback
fn parse_yaml_with_fallback(content: &str) -> Result<OpenAPI, Error> {
    // Try YAML first since content looks like YAML
    match serde_yaml::from_str::<OpenAPI>(content) {
        Ok(spec) => Ok(spec),
        Err(yaml_err) => {
            // Try JSON as fallback
            if let Ok(spec) = serde_json::from_str::<OpenAPI>(content) {
                return Ok(spec);
            }

            // Return YAML error since content looked like YAML
            Err(Error::Yaml(yaml_err))
        }
    }
}

/// Direct parsing with oas3 for known 3.1 specs with original content for security scheme extraction
#[cfg(feature = "openapi31")]
fn parse_with_oas3_direct_with_original(
    preprocessed: &str,
    original: &str,
) -> Result<OpenAPI, Error> {
    // First, extract security schemes from the original content before any conversions
    let security_schemes_from_yaml = extract_security_schemes_from_yaml(original);

    // Try parsing with oas3 (supports OpenAPI 3.1.x) using preprocessed content
    // First try as YAML, then as JSON if YAML fails
    let oas3_spec = match oas3::from_yaml(preprocessed) {
        Ok(spec) => spec,
        Err(_yaml_err) => {
            // Try parsing as JSON
            oas3::from_json(preprocessed).map_err(|e| {
                Error::serialization_error(format!(
                    "Failed to parse OpenAPI 3.1 spec as YAML or JSON: {e}"
                ))
            })?
        }
    };

    eprintln!(
        "{} OpenAPI 3.1 specification detected. Using compatibility mode.",
        crate::constants::MSG_WARNING_PREFIX
    );
    eprintln!("         Some 3.1-specific features may not be available.");

    // Convert oas3 spec to JSON, then attempt to parse as openapiv3
    let json = oas3::to_json(&oas3_spec).map_err(|e| {
        Error::serialization_error(format!("Failed to serialize OpenAPI 3.1 spec: {e}"))
    })?;

    // Parse the JSON as OpenAPI 3.0.x
    // This may fail if there are incompatible 3.1 features
    let mut spec = serde_json::from_str::<OpenAPI>(&json).map_err(|e| {
        Error::validation_error(format!(
            "OpenAPI 3.1 spec contains features incompatible with 3.0: {e}. \
            Consider converting the spec to OpenAPI 3.0 format."
        ))
    })?;

    // WORKAROUND: The oas3 conversion loses security schemes, so restore them
    // from the original content that we extracted earlier
    restore_security_schemes(&mut spec, security_schemes_from_yaml);

    Ok(spec)
}

/// Restore security schemes to the OpenAPI spec if they were lost during conversion
#[cfg(feature = "openapi31")]
fn restore_security_schemes(
    spec: &mut OpenAPI,
    security_schemes: Option<
        indexmap::IndexMap<String, openapiv3::ReferenceOr<openapiv3::SecurityScheme>>,
    >,
) {
    if let Some(schemes) = security_schemes {
        // Ensure components exists and add the security schemes
        match spec.components {
            Some(ref mut components) => {
                components.security_schemes = schemes;
            }
            None => {
                let mut components = openapiv3::Components::default();
                components.security_schemes = schemes;
                spec.components = Some(components);
            }
        }
    }
}

/// Extract security schemes from YAML/JSON content before any processing
///
/// This function is needed because the oas3 library's conversion from OpenAPI 3.1 to 3.0
/// sometimes loses security scheme definitions. We extract them from the original content
/// to restore them after conversion.
#[cfg(feature = "openapi31")]
fn extract_security_schemes_from_yaml(
    content: &str,
) -> Option<indexmap::IndexMap<String, openapiv3::ReferenceOr<openapiv3::SecurityScheme>>> {
    // Parse content as either YAML or JSON
    let value = parse_content_as_value(content)?;

    // Navigate to components.securitySchemes
    let security_schemes = value.get("components")?.get("securitySchemes")?;

    // Convert to the expected type
    serde_yaml::from_value(security_schemes.clone()).ok()
}

/// Parse content as either YAML or JSON into a generic Value type
#[cfg(feature = "openapi31")]
fn parse_content_as_value(content: &str) -> Option<serde_yaml::Value> {
    // Try YAML first (more common for OpenAPI specs)
    if let Ok(value) = serde_yaml::from_str::<serde_yaml::Value>(content) {
        return Some(value);
    }

    // Fallback to JSON
    serde_json::from_str::<serde_json::Value>(content)
        .ok()
        .and_then(|json| serde_yaml::to_value(json).ok())
}

/// Fallback for when `OpenAPI` 3.1 support is not compiled in
#[cfg(not(feature = "openapi31"))]
fn parse_with_oas3_direct_with_original(
    _preprocessed: &str,
    _original: &str,
) -> Result<OpenAPI, Error> {
    Err(Error::validation_error(
        "OpenAPI 3.1 support is not enabled. Rebuild with --features openapi31 to enable 3.1 support."
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_openapi_30() {
        let spec_30 = r#"
openapi: 3.0.0
info:
  title: Test API
  version: 1.0.0
paths: {}
"#;

        let result = parse_openapi(spec_30);
        assert!(result.is_ok());
        let spec = result.unwrap();
        assert_eq!(spec.openapi, "3.0.0");
    }

    #[test]
    fn test_parse_openapi_31() {
        let spec_31 = r#"
openapi: 3.1.0
info:
  title: Test API
  version: 1.0.0
paths: {}
"#;

        let result = parse_openapi(spec_31);

        #[cfg(feature = "openapi31")]
        {
            // With the feature, it should parse successfully
            assert!(result.is_ok());
            if let Ok(spec) = result {
                assert!(spec.openapi.starts_with("3."));
            }
        }

        #[cfg(not(feature = "openapi31"))]
        {
            // Without the feature, it should return an error about missing support
            assert!(result.is_err());
            if let Err(Error::Internal {
                kind: crate::error::ErrorKind::Validation,
                message,
                ..
            }) = result
            {
                assert!(message.contains("OpenAPI 3.1 support is not enabled"));
            } else {
                panic!("Expected validation error about missing 3.1 support");
            }
        }
    }

    #[test]
    fn test_parse_invalid_yaml() {
        let invalid_yaml = "not: valid: yaml: at: all:";

        let result = parse_openapi(invalid_yaml);
        assert!(result.is_err());
    }

    #[test]
    fn test_preprocess_boolean_values() {
        // Test that 0/1 are converted to false/true
        let input = r#"
deprecated: 0
required: 1
readOnly: 0
writeOnly: 1
"#;
        let result = preprocess_for_compatibility(input);
        assert!(result.contains("deprecated: false"));
        assert!(result.contains("required: true"));
        assert!(result.contains("readOnly: false"));
        assert!(result.contains("writeOnly: true"));
    }

    #[test]
    fn test_preprocess_exclusive_min_max() {
        // Test that exclusiveMinimum/Maximum 0/1 are converted but other numbers are preserved
        let input = r#"
exclusiveMinimum: 0
exclusiveMaximum: 1
exclusiveMinimum: 10
exclusiveMaximum: 18
exclusiveMinimum: 100
"#;
        let result = preprocess_for_compatibility(input);
        assert!(result.contains("exclusiveMinimum: false"));
        assert!(result.contains("exclusiveMaximum: true"));
        assert!(result.contains("exclusiveMinimum: 10"));
        assert!(result.contains("exclusiveMaximum: 18"));
        assert!(result.contains("exclusiveMinimum: 100"));
    }

    #[test]
    fn test_preprocess_json_format() {
        // Test that JSON format boolean values are converted
        let input = r#"{"deprecated":0,"required":1,"exclusiveMinimum":0,"exclusiveMaximum":1,"otherValue":10}"#;
        let result = preprocess_for_compatibility(input);
        assert!(result.contains(r#""deprecated":false"#));
        assert!(result.contains(r#""required":true"#));
        assert!(result.contains(r#""exclusiveMinimum":false"#));
        assert!(result.contains(r#""exclusiveMaximum":true"#));
        assert!(result.contains(r#""otherValue":10"#)); // Should not be changed
    }

    #[test]
    fn test_preprocess_preserves_multi_digit_numbers() {
        // Test that numbers like 10, 18, 100 are not corrupted
        let input = r#"
paths:
  /test:
    get:
      parameters:
        - name: test
          in: query
          schema:
            type: integer
            minimum: 10
            maximum: 100
            exclusiveMinimum: 18
"#;
        let result = preprocess_for_compatibility(input);
        // These should remain unchanged
        assert!(result.contains("minimum: 10"));
        assert!(result.contains("maximum: 100"));
        assert!(result.contains("exclusiveMinimum: 18"));
        // Should not contain corrupted values
        assert!(!result.contains("true0"));
        assert!(!result.contains("true8"));
        assert!(!result.contains("true00"));
        assert!(!result.contains("false0"));
    }
}
