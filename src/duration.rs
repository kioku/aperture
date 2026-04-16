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

    if let Some(duration) = parse_duration_with_suffixes(s)? {
        return Ok(duration);
    }

    Ok(Duration::from_millis(parse_plain_millis(s)?))
}

fn parse_duration_with_suffixes(value: &str) -> Result<Option<Duration>, Error> {
    if let Some(duration) =
        parse_duration_suffix(value, "ms", "milliseconds", Duration::from_millis)?
    {
        return Ok(Some(duration));
    }

    if let Some(duration) = parse_duration_suffix(value, "m", "minutes", |minutes| {
        Duration::from_secs(minutes * 60)
    })? {
        return Ok(Some(duration));
    }

    parse_duration_suffix(value, "s", "seconds", Duration::from_secs)
}

fn parse_duration_suffix(
    value: &str,
    suffix: &str,
    label: &str,
    build: impl Fn(u64) -> Duration,
) -> Result<Option<Duration>, Error> {
    let Some(raw) = value.strip_suffix(suffix) else {
        return Ok(None);
    };

    let amount: u64 = raw
        .trim()
        .parse()
        .map_err(|_| Error::invalid_config(format!("Invalid {label} value: {raw}")))?;
    Ok(Some(build(amount)))
}

fn parse_plain_millis(value: &str) -> Result<u64, Error> {
    value.parse().map_err(|_| {
        Error::invalid_config(format!(
            "Invalid duration format: {value}. Use format like '500ms', '1s', '30s', or '1m'"
        ))
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_duration_milliseconds() {
        assert_eq!(parse_duration("100ms").unwrap(), Duration::from_millis(100));
        assert_eq!(parse_duration("500ms").unwrap(), Duration::from_millis(500));
        assert_eq!(parse_duration("1000ms").unwrap(), Duration::from_secs(1));
    }

    #[test]
    fn test_parse_duration_seconds() {
        assert_eq!(parse_duration("1s").unwrap(), Duration::from_secs(1));
        assert_eq!(parse_duration("30s").unwrap(), Duration::from_secs(30));
        assert_eq!(parse_duration("120s").unwrap(), Duration::from_mins(2));
    }

    #[test]
    fn test_parse_duration_minutes() {
        assert_eq!(parse_duration("1m").unwrap(), Duration::from_mins(1));
        assert_eq!(parse_duration("5m").unwrap(), Duration::from_mins(5));
    }

    #[test]
    fn test_parse_duration_plain_number() {
        assert_eq!(parse_duration("500").unwrap(), Duration::from_millis(500));
        assert_eq!(parse_duration("1000").unwrap(), Duration::from_secs(1));
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
