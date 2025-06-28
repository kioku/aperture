use serde::{Deserialize, Serialize};

#[derive(Debug, Deserialize, Serialize)]
pub struct GlobalConfig {
    #[serde(default = "default_timeout_secs_value")]
    pub default_timeout_secs: u64,
    #[serde(default)]
    pub agent_defaults: AgentDefaults,
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
        }
    }
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
