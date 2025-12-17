// These lints are overly pedantic for test code that constructs complex OpenAPI types
#![allow(clippy::default_trait_access)]
#![allow(clippy::field_reassign_with_default)]
#![allow(clippy::too_many_lines)]

use aperture_cli::constants;
use aperture_cli::spec::transformer::SpecTransformer;
use openapiv3::{
    ExternalDocumentation, Info, MediaType, OpenAPI, Operation, Parameter, ParameterData,
    ParameterSchemaOrContent, PathItem, Paths, QueryStyle, ReferenceOr, Response, Responses,
    Schema, SchemaKind, StatusCode, StringType, Type,
};

#[test]
fn test_enriched_metadata_preservation() {
    let transformer = SpecTransformer::new();

    // Create OpenAPI spec with rich metadata
    let mut paths = Paths::default();

    // Create an operation with all metadata fields
    let operation = Operation {
        tags: vec!["users".to_string(), "admin".to_string()],
        summary: Some("Get user summary".to_string()),
        description: Some("Get detailed user information by ID".to_string()),
        external_docs: Some(ExternalDocumentation {
            description: Some("Learn more".to_string()),
            url: "https://docs.example.com/users".to_string(),
            extensions: Default::default(),
        }),
        operation_id: Some("getUserById".to_string()),
        parameters: vec![ReferenceOr::Item(Parameter::Query {
            parameter_data: ParameterData {
                name: "include".to_string(),
                description: Some("Fields to include".to_string()),
                required: false,
                deprecated: Some(false),
                format: ParameterSchemaOrContent::Schema(ReferenceOr::Item(Schema {
                    schema_data: Default::default(),
                    schema_kind: SchemaKind::Type(Type::String(StringType {
                        format: Default::default(),
                        pattern: None,
                        enumeration: vec![Some("profile".into()), Some("settings".into())],
                        min_length: None,
                        max_length: None,
                    })),
                })),
                example: Some(serde_json::json!("profile")),
                examples: Default::default(),
                explode: None,
                extensions: Default::default(),
            },
            style: QueryStyle::default(),
            allow_reserved: false,
            allow_empty_value: None,
        })],
        responses: {
            let mut responses = Responses::default();
            let mut response = Response {
                description: "Successful response with user data".to_string(),
                headers: Default::default(),
                content: Default::default(),
                links: Default::default(),
                extensions: Default::default(),
            };
            response.content.insert(
                constants::CONTENT_TYPE_JSON.to_string(),
                MediaType {
                    schema: Some(ReferenceOr::Item(Schema {
                        schema_data: Default::default(),
                        schema_kind: SchemaKind::Type(Type::Object(Default::default())),
                    })),
                    example: None,
                    examples: Default::default(),
                    encoding: Default::default(),
                    extensions: Default::default(),
                },
            );
            responses
                .responses
                .insert(StatusCode::Code(200), ReferenceOr::Item(response));
            responses
        },
        deprecated: true,
        security: None,
        servers: vec![],
        request_body: None,
        callbacks: Default::default(),
        extensions: Default::default(),
    };

    let mut path_item = PathItem::default();
    path_item.get = Some(operation);
    paths
        .paths
        .insert("/users/{id}".to_string(), ReferenceOr::Item(path_item));

    let spec = OpenAPI {
        openapi: "3.0.0".to_string(),
        info: Info {
            title: "Test API".to_string(),
            version: "1.0.0".to_string(),
            description: None,
            terms_of_service: None,
            contact: None,
            license: None,
            extensions: Default::default(),
        },
        servers: vec![],
        paths,
        components: None,
        security: None,
        tags: vec![],
        external_docs: None,
        extensions: Default::default(),
    };

    // Transform the spec
    let cached_spec = transformer
        .transform("test-api", &spec)
        .expect("Transform should succeed");

    // Verify metadata was preserved
    assert_eq!(cached_spec.commands.len(), 1);
    let cmd = &cached_spec.commands[0];

    // Check command metadata
    assert_eq!(cmd.summary, Some("Get user summary".to_string()));
    assert_eq!(
        cmd.description,
        Some("Get detailed user information by ID".to_string())
    );
    assert_eq!(cmd.tags, vec!["users".to_string(), "admin".to_string()]);
    assert!(cmd.deprecated);
    assert_eq!(
        cmd.external_docs_url,
        Some("https://docs.example.com/users".to_string())
    );

    // Check parameter metadata
    assert_eq!(cmd.parameters.len(), 1);
    let param = &cmd.parameters[0];
    assert_eq!(param.name, "include");
    assert_eq!(param.description, Some("Fields to include".to_string()));
    assert_eq!(param.enum_values.len(), 2);
    assert!(param.enum_values.contains(&"\"profile\"".to_string()));
    assert!(param.enum_values.contains(&"\"settings\"".to_string()));
    assert_eq!(param.example, Some("\"profile\"".to_string()));

    // Check response metadata
    assert_eq!(cmd.responses.len(), 1);
    let response = &cmd.responses[0];
    assert_eq!(response.status_code, "200");
    assert_eq!(
        response.description,
        Some("Successful response with user data".to_string())
    );
    assert_eq!(
        response.content_type,
        Some(constants::CONTENT_TYPE_JSON.to_string())
    );
    assert!(response.schema.is_some());
}
