use crate::cache::models::CachedSpec;
use crate::error::Error;
use std::fs;
use std::path::Path;

/// Loads a cached `OpenAPI` specification from the binary cache.
///
/// # Arguments
/// * `cache_dir` - The directory containing cached spec files
/// * `spec_name` - The name of the spec to load (without .bin extension)
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
    let cache_path = cache_dir.as_ref().join(format!("{spec_name}.bin"));

    // Check if the cache file exists
    if !cache_path.exists() {
        return Err(Error::CachedSpecNotFound {
            name: spec_name.to_string(),
        });
    }

    // Read the binary cache file
    let cache_data = fs::read(&cache_path).map_err(Error::Io)?;

    // Deserialize using bincode
    bincode::deserialize(&cache_data).map_err(|e| Error::CachedSpecCorrupted {
        name: spec_name.to_string(),
        reason: e.to_string(),
    })
}
