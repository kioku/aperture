//! Checked per-response policy shared by network execution and cache readers.
use crate::error::Error;

/// Ordinary API default (64 MiB); spec and skill policies are independent.
pub const DEFAULT_MAX_RESPONSE_BYTES: u64 = 64 * 1024 * 1024;
/// Space reserved for cache metadata in addition to worst-case JSON body escaping.
const CACHE_METADATA_BYTES: u64 = 1024 * 1024;
/// Largest portable allocation/envelope value supported on the current platform.
pub const MAX_RESPONSE_BYTES: u64 = (isize::MAX as u64 - CACHE_METADATA_BYTES - 1) / 6;

/// Validate even directly constructed SDK configurations before any request.
///
/// # Errors
/// Rejects zero and values whose bounded cache envelope cannot be represented.
pub fn validate(value: u64) -> Result<usize, Error> {
    if value == 0 || value > MAX_RESPONSE_BYTES {
        return Err(Error::invalid_config(format!(
            "max_response_bytes must be between 1 and {MAX_RESPONSE_BYTES}"
        )));
    }
    usize::try_from(value).map_err(|_| Error::invalid_config("max_response_bytes overflow"))
}

/// Checked allowance: JSON can escape each body byte as six ASCII bytes.
/// Metadata above the fixed allowance may produce a safe cache miss.
pub(crate) fn cache_envelope(value: u64) -> Result<u64, Error> {
    validate(value)?;
    value
        .checked_mul(6)
        .and_then(|bytes| bytes.checked_add(CACHE_METADATA_BYTES))
        .ok_or_else(|| Error::invalid_config("max_response_bytes cache envelope overflow"))
}

/// Safe size failure: no body fragments, URL, or retryable transport cause.
pub(crate) fn exceeded(limit: usize) -> Error {
    Error::validation_error(format!(
        "API response exceeds max_response_bytes ({limit} bytes)"
    ))
}
