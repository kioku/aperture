use aperture_cli::agent;
use aperture_cli::batch::{BatchConfig, BatchProcessor};
use aperture_cli::cache::models::CachedSpec;
use aperture_cli::cli::{Cli, Commands, ConfigCommands};
use aperture_cli::config::manager::{get_config_dir, ConfigManager};
use aperture_cli::config::models::{GlobalConfig, SecretSource};
use aperture_cli::engine::{executor, generator, loader};
use aperture_cli::error::Error;
use aperture_cli::fs::OsFileSystem;
use aperture_cli::response_cache::{CacheConfig, ResponseCache};
use clap::Parser;
use std::fs;
use std::path::PathBuf;
use std::time::Duration;

#[tokio::main]
async fn main() {
    let cli = Cli::parse();
    let json_errors = cli.json_errors;

    let manager = std::env::var("APERTURE_CONFIG_DIR").map_or_else(
        |_| match ConfigManager::new() {
            Ok(manager) => manager,
            Err(e) => {
                print_error_with_json(&e, json_errors);
                std::process::exit(1);
            }
        },
        |config_dir| ConfigManager::with_fs(OsFileSystem, PathBuf::from(config_dir)),
    );

    if let Err(e) = run_command(cli, &manager).await {
        print_error_with_json(&e, json_errors);
        std::process::exit(1);
    }
}

#[allow(clippy::too_many_lines)]
async fn run_command(cli: Cli, manager: &ConfigManager<OsFileSystem>) -> Result<(), Error> {
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
                println!("Spec '{name}' added successfully.");
            }
            ConfigCommands::List { verbose } => {
                let specs = manager.list_specs()?;
                if specs.is_empty() {
                    println!("No API specifications found.");
                } else {
                    println!("Registered API specifications:");
                    list_specs_with_details(manager, specs, verbose);
                }
            }
            ConfigCommands::Remove { name } => {
                manager.remove_spec(&name)?;
                println!("Spec '{name}' removed successfully.");
            }
            ConfigCommands::Edit { name } => {
                manager.edit_spec(&name)?;
                println!("Opened spec '{name}' in editor.");
            }
            ConfigCommands::SetUrl { name, url, env } => {
                manager.set_url(&name, &url, env.as_deref())?;
                if let Some(environment) = env {
                    println!("Set base URL for '{name}' in environment '{environment}': {url}");
                } else {
                    println!("Set base URL for '{name}': {url}");
                }
            }
            ConfigCommands::GetUrl { name } => {
                let (base_override, env_urls, resolved) = manager.get_url(&name)?;

                println!("Base URL configuration for '{name}':");
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

                if let Ok(current_env) = std::env::var("APERTURE_ENV") {
                    println!("(Using APERTURE_ENV={current_env})");
                }
            }
            ConfigCommands::ListUrls {} => {
                let all_urls = manager.list_urls()?;

                if all_urls.is_empty() {
                    println!("No base URLs configured.");
                } else {
                    println!("Configured base URLs:");
                    for (api_name, (base_override, env_urls)) in all_urls {
                        println!("\n{api_name}:");
                        if let Some(base) = base_override {
                            println!("  Base override: {base}");
                        }
                        if !env_urls.is_empty() {
                            println!("  Environment URLs:");
                            for (env, url) in env_urls {
                                println!("    {env}: {url}");
                            }
                        }
                    }
                }
            }
            ConfigCommands::Reinit { context, all } => {
                if all {
                    reinit_all_specs(manager)?;
                } else if let Some(spec_name) = context {
                    reinit_spec(manager, &spec_name)?;
                } else {
                    eprintln!("Error: Either specify a spec name or use --all flag");
                    std::process::exit(1);
                }
            }
            ConfigCommands::ClearCache { api_name, all } => {
                clear_response_cache(manager, api_name.as_deref(), all).await?;
            }
            ConfigCommands::CacheStats { api_name } => {
                show_cache_stats(manager, api_name.as_deref()).await?;
            }
            ConfigCommands::SetSecret {
                api_name,
                scheme_name,
                env,
                interactive,
            } => {
                if interactive {
                    manager.set_secret_interactive(&api_name)?;
                } else if let (Some(scheme), Some(env_var)) = (scheme_name, env) {
                    manager.set_secret(&api_name, &scheme, &env_var)?;
                    println!("Set secret for scheme '{scheme}' in API '{api_name}' to use environment variable '{env_var}'");
                } else {
                    return Err(Error::InvalidConfig {
                        reason: "Either provide --scheme and --env, or use --interactive"
                            .to_string(),
                    });
                }
            }
            ConfigCommands::ListSecrets { api_name } => {
                let secrets = manager.list_secrets(&api_name)?;
                if secrets.is_empty() {
                    println!("No secrets configured for API '{api_name}'");
                } else {
                    println!("Configured secrets for API '{api_name}':");
                    for (scheme_name, secret) in secrets {
                        match secret.source {
                            SecretSource::Env => {
                                println!("  {scheme_name}: environment variable '{}'", secret.name);
                            }
                        }
                    }
                }
            }
            ConfigCommands::RemoveSecret {
                api_name,
                scheme_name,
            } => {
                manager.remove_secret(&api_name, &scheme_name)?;
                println!("✓ Removed secret configuration for scheme '{scheme_name}' from API '{api_name}'");
            }
            ConfigCommands::ClearSecrets { api_name, force } => {
                // Check if API exists and has secrets
                let secrets = manager.list_secrets(&api_name)?;
                if secrets.is_empty() {
                    println!("No secrets configured for API '{api_name}'");
                    return Ok(());
                }

                // Confirm operation unless --force is used
                if !force {
                    use aperture_cli::interactive::confirm;
                    println!(
                        "This will remove all {} secret configuration(s) for API '{api_name}':",
                        secrets.len()
                    );
                    for scheme_name in secrets.keys() {
                        println!("  - {scheme_name}");
                    }
                    if !confirm("Are you sure you want to continue?")? {
                        println!("Operation cancelled");
                        return Ok(());
                    }
                }

                manager.clear_secrets(&api_name)?;
                println!("✓ Cleared all secret configurations for API '{api_name}'");
            }
        },
        Commands::ListCommands { ref context } => {
            list_commands(context)?;
        }
        Commands::Api {
            ref context,
            ref args,
        } => {
            execute_api_command(context, args.clone(), &cli).await?;
        }
    }

    Ok(())
}

