use crate::error::Error;
use std::time::{Duration, Instant};
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
            request_timeout_ms: 30_000,  // 30 seconds
        }
    }
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
    error.status().is_none_or(|status| match status.as_u16() {
        // Client errors (4xx) are generally not retryable except for specific cases
        408 | 429 => true, // Request Timeout, Too Many Requests
        
        // Server errors (5xx) are generally retryable except for specific cases
        500..=599 => !matches!(status.as_u16(), 501 | 505), // Exclude Not Implemented, HTTP Version not supported
        
        _ => false, // All other codes (1xx, 2xx, 3xx, 4xx except 408/429) are not retryable
    })
}

/// Calculates the delay for a given retry attempt with exponential backoff
#[must_use]
#[allow(clippy::cast_precision_loss, clippy::cast_possible_truncation, clippy::cast_sign_loss, clippy::cast_possible_wrap)]
pub fn calculate_retry_delay(config: &RetryConfig, attempt: usize) -> Duration {
    let base_delay = config.initial_delay_ms as f64;
    let attempt_i32 = attempt.min(30) as i32; // Cap attempt to prevent overflow
    let delay_ms = (base_delay * config.backoff_multiplier.powi(attempt_i32))
        .min(config.max_delay_ms as f64);
    
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
    let start_time = Instant::now();
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
                
                if is_last_attempt || !is_retryable {
                    let error_message = error.to_string();
                    last_error = Some(error_message.clone());
                    
                    if !is_retryable {
                        return Err(Error::TransientNetworkError {
                            reason: error_message,
                            retryable: false,
                        });
                    }
                    break;
                }
                
                // Calculate delay and sleep before retry
                let delay = calculate_retry_delay(config, attempt);
                
                sleep(delay).await;
                last_error = Some(error.to_string());
            }
        }
    }
    
    let duration = start_time.elapsed();
    Err(Error::RetryLimitExceeded {
        attempts: config.max_attempts,
        #[allow(clippy::cast_possible_truncation)]
        duration_ms: duration.as_millis().min(u128::from(u64::MAX)) as u64,
        last_error: last_error.unwrap_or_else(|| "Unknown error".to_string()),
    })
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
        .map_err(|e| Error::RequestFailed {
            reason: format!("Failed to create resilient HTTP client: {e}"),
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
}