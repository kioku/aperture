use crate::cache::metadata::CacheMetadataManager;
use crate::config::models::{ApertureSecret, ApiConfig, GlobalConfig, SecretSource};
use crate::config::url_resolver::BaseUrlResolver;
use crate::engine::loader;
use crate::error::Error;
use crate::fs::{FileSystem, OsFileSystem};
use crate::spec::{SpecTransformer, SpecValidator};
use openapiv3::{OpenAPI, ReferenceOr};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::process::Command;

/// Struct to hold categorized validation warnings
struct CategorizedWarnings<'a> {
    content_type: Vec<&'a crate::spec::validator::ValidationWarning>,
    auth: Vec<&'a crate::spec::validator::ValidationWarning>,
    mixed_content: Vec<&'a crate::spec::validator::ValidationWarning>,
}

pub struct ConfigManager<F: FileSystem> {
    fs: F,
    config_dir: PathBuf,
}

impl ConfigManager<OsFileSystem> {
    /// Creates a new `ConfigManager` with the default filesystem and config directory.
    ///
    /// # Errors
    ///
    /// Returns an error if the home directory cannot be determined.
    pub fn new() -> Result<Self, Error> {
        let config_dir = get_config_dir()?;
        Ok(Self {
            fs: OsFileSystem,
            config_dir,
        })
    }
}

impl<F: FileSystem> ConfigManager<F> {
    pub const fn with_fs(fs: F, config_dir: PathBuf) -> Self {
        Self { fs, config_dir }
    }

    /// Get the configuration directory path
    pub fn config_dir(&self) -> &Path {
        &self.config_dir
    }

    /// Convert skipped endpoints to validation warnings for display
    #[must_use]
    pub fn skipped_endpoints_to_warnings(
        skipped_endpoints: &[crate::cache::models::SkippedEndpoint],
    ) -> Vec<crate::spec::validator::ValidationWarning> {
        skipped_endpoints
            .iter()
            .map(|endpoint| crate::spec::validator::ValidationWarning {
                endpoint: crate::spec::validator::UnsupportedEndpoint {
                    path: endpoint.path.clone(),
                    method: endpoint.method.clone(),
                    content_type: endpoint.content_type.clone(),
                },
                reason: endpoint.reason.clone(),
            })
            .collect()
    }

    /// Save the strict mode preference for an API
    fn save_strict_preference(&self, api_name: &str, strict: bool) -> Result<(), Error> {
        let mut config = self.load_global_config()?;
        let api_config = config
            .api_configs
            .entry(api_name.to_string())
            .or_insert_with(|| ApiConfig {
                base_url_override: None,
                environment_urls: HashMap::new(),
                strict_mode: false,
                secrets: HashMap::new(),
            });
        api_config.strict_mode = strict;
        self.save_global_config(&config)?;
        Ok(())
    }

    /// Get the strict mode preference for an API
    ///
    /// # Errors
    ///
    /// Returns an error if the global config cannot be loaded
    pub fn get_strict_preference(&self, api_name: &str) -> Result<bool, Error> {
        let config = self.load_global_config()?;
        Ok(config
            .api_configs
            .get(api_name)
            .is_some_and(|c| c.strict_mode))
    }

    /// Count total operations in an `OpenAPI` spec
    fn count_total_operations(spec: &OpenAPI) -> usize {
        spec.paths
            .iter()
            .filter_map(|(_, path_item)| match path_item {
                ReferenceOr::Item(item) => Some(item),
                ReferenceOr::Reference { .. } => None,
            })
            .map(|item| {
                let mut count = 0;
                if item.get.is_some() {
                    count += 1;
                }
                if item.post.is_some() {
                    count += 1;
                }
                if item.put.is_some() {
                    count += 1;
                }
                if item.delete.is_some() {
                    count += 1;
                }
                if item.patch.is_some() {
                    count += 1;
                }
                if item.head.is_some() {
                    count += 1;
                }
                if item.options.is_some() {
                    count += 1;
                }
                if item.trace.is_some() {
                    count += 1;
                }
                count
            })
            .sum()
    }

    /// Display validation warnings with custom prefix
    #[must_use]
    pub fn format_validation_warnings(
        warnings: &[crate::spec::validator::ValidationWarning],
        total_operations: Option<usize>,
        indent: &str,
    ) -> Vec<String> {
        let mut lines = Vec::new();

        if !warnings.is_empty() {
            let categorized_warnings = Self::categorize_warnings(warnings);
            let total_skipped =
                categorized_warnings.content_type.len() + categorized_warnings.auth.len();

            Self::format_content_type_warnings(
                &mut lines,
                &categorized_warnings.content_type,
                total_operations,
                total_skipped,
                indent,
            );
            Self::format_auth_warnings(
                &mut lines,
                &categorized_warnings.auth,
                total_operations,
                total_skipped,
                indent,
                !categorized_warnings.content_type.is_empty(),
            );
            Self::format_mixed_content_warnings(
                &mut lines,
                &categorized_warnings.mixed_content,
                indent,
                !categorized_warnings.content_type.is_empty()
                    || !categorized_warnings.auth.is_empty(),
            );
        }

        lines
    }

