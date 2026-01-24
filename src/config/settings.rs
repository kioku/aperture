//! Configuration settings management
//!
//! This module provides type-safe access to global configuration settings,
//! supporting dot-notation keys for nested values and appropriate type validation.

use crate::error::Error;
use serde::{Deserialize, Serialize};
use std::fmt;
use std::str::FromStr;

/// Represents a valid configuration setting key.
///
/// Each variant maps to a specific path in the configuration file,
/// with dot-notation used for nested values (e.g., `agent_defaults.json_errors`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SettingKey {
    /// Default timeout for API requests in seconds (`default_timeout_secs`)
    DefaultTimeoutSecs,
    /// Whether to output errors as JSON by default (`agent_defaults.json_errors`)
    AgentDefaultsJsonErrors,
}

impl SettingKey {
    /// All available setting keys for enumeration.
    pub const ALL: &'static [Self] = &[Self::DefaultTimeoutSecs, Self::AgentDefaultsJsonErrors];

    /// Returns the dot-notation key string for this setting.
    #[must_use]
    pub const fn as_str(&self) -> &'static str {
        match self {
            Self::DefaultTimeoutSecs => "default_timeout_secs",
            Self::AgentDefaultsJsonErrors => "agent_defaults.json_errors",
        }
    }

    /// Returns the expected type name for this setting.
    #[must_use]
    pub const fn type_name(&self) -> &'static str {
        match self {
            Self::DefaultTimeoutSecs => "integer",
            Self::AgentDefaultsJsonErrors => "boolean",
        }
    }

    /// Returns a human-readable description of this setting.
    #[must_use]
    pub const fn description(&self) -> &'static str {
        match self {
            Self::DefaultTimeoutSecs => "Default timeout for API requests in seconds",
            Self::AgentDefaultsJsonErrors => "Output errors as JSON by default",
        }
    }

    /// Returns the default value for this setting as a string.
    #[must_use]
    pub const fn default_value_str(&self) -> &'static str {
        match self {
            Self::DefaultTimeoutSecs => "30",
            Self::AgentDefaultsJsonErrors => "false",
        }
    }

    /// Extracts the current value for this setting from a `GlobalConfig`.
    #[must_use]
    pub const fn value_from_config(&self, config: &super::models::GlobalConfig) -> SettingValue {
        match self {
            Self::DefaultTimeoutSecs => SettingValue::U64(config.default_timeout_secs),
            Self::AgentDefaultsJsonErrors => SettingValue::Bool(config.agent_defaults.json_errors),
        }
    }
}

impl fmt::Display for SettingKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.as_str())
    }
}

impl FromStr for SettingKey {
    type Err = Error;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "default_timeout_secs" => Ok(Self::DefaultTimeoutSecs),
            "agent_defaults.json_errors" => Ok(Self::AgentDefaultsJsonErrors),
            _ => Err(Error::unknown_setting_key(s)),
        }
    }
}

/// Type-safe representation of a configuration setting value.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SettingValue {
    /// Unsigned 64-bit integer value
    U64(u64),
    /// Boolean value
    Bool(bool),
}

/// Maximum allowed timeout value (1 year in seconds).
/// This prevents overflow when converting to i64 and catches obviously wrong values.
const MAX_TIMEOUT_SECS: u64 = 365 * 24 * 60 * 60;

impl SettingValue {
    /// Parse a string value into the appropriate type for the given key.
    ///
    /// # Errors
    ///
    /// Returns an error if the value cannot be parsed as the expected type,
    /// or if the value is outside the allowed range for the setting.
    pub fn parse_for_key(key: SettingKey, value: &str) -> Result<Self, Error> {
        match key {
            SettingKey::DefaultTimeoutSecs => {
                let parsed = value
                    .parse::<u64>()
                    .map_err(|_| Error::invalid_setting_value(key, value))?;

                // Validate range: must be > 0 and <= MAX_TIMEOUT_SECS
                if parsed == 0 {
                    return Err(Error::setting_value_out_of_range(
                        key,
                        value,
                        "timeout must be greater than 0",
                    ));
                }
                if parsed > MAX_TIMEOUT_SECS {
                    return Err(Error::setting_value_out_of_range(
                        key,
                        value,
                        &format!("timeout cannot exceed {MAX_TIMEOUT_SECS} seconds (1 year)"),
                    ));
                }

                Ok(Self::U64(parsed))
            }
            SettingKey::AgentDefaultsJsonErrors => {
                let parsed = match value.to_lowercase().as_str() {
                    "true" | "1" | "yes" | "on" => true,
                    "false" | "0" | "no" | "off" => false,
                    _ => return Err(Error::invalid_setting_value(key, value)),
                };
                Ok(Self::Bool(parsed))
            }
        }
    }

    /// Returns the value as a u64, if it is one.
    #[must_use]
    pub const fn as_u64(&self) -> Option<u64> {
        match self {
            Self::U64(v) => Some(*v),
            Self::Bool(_) => None,
        }
    }

