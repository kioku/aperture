use serde::{Deserialize, Serialize};
use std::collections::HashMap;

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
pub struct CachedSpec {
    /// Cache format version to detect incompatible changes
    pub cache_format_version: u32,
    pub name: String,
    pub version: String,
    pub commands: Vec<CachedCommand>,
    /// Base URL extracted from the first server in the `OpenAPI` spec
    pub base_url: Option<String>,
    /// All server URLs from the `OpenAPI` spec for future multi-environment support
    pub servers: Vec<String>,
    /// Security schemes defined in the `OpenAPI` spec with `x-aperture-secret` mappings
    pub security_schemes: HashMap<String, CachedSecurityScheme>,
    /// Endpoints skipped during validation due to unsupported features (added in v0.1.2)
    #[serde(default)]
    pub skipped_endpoints: Vec<SkippedEndpoint>,
    /// Server variables defined in the `OpenAPI` spec for URL template resolution (added in v0.1.3)
    #[serde(default)]
    pub server_variables: HashMap<String, ServerVariable>,
}

impl CachedSpec {
    /// Creates a new `CachedSpec` with default values for testing
    #[cfg(test)]
    #[must_use]
    pub fn new_for_test(name: &str) -> Self {
        Self {
            cache_format_version: CACHE_FORMAT_VERSION,
            name: name.to_string(),
            version: "1.0.0".to_string(),
            commands: vec![],
            base_url: None,
            servers: vec![],
            security_schemes: HashMap::new(),
            skipped_endpoints: vec![],
            server_variables: HashMap::new(),
        }
    }
}

/// Pagination strategy detected from the `OpenAPI` spec for an operation.
///
/// Stored at cache time to avoid re-parsing the spec on each request.
#[derive(Debug, Clone, Copy, Deserialize, Serialize, PartialEq, Eq, Default)]
#[serde(rename_all = "kebab-case")]
pub enum PaginationStrategy {
    /// No pagination detected; `--auto-paginate` will warn and execute once.
    #[default]
    None,
    /// Cursor-based: a field in the response body carries the next-page token.
    Cursor,
    /// Offset/page-based: incrementing a `page` or `offset` query parameter.
    Offset,
    /// RFC 5988 `Link: <url>; rel="next"` header drives the next request URL.
    LinkHeader,
}

/// Detected pagination configuration for a single operation.
///
/// All `Option` fields are serialized without `skip_serializing_if` so that
/// postcard binary encoding remains position-stable across reads and writes.
/// JSON compactness is handled by [`PaginationManifestInfo`] in the agent layer.
#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq, Default)]
pub struct PaginationInfo {
    /// How pagination is driven for this operation.
    pub strategy: PaginationStrategy,
    /// Response body field containing the next cursor (`cursor` strategy).
    pub cursor_field: Option<String>,
    /// Query parameter to inject the cursor into (`cursor` strategy).
    pub cursor_param: Option<String>,
    /// Query parameter to increment (`offset` strategy, e.g. `"page"`, `"offset"`).
    pub page_param: Option<String>,
    /// Query parameter carrying the page size (`offset` strategy).
    pub limit_param: Option<String>,
}

/// Example usage for a command
#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
pub struct CommandExample {
    /// Brief description of what this example demonstrates
    pub description: String,
    /// The complete command line example
    pub command_line: String,
    /// Optional explanation of the parameters used
    pub explanation: Option<String>,
}

/// Information about an endpoint that was skipped during spec validation
#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
pub struct SkippedEndpoint {
    pub path: String,
    pub method: String,
    pub content_type: String,
    pub reason: String,
}

/// Current cache format version - increment when making breaking changes to `CachedSpec`
///
/// Version 2: Added `skipped_endpoints` field to track endpoints skipped during validation
/// Version 3: Added `server_variables` field to support `OpenAPI` server URL template variables
/// Version 4: Added `example` field to `CachedResponse` for response schema examples
/// Version 5: Added `display_group`, `display_name`, `aliases`, `hidden` fields for command mapping
/// Version 6: Added `pagination` field to `CachedCommand` for auto-pagination support
/// Version 7: Preserves all response media and `default` declarations during transformation
pub const CACHE_FORMAT_VERSION: u32 = 7;

