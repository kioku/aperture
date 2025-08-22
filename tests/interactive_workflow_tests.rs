#[cfg(test)]
use mockall::mock;
#[cfg(test)]
use mockall::predicate::*;

use aperture_cli::interactive::mock::InputOutput;
use aperture_cli::interactive::{
    confirm_with_io, confirm_with_io_and_timeout, prompt_for_input_with_io,
    prompt_for_input_with_io_and_timeout, select_from_options_with_io, validate_env_var_name,
};
use std::time::Duration;

// Create our own mock for integration tests since the one in the lib is only available in unit tests
mock! {
    pub InputOutputImpl {}

    impl InputOutput for InputOutputImpl {
        fn print(&self, text: &str) -> Result<(), aperture_cli::error::Error>;
        fn println(&self, text: &str) -> Result<(), aperture_cli::error::Error>;
        fn flush(&self) -> Result<(), aperture_cli::error::Error>;
        fn read_line(&self) -> Result<String, aperture_cli::error::Error>;
        fn read_line_with_timeout(&self, timeout: std::time::Duration) -> Result<String, aperture_cli::error::Error>;
    }
}

#[test]
fn test_prompt_for_input_with_valid_input() {
    let mut mock = MockInputOutputImpl::new();

    mock.expect_print()
        .with(eq("Enter name: "))
        .times(1)
        .returning(|_| Ok(()));

    mock.expect_flush().times(1).returning(|| Ok(()));

    mock.expect_read_line_with_timeout()
        .times(1)
        .returning(|_| Ok("test_value\n".to_string()));

    let result = prompt_for_input_with_io("Enter name: ", &mock);
    assert!(result.is_ok());
    assert_eq!(result.unwrap(), "test_value");
}

#[test]
fn test_prompt_for_input_with_too_long_input() {
    let mut mock = MockInputOutputImpl::new();
    let long_input = "a".repeat(1025); // Exceeds MAX_INPUT_LENGTH

    mock.expect_print()
        .with(eq("Enter text: "))
        .times(1)
        .returning(|_| Ok(()));

    mock.expect_flush().times(1).returning(|| Ok(()));

    mock.expect_read_line_with_timeout()
        .times(1)
        .returning(move |_| Ok(format!("{long_input}\n")));

    let result = prompt_for_input_with_io("Enter text: ", &mock);
    assert!(result.is_err());
    assert!(result.unwrap_err().to_string().contains("Input too long"));
}

#[test]
fn test_prompt_for_input_with_control_characters() {
    let mut mock = MockInputOutputImpl::new();

    mock.expect_print()
        .with(eq("Enter text: "))
        .times(1)
        .returning(|_| Ok(()));

    mock.expect_flush().times(1).returning(|| Ok(()));

    mock.expect_read_line_with_timeout()
        .times(1)
        .returning(|_| Ok("test\x00invalid\n".to_string()));

    let result = prompt_for_input_with_io("Enter text: ", &mock);
    assert!(result.is_err());
    let error_msg = result.unwrap_err().to_string();
    assert!(error_msg.contains("Invalid characters") || error_msg.contains("invalid characters"));
}

#[test]
fn test_select_from_options_with_number_selection() {
    let mut mock = MockInputOutputImpl::new();
    let options = vec![
        ("option1".to_string(), "First option".to_string()),
        ("option2".to_string(), "Second option".to_string()),
    ];

    mock.expect_println()
        .with(eq("Choose an option:"))
        .times(1)
        .returning(|_| Ok(()));

    mock.expect_println()
        .with(eq("  1: option1 - First option"))
        .times(1)
        .returning(|_| Ok(()));

    mock.expect_println()
        .with(eq("  2: option2 - Second option"))
        .times(1)
        .returning(|_| Ok(()));

    // For the prompt_for_input_with_io call inside select_from_options_with_io
    mock.expect_print()
        .with(eq("Enter your choice (number or name): "))
        .times(1)
        .returning(|_| Ok(()));

    mock.expect_flush().times(1).returning(|| Ok(()));

    mock.expect_read_line_with_timeout()
        .times(1)
        .returning(|_| Ok("2\n".to_string()));

    let result = select_from_options_with_io("Choose an option:", &options, &mock);
    assert!(result.is_ok());
    assert_eq!(result.unwrap(), "option2");
}

