#![allow(dead_code)]

use aperture_cli::cache::models::{
    CachedCommand, CachedParameter, CachedRequestBody, CachedResponse,
};
use aperture_cli::constants;

/// Initialize the rustls crypto provider before any tests run.
/// This runs once per test binary when `test_helpers` is included.
#[ctor::ctor]
fn init_crypto_provider() {
    let _ = rustls::crypto::aws_lc_rs::default_provider().install_default();
}

#[must_use]
pub fn test_parameter(name: &str, location: &str, required: bool) -> CachedParameter {
    CachedParameter {
        name: name.to_string(),
        location: location.to_string(),
        required,
        description: None,
        schema: Some(r#"{"type": "string"}"#.to_string()),
        schema_type: Some("string".to_string()),
        format: None,
        default_value: None,
        enum_values: vec![],
        example: None,
    }
}

#[must_use]
pub fn test_response(status_code: &str) -> CachedResponse {
    CachedResponse {
        status_code: status_code.to_string(),
        description: None,
        content_type: Some(constants::CONTENT_TYPE_JSON.to_string()),
        schema: Some(r#"{"type": "object"}"#.to_string()),
        example: None,
    }
}

#[must_use]
pub fn test_request_body() -> CachedRequestBody {
    CachedRequestBody {
        content_type: constants::CONTENT_TYPE_JSON.to_string(),
        schema: r#"{"type": "object"}"#.to_string(),
        required: true,
        description: None,
        example: None,
    }
}

#[must_use]
pub fn test_command(name: &str, operation_id: &str, method: &str, path: &str) -> CachedCommand {
    CachedCommand {
        name: name.to_string(),
        description: None,
        summary: None,
        operation_id: operation_id.to_string(),
        method: method.to_string(),
        path: path.to_string(),
        parameters: vec![],
        request_body: None,
        responses: vec![test_response("200")],
        security_requirements: vec![],
        tags: vec![name.to_string()],
        deprecated: false,
        external_docs_url: None,
        examples: vec![],
    }
}
