use aperture_cli::cache::models::{CachedCommand, CachedParameter, CachedSpec};
use aperture_cli::cli::OutputFormat;
use aperture_cli::constants;
use aperture_cli::engine::executor::execute_request;
use aperture_cli::engine::generator::generate_command_tree;
use std::collections::HashMap;
use wiremock::matchers::{method, path, query_param};
use wiremock::{Mock, MockServer, ResponseTemplate};

/// Create a test spec with various snake_case and mixed-case parameters
fn create_snake_case_spec() -> CachedSpec {
    CachedSpec {
        cache_format_version: aperture_cli::cache::models::CACHE_FORMAT_VERSION,
        name: "snake-case-api".to_string(),
        version: "1.0.0".to_string(),
        commands: vec![CachedCommand {
            name: "organizations".to_string(),
            description: Some("Organization operations".to_string()),
            summary: None,
            operation_id: "getOrganizationDetails".to_string(),
            method: constants::HTTP_METHOD_GET.to_string(),
            path: "/orgs/{organization_id_or_slug}/details".to_string(),
            parameters: vec![
                CachedParameter {
                    name: "organization_id_or_slug".to_string(),
                    location: constants::PARAM_LOCATION_PATH.to_string(),
                    required: true,
                    description: Some("Organization ID or slug".to_string()),
                    schema: Some(r#"{"type": "string"}"#.to_string()),
                    schema_type: Some(constants::SCHEMA_TYPE_STRING.to_string()),
                    format: None,
                    default_value: None,
                    enum_values: vec![],
                    example: None,
                },
                CachedParameter {
                    name: "include_members".to_string(),
                    location: constants::PARAM_LOCATION_QUERY.to_string(),
                    required: false,
                    description: Some("Include member information".to_string()),
                    schema: Some(r#"{"type": "boolean"}"#.to_string()),
                    schema_type: Some(constants::SCHEMA_TYPE_BOOLEAN.to_string()),
                    format: None,
                    default_value: Some("false".to_string()),
                    enum_values: vec![],
                    example: None,
                },
                CachedParameter {
                    name: "X_Custom_Header".to_string(),
                    location: constants::PARAM_LOCATION_HEADER.to_string(),
                    required: false,
                    description: Some("Custom header value".to_string()),
                    schema: Some(r#"{"type": "string"}"#.to_string()),
                    schema_type: Some(constants::SCHEMA_TYPE_STRING.to_string()),
                    format: None,
                    default_value: None,
                    enum_values: vec![],
                    example: None,
                },
            ],
            responses: vec![],
            request_body: None,
            security_requirements: vec![],
            tags: vec!["organizations".to_string()],
            deprecated: false,
            external_docs_url: None,
            examples: vec![],
        }],
        base_url: None,
        servers: vec!["https://api.example.com".to_string()],
        security_schemes: HashMap::new(),
        skipped_endpoints: vec![],
        server_variables: HashMap::new(),
    }
}

#[test]
fn test_snake_case_parameters_converted_to_kebab_case() {
    let spec = create_snake_case_spec();
    let command = generate_command_tree(&spec);

    // Navigate to the operation
    let orgs_group = command
        .find_subcommand("organizations")
        .expect("organizations group should exist");
    let operation = orgs_group
        .find_subcommand("get-organization-details")
        .expect("get-organization-details operation should exist");

    // Verify path parameter has kebab-case flag
    let org_id_arg = operation
        .get_arguments()
        .find(|arg| arg.get_id() == "organization_id_or_slug")
        .expect("organization_id_or_slug argument should exist");
    assert_eq!(
        org_id_arg.get_long(),
        Some("organization-id-or-slug"),
        "Path parameter flag should be converted to kebab-case"
    );

    // Verify query parameter has kebab-case flag
    let include_members_arg = operation
        .get_arguments()
        .find(|arg| arg.get_id() == "include_members")
        .expect("include_members argument should exist");
    assert_eq!(
        include_members_arg.get_long(),
        Some("include-members"),
        "Query parameter flag should be converted to kebab-case"
    );

    // Verify header parameter has kebab-case flag (and lowercase)
    let custom_header_arg = operation
        .get_arguments()
        .find(|arg| arg.get_id() == "X_Custom_Header")
        .expect("X_Custom_Header argument should exist");
    assert_eq!(
        custom_header_arg.get_long(),
        Some("x-custom-header"),
        "Header parameter flag should be converted to kebab-case and lowercase"
    );
}

#[tokio::test]
async fn test_kebab_case_parameters_work_end_to_end() {
    let spec = create_snake_case_spec();
    let mock_server = MockServer::start().await;

    // Set up mock response
    Mock::given(method("GET"))
        .and(path("/orgs/my-org/details"))
        .and(query_param("include_members", "true"))
        .and(wiremock::matchers::header("X_Custom_Header", "test-value"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "id": "org-123",
            "name": "My Organization",
            "members_included": true
        })))
        .mount(&mock_server)
        .await;

    // Build command with kebab-case flags
    let command = generate_command_tree(&spec);

    // Simulate command line arguments with kebab-case flags
    let matches = command
        .try_get_matches_from(vec![
            "api",
            "organizations",
            "get-organization-details",
            "--organization-id-or-slug",
            "my-org",
            "--include-members",
            "--x-custom-header",
            "test-value",
        ])
        .expect("Should parse kebab-case arguments");

    // Execute the request
    let result = execute_request(
        &spec,
        &matches,
        Some(&mock_server.uri()),
        false, // dry_run
        None,  // idempotency_key
        None,  // global_config
        &OutputFormat::Json,
        None,  // jq_filter
        None,  // cache_config
        false, // capture_output
    )
    .await;

    assert!(
        result.is_ok(),
        "Request should succeed with kebab-case parameters"
    );

    // Verify the response
    if let Ok(Some(output)) = result {
        let json: serde_json::Value =
            serde_json::from_str(&output).expect("Output should be valid JSON");
        assert_eq!(json["id"], "org-123");
        assert_eq!(json["members_included"], true);
    }
}

#[test]
fn test_mixed_case_parameters_normalization() {
    let spec = CachedSpec {
        cache_format_version: aperture_cli::cache::models::CACHE_FORMAT_VERSION,
        name: "mixed-case-api".to_string(),
        version: "1.0.0".to_string(),
        commands: vec![CachedCommand {
            name: "data".to_string(),
            description: Some("Data operations".to_string()),
            summary: None,
            operation_id: "getData".to_string(),
            method: constants::HTTP_METHOD_GET.to_string(),
            path: "/data/{DataID}".to_string(),
            parameters: vec![
                CachedParameter {
                    name: "DataID".to_string(),
                    location: constants::PARAM_LOCATION_PATH.to_string(),
                    required: true,
                    description: Some("Data identifier".to_string()),
                    schema: Some(r#"{"type": "string"}"#.to_string()),
                    schema_type: Some(constants::SCHEMA_TYPE_STRING.to_string()),
                    format: None,
                    default_value: None,
                    enum_values: vec![],
                    example: None,
                },
                CachedParameter {
                    name: "IncludeMetaData".to_string(),
                    location: constants::PARAM_LOCATION_QUERY.to_string(),
                    required: false,
                    description: Some("Include metadata".to_string()),
                    schema: Some(r#"{"type": "boolean"}"#.to_string()),
                    schema_type: Some(constants::SCHEMA_TYPE_BOOLEAN.to_string()),
                    format: None,
                    default_value: None,
                    enum_values: vec![],
                    example: None,
                },
            ],
            responses: vec![],
            request_body: None,
            security_requirements: vec![],
            tags: vec!["data".to_string()],
            deprecated: false,
            external_docs_url: None,
            examples: vec![],
        }],
        base_url: None,
        servers: vec!["https://api.example.com".to_string()],
        security_schemes: HashMap::new(),
        skipped_endpoints: vec![],
        server_variables: HashMap::new(),
    };

    let command = generate_command_tree(&spec);
    let data_group = command
        .find_subcommand("data")
        .expect("data group should exist");
    let operation = data_group
        .find_subcommand("get-data")
        .expect("get-data operation should exist");

    // Verify mixed case parameters are converted properly
    let data_id_arg = operation
        .get_arguments()
        .find(|arg| arg.get_id() == "DataID")
        .expect("DataID argument should exist");
    assert_eq!(
        data_id_arg.get_long(),
        Some("data-id"),
        "Mixed case parameter should be converted to lowercase kebab-case"
    );

    let include_meta_arg = operation
        .get_arguments()
        .find(|arg| arg.get_id() == "IncludeMetaData")
        .expect("IncludeMetaData argument should exist");
    assert_eq!(
        include_meta_arg.get_long(),
        Some("include-meta-data"),
        "CamelCase parameter should be converted to kebab-case"
    );
}
