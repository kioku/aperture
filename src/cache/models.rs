use serde::{Deserialize, Serialize};
use std::collections::HashMap;

#[derive(Debug, Deserialize, Serialize, PartialEq, Eq)]
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
}

/// Current cache format version - increment when making breaking changes to `CachedSpec`
pub const CACHE_FORMAT_VERSION: u32 = 1;

#[derive(Debug, Deserialize, Serialize, PartialEq, Eq)]
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
}

#[derive(Debug, Deserialize, Serialize, PartialEq, Eq)]
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

#[derive(Debug, Deserialize, Serialize, PartialEq, Eq)]
pub struct CachedRequestBody {
    pub content_type: String,
    pub schema: String,
    pub required: bool,
    pub description: Option<String>,
    pub example: Option<String>,
}

#[derive(Debug, Deserialize, Serialize, PartialEq, Eq)]
pub struct CachedResponse {
    pub status_code: String,
    pub description: Option<String>,
    pub content_type: Option<String>,
    pub schema: Option<String>,
}

/// Cached representation of a security scheme with x-aperture-secret mapping
#[derive(Debug, Deserialize, Serialize, PartialEq, Eq)]
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