#[test]
fn test_select_from_options_with_name_selection() {
    let mut mock = MockInputOutputImpl::new();
    let options = vec![
        (
            "bearerAuth".to_string(),
            "Bearer token authentication".to_string(),
        ),
        ("apiKey".to_string(), "API key authentication".to_string()),
    ];

    mock.expect_println()
        .with(eq("Choose auth method:"))
        .times(1)
        .returning(|_| Ok(()));

    mock.expect_println()
        .with(eq("  1: bearerAuth - Bearer token authentication"))
        .times(1)
        .returning(|_| Ok(()));

    mock.expect_println()
        .with(eq("  2: apiKey - API key authentication"))
        .times(1)
        .returning(|_| Ok(()));

    // For the prompt_for_input_with_io call
    mock.expect_print()
        .with(eq("Enter your choice (number or name): "))
        .times(1)
        .returning(|_| Ok(()));

    mock.expect_flush().times(1).returning(|| Ok(()));

    mock.expect_read_line_with_timeout()
        .times(1)
        .returning(|_| Ok("apikey\n".to_string())); // case insensitive

    let result = select_from_options_with_io("Choose auth method:", &options, &mock);
    assert!(result.is_ok());
    assert_eq!(result.unwrap(), "apiKey");
}

#[test]
fn test_select_from_options_with_empty_input_and_cancellation() {
    let mut mock = MockInputOutputImpl::new();
    let options = vec![("option1".to_string(), "First option".to_string())];

    mock.expect_println()
        .with(eq("Choose:"))
        .times(1)
        .returning(|_| Ok(()));

    mock.expect_println()
        .with(eq("  1: option1 - First option"))
        .times(1)
        .returning(|_| Ok(()));

    // First attempt - empty input
    mock.expect_print()
        .with(eq("Enter your choice (number or name): "))
        .times(1)
        .returning(|_| Ok(()));

    mock.expect_flush().times(1).returning(|| Ok(()));

    mock.expect_read_line_with_timeout()
        .times(1)
        .returning(|_| Ok("\n".to_string())); // empty input

    // Cancellation confirmation
    mock.expect_print()
        .with(eq(
            "Do you want to continue with the current operation? (y/n): ",
        ))
        .times(1)
        .returning(|_| Ok(()));

    mock.expect_flush().times(1).returning(|| Ok(()));

    mock.expect_read_line_with_timeout()
        .times(1)
        .returning(|_| Ok("n\n".to_string())); // choose to cancel

    let result = select_from_options_with_io("Choose:", &options, &mock);
    assert!(result.is_err());
    assert!(result
        .unwrap_err()
        .to_string()
        .contains("cancelled by user"));
}

#[test]
fn test_select_from_options_with_invalid_input_retry() {
    let mut mock = MockInputOutputImpl::new();
    let options = vec![("option1".to_string(), "First option".to_string())];

    mock.expect_println()
        .with(eq("Choose:"))
        .times(1)
        .returning(|_| Ok(()));

    mock.expect_println()
        .with(eq("  1: option1 - First option"))
        .times(1)
        .returning(|_| Ok(()));

    // First attempt - invalid input
    mock.expect_print()
        .with(eq("Enter your choice (number or name): "))
        .times(1)
        .returning(|_| Ok(()));

    mock.expect_flush().times(1).returning(|| Ok(()));

    mock.expect_read_line_with_timeout()
        .times(1)
        .returning(|_| Ok("invalid\n".to_string()));

    // Error message
    mock.expect_println()
        .with(eq(
            "Invalid selection. Please enter a number (1-1) or a valid name. (Attempt 1 of 3)",
        ))
        .times(1)
        .returning(|_| Ok(()));

    // Second attempt - valid input
    mock.expect_print()
        .with(eq("Enter your choice (number or name): "))
        .times(1)
        .returning(|_| Ok(()));

    mock.expect_flush().times(1).returning(|| Ok(()));

    mock.expect_read_line_with_timeout()
        .times(1)
        .returning(|_| Ok("1\n".to_string()));

    let result = select_from_options_with_io("Choose:", &options, &mock);
    assert!(result.is_ok());
    assert_eq!(result.unwrap(), "option1");
}

#[test]
fn test_confirm_with_yes_response() {
    let mut mock = MockInputOutputImpl::new();

    mock.expect_print()
        .with(eq("Continue? (y/n): "))
        .times(1)
        .returning(|_| Ok(()));

    mock.expect_flush().times(1).returning(|| Ok(()));

    mock.expect_read_line_with_timeout()
        .times(1)
        .returning(|_| Ok("y\n".to_string()));

    let result = confirm_with_io("Continue?", &mock);
    assert!(result.is_ok());
    assert!(result.unwrap());
}

#[test]
fn test_confirm_with_no_response() {
    let mut mock = MockInputOutputImpl::new();

    mock.expect_print()
        .with(eq("Delete file? (y/n): "))
        .times(1)
        .returning(|_| Ok(()));

    mock.expect_flush().times(1).returning(|| Ok(()));

    mock.expect_read_line_with_timeout()
        .times(1)
        .returning(|_| Ok("no\n".to_string()));

    let result = confirm_with_io("Delete file?", &mock);
    assert!(result.is_ok());
    assert!(!result.unwrap());
}

