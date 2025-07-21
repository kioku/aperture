use crate::cache::metadata::CacheMetadataManager;
use crate::config::models::{ApiConfig, GlobalConfig};
use crate::config::url_resolver::BaseUrlResolver;
use crate::engine::loader;
use crate::error::Error;
use crate::fs::{FileSystem, OsFileSystem};
use crate::spec::{SpecTransformer, SpecValidator};
use openapiv3::{OpenAPI, ReferenceOr};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::process::Command;

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

    /// Display validation warnings to stderr
    fn display_validation_warnings(
        warnings: &[crate::spec::validator::ValidationWarning],
        total_operations: Option<usize>,
    ) {
        if !warnings.is_empty() {
            let warning_msg = total_operations.map_or_else(
                || {
                    format!(
                        "Warning: Skipping {} endpoints with unsupported content types:",
                        warnings.len()
                    )
                },
                |total| {
                    let available = total.saturating_sub(warnings.len());
                    format!(
                        "Warning: Skipping {} endpoints with unsupported content types ({} of {} endpoints will be available):",
                        warnings.len(),
                        available,
                        total
                    )
                },
            );
            eprintln!("{warning_msg}");

            for warning in warnings {
                eprintln!(
                    "  - {} {} ({}) - {}",
                    warning.endpoint.method,
                    warning.endpoint.path,
                    warning.endpoint.content_type,
                    warning.reason
                );
            }
            eprintln!("\nUse --strict to reject specs with unsupported content types.");
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
        let spec_path = self.config_dir.join("specs").join(format!("{name}.yaml"));
        let cache_path = self.config_dir.join(".cache").join(format!("{name}.bin"));

        if self.fs.exists(&spec_path) && !force {
            return Err(Error::SpecAlreadyExists {
                name: name.to_string(),
            });
        }

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

        // Transform into internal cached representation using SpecTransformer
        let transformer = SpecTransformer::new();

        // Convert warnings to skip_endpoints format
        let skip_endpoints: Vec<(String, String)> = validation_result
            .warnings
            .iter()
            .map(|w| (w.endpoint.path.clone(), w.endpoint.method.clone()))
            .collect();

        let cached_spec = transformer.transform_with_warnings(
            name,
            &openapi_spec,
            &skip_endpoints,
            &validation_result.warnings,
        )?;

        // Create directories
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

        // Write original spec file
        self.fs.write_all(&spec_path, content.as_bytes())?;

        // Serialize and write cached representation
        let cached_data =
            bincode::serialize(&cached_spec).map_err(|e| Error::SerializationError {
                reason: e.to_string(),
            })?;
        self.fs.write_all(&cache_path, &cached_data)?;

        // Update cache metadata for optimized version checking
        let cache_dir = self.config_dir.join(".cache");
        let metadata_manager = CacheMetadataManager::new(&self.fs);
        metadata_manager.update_spec_metadata(&cache_dir, name, cached_data.len() as u64)?;

        Ok(())
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
        let spec_path = self.config_dir.join("specs").join(format!("{name}.yaml"));
        let cache_path = self.config_dir.join(".cache").join(format!("{name}.bin"));

        if self.fs.exists(&spec_path) && !force {
            return Err(Error::SpecAlreadyExists {
                name: name.to_string(),
            });
        }

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

        // Transform into internal cached representation using SpecTransformer
        let transformer = SpecTransformer::new();

        // Convert warnings to skip_endpoints format
        let skip_endpoints: Vec<(String, String)> = validation_result
            .warnings
            .iter()
            .map(|w| (w.endpoint.path.clone(), w.endpoint.method.clone()))
            .collect();

        let cached_spec = transformer.transform_with_warnings(
            name,
            &openapi_spec,
            &skip_endpoints,
            &validation_result.warnings,
        )?;

        // Create directories
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

        // Write original spec file
        self.fs.write_all(&spec_path, content.as_bytes())?;

        // Serialize and write cached representation
        let cached_data =
            bincode::serialize(&cached_spec).map_err(|e| Error::SerializationError {
                reason: e.to_string(),
            })?;
        self.fs.write_all(&cache_path, &cached_data)?;

        // Update cache metadata for optimized version checking
        let cache_dir = self.config_dir.join(".cache");
        let metadata_manager = CacheMetadataManager::new(&self.fs);
        metadata_manager.update_spec_metadata(&cache_dir, name, cached_data.len() as u64)?;

        Ok(())
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
        // Default to strict mode for backward compatibility in tests
        self.add_spec_from_url_with_timeout_and_mode(name, url, force, timeout, true)
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
        let spec_path = self.config_dir.join("specs").join(format!("{name}.yaml"));
        let cache_path = self.config_dir.join(".cache").join(format!("{name}.bin"));

        if self.fs.exists(&spec_path) && !force {
            return Err(Error::SpecAlreadyExists {
                name: name.to_string(),
            });
        }

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

        // Note: Not displaying warnings in test method

        // Transform into internal cached representation using SpecTransformer
        let transformer = SpecTransformer::new();

        // Convert warnings to skip_endpoints format
        let skip_endpoints: Vec<(String, String)> = validation_result
            .warnings
            .iter()
            .map(|w| (w.endpoint.path.clone(), w.endpoint.method.clone()))
            .collect();

        let cached_spec = transformer.transform_with_warnings(
            name,
            &openapi_spec,
            &skip_endpoints,
            &validation_result.warnings,
        )?;

        // Create directories
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

        // Write original spec file
        self.fs.write_all(&spec_path, content.as_bytes())?;

        // Serialize and write cached representation
        let cached_data =
            bincode::serialize(&cached_spec).map_err(|e| Error::SerializationError {
                reason: e.to_string(),
            })?;
        self.fs.write_all(&cache_path, &cached_data)?;

        Ok(())
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