    /// Returns the value as a bool, if it is one.
    #[must_use]
    pub const fn as_bool(&self) -> Option<bool> {
        match self {
            Self::Bool(v) => Some(*v),
            Self::U64(_) => None,
        }
    }
}

impl fmt::Display for SettingValue {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::U64(v) => write!(f, "{v}"),
            Self::Bool(v) => write!(f, "{v}"),
        }
    }
}

/// Information about a configuration setting for display purposes.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SettingInfo {
    /// The setting key in dot-notation
    pub key: String,
    /// Current value as a string
    pub value: String,
    /// Expected type name
    #[serde(rename = "type")]
    pub type_name: String,
    /// Human-readable description
    pub description: String,
    /// Default value as a string
    pub default: String,
}

impl SettingInfo {
    /// Create a new `SettingInfo` from a key and current value.
    #[must_use]
    pub fn new(key: SettingKey, current_value: &SettingValue) -> Self {
        Self {
            key: key.as_str().to_string(),
            value: current_value.to_string(),
            type_name: key.type_name().to_string(),
            description: key.description().to_string(),
            default: key.default_value_str().to_string(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_setting_key_from_str_valid() {
        assert_eq!(
            "default_timeout_secs".parse::<SettingKey>().unwrap(),
            SettingKey::DefaultTimeoutSecs
        );
        assert_eq!(
            "agent_defaults.json_errors".parse::<SettingKey>().unwrap(),
            SettingKey::AgentDefaultsJsonErrors
        );
    }

    #[test]
    fn test_setting_key_from_str_invalid() {
        let result = "unknown_key".parse::<SettingKey>();
        assert!(result.is_err());
    }

    #[test]
    fn test_setting_key_as_str() {
        assert_eq!(
            SettingKey::DefaultTimeoutSecs.as_str(),
            "default_timeout_secs"
        );
        assert_eq!(
            SettingKey::AgentDefaultsJsonErrors.as_str(),
            "agent_defaults.json_errors"
        );
    }

    #[test]
    fn test_setting_value_parse_u64_valid() {
        let value = SettingValue::parse_for_key(SettingKey::DefaultTimeoutSecs, "60").unwrap();
        assert_eq!(value, SettingValue::U64(60));
    }

    #[test]
    fn test_setting_value_parse_u64_invalid() {
        let result = SettingValue::parse_for_key(SettingKey::DefaultTimeoutSecs, "abc");
        assert!(result.is_err());
    }

    #[test]
    fn test_setting_value_parse_bool_valid() {
        let key = SettingKey::AgentDefaultsJsonErrors;

        assert_eq!(
            SettingValue::parse_for_key(key, "true").unwrap(),
            SettingValue::Bool(true)
        );
        assert_eq!(
            SettingValue::parse_for_key(key, "false").unwrap(),
            SettingValue::Bool(false)
        );
        assert_eq!(
            SettingValue::parse_for_key(key, "1").unwrap(),
            SettingValue::Bool(true)
        );
        assert_eq!(
            SettingValue::parse_for_key(key, "0").unwrap(),
            SettingValue::Bool(false)
        );
        assert_eq!(
            SettingValue::parse_for_key(key, "yes").unwrap(),
            SettingValue::Bool(true)
        );
        assert_eq!(
            SettingValue::parse_for_key(key, "no").unwrap(),
            SettingValue::Bool(false)
        );
    }

    #[test]
    fn test_setting_value_parse_bool_invalid() {
        let result = SettingValue::parse_for_key(SettingKey::AgentDefaultsJsonErrors, "maybe");
        assert!(result.is_err());
    }

    #[test]
    fn test_setting_value_display() {
        assert_eq!(SettingValue::U64(30).to_string(), "30");
        assert_eq!(SettingValue::Bool(true).to_string(), "true");
        assert_eq!(SettingValue::Bool(false).to_string(), "false");
    }

    #[test]
    fn test_setting_info_new() {
        let info = SettingInfo::new(SettingKey::DefaultTimeoutSecs, &SettingValue::U64(60));
        assert_eq!(info.key, "default_timeout_secs");
        assert_eq!(info.value, "60");
        assert_eq!(info.type_name, "integer");
        assert_eq!(info.default, "30");
    }

    #[test]
    fn test_setting_value_parse_timeout_zero_rejected() {
        let result = SettingValue::parse_for_key(SettingKey::DefaultTimeoutSecs, "0");
        assert!(result.is_err());
    }

    #[test]
    fn test_setting_value_parse_timeout_max_boundary() {
        // 1 year in seconds should be accepted
        let result = SettingValue::parse_for_key(
            SettingKey::DefaultTimeoutSecs,
            &super::MAX_TIMEOUT_SECS.to_string(),
        );
        assert!(result.is_ok());
    }

    #[test]
    fn test_setting_value_parse_timeout_over_max_rejected() {
        // 1 year + 1 second should be rejected
        let over_max = super::MAX_TIMEOUT_SECS + 1;
        let result =
            SettingValue::parse_for_key(SettingKey::DefaultTimeoutSecs, &over_max.to_string());
        assert!(result.is_err());
    }
}
