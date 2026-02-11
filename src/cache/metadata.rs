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
        let mut metadata = self.load_metadata(&cache_dir)?;

        let spec_metadata = SpecMetadata {
            updated_at: chrono::Utc::now().to_rfc3339(),
            file_size,
            content_hash: None,
            mtime_secs: None,
            spec_file_size: None,
        };

        metadata.specs.insert(spec_name.to_string(), spec_metadata);
        self.save_metadata(&cache_dir, &metadata)?;
        Ok(())
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
