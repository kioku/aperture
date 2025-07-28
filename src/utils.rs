/// Converts a string to kebab-case
///
/// Handles multiple input formats:
/// - `camelCase`: `"getUserById"` -> "get-user-by-id"
/// - `PascalCase`: `"GetUser"` -> "get-user"
/// - `snake_case`: `"get_user_by_id"` -> "get-user-by-id"
/// - Spaces: "List an Organization's Issues" -> "list-an-organizations-issues"
/// - Mixed: `"XMLHttpRequest"` -> "xml-http-request"
///
/// Special handling:
/// - Apostrophes are removed entirely: "Organization's" -> "organizations"
/// - Special characters become hyphens: "hello!world" -> "hello-world"
/// - Consecutive non-alphanumeric characters are collapsed: "a---b" -> "a-b"
/// - Leading/trailing hyphens are trimmed
#[must_use]
pub fn to_kebab_case(s: &str) -> String {
    let mut result = String::new();
    let mut chars = s.chars().peekable();
    let mut prev_was_lowercase = false;
    let mut prev_was_uppercase = false;
    let mut prev_was_separator = true; // Start true to avoid leading hyphen

    while let Some(ch) = chars.next() {
        match ch {
            c if c.is_alphanumeric() => {
                let is_upper = c.is_uppercase();
                let is_lower = c.is_lowercase();

                let needs_hyphen = should_insert_hyphen(
                    is_upper,
                    prev_was_lowercase,
                    prev_was_uppercase,
                    prev_was_separator,
                    chars.peek().is_some_and(|&next| next.is_lowercase()),
                );

                if needs_hyphen {
                    result.push('-');
                }

                result.push(c.to_ascii_lowercase());
                // Treat numbers as "lowercase" for transition purposes
                prev_was_lowercase = is_lower || c.is_numeric();
                prev_was_uppercase = is_upper;
                prev_was_separator = false;
            }
            '\'' => {} // Skip apostrophes entirely
            _ if !prev_was_separator => {
                // Replace any non-alphanumeric with hyphen (but avoid consecutive hyphens)
                result.push('-');
                prev_was_separator = true;
                prev_was_lowercase = false;
                prev_was_uppercase = false;
            }
            _ => {} // Skip consecutive non-alphanumeric characters
        }
    }

    result.trim_end_matches('-').to_string()
}

/// Determines if a hyphen should be inserted before the current character
#[inline]
#[allow(clippy::fn_params_excessive_bools)]
const fn should_insert_hyphen(
    is_upper: bool,
    prev_was_lowercase: bool,
    prev_was_uppercase: bool,
    prev_was_separator: bool,
    next_is_lowercase: bool,
) -> bool {
    !prev_was_separator
        && is_upper
        && (prev_was_lowercase || (prev_was_uppercase && next_is_lowercase))
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
        assert_eq!(to_kebab_case("hello@world.com"), "hello-world-com");
        assert_eq!(to_kebab_case("price$99"), "price-99");
        assert_eq!(to_kebab_case("100%Complete"), "100-complete");

        // Consecutive uppercase handling
        assert_eq!(to_kebab_case("ABCDefg"), "abc-defg");
        assert_eq!(to_kebab_case("HTTPSProxy"), "https-proxy");
        assert_eq!(to_kebab_case("HTTPAPI"), "httpapi");
        assert_eq!(to_kebab_case("HTTPAPIs"), "httpap-is");

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
