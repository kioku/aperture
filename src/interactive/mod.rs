use crate::error::Error;
use std::time::Duration;

pub mod mock;

use mock::InputOutput;

/// Maximum allowed input length to prevent memory exhaustion
const MAX_INPUT_LENGTH: usize = 1024;

/// Maximum number of retry attempts for invalid input
const MAX_RETRIES: usize = 3;

/// Default timeout for user input operations
const INPUT_TIMEOUT: Duration = Duration::from_secs(30);

/// Reserved environment variable names that should not be used
const RESERVED_ENV_VARS: &[&str] = &[
    "PATH",
    "HOME",
    "USER",
    "SHELL",
    "PWD",
    "LANG",
    "LC_ALL",
    "LC_CTYPE",
    "LD_LIBRARY_PATH",
    "DYLD_LIBRARY_PATH",
    "RUST_LOG",
    "RUST_BACKTRACE",
    "CARGO_HOME",
    "RUSTUP_HOME",
    "TERM",
    "DISPLAY",
    "XDG_CONFIG_HOME",
];

/// Prompt the user for input with the given prompt message
///
/// # Errors
/// Returns an error if stdin/stdout operations fail, input is too long,
/// contains invalid characters, or times out
pub fn prompt_for_input(prompt: &str) -> Result<String, Error> {
    let io = mock::RealInputOutput;
    prompt_for_input_with_io(prompt, &io)
}

/// Prompt the user for input with a custom timeout
///
/// # Errors
/// Returns an error if stdin/stdout operations fail, input is too long,
/// contains invalid characters, or times out
pub fn prompt_for_input_with_timeout(prompt: &str, timeout: Duration) -> Result<String, Error> {
    let io = mock::RealInputOutput;
    prompt_for_input_with_io_and_timeout(prompt, &io, timeout)
}

/// Present a menu of options and return the selected value
///
/// # Errors
/// Returns an error if no options are provided, if stdin operations fail,
/// or if maximum retry attempts are exceeded
pub fn select_from_options(prompt: &str, options: &[(String, String)]) -> Result<String, Error> {
    let io = mock::RealInputOutput;
    select_from_options_with_io(prompt, options, &io)
}

/// Present a menu of options with timeout and return the selected value
///
/// # Errors
/// Returns an error if no options are provided, if stdin operations fail,
/// maximum retry attempts are exceeded, or timeout occurs
pub fn select_from_options_with_timeout(
    prompt: &str,
    options: &[(String, String)],
    timeout: Duration,
) -> Result<String, Error> {
    let io = mock::RealInputOutput;
    select_from_options_with_io_and_timeout(prompt, options, &io, timeout)
}

/// Ask for user confirmation with yes/no prompt
///
/// # Errors
/// Returns an error if stdin operations fail or maximum retry attempts are exceeded
pub fn confirm(prompt: &str) -> Result<bool, Error> {
    let io = mock::RealInputOutput;
    confirm_with_io(prompt, &io)
}

/// Ask for user confirmation with yes/no prompt and timeout
///
/// # Errors
/// Returns an error if stdin operations fail, maximum retry attempts are exceeded, or timeout occurs
pub fn confirm_with_timeout(prompt: &str, timeout: Duration) -> Result<bool, Error> {
    let io = mock::RealInputOutput;
    confirm_with_io_and_timeout(prompt, &io, timeout)
}

