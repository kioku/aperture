use crate::error::Error;
use reqwest::header::HeaderMap;
use std::time::{Duration, Instant, SystemTime};
use tokio::time::sleep;

/// Configuration for retry behavior
#[derive(Debug, Clone)]
pub struct RetryConfig {
    pub max_attempts: usize,
    pub initial_delay_ms: u64,
    pub max_delay_ms: u64,
    pub backoff_multiplier: f64,
    pub jitter: bool,
}

impl Default for RetryConfig {
    fn default() -> Self {
        Self {
            max_attempts: 3,
            initial_delay_ms: 100,
            max_delay_ms: 5000,
            backoff_multiplier: 2.0,
            jitter: true,
        }
    }
}

/// Configuration for timeout behavior
#[derive(Debug, Clone)]
pub struct TimeoutConfig {
    pub connect_timeout_ms: u64,
    pub request_timeout_ms: u64,
}

impl Default for TimeoutConfig {
    fn default() -> Self {
        Self {
            connect_timeout_ms: 10_000, // 10 seconds
            request_timeout_ms: 30_000, // 30 seconds
        }
    }
}

/// Information about a single retry attempt for logging and error reporting.
#[derive(Debug, Clone)]
pub struct RetryInfo {
    /// The retry attempt number (1-indexed)
    pub attempt: u32,
    /// The HTTP status code that triggered the retry, if available
    pub status_code: Option<u16>,
    /// The delay in milliseconds before this retry
    pub delay_ms: u64,
    /// Human-readable reason for the retry
    pub reason: String,
}

impl RetryInfo {
    /// Creates a new `RetryInfo` instance.
    #[must_use]
    pub fn new(
        attempt: u32,
        status_code: Option<u16>,
        delay_ms: u64,
        reason: impl Into<String>,
    ) -> Self {
        Self {
            attempt,
            status_code,
            delay_ms,
            reason: reason.into(),
        }
    }
}

/// Result of a retry operation, including retry history for diagnostics.
#[derive(Debug)]
pub struct RetryResult<T> {
    /// The successful result, if any
    pub result: Result<T, Error>,
    /// History of retry attempts (empty if succeeded on first try)
    pub retry_history: Vec<RetryInfo>,
    /// Total number of attempts made (including the final one)
    pub total_attempts: u32,
}

/// Parses the `Retry-After` HTTP header and returns the delay duration.
///
/// The `Retry-After` header can be specified in two formats:
/// - Delay in seconds: `Retry-After: 120`
/// - HTTP-date: `Retry-After: Wed, 21 Oct 2015 07:28:00 GMT`
///
/// Returns `None` if the header is absent, malformed, or represents a time in the past.
#[must_use]
pub fn parse_retry_after_header(headers: &HeaderMap) -> Option<Duration> {
    let retry_after = headers.get("retry-after")?;
    let value = retry_after.to_str().ok()?;
    parse_retry_after_value(value)
}

/// Parses a `Retry-After` header value string and returns the delay duration.
///
/// This is the core parsing logic that can be used with any string source.
/// Supports two formats:
/// - Delay in seconds: `"120"`
/// - HTTP-date: `"Wed, 21 Oct 2015 07:28:00 GMT"`
///
/// Returns `None` if the value is malformed or represents a time in the past.
#[must_use]
pub fn parse_retry_after_value(value: &str) -> Option<Duration> {
    // Try parsing as seconds first (most common)
    if let Ok(seconds) = value.parse::<u64>() {
        return Some(Duration::from_secs(seconds));
    }

    // Try parsing as HTTP-date (RFC 7231 format)
    // Format: "Wed, 21 Oct 2015 07:28:00 GMT"
    if let Ok(date) = httpdate::parse_http_date(value) {
        let now = SystemTime::now();
        if let Ok(duration) = date.duration_since(now) {
            return Some(duration);
        }
        // Date is in the past, return None
        return None;
    }

    None
}

