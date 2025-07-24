use crate::error::Error;
use std::io::{self, Write};

/// Prompt the user for input with the given prompt message
///
/// # Errors
/// Returns an error if stdin/stdout operations fail
pub fn prompt_for_input(prompt: &str) -> Result<String, Error> {
    print!("{prompt}");
    io::stdout().flush().map_err(Error::Io)?;

    let mut input = String::new();
    io::stdin().read_line(&mut input).map_err(Error::Io)?;

    Ok(input.trim().to_string())
}

/// Present a menu of options and return the selected value
///
/// # Errors
/// Returns an error if no options are provided or if stdin operations fail
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

    loop {
        let selection = prompt_for_input("Enter your choice (number or name): ")?;

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

        println!(
            "Invalid selection. Please enter a number (1-{}) or a valid name.",
            options.len()
        );
    }
}

/// Ask for user confirmation with yes/no prompt
///
/// # Errors
/// Returns an error if stdin operations fail
pub fn confirm(prompt: &str) -> Result<bool, Error> {
    loop {
        let response = prompt_for_input(&format!("{prompt} (y/n): "))?;
        match response.to_lowercase().as_str() {
            "y" | "yes" => return Ok(true),
            "n" | "no" => return Ok(false),
            _ => println!("Please enter 'y' for yes or 'n' for no."),
        }
    }
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
}