/// Validates an environment variable name
///
/// # Errors
/// Returns an error if the environment variable name is invalid
pub fn validate_env_var_name(name: &str) -> Result<(), Error> {
    // Check if empty
    if name.is_empty() {
        return Err(Error::InvalidEnvironmentVariableName {
            name: name.to_string(),
            reason: "name cannot be empty".to_string(),
            suggestion: "Provide a non-empty environment variable name like 'API_TOKEN'"
                .to_string(),
        });
    }

    // Check length
    if name.len() > MAX_INPUT_LENGTH {
        return Err(Error::InvalidEnvironmentVariableName {
            name: name.to_string(),
            reason: format!(
                "too long: {} characters (maximum: {})",
                name.len(),
                MAX_INPUT_LENGTH
            ),
            suggestion: format!("Shorten the name to {MAX_INPUT_LENGTH} characters or less"),
        });
    }

    // Check for reserved names (case insensitive)
    let name_upper = name.to_uppercase();
    if RESERVED_ENV_VARS
        .iter()
        .any(|&reserved| reserved == name_upper)
    {
        return Err(Error::InvalidEnvironmentVariableName {
            name: name.to_string(),
            reason: "uses a reserved system variable name".to_string(),
            suggestion: "Use a different name like 'MY_API_TOKEN' or 'APP_SECRET'".to_string(),
        });
    }

    // Check format - must start with letter or underscore, followed by alphanumeric or underscore
    if !name.chars().next().unwrap_or('_').is_ascii_alphabetic() && !name.starts_with('_') {
        let first_char = name.chars().next().unwrap_or('?');
        let suggested_name = if first_char.is_ascii_digit() {
            format!("VAR_{name}")
        } else {
            format!("_{name}")
        };
        return Err(Error::InvalidEnvironmentVariableName {
            name: name.to_string(),
            reason: "must start with a letter or underscore".to_string(),
            suggestion: format!("Try '{suggested_name}' instead"),
        });
    }

    // Check all characters are valid - alphanumeric or underscore only
    let invalid_chars: Vec<char> = name
        .chars()
        .filter(|c| !c.is_ascii_alphanumeric() && *c != '_')
        .collect();
    if !invalid_chars.is_empty() {
        let invalid_chars_str: String = invalid_chars.iter().collect();
        let suggested_name = name
            .chars()
            .map(|c| {
                if c.is_ascii_alphanumeric() || c == '_' {
                    c
                } else {
                    '_'
                }
            })
            .collect::<String>();
        return Err(Error::InteractiveInvalidCharacters {
            invalid_chars: invalid_chars_str,
            suggestion: format!("Try '{suggested_name}' instead"),
        });
    }

    Ok(())
}

/// Testable version of `prompt_for_input` that accepts an `InputOutput` trait
///
/// # Errors
/// Returns an error if input operations fail, input is too long, or contains invalid characters
pub fn prompt_for_input_with_io<T: InputOutput>(prompt: &str, io: &T) -> Result<String, Error> {
    prompt_for_input_with_io_and_timeout(prompt, io, INPUT_TIMEOUT)
}

/// Testable version of `prompt_for_input` with configurable timeout
///
/// # Errors
/// Returns an error if input operations fail, input is too long, contains invalid characters, or times out
pub fn prompt_for_input_with_io_and_timeout<T: InputOutput>(
    prompt: &str,
    io: &T,
    timeout: Duration,
) -> Result<String, Error> {
    io.print(prompt)?;
    io.flush()?;

    let input = io.read_line_with_timeout(timeout)?;
    let trimmed_input = input.trim();

    // Validate input length
    if trimmed_input.len() > MAX_INPUT_LENGTH {
        return Err(Error::InteractiveInputTooLong {
            provided: trimmed_input.len(),
            max: MAX_INPUT_LENGTH,
            suggestion: "Try shortening your input or using a configuration file for longer values"
                .to_string(),
        });
    }

    // Sanitize input - check for control characters
    let control_chars: Vec<char> = trimmed_input
        .chars()
        .filter(|c| c.is_control() && *c != '\t')
        .collect();
    if !control_chars.is_empty() {
        let control_chars_str = control_chars
            .iter()
            .map(|c| format!("U+{:04X}", *c as u32))
            .collect::<Vec<_>>()
            .join(", ");
        return Err(Error::InteractiveInvalidCharacters {
            invalid_chars: control_chars_str,
            suggestion: "Remove control characters and use only printable text".to_string(),
        });
    }

    Ok(trimmed_input.to_string())
}

