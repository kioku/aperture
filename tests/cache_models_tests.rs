use aperture_cli::cache::models::{CachedCommand, CachedResponse, CachedSpec};
use std::collections::HashMap;

mod test_helpers;
use test_helpers::*;

#[test]
fn test_cached_spec_serialization_deserialization() {
    let spec = CachedSpec {
        cache_format_version: aperture_cli::cache::models::CACHE_FORMAT_VERSION,
        name: "test-api".to_string(),
        version: "1.0.0".to_string(),
        commands: vec![
            CachedCommand {
                name: "get-user".to_string(),
                description: Some("Get user by ID".to_string()),
                summary: None,
                operation_id: "getUserById".to_string(),
                method: "get".to_string(),
                path: "/users/{id}".to_string(),
                parameters: vec![
                    test_parameter("id", "path", true),
                    test_parameter("token", "header", false),
                ],
                request_body: None,
                responses: vec![test_response("200")],
                security_requirements: vec![],
                tags: vec!["get-user".to_string()],
                deprecated: false,
                external_docs_url: None,
                examples: vec![],
            },
            CachedCommand {
                name: "create-user".to_string(),
                description: None,
                summary: None,
                operation_id: "createUser".to_string(),
                method: "post".to_string(),
                path: "/users".to_string(),
                parameters: vec![],
                request_body: Some(test_request_body()),
                responses: vec![CachedResponse {
                    status_code: "201".to_string(),
                    description: None,
                    content_type: None,
                    schema: None,
                    example: None,
                }],
                security_requirements: vec![],
                tags: vec!["create-user".to_string()],
                deprecated: false,
                external_docs_url: None,
                examples: vec![],
            },
        ],
        base_url: Some("https://api.example.com".to_string()),
        servers: vec!["https://api.example.com".to_string()],
        security_schemes: HashMap::new(),
        skipped_endpoints: vec![],
        server_variables: HashMap::new(),
    };

    // Test serialization
    let serialized = serde_json::to_string_pretty(&spec).unwrap();
    println!("Serialized: {serialized}\n");

    // Test deserialization
    let deserialized: CachedSpec = serde_json::from_str(&serialized).unwrap();
    assert_eq!(spec, deserialized);
}