    /// Display validation warnings to stderr
    pub fn display_validation_warnings(
        warnings: &[crate::spec::validator::ValidationWarning],
        total_operations: Option<usize>,
    ) {
        if !warnings.is_empty() {
            let lines = Self::format_validation_warnings(warnings, total_operations, "");
            for line in lines {
                if line.is_empty() {
                    eprintln!();
                } else if line.starts_with("Skipping") || line.starts_with("Endpoints") {
                    eprintln!("Warning: {line}");
                } else {
                    eprintln!("{line}");
                }
            }
            eprintln!("\nUse --strict to reject specs with unsupported features.");
        }
    }

    /// Adds a new `OpenAPI` specification to the configuration from a local file.
    ///
    /// # Errors
    ///
    /// Returns an error if:
    /// - The spec already exists and `force` is false
    /// - File I/O operations fail
    /// - The `OpenAPI` spec is invalid YAML
    /// - The spec contains unsupported features
    ///
    /// # Panics
    ///
    /// Panics if the spec path parent directory is None (should not happen in normal usage).
    pub fn add_spec(
        &self,
        name: &str,
        file_path: &Path,
        force: bool,
        strict: bool,
    ) -> Result<(), Error> {
        self.check_spec_exists(name, force)?;

        let content = self.fs.read_to_string(file_path)?;
        let openapi_spec: OpenAPI = serde_yaml::from_str(&content)?;

        // Validate against Aperture's supported feature set using SpecValidator
        let validator = SpecValidator::new();
        let validation_result = validator.validate_with_mode(&openapi_spec, strict);

        // Check for errors first
        if !validation_result.is_valid() {
            return validation_result.into_result();
        }

        // Count total operations for better UX
        let total_operations = Self::count_total_operations(&openapi_spec);

        // Display warnings if any
        Self::display_validation_warnings(&validation_result.warnings, Some(total_operations));

        self.add_spec_from_validated_openapi(
            name,
            &openapi_spec,
            &content,
            &validation_result,
            strict,
        )
    }

    /// Adds a new `OpenAPI` specification to the configuration from a URL.
    ///
    /// # Errors
    ///
    /// Returns an error if:
    /// - The spec already exists and `force` is false
    /// - Network requests fail
    /// - The `OpenAPI` spec is invalid YAML
    /// - The spec contains unsupported features
    /// - Response size exceeds 10MB limit
    /// - Request times out (30 seconds)
    ///
    /// # Panics
    ///
    /// Panics if the spec path parent directory is None (should not happen in normal usage).
    #[allow(clippy::future_not_send)]
    pub async fn add_spec_from_url(
        &self,
        name: &str,
        url: &str,
        force: bool,
        strict: bool,
    ) -> Result<(), Error> {
        self.check_spec_exists(name, force)?;

        // Fetch content from URL
        let content = fetch_spec_from_url(url).await?;
        let openapi_spec: OpenAPI = serde_yaml::from_str(&content)?;

        // Validate against Aperture's supported feature set using SpecValidator
        let validator = SpecValidator::new();
        let validation_result = validator.validate_with_mode(&openapi_spec, strict);

        // Check for errors first
        if !validation_result.is_valid() {
            return validation_result.into_result();
        }

        // Count total operations for better UX
        let total_operations = Self::count_total_operations(&openapi_spec);

        // Display warnings if any
        Self::display_validation_warnings(&validation_result.warnings, Some(total_operations));

        self.add_spec_from_validated_openapi(
            name,
            &openapi_spec,
            &content,
            &validation_result,
            strict,
        )
    }

    /// Adds a new `OpenAPI` specification from either a file path or URL.
    ///
    /// This is a convenience method that automatically detects whether the input
    /// is a URL or file path and calls the appropriate method.
    ///
    /// # Errors
    ///
    /// Returns an error if:
    /// - The spec already exists and `force` is false
    /// - File I/O operations fail (for local files)
    /// - Network requests fail (for URLs)
    /// - The `OpenAPI` spec is invalid YAML
    /// - The spec contains unsupported features
    /// - Response size exceeds 10MB limit (for URLs)
    /// - Request times out (for URLs)
    #[allow(clippy::future_not_send)]
    pub async fn add_spec_auto(
        &self,
        name: &str,
        file_or_url: &str,
        force: bool,
        strict: bool,
    ) -> Result<(), Error> {
        if is_url(file_or_url) {
            self.add_spec_from_url(name, file_or_url, force, strict)
                .await
        } else {
            // Convert file path string to Path and call sync method
            let path = std::path::Path::new(file_or_url);
            self.add_spec(name, path, force, strict)
        }
    }

    /// Lists all registered API contexts.
    ///
    /// # Errors
    ///
    /// Returns an error if the specs directory cannot be read.
    pub fn list_specs(&self) -> Result<Vec<String>, Error> {
        let specs_dir = self.config_dir.join("specs");
        if !self.fs.exists(&specs_dir) {
            return Ok(Vec::new());
        }

        let mut specs = Vec::new();
        for entry in self.fs.read_dir(&specs_dir)? {
            if self.fs.is_file(&entry) {
                if let Some(file_name) = entry.file_name().and_then(|s| s.to_str()) {
                    if std::path::Path::new(file_name)
                        .extension()
                        .is_some_and(|ext| ext.eq_ignore_ascii_case("yaml"))
                    {
                        specs.push(file_name.trim_end_matches(".yaml").to_string());
                    }
                }
            }
        }
        Ok(specs)
    }