fn list_commands(context: &str) -> Result<(), Error> {
    // Get the cache directory - respecting APERTURE_CONFIG_DIR if set
    let config_dir = if let Ok(dir) = std::env::var("APERTURE_CONFIG_DIR") {
        PathBuf::from(dir)
    } else {
        get_config_dir()?
    };
    let cache_dir = config_dir.join(".cache");

    // Load the cached spec for the context
    let spec = loader::load_cached_spec(&cache_dir, context).map_err(|e| match e {
        Error::Io(_) => Error::SpecNotFound {
            name: context.to_string(),
        },
        _ => e,
    })?;

    // Group commands by their primary tag
    let mut tag_groups: std::collections::BTreeMap<
        String,
        Vec<&aperture_cli::cache::models::CachedCommand>,
    > = std::collections::BTreeMap::new();

    for command in &spec.commands {
        let primary_tag = command
            .tags
            .first()
            .map_or_else(|| "default".to_string(), std::clone::Clone::clone);
        tag_groups.entry(primary_tag).or_default().push(command);
    }

    println!("Available commands for API: {}", spec.name);
    println!("API Version: {}", spec.version);
    if let Some(base_url) = &spec.base_url {
        println!("Base URL: {base_url}");
    }
    println!();

    if tag_groups.is_empty() {
        println!("No commands available for this API.");
        return Ok(());
    }

    for (tag, commands) in tag_groups {
        println!("📁 {tag}");
        for command in commands {
            let kebab_id = to_kebab_case(&command.operation_id);
            let description = command
                .summary
                .as_ref()
                .or(command.description.as_ref())
                .map(|s| format!(" - {s}"))
                .unwrap_or_default();
            println!(
                "  ├─ {} ({}){}",
                kebab_id,
                command.method.to_uppercase(),
                description
            );
        }
        println!();
    }

    Ok(())
}

/// Converts a string to kebab-case (copied from generator.rs)
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

