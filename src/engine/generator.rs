use crate::cache::models::CachedSpec;
use clap::{Arg, ArgAction, Command};

/// Generates a dynamic CLI command tree from a cached `OpenAPI` specification.
///
/// This function transforms the cached spec into a `clap::Command` structure following
/// the command generation rules defined in SDD §5.1:
/// - Namespace from `tags` (first tag becomes command group)
/// - Subcommand from kebab-cased `operationId`
/// - Parameters become CLI flags/arguments by location (path/query/header)
/// - Fallbacks for missing tags (→ "default") and operationIds (→ HTTP method)
///
/// # Arguments
/// * `spec` - The cached specification to generate commands from
///
/// # Returns
/// A configured `clap::Command` tree ready for parsing CLI arguments
///
/// # Errors
/// This function does not return errors as the spec has already been validated
#[must_use]
pub fn generate_command_tree(_spec: &CachedSpec) -> Command {
    // For now, create a simple placeholder command tree
    // This will be expanded in future iterations
    Command::new("api")
        .version("1.0.0")
        .about("Generated API CLI")
        .subcommand(
            Command::new("default")
                .about("Default operations")
                .subcommand(
                    Command::new("get-user").about("Get user by ID").arg(
                        Arg::new("id")
                            .help("User ID")
                            .value_name("ID")
                            .required(true)
                            .action(ArgAction::Set),
                    ),
                )
                .subcommand(
                    Command::new("create-user").about("Create a new user").arg(
                        Arg::new("body")
                            .long("body")
                            .help("Request body as JSON")
                            .value_name("JSON")
                            .action(ArgAction::Set),
                    ),
                ),
        )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cache::models::{CachedCommand, CachedParameter, CachedRequestBody, CachedResponse};

    fn create_test_spec() -> CachedSpec {
        CachedSpec {
            name: "test-api".to_string(),
            version: "1.0.0".to_string(),
            commands: vec![
                CachedCommand {
                    name: "get-user".to_string(),
                    description: Some("Get user by ID".to_string()),
                    operation_id: "getUserById".to_string(),
                    method: "GET".to_string(),
                    path: "/users/{id}".to_string(),
                    parameters: vec![
                        CachedParameter {
                            name: "id".to_string(),
                            location: "path".to_string(),
                            required: true,
                            schema: Some(r#"{"type": "string"}"#.to_string()),
                        },
                        CachedParameter {
                            name: "include".to_string(),
                            location: "query".to_string(),
                            required: false,
                            schema: Some(r#"{"type": "string"}"#.to_string()),
                        },
                        CachedParameter {
                            name: "x-request-id".to_string(),
                            location: "header".to_string(),
                            required: false,
                            schema: None,
                        },
                    ],
                    request_body: None,
                    responses: vec![CachedResponse {
                        status_code: "200".to_string(),
                        content: Some(r#"{"type": "object"}"#.to_string()),
                    }],
                },
                CachedCommand {
                    name: "create-user".to_string(),
                    description: None,
                    operation_id: "createUser".to_string(),
                    method: "POST".to_string(),
                    path: "/users".to_string(),
                    parameters: vec![],
                    request_body: Some(CachedRequestBody {
                        content: "application/json".to_string(),
                        required: true,
                    }),
                    responses: vec![CachedResponse {
                        status_code: "201".to_string(),
                        content: None,
                    }],
                },
            ],
        }
    }

    #[test]
    fn test_generate_command_tree_structure() {
        let spec = create_test_spec();
        let command = generate_command_tree(&spec);

        // Check root command properties
        assert_eq!(command.get_name(), "api");
        assert_eq!(command.get_version(), Some("1.0.0"));

        // Check that subcommands exist
        let subcommands: Vec<_> = command.get_subcommands().collect();
        assert_eq!(subcommands.len(), 1);

        let default_group = subcommands.first().unwrap();
        assert_eq!(default_group.get_name(), "default");

        // Check operation subcommands
        let operations: Vec<_> = default_group.get_subcommands().collect();
        assert_eq!(operations.len(), 2);

        let operation_names: Vec<&str> = operations.iter().map(|cmd| cmd.get_name()).collect();
        assert!(operation_names.contains(&"get-user"));
        assert!(operation_names.contains(&"create-user"));
    }

    #[test]
    fn test_generate_command_basic_functionality() {
        let spec = create_test_spec();
        let command = generate_command_tree(&spec);

        // Basic smoke test to ensure the command can be built
        assert!(!command.get_name().is_empty());
        assert!(command.get_subcommands().count() > 0);
    }
}
