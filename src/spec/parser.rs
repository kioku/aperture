use crate::error::Error;
use openapiv3::OpenAPI;
/// Parse once into a structural value and normalize only recognized boolean fields.
/// Payloads under examples, defaults and extensions are never traversed.
fn preprocess_for_compatibility(content: &str) -> Result<serde_json::Value, Error> {
    let mut value: serde_json::Value = if content.trim_start().starts_with('{') {
        serde_json::from_str(content)
            .or_else(|json_error| {
                // YAML flow mappings also begin with `{`; retain the JSON-first
                // fallback without rewriting their source or payloads.
                parse_yaml_value(content).map_err(|_| json_error)
            })
            .map_err(|error| Error::serialization_error(error.to_string()))?
    } else {
        parse_yaml_value(content)?
    };
    super::normalization::normalize_document(&mut value);
    Ok(value)
}

/// Use YAML's own mapping decoder so duplicate-key rejection does not depend on
/// whether optional dependencies enable `serde_json`'s preserve-order feature.
fn parse_yaml_value(content: &str) -> Result<serde_json::Value, Error> {
    let value: serde_yaml::Value = serde_yaml::from_str(content)?;
    validate_yaml_numbers(&value)?;
    serde_json::to_value(value).map_err(|error| Error::serialization_error(error.to_string()))
}

/// Reject numbers JSON cannot represent before serialization can turn them into
/// null. Mapping serialization retains YAML's numeric response-code keys.
fn validate_yaml_numbers(value: &serde_yaml::Value) -> Result<(), serde_yaml::Error> {
    match value {
        serde_yaml::Value::Number(number) => validate_yaml_number(number),
        serde_yaml::Value::Sequence(values) => validate_yaml_sequence(values),
        serde_yaml::Value::Mapping(values) => validate_yaml_mapping(values),
        serde_yaml::Value::Tagged(value) => validate_yaml_numbers(&value.value),
        _ => Ok(()),
    }
}

fn validate_yaml_number(number: &serde_yaml::Number) -> Result<(), serde_yaml::Error> {
    if number.as_f64().is_some_and(|number| !number.is_finite()) {
        return Err(serde::de::Error::custom(
            "Non-finite YAML number is not a valid JSON value",
        ));
    }
    Ok(())
}

fn validate_yaml_sequence(values: &[serde_yaml::Value]) -> Result<(), serde_yaml::Error> {
    for value in values {
        validate_yaml_numbers(value)?;
    }
    Ok(())
}

fn validate_yaml_mapping(values: &serde_yaml::Mapping) -> Result<(), serde_yaml::Error> {
    for (key, value) in values {
        validate_yaml_numbers(key)?;
        validate_yaml_numbers(value)?;
    }
    Ok(())
}

/// Parses `OpenAPI` content, supporting both 3.0.x (directly) and 3.1.x (via oas3 fallback).
///
/// Content is parsed structurally, then recognized numeric boolean fields are normalized.
/// `OpenAPI` 3.0 documents deserialize directly; 3.1 documents use the optional `oas3`
/// compatibility conversion before deserializing into the 3.0 model.
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
    // Always preprocess for compatibility issues.
    let preprocessed = preprocess_for_compatibility(content)?;

    let is_openapi_31 = preprocessed
        .get("openapi")
        .and_then(serde_json::Value::as_str)
        .is_some_and(|version| version.starts_with("3.1"));
    #[cfg(feature = "openapi31")]
    {
        let parsed_openapi_31 = if is_openapi_31 {
            parse_with_oas3_direct_with_original(&preprocessed.to_string(), content).ok()
        } else {
            None
        };

        if let Some(spec) = parsed_openapi_31 {
            return Ok(spec);
        }
    }

    #[cfg(not(feature = "openapi31"))]
    if is_openapi_31 {
        return parse_with_oas3_direct_with_original(&preprocessed.to_string(), content);
    }

    serde_json::from_value(preprocessed).map_err(|error| openapi_decode_error(&error, content))
}

