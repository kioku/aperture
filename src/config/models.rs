use serde::{Deserialize, Serialize};
use std::collections::HashMap;

#[derive(Debug, Deserialize, Serialize)]
pub struct GlobalConfig {
    #[serde(default = "default_timeout_secs_value")]
    pub default_timeout_secs: u64,
    #[serde(default)]
    pub agent_defaults: AgentDefaults,
    /// Per-API configuration overrides
    #[serde(default)]
    pub api_configs: HashMap<String, ApiConfig>,
}

const fn default_timeout_secs_value() -> u64 {
    30
}

#[derive(Debug, Deserialize, Serialize, Default)]
pub struct AgentDefaults {
    #[serde(default)]
    pub json_errors: bool,
}

impl Default for GlobalConfig {
    fn default() -> Self {
        Self {
            default_timeout_secs: 30,
            agent_defaults: AgentDefaults::default(),
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
}

#[derive(Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ApertureSecret {
    pub source: SecretSource,
    pub name: String,
}

#[derive(Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum SecretSource {
    Env,
    // Keychain, // Future option
}