    /// Removes an API specification from the configuration.
    ///
    /// # Errors
    ///
    /// Returns an error if the spec does not exist or cannot be removed.
    pub fn remove_spec(&self, name: &str) -> Result<(), Error> {
        let spec_path = self.config_dir.join("specs").join(format!("{name}.yaml"));
        let cache_path = self.config_dir.join(".cache").join(format!("{name}.bin"));

        if !self.fs.exists(&spec_path) {
            return Err(Error::SpecNotFound {
                name: name.to_string(),
            });
        }

        self.fs.remove_file(&spec_path)?;
        if self.fs.exists(&cache_path) {
            self.fs.remove_file(&cache_path)?;
        }

        // Remove from cache metadata
        let cache_dir = self.config_dir.join(".cache");
        let metadata_manager = CacheMetadataManager::new(&self.fs);
        // Ignore errors if metadata removal fails - the important files are already removed
        let _ = metadata_manager.remove_spec_metadata(&cache_dir, name);

        Ok(())
    }

    /// Opens an API specification in the default editor.
    ///
    /// # Errors
    ///
    /// Returns an error if:
    /// - The spec does not exist.
    /// - The `$EDITOR` environment variable is not set.
    /// - The editor command fails to execute.
    pub fn edit_spec(&self, name: &str) -> Result<(), Error> {
        let spec_path = self.config_dir.join("specs").join(format!("{name}.yaml"));

        if !self.fs.exists(&spec_path) {
            return Err(Error::SpecNotFound {
                name: name.to_string(),
            });
        }

        let editor = std::env::var("EDITOR").map_err(|_| Error::EditorNotSet)?;

        Command::new(editor)
            .arg(&spec_path)
            .status()
            .map_err(Error::Io)?
            .success()
            .then_some(()) // Convert bool to Option<()>
            .ok_or_else(|| Error::EditorFailed {
                name: name.to_string(),
            })
    }

    /// Loads the global configuration from `config.toml`.
    ///
    /// # Errors
    ///
    /// Returns an error if the configuration file exists but cannot be read or parsed.
    pub fn load_global_config(&self) -> Result<GlobalConfig, Error> {
        let config_path = self.config_dir.join("config.toml");
        if self.fs.exists(&config_path) {
            let content = self.fs.read_to_string(&config_path)?;
            toml::from_str(&content).map_err(|e| Error::InvalidConfig {
                reason: e.to_string(),
            })
        } else {
            Ok(GlobalConfig::default())
        }
    }

    /// Saves the global configuration to `config.toml`.
    ///
    /// # Errors
    ///
    /// Returns an error if the configuration cannot be serialized or written.
    pub fn save_global_config(&self, config: &GlobalConfig) -> Result<(), Error> {
        let config_path = self.config_dir.join("config.toml");

        // Ensure config directory exists
        self.fs.create_dir_all(&self.config_dir)?;

        let content = toml::to_string_pretty(config).map_err(|e| Error::SerializationError {
            reason: format!("Failed to serialize config: {e}"),
        })?;

        self.fs.write_all(&config_path, content.as_bytes())?;
        Ok(())
    }

    /// Sets the base URL for an API specification.
    ///
    /// # Arguments
    /// * `api_name` - The name of the API specification
    /// * `url` - The base URL to set
    /// * `environment` - Optional environment name for environment-specific URLs
    ///
    /// # Errors
    ///
    /// Returns an error if the spec doesn't exist or config cannot be saved.
    pub fn set_url(
        &self,
        api_name: &str,
        url: &str,
        environment: Option<&str>,
    ) -> Result<(), Error> {
        // Verify the spec exists
        let spec_path = self
            .config_dir
            .join("specs")
            .join(format!("{api_name}.yaml"));
        if !self.fs.exists(&spec_path) {
            return Err(Error::SpecNotFound {
                name: api_name.to_string(),
            });
        }

        // Load current config
        let mut config = self.load_global_config()?;

        // Get or create API config
        let api_config = config
            .api_configs
            .entry(api_name.to_string())
            .or_insert_with(|| ApiConfig {
                base_url_override: None,
                environment_urls: HashMap::new(),
                strict_mode: false,
                secrets: HashMap::new(),
            });

        // Set the URL
        if let Some(env) = environment {
            api_config
                .environment_urls
                .insert(env.to_string(), url.to_string());
        } else {
            api_config.base_url_override = Some(url.to_string());
        }

        // Save updated config
        self.save_global_config(&config)?;
        Ok(())
    }

    /// Gets the base URL configuration for an API specification.
    ///
    /// # Arguments
    /// * `api_name` - The name of the API specification
    ///
    /// # Returns
    /// A tuple of (`base_url_override`, `environment_urls`, `resolved_url`)
    ///
    /// # Errors
    ///
    /// Returns an error if the spec doesn't exist.
    #[allow(clippy::type_complexity)]
    pub fn get_url(
        &self,
        api_name: &str,
    ) -> Result<(Option<String>, HashMap<String, String>, String), Error> {
        // Verify the spec exists
        let spec_path = self
            .config_dir
            .join("specs")
            .join(format!("{api_name}.yaml"));
        if !self.fs.exists(&spec_path) {
            return Err(Error::SpecNotFound {
                name: api_name.to_string(),
            });
        }

        // Load the cached spec to get its base URL
        let cache_dir = self.config_dir.join(".cache");
        let cached_spec = loader::load_cached_spec(&cache_dir, api_name).ok();

        // Load global config
        let config = self.load_global_config()?;

        // Get API config
        let api_config = config.api_configs.get(api_name);

        let base_url_override = api_config.and_then(|c| c.base_url_override.clone());
        let environment_urls = api_config
            .map(|c| c.environment_urls.clone())
            .unwrap_or_default();

        // Resolve the URL that would actually be used
        let resolved_url = cached_spec.map_or_else(
            || "https://api.example.com".to_string(),
            |spec| {
                let resolver = BaseUrlResolver::new(&spec);
                let resolver = if api_config.is_some() {
                    resolver.with_global_config(&config)
                } else {
                    resolver
                };
                resolver.resolve(None)
            },
        );

        Ok((base_url_override, environment_urls, resolved_url))
    }

