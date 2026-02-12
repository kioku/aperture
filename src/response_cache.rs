use crate::constants;
use crate::error::Error;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::path::PathBuf;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

/// Configuration for response caching
#[derive(Debug, Clone)]
pub struct CacheConfig {
    /// Directory where cache files are stored
    pub cache_dir: PathBuf,
    /// Default TTL for cached responses
    pub default_ttl: Duration,
    /// Maximum number of cached responses per API
    pub max_entries: usize,
    /// Whether caching is enabled globally
    pub enabled: bool,
    /// Whether to cache responses from authenticated requests.
    /// Default is `false` for security: auth headers could leak to disk.
    pub allow_authenticated: bool,
}

impl Default for CacheConfig {
    fn default() -> Self {
        Self {
            cache_dir: PathBuf::from(".cache/responses"),
            default_ttl: Duration::from_secs(300), // 5 minutes
            max_entries: 1000,
            enabled: true,
            allow_authenticated: false, // Secure by default
        }
    }
}

/// A cached API response
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CachedResponse {
    /// The HTTP response body
    pub body: String,
    /// HTTP status code
    pub status_code: u16,
    /// Response headers
    pub headers: HashMap<String, String>,
    /// When this response was cached (Unix timestamp)
    pub cached_at: u64,
    /// TTL in seconds from `cached_at`
    pub ttl_seconds: u64,
    /// The original request that generated this response
    pub request_info: CachedRequestInfo,
}

/// Information about the request that generated a cached response
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CachedRequestInfo {
    /// HTTP method
    pub method: String,
    /// Full URL
    pub url: String,
    /// Request headers (excluding auth headers for security)
    pub headers: HashMap<String, String>,
    /// Request body hash (for POST/PUT requests)
    pub body_hash: Option<String>,
}

/// Cache key components for generating cache file names
#[derive(Debug)]
pub struct CacheKey {
    /// API specification name
    pub api_name: String,
    /// Operation ID from `OpenAPI` spec
    pub operation_id: String,
    /// Hash of request parameters and body
    pub request_hash: String,
}

impl CacheKey {
    /// Generate a cache key from request information
    ///
    /// # Errors
    ///
    /// Returns an error if hashing fails (should be rare)
    pub fn from_request(
        api_name: &str,
        operation_id: &str,
        method: &str,
        url: &str,
        headers: &HashMap<String, String>,
        body: Option<&str>,
    ) -> Result<Self, Error> {
        let mut hasher = Sha256::new();

        // Include method, URL, and relevant headers in hash
        hasher.update(method.as_bytes());
        hasher.update(url.as_bytes());

        // Sort headers for consistent hashing (exclude auth headers)
        let mut sorted_headers: Vec<_> = headers
            .iter()
            .filter(|(key, _)| !is_auth_header(key))
            .collect();
        sorted_headers.sort_by_key(|(key, _)| *key);

        for (key, value) in sorted_headers {
            hasher.update(key.as_bytes());
            hasher.update(value.as_bytes());
        }

        // Include body hash if present
        if let Some(body_content) = body {
            hasher.update(body_content.as_bytes());
        }

        let hash = hasher.finalize();
        let request_hash = format!("{hash:x}");

        Ok(Self {
            api_name: api_name.to_string(),
            operation_id: operation_id.to_string(),
            request_hash,
        })
    }

    /// Generate the cache file name for this key
    #[must_use]
    pub fn to_filename(&self) -> String {
        let hash_prefix = if self.request_hash.len() >= 16 {
            &self.request_hash[..16]
        } else {
            &self.request_hash
        };

        format!(
            "{}_{}_{}_{}{}",
            self.api_name,
            self.operation_id,
            hash_prefix,
            constants::CACHE_SUFFIX,
            constants::FILE_EXT_JSON
        )
    }
}

/// Response cache manager
pub struct ResponseCache {
    config: CacheConfig,
}

impl ResponseCache {
    /// Creates a new response cache with the given configuration
    ///
    /// # Errors
    ///
    /// Returns an error if the cache directory cannot be created
    pub fn new(config: CacheConfig) -> Result<Self, Error> {
        // Ensure cache directory exists
        std::fs::create_dir_all(&config.cache_dir)
            .map_err(|e| Error::io_error(format!("Failed to create cache directory: {e}")))?;

        Ok(Self { config })
    }

