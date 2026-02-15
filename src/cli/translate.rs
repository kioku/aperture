//! CLI translation layer: converts clap `ArgMatches` into domain types.
//!
//! This module bridges the clap-specific parsing world with the
//! CLI-agnostic [`OperationCall`] and [`ExecutionContext`] types used
//! by the execution engine.

use crate::cache::models::{CachedCommand, CachedParameter, CachedSpec};
use crate::cli::Cli;
use crate::config::models::GlobalConfig;
use crate::constants;
use crate::duration::parse_duration;
use crate::engine::executor::RetryContext;
use crate::error::Error;
use crate::invocation::{ExecutionContext, OperationCall};
use crate::response_cache::CacheConfig;
use crate::utils::to_kebab_case;
use clap::ArgMatches;
use std::collections::HashMap;
use std::path::PathBuf;
use std::time::Duration;

/// Converts clap `ArgMatches` (from a dynamically generated command tree)
/// into a CLI-agnostic [`OperationCall`].
///
/// Walks the subcommand hierarchy to identify the operation, then extracts
/// path, query, and header parameters, the request body, and any custom
/// headers.
///
/// # Errors
///
/// Returns an error if the operation cannot be found in the spec.
pub fn matches_to_operation_call(
    spec: &CachedSpec,
    matches: &ArgMatches,
) -> Result<OperationCall, Error> {
    let (operation, current_matches) = find_operation_from_matches(spec, matches)?;

    // Extract parameters by location
    let mut path_params = HashMap::new();
    let mut query_params = HashMap::new();
    let mut header_params = HashMap::new();

    for param in &operation.parameters {
        extract_param(
            param,
            current_matches,
            &mut path_params,
            &mut query_params,
            &mut header_params,
        );
    }

    // Extract request body
    let body = extract_body(operation.request_body.is_some(), current_matches)?;

    // Extract custom headers from --header/-H flags
    let custom_headers = current_matches
        .try_get_many::<String>("header")
        .ok()
        .flatten()
        .map(|values| values.cloned().collect())
        .unwrap_or_default();

    Ok(OperationCall {
        operation_id: operation.operation_id.clone(),
        path_params,
        query_params,
        header_params,
        body,
        custom_headers,
    })
}

/// Resolves the matched operation and returns its `operation_id`.
///
/// Unlike [`matches_to_operation_call`], this does not attempt to parse or
/// validate request body content, making it suitable for purely metadata-driven
/// flows like `--show-examples`.
///
/// # Errors
///
/// Returns an error if no matching operation can be resolved from the
/// subcommand hierarchy.
pub fn matches_to_operation_id(spec: &CachedSpec, matches: &ArgMatches) -> Result<String, Error> {
    let (operation, _) = find_operation_from_matches(spec, matches)?;
    Ok(operation.operation_id.clone())
}

/// Walks the clap hierarchy and resolves the target operation plus deepest matches.
fn find_operation_from_matches<'a>(
    spec: &'a CachedSpec,
    matches: &'a ArgMatches,
) -> Result<(&'a CachedCommand, &'a ArgMatches), Error> {
    let mut current_matches = matches;
    let mut subcommand_path = Vec::new();

    while let Some((name, sub_matches)) = current_matches.subcommand() {
        subcommand_path.push(name.to_string());
        current_matches = sub_matches;
    }

    let operation_name = subcommand_path.last().ok_or_else(|| {
        let name = "unknown".to_string();
        let suggestions = crate::suggestions::suggest_similar_operations(spec, &name);
        Error::operation_not_found_with_suggestions(name, &suggestions)
    })?;

    // Dynamic tree shape is: <group> <operation>
    let group_name = subcommand_path
        .len()
        .checked_sub(2)
        .and_then(|idx| subcommand_path.get(idx));

    let operation = spec
        .commands
        .iter()
        .find(|cmd| matches_effective_command_path(cmd, group_name, operation_name))
        // Backward-compatible fallback: resolve by operation name only.
        // This supports existing tests and any callers that manually construct
        // `ArgMatches` without a generator-consistent group segment.
        .or_else(|| {
            spec.commands
                .iter()
                .find(|cmd| matches_effective_command_path(cmd, None, operation_name))
        })
        .ok_or_else(|| {
            let suggestions = crate::suggestions::suggest_similar_operations(spec, operation_name);
            Error::operation_not_found_with_suggestions(operation_name.clone(), &suggestions)
        })?;

    Ok((operation, current_matches))
}

/// Returns true when a command matches a parsed group/operation subcommand path.
fn matches_effective_command_path(
    command: &CachedCommand,
    group_name: Option<&String>,
    operation_name: &str,
) -> bool {
    let operation_matches = effective_operation_name(command) == operation_name
        || command
            .aliases
            .iter()
            .any(|alias| to_kebab_case(alias) == operation_name);

    if !operation_matches {
        return false;
    }

    group_name.is_none_or(|group| effective_group_name(command) == *group)
}

/// Computes the effective group name used by the command tree generator.
fn effective_group_name(command: &CachedCommand) -> String {
    command.display_group.as_ref().map_or_else(
        || {
            if command.name.is_empty() {
                constants::DEFAULT_GROUP.to_string()
            } else {
                to_kebab_case(&command.name)
            }
        },
        |group| to_kebab_case(group),
    )
}

/// Computes the effective operation name used by the command tree generator.
fn effective_operation_name(command: &CachedCommand) -> String {
    command.display_name.as_ref().map_or_else(
        || {
            if command.operation_id.is_empty() {
                command.method.to_lowercase()
            } else {
                to_kebab_case(&command.operation_id)
            }
        },
        |name| to_kebab_case(name),
    )
}

