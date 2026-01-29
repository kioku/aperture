use aperture_cli::agent;
use aperture_cli::batch::{BatchConfig, BatchProcessor};
use aperture_cli::cache::models::CachedSpec;
use aperture_cli::cli::{Cli, Commands, ConfigCommands};
use aperture_cli::config::manager::{get_config_dir, ConfigManager};
use aperture_cli::config::models::{GlobalConfig, SecretSource};
use aperture_cli::constants;
use aperture_cli::docs::{DocumentationGenerator, HelpFormatter};
use aperture_cli::duration::parse_duration;
use aperture_cli::engine::executor::RetryContext;
use aperture_cli::engine::{executor, generator, loader};
use aperture_cli::error::Error;
use aperture_cli::fs::OsFileSystem;
use aperture_cli::interactive::confirm;
use aperture_cli::output::Output;
use aperture_cli::response_cache::{CacheConfig, ResponseCache};
use aperture_cli::search::{format_search_results, CommandSearcher};
use aperture_cli::shortcuts::{ResolutionResult, ShortcutResolver};
use clap::Parser;
use std::fs;
use std::path::PathBuf;
use std::time::Duration;
use tracing_subscriber::EnvFilter;

#[tokio::main]
async fn main() {
    // Install the aws-lc-rs crypto provider for rustls.
    // Ensures the provider is initialized before any TLS operations.
    let _ = rustls::crypto::aws_lc_rs::default_provider().install_default();

    // Initialize tracing-subscriber for request/response logging
    init_tracing();

    let cli = Cli::parse();
    let json_errors = cli.json_errors;
    let output = Output::new(cli.quiet, cli.json_errors);

    let manager = std::env::var(constants::ENV_APERTURE_CONFIG_DIR).map_or_else(
        |_| match ConfigManager::new() {
            Ok(manager) => manager,
            Err(e) => {
                print_error_with_json(&e, json_errors);
                std::process::exit(1);
            }
        },
        |config_dir| ConfigManager::with_fs(OsFileSystem, PathBuf::from(config_dir)),
    );

    if let Err(e) = run_command(cli, &manager, &output).await {
        print_error_with_json(&e, json_errors);
        std::process::exit(1);
    }
}

