use crate::cache::models::CachedSpec;
use crate::config::models::{ApiConfig, GlobalConfig};

/// Resolves the base URL for an API based on a priority hierarchy
pub struct BaseUrlResolver<'a> {
    /// The cached API specification
    spec: &'a CachedSpec,
    /// Global configuration containing API overrides
    global_config: Option<&'a GlobalConfig>,
    /// Optional environment override (if None, reads from `APERTURE_ENV` at resolve time)
    environment_override: Option<String>,
}

impl<'a> BaseUrlResolver<'a> {
    /// Creates a new URL resolver for the given spec
    #[must_use]
    pub const fn new(spec: &'a CachedSpec) -> Self {
        Self {
            spec,
            global_config: None,
            environment_override: None,
        }
    }

    /// Sets the global configuration for API-specific overrides
    #[must_use]
    #[allow(clippy::missing_const_for_fn)]
    pub fn with_global_config(mut self, config: &'a GlobalConfig) -> Self {
        self.global_config = Some(config);
        self
    }

    /// Sets the environment explicitly (overrides `APERTURE_ENV`)
    #[must_use]
    pub fn with_environment(mut self, env: Option<String>) -> Self {
        self.environment_override = env;
        self
    }

    /// Resolves the base URL according to the priority hierarchy:
    /// 1. Explicit parameter (for testing)
    /// 2. Per-API config override with environment support
    /// 3. Environment variable: `APERTURE_BASE_URL`
    /// 4. Cached spec default
    /// 5. Fallback: <https://api.example.com>
    #[must_use]
    pub fn resolve(&self, explicit_url: Option<&str>) -> String {
        // Priority 1: Explicit parameter (for testing)
        if let Some(url) = explicit_url {
            return url.to_string();
        }

        // Priority 2: Per-API config override
        if let Some(config) = self.global_config {
            if let Some(api_config) = config.api_configs.get(&self.spec.name) {
                // Check environment-specific URL first
                let env_to_check = self.environment_override.as_ref().map_or_else(
                    || std::env::var("APERTURE_ENV").unwrap_or_default(),
                    std::clone::Clone::clone,
                );

                if !env_to_check.is_empty() {
                    if let Some(env_url) = api_config.environment_urls.get(&env_to_check) {
                        return env_url.clone();
                    }
                }

                // Then check general override
                if let Some(override_url) = &api_config.base_url_override {
                    return override_url.clone();
                }
            }
        }

        // Priority 3: Environment variable
        if let Ok(url) = std::env::var("APERTURE_BASE_URL") {
            return url;
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
    use std::sync::Mutex;

    // Static mutex to ensure only one test can modify environment variables at a time
    static ENV_TEST_MUTEX: Mutex<()> = Mutex::new(());

    fn create_test_spec(name: &str, base_url: Option<&str>) -> CachedSpec {
        CachedSpec {
            cache_format_version: crate::cache::models::CACHE_FORMAT_VERSION,
            name: name.to_string(),
            version: "1.0.0".to_string(),
            commands: vec![],
            base_url: base_url.map(|s| s.to_string()),
            servers: base_url.map(|s| vec![s.to_string()]).unwrap_or_default(),
            security_schemes: HashMap::new(),
            skipped_endpoints: vec![],
        }
    }

    /// Test harness to isolate environment variable changes with mutual exclusion
    fn test_with_env_isolation<F>(test_fn: F)
    where
        F: FnOnce() + std::panic::UnwindSafe,
    {
        // Acquire mutex to prevent parallel env var access
        let _guard = ENV_TEST_MUTEX.lock().unwrap();

        // Store original value
        let original_value = std::env::var("APERTURE_BASE_URL").ok();

        // Clean up first
        std::env::remove_var("APERTURE_BASE_URL");

        // Run the test with panic protection
        let result = std::panic::catch_unwind(test_fn);

        // Always restore original state, even if test panicked
        if let Some(original) = original_value {
            std::env::set_var("APERTURE_BASE_URL", original);
        } else {
            std::env::remove_var("APERTURE_BASE_URL");
        }

        // Drop the guard before re-panicking to release the mutex
        drop(_guard);

        // Re-panic if the test failed
        if let Err(panic_info) = result {
            std::panic::resume_unwind(panic_info);
        }
    }

    #[test]
    fn test_priority_1_explicit_url() {
        test_with_env_isolation(|| {
            let spec = create_test_spec("test-api", Some("https://spec.example.com"));
            let resolver = BaseUrlResolver::new(&spec);

            assert_eq!(
                resolver.resolve(Some("https://explicit.example.com")),
                "https://explicit.example.com"
            );
        });
    }

    #[test]
    fn test_priority_2_api_config_override() {
        test_with_env_isolation(|| {
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
        });
    }

    #[test]
    fn test_priority_2_environment_specific() {
        test_with_env_isolation(|| {
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
        });
    }

    #[test]
    fn test_priority_config_override_beats_env_var() {
        // Test that config override takes precedence over environment variable
        test_with_env_isolation(|| {
            let spec = create_test_spec("test-api", Some("https://spec.example.com"));

            // Set env var
            std::env::set_var("APERTURE_BASE_URL", "https://env.example.com");

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

            // Config override should win over env var
            assert_eq!(resolver.resolve(None), "https://config.example.com");
        });
    }

    #[test]
    fn test_priority_3_env_var() {
        // Use a custom test harness to isolate environment variables
        test_with_env_isolation(|| {
            let spec = create_test_spec("test-api", Some("https://spec.example.com"));

            // Set env var
            std::env::set_var("APERTURE_BASE_URL", "https://env.example.com");

            let resolver = BaseUrlResolver::new(&spec);

            assert_eq!(resolver.resolve(None), "https://env.example.com");
        });
    }

    #[test]
    fn test_priority_4_spec_default() {
        test_with_env_isolation(|| {
            let spec = create_test_spec("test-api", Some("https://spec.example.com"));
            let resolver = BaseUrlResolver::new(&spec);

            assert_eq!(resolver.resolve(None), "https://spec.example.com");
        });
    }

    #[test]
    fn test_priority_5_fallback() {
        test_with_env_isolation(|| {
            let spec = create_test_spec("test-api", None);
            let resolver = BaseUrlResolver::new(&spec);

            assert_eq!(resolver.resolve(None), "https://api.example.com");
        });
    }
}