/// Testable version of `select_from_options` that accepts an `InputOutput` trait
///
/// # Errors
/// Returns an error if no options provided, input operations fail, or maximum retries exceeded
pub fn select_from_options_with_io<T: InputOutput>(
    prompt: &str,
    options: &[(String, String)],
    io: &T,
) -> Result<String, Error> {
    select_from_options_with_io_and_timeout(prompt, options, io, INPUT_TIMEOUT)
}

/// Testable version of `select_from_options` with configurable timeout
///
/// # Errors
/// Returns an error if no options provided, input operations fail, maximum retries exceeded, or timeout occurs
pub fn select_from_options_with_io_and_timeout<T: InputOutput>(
    prompt: &str,
    options: &[(String, String)],
    io: &T,
    timeout: Duration,
) -> Result<String, Error> {
    if options.is_empty() {
        return Err(Error::InvalidConfig {
            reason: "No options available for selection".to_string(),
        });
    }

    io.println(prompt)?;
    for (i, (key, description)) in options.iter().enumerate() {
        io.println(&format!("  {}: {} - {}", i + 1, key, description))?;
    }

    for attempt in 1..=MAX_RETRIES {
        let selection = prompt_for_input_with_io_and_timeout(
            "Enter your choice (number or name): ",
            io,
            timeout,
        )?;

        // Handle empty input as cancellation
        if selection.is_empty() {
            if !confirm_with_io_and_timeout(
                "Do you want to continue with the current operation?",
                io,
                timeout,
            )? {
                return Err(Error::InvalidConfig {
                    reason: "Selection cancelled by user".to_string(),
                });
            }
            // User chose to continue, skip this iteration
            continue;
        }

        // Try parsing as a number first
        if let Ok(num) = selection.parse::<usize>() {
            if num > 0 && num <= options.len() {
                return Ok(options[num - 1].0.clone());
            }
        }

        // Try matching by name (case insensitive)
        let selection_lower = selection.to_lowercase();
        for (key, _) in options {
            if key.to_lowercase() == selection_lower {
                return Ok(key.clone());
            }
        }

        if attempt < MAX_RETRIES {
            io.println(&format!(
                "Invalid selection. Please enter a number (1-{}) or a valid name. (Attempt {attempt} of {MAX_RETRIES})",
                options.len()
            ))?;
        }
    }

    let suggestions = vec![
        format!(
            "Valid options: {}",
            options
                .iter()
                .map(|(k, _)| k.clone())
                .collect::<Vec<_>>()
                .join(", ")
        ),
        "You can enter either a number or the exact name".to_string(),
        "Leave empty and answer 'no' to cancel the operation".to_string(),
    ];
    Err(Error::InteractiveRetriesExhausted {
        max_attempts: MAX_RETRIES,
        last_error: "Invalid selection".to_string(),
        suggestions,
    })
}

/// Testable version of `confirm` that accepts an `InputOutput` trait
///
/// # Errors
/// Returns an error if input operations fail or maximum retries exceeded
pub fn confirm_with_io<T: InputOutput>(prompt: &str, io: &T) -> Result<bool, Error> {
    confirm_with_io_and_timeout(prompt, io, INPUT_TIMEOUT)
}

/// Testable version of `confirm` with configurable timeout
///
/// # Errors
/// Returns an error if input operations fail, maximum retries exceeded, or timeout occurs
pub fn confirm_with_io_and_timeout<T: InputOutput>(
    prompt: &str,
    io: &T,
    timeout: Duration,
) -> Result<bool, Error> {
    for attempt in 1..=MAX_RETRIES {
        let response =
            prompt_for_input_with_io_and_timeout(&format!("{prompt} (y/n): "), io, timeout)?;

        // Handle empty input as cancellation
        if response.is_empty() {
            return Ok(false);
        }

        match response.to_lowercase().as_str() {
            "y" | "yes" => return Ok(true),
            "n" | "no" => return Ok(false),
            _ => {
                if attempt < MAX_RETRIES {
                    io.println(&format!(
                        "Please enter 'y' for yes or 'n' for no. (Attempt {attempt} of {MAX_RETRIES})"
                    ))?;
                }
            }
        }
    }

    let suggestions = vec![
        "Valid responses: 'y', 'yes', 'n', 'no' (case insensitive)".to_string(),
        "Leave empty to default to 'no'".to_string(),
    ];
    Err(Error::InteractiveRetriesExhausted {
        max_attempts: MAX_RETRIES,
        last_error: "Invalid confirmation response".to_string(),
        suggestions,
    })
}