    /// Store a response in the cache
    ///
    /// # Errors
    ///
    /// Returns an error if:
    /// - The cache file cannot be written
    /// - JSON serialization fails
    /// - Cache cleanup fails
    pub async fn store(
        &self,
        key: &CacheKey,
        body: &str,
        status_code: u16,
        headers: &HashMap<String, String>,
        request_info: CachedRequestInfo,
        ttl: Option<Duration>,
    ) -> Result<(), Error> {
        if !self.config.enabled {
            return Ok(());
        }

        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_err(|e| Error::invalid_config(format!("System time error: {e}")))?
            .as_secs();

        let ttl_seconds = ttl.unwrap_or(self.config.default_ttl).as_secs();

        let cached_response = CachedResponse {
            body: body.to_string(),
            status_code,
            headers: headers.clone(),
            cached_at: now,
            ttl_seconds,
            request_info,
        };

        let cache_file = self.config.cache_dir.join(key.to_filename());
        let json_content = serde_json::to_string_pretty(&cached_response).map_err(|e| {
            Error::serialization_error(format!("Failed to serialize cached response: {e}"))
        })?;

        // Acquire advisory lock on the cache directory to coordinate with
        // other Aperture processes writing to the same cache.
        let cache_dir = self.config.cache_dir.clone();
        let _lock =
            tokio::task::spawn_blocking(move || crate::atomic::DirLock::acquire(&cache_dir))
                .await
                .map_err(|e| Error::io_error(format!("Lock task failed: {e}")))?
                .map_err(|e| Error::io_error(format!("Failed to acquire cache lock: {e}")))?;

        crate::atomic::atomic_write(&cache_file, json_content.as_bytes())
            .await
            .map_err(|e| Error::io_error(format!("Failed to write cache file: {e}")))?;

        // Clean up old entries if we exceed max_entries
        self.cleanup_old_entries(&key.api_name).await?;

        // Lock is released when `_lock` is dropped
        Ok(())
    }

    /// Retrieve a response from the cache if it exists and is still valid
    ///
    /// # Errors
    ///
    /// Returns an error if:
    /// - The cache file cannot be read
    /// - JSON deserialization fails
    pub async fn get(&self, key: &CacheKey) -> Result<Option<CachedResponse>, Error> {
        if !self.config.enabled {
            return Ok(None);
        }

        let cache_file = self.config.cache_dir.join(key.to_filename());

        if !cache_file.exists() {
            return Ok(None);
        }

        let json_content = tokio::fs::read_to_string(&cache_file)
            .await
            .map_err(|e| Error::io_error(format!("Failed to read cache file: {e}")))?;
        let cached_response: CachedResponse = serde_json::from_str(&json_content).map_err(|e| {
            Error::serialization_error(format!("Failed to deserialize cached response: {e}"))
        })?;

        // Check if the cache entry is still valid
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_err(|e| Error::invalid_config(format!("System time error: {e}")))?
            .as_secs();

        if now > cached_response.cached_at + cached_response.ttl_seconds {
            // Cache entry has expired, remove it
            let _ = tokio::fs::remove_file(&cache_file).await;
            return Ok(None);
        }

        Ok(Some(cached_response))
    }

    /// Check if a response is cached and valid for the given key
    ///
    /// # Errors
    ///
    /// Returns an error if cache validation fails
    pub async fn is_cached(&self, key: &CacheKey) -> Result<bool, Error> {
        Ok(self.get(key).await?.is_some())
    }

    /// Clear all cached responses for a specific API
    ///
    /// # Errors
    ///
    /// Returns an error if cache files cannot be removed
    pub async fn clear_api_cache(&self, api_name: &str) -> Result<usize, Error> {
        let mut cleared_count = 0;
        let mut entries = tokio::fs::read_dir(&self.config.cache_dir)
            .await
            .map_err(|e| Error::io_error(format!("I/O operation failed: {e}")))?;

        while let Some(entry) = entries
            .next_entry()
            .await
            .map_err(|e| Error::io_error(format!("I/O operation failed: {e}")))?
        {
            let filename = entry.file_name();
            let filename_str = filename.to_string_lossy();

            if filename_str.starts_with(&format!("{api_name}_"))
                && filename_str.ends_with(constants::CACHE_FILE_SUFFIX)
            {
                tokio::fs::remove_file(entry.path())
                    .await
                    .map_err(|e| Error::io_error(format!("I/O operation failed: {e}")))?;
                cleared_count += 1;
            }
        }

        Ok(cleared_count)
    }