#[test]
fn test_confirm_with_empty_input() {
    let mut mock = MockInputOutputImpl::new();

    mock.expect_print()
        .with(eq("Continue? (y/n): "))
        .times(1)
        .returning(|_| Ok(()));

    mock.expect_flush().times(1).returning(|| Ok(()));

    mock.expect_read_line_with_timeout()
        .times(1)
        .returning(|_| Ok("\n".to_string())); // empty input

    let result = confirm_with_io("Continue?", &mock);
    assert!(result.is_ok());
    assert!(!result.unwrap()); // empty input should default to false
}

#[test]
fn test_confirm_with_invalid_input_retry() {
    let mut mock = MockInputOutputImpl::new();

    // First attempt - invalid input
    mock.expect_print()
        .with(eq("Save changes? (y/n): "))
        .times(1)
        .returning(|_| Ok(()));

    mock.expect_flush().times(1).returning(|| Ok(()));

    mock.expect_read_line_with_timeout()
        .times(1)
        .returning(|_| Ok("maybe\n".to_string()));

    // Error message
    mock.expect_println()
        .with(eq(
            "Please enter 'y' for yes or 'n' for no. (Attempt 1 of 3)",
        ))
        .times(1)
        .returning(|_| Ok(()));

    // Second attempt - valid input
    mock.expect_print()
        .with(eq("Save changes? (y/n): "))
        .times(1)
        .returning(|_| Ok(()));

    mock.expect_flush().times(1).returning(|| Ok(()));

    mock.expect_read_line_with_timeout()
        .times(1)
        .returning(|_| Ok("yes\n".to_string()));

    let result = confirm_with_io("Save changes?", &mock);
    assert!(result.is_ok());
    assert!(result.unwrap());
}

#[test]
fn test_validate_env_var_name_comprehensive() {
    // Valid names
    assert!(validate_env_var_name("API_TOKEN").is_ok());
    assert!(validate_env_var_name("MY_SECRET_123").is_ok());
    assert!(validate_env_var_name("_PRIVATE").is_ok());

    // Invalid names
    assert!(validate_env_var_name("").is_err());
    assert!(validate_env_var_name("123_TOKEN").is_err());
    assert!(validate_env_var_name("API-TOKEN").is_err());
    assert!(validate_env_var_name("PATH").is_err()); // reserved

    // Edge cases
    let long_name = "A".repeat(1025);
    assert!(validate_env_var_name(&long_name).is_err());
}

#[test]
fn test_end_to_end_interactive_workflow() {
    let mut mock = MockInputOutputImpl::new();

    // Simulate a complete interactive session for selecting auth method
    // and confirming the choice
    let options = vec![
        ("bearerAuth".to_string(), "Bearer token".to_string()),
        ("apiKey".to_string(), "API key".to_string()),
    ];

    // Display options
    mock.expect_println()
        .with(eq("Select authentication method:"))
        .times(1)
        .returning(|_| Ok(()));

    mock.expect_println()
        .with(eq("  1: bearerAuth - Bearer token"))
        .times(1)
        .returning(|_| Ok(()));

    mock.expect_println()
        .with(eq("  2: apiKey - API key"))
        .times(1)
        .returning(|_| Ok(()));

    // User selects bearerAuth
    mock.expect_print()
        .with(eq("Enter your choice (number or name): "))
        .times(1)
        .returning(|_| Ok(()));

    mock.expect_flush().times(1).returning(|| Ok(()));

    mock.expect_read_line_with_timeout()
        .times(1)
        .returning(|_| Ok("bearerAuth\n".to_string()));

    // Confirm selection
    mock.expect_print()
        .with(eq("Use bearerAuth authentication? (y/n): "))
        .times(1)
        .returning(|_| Ok(()));

    mock.expect_flush().times(1).returning(|| Ok(()));

    mock.expect_read_line_with_timeout()
        .times(1)
        .returning(|_| Ok("y\n".to_string()));

    // Execute the workflow
    let selected = select_from_options_with_io("Select authentication method:", &options, &mock);
    assert!(selected.is_ok());
    let auth_method = selected.unwrap();
    assert_eq!(auth_method, "bearerAuth");

    let confirmed = confirm_with_io(&format!("Use {auth_method} authentication?"), &mock);
    assert!(confirmed.is_ok());
    assert!(confirmed.unwrap());
}

