use crate::cache::metadata::CacheMetadataManager;
use crate::cache::models::{CachedSpec, CACHE_FORMAT_VERSION};
use crate::error::Error;
use crate::fs::OsFileSystem;
use std::fs;
use std::path::Path;

/// Loads a cached `OpenAPI` specification from the binary cache with optimized version checking.
///
/// This function uses a global cache metadata file for fast version checking before
/// loading the full specification, significantly improving performance.
///
/// # Arguments
/// * `cache_dir` - The directory containing cached spec files
/// * `spec_name` - The name of the spec to load (without binary extension)
///
/// # Returns
/// * `Ok(CachedSpec)` - The loaded and deserialized specification
/// * `Err(Error)` - If the file doesn't exist or deserialization fails
///
/// # Errors
/// Returns an error if the cache file doesn't exist or if deserialization fails
pub fn load_cached_spec<P: AsRef<Path>>(
    cache_dir: P,
    spec_name: &str,
) -> Result<CachedSpec, Error> {
    // Fast version check using metadata
    let fs = OsFileSystem;
    let metadata_manager = CacheMetadataManager::new(&fs);

    // Check if spec exists and version is compatible
    match metadata_manager.check_spec_version(&cache_dir, spec_name) {
        Ok(true) => {
            // Version is compatible, load spec directly (no version check needed)
            load_cached_spec_without_version_check(&cache_dir, spec_name)
        }
        Ok(false) => {
            // Version mismatch or spec not in metadata, fall back to legacy method
            load_cached_spec_with_version_check(&cache_dir, spec_name)
        }
        Err(_) => {
            // Metadata loading failed, fall back to legacy method
            load_cached_spec_with_version_check(&cache_dir, spec_name)
        }
    }
}

/// Load cached spec without version checking (optimized path)
fn load_cached_spec_without_version_check<P: AsRef<Path>>(
    cache_dir: P,
    spec_name: &str,
) -> Result<CachedSpec, Error> {
    let cache_path = cache_dir
        .as_ref()
        .join(format!("{spec_name}{}", crate::constants::FILE_EXT_BIN));

    if !cache_path.exists() {
        return Err(Error::CachedSpecNotFound {
            name: spec_name.to_string(),
        });
    }

    let cache_data = fs::read(&cache_path).map_err(Error::Io)?;
    bincode::deserialize(&cache_data).map_err(|e| Error::CachedSpecCorrupted {
        name: spec_name.to_string(),
        reason: e.to_string(),
    })
}

/// Load cached spec with embedded version checking (legacy/fallback path)
fn load_cached_spec_with_version_check<P: AsRef<Path>>(
    cache_dir: P,
    spec_name: &str,
) -> Result<CachedSpec, Error> {
    let cache_path = cache_dir
        .as_ref()
        .join(format!("{spec_name}{}", crate::constants::FILE_EXT_BIN));

    if !cache_path.exists() {
        return Err(Error::CachedSpecNotFound {
            name: spec_name.to_string(),
        });
    }

    let cache_data = fs::read(&cache_path).map_err(Error::Io)?;
    let cached_spec: CachedSpec =
        bincode::deserialize(&cache_data).map_err(|e| Error::CachedSpecCorrupted {
            name: spec_name.to_string(),
            reason: e.to_string(),
        })?;

    // Check cache format version
    if cached_spec.cache_format_version != CACHE_FORMAT_VERSION {
        return Err(Error::CacheVersionMismatch {
            name: spec_name.to_string(),
            found: cached_spec.cache_format_version,
            expected: CACHE_FORMAT_VERSION,
        });
    }

    Ok(cached_spec)
}
