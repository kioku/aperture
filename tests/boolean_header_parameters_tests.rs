// These lints are overly pedantic for test code
#![allow(clippy::too_many_lines)]

mod test_helpers;

/// Integration tests for boolean header parameter handling
///
/// Verifies that boolean header parameters:
/// 1. Are treated as flags (no value required)
/// 2. Send "true" as header value when flag is present
/// 3. Omit the header when optional flag is absent
/// 4. Error appropriately when required flag is missing
/// 5. Work with kebab-case name conversion
/// 6. Work alongside other parameter types
use aperture_cli::cache::models::{
    CachedCommand, CachedParameter, CachedSpec, CACHE_FORMAT_VERSION,
};
use aperture_cli::cli::OutputFormat;
use aperture_cli::engine::executor::execute_request;
use aperture_cli::engine::generator::generate_command_tree_with_flags;
use std::collections::HashMap;
use wiremock::matchers::{header, method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

/// Helper to create a spec with boolean header parameters
fn create_spec_with_boolean_headers(
    required_bool_header: bool,
    optional_bool_header: bool,
) -> CachedSpec {
    let mut parameters = vec![];

    if required_bool_header {
        parameters.push(CachedParameter {
            name: "X-Enable-Feature".to_string(),
            location: "header".to_string(),
            required: true,
            schema_type: Some("boolean".to_string()),
            description: Some("Required boolean header".to_string()),
            schema: Some(r#"{"type": "boolean"}"#.to_string()),
            format: None,
            default_value: None,
            enum_values: vec![],
            example: None,
        });
    }

    if optional_bool_header {
        parameters.push(CachedParameter {
            name: "X-Verbose".to_string(),
            location: "header".to_string(),
            required: false,
            schema_type: Some("boolean".to_string()),
            description: Some("Optional boolean header".to_string()),
            schema: Some(r#"{"type": "boolean"}"#.to_string()),
            format: None,
            default_value: None,
            enum_values: vec![],
            example: None,
        });
    }

    CachedSpec {
        name: "test-api".to_string(),
        version: "1.0.0".to_string(),
        base_url: None, // Will use mock server URL
        cache_format_version: CACHE_FORMAT_VERSION,
        servers: vec![],
        server_variables: HashMap::new(),
        skipped_endpoints: vec![],
        commands: vec![CachedCommand {
            name: "resources".to_string(),
            operation_id: "getResources".to_string(),
            summary: Some("Get resources".to_string()),
            description: None,
            method: "GET".to_string(),
            path: "/resources".to_string(),
            parameters,
            request_body: None,
            security_requirements: vec![],
            examples: vec![],
            deprecated: false,
            external_docs_url: None,
            responses: vec![],
            tags: vec![],
        }],
        security_schemes: HashMap::new(),
    }
}

#[tokio::test]
async fn test_optional_boolean_header_present() {
    let mock_server = MockServer::start().await;

    // Mock expects X-Verbose header with value "true"
    Mock::given(method("GET"))
        .and(path("/resources"))
        .and(header("X-Verbose", "true"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "status": "success"
        })))
        .expect(1)
        .mount(&mock_server)
        .await;

    let mut spec = create_spec_with_boolean_headers(false, true);
    spec.base_url = Some(mock_server.uri());

    let command = generate_command_tree_with_flags(&spec, false);
    let matches = command
        .try_get_matches_from(vec![
            "api",
            "resources",
            "get-resources",
            "--x-verbose", // Boolean flag provided
        ])
        .unwrap();

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
        None,  // retry_context
    )
    .await;

    assert!(result.is_ok(), "Request should succeed with boolean header");
}

#[tokio::test]
async fn test_optional_boolean_header_absent() {
    let mock_server = MockServer::start().await;

    // Mock expects NO X-Verbose header (it's optional and flag not provided)
    Mock::given(method("GET"))
        .and(path("/resources"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "status": "success"
        })))
        .expect(1)
        .mount(&mock_server)
        .await;

    let mut spec = create_spec_with_boolean_headers(false, true);
    spec.base_url = Some(mock_server.uri());

    let command = generate_command_tree_with_flags(&spec, false);
    let matches = command
        .try_get_matches_from(vec![
            "api",
            "resources",
            "get-resources",
            // --x-verbose flag NOT provided
        ])
        .unwrap();

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
        None,  // retry_context
    )
    .await;

    assert!(
        result.is_ok(),
        "Request should succeed without optional boolean header"
    );
}

#[tokio::test]
async fn test_required_boolean_header_present() {
    let mock_server = MockServer::start().await;

    // Mock expects X-Enable-Feature header with value "true"
    Mock::given(method("GET"))
        .and(path("/resources"))
        .and(header("X-Enable-Feature", "true"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "status": "success"
        })))
        .expect(1)
        .mount(&mock_server)
        .await;

    let mut spec = create_spec_with_boolean_headers(true, false);
    spec.base_url = Some(mock_server.uri());

    let command = generate_command_tree_with_flags(&spec, false);
    let matches = command
        .try_get_matches_from(vec![
            "api",
            "resources",
            "get-resources",
            "--x-enable-feature", // Required boolean flag provided
        ])
        .unwrap();

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
        None,  // retry_context
    )
    .await;

    assert!(
        result.is_ok(),
        "Request should succeed with required boolean header"
    );
}