fn reinit_spec(manager: &ConfigManager<OsFileSystem>, spec_name: &str) -> Result<(), Error> {
    println!("Reinitializing cached specification: {spec_name}");

    // Check if the spec exists
    let specs = manager.list_specs()?;
    if !specs.contains(&spec_name.to_string()) {
        return Err(Error::SpecNotFound {
            name: spec_name.to_string(),
        });
    }

    // Get the config directory
    let config_dir = if let Ok(dir) = std::env::var("APERTURE_CONFIG_DIR") {
        PathBuf::from(dir)
    } else {
        get_config_dir()?
    };

    // Get the original spec file path
    let specs_dir = config_dir.join("specs");
    let spec_path = specs_dir.join(format!("{spec_name}.yaml"));

    // Get the original strict mode preference (default to false if not set)
    let strict = manager.get_strict_preference(spec_name).unwrap_or(false);

    // Re-add the spec with force to regenerate the cache using original strict preference
    manager.add_spec(spec_name, &spec_path, true, strict)?;

    println!("Successfully reinitialized cache for '{spec_name}'");
    Ok(())
}

fn reinit_all_specs(manager: &ConfigManager<OsFileSystem>) -> Result<(), Error> {
    let specs = manager.list_specs()?;

    if specs.is_empty() {
        println!("No API specifications found to reinitialize.");
        return Ok(());
    }

    println!("Reinitializing {} cached specification(s)...", specs.len());

    for spec_name in &specs {
        match reinit_spec(manager, spec_name) {
            Ok(()) => {
                println!("  ✓ {spec_name}");
            }
            Err(e) => {
                eprintln!("  ✗ {spec_name}: {e}");
            }
        }
    }

    println!("Reinitialization complete.");
    Ok(())
}

fn list_specs_with_details(
    manager: &ConfigManager<OsFileSystem>,
    specs: Vec<String>,
    verbose: bool,
) {
    let cache_dir = manager.config_dir().join(".cache");

    for spec_name in specs {
        println!("- {spec_name}");

        if !verbose {
            continue;
        }

        // Try to load cached spec for verbose details
        let Ok(cached_spec) =
            aperture_cli::engine::loader::load_cached_spec(&cache_dir, &spec_name)
        else {
            continue;
        };

        if cached_spec.skipped_endpoints.is_empty() {
            continue;
        }

        display_skipped_endpoints_info(&cached_spec);
    }
}

fn display_skipped_endpoints_info(cached_spec: &aperture_cli::cache::models::CachedSpec) {
    use aperture_cli::config::manager::ConfigManager;
    use aperture_cli::fs::OsFileSystem;

    // Convert to warnings for consistent display
    let warnings = ConfigManager::<OsFileSystem>::skipped_endpoints_to_warnings(
        &cached_spec.skipped_endpoints,
    );

    // Count total operations including all skipped ones
    let skipped_count = warnings.len();
    let total_operations = cached_spec.commands.len() + skipped_count;

    // Format and display warnings
    let lines = ConfigManager::<OsFileSystem>::format_validation_warnings(
        &warnings,
        Some(total_operations),
        "  ",
    );

    for line in lines {
        println!("{line}");
    }
}

