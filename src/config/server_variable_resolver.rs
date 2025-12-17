use crate::cache::models::{CachedSpec, ServerVariable};
use crate::error::Error;
use std::collections::HashMap;

/// Resolves server template variables by parsing CLI arguments and applying validation
pub struct ServerVariableResolver<'a> {
    spec: &'a CachedSpec,
}

impl<'a> ServerVariableResolver<'a> {
    /// Creates a new resolver for the given spec
    #[must_use]
    pub const fn new(spec: &'a CachedSpec) -> Self {
        Self { spec }
    }

    /// Parses and validates server variables from CLI arguments
    ///
    /// # Arguments
    /// * `server_var_args` - Command line arguments in format "key=value"
    ///
    /// # Returns
    /// * `Ok(HashMap<String, String>)` - Resolved server variables ready for URL substitution
    /// * `Err(Error)` - Validation errors or parsing failures
    ///
    /// # Errors
    /// Returns errors for:
    /// - Invalid key=value format
    /// - Unknown server variables not defined in `OpenAPI` spec
    /// - Enum constraint violations
    /// - Missing required variables (when defaults are not available)
    pub fn resolve_variables(
        &self,
        server_var_args: &[String],
    ) -> Result<HashMap<String, String>, Error> {
        let mut resolved_vars = HashMap::new();

        // Parse CLI arguments
        for arg in server_var_args {
            let (key, value) = Self::parse_key_value(arg)?;
            resolved_vars.insert(key, value);
        }

        // Validate and apply defaults
        let mut final_vars = HashMap::new();

        for (var_name, var_def) in &self.spec.server_variables {
            // Check if user provided a value
            if let Some(provided_value) = resolved_vars.get(var_name) {
                // Validate provided value against enum constraints
                Self::validate_enum_constraint(var_name, provided_value, var_def)?;
                final_vars.insert(var_name.clone(), provided_value.clone());
                continue;
            }

            // Check if there's a default value
            if let Some(default_value) = &var_def.default {
                // Validate default value against enum constraints
                Self::validate_enum_constraint(var_name, default_value, var_def)?;
                // Use default value
                final_vars.insert(var_name.clone(), default_value.clone());
                continue;
            }

            // Required variable with no default - this is an error
            return Err(Error::missing_server_variable(var_name));
        }

        // Check for unknown variables provided by user
        for provided_var in resolved_vars.keys() {
            if !self.spec.server_variables.contains_key(provided_var) {
                return Err(Error::unknown_server_variable(
                    provided_var,
                    &self
                        .spec
                        .server_variables
                        .keys()
                        .cloned()
                        .collect::<Vec<_>>(),
                ));
            }
        }

        Ok(final_vars)
    }

    /// Substitutes server variables in a URL template
    ///
    /// # Arguments
    /// * `url_template` - URL with template variables like `<https://{region}.api.com>`
    /// * `variables` - Resolved variable values from `resolve_variables`
    ///
    /// # Returns
    /// * `Ok(String)` - URL with all variables substituted
    /// * `Err(Error)` - If template contains variables not in the provided map
    ///
    /// # Errors
    /// Returns errors for:
    /// - Unresolved template variables not found in the provided variables map
    /// - Invalid template variable names (malformed or too long)
    pub fn substitute_url(
        &self,
        url_template: &str,
        variables: &HashMap<String, String>,
    ) -> Result<String, Error> {
        let mut result = url_template.to_string();
        let mut start = 0;

        while let Some((open_pos, close_pos)) = find_next_template(&result, start) {
            let var_name = &result[open_pos + 1..close_pos];
            Self::validate_template_variable_name(var_name)?;

            let value = Self::get_variable_value(var_name, variables, url_template)?;

            // Perform minimal URL encoding to preserve URL structure while escaping dangerous characters
            // We don't encode forward slashes as server variables often contain path segments
            let encoded_value = Self::encode_server_variable(value);

            result.replace_range(open_pos..=close_pos, &encoded_value);
            start = open_pos + encoded_value.len();
        }

        Ok(result)
    }