#[allow(clippy::too_many_lines, clippy::cognitive_complexity)]
async fn run_command(
    cli: Cli,
    manager: &ConfigManager<OsFileSystem>,
    output: &Output,
) -> Result<(), Error> {
    match cli.command {
        Commands::Config { command } => match command {
            ConfigCommands::Add {
                name,
                file_or_url,
                force,
                strict,
            } => {
                manager
                    .add_spec_auto(&name, &file_or_url, force, strict)
                    .await?;
                output.success(format!("Spec '{name}' added successfully."));
            }
            ConfigCommands::List { verbose } => {
                let specs = manager.list_specs()?;
                if specs.is_empty() {
                    output.info("No API specifications found.");
                } else {
                    output.info("Registered API specifications:");
                    list_specs_with_details(manager, specs, verbose, output);
                }
            }
            ConfigCommands::Remove { name } => {
                manager.remove_spec(&name)?;
                output.success(format!("Spec '{name}' removed successfully."));
            }
            ConfigCommands::Edit { name } => {
                manager.edit_spec(&name)?;
                output.success(format!("Opened spec '{name}' in editor."));
            }
            ConfigCommands::SetUrl { name, url, env } => {
                manager.set_url(&name, &url, env.as_deref())?;
                if let Some(environment) = env {
                    output.success(format!(
                        "Set base URL for '{name}' in environment '{environment}': {url}"
                    ));
                } else {
                    output.success(format!("Set base URL for '{name}': {url}"));
                }
            }
            ConfigCommands::GetUrl { name } => {
                let (base_override, env_urls, resolved) = manager.get_url(&name)?;
                print_url_configuration(
                    &name,
                    base_override.as_deref(),
                    &env_urls,
                    &resolved,
                    output,
                );
            }
            ConfigCommands::ListUrls {} => {
                let all_urls = manager.list_urls()?;

                if all_urls.is_empty() {
                    output.info("No base URLs configured.");
                    return Ok(());
                }

                output.info("Configured base URLs:");
                for (api_name, (base_override, env_urls)) in all_urls {
                    print_api_url_entry(&api_name, base_override.as_deref(), &env_urls, output);
                }
            }
            ConfigCommands::Reinit { context, all } => {
                if all {
                    reinit_all_specs(manager, output)?;
                    return Ok(());
                }

                let Some(spec_name) = context else {
                    eprintln!("Error: Either specify a spec name or use --all flag");
                    std::process::exit(1);
                };

                reinit_spec(manager, &spec_name, output)?;
            }
            ConfigCommands::ClearCache { api_name, all } => {
                clear_response_cache(manager, api_name.as_deref(), all, output).await?;
            }
            ConfigCommands::CacheStats { api_name } => {
                show_cache_stats(manager, api_name.as_deref(), output).await?;
            }
            ConfigCommands::SetSecret {
                api_name,
                scheme_name,
                env,
                interactive,
            } => {
                if interactive {
                    manager.set_secret_interactive(&api_name)?;
                    return Ok(());
                }

                let (Some(scheme), Some(env_var)) = (scheme_name, env) else {
                    return Err(Error::invalid_config(
                        "Either provide --scheme and --env, or use --interactive",
                    ));
                };

                manager.set_secret(&api_name, &scheme, &env_var)?;
                output.success(format!(
                    "Set secret for scheme '{scheme}' in API '{api_name}' to use environment variable '{env_var}'"
                ));
            }
            ConfigCommands::ListSecrets { api_name } => {
                let secrets = manager.list_secrets(&api_name)?;
                if secrets.is_empty() {
                    output.info(format!("No secrets configured for API '{api_name}'"));
                } else {
                    print_secrets_list(&api_name, secrets, output);
                }
            }
            ConfigCommands::RemoveSecret {
                api_name,
                scheme_name,
            } => {
                manager.remove_secret(&api_name, &scheme_name)?;
                output.success(format!(
                    "Removed secret configuration for scheme '{scheme_name}' from API '{api_name}'"
                ));
            }
            ConfigCommands::ClearSecrets { api_name, force } => {
                // Check if API exists and has secrets
                let secrets = manager.list_secrets(&api_name)?;
                if secrets.is_empty() {
                    output.info(format!("No secrets configured for API '{api_name}'"));
                    return Ok(());
                }

                // Confirm operation unless --force is used
                if force {
                    manager.clear_secrets(&api_name)?;
                    output.success(format!(
                        "Cleared all secret configurations for API '{api_name}'"
                    ));
                    return Ok(());
                }

                output.info(format!(
                    "This will remove all {} secret configuration(s) for API '{api_name}':",
                    secrets.len()
                ));
                for scheme_name in secrets.keys() {
                    output.info(format!("  - {scheme_name}"));
                }
                if !confirm("Are you sure you want to continue?")? {
                    output.info("Operation cancelled");
                    return Ok(());
                }

                manager.clear_secrets(&api_name)?;
                output.success(format!(
                    "Cleared all secret configurations for API '{api_name}'"
                ));
            }
            ConfigCommands::Set { key, value } => {
                use aperture_cli::config::settings::{SettingKey, SettingValue};

                let setting_key: SettingKey = key.parse()?;
                let setting_value = SettingValue::parse_for_key(setting_key, &value)?;
                manager.set_setting(&setting_key, &setting_value)?;
                output.success(format!("Set {key} = {value}"));
            }
            ConfigCommands::Get { key, json } => {
                use aperture_cli::config::settings::SettingKey;

                let setting_key: SettingKey = key.parse()?;
                let value = manager.get_setting(&setting_key)?;

                if json {
                    println!(
                        "{}",
                        serde_json::json!({
                            "key": key,
                            "value": value.to_string()
                        })
                    );
                } else {
                    println!("{value}");
                }
            }
            ConfigCommands::Settings { json } => {
                let settings = manager.list_settings()?;
                print_settings_list(settings, json, output)?;
            }
        },
        Commands::ListCommands { ref context } => {
            list_commands(context, output)?;
        }
        Commands::Api {
            ref context,
            ref args,
        } => {
            execute_api_command(context, args.clone(), &cli).await?;
        }
        Commands::Search {
            ref query,
            ref api,
            verbose,
        } => {
            execute_search_command(manager, query, api.as_deref(), verbose, output)?;
        }
        Commands::Exec { ref args } => {
            execute_shortcut_command(manager, args.clone(), &cli).await?;
        }
        Commands::Docs {
            ref api,
            ref tag,
            ref operation,
            enhanced,
        } => {
            execute_help_command(
                manager,
                api.as_deref(),
                tag.as_deref(),
                operation.as_deref(),
                enhanced,
                output,
            )?;
        }
        Commands::Overview { ref api, all } => {
            execute_overview_command(manager, api.as_deref(), all, output)?;
        }
    }

    Ok(())
}

/// Print the list of configured secrets for an API
fn print_secrets_list(
    api_name: &str,
    secrets: std::collections::HashMap<String, aperture_cli::config::models::ApertureSecret>,
    output: &Output,
) {
    // Header is informational
    output.info(format!("Configured secrets for API '{api_name}':"));
    // Secret configurations are data - always shown
    for (scheme_name, secret) in secrets {
        match secret.source {
            SecretSource::Env => {
                println!("  {scheme_name}: environment variable '{}'", secret.name);
            }
        }
    }
}

/// Print a single API URL entry in the list
fn print_api_url_entry(
    api_name: &str,
    base_override: Option<&str>,
    env_urls: &std::collections::HashMap<String, String>,
    output: &Output,
) {
    // API name is data - always shown
    println!("\n{api_name}:");
    if let Some(base) = base_override {
        // URL data - always shown
        println!("  Base override: {base}");
    }
    if !env_urls.is_empty() {
        // Sub-header is informational
        output.info("  Environment URLs:");
        // URL data - always shown
        for (env, url) in env_urls {
            println!("    {env}: {url}");
        }
    }
}

/// Print URL configuration for a specific API
fn print_url_configuration(
    name: &str,
    base_override: Option<&str>,
    env_urls: &std::collections::HashMap<String, String>,
    resolved: &str,
    output: &Output,
) {
    // Header is informational
    output.info(format!("Base URL configuration for '{name}':"));

    // URL data is always shown
    if let Some(base) = base_override {
        println!("  Base override: {base}");
    } else {
        println!("  Base override: (none)");
    }

    if !env_urls.is_empty() {
        println!("  Environment URLs:");
        for (env, url) in env_urls {
            println!("    {env}: {url}");
        }
    }

    println!("\nResolved URL (current): {resolved}");

    // Environment context is informational
    if let Ok(current_env) = std::env::var(constants::ENV_APERTURE_ENV) {
        output.info(format!("(Using APERTURE_ENV={current_env})"));
    }
}

