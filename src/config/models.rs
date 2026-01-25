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
    /// HTTP status codes that trigger retries (default: [429, 503, 504])
    #[serde(default = "default_retry_status_codes")]
    pub retry_status_codes: Vec<u16>,
}

const fn default_initial_delay_ms() -> u64 {
    500
}

const fn default_max_delay_ms() -> u64 {
    30_000
}

fn default_retry_status_codes() -> Vec<u16> {
    vec![429, 503, 504]
}

impl Default for RetryDefaults {
    fn default() -> Self {
        Self {
            max_attempts: 0, // Disabled by default
            initial_delay_ms: default_initial_delay_ms(),
            max_delay_ms: default_max_delay_ms(),
            retry_status_codes: default_retry_status_codes(),
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
