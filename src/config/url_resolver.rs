use crate::cache::models::CachedSpec;
use crate::config::models::{ApiConfig, GlobalConfig};

/// Resolves the base URL for an API based on a priority hierarchy
pub struct BaseUrlResolver<'a> {
    /// The cached API specification
    spec: &'a CachedSpec,
    /// Global configuration containing API overrides
    global_config: Option<&'a GlobalConfig>,
    /// Current environment (from APERTURE_ENV)
    environment: Option<String>,
}

impl<'a> BaseUrlResolver<'a> {
    /// Creates a new URL resolver for the given spec
    pub fn new(spec: &'a CachedSpec) -> Self {
        Self {
            spec,
            global_config: None,
            environment: std::env::var("APERTURE_ENV").ok(),
        }
    }

    /// Sets the global configuration for API-specific overrides
    #[must_use]
    pub fn with_global_config(mut self, config: &'a GlobalConfig) -> Self {
        self.global_config = Some(config);
        self
    }

    /// Sets the environment explicitly (overrides `APERTURE_ENV`)
    #[must_use]
    pub fn with_environment(mut self, env: Option<String>) -> Self {
        self.environment = env;
        self
    }

    /// Resolves the base URL according to the priority hierarchy:
    /// 1. Explicit parameter (for testing)
    /// 2. Environment variable: `APERTURE_BASE_URL`
    /// 3. Per-API config override with environment support
    /// 4. Cached spec default
    /// 5. Fallback: <https://api.example.com>
    #[must_use]
    pub fn resolve(&self, explicit_url: Option<&str>) -> String {
        // Priority 1: Explicit parameter (for testing)
        if let Some(url) = explicit_url {
            return url.to_string();
        }

        // Priority 2: Environment variable
        if let Ok(url) = std::env::var("APERTURE_BASE_URL") {
            return url;
        }

        // Priority 3: Per-API config override
        if let Some(config) = self.global_config {
            if let Some(api_config) = config.api_configs.get(&self.spec.name) {
                // Check environment-specific URL first
                if let Some(env) = &self.environment {
                    if let Some(env_url) = api_config.environment_urls.get(env) {
                        return env_url.clone();
                    }
                }

                // Then check general override
                if let Some(override_url) = &api_config.base_url_override {
                    return override_url.clone();
                }
            }
        }

        // Priority 4: Cached spec default
        if let Some(base_url) = &self.spec.base_url {
            return base_url.clone();
        }

        // Priority 5: Fallback
        "https://api.example.com".to_string()
    }

    /// Gets the API config if available
    #[must_use]
    pub fn get_api_config(&self) -> Option<&ApiConfig> {
        self.global_config
            .and_then(|config| config.api_configs.get(&self.spec.name))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cache::models::CachedSpec;
    use std::collections::HashMap;

    fn create_test_spec(name: &str, base_url: Option<&str>) -> CachedSpec {
        CachedSpec {
            name: name.to_string(),
            version: "1.0.0".to_string(),
            commands: vec![],
            base_url: base_url.map(|s| s.to_string()),
            servers: base_url.map(|s| vec![s.to_string()]).unwrap_or_default(),
        }
    }

    #[test]
    fn test_priority_1_explicit_url() {
        // Clean env first
        std::env::remove_var("APERTURE_BASE_URL");
        
        let spec = create_test_spec("test-api", Some("https://spec.example.com"));
        let resolver = BaseUrlResolver::new(&spec);

        assert_eq!(
            resolver.resolve(Some("https://explicit.example.com")),
            "https://explicit.example.com"
        );
    }

    #[test]
    fn test_priority_2_env_var() {
        // Clean up first
        std::env::remove_var("APERTURE_BASE_URL");
        
        let spec = create_test_spec("test-api", Some("https://spec.example.com"));
        
        // Set env var before creating resolver
        std::env::set_var("APERTURE_BASE_URL", "https://env.example.com");
        
        let resolver = BaseUrlResolver::new(&spec);

        assert_eq!(resolver.resolve(None), "https://env.example.com");

        // Clean up
        std::env::remove_var("APERTURE_BASE_URL");
    }

    #[test]
    fn test_priority_3_api_config_override() {
        // Clean env first
        std::env::remove_var("APERTURE_BASE_URL");
        
        let spec = create_test_spec("test-api", Some("https://spec.example.com"));

        let mut api_configs = HashMap::new();
        api_configs.insert(
            "test-api".to_string(),
            ApiConfig {
                base_url_override: Some("https://config.example.com".to_string()),
                environment_urls: HashMap::new(),
            },
        );

        let global_config = GlobalConfig {
            api_configs,
            ..Default::default()
        };

        let resolver = BaseUrlResolver::new(&spec).with_global_config(&global_config);

        assert_eq!(resolver.resolve(None), "https://config.example.com");
    }

    #[test]
    fn test_priority_3_environment_specific() {
        // Clean env first
        std::env::remove_var("APERTURE_BASE_URL");
        
        let spec = create_test_spec("test-api", Some("https://spec.example.com"));

        let mut environment_urls = HashMap::new();
        environment_urls.insert(
            "staging".to_string(),
            "https://staging.example.com".to_string(),
        );
        environment_urls.insert("prod".to_string(), "https://prod.example.com".to_string());

        let mut api_configs = HashMap::new();
        api_configs.insert(
            "test-api".to_string(),
            ApiConfig {
                base_url_override: Some("https://config.example.com".to_string()),
                environment_urls,
            },
        );

        let global_config = GlobalConfig {
            api_configs,
            ..Default::default()
        };

        let resolver = BaseUrlResolver::new(&spec)
            .with_global_config(&global_config)
            .with_environment(Some("staging".to_string()));

        assert_eq!(resolver.resolve(None), "https://staging.example.com");
    }

    #[test]
    fn test_priority_4_spec_default() {
        // Clean env first
        std::env::remove_var("APERTURE_BASE_URL");

        let spec = create_test_spec("test-api", Some("https://spec.example.com"));
        let resolver = BaseUrlResolver::new(&spec);

        assert_eq!(resolver.resolve(None), "https://spec.example.com");
    }

    #[test]
    fn test_priority_5_fallback() {
        // Clean env first
        std::env::remove_var("APERTURE_BASE_URL");

        let spec = create_test_spec("test-api", None);
        let resolver = BaseUrlResolver::new(&spec);

        assert_eq!(resolver.resolve(None), "https://api.example.com");
    }
}