/// Calculates the retry delay, respecting an optional `Retry-After` header value.
///
/// If `retry_after` is provided and greater than the calculated exponential backoff delay,
/// the `retry_after` value is used instead (still capped at `max_delay_ms`).
#[must_use]
#[allow(
    clippy::cast_precision_loss,
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    clippy::cast_possible_wrap
)]
pub fn calculate_retry_delay_with_header(
    config: &RetryConfig,
    attempt: usize,
    retry_after: Option<Duration>,
) -> Duration {
    let calculated_delay = calculate_retry_delay(config, attempt);

    retry_after.map_or(calculated_delay, |server_delay| {
        // Use server-specified delay if it's longer than our calculated delay
        let delay = calculated_delay.max(server_delay);
        // But cap it at max_delay_ms
        let max_delay = Duration::from_millis(config.max_delay_ms);
        delay.min(max_delay)
    })
}

/// Determines if an error is retryable based on its characteristics
#[must_use]
pub fn is_retryable_error(error: &reqwest::Error) -> bool {
    // Connection errors are usually retryable
    if error.is_connect() {
        return true;
    }

    // Timeout errors are retryable
    if error.is_timeout() {
        return true;
    }

    // Check HTTP status codes
    error
        .status()
        .is_none_or(|status| is_retryable_status(status.as_u16()))
}

/// Determines if an HTTP status code is retryable.
///
/// Retryable status codes:
/// - 408 Request Timeout
/// - 429 Too Many Requests
/// - 500-599 Server errors (except 501 Not Implemented, 505 HTTP Version Not Supported)
#[must_use]
pub const fn is_retryable_status(status: u16) -> bool {
    match status {
        // Client errors (4xx) are generally not retryable except for specific cases
        408 | 429 => true, // Request Timeout, Too Many Requests

        // Server errors (5xx) are generally retryable except for specific cases
        500..=599 => !matches!(status, 501 | 505), // Exclude Not Implemented, HTTP Version not supported

        _ => false, // All other codes (1xx, 2xx, 3xx, 4xx except 408/429) are not retryable
    }
}

/// Calculates the delay for a given retry attempt with exponential backoff
#[must_use]
#[allow(
    clippy::cast_precision_loss,
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    clippy::cast_possible_wrap
)]
pub fn calculate_retry_delay(config: &RetryConfig, attempt: usize) -> Duration {
    let base_delay = config.initial_delay_ms as f64;
    let attempt_i32 = attempt.min(30) as i32; // Cap attempt to prevent overflow
    let delay_ms =
        (base_delay * config.backoff_multiplier.powi(attempt_i32)).min(config.max_delay_ms as f64);

    let final_delay_ms = if config.jitter {
        // Add up to 25% jitter to prevent thundering herd
        let jitter_factor = fastrand::f64().mul_add(0.25, 1.0);
        delay_ms * jitter_factor
    } else {
        delay_ms
    } as u64;

    Duration::from_millis(final_delay_ms)
}

/// Executes a future with retry logic based on the configuration
///
/// # Errors
/// Returns an error if all retry attempts fail or if a non-retryable error occurs
pub async fn execute_with_retry<F, Fut, T>(
    config: &RetryConfig,
    _operation_name: &str,
    mut operation: F,
) -> Result<T, Error>
where
    F: FnMut() -> Fut,
    Fut: std::future::Future<Output = Result<T, reqwest::Error>>,
{
    let _start_time = Instant::now();
    let mut last_error = None;

    for attempt in 0..config.max_attempts {
        match operation().await {
            Ok(result) => {
                // Successfully completed operation
                return Ok(result);
            }
            Err(error) => {
                let is_last_attempt = attempt + 1 >= config.max_attempts;
                let is_retryable = is_retryable_error(&error);

                // Handle non-retryable errors immediately
                if !is_retryable {
                    let error_message = error.to_string();
                    return Err(Error::transient_network_error(error_message, false));
                }

                // Handle last attempt
                if is_last_attempt {
                    let error_message = error.to_string();
                    last_error = Some(error_message);
                    break;
                }

                // Calculate delay and sleep before retry
                let delay = calculate_retry_delay(config, attempt);

                sleep(delay).await;
                last_error = Some(error.to_string());
            }
        }
    }

    Err(Error::retry_limit_exceeded(
        config.max_attempts.try_into().unwrap_or(u32::MAX),
        last_error.unwrap_or_else(|| "Unknown error".to_string()),
    ))
}

