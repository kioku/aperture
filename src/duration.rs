//! Duration parsing utilities for CLI arguments.
//!
//! Supports human-readable duration formats like "500ms", "1s", "30s", "1m".

use crate::error::Error;
use std::time::Duration;

/// Parses a human-readable duration string into a `Duration`.
///
/// Supported formats:
/// - Milliseconds: "100ms", "500ms"
/// - Seconds: "1s", "30s", "120s"
/// - Minutes: "1m", "5m"
/// - Plain number (treated as milliseconds): "500"
///
/// # Errors
///
/// Returns an error if the format is invalid or the value is out of range.
///
/// # Examples
///
/// ```
/// use aperture_cli::duration::parse_duration;
/// use std::time::Duration;
///
/// assert_eq!(parse_duration("500ms").unwrap(), Duration::from_millis(500));
/// assert_eq!(parse_duration("1s").unwrap(), Duration::from_secs(1));
/// assert_eq!(parse_duration("30s").unwrap(), Duration::from_secs(30));
/// assert_eq!(parse_duration("1m").unwrap(), Duration::from_secs(60));
/// assert_eq!(parse_duration("500").unwrap(), Duration::from_millis(500));
/// ```
pub fn parse_duration(s: &str) -> Result<Duration, Error> {
    let s = s.trim();

    if s.is_empty() {
        return Err(Error::invalid_config(
            "Duration cannot be empty".to_string(),
        ));
    }

    // Try parsing with suffixes
    if let Some(ms_str) = s.strip_suffix("ms") {
        let ms: u64 = ms_str
            .trim()
            .parse()
            .map_err(|_| Error::invalid_config(format!("Invalid milliseconds value: {ms_str}")))?;
        return Ok(Duration::from_millis(ms));
    }

    if let Some(m_str) = s.strip_suffix('m') {
        // Make sure it's not "ms" (already handled above)
        let minutes: u64 = m_str
            .trim()
            .parse()
            .map_err(|_| Error::invalid_config(format!("Invalid minutes value: {m_str}")))?;
        return Ok(Duration::from_secs(minutes * 60));
    }

    if let Some(s_str) = s.strip_suffix('s') {
        let secs: u64 = s_str
            .trim()
            .parse()
            .map_err(|_| Error::invalid_config(format!("Invalid seconds value: {s_str}")))?;
        return Ok(Duration::from_secs(secs));
    }

    // Plain number - treat as milliseconds
    let ms: u64 = s.parse().map_err(|_| {
        Error::invalid_config(format!(
            "Invalid duration format: {s}. Use format like '500ms', '1s', '30s', or '1m'"
        ))
    })?;
    Ok(Duration::from_millis(ms))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_duration_milliseconds() {
        assert_eq!(parse_duration("100ms").unwrap(), Duration::from_millis(100));
        assert_eq!(parse_duration("500ms").unwrap(), Duration::from_millis(500));
        assert_eq!(
            parse_duration("1000ms").unwrap(),
            Duration::from_millis(1000)
        );
    }

    #[test]
    fn test_parse_duration_seconds() {
        assert_eq!(parse_duration("1s").unwrap(), Duration::from_secs(1));
        assert_eq!(parse_duration("30s").unwrap(), Duration::from_secs(30));
        assert_eq!(parse_duration("120s").unwrap(), Duration::from_secs(120));
    }

    #[test]
    fn test_parse_duration_minutes() {
        assert_eq!(parse_duration("1m").unwrap(), Duration::from_secs(60));
        assert_eq!(parse_duration("5m").unwrap(), Duration::from_secs(300));
    }

    #[test]
    fn test_parse_duration_plain_number() {
        assert_eq!(parse_duration("500").unwrap(), Duration::from_millis(500));
        assert_eq!(parse_duration("1000").unwrap(), Duration::from_millis(1000));
    }

    #[test]
    fn test_parse_duration_with_whitespace() {
        assert_eq!(
            parse_duration(" 500ms ").unwrap(),
            Duration::from_millis(500)
        );
        assert_eq!(parse_duration("  1s  ").unwrap(), Duration::from_secs(1));
    }

    #[test]
    fn test_parse_duration_empty() {
        assert!(parse_duration("").is_err());
        assert!(parse_duration("   ").is_err());
    }

    #[test]
    fn test_parse_duration_invalid() {
        assert!(parse_duration("abc").is_err());
        assert!(parse_duration("1x").is_err());
        assert!(parse_duration("ms").is_err());
        assert!(parse_duration("-1s").is_err());
    }
}
