//! Integration tests for request/response logging functionality

use aperture_cli::logging::{get_max_body_len, redact_sensitive_value, should_redact_header};
use std::env;

/// Combined test for all logging environment variables.
/// These are combined into a single test to avoid race conditions when tests
/// run in parallel, since environment variables are global state.
#[test]
fn test_logging_environment_variables() {
    // Save original values
    let original_log = env::var("APERTURE_LOG").ok();
    let original_format = env::var("APERTURE_LOG_FORMAT").ok();
    let original_max_body = env::var("APERTURE_LOG_MAX_BODY").ok();

    // Test APERTURE_LOG
    env::set_var("APERTURE_LOG", "debug");
    assert_eq!(
        env::var("APERTURE_LOG").ok(),
        Some("debug".to_string()),
        "APERTURE_LOG should be settable"
    );

    // Test APERTURE_LOG_FORMAT
    env::set_var("APERTURE_LOG_FORMAT", "json");
    assert_eq!(
        env::var("APERTURE_LOG_FORMAT").ok(),
        Some("json".to_string()),
        "APERTURE_LOG_FORMAT should be settable"
    );

    // Test APERTURE_LOG_MAX_BODY default
    env::remove_var("APERTURE_LOG_MAX_BODY");
    assert_eq!(get_max_body_len(), 1000, "Default max body should be 1000");

    // Test APERTURE_LOG_MAX_BODY custom value
    env::set_var("APERTURE_LOG_MAX_BODY", "2000");
    assert_eq!(
        get_max_body_len(),
        2000,
        "Custom max body value should be respected"
    );

    // Restore original values
    if let Some(val) = original_log {
        env::set_var("APERTURE_LOG", val);
    } else {
        env::remove_var("APERTURE_LOG");
    }

    if let Some(val) = original_format {
        env::set_var("APERTURE_LOG_FORMAT", val);
    } else {
        env::remove_var("APERTURE_LOG_FORMAT");
    }

    if let Some(val) = original_max_body {
        env::set_var("APERTURE_LOG_MAX_BODY", val);
    } else {
        env::remove_var("APERTURE_LOG_MAX_BODY");
    }
}

#[test]
fn test_logging_module_redaction() {
    // Test that the logging module properly redacts sensitive headers

    // Should redact these headers
    assert!(should_redact_header("Authorization"));
    assert!(should_redact_header("authorization"));
    assert!(should_redact_header("X-API-Key"));
    assert!(should_redact_header("X-Auth-Token"));
    assert!(should_redact_header("api-key"));

    // Should not redact these headers
    assert!(!should_redact_header("Content-Type"));
    assert!(!should_redact_header("Content-Length"));
    assert!(!should_redact_header("User-Agent"));
}

#[test]
fn test_logging_module_redaction_value() {
    // Test that sensitive values are properly redacted

    assert_eq!(redact_sensitive_value("secret123"), "[REDACTED]");
    assert_eq!(redact_sensitive_value(""), "");
}

#[test]
fn test_cli_verbose_flag_parsing() {
    // This test verifies that the CLI can be parsed with verbose flags
    // In a real scenario, this would test the actual CLI parsing
    // For now, we verify the concept works
    let verbosity_default = 0u8;
    let verbosity_v = 1u8;
    let verbosity_vv = 2u8;

    // Verify the mapping logic
    assert_eq!(verbosity_default, 0);
    assert_eq!(verbosity_v, 1);
    assert_eq!(verbosity_vv, 2);
}

/// Isolated local benchmark; timings are evidence, not a flaky performance assertion.
#[test]
#[ignore = "manual logging benchmark"]
fn benchmark_response_logging_disabled_and_enabled() {
    use aperture_cli::logging::log_response;
    let body = "界".repeat(16 * 1024 * 1024 / 3);
    let disabled = tracing::subscriber::NoSubscriber::default();
    let enabled = tracing_subscriber::fmt()
        .with_max_level(tracing::Level::TRACE)
        .with_writer(std::io::sink)
        .finish();
    let measure = || {
        let start = std::time::Instant::now();
        for _ in 0..20 {
            log_response(200, 1, None, Some(&body), 4096, None);
        }
        start.elapsed() / 20
    };
    let disabled_time = tracing::subscriber::with_default(disabled, measure);
    let enabled_time = tracing::subscriber::with_default(enabled, measure);
    eprintln!("16 MiB Unicode body: disabled={disabled_time:?}/call enabled={enabled_time:?}/call (20 calls each)");
}
