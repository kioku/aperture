//! Handlers for `aperture api`, `aperture exec`, and batch operations.

use crate::batch::{BatchConfig, BatchProcessor};
use crate::cache::models::CachedSpec;
use crate::cli::Cli;
use crate::config::manager::{get_config_dir, ConfigManager};
use crate::config::models::GlobalConfig;
use crate::constants;
use crate::engine::{executor, generator, loader};
use crate::error::Error;
use crate::fs::OsFileSystem;
use crate::output::Output;
use crate::shortcuts::{ResolutionResult, ShortcutResolver};
use std::path::PathBuf;

/// Adds connection/timeout context to network errors.
fn enrich_network_error(e: Error) -> Error {
    let Error::Network(ref req_err) = e else {
        return e;
    };
    if req_err.is_connect() {
        return e.with_context("Failed to connect to API server");
    }
    if req_err.is_timeout() {
        return e.with_context("Request timed out");
    }
    e
}

/// Writes a structured JSON error as the final NDJSON line when `--json-errors` is active.
fn emit_pagination_error_ndjson(cli: &Cli, writer: &mut impl std::io::Write, error: &Error) {
    if !cli.json_errors {
        return;
    }
    let Ok(json) = serde_json::to_string(&error.to_json()) else {
        return;
    };
    let _ = writeln!(writer, "{json}");
}

/// Resolves the output format from dynamic matches vs CLI global flag.
fn resolve_output_format(
    matches: &clap::ArgMatches,
    cli_format: &crate::cli::OutputFormat,
) -> crate::cli::OutputFormat {
    use clap::parser::ValueSource;

    let Some(format_str) = matches.get_one::<String>("format") else {
        return cli_format.clone();
    };

    // The dynamic command tree always sets a default of "json".
    // If clap reports this value came from a default (not user input),
    // preserve the top-level CLI format parsed by `Cli`.
    if matches.value_source("format") == Some(ValueSource::DefaultValue) {
        return cli_format.clone();
    }

    match format_str.as_str() {
        "json" => crate::cli::OutputFormat::Json,
        "yaml" => crate::cli::OutputFormat::Yaml,
        "table" => crate::cli::OutputFormat::Table,
        _ => cli_format.clone(),
    }
}

struct ApiCommandContext {
    config_dir: PathBuf,
    spec: CachedSpec,
    global_config: Option<GlobalConfig>,
}

fn load_api_command_context(context: &str) -> Result<ApiCommandContext, Error> {
    let config_dir = if let Ok(dir) = std::env::var(constants::ENV_APERTURE_CONFIG_DIR) {
        PathBuf::from(dir)
    } else {
        get_config_dir()?
    };
    let cache_dir = config_dir.join(constants::DIR_CACHE);

    let manager = ConfigManager::with_fs(OsFileSystem, config_dir.clone());
    let global_config = manager.load_global_config().ok();

    let spec = loader::load_cached_spec(&cache_dir, context).map_err(|e| match e {
        Error::Io(_) => Error::spec_not_found(context),
        _ => e,
    })?;

    Ok(ApiCommandContext {
        config_dir,
        spec,
        global_config,
    })
}

fn handle_describe_json_command(
    context: &str,
    command_context: &ApiCommandContext,
    cli: &Cli,
) -> Result<(), Error> {
    let specs_dir = command_context.config_dir.join(constants::DIR_SPECS);
    let spec_path = specs_dir.join(format!("{context}.yaml"));
    // ast-grep-ignore: no-nested-if
    if !spec_path.exists() {
        return Err(Error::spec_not_found(context));
    }
    let spec_content = std::fs::read_to_string(&spec_path)?;
    let openapi_spec = crate::spec::parse_openapi(&spec_content)
        .map_err(|e| Error::invalid_config(format!("Failed to parse OpenAPI spec: {e}")))?;
    let manifest = crate::agent::generate_capability_manifest_from_openapi(
        context,
        &openapi_spec,
        &command_context.spec,
        command_context.global_config.as_ref(),
    )?;
    let output = match &cli.jq {
        Some(jq_filter) => executor::apply_jq_filter(&manifest, jq_filter)?,
        None => manifest,
    };
    // ast-grep-ignore: no-println
    println!("{output}");
    Ok(())
}

