use crate::error::Error;
use openapiv3::OpenAPI;

/// Preprocesses `OpenAPI` content to fix common compatibility issues
fn preprocess_for_compatibility(content: &str) -> String {
    let mut result = content.to_string();

    // Fix integer boolean values
    let boolean_properties = [
        "deprecated",
        "required",
        "readOnly",
        "writeOnly",
        "nullable",
        "uniqueItems",
        "allowEmptyValue",
        "explode",
        "allowReserved",
        "exclusiveMinimum",
        "exclusiveMaximum",
    ];

    for property in &boolean_properties {
        // Replace "property: 0" with "property: false"
        result = result.replace(&format!("{property}: 0"), &format!("{property}: false"));
        // Replace "property: 1" with "property: true"
        result = result.replace(&format!("{property}: 1"), &format!("{property}: true"));
    }

    // Note: We don't change exclusiveMinimum/Maximum here because in 3.1 they're meant to be numbers

    result
}

/// Fixes common indentation issues in components section for malformed specs
/// This is only applied to `OpenAPI` 3.1 specs where we've seen such issues
fn fix_component_indentation(content: &str) -> String {
    let mut result = content.to_string();

    // Some 3.1 specs (like OpenProject) have component subsections at 2 spaces instead of 4
    // Only fix these specific sections when they appear at the wrong indentation level
    let component_sections = [
        "schemas",
        "responses",
        "examples",
        "parameters",
        "requestBodies",
        "headers",
        "securitySchemes",
        "links",
        "callbacks",
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

    // Check if this looks like OpenAPI 3.1.x
    if content.contains("openapi: 3.1")
        || content.contains("openapi: \"3.1")
        || content.contains("openapi: '3.1")
    {
        // For OpenAPI 3.1 specs, also fix potential indentation issues
        // (some 3.1 specs like OpenProject have malformed indentation)
        preprocessed = fix_component_indentation(&preprocessed);

        // Try oas3 first for 3.1 specs
        match parse_with_oas3_direct(&preprocessed) {
            Ok(spec) => return Ok(spec),
            #[cfg(not(feature = "openapi31"))]
            Err(e) => return Err(e), // Return the "not enabled" error immediately
            #[cfg(feature = "openapi31")]
            Err(_) => {} // Fall through to try regular parsing
        }
    }

    // Try parsing as OpenAPI 3.0.x (most common case)
    match serde_yaml::from_str::<OpenAPI>(&preprocessed) {
        Ok(spec) => Ok(spec),
        Err(yaml_err) => {
            // Only use oas3 fallback for 3.1 specs that failed initial oas3 attempt
            // Don't use fallback for 3.0 specs - they should fail with original error
            Err(Error::Yaml(yaml_err))
        }
    }
}

/// Direct parsing with oas3 for known 3.1 specs (already preprocessed)
#[cfg(feature = "openapi31")]
fn parse_with_oas3_direct(content: &str) -> Result<OpenAPI, Error> {
    // Try parsing with oas3 (supports OpenAPI 3.1.x)
    let oas3_spec = oas3::from_yaml(content).map_err(Error::Yaml)?;

    eprintln!("Warning: OpenAPI 3.1 specification detected. Using compatibility mode.");
    eprintln!("         Some 3.1-specific features may not be available.");

    // Convert oas3 spec to JSON, then attempt to parse as openapiv3
    let json = oas3::to_json(&oas3_spec).map_err(|e| Error::SerializationError {
        reason: format!("Failed to serialize OpenAPI 3.1 spec: {e}"),
    })?;

    // Parse the JSON as OpenAPI 3.0.x
    // This may fail if there are incompatible 3.1 features
    serde_json::from_str::<OpenAPI>(&json).map_err(|e| {
        Error::Validation(format!(
            "OpenAPI 3.1 spec contains features incompatible with 3.0: {e}. \
            Consider converting the spec to OpenAPI 3.0 format."
        ))
    })
}

/// Fallback for when `OpenAPI` 3.1 support is not compiled in
#[cfg(not(feature = "openapi31"))]
fn parse_with_oas3_direct(_content: &str) -> Result<OpenAPI, Error> {
    Err(Error::Validation(
        "OpenAPI 3.1 support is not enabled. Rebuild with --features openapi31 to enable 3.1 support.".to_string()
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
            if let Err(Error::Validation(msg)) = result {
                assert!(msg.contains("OpenAPI 3.1 support is not enabled"));
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
}