    /// Clear all cached responses
    ///
    /// # Errors
    ///
    /// Returns an error if cache directory cannot be cleared
    pub async fn clear_all(&self) -> Result<usize, Error> {
        let mut cleared_count = 0;
        let mut entries = tokio::fs::read_dir(&self.config.cache_dir)
            .await
            .map_err(|e| Error::io_error(format!("I/O operation failed: {e}")))?;

        while let Some(entry) = entries
            .next_entry()
            .await
            .map_err(|e| Error::io_error(format!("I/O operation failed: {e}")))?
        {
            let filename = entry.file_name();
            let filename_str = filename.to_string_lossy();

            if filename_str.ends_with(constants::CACHE_FILE_SUFFIX) {
                tokio::fs::remove_file(entry.path())
                    .await
                    .map_err(|e| Error::io_error(format!("I/O operation failed: {e}")))?;
                cleared_count += 1;
            }
        }

        Ok(cleared_count)
    }

    /// Get cache statistics for an API
    ///
    /// # Errors
    ///
    /// Returns an error if cache directory cannot be read
    pub async fn get_stats(&self, api_name: Option<&str>) -> Result<CacheStats, Error> {
        let mut stats = CacheStats::default();
        let mut entries = tokio::fs::read_dir(&self.config.cache_dir)
            .await
            .map_err(|e| Error::io_error(format!("I/O operation failed: {e}")))?;

        while let Some(entry) = entries
            .next_entry()
            .await
            .map_err(|e| Error::io_error(format!("I/O operation failed: {e}")))?
        {
            let filename = entry.file_name();
            let filename_str = filename.to_string_lossy();

            if !filename_str.ends_with(constants::CACHE_FILE_SUFFIX) {
                continue;
            }

            // Check if this entry matches the requested API
            let Some(target_api) = api_name else {
                // No filter, include all entries
                stats.total_entries += 1;

                // Check if entry is expired
                let Ok(metadata) = entry.metadata().await else {
                    continue;
                };

                stats.total_size_bytes += metadata.len();

                // Try to read the cache file to check expiration
                let Ok(json_content) = tokio::fs::read_to_string(entry.path()).await else {
                    continue;
                };

                let Ok(cached_response) = serde_json::from_str::<CachedResponse>(&json_content)
                else {
                    continue;
                };

                let now = SystemTime::now()
                    .duration_since(UNIX_EPOCH)
                    .map_err(|e| Error::invalid_config(format!("System time error: {e}")))?
                    .as_secs();

                if now > cached_response.cached_at + cached_response.ttl_seconds {
                    stats.expired_entries += 1;
                } else {
                    stats.valid_entries += 1;
                }

                continue;
            };

            if !filename_str.starts_with(&format!("{target_api}_")) {
                continue;
            }

            stats.total_entries += 1;

            // Check if entry is expired
            let Ok(metadata) = entry.metadata().await else {
                continue;
            };

            stats.total_size_bytes += metadata.len();

            // Try to read the cache file to check expiration
            let Ok(json_content) = tokio::fs::read_to_string(entry.path()).await else {
                continue;
            };

            let Ok(cached_response) = serde_json::from_str::<CachedResponse>(&json_content) else {
                continue;
            };

            let now = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map_err(|e| Error::invalid_config(format!("System time error: {e}")))?
                .as_secs();

            if now > cached_response.cached_at + cached_response.ttl_seconds {
                stats.expired_entries += 1;
            } else {
                stats.valid_entries += 1;
            }
        }

        Ok(stats)
    }