fn execute_search_command(
    manager: &ConfigManager<OsFileSystem>,
    query: &str,
    api_filter: Option<&str>,
    verbose: bool,
    output: &Output,
) -> Result<(), Error> {
    // Get all registered APIs
    let specs = manager.list_specs()?;

    if specs.is_empty() {
        output.info("No API specifications found. Use 'aperture config add' to register APIs.");
        return Ok(());
    }

    // Load all cached specs
    let cache_dir = manager.config_dir().join(constants::DIR_CACHE);
    let mut all_specs = std::collections::BTreeMap::new();

    for spec_name in &specs {
        // Skip if we have an API filter and this isn't the one
        if api_filter.is_some_and(|filter| spec_name != filter) {
            continue;
        }

        match loader::load_cached_spec(&cache_dir, spec_name) {
            Ok(spec) => {
                all_specs.insert(spec_name.clone(), spec);
            }
            Err(e) => {
                eprintln!("Warning: Could not load spec '{spec_name}': {e}");
            }
        }
    }

    if all_specs.is_empty() {
        match api_filter {
            Some(filter) => {
                output.info(format!("API '{filter}' not found or could not be loaded."));
            }
            None => output.info("No API specifications could be loaded."),
        }
        return Ok(());
    }

    // Perform the search
    let searcher = CommandSearcher::new();
    let results = searcher.search(&all_specs, query, api_filter)?;

    // Format and display results - search results are data output
    let formatted_results = format_search_results(&results, verbose);
    for line in formatted_results {
        println!("{line}");
    }

    Ok(())
}

fn list_commands(context: &str, output: &Output) -> Result<(), Error> {
    // Get the cache directory - respecting APERTURE_CONFIG_DIR if set
    let config_dir = if let Ok(dir) = std::env::var(constants::ENV_APERTURE_CONFIG_DIR) {
        PathBuf::from(dir)
    } else {
        get_config_dir()?
    };
    let cache_dir = config_dir.join(constants::DIR_CACHE);

    // Load the cached spec for the context
    let spec = loader::load_cached_spec(&cache_dir, context).map_err(|e| match e {
        Error::Io(_) => Error::spec_not_found(context),
        _ => e,
    })?;

    // Use enhanced formatter for better output
    let formatted_output = HelpFormatter::format_command_list(&spec);
    println!("{formatted_output}");

    // Add helpful tips at the end (suppressed in quiet mode)
    output.tip(format!(
        "Use 'aperture docs {context}' for detailed API documentation"
    ));
    output.tip(format!(
        "Use 'aperture search <term> --api {context}' to find specific operations"
    ));
    output.tip("Use shortcuts: 'aperture exec <operation-id> --help'");

    Ok(())
}

fn reinit_spec(
    manager: &ConfigManager<OsFileSystem>,
    spec_name: &str,
    output: &Output,
) -> Result<(), Error> {
    output.info(format!("Reinitializing cached specification: {spec_name}"));

    // Check if the spec exists
    let specs = manager.list_specs()?;
    if !specs.contains(&spec_name.to_string()) {
        return Err(Error::spec_not_found(spec_name));
    }

    // Get the config directory
    let config_dir = if let Ok(dir) = std::env::var(constants::ENV_APERTURE_CONFIG_DIR) {
        PathBuf::from(dir)
    } else {
        get_config_dir()?
    };

    // Get the original spec file path
    let specs_dir = config_dir.join(constants::DIR_SPECS);
    let spec_path = specs_dir.join(format!("{spec_name}.yaml"));

    // Get the original strict mode preference (default to false if not set)
    let strict = manager.get_strict_preference(spec_name).unwrap_or(false);

    // Re-add the spec with force to regenerate the cache using original strict preference
    manager.add_spec(spec_name, &spec_path, true, strict)?;

    output.success(format!(
        "Successfully reinitialized cache for '{spec_name}'"
    ));
    Ok(())
}

fn reinit_all_specs(manager: &ConfigManager<OsFileSystem>, output: &Output) -> Result<(), Error> {
    let specs = manager.list_specs()?;

    if specs.is_empty() {
        output.info("No API specifications found to reinitialize.");
        return Ok(());
    }

    output.info(format!(
        "Reinitializing {} cached specification(s)...",
        specs.len()
    ));

    for spec_name in &specs {
        match reinit_spec(manager, spec_name, output) {
            Ok(()) => {
                output.info(format!("  {spec_name}"));
            }
            Err(e) => {
                eprintln!("  {spec_name}: {e}");
            }
        }
    }

    output.success("Reinitialization complete.");
    Ok(())
}

fn list_specs_with_details(
    manager: &ConfigManager<OsFileSystem>,
    specs: Vec<String>,
    verbose: bool,
    output: &Output,
) {
    let cache_dir = manager.config_dir().join(constants::DIR_CACHE);

    for spec_name in specs {
        if !verbose {
            // Simple output - just show spec name
            println!("- {spec_name}");
            continue;
        }

        // Verbose output - try to load cached spec for details
        let Ok(cached_spec) =
            aperture_cli::engine::loader::load_cached_spec(&cache_dir, &spec_name)
        else {
            // Fallback to simple output if cache can't be loaded
            println!("- {spec_name}");
            continue;
        };

        // Show spec name with colon for verbose format
        println!("- {spec_name}:");

        // Show version
        output.info(format!("  Version: {}", cached_spec.version));

        // Show endpoint statistics
        let available = cached_spec.commands.len();
        let skipped = cached_spec.skipped_endpoints.len();
        let total = available + skipped;

        if skipped > 0 {
            output.info(format!(
                "  Endpoints: {available} of {total} available ({skipped} skipped)"
            ));
            display_skipped_endpoints_info(&cached_spec, output);
        } else {
            output.info(format!("  Endpoints: {available} available"));
        }
    }
}

