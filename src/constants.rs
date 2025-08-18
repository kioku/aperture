//! Centralized string constants for the Aperture CLI
//!
//! This module contains commonly used string literals to:
//! - Reduce string duplication
//! - Improve maintainability
//! - Ensure consistency across the codebase

// HTTP Headers
pub const HEADER_ACCEPT: &str = "Accept";
pub const HEADER_AUTHORIZATION: &str = "Authorization";
pub const HEADER_CONTENT_TYPE: &str = "Content-Type";
pub const HEADER_PROXY_AUTHORIZATION: &str = "Proxy-Authorization";
pub const HEADER_X_API_KEY: &str = "X-Api-Key";
pub const HEADER_X_API_TOKEN: &str = "X-Api-Token";
pub const HEADER_X_AUTH_TOKEN: &str = "X-Auth-Token";
pub const HEADER_API_KEY: &str = "Api-Key";
pub const HEADER_TOKEN: &str = "Token";
pub const HEADER_BEARER: &str = "Bearer";
pub const HEADER_COOKIE: &str = "Cookie";

// Lowercase header names (for HTTP/2 compatibility and internal use)
pub const HEADER_AUTHORIZATION_LC: &str = "authorization";
pub const HEADER_CONTENT_TYPE_LC: &str = "content-type";

// Header prefixes for authentication detection
pub const HEADER_PREFIX_X_AUTH: &str = "x-auth-";
pub const HEADER_PREFIX_X_API: &str = "x-api-";

// Content Types
pub const CONTENT_TYPE_JSON: &str = "application/json";
pub const CONTENT_TYPE_YAML: &str = "application/yaml";
pub const CONTENT_TYPE_XML: &str = "application/xml";
pub const CONTENT_TYPE_FORM: &str = "application/x-www-form-urlencoded";
pub const CONTENT_TYPE_MULTIPART: &str = "multipart/form-data";
pub const CONTENT_TYPE_TEXT: &str = "text/plain";
pub const CONTENT_TYPE_PDF: &str = "application/pdf";
pub const CONTENT_TYPE_GRAPHQL: &str = "application/graphql";
pub const CONTENT_TYPE_OCTET_STREAM: &str = "application/octet-stream";
pub const CONTENT_TYPE_NDJSON: &str = "application/x-ndjson";
pub const CONTENT_TYPE_TEXT_XML: &str = "text/xml";
pub const CONTENT_TYPE_CSV: &str = "text/csv";

// Content type prefixes and identifiers
pub const CONTENT_TYPE_PREFIX_IMAGE: &str = "image/";
pub const CONTENT_TYPE_IDENTIFIER_JSON: &str = "json";
pub const CONTENT_TYPE_IDENTIFIER_YAML: &str = "yaml";
pub const CONTENT_TYPE_IDENTIFIER_TEXT: &str = "text";

// OpenAPI Extensions
pub const EXT_APERTURE_SECRET: &str = "x-aperture-secret";
pub const EXT_KEY_SOURCE: &str = "source";
pub const EXT_KEY_NAME: &str = "name";

// Authentication Schemes
pub const AUTH_SCHEME_BEARER: &str = "bearer";
pub const AUTH_SCHEME_BASIC: &str = "basic";
pub const AUTH_SCHEME_APIKEY: &str = "apiKey";
pub const AUTH_SCHEME_OAUTH2: &str = "oauth2";
pub const AUTH_SCHEME_OPENID: &str = "openidconnect";

// Environment Variables
pub const ENV_APERTURE_CONFIG_DIR: &str = "APERTURE_CONFIG_DIR";
pub const ENV_APERTURE_BASE_URL: &str = "APERTURE_BASE_URL";
pub const ENV_APERTURE_ENV: &str = "APERTURE_ENV";

// Common Response Messages
pub const EMPTY_RESPONSE: &str = "(empty response)";
pub const EMPTY_ARRAY: &str = "(empty array)";
pub const NULL_VALUE: &str = "null";

// Error Context Messages
pub const ERR_API_CREDENTIALS: &str =
    "Check your API credentials and authentication configuration.";
pub const ERR_PERMISSION_DENIED: &str =
    "Your credentials may be valid but lack permission for this operation.";
pub const ERR_ENDPOINT_NOT_FOUND: &str = "Check that the API endpoint and parameters are correct.";
pub const ERR_RATE_LIMITED: &str = "You're making requests too quickly. Wait before trying again.";
pub const ERR_SERVER_ERROR: &str = "The API server is experiencing issues. Try again later.";
pub const ERR_CONNECTION: &str = "Check that the API server is running and accessible.";
pub const ERR_TIMEOUT: &str = "The API server may be slow or unresponsive. Try again later.";

// File System Messages
pub const ERR_FILE_NOT_FOUND: &str = "Check that the file path is correct and the file exists.";
pub const ERR_PERMISSION: &str = "Check file permissions or run with appropriate privileges.";

// Validation Messages
pub const ERR_YAML_SYNTAX: &str = "Check that your OpenAPI specification is valid YAML syntax.";
pub const ERR_JSON_SYNTAX: &str = "Check that your request body or response contains valid JSON.";
pub const ERR_TOML_SYNTAX: &str = "Check that your configuration file is valid TOML syntax.";
pub const ERR_OPENAPI_FORMAT: &str =
    "Check that your OpenAPI specification follows the required format.";