    /// Clean up old cache entries for an API, keeping only the most recent `max_entries`
    async fn cleanup_old_entries(&self, api_name: &str) -> Result<(), Error> {
        let mut entries = Vec::new();
        let mut dir_entries = tokio::fs::read_dir(&self.config.cache_dir)
            .await
            .map_err(|e| Error::io_error(format!("I/O operation failed: {e}")))?;

        while let Some(entry) = dir_entries
            .next_entry()
            .await
            .map_err(|e| Error::io_error(format!("I/O operation failed: {e}")))?
        {
            let filename = entry.file_name();
            let filename_str = filename.to_string_lossy();

            if !filename_str.starts_with(&format!("{api_name}_"))
                || !filename_str.ends_with(constants::CACHE_FILE_SUFFIX)
            {
                continue;
            }

            let Ok(metadata) = entry.metadata().await else {
                continue;
            };

            let Ok(modified) = metadata.modified() else {
                continue;
            };

            entries.push((entry.path(), modified));
        }

        // If we have more entries than max_entries, remove the oldest ones
        if entries.len() > self.config.max_entries {
            entries.sort_by_key(|(_, modified)| *modified);
            let to_remove = entries.len() - self.config.max_entries;

            for (path, _) in entries.iter().take(to_remove) {
                let _ = tokio::fs::remove_file(path).await;
            }
        }

        Ok(())
    }
}

/// Cache statistics
#[derive(Debug, Default)]
pub struct CacheStats {
    /// Total number of cache entries
    pub total_entries: usize,
    /// Number of valid (non-expired) entries
    pub valid_entries: usize,
    /// Number of expired entries
    pub expired_entries: usize,
    /// Total size of cache files in bytes
    pub total_size_bytes: u64,
}

/// Check if a header is an authentication header that should be excluded from caching
#[must_use]
pub fn is_auth_header(header_name: &str) -> bool {
    constants::is_auth_header(header_name)
        || header_name
            .to_lowercase()
            .starts_with(constants::HEADER_PREFIX_X_AUTH)
        || header_name
            .to_lowercase()
            .starts_with(constants::HEADER_PREFIX_X_API)
}

