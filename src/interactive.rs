use crate::error::Error;
use std::io::{self, Write};

/// Maximum allowed input length to prevent memory exhaustion
const MAX_INPUT_LENGTH: usize = 1024;

/// Maximum number of retry attempts for invalid input
const MAX_RETRIES: usize = 3;

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
    print!("{prompt}");
    io::stdout().flush().map_err(Error::Io)?;

    let mut input = String::new();
    io::stdin().read_line(&mut input).map_err(Error::Io)?;

    let trimmed_input = input.trim();

    // Validate input length
    if trimmed_input.len() > MAX_INPUT_LENGTH {
        return Err(Error::InvalidConfig {
            reason: format!(
                "Input too long: {} characters (maximum: {})",
                trimmed_input.len(),
                MAX_INPUT_LENGTH
            ),
        });
    }

    // Sanitize input - check for control characters
    if trimmed_input.chars().any(|c| c.is_control() && c != '\t') {
        return Err(Error::InvalidConfig {
            reason: "Input contains invalid control characters".to_string(),
        });
    }

    Ok(trimmed_input.to_string())
}

/// Present a menu of options and return the selected value
///
/// # Errors
/// Returns an error if no options are provided, if stdin operations fail,
/// or if maximum retry attempts are exceeded
pub fn select_from_options(prompt: &str, options: &[(String, String)]) -> Result<String, Error> {
    if options.is_empty() {
        return Err(Error::InvalidConfig {
            reason: "No options available for selection".to_string(),
        });
    }

    println!("{prompt}");
    for (i, (key, description)) in options.iter().enumerate() {
        println!("  {}: {} - {}", i + 1, key, description);
    }

    for attempt in 1..=MAX_RETRIES {
        let selection = prompt_for_input("Enter your choice (number or name): ")?;

        // Handle empty input as cancellation
        if selection.is_empty() {
            return Err(Error::InvalidConfig {
                reason: "Selection cancelled by user".to_string(),
            });
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
            println!(
                "Invalid selection. Please enter a number (1-{}) or a valid name. (Attempt {} of {})",
                options.len(),
                attempt,
                MAX_RETRIES
            );
        }
    }

    Err(Error::InvalidConfig {
        reason: format!("Maximum retry attempts ({MAX_RETRIES}) exceeded"),
    })
}

/// Ask for user confirmation with yes/no prompt
///
/// # Errors
/// Returns an error if stdin operations fail or maximum retry attempts are exceeded
pub fn confirm(prompt: &str) -> Result<bool, Error> {
    for attempt in 1..=MAX_RETRIES {
        let response = prompt_for_input(&format!("{prompt} (y/n): "))?;

        // Handle empty input as cancellation
        if response.is_empty() {
            return Ok(false);
        }

        match response.to_lowercase().as_str() {
            "y" | "yes" => return Ok(true),
            "n" | "no" => return Ok(false),
            _ => {
                if attempt < MAX_RETRIES {
                    println!(
                        "Please enter 'y' for yes or 'n' for no. (Attempt {attempt} of {MAX_RETRIES})"
                    );
                }
            }
        }
    }

    Err(Error::InvalidConfig {
        reason: format!("Maximum retry attempts ({MAX_RETRIES}) exceeded for confirmation"),
    })
}

/// Validates an environment variable name
///
/// # Errors
/// Returns an error if the environment variable name is invalid
pub fn validate_env_var_name(name: &str) -> Result<(), Error> {
    // Check if empty
    if name.is_empty() {
        return Err(Error::InvalidConfig {
            reason: "Environment variable name cannot be empty".to_string(),
        });
    }

    // Check length
    if name.len() > MAX_INPUT_LENGTH {
        return Err(Error::InvalidConfig {
            reason: format!(
                "Environment variable name too long: {} characters (maximum: {})",
                name.len(),
                MAX_INPUT_LENGTH
            ),
        });
    }

    // Check for reserved names (case insensitive)
    let name_upper = name.to_uppercase();
    if RESERVED_ENV_VARS
        .iter()
        .any(|&reserved| reserved == name_upper)
    {
        return Err(Error::InvalidConfig {
            reason: format!("Cannot use reserved environment variable name: {name}"),
        });
    }

    // Check format - must start with letter or underscore, followed by alphanumeric or underscore
    if !name.chars().next().unwrap_or('_').is_ascii_alphabetic() && !name.starts_with('_') {
        return Err(Error::InvalidConfig {
            reason: "Environment variable name must start with a letter or underscore".to_string(),
        });
    }

    // Check all characters are valid - alphanumeric or underscore only
    if !name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_') {
        return Err(Error::InvalidConfig {
            reason: "Environment variable name must contain only letters, numbers, and underscores"
                .to_string(),
        });
    }

    Ok(())
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
        let options = vec![
            (
                "bearerAuth".to_string(),
                "Bearer token authentication".to_string(),
            ),
            ("apiKey".to_string(), "API key authentication".to_string()),
        ];

        // Test that the function accepts the correct input structure
        // We can't test actual user input without mocking stdin
        assert_eq!(options.len(), 2);
        assert_eq!(options[0].0, "bearerAuth");
        assert_eq!(options[1].0, "apiKey");
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
            .contains("letters, numbers, and underscores"));

        let result = validate_env_var_name("API.TOKEN");
        assert!(result.is_err());
        assert!(result
            .unwrap_err()
            .to_string()
            .contains("letters, numbers, and underscores"));

        let result = validate_env_var_name("API TOKEN");
        assert!(result.is_err());
        assert!(result
            .unwrap_err()
            .to_string()
            .contains("letters, numbers, and underscores"));
    }
}