    /// Lists all configured base URLs across all API specifications.
    ///
    /// # Returns
    /// A map of API names to their URL configurations
    ///
    /// # Errors
    ///
    /// Returns an error if the config cannot be loaded.
    #[allow(clippy::type_complexity)]
    pub fn list_urls(
        &self,
    ) -> Result<HashMap<String, (Option<String>, HashMap<String, String>)>, Error> {
        let config = self.load_global_config()?;

        let mut result = HashMap::new();
        for (api_name, api_config) in config.api_configs {
            result.insert(
                api_name,
                (api_config.base_url_override, api_config.environment_urls),
            );
        }

        Ok(result)
    }

    /// Test-only method to add spec from URL with custom timeout
    #[doc(hidden)]
    #[allow(clippy::future_not_send)]
    pub async fn add_spec_from_url_with_timeout(
        &self,
        name: &str,
        url: &str,
        force: bool,
        timeout: std::time::Duration,
    ) -> Result<(), Error> {
        // Default to non-strict mode to match CLI behavior
        self.add_spec_from_url_with_timeout_and_mode(name, url, force, timeout, false)
            .await
    }

    /// Test-only method to add spec from URL with custom timeout and validation mode
    #[doc(hidden)]
    #[allow(clippy::future_not_send)]
    async fn add_spec_from_url_with_timeout_and_mode(
        &self,
        name: &str,
        url: &str,
        force: bool,
        timeout: std::time::Duration,
        strict: bool,
    ) -> Result<(), Error> {
        self.check_spec_exists(name, force)?;

        // Fetch content from URL with custom timeout
        let content = fetch_spec_from_url_with_timeout(url, timeout).await?;
        let openapi_spec: OpenAPI = serde_yaml::from_str(&content)?;

        // Validate against Aperture's supported feature set using SpecValidator
        let validator = SpecValidator::new();
        let validation_result = validator.validate_with_mode(&openapi_spec, strict);

        // Check for errors first
        if !validation_result.is_valid() {
            return validation_result.into_result();
        }

        // Note: Not displaying warnings in test method to avoid polluting test output

        self.add_spec_from_validated_openapi(
            name,
            &openapi_spec,
            &content,
            &validation_result,
            strict,
        )
    }

    /// Sets a secret configuration for a specific security scheme
    ///
    /// # Arguments
    /// * `api_name` - The name of the API specification
    /// * `scheme_name` - The name of the security scheme
    /// * `env_var_name` - The environment variable name containing the secret
    ///
    /// # Errors
    ///
    /// Returns an error if the spec doesn't exist or config cannot be saved.
    pub fn set_secret(
        &self,
        api_name: &str,
        scheme_name: &str,
        env_var_name: &str,
    ) -> Result<(), Error> {
        // Verify the spec exists
        let spec_path = self
            .config_dir
            .join("specs")
            .join(format!("{api_name}.yaml"));
        if !self.fs.exists(&spec_path) {
            return Err(Error::SpecNotFound {
                name: api_name.to_string(),
            });
        }

        // Load current config
        let mut config = self.load_global_config()?;

        // Get or create API config
        let api_config = config
            .api_configs
            .entry(api_name.to_string())
            .or_insert_with(|| ApiConfig {
                base_url_override: None,
                environment_urls: HashMap::new(),
                strict_mode: false,
                secrets: HashMap::new(),
            });

        // Set the secret
        api_config.secrets.insert(
            scheme_name.to_string(),
            ApertureSecret {
                source: SecretSource::Env,
                name: env_var_name.to_string(),
            },
        );

        // Save updated config
        self.save_global_config(&config)?;
        Ok(())
    }

    /// Lists configured secrets for an API specification
    ///
    /// # Arguments
    /// * `api_name` - The name of the API specification
    ///
    /// # Returns
    /// A map of scheme names to their secret configurations
    ///
    /// # Errors
    ///
    /// Returns an error if the spec doesn't exist.
    pub fn list_secrets(&self, api_name: &str) -> Result<HashMap<String, ApertureSecret>, Error> {
        // Verify the spec exists
        let spec_path = self
            .config_dir
            .join("specs")
            .join(format!("{api_name}.yaml"));
        if !self.fs.exists(&spec_path) {
            return Err(Error::SpecNotFound {
                name: api_name.to_string(),
            });
        }

        // Load global config
        let config = self.load_global_config()?;

        // Get API config secrets
        let secrets = config
            .api_configs
            .get(api_name)
            .map(|c| c.secrets.clone())
            .unwrap_or_default();

        Ok(secrets)
    }