fn display_skipped_endpoints_info(
    cached_spec: &aperture_cli::cache::models::CachedSpec,
    output: &Output,
) {
    output.info("  Skipped endpoints:");

    for endpoint in &cached_spec.skipped_endpoints {
        output.info(format!(
            "    - {} {} - {} not supported",
            endpoint.method, endpoint.path, endpoint.content_type
        ));
    }
}

fn print_settings_list(
    settings: Vec<aperture_cli::config::settings::SettingInfo>,
    json: bool,
    output: &Output,
) -> Result<(), Error> {
    if json {
        println!("{}", serde_json::to_string_pretty(&settings)?);
        return Ok(());
    }

    output.info("Available configuration settings:");
    println!();
    for setting in settings {
        println!("  {} = {}", setting.key, setting.value);
        println!(
            "    Type: {}  Default: {}",
            setting.type_name, setting.default
        );
        println!("    {}", setting.description);
        println!();
    }
    Ok(())
}

#[allow(clippy::too_many_lines)]
async fn execute_api_command(context: &str, args: Vec<String>, cli: &Cli) -> Result<(), Error> {
    // Get the cache directory - respecting APERTURE_CONFIG_DIR if set
    let config_dir = if let Ok(dir) = std::env::var(constants::ENV_APERTURE_CONFIG_DIR) {
        PathBuf::from(dir)
    } else {
        get_config_dir()?
    };
    let cache_dir = config_dir.join(constants::DIR_CACHE);

    // Create config manager and load global config
    let manager = ConfigManager::with_fs(OsFileSystem, config_dir.clone());
    let global_config = manager.load_global_config().ok();

    // Load the cached spec for the context
    let spec = loader::load_cached_spec(&cache_dir, context).map_err(|e| match e {
        Error::Io(_) => Error::spec_not_found(context),
        _ => e,
    })?;

    // Handle --describe-json flag - output capability manifest and exit
    if cli.describe_json {
        // Load the original spec file for complete metadata
        let specs_dir = config_dir.join(constants::DIR_SPECS);
        let spec_path = specs_dir.join(format!("{context}.yaml"));

        // Check path exists inline to avoid nested if
        // ast-grep-ignore: no-nested-if
        if !spec_path.exists() {
            return Err(Error::spec_not_found(context));
        }

        let spec_content = fs::read_to_string(&spec_path)?;
        let openapi_spec = aperture_cli::spec::parse_openapi(&spec_content)
            .map_err(|e| Error::invalid_config(format!("Failed to parse OpenAPI spec: {e}")))?;

        // Generate manifest from the original spec with all metadata
        // Pass the cached spec for endpoint statistics (available vs skipped)
        let manifest = agent::generate_capability_manifest_from_openapi(
            context,
            &openapi_spec,
            &spec,
            global_config.as_ref(),
        )?;

        // Apply JQ filter if provided
        let output = match &cli.jq {
            Some(jq_filter) => executor::apply_jq_filter(&manifest, jq_filter)?,
            None => manifest,
        };

        println!("{output}");
        return Ok(());
    }

    // Handle --batch-file flag - execute batch operations and exit
    if let Some(batch_file_path) = &cli.batch_file {
        return execute_batch_operations(
            context,
            batch_file_path,
            &spec,
            global_config.as_ref(),
            cli,
        )
        .await;
    }

    // Generate the dynamic command tree
    let command = generator::generate_command_tree_with_flags(&spec, cli.positional_args);

    // Parse the arguments against the dynamic command
    let matches = command
        .try_get_matches_from(std::iter::once(constants::CLI_ROOT_COMMAND.to_string()).chain(args))
        .map_err(|e| Error::invalid_command(context, e.to_string()))?;

    // Extract JQ filter from dynamic matches (takes precedence) or CLI global flag
    let jq_filter = matches
        .get_one::<String>("jq")
        .map(String::as_str)
        .or(cli.jq.as_deref());

    // Extract format from dynamic matches or fall back to CLI global flag
    // Only override the CLI format if the dynamic format was explicitly provided (not default)
    let output_format = matches.get_one::<String>("format").map_or_else(
        || cli.format.clone(),
        |format_str| {
            // Check if the user explicitly provided a format or if it's the default
            // If the CLI format is not the default Json, use the CLI format
            let is_default_json = format_str == "json"
                && !matches!(cli.format, aperture_cli::cli::OutputFormat::Json);

            if is_default_json {
                // User didn't explicitly set format in dynamic command, use CLI global format
                return cli.format.clone();
            }

            match format_str.as_str() {
                "json" => aperture_cli::cli::OutputFormat::Json,
                "yaml" => aperture_cli::cli::OutputFormat::Yaml,
                "table" => aperture_cli::cli::OutputFormat::Table,
                _ => cli.format.clone(),
            }
        },
    );

    // Create cache configuration from CLI flags
    let cache_config = if cli.no_cache {
        None
    } else {
        Some(CacheConfig {
            cache_dir: config_dir
                .join(constants::DIR_CACHE)
                .join(constants::DIR_RESPONSES),
            default_ttl: Duration::from_secs(cli.cache_ttl.unwrap_or(300)),
            max_entries: 1000,
            enabled: cli.cache || cli.cache_ttl.is_some(),
        })
    };

    // Build retry configuration from CLI flags and global config defaults
    let retry_context = build_retry_context(cli, global_config.as_ref())?;

    // Execute the request with agent flags
    executor::execute_request(
        &spec,
        &matches,
        None, // base_url (None = use BaseUrlResolver)
        cli.dry_run,
        cli.idempotency_key.as_deref(),
        global_config.as_ref(),
        &output_format,
        jq_filter,
        cache_config.as_ref(),
        false, // capture_output
        retry_context.as_ref(),
    )
    .await
    .map_err(|e| {
        let Error::Network(req_err) = &e else {
            return e;
        };

        if req_err.is_connect() {
            return e.with_context("Failed to connect to API server");
        }

        if req_err.is_timeout() {
            return e.with_context("Request timed out");
        }

        e
    })?;

    Ok(())
}

