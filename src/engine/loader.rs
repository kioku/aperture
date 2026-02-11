use crate::cache::metadata::CacheMetadataManager;
use crate::cache::models::{CachedSpec, CACHE_FORMAT_VERSION};
use crate::config::manager::{compute_content_hash, get_file_mtime_secs};
use crate::error::Error;
use crate::fs::OsFileSystem;
use std::fs;
use std::path::Path;

/// Loads a cached `OpenAPI` specification from the binary cache with optimized version checking.
///
/// This function uses a global cache metadata file for fast version checking before
/// loading the full specification, significantly improving performance.
///
/// After version checks pass, validates the spec file fingerprint (mtime, size, content hash)
/// against the cached metadata. If the spec file has been modified since caching, returns
/// `Error::cache_stale` with a suggestion to reinitialize.
///
/// # Arguments
/// * `cache_dir` - The directory containing cached spec files
/// * `spec_name` - The name of the spec to load (without binary extension)
///
/// # Returns
/// * `Ok(CachedSpec)` - The loaded and deserialized specification
/// * `Err(Error)` - If the file doesn't exist, deserialization fails, or cache is stale
///
/// # Errors
/// Returns an error if the cache file doesn't exist, deserialization fails, or
/// the spec file has been modified since the cache was built
pub fn load_cached_spec<P: AsRef<Path>>(
    cache_dir: P,
    spec_name: &str,
) -> Result<CachedSpec, Error> {
    // Fast version check using metadata
    let fs = OsFileSystem;
    let metadata_manager = CacheMetadataManager::new(&fs);

    // Check if spec exists and version is compatible
    let spec = match metadata_manager.check_spec_version(&cache_dir, spec_name) {
        Ok(true) => {
            // Version is compatible, load spec directly (no version check needed)
            load_cached_spec_without_version_check(&cache_dir, spec_name)?
        }
        Ok(false) => {
            // Version mismatch or spec not in metadata, fall back to legacy method
            load_cached_spec_with_version_check(&cache_dir, spec_name)?
        }
        Err(_) => {
            // Metadata loading failed, fall back to legacy method
            load_cached_spec_with_version_check(&cache_dir, spec_name)?
        }
    };

    // Validate spec file fingerprint to detect stale caches
    check_spec_file_freshness(&cache_dir, spec_name, &metadata_manager)?;

    Ok(spec)
}

/// Checks whether the spec source file has been modified since the cache was built.
///
/// Derives the spec file path from the cache directory (sibling `specs/` directory).
/// Uses a fast path: checks mtime + file_size first, only computes content hash if needed.
/// Silently passes through if fingerprint data is unavailable (legacy metadata) or
/// if the spec file cannot be read (e.g., deleted after caching).
fn check_spec_file_freshness<P: AsRef<Path>>(
    cache_dir: P,
    spec_name: &str,
    metadata_manager: &CacheMetadataManager<'_, OsFileSystem>,
) -> Result<(), Error> {
    // Derive the spec file path from the cache directory
    // cache_dir is typically ~/.config/aperture/.cache
    // specs_dir is typically ~/.config/aperture/specs
    let Some(config_dir) = cache_dir.as_ref().parent() else {
        return Ok(()); // Can't determine spec path, skip check
    };
    let spec_path = config_dir
        .join(crate::constants::DIR_SPECS)
        .join(format!("{spec_name}{}", crate::constants::FILE_EXT_YAML));

    // If the spec file doesn't exist, skip the freshness check
    // (the file might have been deleted but cache is still usable)
    if !spec_path.exists() {
        return Ok(());
    }

    // Get current file attributes
    let current_mtime = match get_file_mtime_secs(&spec_path) {
        Some(mtime) => mtime,
        None => return Ok(()), // Can't read mtime, skip check
    };
    let current_size = match fs::metadata(&spec_path) {
        Ok(m) => m.len(),
        Err(_) => return Ok(()), // Can't read metadata, skip check
    };

    // Read file content and compute hash
    let content = match fs::read(&spec_path) {
        Ok(c) => c,
        Err(_) => return Ok(()), // Can't read file, skip check
    };
    let current_hash = compute_content_hash(&content);

    // Check freshness against stored fingerprint
    match metadata_manager.check_spec_freshness(
        &cache_dir,
        spec_name,
        current_mtime,
        current_size,
        &current_hash,
    ) {
        Ok(Some(true)) => Ok(()),  // Fresh
        Ok(Some(false)) => Err(Error::cache_stale(spec_name)), // Stale
        Ok(None) => Ok(()),        // No fingerprint data (legacy), pass through
        Err(_) => Ok(()),          // Metadata error, pass through
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
        return Err(Error::cached_spec_not_found(spec_name));
    }

    let cache_data = fs::read(&cache_path)
        .map_err(|e| Error::io_error(format!("Failed to read cache file: {e}")))?;
    bincode::deserialize(&cache_data)
        .map_err(|e| Error::cached_spec_corrupted(spec_name, e.to_string()))
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
        return Err(Error::cached_spec_not_found(spec_name));
    }

    let cache_data = fs::read(&cache_path)
        .map_err(|e| Error::io_error(format!("Failed to read cache file: {e}")))?;
    let cached_spec: CachedSpec = bincode::deserialize(&cache_data)
        .map_err(|e| Error::cached_spec_corrupted(spec_name, e.to_string()))?;

    // Check cache format version
    if cached_spec.cache_format_version != CACHE_FORMAT_VERSION {
        return Err(Error::cache_version_mismatch(
            spec_name,
            cached_spec.cache_format_version,
            CACHE_FORMAT_VERSION,
        ));
    }

    Ok(cached_spec)
}
