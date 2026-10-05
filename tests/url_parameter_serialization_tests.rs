mod test_helpers;

use aperture_cli::cache::models::{CachedParameter, CachedSpec, ParameterSerialization};
use aperture_cli::engine::executor::execute;
use aperture_cli::invocation::{ExecutionContext, OperationCall};
use aperture_cli::spec::SpecTransformer;
use serde_json::{json, Value};
use std::collections::HashMap;
use wiremock::matchers::method;
use wiremock::{Mock, MockServer, ResponseTemplate};

fn spec(base: &str, parameter: &Value) -> CachedSpec {
    let api = serde_json::from_value(json!({
        "openapi": "3.0.3", "info": {"title": "serialization", "version": "1"},
        "servers": [{"url": base}],
        "paths": {"/items/{id}": {"get": {
            "operationId": "getItems", "parameters": [parameter],
            "responses": {"200": {"description": "ok"}}
        }}}
    }))
    .unwrap();
    SpecTransformer::new()
        .transform("serialization", &api)
        .unwrap()
}

fn parameter(location: &str, style: &str, explode: bool, kind: &str) -> Value {
    json!({"name": "id", "in": location, "required": true, "style": style,
        "explode": explode, "schema": {"type": kind}})
}

fn call(location: &str, raw: &str) -> OperationCall {
    let mut call = OperationCall {
        pagination_url: None,
        operation_id: "getItems".into(),
        path_params: HashMap::from([("id".into(), "fixed".into())]),
        query_params: HashMap::new(),
        header_params: HashMap::new(),
        body: None,
        custom_headers: vec![],
    };
    let values = if location == "path" {
        &mut call.path_params
    } else {
        &mut call.query_params
    };
    values.insert("id".into(), raw.into());
    call
}

async fn observed_url(parameter: &Value, location: &str, raw: &str) -> reqwest::Url {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .respond_with(ResponseTemplate::new(200).set_body_string("{}"))
        .mount(&server)
        .await;
    let cached = spec(&server.uri(), parameter);
    execute(&cached, call(location, raw), ExecutionContext::default())
        .await
        .unwrap();
    server.received_requests().await.unwrap()[0].url.clone()
}