/// Executes batch operations from a batch file
async fn execute_batch_operations(
    _context: &str,
    batch_file_path: &str,
    spec: &CachedSpec,
    global_config: Option<&GlobalConfig>,
    cli: &Cli,
) -> Result<(), Error> {
    // Parse the batch file
    let batch_file =
        BatchProcessor::parse_batch_file(std::path::Path::new(batch_file_path)).await?;

    // Create batch configuration from CLI options
    // Quiet mode (--quiet or --json-errors) suppresses progress output
    let batch_config = BatchConfig {
        max_concurrency: cli.batch_concurrency,
        rate_limit: cli.batch_rate_limit,
        continue_on_error: true, // Default to continuing on error for batch operations
        show_progress: !cli.quiet && !cli.json_errors, // Disable progress in quiet mode
        suppress_output: cli.json_errors, // Suppress individual outputs when using JSON output
    };

    // Create batch processor
    let processor = BatchProcessor::new(batch_config);

    // Execute the batch
    let result = processor
        .execute_batch(
            spec,
            batch_file,
            global_config,
            None, // base_url (None = use BaseUrlResolver)
            cli.dry_run,
            &cli.format,
            None, // Don't pass JQ filter to individual operations
        )
        .await?;

    // Create output handler for consistent quiet mode behavior
    let output = Output::new(cli.quiet, cli.json_errors);

    // Print batch results summary
    if cli.json_errors {
        // Output structured JSON summary
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

        // Apply JQ filter if provided
        let json_output = match &cli.jq {
            Some(jq_filter) => {
                let summary_json = serde_json::to_string(&summary)
                    .expect("JSON serialization of valid structure cannot fail");
                executor::apply_jq_filter(&summary_json, jq_filter)?
            }
            None => serde_json::to_string_pretty(&summary)
                .expect("JSON serialization of valid structure cannot fail"),
        };

        println!("{json_output}");
        // Exit with error code if any operations failed
        // ast-grep-ignore: no-nested-if
        if result.failure_count > 0 {
            std::process::exit(1);
        }
        return Ok(());
    }

    // Output human-readable summary
    // Header is informational, stats are data
    output.info("\n=== Batch Execution Summary ===");
    println!("Total operations: {}", result.results.len());
    println!("Successful: {}", result.success_count);
    println!("Failed: {}", result.failure_count);
    println!("Total time: {:.2}s", result.total_duration.as_secs_f64());

    if result.failure_count == 0 {
        return Ok(());
    }

    output.info("\nFailed operations:");
    for (i, op_result) in result.results.iter().enumerate() {
        if op_result.success {
            continue;
        }
        // Failed operation details are data output
        println!(
            "  {} - {}: {}",
            i + 1,
            op_result.operation.args.join(" "),
            op_result.error.as_deref().unwrap_or("Unknown error")
        );
    }

    // Exit with error code if any operations failed
    if result.failure_count > 0 {
        std::process::exit(1);
    }

    Ok(())
}

/// Initialize tracing-subscriber for request/response logging
fn init_tracing() {
    use tracing_subscriber::fmt::format::FmtSpan;
    use tracing_subscriber::layer::SubscriberExt;
    use tracing_subscriber::util::SubscriberInitExt;

    // Get log level from APERTURE_LOG environment variable (defaults to "error")
    let env_filter = EnvFilter::try_from_default_env()
        .or_else(|_| EnvFilter::try_new("error"))
        .unwrap_or_else(|_| EnvFilter::new("error"));

    // Get log format from APERTURE_LOG_FORMAT environment variable (defaults to "text")
    let log_format = std::env::var("APERTURE_LOG_FORMAT")
        .map_or_else(|_| "text".to_string(), |s| s.to_lowercase());

    if log_format == "json" {
        let json_layer = tracing_subscriber::fmt::layer()
            .json()
            .with_span_list(false)
            .with_target(true)
            .with_thread_ids(false)
            .with_line_number(true);

        tracing_subscriber::registry()
            .with(env_filter)
            .with(json_layer)
            .init();
    } else {
        // Default text format
        let fmt_layer = tracing_subscriber::fmt::layer()
            .pretty()
            .with_span_events(FmtSpan::CLOSE)
            .with_target(false)
            .with_thread_ids(false)
            .with_line_number(false);

        tracing_subscriber::registry()
            .with(env_filter)
            .with(fmt_layer)
            .init();
    }
}

/// Prints an error message, either as JSON or user-friendly format
fn print_error_with_json(error: &Error, json_format: bool) {
    if !json_format {
        print_error(error);
        return;
    }

    let json_error = error.to_json();
    let Ok(json_output) = serde_json::to_string_pretty(&json_error) else {
        // Fallback to regular error if JSON serialization fails
        print_error(error);
        return;
    };

    eprintln!("{json_output}");
}