#[test]
fn test_multiple_retry_attempts_until_max() {
    let mut mock = MockInputOutputImpl::new();
    let options = vec![("valid".to_string(), "Valid option".to_string())];

    // Display options
    mock.expect_println()
        .with(eq("Choose:"))
        .times(1)
        .returning(|_| Ok(()));

    mock.expect_println()
        .with(eq("  1: valid - Valid option"))
        .times(1)
        .returning(|_| Ok(()));

    // Attempt 1 - invalid
    mock.expect_print()
        .with(eq("Enter your choice (number or name): "))
        .times(1)
        .returning(|_| Ok(()));
    mock.expect_flush().times(1).returning(|| Ok(()));
    mock.expect_read_line_with_timeout()
        .times(1)
        .returning(|_| Ok("invalid1\n".to_string()));
    mock.expect_println()
        .with(eq(
            "Invalid selection. Please enter a number (1-1) or a valid name. (Attempt 1 of 3)",
        ))
        .times(1)
        .returning(|_| Ok(()));

    // Attempt 2 - invalid
    mock.expect_print()
        .with(eq("Enter your choice (number or name): "))
        .times(1)
        .returning(|_| Ok(()));
    mock.expect_flush().times(1).returning(|| Ok(()));
    mock.expect_read_line_with_timeout()
        .times(1)
        .returning(|_| Ok("invalid2\n".to_string()));
    mock.expect_println()
        .with(eq(
            "Invalid selection. Please enter a number (1-1) or a valid name. (Attempt 2 of 3)",
        ))
        .times(1)
        .returning(|_| Ok(()));

    // Attempt 3 - invalid (final attempt, no retry message)
    mock.expect_print()
        .with(eq("Enter your choice (number or name): "))
        .times(1)
        .returning(|_| Ok(()));
    mock.expect_flush().times(1).returning(|| Ok(()));
    mock.expect_read_line_with_timeout()
        .times(1)
        .returning(|_| Ok("invalid3\n".to_string()));

    let result = select_from_options_with_io("Choose:", &options, &mock);
    assert!(result.is_err());
    assert!(result
        .unwrap_err()
        .to_string()
        .contains("Maximum retry attempts"));
}

#[test]
fn test_prompt_with_timeout_success() {
    let mut mock = MockInputOutputImpl::new();
    let timeout = Duration::from_secs(5);

    mock.expect_print()
        .with(eq("Enter value: "))
        .times(1)
        .returning(|_| Ok(()));

    mock.expect_flush().times(1).returning(|| Ok(()));

    mock.expect_read_line_with_timeout()
        .with(eq(timeout))
        .times(1)
        .returning(|_| Ok("test_value\n".to_string()));

    let result = prompt_for_input_with_io_and_timeout("Enter value: ", &mock, timeout);
    assert!(result.is_ok());
    assert_eq!(result.unwrap(), "test_value");
}

#[test]
fn test_prompt_with_timeout_failure() {
    let mut mock = MockInputOutputImpl::new();
    let timeout = Duration::from_secs(1);

    mock.expect_print()
        .with(eq("Enter value: "))
        .times(1)
        .returning(|_| Ok(()));

    mock.expect_flush().times(1).returning(|| Ok(()));

    mock.expect_read_line_with_timeout()
        .with(eq(timeout))
        .times(1)
        .returning(|_| Err(aperture_cli::error::Error::interactive_timeout()));

    let result = prompt_for_input_with_io_and_timeout("Enter value: ", &mock, timeout);
    assert!(result.is_err());
    assert!(result.unwrap_err().to_string().contains("timeout"));
}

#[test]
fn test_confirm_with_timeout_success() {
    let mut mock = MockInputOutputImpl::new();
    let timeout = Duration::from_secs(5);

    mock.expect_print()
        .with(eq("Continue? (y/n): "))
        .times(1)
        .returning(|_| Ok(()));

    mock.expect_flush().times(1).returning(|| Ok(()));

    mock.expect_read_line_with_timeout()
        .with(eq(timeout))
        .times(1)
        .returning(|_| Ok("y\n".to_string()));

    let result = confirm_with_io_and_timeout("Continue?", &mock, timeout);
    assert!(result.is_ok());
    assert!(result.unwrap());
}

#[test]
fn test_confirm_with_timeout_failure() {
    let mut mock = MockInputOutputImpl::new();
    let timeout = Duration::from_secs(1);

    mock.expect_print()
        .with(eq("Save? (y/n): "))
        .times(1)
        .returning(|_| Ok(()));

    mock.expect_flush().times(1).returning(|| Ok(()));

    mock.expect_read_line_with_timeout()
        .with(eq(timeout))
        .times(1)
        .returning(|_| Err(aperture_cli::error::Error::interactive_timeout()));

    let result = confirm_with_io_and_timeout("Save?", &mock, timeout);
    assert!(result.is_err());
    assert!(result.unwrap_err().to_string().contains("timeout"));
}