#[tokio::test]
async fn declared_path_styles_cover_scalars_arrays_and_objects() {
    let cases = [
        ("simple", false, "string", "blue", "blue"),
        ("label", false, "string", "blue", ".blue"),
        ("matrix", false, "string", "blue", ";id=blue"),
        (
            "simple",
            false,
            "array",
            r#"["blue","black"]"#,
            "blue,black",
        ),
        ("simple", true, "array", r#"["blue","black"]"#, "blue,black"),
        (
            "label",
            false,
            "array",
            r#"["blue","black"]"#,
            ".blue,black",
        ),
        ("label", true, "array", r#"["blue","black"]"#, ".blue.black"),
        (
            "matrix",
            false,
            "array",
            r#"["blue","black"]"#,
            ";id=blue,black",
        ),
        (
            "matrix",
            true,
            "array",
            r#"["blue","black"]"#,
            ";id=blue;id=black",
        ),
        (
            "simple",
            false,
            "object",
            r#"{"a":"blue","b":"black"}"#,
            "a,blue,b,black",
        ),
        (
            "simple",
            true,
            "object",
            r#"{"a":"blue","b":"black"}"#,
            "a=blue,b=black",
        ),
        (
            "label",
            false,
            "object",
            r#"{"a":"blue","b":"black"}"#,
            ".a,blue,b,black",
        ),
        (
            "label",
            true,
            "object",
            r#"{"a":"blue","b":"black"}"#,
            ".a=blue.b=black",
        ),
        (
            "matrix",
            false,
            "object",
            r#"{"a":"blue","b":"black"}"#,
            ";id=a,blue,b,black",
        ),
        (
            "matrix",
            true,
            "object",
            r#"{"a":"blue","b":"black"}"#,
            ";a=blue;b=black",
        ),
        ("matrix", false, "integer", "42", ";id=42"),
        ("label", true, "boolean", "false", ".false"),
        ("simple", false, "number", "1.5", "1.5"),
        ("matrix", true, "string", "", ";id"),
        ("matrix", true, "array", "[]", ""),
        ("label", true, "object", "{}", ""),
    ];
    for (style, explode, kind, raw, expected) in cases {
        let url = observed_url(&parameter("path", style, explode, kind), "path", raw).await;
        assert_eq!(
            url.path(),
            format!("/items/{expected}"),
            "{style} {explode} {kind}"
        );
        assert!(url.query().is_none());
        assert!(url.fragment().is_none());
    }
}

#[tokio::test]
async fn style_punctuation_never_turns_path_data_into_structure() {
    let raw = r#"{"a,;=/?#% 雪":"v,;=/?#% 雪"}"#;
    for (style, separator) in [("simple", ""), ("label", "."), ("matrix", ";")] {
        let url = observed_url(&parameter("path", style, true, "object"), "path", raw).await;
        assert_eq!(url.path(), format!("/items/{separator}a%2C%3B%3D%2F%3F%23%25%20%E9%9B%AA=v%2C%3B%3D%2F%3F%23%25%20%E9%9B%AA"));
        assert!(url.query().is_none());
        assert!(url.fragment().is_none());
    }
    let url = observed_url(
        &parameter("path", "label", true, "array"),
        "path",
        r#"["a.b","c/d"]"#,
    )
    .await;
    assert_eq!(url.path(), "/items/.a%2Eb.c%2Fd");
}

#[tokio::test]
async fn query_styles_preserve_pairs_keys_values_and_defaults() {
    let cases = [
        (
            "form",
            true,
            "array",
            r#"["a&=?#% 雪", "b,c"]"#,
            vec![("id", "a&=?#% 雪"), ("id", "b,c")],
        ),
        ("form", false, "array", r#"["a", "b"]"#, vec![("id", "a,b")]),
        (
            "form",
            true,
            "object",
            r#"{"a&=?#% 雪":"value+& 雪"}"#,
            vec![("a&=?#% 雪", "value+& 雪")],
        ),
        (
            "form",
            false,
            "object",
            r#"{"a":"blue","b":"black"}"#,
            vec![("id", "a,blue,b,black")],
        ),
        (
            "spaceDelimited",
            false,
            "array",
            r#"["a", "b"]"#,
            vec![("id", "a b")],
        ),
        (
            "pipeDelimited",
            false,
            "array",
            r#"["a", "b"]"#,
            vec![("id", "a|b")],
        ),
        (
            "deepObject",
            true,
            "object",
            r#"{"a":"blue","b":"black"}"#,
            vec![("id[a]", "blue"), ("id[b]", "black")],
        ),
    ];
    for (style, explode, kind, raw, expected) in cases {
        let url = observed_url(&parameter("query", style, explode, kind), "query", raw).await;
        let pairs: Vec<_> = url
            .query_pairs()
            .map(|(key, value)| (key.into_owned(), value.into_owned()))
            .collect();
        let expected: Vec<_> = expected
            .into_iter()
            .map(|(key, value)| (key.to_string(), value.to_string()))
            .collect();
        assert_eq!(pairs, expected, "{style} {explode} {kind}");
        assert!(url.fragment().is_none());
    }
    let mut default_form = parameter("query", "form", false, "array");
    default_form.as_object_mut().unwrap().remove("explode");
    let url = observed_url(&default_form, "query", r#"["a", "b"]"#).await;
    assert_eq!(url.query_pairs().count(), 2);
}

#[tokio::test]
async fn invalid_and_unsupported_inputs_fail_before_network() {
    let cases = [
        ("path", "simple", false, "array", "a,b"),
        ("path", "simple", false, "array", "{}"),
        ("path", "simple", false, "array", "[null]"),
        ("path", "matrix", true, "object", r#"{"a":{"nested":1}}"#),
        ("path", "label", false, "integer", "1.5"),
        ("path", "simple", false, "boolean", "yes"),
        ("path", "simple", false, "string", ".."),
        ("path", "label", false, "string", "."),
        ("query", "spaceDelimited", true, "array", "[1,2]"),
        ("query", "spaceDelimited", false, "string", "data"),
        ("query", "pipeDelimited", false, "array", r#"["a|b"]"#),
        ("query", "form", false, "array", r#"["a,b"]"#),
        ("query", "deepObject", false, "object", "{}"),
        ("query", "deepObject", true, "object", r#"{"a[b]":1}"#),
    ];
    let server = MockServer::start().await;
    for (location, style, explode, kind, raw) in cases {
        let cached = spec(&server.uri(), &parameter(location, style, explode, kind));
        let error = execute(&cached, call(location, raw), ExecutionContext::default())
            .await
            .unwrap_err();
        assert!(error.to_string().contains("Validation"), "{error}");
    }
    assert!(server.received_requests().await.unwrap().is_empty());
}

#[tokio::test]
async fn unsupported_content_and_reserved_query_policy_are_explicit() {
    let server = MockServer::start().await;
    let mut reserved = parameter("query", "form", true, "string");
    reserved["allowReserved"] = json!(true);
    let content = json!({"name":"id", "in":"query", "content":{"application/json":{"schema":{"type":"object"}}}});
    for definition in [reserved, content] {
        let cached = spec(&server.uri(), &definition);
        let error = execute(&cached, call("query", "data"), ExecutionContext::default())
            .await
            .unwrap_err();
        assert!(error.to_string().contains("unsupported"), "{error}");
    }
    assert!(server.received_requests().await.unwrap().is_empty());
}

#[test]
fn metadata_survives_json_and_binary_roundtrips_with_legacy_json_defaults() {
    let cached = spec(
        "https://example.test",
        &parameter("path", "matrix", true, "object"),
    );
    let parameter = &cached.commands[0].parameters[0];
    assert_eq!(parameter.serialization.style.as_deref(), Some("matrix"));
    assert_eq!(parameter.serialization.explode, Some(true));
    let binary = postcard::to_allocvec(parameter).unwrap();
    assert_eq!(
        postcard::from_bytes::<CachedParameter>(&binary).unwrap(),
        *parameter
    );
    let mut json = serde_json::to_value(parameter).unwrap();
    assert_eq!(
        serde_json::from_value::<CachedParameter>(json.clone()).unwrap(),
        *parameter
    );
    json.as_object_mut().unwrap().remove("serialization");
    let legacy: CachedParameter = serde_json::from_value(json).unwrap();
    assert_eq!(legacy.serialization, ParameterSerialization::default());
    assert_eq!(
        cached.cache_format_version,
        aperture_cli::cache::models::CACHE_FORMAT_VERSION
    );
}

#[tokio::test]
async fn ambiguous_composed_shapes_are_rejected_instead_of_assumed_scalar() {
    let server = MockServer::start().await;
    for schema in [json!({"oneOf":[{"type":"string"},{"type":"array"}]})] {
        let definition = json!({"name":"id","in":"path","required":true,"schema":schema});
        let cached = spec(&server.uri(), &definition);
        let error = execute(&cached, call("path", "data"), ExecutionContext::default())
            .await
            .unwrap_err();
        assert!(error.to_string().contains("unambiguous"), "{error}");
    }
    assert!(server.received_requests().await.unwrap().is_empty());
}

#[test]
fn cli_string_value_model_accepts_json_compound_parameters() {
    let cached = spec(
        "https://example.test",
        &parameter("path", "matrix", true, "array"),
    );
    let group = cached.commands[0].name.as_str();
    let tree = aperture_cli::engine::generator::generate_command_tree(&cached);
    let matches = tree
        .try_get_matches_from(["aperture", group, "get-items", "--id", r#"["a","b"]"#])
        .unwrap();
    let call = aperture_cli::cli::translate::matches_to_operation_call(&cached, &matches).unwrap();
    assert_eq!(call.path_params["id"], r#"["a","b"]"#);
}

#[tokio::test]
async fn compound_items_and_properties_respect_declared_primitive_types() {
    let server = MockServer::start().await;
    let mut array = parameter("path", "matrix", true, "array");
    array["schema"]["items"] = json!({"type":"integer"});
    let mut object = parameter("path", "label", true, "object");
    object["schema"]["properties"] = json!({"active":{"type":"boolean"}});
    for (definition, raw) in [(array, r#"["wrong"]"#), (object, r#"{"active":"wrong"}"#)] {
        let cached = spec(&server.uri(), &definition);
        let error = execute(&cached, call("path", raw), ExecutionContext::default())
            .await
            .unwrap_err();
        assert!(error.to_string().contains("Parameter"), "{error}");
    }
    assert!(server.received_requests().await.unwrap().is_empty());
}

#[test]
fn old_binary_cache_metadata_is_invalidated_before_loading_changed_layout() {
    use aperture_cli::cache::metadata::CacheMetadataManager;
    use aperture_cli::cache::models::{GlobalCacheMetadata, SpecMetadata};
    let directory = tempfile::tempdir().unwrap();
    let fs = aperture_cli::fs::OsFileSystem;
    let manager = CacheMetadataManager::new(&fs);
    let mut metadata = GlobalCacheMetadata {
        cache_format_version: 8,
        specs: HashMap::from([(
            "serialization".into(),
            SpecMetadata {
                updated_at: "2026-10-03T00:00:00Z".into(),
                file_size: 1,
                content_hash: None,
                mtime_secs: None,
                spec_file_size: None,
            },
        )]),
    };
    manager.save_metadata(directory.path(), &metadata).unwrap();
    assert!(!manager
        .check_spec_version(directory.path(), "serialization")
        .unwrap());
    metadata.cache_format_version = aperture_cli::cache::models::CACHE_FORMAT_VERSION;
    manager.save_metadata(directory.path(), &metadata).unwrap();
    assert!(manager
        .check_spec_version(directory.path(), "serialization")
        .unwrap());
}

#[tokio::test]
async fn adversarial_compound_input_sweep_has_explicit_empty_and_duplicate_behavior() {
    for (kind, raw, expected) in [
        ("string", "", vec![("id", "")]),
        ("string", "  ", vec![("id", "  ")]),
        ("array", "[]", vec![]),
        ("object", "{}", vec![]),
        (
            "array",
            r#"["same","same"]"#,
            vec![("id", "same"), ("id", "same")],
        ),
        (
            "object",
            r#"{"unknown":"value"}"#,
            vec![("unknown", "value")],
        ),
        // serde_json's object decoder keeps the last duplicate property.
        ("object", r#"{"a":"first","a":"last"}"#, vec![("a", "last")]),
    ] {
        let url = observed_url(&parameter("query", "form", true, kind), "query", raw).await;
        let pairs: Vec<_> = url
            .query_pairs()
            .map(|(k, v)| (k.into_owned(), v.into_owned()))
            .collect();
        let expected: Vec<_> = expected
            .into_iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect();
        assert_eq!(pairs, expected, "{kind}: {raw}");
    }
    let server = MockServer::start().await;
    for kind in ["array", "object", "boolean", "integer"] {
        for raw in ["", "  ", "none", "invalid", "[", "null", "true false"] {
            let cached = spec(&server.uri(), &parameter("query", "form", true, kind));
            assert!(
                execute(&cached, call("query", raw), ExecutionContext::default())
                    .await
                    .is_err(),
                "{kind}: {raw}"
            );
        }
    }
    let mut cached = spec(&server.uri(), &parameter("query", "form", true, "string"));
    cached.commands[0].parameters[0].serialization.style = Some("unknown".into());
    assert!(
        execute(&cached, call("query", "value"), ExecutionContext::default())
            .await
            .is_err()
    );
    assert!(server.received_requests().await.unwrap().is_empty());
}

fn referenced_spec(
    schema: &Value,
    components: &Value,
) -> Result<CachedSpec, aperture_cli::error::Error> {
    let api = serde_json::from_value(json!({
        "openapi":"3.0.3", "info":{"title":"refs","version":"1"},
        "servers":[{"url":"http://127.0.0.1:9"}],
        "paths":{"/items/{id}":{"get":{"operationId":"getItems",
            "parameters":[{"name":"id","in":"path","required":true,"schema":schema}],
            "responses":{"200":{"description":"ok"}}}}},
        "components":{"schemas":components}
    }))
    .unwrap();
    SpecTransformer::new().transform("refs", &api)
}

#[tokio::test]
async fn local_schema_refs_and_opaque_scalars_preserve_serialization() {
    let payload = json!({"$ref":"literal payload", "type":false});
    let components = json!({
        "Text":{"type":"string", "example":payload, "default":payload},
        "Alias":{"$ref":"#/components/schemas/Text"},
        "List":{"type":"array", "items":{"$ref":"#/components/schemas/Text"}},
        "Map":{"type":"object", "properties":{"key":{"$ref":"#/components/schemas/Text"}}}
    });
    let cases = [
        (
            json!({"$ref":"#/components/schemas/Alias"}),
            "a/b?雪",
            "a%2Fb%3F%E9%9B%AA",
        ),
        (
            json!({"$ref":"#/components/schemas/List"}),
            r#"["a/b","c"]"#,
            "a%2Fb,c",
        ),
        (
            json!({"$ref":"#/components/schemas/Map"}),
            r#"{"key":"a/b"}"#,
            "key,a%2Fb",
        ),
        (json!({}), "a/b?雪", "a%2Fb%3F%E9%9B%AA"),
        (json!({"description":"opaque"}), "a/b", "a%2Fb"),
        (
            json!({"allOf":[{"type":"string"},{"description":"opaque"}]}),
            "a/b",
            "a%2Fb",
        ),
        (
            json!({"anyOf":[{"type":"string"},{"type":"string","minLength":1}]}),
            "a/b",
            "a%2Fb",
        ),
    ];
    for (schema, raw, suffix) in cases {
        let cached = referenced_spec(&schema, &components).unwrap();
        let result = execute(
            &cached,
            call("path", raw),
            ExecutionContext {
                dry_run: true,
                ..Default::default()
            },
        )
        .await
        .unwrap();
        let aperture_cli::invocation::ExecutionResult::DryRun { request_info } = result else {
            panic!("expected dry run")
        };
        assert!(
            request_info["url"].as_str().unwrap().ends_with(suffix),
            "{request_info}"
        );
    }
    let cached =
        referenced_spec(&json!({"$ref":"#/components/schemas/Text"}), &components).unwrap();
    let schema: Value =
        serde_json::from_str(cached.commands[0].parameters[0].schema.as_ref().unwrap()).unwrap();
    assert_eq!(schema["example"], payload);
    assert_eq!(schema["default"], payload);
}

#[test]
fn missing_cyclic_external_and_nested_refs_fail_safely() {
    let components = json!({
        "A":{"$ref":"#/components/schemas/B"}, "B":{"$ref":"#/components/schemas/A"},
        "List":{"type":"array","items":{"$ref":"#/components/schemas/List"}}
    });
    for reference in [
        "#/components/schemas/Missing",
        "#/components/schemas/A",
        "#/components/schemas/List",
        "https://example.test/schema",
    ] {
        assert!(
            referenced_spec(&json!({"$ref":reference}), &components).is_err(),
            "{reference}"
        );
    }
}
