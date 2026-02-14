use serde::{Deserialize, Serialize};
use std::collections::HashMap;

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct GlobalConfig {
    #[serde(default = "default_timeout_secs_value")]
    pub default_timeout_secs: u64,
    #[serde(default)]
    pub agent_defaults: AgentDefaults,
    /// Default retry configuration for transient failures
    #[serde(default)]
    pub retry_defaults: RetryDefaults,
    /// Per-API configuration overrides
    #[serde(default)]
    pub api_configs: HashMap<String, ApiConfig>,
}

const fn default_timeout_secs_value() -> u64 {
    30
}

#[derive(Debug, Clone, Deserialize, Serialize, Default)]
pub struct AgentDefaults {
    #[serde(default)]
    pub json_errors: bool,
}

/// Default retry configuration for API requests.
///
/// These settings control automatic retry behavior for transient failures.
/// Set `max_attempts` to 0 to disable retries (the default).
///
/// Retryable status codes are determined by the `is_retryable_status` function
/// in the resilience module (408, 429, 500-504 excluding 501/505).
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct RetryDefaults {
    /// Maximum number of retry attempts (0 = disabled, 1-10 recommended)
    #[serde(default)]
    pub max_attempts: u32,
    /// Initial delay between retries in milliseconds (default: 500ms)
    #[serde(default = "default_initial_delay_ms")]
    pub initial_delay_ms: u64,
    /// Maximum delay cap in milliseconds (default: 30000ms = 30s)
    #[serde(default = "default_max_delay_ms")]
    pub max_delay_ms: u64,
}

const fn default_initial_delay_ms() -> u64 {
    500
}

const fn default_max_delay_ms() -> u64 {
    30_000
}

impl Default for RetryDefaults {
    fn default() -> Self {
        Self {
            max_attempts: 0, // Disabled by default
            initial_delay_ms: default_initial_delay_ms(),
            max_delay_ms: default_max_delay_ms(),
        }
    }
}

impl Default for GlobalConfig {
    fn default() -> Self {
        Self {
            default_timeout_secs: 30,
            agent_defaults: AgentDefaults::default(),
            retry_defaults: RetryDefaults::default(),
            api_configs: HashMap::new(),
        }
    }
}

/// Per-API configuration for base URLs and environment-specific settings
#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct ApiConfig {
    /// Override base URL for this API
    pub base_url_override: Option<String>,
    /// Environment-specific base URLs (e.g., "dev", "staging", "prod")
    #[serde(default)]
    pub environment_urls: HashMap<String, String>,
    /// Whether this spec was added with --strict flag (preserved for reinit)
    #[serde(default)]
    pub strict_mode: bool,
    /// Secret configurations for security schemes (overrides x-aperture-secret extensions)
    #[serde(default)]
    pub secrets: HashMap<String, ApertureSecret>,
    /// Custom command tree mapping (rename groups, operations, add aliases, hide commands)
    #[serde(default)]
    pub command_mapping: Option<CommandMapping>,
}

/// Custom command tree mapping for an API specification.
///
/// Allows users to customize the CLI command tree structure generated from an
/// `OpenAPI` spec without modifying the original specification. This is especially
/// useful for third-party specs with verbose or awkward naming.
///
/// Config-based mappings take precedence over default tag/operationId naming.
#[derive(Debug, Deserialize, Serialize, Clone, Default, PartialEq, Eq)]
pub struct CommandMapping {
    /// Tag group renames: original tag name → custom display name.
    /// The original tag (as it appears in the `OpenAPI` spec) is the key,
    /// and the desired CLI group name is the value.
    #[serde(default)]
    pub groups: HashMap<String, String>,
    /// Per-operation mappings keyed by the original operationId from the `OpenAPI` spec.
    #[serde(default)]
    pub operations: HashMap<String, OperationMapping>,
}

/// Mapping overrides for a single API operation.
#[derive(Debug, Deserialize, Serialize, Clone, Default, PartialEq, Eq)]
pub struct OperationMapping {
    /// Override the subcommand name (replaces the kebab-cased operationId)
    #[serde(default)]
    pub name: Option<String>,
    /// Override the command group (replaces the tag-based group)
    #[serde(default)]
    pub group: Option<String>,
    /// Additional subcommand aliases
    #[serde(default)]
    pub aliases: Vec<String>,
    /// Whether this command is hidden from help output
    #[serde(default)]
    pub hidden: bool,
}

#[derive(Debug, Deserialize, Serialize, PartialEq, Eq, Clone)]
#[serde(rename_all = "camelCase")]
pub struct ApertureSecret {
    pub source: SecretSource,
    pub name: String,
}

#[derive(Debug, Deserialize, Serialize, PartialEq, Eq, Clone)]
#[serde(rename_all = "camelCase")]
pub enum SecretSource {
    Env,
    // Keychain, // Future option
}