/// Clear response cache for a specific API or all APIs
async fn clear_response_cache(
    _manager: &ConfigManager<OsFileSystem>,
    api_name: Option<&str>,
    all: bool,
    output: &Output,
) -> Result<(), Error> {
    let config_dir = if let Ok(dir) = std::env::var(constants::ENV_APERTURE_CONFIG_DIR) {
        PathBuf::from(dir)
    } else {
        get_config_dir()?
    };

    let cache_config = CacheConfig {
        cache_dir: config_dir
            .join(constants::DIR_CACHE)
            .join(constants::DIR_RESPONSES),
        ..Default::default()
    };

    let cache = ResponseCache::new(cache_config)?;

    let cleared_count = if all {
        cache.clear_all().await?
    } else {
        let Some(api) = api_name else {
            eprintln!("Error: Either specify an API name or use --all flag");
            std::process::exit(1);
        };
        cache.clear_api_cache(api).await?
    };

    if all {
        output.success(format!(
            "Cleared {cleared_count} cached responses for all APIs"
        ));
    } else {
        let Some(api) = api_name else {
            // This should never be reached due to the earlier check, but keeping for symmetry
            unreachable!("API name must be Some if not all");
        };
        output.success(format!(
            "Cleared {cleared_count} cached responses for API '{api}'"
        ));
    }

    Ok(())
}

/// Show cache statistics for a specific API or all APIs
async fn show_cache_stats(
    _manager: &ConfigManager<OsFileSystem>,
    api_name: Option<&str>,
    output: &Output,
) -> Result<(), Error> {
    let config_dir = if let Ok(dir) = std::env::var(constants::ENV_APERTURE_CONFIG_DIR) {
        PathBuf::from(dir)
    } else {
        get_config_dir()?
    };

    let cache_config = CacheConfig {
        cache_dir: config_dir
            .join(constants::DIR_CACHE)
            .join(constants::DIR_RESPONSES),
        ..Default::default()
    };

    let cache = ResponseCache::new(cache_config)?;
    let stats = cache.get_stats(api_name).await?;

    // Header is informational
    if let Some(api) = api_name {
        output.info(format!("Cache statistics for API '{api}':"));
    } else {
        output.info("Cache statistics for all APIs:");
    }

    // Stats are data output - always shown
    println!("  Total entries: {}", stats.total_entries);
    println!("  Valid entries: {}", stats.valid_entries);
    println!("  Expired entries: {}", stats.expired_entries);
    #[allow(clippy::cast_precision_loss)]
    let size_mb = stats.total_size_bytes as f64 / 1024.0 / 1024.0;
    println!("  Total size: {size_mb:.2} MB");

    if stats.total_entries == 0 {
        // No entries, skip hit rate calculation
    } else {
        #[allow(clippy::cast_precision_loss)]
        let hit_rate = stats.valid_entries as f64 / stats.total_entries as f64 * 100.0;
        println!("  Hit rate: {hit_rate:.1}%");
    }

    Ok(())
}

/// Prints a user-friendly error message with context and suggestions
#[allow(clippy::too_many_lines)]
fn print_error(error: &Error) {
    match error {
        Error::Internal {
            kind,
            message,
            context,
        } => {
            eprintln!("{kind}: {message}");
            let Some(ctx) = context else {
                return;
            };
            if let Some(suggestion) = &ctx.suggestion {
                eprintln!("\nHint: {suggestion}");
            }
        }
        Error::Io(io_err) => match io_err.kind() {
            std::io::ErrorKind::NotFound => {
                eprintln!(
                    "File Not Found\n{io_err}\n\nHint: {}",
                    constants::ERR_FILE_NOT_FOUND
                );
            }
            std::io::ErrorKind::PermissionDenied => {
                eprintln!(
                    "Permission Denied\n{io_err}\n\nHint: {}",
                    constants::ERR_PERMISSION
                );
            }
            _ => {
                eprintln!("File System Error\n{io_err}");
            }
        },
        Error::Network(req_err) => {
            if req_err.is_connect() {
                eprintln!(
                    "Connection Error\n{req_err}\n\nHint: {}",
                    constants::ERR_CONNECTION
                );
                return;
            }

            if req_err.is_timeout() {
                eprintln!(
                    "Timeout Error\n{req_err}\n\nHint: {}",
                    constants::ERR_TIMEOUT
                );
                return;
            }

            if !req_err.is_status() {
                eprintln!("Network Error\n{req_err}");
                return;
            }

            let Some(status) = req_err.status() else {
                eprintln!("Network Error\n{req_err}");
                return;
            };

            match status.as_u16() {
                401 => eprintln!(
                    "Authentication Error\n{req_err}\n\nHint: {}",
                    constants::ERR_API_CREDENTIALS
                ),
                403 => eprintln!(
                    "Permission Error\n{req_err}\n\nHint: {}",
                    constants::ERR_PERMISSION_DENIED
                ),
                404 => eprintln!(
                    "Not Found Error\n{req_err}\n\nHint: {}",
                    constants::ERR_ENDPOINT_NOT_FOUND
                ),
                429 => eprintln!(
                    "Rate Limited\n{req_err}\n\nHint: {}",
                    constants::ERR_RATE_LIMITED
                ),
                500..=599 => eprintln!(
                    "Server Error\n{req_err}\n\nHint: {}",
                    constants::ERR_SERVER_ERROR
                ),
                _ => eprintln!("HTTP Error\n{req_err}"),
            }
        }
        Error::Yaml(yaml_err) => {
            eprintln!(
                "YAML Parsing Error\n{yaml_err}\n\nHint: {}",
                constants::ERR_YAML_SYNTAX
            );
        }
        Error::Json(json_err) => {
            eprintln!(
                "JSON Parsing Error\n{json_err}\n\nHint: {}",
                constants::ERR_JSON_SYNTAX
            );
        }
        Error::Toml(toml_err) => {
            eprintln!(
                "TOML Parsing Error\n{toml_err}\n\nHint: {}",
                constants::ERR_TOML_SYNTAX
            );
        }
        Error::Anyhow(anyhow_err) => {
            eprintln!("Error\n{anyhow_err}");
        }
    }
}