async fn handle_batch_file_command(
    context: &str,
    batch_file_path: &str,
    command_context: &ApiCommandContext,
    cli: &Cli,
) -> Result<(), Error> {
    execute_batch_operations(
        context,
        batch_file_path,
        &command_context.spec,
        command_context.global_config.as_ref(),
        cli,
    )
    .await
}

fn handle_show_examples_command(
    context: &str,
    matches: &clap::ArgMatches,
    command_context: &ApiCommandContext,
) -> Result<(), Error> {
    let operation_id =
        crate::cli::translate::matches_to_operation_id(&command_context.spec, matches)?;
    let operation = command_context
        .spec
        .commands
        .iter()
        .find(|cmd| cmd.operation_id == operation_id)
        .ok_or_else(|| Error::spec_not_found(context))?;
    crate::cli::render::render_examples(operation);
    Ok(())
}

async fn execute_api_runtime(
    spec: &CachedSpec,
    matches: &clap::ArgMatches,
    cli: &Cli,
    global_config: Option<GlobalConfig>,
) -> Result<(), Error> {
    let jq_filter = matches
        .get_one::<String>("jq")
        .map(String::as_str)
        .or(cli.jq.as_deref());
    let output_format = resolve_output_format(matches, &cli.format);
    let call = crate::cli::translate::matches_to_operation_call(spec, matches)?;
    let mut ctx = crate::cli::translate::cli_to_execution_context(cli, global_config)?;
    ctx.server_var_args = crate::cli::translate::extract_server_var_args(matches);

    if ctx.auto_paginate {
        return execute_paginated_api_runtime(spec, call, ctx, cli, jq_filter, output_format).await;
    }

    execute_standard_api_runtime(spec, call, ctx, output_format, jq_filter).await
}

async fn execute_paginated_api_runtime(
    spec: &CachedSpec,
    call: crate::invocation::OperationCall,
    ctx: crate::invocation::ExecutionContext,
    cli: &Cli,
    jq_filter: Option<&str>,
    output_format: crate::cli::OutputFormat,
) -> Result<(), Error> {
    if jq_filter.is_some() {
        tracing::warn!(
            "--jq is ignored with --auto-paginate; \
             pipe NDJSON output through an external jq process instead"
        );
    }
    if !matches!(output_format, crate::cli::OutputFormat::Json) {
        tracing::warn!("--format is ignored with --auto-paginate; output is always NDJSON");
    }

    let mut stdout = std::io::stdout();
    let result = crate::pagination::execute_paginated(spec, call, ctx, &mut stdout).await;
    match result {
        Ok(_) => Ok(()),
        Err(e) => {
            let e = enrich_network_error(e);
            // When --json-errors is active, emit the error as the final NDJSON
            // line on stdout so pipeline consumers can detect mid-stream failure
            // without inspecting stderr.
            emit_pagination_error_ndjson(cli, &mut stdout, &e);
            Err(e)
        }
    }
}

async fn execute_standard_api_runtime(
    spec: &CachedSpec,
    call: crate::invocation::OperationCall,
    ctx: crate::invocation::ExecutionContext,
    output_format: crate::cli::OutputFormat,
    jq_filter: Option<&str>,
) -> Result<(), Error> {
    let result = executor::execute(spec, call, ctx)
        .await
        .map_err(enrich_network_error)?;

    crate::cli::render::render_result(&result, &output_format, jq_filter)?;
    Ok(())
}

#[allow(clippy::too_many_lines)]
pub async fn execute_api_command(context: &str, args: Vec<String>, cli: &Cli) -> Result<(), Error> {
    let command_context = load_api_command_context(context)?;

    if cli.describe_json {
        return handle_describe_json_command(context, &command_context, cli);
    }

    if let Some(batch_file_path) = &cli.batch_file {
        return handle_batch_file_command(context, batch_file_path, &command_context, cli).await;
    }

    // Generate the dynamic command tree and parse arguments
    let command =
        generator::generate_command_tree_with_flags(&command_context.spec, cli.positional_args);
    let matches = command
        .try_get_matches_from(std::iter::once(constants::CLI_ROOT_COMMAND.to_string()).chain(args))
        .map_err(|e| Error::invalid_command(context, e.to_string()))?;

    // Check --show-examples flag
    if crate::cli::translate::has_show_examples_flag(&matches) {
        handle_show_examples_command(context, &matches, &command_context)?;
        return Ok(());
    }

    execute_api_runtime(
        &command_context.spec,
        &matches,
        cli,
        command_context.global_config.clone(),
    )
    .await
}