/// Global cache metadata for all cached specifications
#[derive(Debug, Deserialize, Serialize, PartialEq, Eq)]
pub struct GlobalCacheMetadata {
    /// Cache format version for all specs
    pub cache_format_version: u32,
    /// Individual spec metadata
    pub specs: std::collections::HashMap<String, SpecMetadata>,
}

/// Metadata for a single cached specification
#[derive(Debug, Deserialize, Serialize, PartialEq, Eq, Clone)]
pub struct SpecMetadata {
    /// When this spec cache was created/updated
    pub updated_at: String, // Using String for simplicity in serialization
    /// Size of the cached spec file in bytes
    pub file_size: u64,
    /// SHA-256 hash of the original spec file content for cache invalidation
    #[serde(default)]
    pub content_hash: Option<String>,
    /// File modification time (seconds since epoch) for fast staleness checks
    #[serde(default)]
    pub mtime_secs: Option<u64>,
    /// Size of the original spec file in bytes (distinct from cached binary size)
    #[serde(default)]
    pub spec_file_size: Option<u64>,
}

impl Default for GlobalCacheMetadata {
    fn default() -> Self {
        Self {
            cache_format_version: CACHE_FORMAT_VERSION,
            specs: std::collections::HashMap::new(),
        }
    }
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
pub struct CachedCommand {
    pub name: String,
    pub description: Option<String>,
    pub summary: Option<String>,
    pub operation_id: String,
    pub method: String,
    pub path: String,
    pub parameters: Vec<CachedParameter>,
    pub request_body: Option<CachedRequestBody>,
    pub responses: Vec<CachedResponse>,
    /// Security requirements for this operation (references to security scheme names)
    pub security_requirements: Vec<String>,
    /// All tags associated with this operation
    pub tags: Vec<String>,
    /// Whether this operation is deprecated
    pub deprecated: bool,
    /// External documentation URL if available
    pub external_docs_url: Option<String>,
    /// Usage examples for this command (added in v0.1.6)
    #[serde(default)]
    pub examples: Vec<CommandExample>,
    /// Display name override for the command group (tag), from command mapping (added in v5)
    #[serde(default)]
    pub display_group: Option<String>,
    /// Display name override for the subcommand (operation), from command mapping (added in v5)
    #[serde(default)]
    pub display_name: Option<String>,
    /// Additional subcommand aliases from command mapping (added in v5)
    #[serde(default)]
    pub aliases: Vec<String>,
    /// Whether this command is hidden from help output, from command mapping (added in v5)
    #[serde(default)]
    pub hidden: bool,
    /// Pagination configuration detected from the `OpenAPI` spec (added in v6)
    #[serde(default)]
    pub pagination: PaginationInfo,
}

impl CachedCommand {
    fn successful_body_responses(&self) -> impl Iterator<Item = &CachedResponse> {
        self.responses
            .iter()
            .filter(|response| response.may_be_successful_body())
    }

    /// Returns whether any successful response explicitly declares supported binary bytes.
    #[must_use]
    pub fn has_binary_response(&self) -> bool {
        self.successful_body_responses()
            .any(CachedResponse::is_binary)
    }

    /// Returns whether successful bodies mix binary and non-binary representations.
    #[must_use]
    pub fn has_ambiguous_binary_response(&self) -> bool {
        self.has_binary_response()
            && self
                .successful_body_responses()
                .any(|response| !response.is_binary())
    }

    /// Returns the single declared binary response media type when all body variants agree.
    #[must_use]
    pub fn binary_response_content_type(&self) -> Option<&str> {
        if self.has_ambiguous_binary_response() {
            return None;
        }
        let mut binary = self
            .successful_body_responses()
            .filter(|response| response.is_binary());
        let first = binary.next()?.content_type.as_deref()?;
        let normalized = normalized_content_type(first);
        binary
            .all(|response| {
                response
                    .content_type
                    .as_deref()
                    .is_some_and(|content_type| normalized_content_type(content_type) == normalized)
            })
            .then_some(first)
    }