    /// Gets a secret configuration for a specific security scheme
    ///
    /// # Arguments
    /// * `api_name` - The name of the API specification
    /// * `scheme_name` - The name of the security scheme
    ///
    /// # Returns
    /// The secret configuration if found
    ///
    /// # Errors
    ///
    /// Returns an error if the spec doesn't exist.
    pub fn get_secret(
        &self,
        api_name: &str,
        scheme_name: &str,
    ) -> Result<Option<ApertureSecret>, Error> {
        let secrets = self.list_secrets(api_name)?;
        Ok(secrets.get(scheme_name).cloned())
    }

    /// Removes a specific secret configuration for a security scheme
    ///
    /// # Arguments
    /// * `api_name` - The name of the API specification
    /// * `scheme_name` - The name of the security scheme to remove
    ///
    /// # Errors
    /// Returns an error if the spec doesn't exist or if the scheme is not configured
    pub fn remove_secret(&self, api_name: &str, scheme_name: &str) -> Result<(), Error> {
        // Verify the spec exists
        let spec_path = self
            .config_dir
            .join("specs")
            .join(format!("{api_name}.yaml"));
        if !self.fs.exists(&spec_path) {
            return Err(Error::SpecNotFound {
                name: api_name.to_string(),
            });
        }

        // Load global config
        let mut config = self.load_global_config()?;

        // Check if the API has any configured secrets
        let Some(api_config) = config.api_configs.get_mut(api_name) else {
            return Err(Error::InvalidConfig {
                reason: format!("No secrets configured for API '{api_name}'"),
            });
        };

        // Check if the API config exists but has no secrets
        if api_config.secrets.is_empty() {
            return Err(Error::InvalidConfig {
                reason: format!("No secrets configured for API '{api_name}'"),
            });
        }

        // Check if the specific scheme exists
        if !api_config.secrets.contains_key(scheme_name) {
            return Err(Error::InvalidConfig {
                reason: format!(
                    "Secret for scheme '{scheme_name}' is not configured for API '{api_name}'"
                ),
            });
        }

        // Remove the secret
        api_config.secrets.remove(scheme_name);

        // If no secrets remain, remove the entire API config
        if api_config.secrets.is_empty() && api_config.base_url_override.is_none() {
            config.api_configs.remove(api_name);
        }

        // Save the updated config
        self.save_global_config(&config)?;

        Ok(())
    }

    /// Removes all secret configurations for an API specification
    ///
    /// # Arguments
    /// * `api_name` - The name of the API specification
    ///
    /// # Errors
    /// Returns an error if the spec doesn't exist
    pub fn clear_secrets(&self, api_name: &str) -> Result<(), Error> {
        // Verify the spec exists
        let spec_path = self
            .config_dir
            .join("specs")
            .join(format!("{api_name}.yaml"));
        if !self.fs.exists(&spec_path) {
            return Err(Error::SpecNotFound {
                name: api_name.to_string(),
            });
        }

        // Load global config
        let mut config = self.load_global_config()?;

        // Check if the API exists in config
        if let Some(api_config) = config.api_configs.get_mut(api_name) {
            // Clear all secrets
            api_config.secrets.clear();

            // If no other configuration remains, remove the entire API config
            if api_config.base_url_override.is_none() {
                config.api_configs.remove(api_name);
            }

            // Save the updated config
            self.save_global_config(&config)?;
        }
        // If API config doesn't exist, that's fine - no secrets to clear

        Ok(())
    }

    /// Configure secrets interactively for an API specification
    ///
    /// Loads the cached spec to discover available security schemes and
    /// presents an interactive menu for configuration.
    ///
    /// # Arguments
    /// * `api_name` - The name of the API specification
    ///
    /// # Errors
    ///
    /// Returns an error if:
    /// - The spec doesn't exist
    /// - Cannot load cached spec
    /// - User interaction fails
    /// - Cannot save configuration
    ///
    /// # Panics
    ///
    /// Panics if the selected scheme is not found in the cached spec
    /// (this should never happen due to menu validation)
    pub fn set_secret_interactive(&self, api_name: &str) -> Result<(), Error> {
        // Verify the spec exists and load cached spec
        let (cached_spec, current_secrets) = self.load_spec_for_interactive_config(api_name)?;

        if cached_spec.security_schemes.is_empty() {
            println!("No security schemes found in API '{api_name}'.");
            return Ok(());
        }

        Self::display_interactive_header(api_name, &cached_spec);

        // Create options for selection with rich descriptions
        let options = Self::build_security_scheme_options(&cached_spec, &current_secrets);

        // Interactive loop for configuration
        self.run_interactive_configuration_loop(
            api_name,
            &cached_spec,
            &current_secrets,
            &options,
        )?;

        println!("\n🎉 Interactive configuration complete!");
        Ok(())
    }

    /// Checks if a spec already exists and handles force flag
    ///
    /// # Errors
    ///
    /// Returns an error if the spec already exists and force is false
    fn check_spec_exists(&self, name: &str, force: bool) -> Result<(), Error> {
        let spec_path = self.config_dir.join("specs").join(format!("{name}.yaml"));

        if self.fs.exists(&spec_path) && !force {
            return Err(Error::SpecAlreadyExists {
                name: name.to_string(),
            });
        }

        Ok(())
    }

