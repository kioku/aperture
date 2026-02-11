use crate::cache::fingerprint::compute_content_hash;
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
/// Uses a fast path: checks mtime + file size first and only reads the file to
/// compute a content hash when those match (avoiding I/O on every load when mtime
/// already differs).
/// Silently passes through if fingerprint data is unavailable (legacy metadata) or
/// if the spec file cannot be read (e.g., deleted after caching).
fn check_spec_file_freshness<P: AsRef<Path>>(
    cache_dir: P,
    spec_name: &str,
    metadata_manager: &CacheMetadataManager<'_, OsFileSystem>,
) -> Result<(), Error> {
    // Bail early if no fingerprint data (legacy metadata) or metadata error
    let Ok(Some((stored_hash, stored_mtime, stored_size))) =
        metadata_manager.get_stored_fingerprint(&cache_dir, spec_name)
    else {
        return Ok(());
    };

    // Derive the spec file path from the cache directory
    // cache_dir is typically ~/.config/aperture/.cache
    // specs_dir is typically ~/.config/aperture/specs
    let Some(config_dir) = cache_dir.as_ref().parent() else {
        return Ok(()); // Can't determine spec path, skip check
    };
    let spec_path = config_dir
        .join(crate::constants::DIR_SPECS)
        .join(format!("{spec_name}{}", crate::constants::FILE_EXT_YAML));

    // Get current file metadata (single syscall for both mtime and size)
    let Ok(file_meta) = fs::metadata(&spec_path) else {
        return Ok(()); // File missing or unreadable, skip check
    };
    let current_mtime = file_meta
        .modified()
        .ok()
        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|d| d.as_secs());
    let Some(current_mtime) = current_mtime else {
        return Ok(()); // Can't read mtime, skip check
    };
    let current_size = file_meta.len();

    // Fast path: if mtime or file size differ, cache is likely stale — no need to
    // read file content or compute hash
    if stored_mtime != current_mtime || stored_size != current_size {
        return Err(Error::cache_stale(spec_name));
    }

    // Slow path: mtime and size match — read file and verify content hash for certainty
    let Ok(content) = fs::read(&spec_path) else {
        return Ok(()); // Can't read file, skip check
    };
    let current_hash = compute_content_hash(&content);

    if stored_hash != current_hash {
        return Err(Error::cache_stale(spec_name));
    }

    Ok(())
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
