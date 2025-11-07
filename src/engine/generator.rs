use crate::cache::models::{CachedCommand, CachedParameter, CachedSpec};
use crate::constants;
use crate::utils::to_kebab_case;
use clap::{Arg, ArgAction, Command};
use std::collections::HashMap;
use std::fmt::Write;

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
/// - Root command: "api" (`CLI_ROOT_COMMAND`)
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

/// Generates a dynamic clap command tree with optional legacy positional parameter syntax.
#[must_use]
pub fn generate_command_tree_with_flags(spec: &CachedSpec, use_positional_args: bool) -> Command {
    let mut root_command = Command::new(constants::CLI_ROOT_COMMAND)
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
        )
        .arg(
            Arg::new("server-var")
                .long("server-var")
                .global(true)
                .help("Set server template variable (e.g., --server-var region=us --server-var env=prod)")
                .value_name("KEY=VALUE")
                .action(ArgAction::Append),
        );

    // Group commands by their tag (namespace)
    let mut command_groups: HashMap<String, Vec<&CachedCommand>> = HashMap::new();

    for command in &spec.commands {
        // Use the command name (first tag) or "default" as fallback
        let group_name = if command.name.is_empty() {
            constants::DEFAULT_GROUP.to_string()
        } else {
            command.name.clone()
        };

        command_groups.entry(group_name).or_default().push(command);
    }

    // Build subcommands for each group
    for (group_name, commands) in command_groups {
        let group_name_kebab = to_kebab_case(&group_name);
        let group_name_static = to_static_str(group_name_kebab);
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

            // Build help text with examples
            let mut help_text = cached_command.description.clone().unwrap_or_default();
            if !cached_command.examples.is_empty() {
                help_text.push_str("\n\nExamples:");
                for example in &cached_command.examples {
                    write!(
                        &mut help_text,
                        "\n  {}\n    {}",
                        example.description, example.command_line
                    )
                    .unwrap();
                    if let Some(ref explanation) = example.explanation {
                        write!(&mut help_text, "\n    ({explanation})").unwrap();
                    }
                }
            }

            let mut operation_command = Command::new(subcommand_name_static).about(help_text);

            // Add parameters as CLI arguments
            for param in &cached_command.parameters {
                let arg = create_arg_from_parameter(param, use_positional_args);
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

            // Add examples flag for showing extended examples
            operation_command = operation_command.arg(
                Arg::new("show-examples")
                    .long("show-examples")
                    .help("Show extended usage examples for this command")
                    .action(ArgAction::SetTrue),
            );

            group_command = group_command.subcommand(operation_command);
        }

        root_command = root_command.subcommand(group_command);
    }

    root_command
}

/// Creates a clap Arg from a `CachedParameter`
///
/// # Boolean Parameter Handling
///
/// Boolean parameters use `ArgAction::SetTrue`, treating them as flags:
///
/// **Path Parameters:**
/// - Always optional regardless of `OpenAPI` `required` field
/// - Flag presence = true (substitutes "true" in path), absence = false (substitutes "false")
/// - Example: `/items/{active}` with `--active` → `/items/true`, without → `/items/false`
///
/// **Query/Header Parameters:**
/// - **Optional booleans** (`required: false`): Flag presence = true, absence = false
/// - **Required booleans** (`required: true`): Flag MUST be provided, presence = true
/// - Example: `--verbose` (optional) omitted means `verbose=false`
///
/// This differs from non-boolean parameters which require explicit values (e.g., `--id 123`).
fn create_arg_from_parameter(param: &CachedParameter, use_positional_args: bool) -> Arg {
    let param_name_static = to_static_str(param.name.clone());
    let mut arg = Arg::new(param_name_static);

    // Check if this is a boolean parameter (type: "boolean" in OpenAPI schema)
    let is_boolean = param.schema_type.as_ref().is_some_and(|t| t == "boolean");

    match param.location.as_str() {
        "path" => {
            if use_positional_args {
                // Legacy mode: path parameters are positional arguments
                let value_name = to_static_str(param.name.to_uppercase());
                arg = arg
                    .help(format!("{} parameter", param.name))
                    .value_name(value_name)
                    .required(param.required)
                    .action(ArgAction::Set);
            } else {
                // Default mode: path parameters become flags too
                let long_name = to_static_str(to_kebab_case(&param.name));

                if is_boolean {
                    // Boolean path parameters are treated as flags
                    // Always optional: flag presence = true, absence = false (substituted in path)
                    // This provides consistent UX regardless of OpenAPI required field
                    arg = arg
                        .long(long_name)
                        .help(format!("Path parameter: {}", param.name))
                        .required(false)
                        .action(ArgAction::SetTrue);
                } else {
                    let value_name = to_static_str(param.name.to_uppercase());
                    arg = arg
                        .long(long_name)
                        .help(format!("Path parameter: {}", param.name))
                        .value_name(value_name)
                        .required(param.required)
                        .action(ArgAction::Set);
                }
            }
        }
        "query" | "header" => {
            // Query and header parameters are flags
            let long_name = to_static_str(to_kebab_case(&param.name));

            if is_boolean {
                // Boolean parameters are proper flags
                // Required booleans must be provided; optional booleans default to false when absent
                arg = arg
                    .long(long_name)
                    .help(format!(
                        "{} {} parameter",
                        capitalize_first(&param.location),
                        param.name
                    ))
                    .required(param.required)
                    .action(ArgAction::SetTrue);
            } else {
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
        }
        _ => {
            // Unknown location, treat as flag
            let long_name = to_static_str(to_kebab_case(&param.name));

            if is_boolean {
                // Boolean parameters are proper flags
                // Required booleans must be provided; optional booleans default to false when absent
                arg = arg
                    .long(long_name)
                    .help(format!("{} parameter", param.name))
                    .required(param.required)
                    .action(ArgAction::SetTrue);
            } else {
                let value_name = to_static_str(param.name.to_uppercase());
                arg = arg
                    .long(long_name)
                    .help(format!("{} parameter", param.name))
                    .value_name(value_name)
                    .required(param.required)
                    .action(ArgAction::Set);
            }
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