    /// Returns whether request or response bytes make the text response cache unsafe.
    #[must_use]
    pub fn has_binary_io(&self) -> bool {
        self.request_body
            .as_ref()
            .is_some_and(CachedRequestBody::is_binary)
            || self.has_binary_response()
    }
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
pub struct CachedParameter {
    pub name: String,
    pub location: String,
    pub required: bool,
    pub description: Option<String>,
    pub schema: Option<String>,
    pub schema_type: Option<String>,
    pub format: Option<String>,
    pub default_value: Option<String>,
    pub enum_values: Vec<String>,
    pub example: Option<String>,
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
pub struct CachedRequestBody {
    pub content_type: String,
    pub schema: String,
    pub required: bool,
    pub description: Option<String>,
    pub example: Option<String>,
}

impl CachedRequestBody {
    /// Returns whether this body uses the native single-part byte stream contract.
    #[must_use]
    pub fn is_binary(&self) -> bool {
        is_supported_binary_media_schema(&self.content_type, &self.schema)
    }

    /// Returns whether this body uses a JSON media type.
    #[must_use]
    pub fn is_json(&self) -> bool {
        let content_type = normalized_content_type(&self.content_type);
        content_type == crate::constants::CONTENT_TYPE_JSON || content_type.ends_with("+json")
    }
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
pub struct CachedResponse {
    pub status_code: String,
    pub description: Option<String>,
    pub content_type: Option<String>,
    pub schema: Option<String>,
    /// Example response value (JSON-serialized)
    #[serde(default)]
    pub example: Option<String>,
}

impl CachedResponse {
    /// Returns whether this declaration can represent a successful response body.
    #[must_use]
    pub(crate) fn may_be_successful_body(&self) -> bool {
        crate::spec::response_status_may_be_successful(&self.status_code)
            && self.content_type.is_some()
    }

    /// Returns whether this response declares a JSON media type.
    #[must_use]
    pub fn is_json(&self) -> bool {
        self.content_type.as_deref().is_some_and(|content_type| {
            let content_type = normalized_content_type(content_type);
            content_type == crate::constants::CONTENT_TYPE_JSON || content_type.ends_with("+json")
        })
    }

    /// Returns whether this response uses the native single-part byte stream contract.
    #[must_use]
    pub fn is_binary(&self) -> bool {
        self.content_type.as_deref().is_some_and(|content_type| {
            self.schema
                .as_deref()
                .is_some_and(|schema| is_supported_binary_media_schema(content_type, schema))
        })
    }
}

fn normalized_content_type(content_type: &str) -> String {
    content_type
        .split(';')
        .next()
        .unwrap_or(content_type)
        .trim()
        .to_ascii_lowercase()
}

/// Returns whether a media type can use Aperture's raw single-part byte path.
#[must_use]
pub(crate) fn is_supported_binary_media_type(content_type: &str) -> bool {
    let media_type = normalized_content_type(content_type);
    !media_type.is_empty()
        && media_type.contains('/')
        && media_type != crate::constants::CONTENT_TYPE_JSON
        && !media_type.ends_with("+json")
        && !media_type.starts_with("text/")
        && !media_type.starts_with("multipart/")
        && media_type != "application/x-www-form-urlencoded"
        && !matches!(
            media_type.as_str(),
            "application/xml"
                | "application/graphql"
                | "application/x-ndjson"
                | "application/ndjson"
                | "application/yaml"
                | "application/x-yaml"
                | "application/javascript"
        )
        && !media_type.ends_with("+xml")
}

/// Classifies a serialized root schema and media type using the cached-model contract.
#[must_use]
pub(crate) fn is_supported_binary_media_schema(content_type: &str, schema: &str) -> bool {
    is_supported_binary_media_type(content_type) && schema_is_string_binary(schema)
}

fn schema_is_string_binary(schema: &str) -> bool {
    let Ok(serde_json::Value::Object(schema)) = serde_json::from_str(schema) else {
        return false;
    };
    schema.get("type").and_then(serde_json::Value::as_str) == Some("string")
        && schema.get("format").and_then(serde_json::Value::as_str) == Some("binary")
}

/// Cached representation of a security scheme with x-aperture-secret mapping
#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
pub struct CachedSecurityScheme {
    /// Security scheme name from the `OpenAPI` spec
    pub name: String,
    /// Type of security scheme (apiKey, http, oauth2, etc.)
    pub scheme_type: String,
    /// Subtype for http schemes (bearer, basic, etc.)
    pub scheme: Option<String>,
    /// Location for apiKey schemes (header, query, cookie)
    pub location: Option<String>,
    /// Parameter name for apiKey schemes (e.g., "Authorization", "X-API-Key")
    pub parameter_name: Option<String>,
    /// Description of the security scheme from `OpenAPI` spec
    pub description: Option<String>,
    /// Bearer format for HTTP bearer schemes (e.g., "JWT")
    pub bearer_format: Option<String>,
    /// x-aperture-secret mapping for environment variable resolution
    pub aperture_secret: Option<CachedApertureSecret>,
}

/// Cached representation of x-aperture-secret extension
#[derive(Debug, Deserialize, Serialize, PartialEq, Eq, Clone)]
pub struct CachedApertureSecret {
    /// Source of the secret (currently only "env" supported)
    pub source: String,
    /// Environment variable name to read the secret from
    pub name: String,
}

/// Cached representation of an `OpenAPI` server variable for URL template resolution
#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
pub struct ServerVariable {
    /// Default value for the variable if not provided via CLI
    pub default: Option<String>,
    /// Allowed values for the variable (enum constraint)
    pub enum_values: Vec<String>,
    /// Description of the server variable from `OpenAPI` spec
    pub description: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    const BINARY_SCHEMA: &str = r#"{"type":"string","format":"binary"}"#;