/// Executes a future with retry logic, tracking all retry attempts for diagnostics.
///
/// Unlike `execute_with_retry`, this function returns a `RetryResult` that includes
/// the full retry history, useful for logging and structured error reporting.
#[allow(clippy::cast_possible_truncation)]
pub async fn execute_with_retry_tracking<F, Fut, T>(
    config: &RetryConfig,
    operation_name: &str,
    mut operation: F,
) -> RetryResult<T>
where
    F: FnMut() -> Fut,
    Fut: std::future::Future<Output = Result<T, reqwest::Error>>,
{
    let mut retry_history = Vec::new();
    let mut last_error = None;

    for attempt in 0..config.max_attempts {
        match operation().await {
            Ok(result) => {
                return RetryResult {
                    result: Ok(result),
                    retry_history,
                    total_attempts: (attempt + 1) as u32,
                };
            }
            Err(error) => {
                let is_last_attempt = attempt + 1 >= config.max_attempts;
                let is_retryable = is_retryable_error(&error);
                let status_code = error.status().map(|s| s.as_u16());
                let error_message = error.to_string();

                // Handle non-retryable errors immediately
                if !is_retryable {
                    return RetryResult {
                        result: Err(Error::transient_network_error(error_message, false)),
                        retry_history,
                        total_attempts: (attempt + 1) as u32,
                    };
                }

                // Handle last attempt
                if is_last_attempt {
                    last_error = Some(error_message);
                    break;
                }

                // Calculate delay
                let delay = calculate_retry_delay(config, attempt);
                let delay_ms = delay.as_millis() as u64;

                // Record retry info
                retry_history.push(RetryInfo::new(
                    (attempt + 1) as u32,
                    status_code,
                    delay_ms,
                    format!("{operation_name}: {error_message}"),
                ));

                // Sleep before retry
                sleep(delay).await;
                last_error = Some(error_message);
            }
        }
    }

    RetryResult {
        result: Err(Error::retry_limit_exceeded(
            config.max_attempts.try_into().unwrap_or(u32::MAX),
            last_error.unwrap_or_else(|| "Unknown error".to_string()),
        )),
        retry_history,
        total_attempts: config.max_attempts as u32,
    }
}

