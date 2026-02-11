//! Validated API context name type.
//!
//! This module provides `ApiContextName`, a newtype wrapper around `String`
//! that enforces strict naming rules to prevent path traversal attacks and
//! invalid filesystem states when API names are used to construct file paths.

use crate::error::Error;
use std::fmt;

/// Maximum allowed length for an API context name.
const MAX_NAME_LENGTH: usize = 64;

/// A validated API context name.
///
/// API context names are used to construct file paths for spec storage and
/// caching, so they must be strictly validated to prevent path traversal.
///
/// # Naming Rules
///
/// - Must start with an ASCII letter or digit
/// - May contain only ASCII letters, digits, dots (`.`), hyphens (`-`), or underscores (`_`)
/// - Maximum length: 64 characters
/// - No path separators, no leading dots, no whitespace
///
/// # Examples
///
/// ```
/// use aperture_cli::config::context_name::ApiContextName;
///
/// // Valid names
/// assert!(ApiContextName::new("my-api").is_ok());
/// assert!(ApiContextName::new("api_v2").is_ok());
/// assert!(ApiContextName::new("foo.bar").is_ok());
/// assert!(ApiContextName::new("API123").is_ok());
///
/// // Invalid names
/// assert!(ApiContextName::new("../foo").is_err());
/// assert!(ApiContextName::new("foo/bar").is_err());
/// assert!(ApiContextName::new(".hidden").is_err());
/// assert!(ApiContextName::new("").is_err());
/// ```
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct ApiContextName(String);

impl ApiContextName {
    /// Creates a new validated `ApiContextName`.
    ///
    /// # Errors
    ///
    /// Returns an error if the name:
    /// - Is empty
    /// - Exceeds 64 characters
    /// - Does not start with an ASCII letter or digit
    /// - Contains characters other than ASCII letters, digits, `.`, `-`, or `_`
    pub fn new(name: &str) -> Result<Self, Error> {
        if name.is_empty() {
            return Err(Error::invalid_api_context_name(
                name,
                "name cannot be empty",
            ));
        }

        if name.len() > MAX_NAME_LENGTH {
            return Err(Error::invalid_api_context_name(
                name,
                format!(
                    "name exceeds maximum length of {MAX_NAME_LENGTH} characters ({} given)",
                    name.len()
                ),
            ));
        }

        // First character must be ASCII alphanumeric.
        // Safety: we verified `name` is non-empty above.
        let first = name.as_bytes()[0];
        if !first.is_ascii_alphanumeric() {
            return Err(Error::invalid_api_context_name(
                name,
                "name must start with an ASCII letter or digit",
            ));
        }

        // All characters must be ASCII alphanumeric, dot, hyphen, or underscore
        if let Some(invalid) = name
            .chars()
            .find(|c| !c.is_ascii_alphanumeric() && *c != '.' && *c != '-' && *c != '_')
        {
            return Err(Error::invalid_api_context_name(
                name,
                format!("contains invalid character '{invalid}'"),
            ));
        }

        Ok(Self(name.to_string()))
    }

    /// Returns the validated name as a string slice.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl AsRef<str> for ApiContextName {
    fn as_ref(&self) -> &str {
        &self.0
    }
}

impl std::ops::Deref for ApiContextName {
    type Target = str;

    fn deref(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for ApiContextName {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}