#[allow(clippy::too_many_lines)]
async fn execute_api_command(context: &str, args: Vec<String>, cli: &Cli) -> Result<(), Error> {
    // Get the cache directory - respecting APERTURE_CONFIG_DIR if set
    let config_dir = if let Ok(dir) = std::env::var("APERTURE_CONFIG_DIR") {
        PathBuf::from(dir)
    } else {
        get_config_dir()?
    };
    let cache_dir = config_dir.join(".cache");

    // Create config manager and load global config
    let manager = ConfigManager::with_fs(OsFileSystem, config_dir.clone());
    let global_config = manager.load_global_config().ok();

    // Load the cached spec for the context
    let spec = loader::load_cached_spec(&cache_dir, context).map_err(|e| match e {
        Error::Io(_) => Error::SpecNotFound {
            name: context.to_string(),
        },
        _ => e,
    })?;

    // Handle --describe-json flag - output capability manifest and exit
    if cli.describe_json {
        // Load the original spec file for complete metadata
        let specs_dir = config_dir.join("specs");
        let spec_path = specs_dir.join(format!("{context}.yaml"));

        if !spec_path.exists() {
            return Err(Error::SpecNotFound {
                name: context.to_string(),
            });
        }

        let spec_content = fs::read_to_string(&spec_path)?;
        let openapi_spec: openapiv3::OpenAPI = serde_yaml::from_str(&spec_content)
            .map_err(|e| Error::Config(format!("Failed to parse OpenAPI spec: {e}")))?;

        // Generate manifest from the original spec with all metadata
        let manifest = agent::generate_capability_manifest_from_openapi(
            context,
            &openapi_spec,
            global_config.as_ref(),
        )?;

        // Apply JQ filter if provided
        let output = if let Some(jq_filter) = &cli.jq {
            executor::apply_jq_filter(&manifest, jq_filter)?
        } else {
            manifest
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
        .try_get_matches_from(std::iter::once("api".to_string()).chain(args))
        .map_err(|e| Error::InvalidCommand {
            context: context.to_string(),
            reason: e.to_string(),
        })?;

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
            if format_str == "json" && !matches!(cli.format, aperture_cli::cli::OutputFormat::Json)
            {
                // User didn't explicitly set format in dynamic command, use CLI global format
                cli.format.clone()
            } else {
                match format_str.as_str() {
                    "json" => aperture_cli::cli::OutputFormat::Json,
                    "yaml" => aperture_cli::cli::OutputFormat::Yaml,
                    "table" => aperture_cli::cli::OutputFormat::Table,
                    _ => cli.format.clone(),
                }
            }
        },
    );

    // Create cache configuration from CLI flags
    let cache_config = if cli.no_cache {
        None
    } else {
        Some(CacheConfig {
            cache_dir: config_dir.join(".cache").join("responses"),
            default_ttl: Duration::from_secs(cli.cache_ttl.unwrap_or(300)),
            max_entries: 1000,
            enabled: cli.cache || cli.cache_ttl.is_some(),
        })
    };

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
    )
    .await
    .map_err(|e| match &e {
        Error::Network(req_err) => {
            if req_err.is_connect() {
                e.with_context("Failed to connect to API server")
            } else if req_err.is_timeout() {
                e.with_context("Request timed out")
            } else {
                e
            }
        }
        _ => e,
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
    let batch_config = BatchConfig {
        max_concurrency: cli.batch_concurrency,
        rate_limit: cli.batch_rate_limit,
        continue_on_error: true, // Default to continuing on error for batch operations
        show_progress: !cli.json_errors, // Disable progress when using JSON output
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
        let output = if let Some(jq_filter) = &cli.jq {
            let summary_json = serde_json::to_string(&summary).unwrap();
            executor::apply_jq_filter(&summary_json, jq_filter)?
        } else {
            serde_json::to_string_pretty(&summary).unwrap()
        };

        println!("{output}");
    } else {
        // Output human-readable summary
        println!("\n=== Batch Execution Summary ===");
        println!("Total operations: {}", result.results.len());
        println!("Successful: {}", result.success_count);
        println!("Failed: {}", result.failure_count);
        println!("Total time: {:.2}s", result.total_duration.as_secs_f64());

        if result.failure_count > 0 {
            println!("\nFailed operations:");
            for (i, op_result) in result.results.iter().enumerate() {
                if !op_result.success {
                    println!(
                        "  {} - {}: {}",
                        i + 1,
                        op_result.operation.args.join(" "),
                        op_result.error.as_deref().unwrap_or("Unknown error")
                    );
                }
            }
        }
    }

    // Exit with error code if any operations failed
    if result.failure_count > 0 {
        std::process::exit(1);
    }

    Ok(())
}

/// Prints an error message, either as JSON or user-friendly format
fn print_error_with_json(error: &Error, json_format: bool) {
    if json_format {
        let json_error = error.to_json();
        if let Ok(json_output) = serde_json::to_string_pretty(&json_error) {
            eprintln!("{json_output}");
        } else {
            // Fallback to regular error if JSON serialization fails
            print_error(error);
        }
    } else {
        print_error(error);
    }
}

/// Clear response cache for a specific API or all APIs
async fn clear_response_cache(
    _manager: &ConfigManager<OsFileSystem>,
    api_name: Option<&str>,
    all: bool,
) -> Result<(), Error> {
    let config_dir = if let Ok(dir) = std::env::var("APERTURE_CONFIG_DIR") {
        PathBuf::from(dir)
    } else {
        get_config_dir()?
    };

    let cache_config = CacheConfig {
        cache_dir: config_dir.join(".cache").join("responses"),
        ..Default::default()
    };

    let cache = ResponseCache::new(cache_config)?;

    let cleared_count = if all {
        cache.clear_all().await?
    } else if let Some(api) = api_name {
        cache.clear_api_cache(api).await?
    } else {
        eprintln!("Error: Either specify an API name or use --all flag");
        std::process::exit(1);
    };

    if all {
        println!("Cleared {cleared_count} cached responses for all APIs");
    } else if let Some(api) = api_name {
        println!("Cleared {cleared_count} cached responses for API '{api}'");
    }

    Ok(())
}

/// Show cache statistics for a specific API or all APIs
async fn show_cache_stats(
    _manager: &ConfigManager<OsFileSystem>,
    api_name: Option<&str>,
) -> Result<(), Error> {
    let config_dir = if let Ok(dir) = std::env::var("APERTURE_CONFIG_DIR") {
        PathBuf::from(dir)
    } else {
        get_config_dir()?
    };

    let cache_config = CacheConfig {
        cache_dir: config_dir.join(".cache").join("responses"),
        ..Default::default()
    };

    let cache = ResponseCache::new(cache_config)?;
    let stats = cache.get_stats(api_name).await?;

    if let Some(api) = api_name {
        println!("Cache statistics for API '{api}':");
    } else {
        println!("Cache statistics for all APIs:");
    }

    println!("  Total entries: {}", stats.total_entries);
    println!("  Valid entries: {}", stats.valid_entries);
    println!("  Expired entries: {}", stats.expired_entries);
    #[allow(clippy::cast_precision_loss)]
    let size_mb = stats.total_size_bytes as f64 / 1024.0 / 1024.0;
    println!("  Total size: {size_mb:.2} MB");

    if stats.total_entries > 0 {
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
        Error::Config(msg) => {
            eprintln!("Configuration Error\n{msg}");
        }
        Error::Io(io_err) => match io_err.kind() {
            std::io::ErrorKind::NotFound => {
                eprintln!("File Not Found\n{io_err}\n\nHint: Check that the file path is correct and the file exists.");
            }
            std::io::ErrorKind::PermissionDenied => {
                eprintln!("Permission Denied\n{io_err}\n\nHint: Check file permissions or run with appropriate privileges.");
            }
            _ => {
                eprintln!("File System Error\n{io_err}");
            }
        },
        Error::Network(req_err) => {
            if req_err.is_connect() {
                eprintln!("Connection Error\n{req_err}\n\nHint: Check that the API server is running and accessible.");
            } else if req_err.is_timeout() {
                eprintln!("Request Timeout\n{req_err}\n\nHint: The API server may be slow or unresponsive. Try again later.");
            } else if req_err.is_status() {
                if let Some(status) = req_err.status() {
                    match status.as_u16() {
                        401 => eprintln!("Authentication Error (401)\n{req_err}\n\nHint: Check your API credentials and authentication configuration."),
                        403 => eprintln!("Authorization Error (403)\n{req_err}\n\nHint: Your credentials may be valid but lack permission for this operation."),
                        404 => eprintln!("Resource Not Found (404)\n{req_err}\n\nHint: Check that the API endpoint and parameters are correct."),
                        429 => eprintln!("Rate Limited (429)\n{req_err}\n\nHint: You're making requests too quickly. Wait before trying again."),
                        500..=599 => eprintln!("Server Error ({})\n{req_err}\n\nHint: The API server is experiencing issues. Try again later.", status.as_u16()),
                        _ => eprintln!("HTTP Error ({})\n{req_err}", status.as_u16()),
                    }
                } else {
                    eprintln!("HTTP Error\n{req_err}");
                }
            } else {
                eprintln!("Network Error\n{req_err}");
            }
        }
        Error::Yaml(yaml_err) => {
            eprintln!("YAML Parsing Error\n{yaml_err}\n\nHint: Check that your OpenAPI specification is valid YAML syntax.");
        }
        Error::Json(json_err) => {
            eprintln!("JSON Parsing Error\n{json_err}\n\nHint: Check that your request body or response contains valid JSON.");
        }
        Error::Validation(msg) => {
            eprintln!("Validation Error\n{msg}\n\nHint: Check that your OpenAPI specification follows the required format.");
        }
        Error::Toml(toml_err) => {
            eprintln!("TOML Parsing Error\n{toml_err}\n\nHint: Check that your configuration file is valid TOML syntax.");
        }
        Error::SpecNotFound { name } => {
            eprintln!("API Specification Not Found\n{error}\n\nHint: Use 'aperture config list' to see available specifications\n      or 'aperture config add {name} <file>' to add this specification.");
        }
        Error::SpecAlreadyExists { .. } => {
            eprintln!("Specification Already Exists\n{error}");
        }
        Error::CachedSpecNotFound { .. } => {
            eprintln!("Cached Specification Not Found\n{error}");
        }
        Error::CachedSpecCorrupted { .. } => {
            eprintln!("Cached Specification Corrupted\n{error}\n\nHint: Try removing and re-adding the specification.");
        }
        Error::CacheVersionMismatch { name, .. } => {
            eprintln!("Cache Version Mismatch\n{error}\n\nHint: Run 'aperture config reinit {name}' to regenerate the cache with the current format.");
        }
        Error::SecretNotSet { env_var, .. } => {
            eprintln!("Authentication Secret Not Set\n{error}\n\nHint: Set the environment variable: export {env_var}=<your-secret>");
        }
        Error::InvalidHeaderFormat { .. }
        | Error::InvalidHeaderName { .. }
        | Error::InvalidHeaderValue { .. }
        | Error::EmptyHeaderName => {
            eprintln!("Invalid Header\n{error}");
        }
        Error::EditorNotSet => {
            eprintln!(
                "Editor Not Set\n{error}\n\nHint: Set your preferred editor: export EDITOR=vim"
            );
        }
        Error::EditorFailed { .. } => {
            eprintln!("Editor Failed\n{error}");
        }
        Error::InvalidHttpMethod { .. } => {
            eprintln!("Invalid HTTP Method\n{error}");
        }
        Error::MissingPathParameter { .. } => {
            eprintln!("Missing Path Parameter\n{error}");
        }
        Error::UnsupportedAuthScheme { .. } | Error::UnsupportedSecurityScheme { .. } => {
            eprintln!("Unsupported Security Scheme\n{error}");
        }
        Error::SerializationError { .. } => {
            eprintln!("Serialization Error\n{error}");
        }
        Error::InvalidConfig { .. } => {
            eprintln!("Invalid Configuration\n{error}\n\nHint: Check the TOML syntax in your configuration file.");
        }
        Error::HomeDirectoryNotFound => {
            eprintln!("Home Directory Not Found\n{error}\n\nHint: Ensure HOME environment variable is set.");
        }
        Error::InvalidJsonBody { .. } => {
            eprintln!("Invalid JSON Body\n{error}\n\nHint: Check your JSON syntax and ensure all quotes are properly escaped.");
        }
        Error::RequestFailed { .. } | Error::ResponseReadError { .. } => {
            eprintln!("Request Failed\n{error}");
        }
        Error::HttpErrorWithContext {
            status,
            body,
            api_name,
            operation_id,
            security_schemes,
        } => match status {
            401 => {
                eprintln!("Authentication Error (401) - API: {api_name}");
                if let Some(op_id) = operation_id {
                    eprintln!("Operation: {op_id}");
                }
                eprintln!("Response: {body}");
                eprintln!();

                if security_schemes.is_empty() {
                    eprintln!("Hint: Check your API credentials and authentication configuration.");
                } else {
                    eprintln!("This operation requires authentication. Check these environment variables:");
                    for scheme_name in security_schemes {
                        eprintln!("  • Authentication scheme '{scheme_name}' - verify your environment variable is set");
                    }
                    eprintln!("\nExample: export YOUR_API_KEY=<your-secret>");
                }
            }
            403 => {
                eprintln!("Authorization Error (403) - API: {api_name}");
                if let Some(op_id) = operation_id {
                    eprintln!("Operation: {op_id}");
                }
                eprintln!("Response: {body}");
                eprintln!();
                eprintln!(
                    "Hint: Your credentials may be valid but lack permission for this operation."
                );
            }
            404 => {
                eprintln!("Resource Not Found (404) - API: {api_name}");
                if let Some(op_id) = operation_id {
                    eprintln!("Operation: {op_id}");
                }
                eprintln!("Response: {body}");
                eprintln!();
                eprintln!("Hint: Check that the API endpoint and parameters are correct.");
            }
            429 => {
                eprintln!("Rate Limited (429) - API: {api_name}");
                if let Some(op_id) = operation_id {
                    eprintln!("Operation: {op_id}");
                }
                eprintln!("Response: {body}");
                eprintln!();
                eprintln!("Hint: You're making requests too quickly. Wait before trying again.");
            }
            500..=599 => {
                eprintln!("Server Error ({status}) - API: {api_name}");
                if let Some(op_id) = operation_id {
                    eprintln!("Operation: {op_id}");
                }
                eprintln!("Response: {body}");
                eprintln!();
                eprintln!("Hint: The API server is experiencing issues. Try again later.");
            }
            _ => {
                eprintln!("HTTP Error ({status}) - API: {api_name}");
                if let Some(op_id) = operation_id {
                    eprintln!("Operation: {op_id}");
                }
                eprintln!("Response: {body}");
            }
        },
        Error::InvalidCommand { context, .. } => {
            eprintln!("Invalid Command\n{error}\n\nHint: Use 'aperture api {context} --help' to see available commands.");
        }
        Error::OperationNotFound => {
            eprintln!("Operation Not Found\n{error}\n\nHint: Check that the command matches an available operation.");
        }
        Error::InvalidIdempotencyKey => {
            eprintln!("Invalid Idempotency Key\n{error}\n\nHint: Idempotency key must be a valid header value.");
        }
        Error::JqFilterError { .. } => {
            eprintln!("JQ Filter Error\n{error}\n\nHint: Check your JQ filter syntax. Common examples: '.name', '.[] | select(.active)'");
        }
        Error::InvalidPath { .. } => {
            eprintln!("Invalid Path\n{error}\n\nHint: Check that the path is valid and properly formatted.");
        }
        Error::InteractiveInputTooLong {
            provided,
            max,
            suggestion,
        } => {
            eprintln!("Input Too Long\nProvided {provided} characters (max: {max})\n{suggestion}");
        }
        Error::InteractiveInvalidCharacters {
            invalid_chars,
            suggestion,
        } => {
            eprintln!(
                "Invalid Input Characters\nInvalid characters: {invalid_chars}\n{suggestion}"
            );
        }
        Error::InteractiveTimeout {
            timeout_secs,
            suggestion,
        } => {
            eprintln!("Input Timeout\nTimed out after {timeout_secs} seconds\n{suggestion}");
        }
        Error::InteractiveRetriesExhausted {
            max_attempts,
            last_error,
            suggestions,
        } => {
            eprintln!("Maximum Retries Exceeded\nFailed after {max_attempts} attempts. Last error: {last_error}");
            if !suggestions.is_empty() {
                eprintln!("\nSuggestions:");
                for suggestion in suggestions {
                    eprintln!("  • {suggestion}");
                }
            }
        }
        Error::InvalidEnvironmentVariableName {
            name,
            reason,
            suggestion,
        } => {
            eprintln!("Invalid Environment Variable Name\nName '{name}' is invalid: {reason}\n{suggestion}");
        }
        Error::RequestTimeout {
            attempts,
            timeout_ms,
        } => {
            eprintln!("Request Timeout\nRequest timed out after {attempts} retries (max timeout: {timeout_ms}ms)\n\nHint: The server may be slow or unresponsive. Try again later or increase timeout.");
        }
        Error::RetryLimitExceeded {
            attempts,
            duration_ms,
            last_error,
        } => {
            eprintln!("Retry Limit Exceeded\nFailed after {attempts} attempts over {duration_ms}ms\nLast error: {last_error}\n\nHint: The service may be experiencing issues. Check API status or try again later.");
        }
        Error::TransientNetworkError { reason, retryable } => {
            if *retryable {
                eprintln!("Transient Network Error\n{reason}\n\nHint: This error is retryable. The request will be automatically retried.");
            } else {
                eprintln!("Network Error\n{reason}\n\nHint: This error is not retryable. Check your network connection and API configuration.");
            }
        }
        Error::Anyhow(err) => {
            eprintln!("💥 Unexpected Error\n{err}\n\nHint: This may be a bug. Please report it with the command you were running.");
        }
    }
}