/// Executes batch operations from a batch file
#[allow(clippy::too_many_lines)]
pub async fn execute_batch_operations(
    _context: &str,
    batch_file_path: &str,
    spec: &CachedSpec,
    global_config: Option<&GlobalConfig>,
    cli: &Cli,
) -> Result<(), Error> {
    let batch_file =
        BatchProcessor::parse_batch_file(std::path::Path::new(batch_file_path)).await?;
    let batch_config = BatchConfig {
        max_concurrency: cli.batch_concurrency,
        rate_limit: cli.batch_rate_limit,
        continue_on_error: true,
        show_progress: !cli.quiet && !cli.json_errors,
        suppress_output: cli.json_errors,
    };
    let processor = BatchProcessor::new(batch_config);
    let result = processor
        .execute_batch(
            spec,
            batch_file,
            global_config,
            None,
            cli.dry_run,
            &cli.format,
            None,
        )
        .await?;

    let output = Output::new(cli.quiet, cli.json_errors);
    if cli.json_errors {
        render_batch_json_summary(&result, cli)?;
        return Ok(());
    }

    render_batch_text_summary(&result, &output);
    Ok(())
}

fn render_batch_json_summary(result: &crate::batch::BatchResult, cli: &Cli) -> Result<(), Error> {
    let summary = serde_json::json!({
        "batch_execution_summary": {
            "total_operations": result.results.len(),
            "successful_operations": result.success_count,
            "failed_operations": result.failure_count,
            "total_duration_seconds": result.total_duration.as_secs_f64(),
            "operations": result.results.iter().map(|r| serde_json::json!({
                "operation_id": r.operation.id,
                "args": r.operation.args,
                "success": r.success,
                "duration_seconds": r.duration.as_secs_f64(),
                "error": r.error
            })).collect::<Vec<_>>()
        }
    });
    let json_output = match &cli.jq {
        Some(jq_filter) => {
            let summary_json = serde_json::to_string(&summary)
                .expect("JSON serialization of valid structure cannot fail");
            executor::apply_jq_filter(&summary_json, jq_filter)?
        }
        None => serde_json::to_string_pretty(&summary)
            .expect("JSON serialization of valid structure cannot fail"),
    };
    // ast-grep-ignore: no-println
    println!("{json_output}");
    if result.failure_count > 0 {
        std::process::exit(1);
    }
    Ok(())
}

fn render_batch_text_summary(result: &crate::batch::BatchResult, output: &Output) {
    output.info("\n=== Batch Execution Summary ===");
    // ast-grep-ignore: no-println
    println!("Total operations: {}", result.results.len());
    // ast-grep-ignore: no-println
    println!("Successful: {}", result.success_count);
    // ast-grep-ignore: no-println
    println!("Failed: {}", result.failure_count);
    // ast-grep-ignore: no-println
    println!("Total time: {:.2}s", result.total_duration.as_secs_f64());

    if result.failure_count == 0 {
        return;
    }

    output.info("\nFailed operations:");
    for (i, op_result) in result.results.iter().enumerate() {
        if op_result.success {
            continue;
        }
        // ast-grep-ignore: no-println
        println!(
            "  {} - {}: {}",
            i + 1,
            op_result.operation.args.join(" "),
            op_result.error.as_deref().unwrap_or("Unknown error")
        );
    }

    std::process::exit(1);
}

/// Execute a command using shortcut resolution
pub async fn execute_shortcut_command(
    manager: &ConfigManager<OsFileSystem>,
    args: Vec<String>,
    cli: &Cli,
) -> Result<(), Error> {
    let output = Output::new(cli.quiet, cli.json_errors);

    if args.is_empty() {
        print_shortcut_usage();
    }

    let specs = manager.list_specs()?;
    if specs.is_empty() {
        output.info("No API specifications found. Use 'aperture config add' to register APIs.");
        return Ok(());
    }

    let all_specs = load_shortcut_specs(manager, &specs);
    if all_specs.is_empty() {
        output.info("No valid API specifications found.");
        return Ok(());
    }

    let mut resolver = ShortcutResolver::new();
    resolver.index_specs(&all_specs);
    handle_shortcut_resolution(&resolver, args, cli, &output).await
}