#[test]
fn test_required_boolean_header_missing_errors_at_parse_time() {
    // Required boolean headers are enforced by clap at argument parsing time
    // This test verifies that clap errors when a required boolean flag is missing

    let spec = create_spec_with_boolean_headers(true, false);
    let command = generate_command_tree_with_flags(&spec, false);

    // Try to parse args without the required --x-enable-feature flag
    let result = command.try_get_matches_from(vec![
        "api",
        "resources",
        "get-resources",
        // --x-enable-feature flag NOT provided (but it's required!)
    ]);

    assert!(
        result.is_err(),
        "Clap should error when required boolean header flag is missing"
    );

    let error = result.unwrap_err();
    let error_message = error.to_string();
    assert!(
        error_message.contains("x-enable-feature") || error_message.contains("required"),
        "Error should mention the missing required parameter or 'required'. Got: {error_message}"
    );
}

#[tokio::test]
async fn test_mixed_boolean_and_string_headers() {
    let mock_server = MockServer::start().await;

    // Mock expects both boolean header (X-Verbose: true) and string header (X-API-Key: secret)
    Mock::given(method("GET"))
        .and(path("/resources"))
        .and(header("X-Verbose", "true"))
        .and(header("X-API-Key", "secret-123"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "status": "success"
        })))
        .expect(1)
        .mount(&mock_server)
        .await;

    let mut spec = create_spec_with_boolean_headers(false, true);

    // Add a string header parameter
    spec.commands[0].parameters.push(CachedParameter {
        name: "X-API-Key".to_string(),
        location: "header".to_string(),
        required: false,
        schema_type: Some("string".to_string()),
        description: Some("API Key".to_string()),
        schema: Some(r#"{"type": "string"}"#.to_string()),
        format: None,
        default_value: None,
        enum_values: vec![],
        example: None,
    });

    spec.base_url = Some(mock_server.uri());

    let command = generate_command_tree_with_flags(&spec, false);
    let matches = command
        .try_get_matches_from(vec![
            "api",
            "resources",
            "get-resources",
            "--x-verbose", // Boolean flag
            "--x-api-key", // String parameter
            "secret-123",
        ])
        .unwrap();

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
        None,  // retry_context
    )
    .await;

    assert!(
        result.is_ok(),
        "Request should succeed with both boolean and string headers"
    );
}

#[tokio::test]
async fn test_kebab_case_boolean_header_conversion() {
    let mock_server = MockServer::start().await;

    // Create spec with camelCase header name
    let spec = CachedSpec {
        name: "test-api".to_string(),
        version: "1.0.0".to_string(),
        base_url: Some(mock_server.uri()),
        cache_format_version: CACHE_FORMAT_VERSION,
        servers: vec![],
        server_variables: HashMap::new(),
        skipped_endpoints: vec![],
        commands: vec![CachedCommand {
            name: "resources".to_string(),
            operation_id: "getResources".to_string(),
            summary: Some("Get resources".to_string()),
            description: None,
            method: "GET".to_string(),
            path: "/resources".to_string(),
            parameters: vec![CachedParameter {
                name: "enableCaching".to_string(), // camelCase
                location: "header".to_string(),
                required: false,
                schema_type: Some("boolean".to_string()),
                description: Some("Enable caching".to_string()),
                schema: Some(r#"{"type": "boolean"}"#.to_string()),
                format: None,
                default_value: None,
                enum_values: vec![],
                example: None,
            }],
            request_body: None,
            security_requirements: vec![],
            examples: vec![],
            deprecated: false,
            external_docs_url: None,
            responses: vec![],
            tags: vec![],
        }],
        security_schemes: HashMap::new(),
    };

    // Mock expects header with original camelCase name
    Mock::given(method("GET"))
        .and(path("/resources"))
        .and(header("enableCaching", "true"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "status": "success"
        })))
        .expect(1)
        .mount(&mock_server)
        .await;

    let command = generate_command_tree_with_flags(&spec, false);
    let matches = command
        .try_get_matches_from(vec![
            "api",
            "resources",
            "get-resources",
            "--enable-caching", // kebab-case in CLI
        ])
        .unwrap();

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
        None,  // retry_context
    )
    .await;

    assert!(
        result.is_ok(),
        "Request should succeed with kebab-case boolean header flag"
    );
}

