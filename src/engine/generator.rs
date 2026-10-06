//! Dynamic clap command tree generator from cached `OpenAPI` specifications.
//!
//! # `Box::leak` and `'static` lifetimes
//!
//! Clap requires `'static` strings for command and argument names. Since
//! operation IDs, parameter names, and tag names are determined at runtime
//! from the `OpenAPI` spec, we use [`Box::leak`] via [`to_static_str`] to
//! convert owned `String`s into `&'static str`.
//!
//! This is the standard pattern for dynamic clap usage and is safe because:
//! - The CLI binary runs once and exits — leaked memory is reclaimed by the OS.
//! - Total leaked memory is bounded by the spec size (typically <100KB).
//! - No long-running process or repeated allocation occurs.

use crate::cache::models::{CachedCommand, CachedParameter, CachedSpec};
use crate::constants;
use crate::docs::DocumentationGenerator;
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

/// Builds help text with examples for a command.
fn build_help_text_with_examples(cached_command: &CachedCommand, api_name: &str) -> String {
    let mut help_text = cached_command.description.clone().unwrap_or_default();
    let examples = DocumentationGenerator::canonical_examples(api_name, cached_command);

    if examples.is_empty() {
        return help_text;
    }

    help_text.push_str("\n\nExamples:");
    for example in examples {
        write!(
            &mut help_text,
            "\n  {}\n    {}",
            example.description, example.command_line
        )
        .expect("writing to String buffer cannot fail");

        // Add explanation if present
        if let Some(explanation) = example.explanation {
            write!(&mut help_text, "\n    ({explanation})")
                .expect("writing to String buffer cannot fail");
        }
    }

    help_text
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
    generate_command_tree_for_api_with_flags(spec, "<api>", use_positional_args)
}

/// Generates a dynamic clap command tree with examples tied to the provided API context.
#[must_use]
pub fn generate_command_tree_for_api_with_flags(
    spec: &CachedSpec,
    api_name: &str,
    use_positional_args: bool,
) -> Command {
    generate_tree_for_commands(spec, api_name, use_positional_args, spec.commands.iter())
}

/// Build the selected invocation without allocating unrelated operations.
/// Unresolved paths retain the complete tree for discovery and diagnostics.
pub(crate) fn generate_invocation_command_tree(
    spec: &CachedSpec,
    api_name: &str,
    args: &[String],
    use_positional_args: bool,
) -> Command {
    select_invocation_command(spec, args).map_or_else(
        |_| generate_command_tree_for_api_with_flags(spec, api_name, use_positional_args),
        |selected| {
            generate_tree_for_commands(
                spec,
                api_name,
                use_positional_args,
                std::iter::once(selected),
            )
        },
    )
}

/// Build only the selected operation's clap tree for batch parsing.
/// This avoids allocating every operation in a large immutable specification.
pub(crate) fn generate_batch_command_tree(
    spec: &CachedSpec,
    args: &[String],
) -> Result<Command, crate::error::Error> {
    let selected = select_invocation_command(spec, args)?;
    Ok(generate_tree_for_commands(
        spec,
        "<api>",
        false,
        std::iter::once(selected),
    ))
}

/// Resolve canonical/mapped paths and aliases using the execution flag grammar.
/// Failure is advisory for interactive invocations, which retain full diagnostics.
fn select_invocation_command<'a>(
    spec: &'a CachedSpec,
    args: &[String],
) -> Result<&'a CachedCommand, crate::error::Error> {
    // Parse the two command positions with the same global argument grammar as
    // execution. Flag values must never participate in operation selection.
    let (group, remaining) = parse_batch_subcommand(
        std::iter::once(std::ffi::OsString::from(constants::CLI_ROOT_COMMAND))
            .chain(args.iter().map(std::ffi::OsString::from)),
    )?;
    let (operation, _) =
        parse_batch_subcommand(std::iter::once(std::ffi::OsString::from(&group)).chain(remaining))?;
    spec.commands
        .iter()
        .find(|command| {
            to_kebab_case(&effective_group_name(command)) == group
                && (effective_subcommand_name(command) == operation
                    || command
                        .aliases
                        .iter()
                        .any(|alias| to_kebab_case(alias) == operation))
        })
        .ok_or_else(|| crate::error::Error::validation_error("Batch operation was not found"))
}