    /// Transforms an `OpenAPI` spec into cached representation
    ///
    /// # Errors
    ///
    /// Returns an error if transformation fails
    fn transform_spec_to_cached(
        name: &str,
        openapi_spec: &OpenAPI,
        validation_result: &crate::spec::validator::ValidationResult,
    ) -> Result<crate::cache::models::CachedSpec, Error> {
        let transformer = SpecTransformer::new();

        // Convert warnings to skip_endpoints format - skip endpoints with unsupported content types or auth
        let skip_endpoints: Vec<(String, String)> = validation_result
            .warnings
            .iter()
            .filter_map(super::super::spec::validator::ValidationWarning::to_skip_endpoint)
            .collect();

        transformer.transform_with_warnings(
            name,
            openapi_spec,
            &skip_endpoints,
            &validation_result.warnings,
        )
    }

    /// Creates necessary directories for spec and cache files
    ///
    /// # Errors
    ///
    /// Returns an error if directory creation fails
    fn create_spec_directories(&self, name: &str) -> Result<(PathBuf, PathBuf), Error> {
        let spec_path = self.config_dir.join("specs").join(format!("{name}.yaml"));
        let cache_path = self.config_dir.join(".cache").join(format!("{name}.bin"));

        let spec_parent = spec_path.parent().ok_or_else(|| Error::InvalidPath {
            path: spec_path.display().to_string(),
            reason: "Path has no parent directory".to_string(),
        })?;
        let cache_parent = cache_path.parent().ok_or_else(|| Error::InvalidPath {
            path: cache_path.display().to_string(),
            reason: "Path has no parent directory".to_string(),
        })?;

        self.fs.create_dir_all(spec_parent)?;
        self.fs.create_dir_all(cache_parent)?;

        Ok((spec_path, cache_path))
    }

    /// Writes spec and cache files to disk
    ///
    /// # Errors
    ///
    /// Returns an error if file operations fail
    fn write_spec_files(
        &self,
        name: &str,
        content: &str,
        cached_spec: &crate::cache::models::CachedSpec,
        spec_path: &Path,
        cache_path: &Path,
    ) -> Result<(), Error> {
        // Write original spec file
        self.fs.write_all(spec_path, content.as_bytes())?;

        // Serialize and write cached representation
        let cached_data =
            bincode::serialize(cached_spec).map_err(|e| Error::SerializationError {
                reason: e.to_string(),
            })?;
        self.fs.write_all(cache_path, &cached_data)?;

        // Update cache metadata for optimized version checking
        let cache_dir = self.config_dir.join(".cache");
        let metadata_manager = CacheMetadataManager::new(&self.fs);
        metadata_manager.update_spec_metadata(&cache_dir, name, cached_data.len() as u64)?;

        Ok(())
    }

    /// Common logic for adding a spec from a validated `OpenAPI` object
    ///
    /// # Errors
    ///
    /// Returns an error if transformation or file operations fail
    fn add_spec_from_validated_openapi(
        &self,
        name: &str,
        openapi_spec: &OpenAPI,
        content: &str,
        validation_result: &crate::spec::validator::ValidationResult,
        strict: bool,
    ) -> Result<(), Error> {
        // Transform to cached representation
        let cached_spec = Self::transform_spec_to_cached(name, openapi_spec, validation_result)?;

        // Create directories
        let (spec_path, cache_path) = self.create_spec_directories(name)?;

        // Write files
        self.write_spec_files(name, content, &cached_spec, &spec_path, &cache_path)?;

        // Save strict mode preference
        self.save_strict_preference(name, strict)?;

        Ok(())
    }

    /// Loads spec and current secrets for interactive configuration
    ///
    /// # Errors
    ///
    /// Returns an error if the spec doesn't exist or cannot be loaded
    fn load_spec_for_interactive_config(
        &self,
        api_name: &str,
    ) -> Result<
        (
            crate::cache::models::CachedSpec,
            std::collections::HashMap<String, ApertureSecret>,
        ),
        Error,
    > {
        // Verify the spec exists
        let spec_path = self
            .config_dir
            .join("specs")
            .join(format!("{api_name}.yaml"));
        if !self.fs.exists(&spec_path) {
            return Err(Error::SpecNotFound {
                name: api_name.to_string(),
            });
        }

        // Load cached spec to get security schemes
        let cache_dir = self.config_dir.join(".cache");
        let cached_spec = loader::load_cached_spec(&cache_dir, api_name)?;

        // Get current configuration
        let current_secrets = self.list_secrets(api_name)?;

        Ok((cached_spec, current_secrets))
    }

    /// Displays the interactive configuration header
    fn display_interactive_header(api_name: &str, cached_spec: &crate::cache::models::CachedSpec) {
        println!("🔐 Interactive Secret Configuration for API: {api_name}");
        println!(
            "Found {} security scheme(s):\n",
            cached_spec.security_schemes.len()
        );
    }

    /// Builds options for security scheme selection with rich descriptions
    fn build_security_scheme_options(
        cached_spec: &crate::cache::models::CachedSpec,
        current_secrets: &std::collections::HashMap<String, ApertureSecret>,
    ) -> Vec<(String, String)> {
        cached_spec
            .security_schemes
            .values()
            .map(|scheme| {
                let mut description = format!("{} ({})", scheme.scheme_type, scheme.name);

                // Add type-specific details
                match scheme.scheme_type.as_str() {
                    "apiKey" => {
                        if let (Some(location), Some(param)) =
                            (&scheme.location, &scheme.parameter_name)
                        {
                            description = format!("{description} - {location} parameter: {param}");
                        }
                    }
                    "http" => {
                        if let Some(http_scheme) = &scheme.scheme {
                            description = format!("{description} - {http_scheme} authentication");
                        }
                    }
                    _ => {}
                }

                // Show current configuration status
                if current_secrets.contains_key(&scheme.name) {
                    description = format!("{description} [CONFIGURED]");
                } else if scheme.aperture_secret.is_some() {
                    description = format!("{description} [x-aperture-secret]");
                } else {
                    description = format!("{description} [NOT CONFIGURED]");
                }

                // Add OpenAPI description if available
                if let Some(openapi_desc) = &scheme.description {
                    description = format!("{description} - {openapi_desc}");
                }

                (scheme.name.clone(), description)
            })
            .collect()
    }

