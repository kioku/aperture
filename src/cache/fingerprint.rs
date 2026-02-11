use sha2::{Digest, Sha256};
use std::path::Path;

/// Compute SHA-256 hash of content and return as hex string
#[must_use]
pub fn compute_content_hash(content: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(content);
    format!("{:x}", hasher.finalize())
}

/// Get the modification time of a file in seconds since epoch.
/// Returns `None` if the file metadata cannot be read.
#[must_use]
pub fn get_file_mtime_secs(path: &Path) -> Option<u64> {
    std::fs::metadata(path)
        .ok()
        .and_then(|m| m.modified().ok())
        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|d| d.as_secs())
}