/// Parse one command level, leaving operation arguments untouched for execution.
/// Locate the leaf command's argv using the same global grammar as selection.
pub(crate) fn batch_operation_argument_offset(
    args: &[String],
) -> Result<usize, crate::error::Error> {
    let (group, remaining) = parse_batch_subcommand(
        std::iter::once(std::ffi::OsString::from(constants::CLI_ROOT_COMMAND))
            .chain(args.iter().map(std::ffi::OsString::from)),
    )?;
    let (_, remaining) =
        parse_batch_subcommand(std::iter::once(std::ffi::OsString::from(group)).chain(remaining))?;
    Ok(args.len() - remaining.len())
}

fn parse_batch_subcommand(
    args: impl IntoIterator<Item = std::ffi::OsString>,
) -> Result<(String, Vec<std::ffi::OsString>), crate::error::Error> {
    let matches = with_global_args(Command::new(constants::CLI_ROOT_COMMAND))
        .allow_external_subcommands(true)
        .try_get_matches_from(args)
        .map_err(|error| crate::error::Error::validation_error(error.to_string()))?;
    let (name, remaining) = matches
        .subcommand()
        .ok_or_else(|| crate::error::Error::validation_error("Batch command was not found"))?;
    Ok((
        name.to_string(),
        remaining
            .get_many::<std::ffi::OsString>("")
            .into_iter()
            .flatten()
            .cloned()
            .collect(),
    ))
}

fn generate_tree_for_commands<'a>(
    spec: &CachedSpec,
    api_name: &str,
    use_positional_args: bool,
    commands: impl Iterator<Item = &'a CachedCommand>,
) -> Command {
    let mut root_command = with_global_args(
        Command::new(constants::CLI_ROOT_COMMAND)
            .version(to_static_str(spec.version.clone()))
            .about(format!("CLI for {} API", spec.name)),
    );

    // Group commands by their effective group name (display_group override or tag)
    let mut command_groups: HashMap<String, Vec<&CachedCommand>> = HashMap::new();

    for command in commands {
        let group_name = effective_group_name(command);
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
            let subcommand_name = effective_subcommand_name(cached_command);
            let subcommand_name_static = to_static_str(subcommand_name);

            // Build help text with examples
            let help_text = build_help_text_with_examples(cached_command, api_name);

            let mut operation_command = Command::new(subcommand_name_static).about(help_text);

            // Add parameters as CLI arguments
            for param in &cached_command.parameters {
                let arg = create_arg_from_parameter(param, use_positional_args);
                operation_command = operation_command.arg(arg);
            }

            // Add request body argument if present
            if let Some(request_body) = &cached_command.request_body {
                operation_command = add_body_args(operation_command, request_body.required);
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

            // Apply command mapping: aliases
            let alias_strs: Vec<&'static str> = cached_command
                .aliases
                .iter()
                .map(|a| to_static_str(to_kebab_case(a)))
                .collect();
            if !alias_strs.is_empty() {
                operation_command = operation_command.visible_aliases(alias_strs);
            }

            // Apply command mapping: hidden
            if cached_command.hidden {
                operation_command = operation_command.hide(true);
            }

            group_command = group_command.subcommand(operation_command);
        }

        root_command = root_command.subcommand(group_command);
    }

    root_command
}

/// Keep operation selection and execution on the same global-flag grammar.
fn with_global_args(command: Command) -> Command {
    command
        // Add global flags that should be available to all operations
        // These are hidden from subcommand help to reduce noise - they're documented in `aperture --help`
        .arg(
            Arg::new("jq")
                .long("jq")
                .global(true)
                .hide(true)
                .help("Apply JQ filter to response data (e.g., '.name', '.[] | select(.active)')")
                .value_name("FILTER")
                .action(ArgAction::Set),
        )
        .arg(
            Arg::new("format")
                .long("format")
                .global(true)
                .hide(true)
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
                .hide(true)
                .help("Set server template variable (e.g., --server-var region=us --server-var env=prod)")
                .value_name("KEY=VALUE")
                .action(ArgAction::Append),
        )
}

/// Attaches `--body` and `--body-file` args to a command that accepts a request body.
///
/// When the spec marks the body as required, `--body-file` is an equally valid way to
/// satisfy that requirement. `required_unless_present` lets clap enforce "at least one
/// of the two" without rejecting `--body-file` on its own.
fn add_body_args(cmd: Command, required: bool) -> Command {
    let body_arg = Arg::new("body")
        .long("body")
        .help("Request body as JSON")
        .value_name("JSON")
        .conflicts_with("body-file")
        .action(ArgAction::Set);

    // required_unless_present: --body is required UNLESS --body-file is present.
    // Without this, clap enforces required(true) on --body independently of the
    // conflicts_with guard, causing --body-file-only invocations to be rejected.
    let body_arg = if required {
        body_arg.required_unless_present("body-file")
    } else {
        body_arg
    };

    cmd.arg(body_arg).arg(
        Arg::new("body-file")
            .long("body-file")
            .help("Read request body from a file path, or - for stdin")
            .value_name("PATH")
            .conflicts_with("body")
            .action(ArgAction::Set),
    )
}