/// Creates a resilient HTTP client with timeout configuration
///
/// # Errors
/// Returns an error if the HTTP client cannot be created with the specified configuration
pub fn create_resilient_client(timeout_config: &TimeoutConfig) -> Result<reqwest::Client, Error> {
    reqwest::Client::builder()
        .connect_timeout(Duration::from_millis(timeout_config.connect_timeout_ms))
        .timeout(Duration::from_millis(timeout_config.request_timeout_ms))
        .build()
        .map_err(|e| {
            Error::network_request_failed(format!("Failed to create resilient HTTP client: {e}"))
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_calculate_retry_delay() {
        let config = RetryConfig {
            max_attempts: 5,
            initial_delay_ms: 100,
            max_delay_ms: 1000,
            backoff_multiplier: 2.0,
            jitter: false,
        };

        let delay1 = calculate_retry_delay(&config, 0);
        let delay2 = calculate_retry_delay(&config, 1);
        let delay3 = calculate_retry_delay(&config, 2);

        assert_eq!(delay1.as_millis(), 100);
        assert_eq!(delay2.as_millis(), 200);
        assert_eq!(delay3.as_millis(), 400);

        // Test max delay cap
        let delay_max = calculate_retry_delay(&config, 10);
        assert_eq!(delay_max.as_millis(), 1000);
    }

    #[test]
    fn test_calculate_retry_delay_with_jitter() {
        let config = RetryConfig {
            max_attempts: 3,
            initial_delay_ms: 100,
            max_delay_ms: 1000,
            backoff_multiplier: 2.0,
            jitter: true,
        };

        let delay1 = calculate_retry_delay(&config, 0);
        let delay2 = calculate_retry_delay(&config, 0);

        // With jitter, delays should be different most of the time
        // We test that both delays are within expected range
        assert!(delay1.as_millis() >= 100 && delay1.as_millis() <= 125);
        assert!(delay2.as_millis() >= 100 && delay2.as_millis() <= 125);
    }

    #[test]
    fn test_default_configs() {
        let retry_config = RetryConfig::default();
        assert_eq!(retry_config.max_attempts, 3);
        assert_eq!(retry_config.initial_delay_ms, 100);

        let timeout_config = TimeoutConfig::default();
        assert_eq!(timeout_config.connect_timeout_ms, 10_000);
        assert_eq!(timeout_config.request_timeout_ms, 30_000);
    }

    #[test]
    fn test_parse_retry_after_header_seconds() {
        let mut headers = HeaderMap::new();
        headers.insert("retry-after", "120".parse().unwrap());

        let duration = parse_retry_after_header(&headers);
        assert_eq!(duration, Some(Duration::from_secs(120)));
    }

    #[test]
    fn test_parse_retry_after_header_zero() {
        let mut headers = HeaderMap::new();
        headers.insert("retry-after", "0".parse().unwrap());

        let duration = parse_retry_after_header(&headers);
        assert_eq!(duration, Some(Duration::from_secs(0)));
    }

    #[test]
    fn test_parse_retry_after_header_missing() {
        let headers = HeaderMap::new();

        let duration = parse_retry_after_header(&headers);
        assert_eq!(duration, None);
    }

    #[test]
    fn test_parse_retry_after_header_invalid() {
        let mut headers = HeaderMap::new();
        headers.insert("retry-after", "not-a-number".parse().unwrap());

        let duration = parse_retry_after_header(&headers);
        // Invalid format that's neither a number nor valid HTTP-date
        assert_eq!(duration, None);
    }

    #[test]
    fn test_calculate_retry_delay_with_header_none() {
        let config = RetryConfig {
            max_attempts: 3,
            initial_delay_ms: 100,
            max_delay_ms: 5000,
            backoff_multiplier: 2.0,
            jitter: false,
        };

        let delay = calculate_retry_delay_with_header(&config, 0, None);
        assert_eq!(delay.as_millis(), 100);
    }

    #[test]
    fn test_calculate_retry_delay_with_header_uses_server_delay_when_larger() {
        let config = RetryConfig {
            max_attempts: 3,
            initial_delay_ms: 100,
            max_delay_ms: 5000,
            backoff_multiplier: 2.0,
            jitter: false,
        };

        // Server says wait 3 seconds, which is more than our 100ms
        let retry_after = Some(Duration::from_secs(3));
        let delay = calculate_retry_delay_with_header(&config, 0, retry_after);
        assert_eq!(delay.as_secs(), 3);
    }

    #[test]
    fn test_calculate_retry_delay_with_header_uses_calculated_when_larger() {
        let config = RetryConfig {
            max_attempts: 3,
            initial_delay_ms: 5000,
            max_delay_ms: 30_000,
            backoff_multiplier: 2.0,
            jitter: false,
        };

        // Server says wait 1 second, but our calculated delay is 5 seconds
        let retry_after = Some(Duration::from_secs(1));
        let delay = calculate_retry_delay_with_header(&config, 0, retry_after);
        assert_eq!(delay.as_millis(), 5000);
    }

    #[test]
    fn test_calculate_retry_delay_with_header_caps_at_max() {
        let config = RetryConfig {
            max_attempts: 3,
            initial_delay_ms: 100,
            max_delay_ms: 5000,
            backoff_multiplier: 2.0,
            jitter: false,
        };

        // Server says wait 60 seconds, but we cap at 5 seconds
        let retry_after = Some(Duration::from_secs(60));
        let delay = calculate_retry_delay_with_header(&config, 0, retry_after);
        assert_eq!(delay.as_millis(), 5000);
    }

    #[test]
    fn test_retry_info_new() {
        let info = RetryInfo::new(1, Some(429), 500, "Rate limited");
        assert_eq!(info.attempt, 1);
        assert_eq!(info.status_code, Some(429));
        assert_eq!(info.delay_ms, 500);
        assert_eq!(info.reason, "Rate limited");
    }

    #[test]
    fn test_retry_info_without_status_code() {
        let info = RetryInfo::new(2, None, 1000, "Connection refused");
        assert_eq!(info.attempt, 2);
        assert_eq!(info.status_code, None);
        assert_eq!(info.delay_ms, 1000);
        assert_eq!(info.reason, "Connection refused");
    }

    #[test]
    fn test_retry_result_success_no_retries() {
        let result: RetryResult<i32> = RetryResult {
            result: Ok(42),
            retry_history: vec![],
            total_attempts: 1,
        };
        assert!(result.result.is_ok());
        assert!(result.retry_history.is_empty());
        assert_eq!(result.total_attempts, 1);
    }

    #[test]
    fn test_retry_result_success_after_retries() {
        let result: RetryResult<i32> = RetryResult {
            result: Ok(42),
            retry_history: vec![RetryInfo::new(1, Some(503), 100, "Service unavailable")],
            total_attempts: 2,
        };
        assert!(result.result.is_ok());
        assert_eq!(result.retry_history.len(), 1);
        assert_eq!(result.total_attempts, 2);
    }

    #[test]
    fn test_is_retryable_status_408_request_timeout() {
        assert!(is_retryable_status(408));
    }

    #[test]
    fn test_is_retryable_status_429_too_many_requests() {
        assert!(is_retryable_status(429));
    }

    #[test]
    fn test_is_retryable_status_500_internal_server_error() {
        assert!(is_retryable_status(500));
    }

    #[test]
    fn test_is_retryable_status_502_bad_gateway() {
        assert!(is_retryable_status(502));
    }

    #[test]
    fn test_is_retryable_status_503_service_unavailable() {
        assert!(is_retryable_status(503));
    }

    #[test]
    fn test_is_retryable_status_504_gateway_timeout() {
        assert!(is_retryable_status(504));
    }

    #[test]
    fn test_is_retryable_status_501_not_implemented_not_retryable() {
        // 501 Not Implemented should not be retryable
        assert!(!is_retryable_status(501));
    }

    #[test]
    fn test_is_retryable_status_505_http_version_not_supported_not_retryable() {
        // 505 HTTP Version Not Supported should not be retryable
        assert!(!is_retryable_status(505));
    }

    #[test]
    fn test_is_retryable_status_4xx_not_retryable() {
        // Most 4xx errors should not be retryable
        assert!(!is_retryable_status(400)); // Bad Request
        assert!(!is_retryable_status(401)); // Unauthorized
        assert!(!is_retryable_status(403)); // Forbidden
        assert!(!is_retryable_status(404)); // Not Found
        assert!(!is_retryable_status(405)); // Method Not Allowed
        assert!(!is_retryable_status(422)); // Unprocessable Entity
    }

    #[test]
    fn test_is_retryable_status_2xx_not_retryable() {
        // 2xx success codes should not be retryable
        assert!(!is_retryable_status(200));
        assert!(!is_retryable_status(201));
        assert!(!is_retryable_status(204));
    }

    #[test]
    fn test_is_retryable_status_3xx_not_retryable() {
        // 3xx redirect codes should not be retryable
        assert!(!is_retryable_status(301));
        assert!(!is_retryable_status(302));
        assert!(!is_retryable_status(304));
    }
}