    /// Runs the interactive configuration loop
    ///
    /// # Errors
    ///
    /// Returns an error if user interaction fails or configuration cannot be saved
    fn run_interactive_configuration_loop(
        &self,
        api_name: &str,
        cached_spec: &crate::cache::models::CachedSpec,
        current_secrets: &std::collections::HashMap<String, ApertureSecret>,
        options: &[(String, String)],
    ) -> Result<(), Error> {
        use crate::interactive::{confirm, prompt_for_input, select_from_options};

        loop {
            let selected_scheme =
                select_from_options("\nSelect a security scheme to configure:", options)?;

            let scheme = cached_spec.security_schemes.get(&selected_scheme).expect(
                "Selected scheme should exist in cached spec - menu validation ensures this",
            );

            Self::display_scheme_configuration_details(&selected_scheme, scheme, current_secrets);

            // Prompt for environment variable
            let env_var = prompt_for_input(&format!(
                "\nEnter environment variable name for '{selected_scheme}' (or press Enter to skip): "
            ))?;

            if env_var.is_empty() {
                println!("Skipping configuration for '{selected_scheme}'");
            } else {
                self.handle_secret_configuration(api_name, &selected_scheme, &env_var)?;
            }

            // Ask if user wants to configure another scheme
            if !confirm("\nConfigure another security scheme?")? {
                break;
            }
        }

        Ok(())
    }

    /// Displays configuration details for a selected security scheme
    fn display_scheme_configuration_details(
        selected_scheme: &str,
        scheme: &crate::cache::models::CachedSecurityScheme,
        current_secrets: &std::collections::HashMap<String, ApertureSecret>,
    ) {
        println!("\n📋 Configuration for '{selected_scheme}':");
        println!("   Type: {}", scheme.scheme_type);
        if let Some(desc) = &scheme.description {
            println!("   Description: {desc}");
        }

        // Show current configuration
        if let Some(current_secret) = current_secrets.get(selected_scheme) {
            println!("   Current: environment variable '{}'", current_secret.name);
        } else if let Some(aperture_secret) = &scheme.aperture_secret {
            println!(
                "   Current: x-aperture-secret -> '{}'",
                aperture_secret.name
            );
        } else {
            println!("   Current: not configured");
        }
    }

    /// Handles secret configuration validation and saving
    ///
    /// # Errors
    ///
    /// Returns an error if validation fails or configuration cannot be saved
    fn handle_secret_configuration(
        &self,
        api_name: &str,
        selected_scheme: &str,
        env_var: &str,
    ) -> Result<(), Error> {
        use crate::interactive::confirm;

        // Validate environment variable name using the comprehensive validator
        if let Err(e) = crate::interactive::validate_env_var_name(env_var) {
            println!("❌ Invalid environment variable name: {e}");
            return Ok(()); // Continue the loop, don't fail completely
        }

        // Show preview and confirm
        println!("\n📝 Configuration Preview:");
        println!("   API: {api_name}");
        println!("   Scheme: {selected_scheme}");
        println!("   Environment Variable: {env_var}");

        if confirm("Apply this configuration?")? {
            self.set_secret(api_name, selected_scheme, env_var)?;
            println!("✅ Configuration saved successfully!");
        } else {
            println!("Configuration cancelled.");
        }

        Ok(())
    }

    /// Categorizes warnings by type for better formatting
    fn categorize_warnings(
        warnings: &[crate::spec::validator::ValidationWarning],
    ) -> CategorizedWarnings<'_> {
        let mut categorized = CategorizedWarnings {
            content_type: Vec::new(),
            auth: Vec::new(),
            mixed_content: Vec::new(),
        };

        for warning in warnings {
            if warning.reason.contains("no supported content types") {
                categorized.content_type.push(warning);
            } else if warning.reason.contains("unsupported authentication") {
                categorized.auth.push(warning);
            } else if warning
                .reason
                .contains("unsupported content types alongside JSON")
            {
                categorized.mixed_content.push(warning);
            }
        }