/// Returns the effective group name for a command, using `display_group` override if present.
fn effective_group_name(command: &CachedCommand) -> String {
    command.display_group.as_ref().map_or_else(
        || {
            if command.name.is_empty() {
                constants::DEFAULT_GROUP.to_string()
            } else {
                command.name.clone()
            }
        },
        Clone::clone,
    )
}

/// Returns the effective subcommand name for a command, using `display_name` override if present.
fn effective_subcommand_name(command: &CachedCommand) -> String {
    command.display_name.as_ref().map_or_else(
        || {
            if command.operation_id.is_empty() {
                command.method.to_lowercase()
            } else {
                to_kebab_case(&command.operation_id)
            }
        },
        |n| to_kebab_case(n),
    )
}

/// Creates a clap Arg from a `CachedParameter`
///
/// # Boolean Parameter Handling
///
/// Query/header booleans accept explicit true/false values. A bare flag remains
/// shorthand for true; omission is distinct from false. Path flags retain their
/// existing default-false behavior.
fn create_arg_from_parameter(param: &CachedParameter, use_positional_args: bool) -> Arg {
    let is_boolean = param.schema_type.as_ref().is_some_and(|t| t == "boolean");

    match param.location.as_str() {
        "path" => create_path_parameter_arg(param, use_positional_args, is_boolean),
        "query" | "header" => create_scoped_parameter_arg(param, is_boolean),
        _ => create_generic_parameter_arg(param, is_boolean),
    }
}

fn create_path_parameter_arg(
    param: &CachedParameter,
    use_positional_args: bool,
    is_boolean: bool,
) -> Arg {
    let param_name_static = to_static_str(param.name.clone());
    let arg = Arg::new(param_name_static);

    if is_boolean {
        let long_name = to_static_str(to_kebab_case(&param.name));
        arg.long(long_name)
            .help(format!("Path parameter: {}", param.name))
            .required(false)
            .action(ArgAction::SetTrue)
    } else if use_positional_args {
        let value_name = to_static_str(param.name.to_uppercase());
        arg.help(format!("{} parameter", param.name))
            .value_name(value_name)
            .required(param.required)
            .action(ArgAction::Set)
    } else {
        let long_name = to_static_str(to_kebab_case(&param.name));
        let value_name = to_static_str(param.name.to_uppercase());
        arg.long(long_name)
            .help(format!("Path parameter: {}", param.name))
            .value_name(value_name)
            .required(param.required)
            .action(ArgAction::Set)
    }
}

fn create_scoped_parameter_arg(param: &CachedParameter, is_boolean: bool) -> Arg {
    let param_name_static = to_static_str(param.name.clone());
    let long_name = to_static_str(to_kebab_case(&param.name));
    let help = format!(
        "{} {} parameter",
        capitalize_first(&param.location),
        param.name
    );

    if is_boolean {
        Arg::new(param_name_static)
            .long(long_name)
            .help(help)
            .required(param.required)
            .action(ArgAction::Set)
            .value_parser(clap::value_parser!(bool))
            .num_args(0..=1)
            .default_missing_value("true")
    } else {
        let value_name = to_static_str(param.name.to_uppercase());
        Arg::new(param_name_static)
            .long(long_name)
            .help(help)
            .value_name(value_name)
            .required(param.required)
            .action(ArgAction::Set)
    }
}

fn create_generic_parameter_arg(param: &CachedParameter, is_boolean: bool) -> Arg {
    let param_name_static = to_static_str(param.name.clone());
    let long_name = to_static_str(to_kebab_case(&param.name));

    if is_boolean {
        Arg::new(param_name_static)
            .long(long_name)
            .help(format!("{} parameter", param.name))
            .required(param.required)
            .action(ArgAction::Set)
            .value_parser(clap::value_parser!(bool))
            .num_args(0..=1)
            .default_missing_value("true")
    } else {
        let value_name = to_static_str(param.name.to_uppercase());
        Arg::new(param_name_static)
            .long(long_name)
            .help(format!("{} parameter", param.name))
            .value_name(value_name)
            .required(param.required)
            .action(ArgAction::Set)
    }
}