/// Execute a command using shortcut resolution
async fn execute_shortcut_command(
    manager: &ConfigManager<OsFileSystem>,
    args: Vec<String>,
    cli: &Cli,
) -> Result<(), Error> {
    // Create output handler for consistent quiet mode behavior
    let output = Output::new(cli.quiet, cli.json_errors);

    if args.is_empty() {
        eprintln!("Error: No command specified");
        eprintln!("Usage: aperture exec <shortcut> [args...]");
        eprintln!("Examples:");
        eprintln!("  aperture exec getUserById --id 123");
        eprintln!("  aperture exec GET /users/123");
        eprintln!("  aperture exec users list");
        std::process::exit(1);
    }

    // Load all available specs for resolution
    let specs = manager.list_specs()?;
    if specs.is_empty() {
        output.info("No API specifications found. Use 'aperture config add' to register APIs.");
        return Ok(());
    }

    // Load all cached specs
    let cache_dir = manager.config_dir().join(constants::DIR_CACHE);
    let mut all_specs = std::collections::BTreeMap::new();

    for spec_name in &specs {
        match loader::load_cached_spec(&cache_dir, spec_name) {
            Ok(spec) => {
                all_specs.insert(spec_name.clone(), spec);
            }
            Err(e) => {
                eprintln!("Warning: Could not load spec '{spec_name}': {e}");
            }
        }
    }

    if all_specs.is_empty() {
        output.info("No valid API specifications found.");
        return Ok(());
    }

    // Initialize and index shortcut resolver
    let mut resolver = ShortcutResolver::new();
    resolver.index_specs(&all_specs);

    // Try to resolve the shortcut
    match resolver.resolve_shortcut(&args) {
        ResolutionResult::Resolved(shortcut) => {
            // Found a unique match - show resolution info (informational)
            output.info(format!(
                "Resolved shortcut to: aperture {}",
                shortcut.full_command.join(" ")
            ));

            // Extract the context (API name) and remaining args
            let context = &shortcut.full_command[1]; // Skip "api"
            let operation_args = shortcut.full_command[2..].to_vec(); // Skip "api" and context

            // Add the remaining user arguments (everything after the shortcut pattern)
            let user_args = if args.len() > count_shortcut_args(&args) {
                args[count_shortcut_args(&args)..].to_vec()
            } else {
                Vec::new()
            };

            let final_args = [operation_args, user_args].concat();

            // Execute the resolved command
            execute_api_command(context, final_args, cli).await
        }
        ResolutionResult::Ambiguous(matches) => {
            // Multiple matches found - show suggestions
            eprintln!("Ambiguous shortcut. Multiple commands match:");
            eprintln!("{}", resolver.format_ambiguous_suggestions(&matches));
            eprintln!("\nTip: Use 'aperture search <term>' to explore available commands");
            std::process::exit(1);
        }
        ResolutionResult::NotFound => {
            // No matches found - suggest alternatives
            eprintln!("No command found for shortcut: {}", args.join(" "));
            eprintln!("Try one of these:");
            eprintln!(
                "  aperture search '{}'    # Search for similar commands",
                args[0]
            );
            eprintln!("  aperture list-commands <api>  # List available commands for an API");
            eprintln!("  aperture api <api> --help     # Show help for an API");
            std::process::exit(1);
        }
    }
}

/// Count how many arguments are part of the shortcut pattern
/// This helps separate shortcut args from parameter args
fn count_shortcut_args(args: &[String]) -> usize {
    // Simple heuristic: count until we hit a flag (starts with -) or known parameter pattern
    for (i, arg) in args.iter().enumerate() {
        if arg.starts_with('-') || arg.contains('=') {
            return i;
        }
    }

    // If no flags found, assume up to 3 args can be shortcut components
    // (e.g., "users", "get", "by-id" but not more than that)
    std::cmp::min(args.len(), 3)
}