/// Scrub authentication headers from a header map before caching.
///
/// This ensures sensitive credentials are never persisted to disk,
/// maintaining the security boundary between configuration and secrets.
#[must_use]
pub fn scrub_auth_headers<S: std::hash::BuildHasher>(
    headers: &HashMap<String, String, S>,
) -> HashMap<String, String> {
    headers
        .iter()
        .filter(|(key, _)| !is_auth_header(key))
        .map(|(k, v)| (k.clone(), v.clone()))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn create_test_cache_config() -> (CacheConfig, TempDir) {
        let temp_dir = TempDir::new().unwrap();
        let config = CacheConfig {
            cache_dir: temp_dir.path().to_path_buf(),
            default_ttl: Duration::from_secs(60),
            max_entries: 10,
            enabled: true,
            allow_authenticated: false,
        };
        (config, temp_dir)
    }

    #[test]
    fn test_cache_key_generation() {
        let mut headers = HashMap::new();
        headers.insert(
            constants::HEADER_CONTENT_TYPE_LC.to_string(),
            constants::CONTENT_TYPE_JSON.to_string(),
        );
        headers.insert(
            constants::HEADER_AUTHORIZATION_LC.to_string(),
            "Bearer secret".to_string(),
        ); // Should be excluded

        let key = CacheKey::from_request(
            "test_api",
            "getUser",
            constants::HTTP_METHOD_GET,
            "https://api.example.com/users/123",
            &headers,
            None,
        )
        .unwrap();

        assert_eq!(key.api_name, "test_api");
        assert_eq!(key.operation_id, "getUser");
        assert!(!key.request_hash.is_empty());

        let filename = key.to_filename();
        assert!(filename.starts_with("test_api_getUser_"));
        assert!(filename.ends_with(constants::CACHE_FILE_SUFFIX));
    }

    #[test]
    fn test_is_auth_header() {
        assert!(is_auth_header(constants::HEADER_AUTHORIZATION));
        assert!(is_auth_header("X-API-Key"));
        assert!(is_auth_header("x-auth-token"));
        assert!(!is_auth_header(constants::HEADER_CONTENT_TYPE));
        assert!(!is_auth_header("User-Agent"));
    }

    #[test]
    fn test_scrub_auth_headers() {
        let mut headers = HashMap::new();
        headers.insert("Authorization".to_string(), "Bearer secret".to_string());
        headers.insert("X-API-Key".to_string(), "api-key-123".to_string());
        headers.insert("x-auth-token".to_string(), "token-456".to_string());
        headers.insert("Content-Type".to_string(), "application/json".to_string());
        headers.insert("User-Agent".to_string(), "test-agent".to_string());
        headers.insert("Accept".to_string(), "application/json".to_string());

        let scrubbed = scrub_auth_headers(&headers);

        // Auth headers should be removed
        assert!(!scrubbed.contains_key("Authorization"));
        assert!(!scrubbed.contains_key("X-API-Key"));
        assert!(!scrubbed.contains_key("x-auth-token"));

        // Non-auth headers should be preserved
        assert_eq!(
            scrubbed.get("Content-Type"),
            Some(&"application/json".to_string())
        );
        assert_eq!(scrubbed.get("User-Agent"), Some(&"test-agent".to_string()));
        assert_eq!(
            scrubbed.get("Accept"),
            Some(&"application/json".to_string())
        );

        // Only 3 non-auth headers should remain
        assert_eq!(scrubbed.len(), 3);
    }

    #[tokio::test]
    async fn test_cache_store_and_retrieve() {
        let (config, _temp_dir) = create_test_cache_config();
        let cache = ResponseCache::new(config).unwrap();

        let key = CacheKey {
            api_name: "test_api".to_string(),
            operation_id: "getUser".to_string(),
            request_hash: "abc123".to_string(),
        };

        let mut headers = HashMap::new();
        headers.insert(
            constants::HEADER_CONTENT_TYPE_LC.to_string(),
            constants::CONTENT_TYPE_JSON.to_string(),
        );

        let request_info = CachedRequestInfo {
            method: constants::HTTP_METHOD_GET.to_string(),
            url: "https://api.example.com/users/123".to_string(),
            headers: headers.clone(),
            body_hash: None,
        };

        // Store a response
        cache
            .store(
                &key,
                r#"{"id": 123, "name": "John"}"#,
                200,
                &headers,
                request_info,
                Some(Duration::from_secs(60)),
            )
            .await
            .unwrap();

        // Retrieve the response
        let cached = cache.get(&key).await.unwrap();
        assert!(cached.is_some());

        let response = cached.unwrap();
        assert_eq!(response.body, r#"{"id": 123, "name": "John"}"#);
        assert_eq!(response.status_code, 200);
    }

    #[tokio::test]
    async fn test_cache_expiration() {
        let (config, _temp_dir) = create_test_cache_config();
        let cache = ResponseCache::new(config).unwrap();

        let key = CacheKey {
            api_name: "test_api".to_string(),
            operation_id: "getUser".to_string(),
            request_hash: "abc123def456".to_string(),
        };

        let headers = HashMap::new();
        let request_info = CachedRequestInfo {
            method: constants::HTTP_METHOD_GET.to_string(),
            url: "https://api.example.com/users/123".to_string(),
            headers: headers.clone(),
            body_hash: None,
        };

        // Store a response with 1 second TTL
        cache
            .store(
                &key,
                "test response",
                200,
                &headers,
                request_info,
                Some(Duration::from_secs(1)),
            )
            .await
            .unwrap();

        // Should be cached immediately
        assert!(cache.is_cached(&key).await.unwrap());

        // Manually create an expired cache entry by modifying the cached_at time
        let cache_file = cache.config.cache_dir.join(key.to_filename());
        let mut cached_response: CachedResponse = {
            let json_content = tokio::fs::read_to_string(&cache_file).await.unwrap();
            serde_json::from_str(&json_content).unwrap()
        };

        // Set cached_at to a time in the past that exceeds TTL
        cached_response.cached_at = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs()
            - 2; // 2 seconds ago, which exceeds 1 second TTL

        let json_content = serde_json::to_string_pretty(&cached_response).unwrap();
        tokio::fs::write(&cache_file, json_content).await.unwrap();

        // Should no longer be cached due to expiration
        assert!(!cache.is_cached(&key).await.unwrap());

        // The expired file should be removed
        assert!(!cache_file.exists());
    }
}