        categorized
    }

    /// Formats content type warnings
    fn format_content_type_warnings(
        lines: &mut Vec<String>,
        content_type_warnings: &[&crate::spec::validator::ValidationWarning],
        total_operations: Option<usize>,
        total_skipped: usize,
        indent: &str,
    ) {
        if content_type_warnings.is_empty() {
            return;
        }

        let warning_msg = total_operations.map_or_else(
            || {
                format!(
                    "{}Skipping {} endpoints with unsupported content types:",
                    indent,
                    content_type_warnings.len()
                )
            },
            |total| {
                let available = total.saturating_sub(total_skipped);
                format!(
                    "{}Skipping {} endpoints with unsupported content types ({} of {} endpoints will be available):",
                    indent,
                    content_type_warnings.len(),
                    available,
                    total
                )
            },
        );
        lines.push(warning_msg);

        for warning in content_type_warnings {
            lines.push(format!(
                "{}  - {} {} ({}) - {}",
                indent,
                warning.endpoint.method,
                warning.endpoint.path,
                warning.endpoint.content_type,
                warning.reason
            ));
        }
    }

    /// Formats authentication warnings
    fn format_auth_warnings(
        lines: &mut Vec<String>,
        auth_warnings: &[&crate::spec::validator::ValidationWarning],
        total_operations: Option<usize>,
        total_skipped: usize,
        indent: &str,
        add_blank_line: bool,
    ) {
        if auth_warnings.is_empty() {
            return;
        }

        if add_blank_line {
            lines.push(String::new()); // Add blank line between sections
        }

        let warning_msg = total_operations.map_or_else(
            || {
                format!(
                    "{}Skipping {} endpoints with unsupported authentication:",
                    indent,
                    auth_warnings.len()
                )
            },
            |total| {
                let available = total.saturating_sub(total_skipped);
                format!(
                    "{}Skipping {} endpoints with unsupported authentication ({} of {} endpoints will be available):",
                    indent,
                    auth_warnings.len(),
                    available,
                    total
                )
            },
        );
        lines.push(warning_msg);

        for warning in auth_warnings {
            lines.push(format!(
                "{}  - {} {} - {}",
                indent, warning.endpoint.method, warning.endpoint.path, warning.reason
            ));
        }
    }

    /// Formats mixed content warnings
    fn format_mixed_content_warnings(
        lines: &mut Vec<String>,
        mixed_content_warnings: &[&crate::spec::validator::ValidationWarning],
        indent: &str,
        add_blank_line: bool,
    ) {
        if mixed_content_warnings.is_empty() {
            return;
        }

        if add_blank_line {
            lines.push(String::new()); // Add blank line between sections
        }

        lines.push(format!(
            "{indent}Endpoints with partial content type support:"
        ));
        for warning in mixed_content_warnings {
            lines.push(format!(
                "{}  - {} {} supports JSON but not: {}",
                indent,
                warning.endpoint.method,
                warning.endpoint.path,
                warning.endpoint.content_type
            ));
        }
    }
}

/// Gets the default configuration directory path.
///
/// # Errors
///
/// Returns an error if the home directory cannot be determined.
pub fn get_config_dir() -> Result<PathBuf, Error> {
    let home_dir = dirs::home_dir().ok_or_else(|| Error::HomeDirectoryNotFound)?;
    let config_dir = home_dir.join(".config").join("aperture");
    Ok(config_dir)
}

/// Determines if the input string is a URL (starts with http:// or https://)
#[must_use]
pub fn is_url(input: &str) -> bool {
    input.starts_with("http://") || input.starts_with("https://")
}

/// Fetches `OpenAPI` specification content from a URL with security limits
///
/// # Errors
///
/// Returns an error if:
/// - Network request fails
/// - Response status is not successful
/// - Response size exceeds 10MB limit
/// - Request times out (30 seconds)
const MAX_RESPONSE_SIZE: u64 = 10 * 1024 * 1024; // 10MB

#[allow(clippy::future_not_send)]
async fn fetch_spec_from_url(url: &str) -> Result<String, Error> {
    fetch_spec_from_url_with_timeout(url, std::time::Duration::from_secs(30)).await
}

#[allow(clippy::future_not_send)]
async fn fetch_spec_from_url_with_timeout(
    url: &str,
    timeout: std::time::Duration,
) -> Result<String, Error> {
    // Create HTTP client with timeout and security limits
    let client = reqwest::Client::builder()
        .timeout(timeout)
        .build()
        .map_err(|e| Error::RequestFailed {
            reason: format!("Failed to create HTTP client: {e}"),
        })?;

    // Make the request
    let response = client.get(url).send().await.map_err(|e| {
        if e.is_timeout() {
            Error::RequestFailed {
                reason: format!("Request timed out after {} seconds", timeout.as_secs()),
            }
        } else if e.is_connect() {
            Error::RequestFailed {
                reason: format!("Failed to connect to {url}: {e}"),
            }
        } else {
            Error::RequestFailed {
                reason: format!("Network error: {e}"),
            }
        }
    })?;

    // Check response status
    if !response.status().is_success() {
        return Err(Error::RequestFailed {
            reason: format!("HTTP {} from {url}", response.status()),
        });
    }

    // Check content length before downloading
    if let Some(content_length) = response.content_length() {
        if content_length > MAX_RESPONSE_SIZE {
            return Err(Error::RequestFailed {
                reason: format!(
                    "Response too large: {content_length} bytes (max {MAX_RESPONSE_SIZE} bytes)"
                ),
            });
        }
    }

    // Read response body with size limit
    let bytes = response.bytes().await.map_err(|e| Error::RequestFailed {
        reason: format!("Failed to read response body: {e}"),
    })?;

    // Double-check size after download
    if bytes.len() > usize::try_from(MAX_RESPONSE_SIZE).unwrap_or(usize::MAX) {
        return Err(Error::RequestFailed {
            reason: format!(
                "Response too large: {} bytes (max {MAX_RESPONSE_SIZE} bytes)",
                bytes.len()
            ),
        });
    }

    // Convert to string
    String::from_utf8(bytes.to_vec()).map_err(|e| Error::RequestFailed {
        reason: format!("Invalid UTF-8 in response: {e}"),
    })
}