fn load_shortcut_specs(
    manager: &ConfigManager<OsFileSystem>,
    specs: &[String],
) -> std::collections::BTreeMap<String, crate::cache::models::CachedSpec> {
    let cache_dir = manager.config_dir().join(constants::DIR_CACHE);
    let mut all_specs = std::collections::BTreeMap::new();
    for spec_name in specs {
        match loader::load_cached_spec(&cache_dir, spec_name) {
            Ok(spec) => {
                all_specs.insert(spec_name.clone(), spec);
            }
            Err(e) => tracing::warn!(spec = spec_name, error = %e, "could not load spec"),
        }
    }
    all_specs
}

async fn handle_shortcut_resolution(
    resolver: &ShortcutResolver,
    args: Vec<String>,
    cli: &Cli,
    output: &Output,
) -> Result<(), Error> {
    match resolver.resolve_shortcut(&args) {
        ResolutionResult::Resolved(shortcut) => {
            output.info(format!(
                "Resolved shortcut to: aperture {}",
                shortcut.full_command.join(" ")
            ));
            let context = &shortcut.full_command[1];
            let operation_args = shortcut.full_command[2..].to_vec();
            let user_args = if args.len() > count_shortcut_args(&args) {
                args[count_shortcut_args(&args)..].to_vec()
            } else {
                Vec::new()
            };
            let final_args = [operation_args, user_args].concat();
            execute_api_command(context, final_args, cli).await
        }
        ResolutionResult::Ambiguous(matches) => {
            // Must appear regardless of APERTURE_LOG; tracing may suppress at low levels.
            // ast-grep-ignore: no-println
            eprintln!("Ambiguous shortcut. Multiple commands match:");
            // ast-grep-ignore: no-println
            eprintln!("{}", resolver.format_ambiguous_suggestions(&matches));
            // ast-grep-ignore: no-println
            eprintln!("\nTip: Use 'aperture search <term>' to explore available commands");
            std::process::exit(1);
        }
        ResolutionResult::NotFound => {
            // Must appear regardless of APERTURE_LOG; tracing may suppress at low levels.
            // ast-grep-ignore: no-println
            eprintln!("No command found for shortcut: {}", args.join(" "));
            // ast-grep-ignore: no-println
            eprintln!("Try one of these:");
            // ast-grep-ignore: no-println
            eprintln!(
                "  aperture search '{}'    # Search for similar commands",
                args[0]
            );
            // ast-grep-ignore: no-println
            eprintln!("  aperture list-commands <api>  # List available commands for an API");
            // ast-grep-ignore: no-println
            eprintln!("  aperture api <api> --help     # Show help for an API");
            std::process::exit(1);
        }
    }
}

fn print_shortcut_usage() -> ! {
    // Must appear regardless of APERTURE_LOG; tracing may suppress at low levels.
    // ast-grep-ignore: no-println
    eprintln!("Error: No command specified");
    // ast-grep-ignore: no-println
    eprintln!("Usage: aperture exec <shortcut> [args...]");
    // ast-grep-ignore: no-println
    eprintln!("Examples:");
    // ast-grep-ignore: no-println
    eprintln!("  aperture exec getUserById --id 123");
    // ast-grep-ignore: no-println
    eprintln!("  aperture exec GET /users/123");
    // ast-grep-ignore: no-println
    eprintln!("  aperture exec users list");
    std::process::exit(1);
}

fn count_shortcut_args(args: &[String]) -> usize {
    for (i, arg) in args.iter().enumerate() {
        if arg.starts_with('-') || arg.contains('=') {
            return i;
        }
    }
    std::cmp::min(args.len(), 3)
}

#[cfg(test)]
mod tests {
    use super::resolve_output_format;
    use crate::cli::OutputFormat;
    use clap::{Arg, Command};

    fn matches_from(args: &[&str]) -> clap::ArgMatches {
        Command::new("api")
            .arg(
                Arg::new("format")
                    .long("format")
                    .value_parser(["json", "yaml", "table"])
                    .default_value("json"),
            )
            .get_matches_from(args)
    }

    #[test]
    fn resolve_output_format_prefers_cli_value_when_dynamic_match_is_default() {
        let matches = matches_from(&["api"]);
        let resolved = resolve_output_format(&matches, &OutputFormat::Yaml);

        assert!(matches!(resolved, OutputFormat::Yaml));
    }

    #[test]
    fn resolve_output_format_honors_explicit_json_override() {
        let matches = matches_from(&["api", "--format", "json"]);
        let resolved = resolve_output_format(&matches, &OutputFormat::Yaml);

        assert!(matches!(resolved, OutputFormat::Json));
    }
}
