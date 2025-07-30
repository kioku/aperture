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
    /// Creates a new CachedSpec with default values for testing
    #[cfg(test)]
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
pub const CACHE_FORMAT_VERSION: u32 = 3;

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

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
pub struct CachedResponse {
    pub status_code: String,
    pub description: Option<String>,
    pub content_type: Option<String>,
    pub schema: Option<String>,
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
