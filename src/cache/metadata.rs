use crate::cache::models::{GlobalCacheMetadata, SpecMetadata, CACHE_FORMAT_VERSION};
use crate::constants;
use crate::error::Error;
use crate::fs::FileSystem;
use std::path::Path;

/// Manages cache metadata for optimized version checking
pub struct CacheMetadataManager<'a, F: FileSystem> {
    fs: &'a F,
}

impl<'a, F: FileSystem> CacheMetadataManager<'a, F> {
    pub const fn new(fs: &'a F) -> Self {
        Self { fs }
    }

    /// Load global cache metadata, creating default if it doesn't exist
    ///
    /// # Errors
    /// Returns an error if the metadata file exists but cannot be read or parsed
    pub fn load_metadata<P: AsRef<Path>>(
        &self,
        cache_dir: P,
    ) -> Result<GlobalCacheMetadata, Error> {
        let metadata_path = cache_dir.as_ref().join(constants::CACHE_METADATA_FILENAME);

        if !self.fs.exists(&metadata_path) {
            // Create default metadata file
            let metadata = GlobalCacheMetadata::default();
            self.save_metadata(&cache_dir, &metadata)?;
            return Ok(metadata);
        }

        let content = self.fs.read_to_string(&metadata_path)?;
        serde_json::from_str(&content)
            .map_err(|e| Error::invalid_config(format!("Failed to parse cache metadata: {e}")))
    }

    /// Save global cache metadata
    ///
    /// # Errors
    /// Returns an error if the metadata cannot be serialized or written to disk
    pub fn save_metadata<P: AsRef<Path>>(
        &self,
        cache_dir: P,
        metadata: &GlobalCacheMetadata,
    ) -> Result<(), Error> {
        let metadata_path = cache_dir.as_ref().join(constants::CACHE_METADATA_FILENAME);

        // Ensure cache directory exists
        self.fs.create_dir_all(cache_dir.as_ref())?;

        let content = serde_json::to_string_pretty(metadata).map_err(|e| {
            Error::serialization_error(format!("Failed to serialize cache metadata: {e}"))
        })?;

        self.fs.write_all(&metadata_path, content.as_bytes())?;
        Ok(())
    }

    /// Check if a spec's cache is compatible with current version
    ///
    /// # Errors
    /// Returns an error if the metadata file cannot be loaded
    pub fn check_spec_version<P: AsRef<Path>>(
        &self,
        cache_dir: P,
        spec_name: &str,
    ) -> Result<bool, Error> {
        let metadata = self.load_metadata(&cache_dir)?;

        // Check global format version
        if metadata.cache_format_version != CACHE_FORMAT_VERSION {
            return Ok(false);
        }

        // Check if spec exists in metadata
        Ok(metadata.specs.contains_key(spec_name))
    }

    /// Update metadata for a specific spec
    ///
    /// # Errors
    /// Returns an error if the metadata cannot be loaded or saved
    pub fn update_spec_metadata<P: AsRef<Path>>(
        &self,
        cache_dir: P,
        spec_name: &str,
        file_size: u64,
    ) -> Result<(), Error> {
        self.update_spec_metadata_with_fingerprint(
            cache_dir, spec_name, file_size, None, None, None,
        )
    }

    /// Update metadata for a specific spec including fingerprint data for cache invalidation
    ///
    /// # Errors
    /// Returns an error if the metadata cannot be loaded or saved
    pub fn update_spec_metadata_with_fingerprint<P: AsRef<Path>>(
        &self,
        cache_dir: P,
        spec_name: &str,
        file_size: u64,
        content_hash: Option<String>,
        mtime_secs: Option<u64>,
        spec_file_size: Option<u64>,
    ) -> Result<(), Error> {
        let mut metadata = self.load_metadata(&cache_dir)?;

        let spec_metadata = SpecMetadata {
            updated_at: chrono::Utc::now().to_rfc3339(),
            file_size,
            content_hash,
            mtime_secs,
            spec_file_size,
        };

        metadata.specs.insert(spec_name.to_string(), spec_metadata);
        self.save_metadata(&cache_dir, &metadata)?;
        Ok(())
    }

    /// Check if a spec's cache is fresh by comparing fingerprints
    ///
    /// Returns `true` if the cache is fresh (fingerprint matches), `false` if stale.
    /// Returns `None` if no fingerprint data is available (legacy metadata).
    ///
    /// Uses a fast path: checks mtime + `file_size` first, only computes the
    /// content hash if those match (to avoid hashing on every load when mtime differs).
    ///
    /// # Errors
    /// Returns an error if the metadata file cannot be loaded
    pub fn check_spec_freshness<P: AsRef<Path>>(
        &self,
        cache_dir: P,
        spec_name: &str,
        current_mtime_secs: u64,
        current_file_size: u64,
        current_content_hash: &str,
    ) -> Result<Option<bool>, Error> {
        let metadata = self.load_metadata(&cache_dir)?;

        let Some(spec_meta) = metadata.specs.get(spec_name) else {
            return Ok(None);
        };

        // If no fingerprint data stored, treat as legacy (no opinion on freshness)
        let (Some(stored_hash), Some(stored_mtime), Some(stored_size)) = (
            &spec_meta.content_hash,
            spec_meta.mtime_secs,
            spec_meta.spec_file_size,
        ) else {
            return Ok(None);
        };

        // Fast path: if mtime or file size differ, cache is definitely stale
        if stored_mtime != current_mtime_secs || stored_size != current_file_size {
            return Ok(Some(false));
        }

        // mtime and size match — verify content hash for certainty
        Ok(Some(stored_hash == current_content_hash))
    }

    /// Remove spec from metadata
    ///
    /// # Errors
    /// Returns an error if the metadata cannot be loaded or saved
    pub fn remove_spec_metadata<P: AsRef<Path>>(
        &self,
        cache_dir: P,
        spec_name: &str,
    ) -> Result<(), Error> {
        let mut metadata = self.load_metadata(&cache_dir)?;
        metadata.specs.remove(spec_name);
        self.save_metadata(&cache_dir, &metadata)?;
        Ok(())
    }

    /// Get all specs in metadata
    ///
    /// # Errors
    /// Returns an error if the metadata file cannot be loaded
    pub fn list_cached_specs<P: AsRef<Path>>(&self, cache_dir: P) -> Result<Vec<String>, Error> {
        let metadata = self.load_metadata(&cache_dir)?;
        Ok(metadata.specs.keys().cloned().collect())
    }
}