// CLI Messages
pub const MSG_USE_HELP: &str = "Use --help to see available commands.";
pub const MSG_USE_CONFIG_LIST: &str = "Use 'aperture config list' to see available specifications.";
pub const MSG_WARNING_PREFIX: &str = "Warning:";

// Default Values
pub const DEFAULT_GROUP: &str = "default";
pub const DEFAULT_CACHE_TTL: u64 = 300;
pub const DEFAULT_OPERATION_NAME: &str = "unnamed";

// Context identifiers
pub const CONTEXT_BATCH: &str = "batch";

// CLI command names
pub const CLI_ROOT_COMMAND: &str = "api";

// File suffixes and identifiers
pub const CACHE_SUFFIX: &str = "cache";
pub const FILE_EXT_JSON: &str = ".json";
pub const FILE_EXT_YAML: &str = ".yaml";
pub const FILE_EXT_BIN: &str = ".bin";
pub const CACHE_FILE_SUFFIX: &str = "_cache.json";
pub const CACHE_METADATA_FILENAME: &str = "cache_metadata.json";
pub const CONFIG_FILENAME: &str = "config.toml";

// Directory names
pub const DIR_CACHE: &str = ".cache";
pub const DIR_RESPONSES: &str = "responses";
pub const DIR_SPECS: &str = "specs";

// Schema Types
pub const SCHEMA_TYPE_STRING: &str = "string";
pub const SCHEMA_TYPE_NUMBER: &str = "number";
pub const SCHEMA_TYPE_INTEGER: &str = "integer";
pub const SCHEMA_TYPE_BOOLEAN: &str = "boolean";
pub const SCHEMA_TYPE_ARRAY: &str = "array";
pub const SCHEMA_TYPE_OBJECT: &str = "object";

// HTTP Methods
pub const HTTP_METHOD_GET: &str = "GET";
pub const HTTP_METHOD_POST: &str = "POST";
pub const HTTP_METHOD_PUT: &str = "PUT";
pub const HTTP_METHOD_DELETE: &str = "DELETE";
pub const HTTP_METHOD_PATCH: &str = "PATCH";
pub const HTTP_METHOD_HEAD: &str = "HEAD";
pub const HTTP_METHOD_OPTIONS: &str = "OPTIONS";

// Parameter Locations
pub const PARAM_LOCATION_PATH: &str = "path";
pub const PARAM_LOCATION_QUERY: &str = "query";
pub const PARAM_LOCATION_HEADER: &str = "header";
pub const PARAM_LOCATION_COOKIE: &str = "cookie";

// Security Scheme Types
pub const SECURITY_TYPE_HTTP: &str = "http";
pub const SECURITY_TYPE_APIKEY: &str = "apiKey";

// Common Values
pub const SOURCE_ENV: &str = "env";
pub const LOCATION_HEADER: &str = "header";

// OpenAPI Field Names (for JSON parsing)
pub const FIELD_DEPRECATED: &str = "deprecated";
pub const FIELD_REQUIRED: &str = "required";
pub const FIELD_READ_ONLY: &str = "readOnly";
pub const FIELD_WRITE_ONLY: &str = "writeOnly";
pub const FIELD_NULLABLE: &str = "nullable";
pub const FIELD_UNIQUE_ITEMS: &str = "uniqueItems";
pub const FIELD_ALLOW_EMPTY_VALUE: &str = "allowEmptyValue";
pub const FIELD_EXPLODE: &str = "explode";
pub const FIELD_ALLOW_RESERVED: &str = "allowReserved";
pub const FIELD_EXCLUSIVE_MINIMUM: &str = "exclusiveMinimum";
pub const FIELD_EXCLUSIVE_MAXIMUM: &str = "exclusiveMaximum";

// OpenAPI Component Names
pub const COMPONENT_SCHEMAS: &str = "schemas";
pub const COMPONENT_RESPONSES: &str = "responses";
pub const COMPONENT_EXAMPLES: &str = "examples";
pub const COMPONENT_PARAMETERS: &str = "parameters";
pub const COMPONENT_REQUEST_BODIES: &str = "requestBodies";
pub const COMPONENT_HEADERS: &str = "headers";
pub const COMPONENT_SECURITY_SCHEMES: &str = "securitySchemes";
pub const COMPONENT_LINKS: &str = "links";
pub const COMPONENT_CALLBACKS: &str = "callbacks";
pub const COMPONENT_COMPONENTS: &str = "components";

/// Check if a header name is authentication-related
#[must_use]
pub fn is_auth_header(name: &str) -> bool {
    matches!(
        name.to_lowercase().as_str(),
        "authorization"
            | "proxy-authorization"
            | "x-api-key"
            | "x-api-token"
            | "x-auth-token"
            | "api-key"
            | "token"
            | "bearer"
            | "cookie"
    )
}

/// Check if a content type is JSON
#[must_use]
pub fn is_json_content_type(content_type: &str) -> bool {
    content_type.contains(CONTENT_TYPE_IDENTIFIER_JSON)
}

/// Check if a content type is supported for request/response bodies
#[must_use]
pub fn is_supported_content_type(content_type: &str) -> bool {
    let ct = content_type.to_lowercase();
    ct.contains(CONTENT_TYPE_IDENTIFIER_JSON)
        || ct.contains(CONTENT_TYPE_IDENTIFIER_YAML)
        || ct.contains(CONTENT_TYPE_IDENTIFIER_TEXT)
}
