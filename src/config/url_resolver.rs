use crate::cache::models::CachedSpec;
use crate::config::models::{ApiConfig, GlobalConfig};
use crate::config::server_variable_resolver::ServerVariableResolver;
#[allow(unused_imports)]
use crate::error::{Error, ErrorKind};

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
        self.resolve_with_variables(explicit_url, &[])
            .unwrap_or_else(|err| {
                match err {
                    // For validation errors, log and fallback to basic resolution
                    // This maintains backward compatibility while providing visibility
                    Error::Internal {
                        kind: crate::error::ErrorKind::ServerVariable,
                        ..
                    } => {
                        eprintln!(
                            "{} Server variable error: {err}",
                            crate::constants::MSG_WARNING_PREFIX
                        );
                        self.resolve_basic(explicit_url)
                    }
                    // Fallback for all other errors (template resolution, missing variables, etc.)
                    _ => self.resolve_basic(explicit_url),
                }
            })
    }

    /// Resolves the base URL with server variable substitution support
    ///
    /// # Arguments
    /// * `explicit_url` - Explicit URL override (for testing)
    /// * `server_var_args` - Server variable arguments from CLI (e.g., `["region=us", "env=prod"]`)
    ///
    /// # Returns
    /// * `Ok(String)` - Resolved URL with variables substituted
    /// * `Err(Error)` - Server variable validation or substitution errors
    ///
    /// # Errors
    /// Returns errors for:
    /// - Invalid server variable format or values
    /// - Missing required server variables
    /// - URL template substitution failures
    pub fn resolve_with_variables(
        &self,
        explicit_url: Option<&str>,
        server_var_args: &[String],
    ) -> Result<String, Error> {
        // First resolve the base URL using the standard priority hierarchy
        let base_url = self.resolve_basic(explicit_url);

        // If the URL doesn't contain template variables, return as-is
        if !base_url.contains('{') {
            return Ok(base_url);
        }

        // If no server variables are defined in the spec but URL has templates,
        // this indicates a backward compatibility issue - the spec has template
        // URLs but no server variable definitions
        if self.spec.server_variables.is_empty() {
            let template_vars = extract_template_variables(&base_url);

            if let Some(first_var) = template_vars.first() {
                return Err(Error::unresolved_template_variable(first_var, &base_url));
            }

            return Ok(base_url);
        }

        // Resolve server variables and apply template substitution
        let resolver = ServerVariableResolver::new(self.spec);
        let resolved_variables = resolver.resolve_variables(server_var_args)?;
        resolver.substitute_url(&base_url, &resolved_variables)
    }

    /// Basic URL resolution without server variable processing (internal helper)
    fn resolve_basic(&self, explicit_url: Option<&str>) -> String {
        // Priority 1: Explicit parameter (for testing)
        if let Some(url) = explicit_url {
            return url.to_string();
        }

        // Priority 2: Per-API config override
        if let Some(config) = self.global_config {
            if let Some(api_config) = config.api_configs.get(&self.spec.name) {
                // Check environment-specific URL first
                let env_to_check = self.environment_override.as_ref().map_or_else(
                    || std::env::var(crate::constants::ENV_APERTURE_ENV).unwrap_or_default(),
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
        if let Ok(url) = std::env::var(crate::constants::ENV_APERTURE_BASE_URL) {
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

/// Extracts template variable names from a URL string
fn extract_template_variables(url: &str) -> Vec<String> {
    let mut template_vars = Vec::new();
    let mut start = 0;

    while let Some(open) = url[start..].find('{') {
        let open_pos = start + open;
        if let Some(close) = url[open_pos..].find('}') {
            let close_pos = open_pos + close;
            let var_name = &url[open_pos + 1..close_pos];
            if !var_name.is_empty() {
                template_vars.push(var_name.to_string());
            }
            start = close_pos + 1;
        } else {
            break;
        }
    }

    template_vars
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cache::models::{CachedSpec, ServerVariable};
    use crate::error::ErrorKind;
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
            server_variables: HashMap::new(),
        }
    }

    fn create_test_spec_with_variables(name: &str, base_url: Option<&str>) -> CachedSpec {
        let mut server_variables = HashMap::new();

        // Add test server variables
        server_variables.insert(
            "region".to_string(),
            ServerVariable {
                default: Some("us".to_string()),
                enum_values: vec!["us".to_string(), "eu".to_string(), "ap".to_string()],
                description: Some("API region".to_string()),
            },
        );

        server_variables.insert(
            "env".to_string(),
            ServerVariable {
                default: None,
                enum_values: vec!["dev".to_string(), "staging".to_string(), "prod".to_string()],
                description: Some("Environment".to_string()),
            },
        );

        CachedSpec {
            cache_format_version: crate::cache::models::CACHE_FORMAT_VERSION,
            name: name.to_string(),
            version: "1.0.0".to_string(),
            commands: vec![],
            base_url: base_url.map(|s| s.to_string()),
            servers: base_url.map(|s| vec![s.to_string()]).unwrap_or_default(),
            security_schemes: HashMap::new(),
            skipped_endpoints: vec![],
            server_variables,
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
        let original_value = std::env::var(crate::constants::ENV_APERTURE_BASE_URL).ok();

        // Clean up first
        std::env::remove_var(crate::constants::ENV_APERTURE_BASE_URL);

        // Run the test with panic protection
        let result = std::panic::catch_unwind(test_fn);

        // Always restore original state, even if test panicked
        if let Some(original) = original_value {
            std::env::set_var(crate::constants::ENV_APERTURE_BASE_URL, original);
        } else {
            std::env::remove_var(crate::constants::ENV_APERTURE_BASE_URL);
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
                    strict_mode: false,
                    secrets: HashMap::new(),
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
                    strict_mode: false,
                    secrets: HashMap::new(),
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
            std::env::set_var(
                crate::constants::ENV_APERTURE_BASE_URL,
                "https://env.example.com",
            );

            let mut api_configs = HashMap::new();
            api_configs.insert(
                "test-api".to_string(),
                ApiConfig {
                    base_url_override: Some("https://config.example.com".to_string()),
                    environment_urls: HashMap::new(),
                    strict_mode: false,
                    secrets: HashMap::new(),
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
            std::env::set_var(
                crate::constants::ENV_APERTURE_BASE_URL,
                "https://env.example.com",
            );

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

    #[test]
    fn test_server_variable_resolution_with_all_provided() {
        test_with_env_isolation(|| {
            let spec = create_test_spec_with_variables(
                "test-api",
                Some("https://{region}-{env}.api.example.com"),
            );
            let resolver = BaseUrlResolver::new(&spec);

            let server_vars = vec!["region=eu".to_string(), "env=staging".to_string()];
            let result = resolver.resolve_with_variables(None, &server_vars).unwrap();

            assert_eq!(result, "https://eu-staging.api.example.com");
        });
    }

    #[test]
    fn test_server_variable_resolution_with_defaults() {
        test_with_env_isolation(|| {
            let spec = create_test_spec_with_variables(
                "test-api",
                Some("https://{region}-{env}.api.example.com"),
            );
            let resolver = BaseUrlResolver::new(&spec);

            // Only provide required variable, let region use default
            let server_vars = vec!["env=prod".to_string()];
            let result = resolver.resolve_with_variables(None, &server_vars).unwrap();

            assert_eq!(result, "https://us-prod.api.example.com");
        });
    }

    #[test]
    fn test_server_variable_resolution_missing_required() {
        test_with_env_isolation(|| {
            let spec = create_test_spec_with_variables(
                "test-api",
                Some("https://{region}-{env}.api.example.com"),
            );
            let resolver = BaseUrlResolver::new(&spec);

            // Missing required 'env' variable
            let server_vars = vec!["region=us".to_string()];
            let result = resolver.resolve_with_variables(None, &server_vars);

            assert!(result.is_err());
        });
    }

    #[test]
    fn test_server_variable_resolution_invalid_enum() {
        test_with_env_isolation(|| {
            let spec = create_test_spec_with_variables(
                "test-api",
                Some("https://{region}-{env}.api.example.com"),
            );
            let resolver = BaseUrlResolver::new(&spec);

            let server_vars = vec!["region=invalid".to_string(), "env=prod".to_string()];
            let result = resolver.resolve_with_variables(None, &server_vars);

            assert!(result.is_err());
        });
    }

    #[test]
    fn test_non_template_url_with_server_variables() {
        test_with_env_isolation(|| {
            let spec = create_test_spec_with_variables("test-api", Some("https://api.example.com"));
            let resolver = BaseUrlResolver::new(&spec);

            // Non-template URL should be returned as-is even with server variables defined
            let server_vars = vec!["region=eu".to_string(), "env=prod".to_string()];
            let result = resolver.resolve_with_variables(None, &server_vars).unwrap();

            assert_eq!(result, "https://api.example.com");
        });
    }

    #[test]
    fn test_no_server_variables_defined() {
        test_with_env_isolation(|| {
            let spec = create_test_spec("test-api", Some("https://{region}.api.example.com"));
            let resolver = BaseUrlResolver::new(&spec);

            // Template URL but no server variables defined in spec should error
            let server_vars = vec!["region=eu".to_string()];
            let result = resolver.resolve_with_variables(None, &server_vars);

            // Should fail with UnresolvedTemplateVariable error
            assert!(result.is_err());
            match result.unwrap_err() {
                Error::Internal {
                    kind: ErrorKind::ServerVariable,
                    message,
                    ..
                } => {
                    assert!(message.contains("region"));
                }
                _ => panic!("Expected Internal ServerVariable error"),
            }
        });
    }

    #[test]
    fn test_server_variable_fallback_compatibility() {
        test_with_env_isolation(|| {
            let spec = create_test_spec_with_variables(
                "test-api",
                Some("https://{region}-{env}.api.example.com"),
            );
            let resolver = BaseUrlResolver::new(&spec);

            // resolve() method should gracefully fallback when server variables fail
            // This tests backward compatibility - when server variables are missing
            // required values, it should fallback to basic resolution
            let result = resolver.resolve(None);

            // Should return the basic URL resolution (original template URL)
            assert_eq!(result, "https://{region}-{env}.api.example.com");
        });
    }

    #[test]
    fn test_server_variable_with_config_override() {
        test_with_env_isolation(|| {
            let spec =
                create_test_spec_with_variables("test-api", Some("https://{region}.original.com"));

            let mut api_configs = HashMap::new();
            api_configs.insert(
                "test-api".to_string(),
                ApiConfig {
                    base_url_override: Some("https://{region}-override.example.com".to_string()),
                    environment_urls: HashMap::new(),
                    strict_mode: false,
                    secrets: HashMap::new(),
                },
            );

            let global_config = GlobalConfig {
                api_configs,
                ..Default::default()
            };

            let resolver = BaseUrlResolver::new(&spec).with_global_config(&global_config);

            let server_vars = vec!["env=prod".to_string()]; // region should use default 'us'
            let result = resolver.resolve_with_variables(None, &server_vars).unwrap();

            // Should use config override as base, then apply server variable substitution
            assert_eq!(result, "https://us-override.example.com");
        });
    }

    #[test]
    fn test_malformed_templates_pass_through() {
        test_with_env_isolation(|| {
            // Test URLs with empty braces or malformed templates
            let spec = create_test_spec("test-api", Some("https://api.example.com/path{}"));
            let resolver = BaseUrlResolver::new(&spec);

            let result = resolver.resolve_with_variables(None, &[]).unwrap();
            // Empty braces should pass through as they're not valid template variables
            assert_eq!(result, "https://api.example.com/path{}");
        });
    }

    #[test]
    fn test_backward_compatibility_no_server_vars_non_template() {
        test_with_env_isolation(|| {
            // Non-template URL with no server variables should work normally
            let spec = create_test_spec("test-api", Some("https://api.example.com"));
            let resolver = BaseUrlResolver::new(&spec);

            let result = resolver.resolve_with_variables(None, &[]).unwrap();
            assert_eq!(result, "https://api.example.com");
        });
    }
}