    fn response(status: &str, content_type: &str, schema: &str) -> CachedResponse {
        CachedResponse {
            status_code: status.to_string(),
            description: None,
            content_type: Some(content_type.to_string()),
            schema: Some(schema.to_string()),
            example: None,
        }
    }

    #[test]
    fn binary_schema_must_be_string_binary_at_root() {
        assert!(schema_is_string_binary(BINARY_SCHEMA));
        assert!(!schema_is_string_binary(
            r#"{"type":"object","properties":{"payload":{"type":"string","format":"binary"}}}"#
        ));
        assert!(!schema_is_string_binary(
            r#"{"type":"object","typeHint":"string","formatHint":"binary"}"#
        ));
        assert!(!schema_is_string_binary(
            r#"{"type":"string","properties":{"payload":{"format":"binary"}}}"#
        ));
    }

    #[test]
    fn modeled_png_and_pdf_are_supported_binary_media() {
        for content_type in ["image/png", "Application/PDF; version=1.7"] {
            assert!(is_supported_binary_media_type(content_type));
            assert!(response("200", content_type, BINARY_SCHEMA).is_binary());
        }
        for content_type in [
            "application/json",
            "application/problem+json",
            "multipart/form-data",
            "application/x-www-form-urlencoded",
            "application/xml",
            "image/svg+xml",
            "text/plain",
        ] {
            assert!(!is_supported_binary_media_type(content_type));
        }
    }

    #[test]
    fn mixed_successful_binary_and_json_response_is_ambiguous() {
        let operation = CachedCommand {
            name: "download".to_string(),
            description: None,
            summary: None,
            operation_id: "download".to_string(),
            method: "GET".to_string(),
            path: "/download".to_string(),
            parameters: vec![],
            request_body: None,
            responses: vec![
                response("200", "image/png", BINARY_SCHEMA),
                response("201", "application/pdf", BINARY_SCHEMA),
                CachedResponse {
                    status_code: "202".to_string(),
                    description: None,
                    content_type: Some("application/json".to_string()),
                    schema: None,
                    example: None,
                },
            ],
            security_requirements: vec![],
            tags: vec![],
            deprecated: false,
            external_docs_url: None,
            examples: vec![],
            display_group: None,
            display_name: None,
            aliases: vec![],
            hidden: false,
            pagination: PaginationInfo::default(),
        };

        assert!(operation.has_binary_response());
        assert!(operation.has_ambiguous_binary_response());
        assert!(operation.has_binary_io());
        assert_eq!(operation.binary_response_content_type(), None);
    }
}