/// Extracts a single parameter value from matches and inserts it into the
/// appropriate map based on its `OpenAPI` location.
fn extract_param(
    param: &CachedParameter,
    matches: &ArgMatches,
    path_params: &mut HashMap<String, String>,
    query_params: &mut HashMap<String, String>,
    header_params: &mut HashMap<String, String>,
) {
    let target = match param.location.as_str() {
        "path" => path_params,
        "query" => query_params,
        "header" => header_params,
        _ => return,
    };

    let is_boolean = param.schema_type.as_ref().is_some_and(|t| t == "boolean");

    if !is_boolean {
        // Non-boolean: extract string value (must be checked first to avoid
        // get_flag panic on non-boolean args)
        let Some(value) = matches.try_get_one::<String>(&param.name).ok().flatten() else {
            return;
        };
        target.insert(param.name.clone(), value.clone());
        return;
    }

    // Boolean parameters are flags (SetTrue action in clap)
    // Path booleans always need a value (true/false); query/header only when true
    let flag_set = matches.get_flag(&param.name);
    if flag_set || param.location == "path" {
        target.insert(param.name.clone(), flag_set.to_string());
    }
}

/// Extracts the request body from matches.
fn extract_body(has_request_body: bool, matches: &ArgMatches) -> Result<Option<String>, Error> {
    if !has_request_body {
        return Ok(None);
    }

    matches
        .get_one::<String>("body")
        .map(|body_value| {
            // Validate JSON
            let _: serde_json::Value = serde_json::from_str(body_value)
                .map_err(|e| Error::invalid_json_body(e.to_string()))?;
            Ok(body_value.clone())
        })
        .transpose()
}

/// Extracts server variable arguments from CLI matches.
#[must_use]
pub fn extract_server_var_args(matches: &ArgMatches) -> Vec<String> {
    matches
        .try_get_many::<String>("server-var")
        .ok()
        .flatten()
        .map(|values| values.cloned().collect())
        .unwrap_or_default()
}

/// Returns true if the `--show-examples` flag is set in the operation's matches.
#[must_use]
pub fn has_show_examples_flag(matches: &ArgMatches) -> bool {
    // Walk to the deepest subcommand
    let mut current = matches;
    while let Some((_name, sub)) = current.subcommand() {
        current = sub;
    }

    current.try_contains_id("show-examples").unwrap_or(false) && current.get_flag("show-examples")
}

/// Builds an [`ExecutionContext`] from CLI flags and optional global config.
///
/// # Errors
///
/// Returns an error if duration parsing fails for retry delay values.
#[allow(clippy::cast_possible_truncation)]
pub fn cli_to_execution_context(
    cli: &Cli,
    global_config: Option<GlobalConfig>,
) -> Result<ExecutionContext, Error> {
    let config_dir = if let Ok(dir) = std::env::var(crate::constants::ENV_APERTURE_CONFIG_DIR) {
        PathBuf::from(dir)
    } else {
        crate::config::manager::get_config_dir()?
    };

    // Build cache config from CLI flags
    let cache_config = if cli.no_cache {
        None
    } else {
        Some(CacheConfig {
            cache_dir: config_dir
                .join(crate::constants::DIR_CACHE)
                .join(crate::constants::DIR_RESPONSES),
            default_ttl: Duration::from_secs(cli.cache_ttl.unwrap_or(300)),
            max_entries: 1000,
            enabled: cli.cache || cli.cache_ttl.is_some(),
            allow_authenticated: false,
        })
    };

    // Build retry context
    let retry_context = build_retry_context(cli, global_config.as_ref())?;

    Ok(ExecutionContext {
        dry_run: cli.dry_run,
        idempotency_key: cli.idempotency_key.clone(),
        cache_config,
        retry_context,
        base_url: None, // Resolved by BaseUrlResolver
        global_config,
        server_var_args: Vec::new(), // Populated from dynamic matches in the caller
    })
}

/// Builds a [`RetryContext`] from CLI flags and global configuration.
///
/// CLI flags take precedence over global config defaults.
#[allow(clippy::cast_possible_truncation)]
fn build_retry_context(
    cli: &Cli,
    global_config: Option<&GlobalConfig>,
) -> Result<Option<RetryContext>, Error> {
    let defaults = global_config.map(|c| &c.retry_defaults);

    let max_attempts = cli
        .retry
        .or_else(|| defaults.map(|d| d.max_attempts))
        .unwrap_or(0);

    if max_attempts == 0 {
        return Ok(None);
    }

    // Truncation is safe: delay values in practice are well under u64::MAX milliseconds
    let initial_delay_ms = if let Some(ref delay_str) = cli.retry_delay {
        parse_duration(delay_str)?.as_millis() as u64
    } else {
        defaults.map_or(500, |d| d.initial_delay_ms)
    };

    let max_delay_ms = if let Some(ref delay_str) = cli.retry_max_delay {
        parse_duration(delay_str)?.as_millis() as u64
    } else {
        defaults.map_or(30_000, |d| d.max_delay_ms)
    };

    let has_idempotency_key = cli.idempotency_key.is_some();

    Ok(Some(RetryContext {
        max_attempts,
        initial_delay_ms,
        max_delay_ms,
        force_retry: cli.force_retry,
        method: None, // Determined by executor at execution time
        has_idempotency_key,
    }))
}