/// Capitalizes the first letter of a string
fn capitalize_first(s: &str) -> String {
    let mut chars = s.chars();
    chars.next().map_or_else(String::new, |first| {
        first.to_uppercase().chain(chars).collect()
    })
}

#[cfg(test)]
mod invocation_tests {
    use super::*;

    fn spec() -> CachedSpec {
        let document = crate::spec::parse_openapi(
            r#"{
            "openapi":"3.0.3", "info":{"title":"Test","version":"1"},
            "paths":{
                "/items/{id}":{"get":{"operationId":"getFirst","tags":["items"],
                    "parameters":[{"name":"id","in":"path","required":true,
                        "schema":{"type":"string"}}],"responses":{"200":{"description":"OK"}}}},
                "/items":{"get":{"operationId":"getSecond","tags":["items"],
                    "responses":{"200":{"description":"OK"}}}},
                "/other":{"get":{"operationId":"getOther","tags":["other"],
                    "responses":{"200":{"description":"OK"}}}}
            }}"#,
        )
        .unwrap();
        crate::spec::transformer::SpecTransformer::new()
            .transform("test", &document)
            .unwrap()
    }

    fn tree(spec: &CachedSpec, args: &[&str], positional: bool) -> Command {
        generate_invocation_command_tree(
            spec,
            "test",
            &args.iter().map(|s| (*s).to_string()).collect::<Vec<_>>(),
            positional,
        )
    }

    #[test]
    fn invocation_allocates_only_selected_operation() {
        let command = tree(&spec(), &["items", "get-first"], false);
        assert_eq!(command.get_subcommands().count(), 1);
        assert_eq!(
            command
                .find_subcommand("items")
                .unwrap()
                .get_subcommands()
                .count(),
            1
        );
    }

    #[test]
    fn discovery_and_unknown_paths_retain_full_tree() {
        for args in [
            vec![],
            vec!["--help"],
            vec!["items", "--help"],
            vec!["items", "unknown"],
            vec!["none", "get-first"],
        ] {
            let command = tree(&spec(), &args, false);
            assert_eq!(command.get_subcommands().count(), 2);
            assert_eq!(
                command
                    .find_subcommand("items")
                    .unwrap()
                    .get_subcommands()
                    .count(),
                2
            );
        }
    }

    #[test]
    fn global_values_do_not_select_operations() {
        for args in [
            vec!["--server-var", "region=other", "items", "get-first"],
            vec!["items", "--server-var", "region=get-second", "get-first"],
        ] {
            let command = tree(&spec(), &args, false);
            assert_eq!(
                command
                    .find_subcommand("items")
                    .unwrap()
                    .get_subcommands()
                    .count(),
                1
            );
            assert!(command
                .find_subcommand("items")
                .unwrap()
                .find_subcommand("get-first")
                .is_some());
        }
    }

    #[test]
    fn mapped_alias_and_hidden_operation_remain_selectable() {
        let mut spec = spec();
        let selected = spec
            .commands
            .iter_mut()
            .find(|c| c.operation_id == "getFirst")
            .unwrap();
        selected.display_group = Some("Mapped Group".to_string());
        selected.display_name = Some("Mapped Operation".to_string());
        selected.aliases = vec!["Fetch Alias".to_string()];
        selected.hidden = true;
        let command = tree(&spec, &["mapped-group", "fetch-alias"], false);
        assert_eq!(command.get_subcommands().count(), 1);
        let matches = command
            .try_get_matches_from(["api", "mapped-group", "fetch-alias", "--id", "42"])
            .unwrap();
        assert_eq!(
            matches.subcommand().unwrap().1.subcommand_name(),
            Some("mapped-operation")
        );
    }

    #[test]
    fn selected_parameters_preserve_flag_and_positional_modes() {
        for (positional, args) in [
            (false, vec!["api", "items", "get-first", "--id", "42"]),
            (true, vec!["api", "items", "get-first", "42"]),
        ] {
            let command = tree(&spec(), &["items", "get-first"], positional);
            let matches = command.try_get_matches_from(args).unwrap();
            let operation = matches.subcommand().unwrap().1.subcommand().unwrap().1;
            assert_eq!(
                operation.get_one::<String>("id").map(String::as_str),
                Some("42")
            );
        }
    }
}