    /// Gets the value for a template variable, returning an error if not found
    fn get_variable_value<'b>(
        var_name: &str,
        variables: &'b HashMap<String, String>,
        url_template: &str,
    ) -> Result<&'b String, Error> {
        variables
            .get(var_name)
            .ok_or_else(|| Error::unresolved_template_variable(var_name, url_template))
    }

    /// Parses a key=value string from CLI arguments
    fn parse_key_value(arg: &str) -> Result<(String, String), Error> {
        let Some(eq_pos) = arg.find('=') else {
            return Err(Error::invalid_server_var_format(
                arg,
                "Expected format: key=value",
            ));
        };

        let key = arg[..eq_pos].trim();
        let value = arg[eq_pos + 1..].trim();

        if key.is_empty() {
            return Err(Error::invalid_server_var_format(arg, "Empty variable name"));
        }

        if value.is_empty() {
            return Err(Error::invalid_server_var_format(
                arg,
                "Empty variable value",
            ));
        }

        Ok((key.to_string(), value.to_string()))
    }

    /// Validates a value against enum constraints if defined
    fn validate_enum_constraint(
        var_name: &str,
        value: &str,
        var_def: &ServerVariable,
    ) -> Result<(), Error> {
        if !var_def.enum_values.is_empty() && !var_def.enum_values.contains(&value.to_string()) {
            return Err(Error::invalid_server_var_value(
                var_name,
                value,
                &var_def.enum_values,
            ));
        }
        Ok(())
    }

    /// Validates a template variable name according to `OpenAPI` identifier rules
    fn validate_template_variable_name(name: &str) -> Result<(), Error> {
        if name.is_empty() {
            return Err(Error::invalid_server_var_format(
                "{}",
                "Empty template variable name",
            ));
        }

        if name.len() > 64 {
            return Err(Error::invalid_server_var_format(
                format!("{{{name}}}"),
                "Template variable name too long (max 64 chars)",
            ));
        }

        // OpenAPI identifier rules: must start with letter or underscore,
        // followed by letters, digits, or underscores
        let mut chars = name.chars();
        let Some(first_char) = chars.next() else {
            return Ok(()); // Already checked for empty above
        };

        if !first_char.is_ascii_alphabetic() && first_char != '_' {
            return Err(Error::invalid_server_var_format(
                format!("{{{name}}}"),
                "Template variable names must start with a letter or underscore",
            ));
        }

        for char in chars {
            if !char.is_ascii_alphanumeric() && char != '_' {
                return Err(Error::invalid_server_var_format(
                    format!("{{{name}}}"),
                    "Template variable names must contain only letters, digits, or underscores",
                ));
            }
        }

        Ok(())
    }

    /// Encodes a server variable value for safe inclusion in URLs
    /// This performs selective encoding to preserve URL structure while escaping problematic characters
    fn encode_server_variable(value: &str) -> String {
        // Characters that should be encoded in server variable values
        // We preserve forward slashes as they're often used in path segments
        // but encode other special characters that could break URL parsing
        value
            .chars()
            .map(|c| match c {
                // Preserve forward slashes and common URL-safe characters
                '/' | '-' | '_' | '.' | '~' => c.to_string(),
                // Encode spaces and other special characters
                ' ' => "%20".to_string(),
                '?' => "%3F".to_string(),
                '#' => "%23".to_string(),
                '[' => "%5B".to_string(),
                ']' => "%5D".to_string(),
                '@' => "%40".to_string(),
                '!' => "%21".to_string(),
                '$' => "%24".to_string(),
                '&' => "%26".to_string(),
                '\'' => "%27".to_string(),
                '(' => "%28".to_string(),
                ')' => "%29".to_string(),
                '*' => "%2A".to_string(),
                '+' => "%2B".to_string(),
                ',' => "%2C".to_string(),
                ';' => "%3B".to_string(),
                '=' => "%3D".to_string(),
                '{' => "%7B".to_string(),
                '}' => "%7D".to_string(),
                // Keep alphanumeric and other unreserved characters as-is
                c if c.is_ascii_alphanumeric() => c.to_string(),
                // Encode any other characters
                c => urlencoding::encode(&c.to_string()).to_string(),
            })
            .collect()
    }
}

