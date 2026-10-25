mod common;

use aperture_cli::spec::transformer::SpecTransformer;
use common::aperture_cmd;
use serde_json::{json, Value};

fn object_schema() -> Value {
    json!({
        "type": "object", "additionalProperties": false, "required": ["count"],
        "properties": {"count": {"type": "integer", "minimum": 1}}
    })
}

fn spec_with_schema(schema: &Value) -> Value {
    json!({
        "openapi": "3.0.3", "info": {"title": "Request schema regression", "version": "1"},
        "servers": [{"url": "https://example.invalid"}],
        "paths": {"/probe": {"post": {
            "operationId": "createWithRef", "tags": ["probe"],
            "requestBody": {"required": true, "content": {"application/json": {"schema": schema}}},
            "responses": {"200": {"description": "ok", "content": {"application/json": {
                "schema": {"$ref": "#/components/schemas/ResponseObject"}
            }}}}
        }}},
        "components": {"schemas": {
            "RequestObject": object_schema(),
            "ResponseObject": {"type": "object", "required": ["status"],
                "properties": {"status": {"type": "string", "enum": ["ok"]}}}
        }}
    })
}

fn transformed_schema(document: Value) -> Value {
    let spec = serde_json::from_value(document).unwrap();
    let cached = SpecTransformer::new().transform("probe", &spec).unwrap();
    serde_json::from_str(&cached.commands[0].request_body.as_ref().unwrap().schema).unwrap()
}

#[test]
fn referenced_request_schema_matches_inline_with_constraints() {
    let inline = transformed_schema(spec_with_schema(&object_schema()));
    let referenced = transformed_schema(spec_with_schema(
        &json!({"$ref": "#/components/schemas/RequestObject"}),
    ));
    assert_eq!(inline, object_schema());
    assert_eq!(referenced, inline);
}

#[test]
fn request_schema_resolves_chains_without_expanding_nested_references() {
    let mut document = spec_with_schema(&json!({"$ref": "#/components/schemas/Alias"}));
    document["components"]["schemas"]["Alias"] =
        json!({"$ref": "#/components/schemas/RequestObject"});
    document["components"]["schemas"]["RequestObject"]["properties"]["child"] =
        json!({"$ref": "#/components/schemas/RequestObject"});
    let expected = document["components"]["schemas"]["RequestObject"].clone();
    assert_eq!(transformed_schema(document), expected);
}

#[test]
fn missing_and_cyclic_request_schema_references_are_bounded() {
    for reference in [
        "#/components/schemas/Missing",
        "#/components/schemas/Cycle",
        "https://example.invalid/schema.json",
    ] {
        let mut document = spec_with_schema(&json!({"$ref": reference}));
        document["components"]["schemas"]["Cycle"] =
            json!({"$ref": "#/components/schemas/CyclePeer"});
        document["components"]["schemas"]["CyclePeer"] =
            json!({"$ref": "#/components/schemas/Cycle"});
        assert_eq!(transformed_schema(document), json!({}));
    }
}

#[test]
fn referenced_request_schemas_preserve_non_object_shapes() {
    for schema in [
        json!({"type": "string", "minLength": 2, "enum": ["ok"]}),
        json!({"type": "array", "minItems": 1, "items": {"type": "integer", "minimum": 1}}),
        json!({"oneOf": [{"type": "string"}, {"type": "integer"}]}),
    ] {
        let mut document = spec_with_schema(&json!({"$ref": "#/components/schemas/RequestObject"}));
        document["components"]["schemas"]["RequestObject"] = schema.clone();
        assert_eq!(transformed_schema(document), schema);
    }
}

#[test]
fn excessively_deep_request_schema_references_are_bounded() {
    let mut document = spec_with_schema(&json!({"$ref": "#/components/schemas/Depth0"}));
    for depth in 0..100 {
        document["components"]["schemas"][format!("Depth{depth}")] =
            json!({"$ref": format!("#/components/schemas/Depth{}", depth + 1)});
    }
    document["components"]["schemas"]["Depth100"] = object_schema();
    assert_eq!(transformed_schema(document), json!({}));
}

#[test]
fn absent_request_schema_retains_empty_fallback() {
    let mut document = spec_with_schema(&Value::Null);
    document["paths"]["/probe"]["post"]["requestBody"]["content"]["application/json"] = json!({});
    assert_eq!(transformed_schema(document), json!({}));
}

#[test]
fn docs_json_preserves_referenced_and_inline_request_and_response_schemas() {
    let temp = tempfile::TempDir::new().unwrap();
    let path = temp.path().join("spec.json");
    let mut document = spec_with_schema(&json!({"$ref": "#/components/schemas/RequestObject"}));
    let mut inline = document["paths"]["/probe"].clone();
    inline["post"]["operationId"] = json!("createInline");
    inline["post"]["requestBody"]["content"]["application/json"]["schema"] = object_schema();
    document["paths"]["/inline"] = inline;
    std::fs::write(&path, serde_json::to_vec(&document).unwrap()).unwrap();
    aperture_cmd()
        .env("APERTURE_CONFIG_DIR", temp.path())
        .args(["config", "api", "add", "schema-probe"])
        .arg(&path)
        .assert()
        .success();

    for operation in ["create-with-ref", "create-inline"] {
        let output = aperture_cmd()
            .env("APERTURE_CONFIG_DIR", temp.path())
            .args([
                "docs",
                "schema-probe",
                "probe",
                operation,
                "--format",
                "json",
            ])
            .assert()
            .success()
            .get_output()
            .stdout
            .clone();
        let docs: Value = serde_json::from_slice(&output).unwrap();
        let body = &docs["operation"]["request_body"];
        assert_eq!(body["required"], true);
        assert_eq!(body["content_type"], "application/json");
        let schema: Value = serde_json::from_str(body["schema"].as_str().unwrap()).unwrap();
        assert_eq!(schema, object_schema());
        let response: Value = serde_json::from_str(
            docs["operation"]["responses"][0]["schema"]
                .as_str()
                .unwrap(),
        )
        .unwrap();
        assert_eq!(
            response,
            document["components"]["schemas"]["ResponseObject"]
        );
    }
}