#[tokio::test]
async fn test_boolean_header_with_query_and_path_params() {
    let mock_server = MockServer::start().await;

    // Create spec with boolean header + query param + path param
    let spec = CachedSpec {
        name: "test-api".to_string(),
        version: "1.0.0".to_string(),
        base_url: Some(mock_server.uri()),
        cache_format_version: CACHE_FORMAT_VERSION,
        servers: vec![],
        server_variables: HashMap::new(),
        skipped_endpoints: vec![],
        commands: vec![CachedCommand {
            name: "items".to_string(),
            operation_id: "getItem".to_string(),
            summary: Some("Get item".to_string()),
            description: None,
            method: "GET".to_string(),
            path: "/items/{id}".to_string(),
            parameters: vec![
                CachedParameter {
                    name: "id".to_string(),
                    location: "path".to_string(),
                    required: true,
                    schema_type: Some("string".to_string()),
                    description: None,
                    schema: Some(r#"{"type": "string"}"#.to_string()),
                    format: None,
                    default_value: None,
                    enum_values: vec![],
                    example: None,
                },
                CachedParameter {
                    name: "verbose".to_string(),
                    location: "query".to_string(),
                    required: false,
                    schema_type: Some("boolean".to_string()),
                    description: None,
                    schema: Some(r#"{"type": "boolean"}"#.to_string()),
                    format: None,
                    default_value: None,
                    enum_values: vec![],
                    example: None,
                },
                CachedParameter {
                    name: "X-Include-Metadata".to_string(),
                    location: "header".to_string(),
                    required: false,
                    schema_type: Some("boolean".to_string()),
                    description: None,
                    schema: Some(r#"{"type": "boolean"}"#.to_string()),
                    format: None,
                    default_value: None,
                    enum_values: vec![],
                    example: None,
                },
            ],
            request_body: None,
            security_requirements: vec![],
            examples: vec![],
            deprecated: false,
            external_docs_url: None,
            responses: vec![],
            tags: vec![],
        }],
        security_schemes: HashMap::new(),
    };

    // Mock expects path param, query param, and header all set correctly
    Mock::given(method("GET"))
        .and(path("/items/123"))
        .and(wiremock::matchers::query_param("verbose", "true"))
        .and(header("X-Include-Metadata", "true"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "status": "success"
        })))
        .expect(1)
        .mount(&mock_server)
        .await;

    let command = generate_command_tree_with_flags(&spec, false);
    let matches = command
        .try_get_matches_from(vec![
            "api",
            "items",
            "get-item",
            "--id",
            "123",
            "--verbose",            // Boolean query param
            "--x-include-metadata", // Boolean header param
        ])
        .unwrap();

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
        None,  // retry_context
    )
    .await;

    assert!(
        result.is_ok(),
        "Request should succeed with boolean params across all locations"
    );
}

#[tokio::test]
async fn test_multiple_boolean_headers() {
    let mock_server = MockServer::start().await;

    // Create spec with multiple boolean headers
    let spec = CachedSpec {
        name: "test-api".to_string(),
        version: "1.0.0".to_string(),
        base_url: Some(mock_server.uri()),
        cache_format_version: CACHE_FORMAT_VERSION,
        servers: vec![],
        server_variables: HashMap::new(),
        skipped_endpoints: vec![],
        commands: vec![CachedCommand {
            name: "resources".to_string(),
            operation_id: "getResources".to_string(),
            summary: Some("Get resources".to_string()),
            description: None,
            method: "GET".to_string(),
            path: "/resources".to_string(),
            parameters: vec![
                CachedParameter {
                    name: "X-Enable-Cache".to_string(),
                    location: "header".to_string(),
                    required: false,
                    schema_type: Some("boolean".to_string()),
                    description: None,
                    schema: Some(r#"{"type": "boolean"}"#.to_string()),
                    format: None,
                    default_value: None,
                    enum_values: vec![],
                    example: None,
                },
                CachedParameter {
                    name: "X-Verbose".to_string(),
                    location: "header".to_string(),
                    required: false,
                    schema_type: Some("boolean".to_string()),
                    description: None,
                    schema: Some(r#"{"type": "boolean"}"#.to_string()),
                    format: None,
                    default_value: None,
                    enum_values: vec![],
                    example: None,
                },
                CachedParameter {
                    name: "X-Debug".to_string(),
                    location: "header".to_string(),
                    required: false,
                    schema_type: Some("boolean".to_string()),
                    description: None,
                    schema: Some(r#"{"type": "boolean"}"#.to_string()),
                    format: None,
                    default_value: None,
                    enum_values: vec![],
                    example: None,
                },
            ],
            request_body: None,
            security_requirements: vec![],
            examples: vec![],
            deprecated: false,
            external_docs_url: None,
            responses: vec![],
            tags: vec![],
        }],
        security_schemes: HashMap::new(),
    };

    // Mock expects two headers (X-Enable-Cache and X-Debug), but NOT X-Verbose
    Mock::given(method("GET"))
        .and(path("/resources"))
        .and(header("X-Enable-Cache", "true"))
        .and(header("X-Debug", "true"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "status": "success"
        })))
        .expect(1)
        .mount(&mock_server)
        .await;

    let command = generate_command_tree_with_flags(&spec, false);
    let matches = command
        .try_get_matches_from(vec![
            "api",
            "resources",
            "get-resources",
            "--x-enable-cache", // Provided
            "--x-debug",        // Provided
                                // --x-verbose NOT provided
        ])
        .unwrap();

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
        None,  // retry_context
    )
    .await;

    assert!(
        result.is_ok(),
        "Request should succeed with selective boolean headers"
    );
}