/// Preserve the public YAML error category without another pass on successful parses.
fn openapi_decode_error(error: &serde_json::Error, original: &str) -> Error {
    if original.trim_start().starts_with('{') {
        return Error::serialization_error(format!("Failed to parse OpenAPI spec: {error}"));
    }
    if let Err(yaml_error) = serde_yaml::from_str::<OpenAPI>(original) {
        return Error::Yaml(yaml_error);
    }
    Error::serialization_error(format!("Failed to parse OpenAPI spec: {error}"))
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

    // ast-grep-ignore: no-println
    eprintln!(
        "{} OpenAPI 3.1 specification detected. Using compatibility mode.",
        crate::constants::MSG_WARNING_PREFIX
    );
    // ast-grep-ignore: no-println
    eprintln!("         Some 3.1-specific features may not be available.");

    // Convert oas3 spec to JSON, then attempt to parse as openapiv3
    let json = oas3::to_json(&oas3_spec).map_err(|e| {
        Error::serialization_error(format!("Failed to serialize OpenAPI 3.1 spec: {e}"))
    })?;

    // Parse the JSON as OpenAPI 3.0.x
    // This may fail if there are incompatible 3.1 features
    let mut spec = decode_converted_spec(&json, preprocessed)?;

    // WORKAROUND: The oas3 conversion loses security schemes, so restore them
    // from the original content that we extracted earlier
    restore_security_schemes(&mut spec, security_schemes_from_yaml);

    Ok(spec)
}

/// Decode the restored conversion with the existing compatibility error category.
#[cfg(feature = "openapi31")]
fn decode_converted_spec(json: &str, source: &str) -> Result<OpenAPI, Error> {
    let converted = converted_with_operation_security(json, source)?;
    serde_json::from_value(converted).map_err(|e| {
        Error::validation_error(format!(
            "OpenAPI 3.1 spec contains features incompatible with 3.0: {e}. \
            Consider converting the spec to OpenAPI 3.0 format."
        ))
    })
}

/// Restore source-level presence before typed deserialization validates values.
#[cfg(feature = "openapi31")]
fn converted_with_operation_security(json: &str, source: &str) -> Result<serde_json::Value, Error> {
    let mut converted = serde_json::from_str(json)?;
    let source = serde_json::from_str(source)?;
    restore_operation_security(&mut converted, &source);
    Ok(converted)
}

/// Preserve explicit operation overrides, including empty arrays that `oas3`
/// omits when serializing. Only HTTP operation keys are visited; path metadata
/// and extension payloads must not become operations. Missing security inherits.
#[cfg(feature = "openapi31")]
fn restore_operation_security(converted: &mut serde_json::Value, source: &serde_json::Value) {
    let Some(paths) = source.get("paths").and_then(serde_json::Value::as_object) else {
        return;
    };
    for (path, item) in paths {
        for method in [
            "get", "put", "post", "delete", "options", "head", "patch", "trace",
        ] {
            let security = item
                .get(method)
                .and_then(|operation| operation.get("security"));
            let target = converted
                .get_mut("paths")
                .and_then(|paths| paths.get_mut(path))
                .and_then(|item| item.get_mut(method))
                .and_then(serde_json::Value::as_object_mut);
            if let (Some(security), Some(target)) = (security, target) {
                target.insert("security".to_owned(), security.clone());
            }
        }
    }
}

/// Restore security schemes to the `OpenAPI` spec if they were lost during conversion
#[cfg(feature = "openapi31")]
fn restore_security_schemes(
    spec: &mut OpenAPI,
    security_schemes: Option<
        indexmap::IndexMap<String, openapiv3::ReferenceOr<openapiv3::SecurityScheme>>,
    >,
) {
    if let Some(schemes) = security_schemes {
        spec.components = Some(match spec.components.take() {
            Some(mut components) => {
                components.security_schemes = schemes;
                components
            }
            None => openapiv3::Components {
                security_schemes: schemes,
                ..Default::default()
            },
        });
    }
}