/// Finds the next template variable boundaries (opening and closing braces)
fn find_next_template(s: &str, start: usize) -> Option<(usize, usize)> {
    let open_pos = s[start..].find('{').map(|pos| start + pos)?;
    let close_pos = s[open_pos..].find('}').map(|pos| open_pos + pos)?;
    Some((open_pos, close_pos))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cache::models::{CachedSpec, ServerVariable};
    use crate::error::ErrorKind;
    use std::collections::HashMap;

    fn create_test_spec_with_variables() -> CachedSpec {
        let mut server_variables = HashMap::new();

        // Required variable with enum constraint
        server_variables.insert(
            "region".to_string(),
            ServerVariable {
                default: Some("us".to_string()),
                enum_values: vec!["us".to_string(), "eu".to_string(), "ap".to_string()],
                description: Some("API region".to_string()),
            },
        );

        // Required variable without default
        server_variables.insert(
            "env".to_string(),
            ServerVariable {
                default: None,
                enum_values: vec!["dev".to_string(), "staging".to_string(), "prod".to_string()],
                description: Some("Environment".to_string()),
            },
        );

        CachedSpec {
            cache_format_version: crate::cache::models::CACHE_FORMAT_VERSION,
            name: "test-api".to_string(),
            version: "1.0.0".to_string(),
            commands: vec![],
            base_url: Some("https://{region}-{env}.api.example.com".to_string()),
            servers: vec!["https://{region}-{env}.api.example.com".to_string()],
            security_schemes: HashMap::new(),
            skipped_endpoints: vec![],
            server_variables,
        }
    }

    #[test]
    fn test_resolve_variables_with_all_provided() {
        let spec = create_test_spec_with_variables();
        let resolver = ServerVariableResolver::new(&spec);

        let args = vec!["region=eu".to_string(), "env=staging".to_string()];
        let result = resolver.resolve_variables(&args).unwrap();

        assert_eq!(result.get("region"), Some(&"eu".to_string()));
        assert_eq!(result.get("env"), Some(&"staging".to_string()));
    }

    #[test]
    fn test_resolve_variables_with_defaults() {
        let spec = create_test_spec_with_variables();
        let resolver = ServerVariableResolver::new(&spec);

        let args = vec!["env=prod".to_string()]; // Only provide required var, let region use default
        let result = resolver.resolve_variables(&args).unwrap();

        assert_eq!(result.get("region"), Some(&"us".to_string())); // Default value
        assert_eq!(result.get("env"), Some(&"prod".to_string()));
    }

    #[test]
    fn test_invalid_enum_value() {
        let spec = create_test_spec_with_variables();
        let resolver = ServerVariableResolver::new(&spec);

        let args = vec!["region=invalid".to_string(), "env=prod".to_string()];
        let result = resolver.resolve_variables(&args);

        assert!(result.is_err());
        match result.unwrap_err() {
            Error::Internal {
                kind: ErrorKind::ServerVariable,
                message,
                ..
            } => {
                assert!(message.contains("region") && message.contains("invalid"));
            }
            _ => panic!("Expected Internal ServerVariable error"),
        }
    }

    #[test]
    fn test_missing_required_variable() {
        let spec = create_test_spec_with_variables();
        let resolver = ServerVariableResolver::new(&spec);

        let args = vec!["region=us".to_string()]; // Missing required 'env'
        let result = resolver.resolve_variables(&args);

        assert!(result.is_err());
        match result.unwrap_err() {
            Error::Internal {
                kind: ErrorKind::ServerVariable,
                message,
                ..
            } => {
                assert!(message.contains("env"));
            }
            _ => panic!("Expected Internal ServerVariable error"),
        }
    }

    #[test]
    fn test_unknown_variable() {
        let spec = create_test_spec_with_variables();
        let resolver = ServerVariableResolver::new(&spec);

        let args = vec![
            "region=us".to_string(),
            "env=prod".to_string(),
            "unknown=value".to_string(),
        ];
        let result = resolver.resolve_variables(&args);

        assert!(result.is_err());
        match result.unwrap_err() {
            Error::Internal {
                kind: ErrorKind::ServerVariable,
                message,
                ..
            } => {
                assert!(message.contains("unknown"));
            }
            _ => panic!("Expected Internal ServerVariable error"),
        }
    }

    #[test]
    fn test_invalid_format() {
        let spec = create_test_spec_with_variables();
        let resolver = ServerVariableResolver::new(&spec);

        let args = vec!["invalid-format".to_string()];
        let result = resolver.resolve_variables(&args);

        assert!(result.is_err());
        match result.unwrap_err() {
            Error::Internal {
                kind: ErrorKind::ServerVariable,
                ..
            } => {
                // Expected
            }
            _ => panic!("Expected Internal ServerVariable error"),
        }
    }

    #[test]
    fn test_substitute_url() {
        let spec = create_test_spec_with_variables();
        let resolver = ServerVariableResolver::new(&spec);

        let mut variables = HashMap::new();
        variables.insert("region".to_string(), "eu".to_string());
        variables.insert("env".to_string(), "staging".to_string());

        let result = resolver
            .substitute_url("https://{region}-{env}.api.example.com", &variables)
            .unwrap();
        assert_eq!(result, "https://eu-staging.api.example.com");
    }

    #[test]
    fn test_substitute_url_missing_variable() {
        let spec = create_test_spec_with_variables();
        let resolver = ServerVariableResolver::new(&spec);

        let mut variables = HashMap::new();
        variables.insert("region".to_string(), "eu".to_string());
        // Missing 'env' variable

        let result = resolver.substitute_url("https://{region}-{env}.api.example.com", &variables);

        assert!(result.is_err());
        match result.unwrap_err() {
            Error::Internal {
                kind: ErrorKind::ServerVariable,
                message,
                ..
            } => {
                assert!(message.contains("env"));
            }
            _ => panic!("Expected Internal ServerVariable error"),
        }
    }

    #[test]
    fn test_template_variable_name_validation_empty() {
        let spec = create_test_spec_with_variables();
        let resolver = ServerVariableResolver::new(&spec);

        let variables = HashMap::new();
        let result = resolver.substitute_url("https://{}.api.example.com", &variables);

        assert!(result.is_err());
        match result.unwrap_err() {
            Error::Internal {
                kind: ErrorKind::ServerVariable,
                message,
                ..
            } => {
                assert!(message.contains("Empty template variable name") || message.contains("{}"));
            }
            _ => panic!("Expected Internal ServerVariable error"),
        }
    }

    #[test]
    fn test_template_variable_name_validation_invalid_chars() {
        let spec = create_test_spec_with_variables();
        let resolver = ServerVariableResolver::new(&spec);

        let variables = HashMap::new();
        let result = resolver.substitute_url("https://{invalid-name}.api.example.com", &variables);

        assert!(result.is_err());
        match result.unwrap_err() {
            Error::Internal {
                kind: ErrorKind::ServerVariable,
                message,
                ..
            } => {
                assert!(
                    message.contains("invalid-name")
                        || message.contains("letters, digits, or underscores")
                );
            }
            _ => panic!("Expected Internal ServerVariable error"),
        }
    }

    #[test]
    fn test_template_variable_name_validation_too_long() {
        let spec = create_test_spec_with_variables();
        let resolver = ServerVariableResolver::new(&spec);

        let long_name = "a".repeat(65); // Longer than 64 chars
        let variables = HashMap::new();
        let result = resolver.substitute_url(
            &format!("https://{{{long_name}}}.api.example.com"),
            &variables,
        );

        assert!(result.is_err());
        match result.unwrap_err() {
            Error::Internal {
                kind: ErrorKind::ServerVariable,
                message,
                ..
            } => {
                assert!(message.contains("too long"));
            }
            _ => panic!("Expected Internal ServerVariable error"),
        }
    }

    #[test]
    fn test_template_variable_name_validation_valid_names() {
        let spec = create_test_spec_with_variables();
        let resolver = ServerVariableResolver::new(&spec);

        let mut variables = HashMap::new();
        variables.insert("valid_name".to_string(), "test".to_string());
        variables.insert("_underscore".to_string(), "test".to_string());
        variables.insert("name123".to_string(), "test".to_string());

        // These should all pass validation (though they'll fail with UnresolvedTemplateVariable)
        let test_cases = vec![
            "https://{valid_name}.api.com",
            "https://{_underscore}.api.com",
            "https://{name123}.api.com",
        ];

        for test_case in test_cases {
            let result = resolver.substitute_url(test_case, &variables);
            // Should not fail with InvalidServerVarFormat
            if let Err(Error::Internal {
                kind: ErrorKind::ServerVariable,
                ..
            }) = result
            {
                panic!("Template variable name validation failed for: {test_case}");
            }
        }
    }

    #[test]
    fn test_empty_default_value() {
        let mut server_variables = HashMap::new();

        // Variable with empty string default
        server_variables.insert(
            "prefix".to_string(),
            ServerVariable {
                default: Some(String::new()),
                enum_values: vec![],
                description: Some("Optional prefix".to_string()),
            },
        );

        let spec = CachedSpec {
            cache_format_version: crate::cache::models::CACHE_FORMAT_VERSION,
            name: "test-api".to_string(),
            version: "1.0.0".to_string(),
            commands: vec![],
            base_url: Some("https://{prefix}api.example.com".to_string()),
            servers: vec!["https://{prefix}api.example.com".to_string()],
            security_schemes: HashMap::new(),
            skipped_endpoints: vec![],
            server_variables,
        };

        let resolver = ServerVariableResolver::new(&spec);

        // Test with no args - should use empty string default
        let result = resolver.resolve_variables(&[]).unwrap();
        assert_eq!(result.get("prefix"), Some(&String::new()));

        // Test substitution with empty string default
        let url = resolver
            .substitute_url("https://{prefix}api.example.com", &result)
            .unwrap();
        assert_eq!(url, "https://api.example.com");

        // Test with explicit override
        let args = vec!["prefix=staging-".to_string()];
        let result = resolver.resolve_variables(&args).unwrap();
        assert_eq!(result.get("prefix"), Some(&"staging-".to_string()));

        let url = resolver
            .substitute_url("https://{prefix}api.example.com", &result)
            .unwrap();
        assert_eq!(url, "https://staging-api.example.com");
    }

    #[test]
    fn test_url_encoding_in_substitution() {
        let mut server_variables = HashMap::new();
        server_variables.insert(
            "path".to_string(),
            ServerVariable {
                default: Some("api/v1".to_string()),
                enum_values: vec![],
                description: Some("API path".to_string()),
            },
        );

        let spec = CachedSpec {
            cache_format_version: crate::cache::models::CACHE_FORMAT_VERSION,
            name: "test-api".to_string(),
            version: "1.0.0".to_string(),
            commands: vec![],
            base_url: Some("https://example.com/{path}".to_string()),
            servers: vec!["https://example.com/{path}".to_string()],
            security_schemes: HashMap::new(),
            skipped_endpoints: vec![],
            server_variables,
        };

        let resolver = ServerVariableResolver::new(&spec);

        // Test with value containing special characters
        let args = vec!["path=api/v2/test&debug=true".to_string()];
        let result = resolver.resolve_variables(&args).unwrap();

        let url = resolver
            .substitute_url("https://example.com/{path}", &result)
            .unwrap();

        // The ampersand and equals sign should be URL-encoded, but forward slashes preserved
        assert_eq!(url, "https://example.com/api/v2/test%26debug%3Dtrue");

        // Test with spaces
        let args = vec!["path=api/test endpoint".to_string()];
        let result = resolver.resolve_variables(&args).unwrap();

        let url = resolver
            .substitute_url("https://example.com/{path}", &result)
            .unwrap();

        // Spaces should be encoded as %20, but forward slashes preserved
        assert_eq!(url, "https://example.com/api/test%20endpoint");

        // Test with various special characters
        let args = vec!["path=test?query=1#anchor".to_string()];
        let result = resolver.resolve_variables(&args).unwrap();

        let url = resolver
            .substitute_url("https://example.com/{path}", &result)
            .unwrap();

        // Query and anchor characters should be encoded
        assert_eq!(url, "https://example.com/test%3Fquery%3D1%23anchor");
    }
}