/// Execute help command with enhanced documentation
fn execute_help_command(
    manager: &ConfigManager<OsFileSystem>,
    api_name: Option<&str>,
    tag: Option<&str>,
    operation: Option<&str>,
    enhanced: bool,
    output: &Output,
) -> Result<(), Error> {
    match (api_name, tag, operation) {
        // No arguments - show interactive help menu
        (None, None, None) => {
            let specs = load_all_specs(manager)?;
            let doc_gen = DocumentationGenerator::new(specs);
            // Interactive menu is data output
            println!("{}", doc_gen.generate_interactive_menu());
        }
        // API specified - show API overview or specific command help
        (Some(api), tag_opt, operation_opt) => {
            let specs = load_all_specs(manager)?;
            let doc_gen = DocumentationGenerator::new(specs);

            match (tag_opt, operation_opt) {
                // Just API name - show API overview
                (None, None) => {
                    let overview = doc_gen.generate_api_overview(api)?;
                    // API overview is data output
                    println!("{overview}");
                }
                // API and tag and operation - show detailed command help
                (Some(tag), Some(op)) => {
                    let help = doc_gen.generate_command_help(api, tag, op)?;
                    if enhanced {
                        // Full help is data output
                        println!("{help}");
                    } else {
                        // Simplified help is data output
                        println!("{}", help.lines().take(20).collect::<Vec<_>>().join("\n"));
                        // Tip about enhanced mode
                        output.tip("Use --enhanced for full documentation with examples");
                    }
                }
                // Invalid combination
                _ => {
                    eprintln!("Invalid docs command. Usage:");
                    eprintln!("  aperture docs                        # Interactive menu");
                    eprintln!("  aperture docs <api>                  # API overview");
                    eprintln!("  aperture docs <api> <tag> <operation> # Command help");
                    std::process::exit(1);
                }
            }
        }
        // Invalid combination
        _ => {
            eprintln!("Invalid help command arguments");
            std::process::exit(1);
        }
    }

    Ok(())
}

/// Execute overview command
fn execute_overview_command(
    manager: &ConfigManager<OsFileSystem>,
    api_name: Option<&str>,
    all: bool,
    output: &Output,
) -> Result<(), Error> {
    if !all {
        let Some(api) = api_name else {
            eprintln!("Error: Must specify API name or use --all flag");
            eprintln!("Usage:");
            eprintln!("  aperture overview <api>");
            eprintln!("  aperture overview --all");
            std::process::exit(1);
        };

        let specs = load_all_specs(manager)?;
        let doc_gen = DocumentationGenerator::new(specs);
        let overview = doc_gen.generate_api_overview(api)?;
        // Overview is data output
        println!("{overview}");
        return Ok(());
    }

    let specs = load_all_specs(manager)?;
    if specs.is_empty() {
        output.info("No API specifications configured.");
        output.info("Use 'aperture config add <name> <spec-file>' to get started.");
        return Ok(());
    }

    // Overview data is always shown
    println!("All APIs Overview\n");
    println!("{}", "=".repeat(60));

    for (api_name, spec) in &specs {
        println!("\n** {} ** (v{})", spec.name, spec.version);

        if let Some(ref base_url) = spec.base_url {
            println!("   Base URL: {base_url}");
        }

        let operation_count = spec.commands.len();
        println!("   Operations: {operation_count}");

        // Count methods
        let mut method_counts = std::collections::BTreeMap::new();
        for command in &spec.commands {
            *method_counts.entry(command.method.clone()).or_insert(0) += 1;
        }

        let method_summary: Vec<String> = method_counts
            .iter()
            .map(|(method, count)| format!("{method}: {count}"))
            .collect();
        println!("   Methods: {}", method_summary.join(", "));

        println!("   Quick start: aperture list-commands {api_name}");
    }

    println!("\n{}", "=".repeat(60));
    // Tip is suppressed in quiet mode
    output.tip("Use 'aperture overview <api>' for detailed information about a specific API");

    Ok(())
}

/// Load all cached specs from the manager
fn load_all_specs(
    manager: &ConfigManager<OsFileSystem>,
) -> Result<std::collections::BTreeMap<String, CachedSpec>, Error> {
    let specs = manager.list_specs()?;
    let cache_dir = manager.config_dir().join(constants::DIR_CACHE);
    let mut all_specs = std::collections::BTreeMap::new();

    for spec_name in &specs {
        match loader::load_cached_spec(&cache_dir, spec_name) {
            Ok(spec) => {
                all_specs.insert(spec_name.clone(), spec);
            }
            Err(e) => {
                eprintln!("Warning: Could not load spec '{spec_name}': {e}");
            }
        }
    }

    Ok(all_specs)
}

/// Builds a `RetryContext` from CLI flags and global configuration.
///
/// CLI flags take precedence over global config defaults.
#[allow(clippy::cast_possible_truncation)]
fn build_retry_context(
    cli: &Cli,
    global_config: Option<&GlobalConfig>,
) -> Result<Option<RetryContext>, Error> {
    // Get retry defaults from global config
    let defaults = global_config.map(|c| &c.retry_defaults);

    // Determine max_attempts: CLI > global config > 0 (disabled)
    let max_attempts = cli
        .retry
        .or_else(|| defaults.map(|d| d.max_attempts))
        .unwrap_or(0);

    // If retries are disabled, return None
    if max_attempts == 0 {
        return Ok(None);
    }

    // Determine initial_delay_ms: CLI > global config > 500ms default
    // Truncation is safe: delay values in practice are well under u64::MAX milliseconds
    let initial_delay_ms = if let Some(ref delay_str) = cli.retry_delay {
        parse_duration(delay_str)?.as_millis() as u64
    } else {
        defaults.map_or(500, |d| d.initial_delay_ms)
    };

    // Determine max_delay_ms: CLI > global config > 30000ms default
    // Truncation is safe: delay values in practice are well under u64::MAX milliseconds
    let max_delay_ms = if let Some(ref delay_str) = cli.retry_max_delay {
        parse_duration(delay_str)?.as_millis() as u64
    } else {
        defaults.map_or(30_000, |d| d.max_delay_ms)
    };

    // Check for idempotency key
    let has_idempotency_key = cli.idempotency_key.is_some();

    Ok(Some(RetryContext {
        max_attempts,
        initial_delay_ms,
        max_delay_ms,
        force_retry: cli.force_retry,
        method: None, // Will be determined in executor
        has_idempotency_key,
    }))
}