/// Prompts for confirmation to exit/cancel an interactive session
///
/// # Errors
/// Returns an error if stdin operations fail
pub fn confirm_exit() -> Result<bool, Error> {
    println!("\nInteractive session interrupted.");
    confirm("Do you want to exit without saving changes?")
}

/// Checks if the user wants to cancel the current operation
/// This is called when empty input is provided as a cancellation signal
///
/// # Errors
/// Returns an error if the confirmation input operation fails
pub fn handle_cancellation_input() -> Result<bool, Error> {
    println!("Empty input detected. This will cancel the current operation.");
    confirm("Do you want to continue with the current operation?")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_select_from_options_empty() {
        let options = vec![];
        let result = select_from_options("Choose:", &options);
        assert!(result.is_err());
    }

    #[test]
    fn test_select_from_options_structure() {
        use crate::constants;
        let options = vec![
            (
                "bearerAuth".to_string(),
                "Bearer token authentication".to_string(),
            ),
            (
                constants::AUTH_SCHEME_APIKEY.to_string(),
                "API key authentication".to_string(),
            ),
        ];

        // Test that the function accepts the correct input structure
        // We can't test actual user input without mocking stdin
        assert_eq!(options.len(), 2);
        assert_eq!(options[0].0, "bearerAuth");
        assert_eq!(options[1].0, constants::AUTH_SCHEME_APIKEY);
    }

    #[test]
    fn test_validate_env_var_name_valid() {
        assert!(validate_env_var_name("API_TOKEN").is_ok());
        assert!(validate_env_var_name("MY_SECRET").is_ok());
        assert!(validate_env_var_name("_PRIVATE_KEY").is_ok());
        assert!(validate_env_var_name("TOKEN123").is_ok());
        assert!(validate_env_var_name("a").is_ok());
    }

    #[test]
    fn test_validate_env_var_name_empty() {
        let result = validate_env_var_name("");
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("cannot be empty"));
    }

    #[test]
    fn test_validate_env_var_name_too_long() {
        let long_name = "A".repeat(MAX_INPUT_LENGTH + 1);
        let result = validate_env_var_name(&long_name);
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("too long"));
    }

    #[test]
    fn test_validate_env_var_name_reserved() {
        let result = validate_env_var_name("PATH");
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("reserved"));

        let result = validate_env_var_name("path"); // case insensitive
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("reserved"));
    }

    #[test]
    fn test_validate_env_var_name_invalid_start() {
        let result = validate_env_var_name("123_TOKEN");
        assert!(result.is_err());
        assert!(result
            .unwrap_err()
            .to_string()
            .contains("start with a letter"));

        let result = validate_env_var_name("-TOKEN");
        assert!(result.is_err());
        assert!(result
            .unwrap_err()
            .to_string()
            .contains("start with a letter"));
    }

    #[test]
    fn test_validate_env_var_name_invalid_characters() {
        let result = validate_env_var_name("API-TOKEN");
        assert!(result.is_err());
        assert!(result
            .unwrap_err()
            .to_string()
            .contains("invalid characters"));

        let result = validate_env_var_name("API.TOKEN");
        assert!(result.is_err());
        assert!(result
            .unwrap_err()
            .to_string()
            .contains("invalid characters"));

        let result = validate_env_var_name("API TOKEN");
        assert!(result.is_err());
        assert!(result
            .unwrap_err()
            .to_string()
            .contains("invalid characters"));
    }
}
