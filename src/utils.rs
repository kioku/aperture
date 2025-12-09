/// Converts a string to kebab-case
///
/// Handles multiple input formats:
/// - `camelCase`: `"getUserById"` -> "get-user-by-id"
/// - `PascalCase`: `"GetUser"` -> "get-user"
/// - `snake_case`: `"get_user_by_id"` -> "get-user-by-id"
/// - Spaces: "List an Organization's Issues" -> "list-an-organizations-issues"
/// - Mixed: `"XMLHttpRequest"` -> "xml-http-request"
/// - Unicode: `"CAFÉ"` -> "café"
///
/// Special handling:
/// - Apostrophes are removed entirely: "Organization's" -> "organizations"
/// - Special characters become hyphens: "hello!world" -> "hello-world"
/// - Consecutive non-alphanumeric characters are collapsed: "a---b" -> "a-b"
/// - Leading/trailing hyphens are trimmed
/// - Unicode characters are properly lowercased
#[must_use]
pub fn to_kebab_case(s: &str) -> String {
    let mut result = String::new();
    let mut chars = s.chars().peekable();
    let mut last_was_sep = true;
    let mut last_was_lower = false;

    while let Some(ch) = chars.next() {
        match ch {
            '\'' => {} // Skip apostrophes
            c if c.is_alphanumeric() => {
                let is_upper = c.is_uppercase();

                // Determine if we need to insert a hyphen at word boundaries
                let needs_simple_boundary_hyphen = !last_was_sep && is_upper && last_was_lower;

                // Check if this is an acronym followed by a word (e.g., "HTTPSConnection")
                let needs_acronym_boundary_check = !needs_simple_boundary_hyphen
                    && !last_was_sep
                    && is_upper
                    && chars.peek().is_some_and(|&next| next.is_lowercase())
                    && !result.is_empty()
                    && !result.chars().last().unwrap_or(' ').is_numeric();

                // Only compute acronym pattern if we didn't already add simple boundary hyphen
                let needs_acronym_hyphen = needs_acronym_boundary_check && {
                    // Check if we're not in the middle of an all-caps word ending with 's' (APIs)
                    let remaining: Vec<char> = chars.clone().collect();
                    matches!(remaining.as_slice(),
                        // If next char is lowercase and there are more chars after it, add hyphen
                        [next, _, ..] if next.is_lowercase()
                    )
                };

                // Add hyphen if either condition is true (but not both, due to mutual exclusion above)
                if needs_simple_boundary_hyphen || needs_acronym_hyphen {
                    result.push('-');
                }

                // Use proper Unicode lowercase conversion
                for lower_ch in c.to_lowercase() {
                    result.push(lower_ch);
                }

                last_was_sep = false;
                last_was_lower = c.is_lowercase() || c.is_numeric();
            }
            _ => {
                // Convert other chars to hyphen, but avoid consecutive hyphens
                let should_add_separator = !last_was_sep && !result.is_empty();
                if should_add_separator {
                    result.push('-');
                }
                // Update state only if we added a separator
                if should_add_separator {
                    last_was_sep = true;
                    last_was_lower = false;
                }
            }
        }
    }

    result.trim_end_matches('-').to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_to_kebab_case() {
        // Test cases from the requirements
        assert_eq!(
            to_kebab_case("List an Organization's Issues"),
            "list-an-organizations-issues"
        );
        assert_eq!(to_kebab_case("getUser"), "get-user");
        assert_eq!(to_kebab_case("get_user_by_id"), "get-user-by-id");
        assert_eq!(
            to_kebab_case("Some---Multiple   Spaces"),
            "some-multiple-spaces"
        );

        // Additional test cases
        assert_eq!(to_kebab_case("getUserByID"), "get-user-by-id");
        assert_eq!(to_kebab_case("XMLHttpRequest"), "xml-http-request");
        assert_eq!(to_kebab_case("Simple"), "simple");
        assert_eq!(to_kebab_case("ALLCAPS"), "allcaps");
        assert_eq!(
            to_kebab_case("spaces between words"),
            "spaces-between-words"
        );
        assert_eq!(to_kebab_case("special!@#$%^&*()chars"), "special-chars");
        assert_eq!(to_kebab_case("trailing---"), "trailing");
        assert_eq!(to_kebab_case("---leading"), "leading");
        assert_eq!(to_kebab_case(""), "");
        assert_eq!(to_kebab_case("a"), "a");
        assert_eq!(to_kebab_case("A"), "a");

        // Edge cases with apostrophes
        assert_eq!(to_kebab_case("don't"), "dont");
        assert_eq!(to_kebab_case("it's"), "its");
        assert_eq!(to_kebab_case("users'"), "users");

        // Complex acronym cases
        assert_eq!(to_kebab_case("IOError"), "io-error");
        assert_eq!(to_kebab_case("HTTPSConnection"), "https-connection");
        assert_eq!(to_kebab_case("getHTTPSConnection"), "get-https-connection");

        // Numeric cases
        assert_eq!(to_kebab_case("base64Encode"), "base64-encode");
        assert_eq!(to_kebab_case("getV2API"), "get-v2-api");
        assert_eq!(to_kebab_case("v2APIResponse"), "v2-api-response");

        // More edge cases
        assert_eq!(
            to_kebab_case("_startWithUnderscore"),
            "start-with-underscore"
        );
        assert_eq!(to_kebab_case("endWithUnderscore_"), "end-with-underscore");
        assert_eq!(
            to_kebab_case("multiple___underscores"),
            "multiple-underscores"
        );
        assert_eq!(to_kebab_case("mixedUP_down_CASE"), "mixed-up-down-case");
        assert_eq!(to_kebab_case("123StartWithNumber"), "123-start-with-number");
        assert_eq!(to_kebab_case("has123Numbers456"), "has123-numbers456");

        // Unicode and special cases
        assert_eq!(to_kebab_case("café"), "café"); // Non-ASCII preserved if alphanumeric
        assert_eq!(to_kebab_case("CAFÉ"), "café"); // Unicode uppercase properly lowercased
        assert_eq!(to_kebab_case("ÑOÑO"), "ñoño"); // Spanish characters
        assert_eq!(to_kebab_case("ÄÖÜ"), "äöü"); // German umlauts
        assert_eq!(to_kebab_case("МОСКВА"), "москва"); // Cyrillic
        assert_eq!(to_kebab_case("hello@world.com"), "hello-world-com");
        assert_eq!(to_kebab_case("price$99"), "price-99");
        assert_eq!(to_kebab_case("100%Complete"), "100-complete");

        // Consecutive uppercase handling
        assert_eq!(to_kebab_case("ABCDefg"), "abc-defg");
        assert_eq!(to_kebab_case("HTTPSProxy"), "https-proxy");
        assert_eq!(to_kebab_case("HTTPAPI"), "httpapi");
        assert_eq!(to_kebab_case("HTTPAPIs"), "httpapis");

        // Real-world OpenAPI operation IDs
        assert_eq!(
            to_kebab_case("List an Organization's Projects"),
            "list-an-organizations-projects"
        );
        assert_eq!(to_kebab_case("Update User's Avatar"), "update-users-avatar");
        assert_eq!(
            to_kebab_case("Delete Team's Repository Access"),
            "delete-teams-repository-access"
        );
    }
}
