use crate::cache::fingerprint::{compute_content_hash, get_file_mtime_secs};
use crate::cache::metadata::CacheMetadataManager;
use crate::config::context_name::ApiContextName;
use crate::config::models::{ApertureSecret, ApiConfig, GlobalConfig, SecretSource};
use crate::config::url_resolver::BaseUrlResolver;
use crate::constants;
use crate::engine::loader;
use crate::error::Error;
use crate::fs::{FileSystem, OsFileSystem};
use crate::interactive::{confirm, prompt_for_input, select_from_options};
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
    pub fn get_strict_preference(&self, api_name: &ApiContextName) -> Result<bool, Error> {
        let api_name = api_name.as_str();
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
                // Use pattern matching to flatten nested if
                match line.as_str() {
                    "" => {
                        // ast-grep-ignore: no-println
                        eprintln!();
                    }
                    s if s.starts_with("Skipping") || s.starts_with("Endpoints") => {
                        // ast-grep-ignore: no-println
                        eprintln!("{} {line}", crate::constants::MSG_WARNING_PREFIX);
                    }
                    _ => {
                        // ast-grep-ignore: no-println
                        eprintln!("{line}");
                    }
                }
            }
            // ast-grep-ignore: no-println
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
        name: &ApiContextName,
        file_path: &Path,
        force: bool,
        strict: bool,
    ) -> Result<(), Error> {
        self.check_spec_exists(name.as_str(), force)?;

        let content = self.fs.read_to_string(file_path)?;
        let openapi_spec = crate::spec::parse_openapi(&content)?;

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
            name.as_str(),
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
        name: &ApiContextName,
        url: &str,
        force: bool,
        strict: bool,
    ) -> Result<(), Error> {
        self.check_spec_exists(name.as_str(), force)?;

        // Fetch content from URL
        let content = fetch_spec_from_url(url).await?;
        let openapi_spec = crate::spec::parse_openapi(&content)?;

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
            name.as_str(),
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
        name: &ApiContextName,
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
        let specs_dir = self.config_dir.join(crate::constants::DIR_SPECS);
        if !self.fs.exists(&specs_dir) {
            return Ok(Vec::new());
        }

        let mut specs = Vec::new();
        for entry in self.fs.read_dir(&specs_dir)? {
            // Early return guard clause for non-files
            if !self.fs.is_file(&entry) {
                continue;
            }

            // Use let-else for file name extraction
            let Some(file_name) = entry.file_name().and_then(|s| s.to_str()) else {
                continue;
            };

            // Check if file has yaml extension
            if std::path::Path::new(file_name)
                .extension()
                .is_some_and(|ext| ext.eq_ignore_ascii_case("yaml"))
            {
                specs.push(
                    file_name
                        .trim_end_matches(crate::constants::FILE_EXT_YAML)
                        .to_string(),
                );
            }
        }
        Ok(specs)
    }

    /// Removes an API specification from the configuration.
    ///
    /// # Errors
    ///
    /// Returns an error if the spec does not exist or cannot be removed.
    pub fn remove_spec(&self, name: &ApiContextName) -> Result<(), Error> {
        let name = name.as_str();
        let spec_path = self
            .config_dir
            .join(crate::constants::DIR_SPECS)
            .join(format!("{name}{}", crate::constants::FILE_EXT_YAML));
        let cache_path = self
            .config_dir
            .join(crate::constants::DIR_CACHE)
            .join(format!("{name}{}", crate::constants::FILE_EXT_BIN));

        if !self.fs.exists(&spec_path) {
            return Err(Error::spec_not_found(name));
        }

        self.fs.remove_file(&spec_path)?;
        if self.fs.exists(&cache_path) {
            self.fs.remove_file(&cache_path)?;
        }

        // Remove from cache metadata
        let cache_dir = self.config_dir.join(crate::constants::DIR_CACHE);
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
    pub fn edit_spec(&self, name: &ApiContextName) -> Result<(), Error> {
        let name = name.as_str();
        let spec_path = self
            .config_dir
            .join(crate::constants::DIR_SPECS)
            .join(format!("{name}{}", crate::constants::FILE_EXT_YAML));

        if !self.fs.exists(&spec_path) {
            return Err(Error::spec_not_found(name));
        }

        let editor = std::env::var("EDITOR").map_err(|_| Error::editor_not_set())?;

        // Parse the editor command to handle commands with arguments (e.g., "code --wait")
        let mut parts = editor.split_whitespace();
        let program = parts.next().ok_or_else(Error::editor_not_set)?;
        let args: Vec<&str> = parts.collect();

        Command::new(program)
            .args(&args)
            .arg(&spec_path)
            .status()
            .map_err(|e| Error::io_error(format!("Failed to get editor process status: {e}")))?
            .success()
            .then_some(()) // Convert bool to Option<()>
            .ok_or_else(|| Error::editor_failed(name))
    }

    /// Loads the global configuration from `config.toml`.
    ///
    /// # Errors
    ///
    /// Returns an error if the configuration file exists but cannot be read or parsed.
    pub fn load_global_config(&self) -> Result<GlobalConfig, Error> {
        let config_path = self.config_dir.join(crate::constants::CONFIG_FILENAME);
        if self.fs.exists(&config_path) {
            let content = self.fs.read_to_string(&config_path)?;
            toml::from_str(&content).map_err(|e| Error::invalid_config(e.to_string()))
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
        let config_path = self.config_dir.join(crate::constants::CONFIG_FILENAME);

        // Ensure config directory exists
        self.fs.create_dir_all(&self.config_dir)?;

        let content = toml::to_string_pretty(config)
            .map_err(|e| Error::serialization_error(format!("Failed to serialize config: {e}")))?;

        self.fs.atomic_write(&config_path, content.as_bytes())?;
        Ok(())
    }

    // ---- Settings Management ----

    /// Sets a global configuration setting value.
    ///
    /// Uses `toml_edit` to preserve comments and formatting in the config file.
    ///
    /// # Arguments
    /// * `key` - The setting key to modify
    /// * `value` - The value to set
    ///
    /// # Errors
    ///
    /// Returns an error if the config file cannot be read, parsed, or written.
    pub fn set_setting(
        &self,
        key: &crate::config::settings::SettingKey,
        value: &crate::config::settings::SettingValue,
    ) -> Result<(), Error> {
        use crate::config::settings::{SettingKey, SettingValue};
        use toml_edit::DocumentMut;

        let config_path = self.config_dir.join(crate::constants::CONFIG_FILENAME);

        // Load existing document or create new one
        let content = if self.fs.exists(&config_path) {
            self.fs.read_to_string(&config_path)?
        } else {
            String::new()
        };

        let mut doc: DocumentMut = content
            .parse()
            .map_err(|e| Error::invalid_config(format!("Failed to parse config: {e}")))?;

        // Apply the setting based on key
        // Note: Type mismatches indicate a programming error since parse_for_key
        // should always produce the correct type for each key.
        match (key, value) {
            (SettingKey::DefaultTimeoutSecs, SettingValue::U64(v)) => {
                doc["default_timeout_secs"] =
                    toml_edit::value(i64::try_from(*v).unwrap_or(i64::MAX));
            }
            (SettingKey::AgentDefaultsJsonErrors, SettingValue::Bool(v)) => {
                // Ensure agent_defaults table exists
                if doc.get("agent_defaults").is_none() {
                    doc["agent_defaults"] = toml_edit::Item::Table(toml_edit::Table::new());
                }
                doc["agent_defaults"]["json_errors"] = toml_edit::value(*v);
            }
            (SettingKey::RetryDefaultsMaxAttempts, SettingValue::U64(v)) => {
                // Ensure retry_defaults table exists
                if doc.get("retry_defaults").is_none() {
                    doc["retry_defaults"] = toml_edit::Item::Table(toml_edit::Table::new());
                }
                doc["retry_defaults"]["max_attempts"] =
                    toml_edit::value(i64::try_from(*v).unwrap_or(i64::MAX));
            }
            (SettingKey::RetryDefaultsInitialDelayMs, SettingValue::U64(v)) => {
                // Ensure retry_defaults table exists
                if doc.get("retry_defaults").is_none() {
                    doc["retry_defaults"] = toml_edit::Item::Table(toml_edit::Table::new());
                }
                doc["retry_defaults"]["initial_delay_ms"] =
                    toml_edit::value(i64::try_from(*v).unwrap_or(i64::MAX));
            }
            (SettingKey::RetryDefaultsMaxDelayMs, SettingValue::U64(v)) => {
                // Ensure retry_defaults table exists
                if doc.get("retry_defaults").is_none() {
                    doc["retry_defaults"] = toml_edit::Item::Table(toml_edit::Table::new());
                }
                doc["retry_defaults"]["max_delay_ms"] =
                    toml_edit::value(i64::try_from(*v).unwrap_or(i64::MAX));
            }
            // Type mismatches are programming errors - parse_for_key guarantees correct types
            (
                SettingKey::DefaultTimeoutSecs
                | SettingKey::RetryDefaultsMaxAttempts
                | SettingKey::RetryDefaultsInitialDelayMs
                | SettingKey::RetryDefaultsMaxDelayMs,
                _,
            ) => {
                debug_assert!(false, "Integer settings require U64 value");
            }
            (SettingKey::AgentDefaultsJsonErrors, _) => {
                debug_assert!(false, "AgentDefaultsJsonErrors requires Bool value");
            }
        }

        // Ensure config directory exists
        self.fs.create_dir_all(&self.config_dir)?;

        // Write back preserving formatting (atomic to prevent corruption)
        self.fs
            .atomic_write(&config_path, doc.to_string().as_bytes())?;
        Ok(())
    }

    /// Gets a global configuration setting value.
    ///
    /// # Arguments
    /// * `key` - The setting key to retrieve
    ///
    /// # Errors
    ///
    /// Returns an error if the config file cannot be read or parsed.
    pub fn get_setting(
        &self,
        key: &crate::config::settings::SettingKey,
    ) -> Result<crate::config::settings::SettingValue, Error> {
        let config = self.load_global_config()?;
        Ok(key.value_from_config(&config))
    }

    /// Lists all available configuration settings with their current values.
    ///
    /// # Errors
    ///
    /// Returns an error if the config file cannot be read or parsed.
    pub fn list_settings(&self) -> Result<Vec<crate::config::settings::SettingInfo>, Error> {
        use crate::config::settings::{SettingInfo, SettingKey};

        let config = self.load_global_config()?;
        let settings = SettingKey::ALL
            .iter()
            .map(|key| SettingInfo::new(*key, &key.value_from_config(&config)))
            .collect();

        Ok(settings)
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
        api_name: &ApiContextName,
        url: &str,
        environment: Option<&str>,
    ) -> Result<(), Error> {
        let api_name = api_name.as_str();
        // Verify the spec exists
        let spec_path = self
            .config_dir
            .join(crate::constants::DIR_SPECS)
            .join(format!("{api_name}{}", crate::constants::FILE_EXT_YAML));
        if !self.fs.exists(&spec_path) {
            return Err(Error::spec_not_found(api_name));
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
        api_name: &ApiContextName,
    ) -> Result<(Option<String>, HashMap<String, String>, String), Error> {
        let api_name = api_name.as_str();
        // Verify the spec exists
        let spec_path = self
            .config_dir
            .join(crate::constants::DIR_SPECS)
            .join(format!("{api_name}{}", crate::constants::FILE_EXT_YAML));
        if !self.fs.exists(&spec_path) {
            return Err(Error::spec_not_found(api_name));
        }

        // Load the cached spec to get its base URL
        let cache_dir = self.config_dir.join(crate::constants::DIR_CACHE);
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
        name: &ApiContextName,
        url: &str,
        force: bool,
        timeout: std::time::Duration,
    ) -> Result<(), Error> {
        // Default to non-strict mode to match CLI behavior
        self.add_spec_from_url_with_timeout_and_mode(name.as_str(), url, force, timeout, false)
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
        let openapi_spec = crate::spec::parse_openapi(&content)?;

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
        api_name: &ApiContextName,
        scheme_name: &str,
        env_var_name: &str,
    ) -> Result<(), Error> {
        let api_name = api_name.as_str();
        // Verify the spec exists
        let spec_path = self
            .config_dir
            .join(crate::constants::DIR_SPECS)
            .join(format!("{api_name}{}", crate::constants::FILE_EXT_YAML));
        if !self.fs.exists(&spec_path) {
            return Err(Error::spec_not_found(api_name));
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
    pub fn list_secrets(
        &self,
        api_name: &ApiContextName,
    ) -> Result<HashMap<String, ApertureSecret>, Error> {
        let api_name = api_name.as_str();
        // Verify the spec exists
        let spec_path = self
            .config_dir
            .join(crate::constants::DIR_SPECS)
            .join(format!("{api_name}{}", crate::constants::FILE_EXT_YAML));
        if !self.fs.exists(&spec_path) {
            return Err(Error::spec_not_found(api_name));
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
        api_name: &ApiContextName,
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
    pub fn remove_secret(&self, api_name: &ApiContextName, scheme_name: &str) -> Result<(), Error> {
        let api_name = api_name.as_str();
        // Verify the spec exists
        let spec_path = self
            .config_dir
            .join(crate::constants::DIR_SPECS)
            .join(format!("{api_name}{}", crate::constants::FILE_EXT_YAML));
        if !self.fs.exists(&spec_path) {
            return Err(Error::spec_not_found(api_name));
        }

        // Load global config
        let mut config = self.load_global_config()?;

        // Check if the API has any configured secrets
        let Some(api_config) = config.api_configs.get_mut(api_name) else {
            return Err(Error::invalid_config(format!(
                "No secrets configured for API '{api_name}'"
            )));
        };

        // Check if the API config exists but has no secrets
        if api_config.secrets.is_empty() {
            return Err(Error::invalid_config(format!(
                "No secrets configured for API '{api_name}'"
            )));
        }

        // Check if the specific scheme exists
        if !api_config.secrets.contains_key(scheme_name) {
            return Err(Error::invalid_config(format!(
                "Secret for scheme '{scheme_name}' is not configured for API '{api_name}'"
            )));
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
    pub fn clear_secrets(&self, api_name: &ApiContextName) -> Result<(), Error> {
        let api_name = api_name.as_str();
        // Verify the spec exists
        let spec_path = self
            .config_dir
            .join(crate::constants::DIR_SPECS)
            .join(format!("{api_name}{}", crate::constants::FILE_EXT_YAML));
        if !self.fs.exists(&spec_path) {
            return Err(Error::spec_not_found(api_name));
        }

        // Load global config
        let mut config = self.load_global_config()?;

        // Check if the API exists in config
        let Some(api_config) = config.api_configs.get_mut(api_name) else {
            // If API config doesn't exist, that's fine - no secrets to clear
            return Ok(());
        };

        // Clear all secrets
        api_config.secrets.clear();

        // If no other configuration remains, remove the entire API config
        if api_config.base_url_override.is_none() {
            config.api_configs.remove(api_name);
        }

        // Save the updated config
        self.save_global_config(&config)?;

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
    pub fn set_secret_interactive(&self, api_name: &ApiContextName) -> Result<(), Error> {
        // Verify the spec exists and load cached spec
        let (cached_spec, current_secrets) = self.load_spec_for_interactive_config(api_name)?;

        if cached_spec.security_schemes.is_empty() {
            // ast-grep-ignore: no-println
            println!("No security schemes found in API '{api_name}'.");
            return Ok(());
        }

        Self::display_interactive_header(api_name.as_str(), &cached_spec);

        // Create options for selection with rich descriptions
        let options = Self::build_security_scheme_options(&cached_spec, &current_secrets);

        // Interactive loop for configuration
        self.run_interactive_configuration_loop(
            api_name.as_str(),
            &cached_spec,
            &current_secrets,
            &options,
            api_name,
        )?;

        // ast-grep-ignore: no-println
        println!("\nInteractive configuration complete!");
        Ok(())
    }

    /// Checks if a spec already exists and handles force flag
    ///
    /// # Errors
    ///
    /// Returns an error if the spec already exists and force is false
    fn check_spec_exists(&self, name: &str, force: bool) -> Result<(), Error> {
        let spec_path = self
            .config_dir
            .join(crate::constants::DIR_SPECS)
            .join(format!("{name}{}", crate::constants::FILE_EXT_YAML));

        if self.fs.exists(&spec_path) && !force {
            return Err(Error::spec_already_exists(name));
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
        let spec_path = self
            .config_dir
            .join(crate::constants::DIR_SPECS)
            .join(format!("{name}{}", crate::constants::FILE_EXT_YAML));
        let cache_path = self
            .config_dir
            .join(crate::constants::DIR_CACHE)
            .join(format!("{name}{}", crate::constants::FILE_EXT_BIN));

        let spec_parent = spec_path.parent().ok_or_else(|| {
            Error::invalid_path(
                spec_path.display().to_string(),
                "Path has no parent directory",
            )
        })?;
        let cache_parent = cache_path.parent().ok_or_else(|| {
            Error::invalid_path(
                cache_path.display().to_string(),
                "Path has no parent directory",
            )
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
        // Write original spec file atomically
        self.fs.atomic_write(spec_path, content.as_bytes())?;

        // Serialize and write cached representation atomically
        let cached_data = postcard::to_allocvec(cached_spec)
            .map_err(|e| Error::serialization_error(e.to_string()))?;
        self.fs.atomic_write(cache_path, &cached_data)?;

        // Compute fingerprint for cache invalidation
        let content_hash = compute_content_hash(content.as_bytes());
        let spec_file_size = content.len() as u64;
        let mtime_secs = get_file_mtime_secs(spec_path);

        // Update cache metadata with fingerprint for optimized version checking
        let cache_dir = self.config_dir.join(crate::constants::DIR_CACHE);
        let metadata_manager = CacheMetadataManager::new(&self.fs);
        metadata_manager.update_spec_metadata_with_fingerprint(
            &cache_dir,
            name,
            cached_data.len() as u64,
            Some(content_hash),
            mtime_secs,
            Some(spec_file_size),
        )?;

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
        api_name: &ApiContextName,
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
            .join(crate::constants::DIR_SPECS)
            .join(format!("{api_name}{}", crate::constants::FILE_EXT_YAML));
        if !self.fs.exists(&spec_path) {
            return Err(Error::spec_not_found(api_name.as_str()));
        }

        // Load cached spec to get security schemes
        let cache_dir = self.config_dir.join(crate::constants::DIR_CACHE);
        let cached_spec = loader::load_cached_spec(&cache_dir, api_name.as_str())?;

        // Get current configuration
        let current_secrets = self.list_secrets(api_name)?;

        Ok((cached_spec, current_secrets))
    }

    /// Displays the interactive configuration header
    fn display_interactive_header(api_name: &str, cached_spec: &crate::cache::models::CachedSpec) {
        // ast-grep-ignore: no-println
        println!("Interactive Secret Configuration for API: {api_name}");
        // ast-grep-ignore: no-println
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
                    constants::AUTH_SCHEME_APIKEY => {
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

                // Show current configuration status - use match to avoid nested if
                description = match (
                    current_secrets.contains_key(&scheme.name),
                    &scheme.aperture_secret,
                ) {
                    (true, _) => format!("{description} [CONFIGURED]"),
                    (false, Some(_)) => format!("{description} [x-aperture-secret]"),
                    (false, None) => format!("{description} [NOT CONFIGURED]"),
                };

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
        validated_name: &ApiContextName,
    ) -> Result<(), Error> {
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
                // ast-grep-ignore: no-println
                println!("Skipping configuration for '{selected_scheme}'");
            } else {
                self.handle_secret_configuration(
                    api_name,
                    &selected_scheme,
                    &env_var,
                    validated_name,
                )?;
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
        // ast-grep-ignore: no-println
        println!("\nConfiguration for '{selected_scheme}':");
        // ast-grep-ignore: no-println
        println!("   Type: {}", scheme.scheme_type);
        if let Some(desc) = &scheme.description {
            // ast-grep-ignore: no-println
            println!("   Description: {desc}");
        }

        // Show current configuration - use pattern matching to avoid nested if
        match (
            current_secrets.get(selected_scheme),
            &scheme.aperture_secret,
        ) {
            (Some(current_secret), _) => {
                // ast-grep-ignore: no-println
                println!("   Current: environment variable '{}'", current_secret.name);
            }
            (None, Some(aperture_secret)) => {
                // ast-grep-ignore: no-println
                println!(
                    "   Current: x-aperture-secret -> '{}'",
                    aperture_secret.name
                );
            }
            (None, None) => {
                // ast-grep-ignore: no-println
                println!("   Current: not configured");
            }
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
        validated_name: &ApiContextName,
    ) -> Result<(), Error> {
        // Validate environment variable name using the comprehensive validator
        if let Err(e) = crate::interactive::validate_env_var_name(env_var) {
            // ast-grep-ignore: no-println
            println!("Invalid environment variable name: {e}");
            return Ok(()); // Continue the loop, don't fail completely
        }

        // Show preview and confirm
        // ast-grep-ignore: no-println
        println!("\nConfiguration Preview:");
        // ast-grep-ignore: no-println
        println!("   API: {api_name}");
        // ast-grep-ignore: no-println
        println!("   Scheme: {selected_scheme}");
        // ast-grep-ignore: no-println
        println!("   Environment Variable: {env_var}");

        if confirm("Apply this configuration?")? {
            self.set_secret(validated_name, selected_scheme, env_var)?;
            // ast-grep-ignore: no-println
            println!("Configuration saved successfully!");
        } else {
            // ast-grep-ignore: no-println
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
            // Pattern match on reason content to avoid nested if-else chain
            if warning.reason.contains("no supported content types") {
                categorized.content_type.push(warning);
                continue;
            }

            if warning.reason.contains("unsupported authentication") {
                categorized.auth.push(warning);
                continue;
            }

            if warning
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
    let home_dir = dirs::home_dir().ok_or_else(Error::home_directory_not_found)?;
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
        .map_err(|e| Error::network_request_failed(format!("Failed to create HTTP client: {e}")))?;

    // Make the request
    let response = client.get(url).send().await.map_err(|e| {
        // Use early returns to avoid nested if-else chain
        if e.is_timeout() {
            return Error::network_request_failed(format!(
                "Request timed out after {} seconds",
                timeout.as_secs()
            ));
        }

        if e.is_connect() {
            return Error::network_request_failed(format!("Failed to connect to {url}: {e}"));
        }

        Error::network_request_failed(format!("Network error: {e}"))
    })?;

    // Check response status
    if !response.status().is_success() {
        return Err(Error::request_failed(
            response.status(),
            format!("HTTP {} from {url}", response.status()),
        ));
    }

    // Check content length before downloading - use let-else to flatten
    let Some(content_length) = response.content_length() else {
        // No content length header, proceed to download with size check later
        return download_and_validate_response(response).await;
    };

    if content_length > MAX_RESPONSE_SIZE {
        return Err(Error::network_request_failed(format!(
            "Response too large: {content_length} bytes (max {MAX_RESPONSE_SIZE} bytes)"
        )));
    }

    download_and_validate_response(response).await
}

/// Helper function to download and validate response body
#[allow(clippy::future_not_send)]
async fn download_and_validate_response(response: reqwest::Response) -> Result<String, Error> {
    // Read response body with size limit
    let bytes = response
        .bytes()
        .await
        .map_err(|e| Error::network_request_failed(format!("Failed to read response body: {e}")))?;

    // Double-check size after download
    if bytes.len() > usize::try_from(MAX_RESPONSE_SIZE).unwrap_or(usize::MAX) {
        return Err(Error::network_request_failed(format!(
            "Response too large: {} bytes (max {MAX_RESPONSE_SIZE} bytes)",
            bytes.len()
        )));
    }

    // Convert to string
    String::from_utf8(bytes.to_vec())
        .map_err(|e| Error::network_request_failed(format!("Invalid UTF-8 in response: {e}")))
}
