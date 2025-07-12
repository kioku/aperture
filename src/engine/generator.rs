use crate::cache::models::{CachedCommand, CachedParameter, CachedSpec};
use clap::{Arg, ArgAction, Command};
use std::collections::HashMap;

/// Converts a string to kebab-case
fn to_kebab_case(s: &str) -> String {
    let mut result = String::new();
    let mut prev_lowercase = false;

    for (i, ch) in s.chars().enumerate() {
        if ch.is_uppercase() && i > 0 && prev_lowercase {
            result.push('-');
        }
        result.push(ch.to_ascii_lowercase());
        prev_lowercase = ch.is_lowercase();
    }

    result
}

/// Converts a String to a 'static str by leaking it
///
/// This is necessary for clap's API which requires 'static strings.
/// In a CLI context, this is acceptable as the program runs once and exits.
fn to_static_str(s: String) -> &'static str {
    Box::leak(s.into_boxed_str())
}

/// Generates a dynamic clap command tree from a cached `OpenAPI` specification.
///
/// This function creates a hierarchical command structure based on the `OpenAPI` spec:
/// - Root command: "api"
/// - Tag groups: Operations are grouped by their tags (e.g., "users", "posts")
/// - Operations: Individual API operations as subcommands under their tag group
///
/// # Arguments
/// * `spec` - The cached `OpenAPI` specification
/// * `experimental_flags` - Whether to use flag-based syntax for all parameters
///
/// # Returns
/// A clap Command configured with all operations from the spec
///
/// # Example
/// For an API with a "users" tag containing "getUser" and "createUser" operations:
/// ```text
/// api users get-user <args>
/// api users create-user <args>
/// ```
#[must_use]
pub fn generate_command_tree(spec: &CachedSpec) -> Command {
    generate_command_tree_with_flags(spec, false)
}

/// Generates a dynamic clap command tree with optional experimental flag-based parameter syntax.
#[must_use]
pub fn generate_command_tree_with_flags(spec: &CachedSpec, experimental_flags: bool) -> Command {
    let mut root_command = Command::new("api")
        .version(to_static_str(spec.version.clone()))
        .about(format!("CLI for {} API", spec.name))
        // Add global flags that should be available to all operations
        .arg(
            Arg::new("jq")
                .long("jq")
                .global(true)
                .help("Apply JQ filter to response data (e.g., '.name', '.[] | select(.active)')")
                .value_name("FILTER")
                .action(ArgAction::Set),
        )
        .arg(
            Arg::new("format")
                .long("format")
                .global(true)
                .help("Output format for response data")
                .value_name("FORMAT")
                .value_parser(["json", "yaml", "table"])
                .default_value("json")
                .action(ArgAction::Set),
        );

    // Group commands by their tag (namespace)
    let mut command_groups: HashMap<String, Vec<&CachedCommand>> = HashMap::new();

    for command in &spec.commands {
        // Use the command name (first tag) or "default" as fallback
        let group_name = if command.name.is_empty() {
            "default".to_string()
        } else {
            command.name.clone()
        };

        command_groups.entry(group_name).or_default().push(command);
    }

    // Build subcommands for each group
    for (group_name, commands) in command_groups {
        let group_name_static = to_static_str(group_name.clone());
        let mut group_command = Command::new(group_name_static)
            .about(format!("{} operations", capitalize_first(&group_name)));

        // Add operations as subcommands
        for cached_command in commands {
            let subcommand_name = if cached_command.operation_id.is_empty() {
                // Fallback to HTTP method if no operationId
                cached_command.method.to_lowercase()
            } else {
                to_kebab_case(&cached_command.operation_id)
            };

            let subcommand_name_static = to_static_str(subcommand_name);
            let mut operation_command = Command::new(subcommand_name_static)
                .about(cached_command.description.clone().unwrap_or_default());

            // Add parameters as CLI arguments
            for param in &cached_command.parameters {
                let arg = create_arg_from_parameter(param, experimental_flags);
                operation_command = operation_command.arg(arg);
            }

            // Add request body argument if present
            if let Some(request_body) = &cached_command.request_body {
                operation_command = operation_command.arg(
                    Arg::new("body")
                        .long("body")
                        .help("Request body as JSON")
                        .value_name("JSON")
                        .required(request_body.required)
                        .action(ArgAction::Set),
                );
            }

            // Add custom header support
            operation_command = operation_command.arg(
                Arg::new("header")
                    .long("header")
                    .short('H')
                    .help("Pass custom header(s) to the request. Format: 'Name: Value'. Can be used multiple times.")
                    .value_name("HEADER")
                    .action(ArgAction::Append),
            );

            group_command = group_command.subcommand(operation_command);
        }

        root_command = root_command.subcommand(group_command);
    }

    root_command
}

/// Creates a clap Arg from a `CachedParameter`
fn create_arg_from_parameter(param: &CachedParameter, experimental_flags: bool) -> Arg {
    let param_name_static = to_static_str(param.name.clone());
    let mut arg = Arg::new(param_name_static);

    match param.location.as_str() {
        "path" => {
            if experimental_flags {
                // In experimental mode, path parameters become flags too
                let long_name = to_static_str(param.name.clone());
                let value_name = to_static_str(param.name.to_uppercase());
                arg = arg
                    .long(long_name)
                    .help(format!("Path parameter: {}", param.name))
                    .value_name(value_name)
                    .required(param.required)
                    .action(ArgAction::Set);
            } else {
                // Path parameters are positional arguments
                let value_name = to_static_str(param.name.to_uppercase());
                arg = arg
                    .help(format!("{} parameter", param.name))
                    .value_name(value_name)
                    .required(param.required)
                    .action(ArgAction::Set);
            }
        }
        "query" | "header" => {
            // Query and header parameters are flags
            let long_name = to_static_str(param.name.clone());
            let value_name = to_static_str(param.name.to_uppercase());
            arg = arg
                .long(long_name)
                .help(format!(
                    "{} {} parameter",
                    capitalize_first(&param.location),
                    param.name
                ))
                .value_name(value_name)
                .required(param.required)
                .action(ArgAction::Set);
        }
        _ => {
            // Unknown location, treat as flag
            let long_name = to_static_str(param.name.clone());
            let value_name = to_static_str(param.name.to_uppercase());
            arg = arg
                .long(long_name)
                .help(format!("{} parameter", param.name))
                .value_name(value_name)
                .required(param.required)
                .action(ArgAction::Set);
        }
    }

    arg
}

/// Capitalizes the first letter of a string
fn capitalize_first(s: &str) -> String {
    let mut chars = s.chars();
    chars.next().map_or_else(String::new, |first| {
        first.to_uppercase().chain(chars).collect()
    })
}