/// Extract security schemes from YAML/JSON content before any processing
///
/// This function is needed because the `oas3` library's conversion from `OpenAPI` 3.1 to 3.0
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
    use std::fmt::Write;

    #[cfg(feature = "openapi31")]
    #[test]
    fn operation_security_presence_and_values_survive_conversion() {
        for (version, yaml) in [
            ("3.0.3", false),
            ("3.0.3", true),
            ("3.1.0", false),
            ("3.1.0", true),
        ] {
            let mut item =
                serde_json::json!({"summary": "metadata", "x-security": {"security": []}});
            let requirements = [
                serde_json::json!([]),
                serde_json::json!([{}]),
                serde_json::json!([{"oauth": ["write"]}]),
                serde_json::json!([{"oauth": ["read"], "key": []}, {}]),
            ];
            for (index, method) in [
                "get", "put", "post", "delete", "options", "head", "patch", "trace",
            ]
            .iter()
            .enumerate()
            {
                item[method] = serde_json::json!({"security": requirements[index % 4],
                        "responses": {"200": {"description": "ok"}}});
            }
            let doc = serde_json::json!({"openapi": version, "info": {"title": "Overrides", "version": "1"},
                    "security": [{"oauth": ["read"]}], "paths": {"/overrides": item,
                    "/inherited": {"get": {"responses": {"200": {"description": "ok"}}}}}});
            let source = if yaml {
                serde_yaml::to_string(&doc).unwrap()
            } else {
                doc.to_string()
            };
            let parsed = parse_openapi(&source).unwrap();
            let output = serde_json::to_value(parsed).unwrap();
            assert_eq!(output["security"], doc["security"]);
            for method in [
                "get", "put", "post", "delete", "options", "head", "patch", "trace",
            ] {
                // Typed serialization may omit empty arrays; inspect the model instead below.
                let expected: Option<Vec<openapiv3::SecurityRequirement>> =
                    serde_json::from_value(doc["paths"]["/overrides"][method]["security"].clone())
                        .unwrap();
                let spec = parse_openapi(&source).unwrap();
                let path = spec.paths.paths["/overrides"].as_item().unwrap();
                let operation = path.iter().find(|(name, _)| *name == method).unwrap().1;
                assert_eq!(operation.security, expected);
            }
            assert!(output["paths"]["/inherited"]["get"]
                .get("security")
                .is_none());
        }
    }

    #[test]
    fn test_parse_openapi_30() {
        let spec_30 = r"
openapi: 3.0.0
info:
  title: Test API
  version: 1.0.0
paths: {}
";

        let result = parse_openapi(spec_30);
        assert!(result.is_ok());
        let spec = result.unwrap();
        assert_eq!(spec.openapi, "3.0.0");
    }

    #[test]
    fn test_parse_openapi_31() {
        let spec_31 = r"
openapi: 3.1.0
info:
  title: Test API
  version: 1.0.0
paths: {}
";

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
    fn test_parse_openapi_31_json_format() {
        let spec_31_json = r#"{"openapi": "3.1.0", "info": {"title": "Test API", "version": "1.0.0"}, "paths": {}}"#;

        let result = parse_openapi(spec_31_json);

        #[cfg(feature = "openapi31")]
        {
            assert!(result.is_ok());
            if let Ok(spec) = result {
                assert!(spec.openapi.starts_with("3."));
            }
        }

        #[cfg(not(feature = "openapi31"))]
        {
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
    fn structural_normalization_preserves_payloads_json_and_yaml() {
        let payload = serde_json::json!({"marker":"audit-example", "deprecated":0, "required":1,
            "nested":{"nullable":1}, "text":"required: 1"});
        let document = serde_json::json!({
            "openapi":"3.0.0", "info":{"title":"Test", "version":"1"},
            "paths": {"/test":{"get":{"deprecated":1,"parameters":[{
                "name":"q","in":"query","required":0,"schema":{"type":"integer", "nullable":1,
                    "default":payload, "example":payload,"x-arbitrary":payload}
            }], "responses":{"200":{"description":"ok", "content":{"application/json":{
                "example":payload,"examples":{"sample":{"value":payload}},
                "schema":{"type":"object","properties":{"deprecated":{"type":"integer","readOnly":1}}}
            }}}}}}}, "x-payload":payload
        });
        for input in [
            serde_json::to_string(&document).unwrap(),
            serde_yaml::to_string(&document).unwrap(),
        ] {
            let normalized: serde_json::Value = preprocess_for_compatibility(&input).unwrap();
            assert_eq!(normalized["x-payload"], payload);
            let operation = &normalized["paths"]["/test"]["get"];
            assert_eq!(operation["deprecated"], true);
            assert_eq!(operation["parameters"][0]["required"], false);
            let schema = &operation["parameters"][0]["schema"];
            for key in ["example", "default", "x-arbitrary"] {
                assert_eq!(schema[key], payload);
            }
            assert_eq!(schema["nullable"], true);
            let media = &operation["responses"]["200"]["content"]["application/json"];
            assert_eq!(media["example"], payload);
            assert_eq!(media["examples"]["sample"]["value"], payload);
            let parsed = parse_openapi(&input).unwrap();
            let serialized = serde_json::to_value(parsed).unwrap();
            assert_eq!(
                serialized["paths"]["/test"]["get"]["responses"]["200"]["content"]
                    ["application/json"]["example"],
                payload
            );
        }
    }

    #[test]
    fn flow_yaml_mapping_keeps_yaml_fallback() {
        let input = "{openapi: 3.0.3, info: {title: Flow, version: '1'}, paths: {}}";
        assert_eq!(parse_openapi(input).unwrap().info.title, "Flow");
    }

    #[test]
    fn adversarial_structural_inputs() {
        for input in ["", "  ", "{", "openapi: [", "null", "[]",
            "openapi: 3.0.3\ninfo: {title: Sweep, version: '1'}\npaths: {}\nx-payload: {value: .nan}",
            "openapi: 3.0.3\ninfo: {title: Sweep, version: '1'}\npaths: {}\nx-payload: {value: .inf}"] {
            assert!(parse_openapi(input).is_err(), "accepted {input:?}");
        }
        let base = serde_json::json!({"openapi":"3.0.3", "info":{"title":"Sweep", "version":"1"},
            "paths":{"/test":{"get":{"responses":{},"deprecated":2}}}});
        for input in [base.to_string(), serde_yaml::to_string(&base).unwrap()] {
            assert!(parse_openapi(&input).is_err());
        }
        let duplicate =
            "openapi: 3.0.3\nopenapi: 3.1.0\ninfo: {title: Sweep, version: '1'}\npaths: {}";
        assert!(parse_openapi(duplicate).is_err());
        let json_duplicate = r#"{"openapi":"3.0.1","openapi":"3.0.3","info":{"title":"Sweep","version":"1"},"paths":{},"unknown":{"deprecated":2}}"#;
        assert_eq!(parse_openapi(json_duplicate).unwrap().openapi, "3.0.3");
        let numeric_response_key = "openapi: 3.0.3\ninfo: {title: Sweep, version: '1'}\npaths: {/test: {get: {responses: {200: {description: ok}}}}}";
        assert!(parse_openapi(numeric_response_key).is_ok());
    }

    #[test]
    fn schema_traversal_preserves_data_and_boolean_schemas() {
        let payload = serde_json::json!({"required":1,"nested":{"deprecated":0,"nullable":1}});
        let leaf = serde_json::json!({"deprecated":1,"readOnly":0,"const":payload,
            "enum":[payload],"default":payload,"examples":[payload],"x-deep":payload});
        let schema = serde_json::json!({"properties":{"x-field":leaf},"allOf":[leaf],
            "anyOf":[leaf],"oneOf":[leaf],"additionalProperties":leaf,"not":leaf,
            "$defs":{"Leaf":leaf},"patternProperties":{".*":leaf},"dependentSchemas":{"field":leaf},
            "if":leaf,"then":leaf,"else":leaf,"contains":leaf,"propertyNames":leaf,
            "unevaluatedProperties":false,"unevaluatedItems":true,"prefixItems":[leaf,false],
            "items":true,"required":["field"],"$ref":"#/components/schemas/Other"});
        let input = serde_json::json!({"openapi":"3.1.0","components":{"schemas":{"Root":schema}}});
        for input in [input.to_string(), serde_yaml::to_string(&input).unwrap()] {
            let normalized = preprocess_for_compatibility(&input).unwrap();
            let root = &normalized["components"]["schemas"]["Root"];
            for child in [
                &root["properties"]["x-field"],
                &root["allOf"][0],
                &root["anyOf"][0],
                &root["oneOf"][0],
                &root["additionalProperties"],
                &root["not"],
                &root["$defs"]["Leaf"],
                &root["patternProperties"][".*"],
                &root["dependentSchemas"]["field"],
                &root["if"],
                &root["then"],
                &root["else"],
                &root["contains"],
                &root["propertyNames"],
                &root["prefixItems"][0],
            ] {
                assert_eq!(child["deprecated"], true);
                assert_eq!(child["readOnly"], false);
                for key in ["const", "default", "x-deep"] {
                    assert_eq!(child[key], payload);
                }
                assert_eq!(child["enum"][0], payload);
                assert_eq!(child["examples"][0], payload);
            }
            assert_eq!(root["items"], true);
            assert_eq!(root["unevaluatedProperties"], false);
            assert_eq!(root["prefixItems"][1], false);
            assert_eq!(root["required"], serde_json::json!(["field"]));
        }
    }

    #[test]
    fn x_prefixed_callback_names_are_not_extensions() {
        let input = serde_json::json!({"openapi":"3.0.3", "components":{"callbacks":{
            "x-callback":{"{$request.body#/url}":{"post":{"deprecated":1,"responses":{}}},
                "x-payload":{"deprecated":1}}
        }}});
        for input in [input.to_string(), serde_yaml::to_string(&input).unwrap()] {
            let value = preprocess_for_compatibility(&input).unwrap();
            let callback = &value["components"]["callbacks"]["x-callback"];
            assert_eq!(callback["{$request.body#/url}"]["post"]["deprecated"], true);
            assert_eq!(callback["x-payload"]["deprecated"], 1);
        }
    }

    #[test]
    fn exclusive_bounds_are_version_specific() {
        for (version, expected) in [
            ("3.0.3", serde_json::json!(true)),
            ("3.1.0", serde_json::json!(1)),
        ] {
            let input = serde_json::json!({"openapi":version,"components":{"schemas":{"Bound":{
                "exclusiveMinimum":1,"exclusiveMaximum":18,"required":["name"]
            }}}});
            let value: serde_json::Value =
                preprocess_for_compatibility(&input.to_string()).unwrap();
            let schema = &value["components"]["schemas"]["Bound"];
            assert_eq!(schema["exclusiveMinimum"], expected);
            assert_eq!(schema["exclusiveMaximum"], 18);
            assert_eq!(schema["required"], serde_json::json!(["name"]));
        }
    }
    /// Reproducible local overhead comparison against typed deserialization only.
    /// Set `APERTURE_PARSE_MEASUREMENT` to the report path and run with `--ignored`.
    #[test]
    #[ignore = "local parsing overhead measurement"]
    fn measure_structural_parsing_overhead() {
        let mut paths = serde_json::Map::new();
        for index in 0..500 {
            paths.insert(
                format!("/items/{index}"),
                serde_json::json!({"get":{
                    "responses":{"200":{"description":"ok","content":{"application/json":{
                        "schema":{"type":"object","properties":{"id":{"type":"integer"}}},
                        "example":{"deprecated":0,"required":1,"nested":{"nullable":1}}
                    }}}}
                }}),
            );
        }
        let document = serde_json::json!({"openapi":"3.0.3","info":{"title":"Benchmark","version":"1"},"paths":paths});
        let json = document.to_string();
        let yaml = serde_yaml::to_string(&document).unwrap();
        let mut report = String::new();
        for (format, input) in [("JSON", json), ("YAML", yaml)] {
            let start = std::time::Instant::now();
            for _ in 0..20 {
                let spec: OpenAPI = if format == "JSON" {
                    serde_json::from_str(&input).unwrap()
                } else {
                    serde_yaml::from_str(&input).unwrap()
                };
                std::hint::black_box(spec);
            }
            let direct = start.elapsed();
            let start = std::time::Instant::now();
            for _ in 0..20 {
                std::hint::black_box(parse_openapi(&input).unwrap());
            }
            let normalized = start.elapsed();
            writeln!(report, "{format}: {} bytes, 500 operations, 20 parses; direct={direct:?}, structural={normalized:?}", input.len()).unwrap();
        }
        std::fs::write(
            std::env::var("APERTURE_PARSE_MEASUREMENT").expect("report path required"),
            report,
        )
        .unwrap();
    }
}
