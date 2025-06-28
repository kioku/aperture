use aperture::cache::models::{
    CachedCommand, CachedParameter, CachedRequestBody, CachedResponse, CachedSpec,
};
use serde_json::json;

#[test]
fn test_cached_spec_serialization_deserialization() {
    let spec = CachedSpec {
        name: "test-api".to_string(),
        version: "1.0.0".to_string(),
        commands: vec![
            CachedCommand {
                name: "get-user".to_string(),
                description: Some("Get user by ID".to_string()),
                operation_id: "getUserById".to_string(),
                method: "get".to_string(),
                path: "/users/{id}".to_string(),
                parameters: vec![
                    CachedParameter {
                        name: "id".to_string(),
                        location: "path".to_string(),
                        required: true,
                        schema: Some(json!({ "type": "string" })),
                    },
                    CachedParameter {
                        name: "token".to_string(),
                        location: "header".to_string(),
                        required: false,
                        schema: None,
                    },
                ],
                request_body: None,
                responses: vec![CachedResponse {
                    status_code: "200".to_string(),
                    content: Some(
                        json!({ "type": "object", "properties": { "id": { "type": "string" } } }),
                    ),
                }],
            },
            CachedCommand {
                name: "create-user".to_string(),
                description: None,
                operation_id: "createUser".to_string(),
                method: "post".to_string(),
                path: "/users".to_string(),
                parameters: vec![],
                request_body: Some(CachedRequestBody {
                    content: json!({ "type": "object", "properties": { "name": { "type": "string" } } }),
                    required: true,
                }),
                responses: vec![CachedResponse {
                    status_code: "201".to_string(),
                    content: None,
                }],
            },
        ],
    };

    // Test serialization
    let serialized = serde_json::to_string_pretty(&spec).unwrap();
    println!("Serialized: {}\n", serialized);

    // Test deserialization
    let deserialized: CachedSpec = serde_json::from_str(&serialized).unwrap();
    assert_eq!(spec, deserialized);
}
