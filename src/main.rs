use aperture_cli::cli::commands::config::validate_api_name;
use aperture_cli::cli::{Cli, Commands};
use aperture_cli::config::manager::ConfigManager;
use aperture_cli::constants;
use aperture_cli::error::Error;
use aperture_cli::fs::OsFileSystem;
use aperture_cli::output::Output;
use clap::Parser;
use std::path::PathBuf;

#[tokio::main]
async fn main() {
    #[cfg(not(windows))]
    let _ = rustls::crypto::ring::default_provider().install_default();
    #[cfg(windows)]
    let _ = rustls::crypto::aws_lc_rs::default_provider().install_default();
    let cli = Cli::parse();
    aperture_cli::cli::tracing_init::init_tracing(cli.verbosity);
    let json_errors = cli.json_errors;
    let output = Output::new(cli.quiet, cli.json_errors);

    let manager = std::env::var(constants::ENV_APERTURE_CONFIG_DIR).map_or_else(
        |_| match ConfigManager::new() {
            Ok(manager) => manager,
            Err(e) => {
                aperture_cli::cli::errors::print_error_with_json(&e, json_errors);
                std::process::exit(1);
            }
        },
        |config_dir| ConfigManager::with_fs(OsFileSystem, PathBuf::from(config_dir)),
    );

    if let Err(e) = run_command(cli, &manager, &output).await {
        aperture_cli::cli::errors::print_error_with_json(&e, json_errors);
        std::process::exit(1);
    }
}

fn run_list_commands(context: &str, output: &Output) -> Result<(), Error> {
    let context = validate_api_name(context)?;
    aperture_cli::cli::commands::docs::list_commands(&context, output)
}

async fn run_api_command(cli: &Cli, context: &str, args: &[String]) -> Result<(), Error> {
    let context = validate_api_name(context)?;
    aperture_cli::cli::commands::api::execute_api_command(&context, args.to_vec(), cli).await
}

fn run_search_command(
    manager: &ConfigManager<OsFileSystem>,
    query: &str,
    api: Option<&str>,
    verbose: bool,
    output: &Output,
) -> Result<(), Error> {
    let validated_api = api.map(validate_api_name).transpose()?;
    aperture_cli::cli::commands::search::execute_search_command(
        manager,
        query,
        validated_api.as_deref(),
        verbose,
        output,
    )
}

async fn run_shortcut_command(
    manager: &ConfigManager<OsFileSystem>,
    args: &[String],
    cli: &Cli,
) -> Result<(), Error> {
    aperture_cli::cli::commands::api::execute_shortcut_command(manager, args.to_vec(), cli).await
}

fn run_docs_command(
    manager: &ConfigManager<OsFileSystem>,
    api: Option<&str>,
    tag: Option<&str>,
    operation: Option<&str>,
    enhanced: bool,
    output: &Output,
) -> Result<(), Error> {
    let validated_api = api.map(validate_api_name).transpose()?;
    aperture_cli::cli::commands::docs::execute_help_command(
        manager,
        validated_api.as_deref(),
        tag,
        operation,
        enhanced,
        output,
    )
}

fn run_overview_command(
    manager: &ConfigManager<OsFileSystem>,
    api: Option<&str>,
    all: bool,
    output: &Output,
) -> Result<(), Error> {
    let validated_api = api.map(validate_api_name).transpose()?;
    aperture_cli::cli::commands::docs::execute_overview_command(
        manager,
        validated_api.as_deref(),
        all,
        output,
    )
}

async fn run_non_config_command(
    cli: &Cli,
    manager: &ConfigManager<OsFileSystem>,
    output: &Output,
) -> Result<(), Error> {
    match &cli.command {
        Commands::ListCommands { context } => run_list_commands(context, output),
        Commands::Api { context, args } => run_api_command(cli, context, args).await,
        Commands::Search {
            query,
            api,
            verbose,
        } => run_search_command(manager, query, api.as_deref(), *verbose, output),
        Commands::Exec { args } => run_shortcut_command(manager, args, cli).await,
        Commands::Docs {
            api,
            tag,
            operation,
            enhanced,
        } => run_docs_command(
            manager,
            api.as_deref(),
            tag.as_deref(),
            operation.as_deref(),
            *enhanced,
            output,
        ),
        Commands::Overview { api, all } => {
            run_overview_command(manager, api.as_deref(), *all, output)
        }
        Commands::Config { .. } => unreachable!("config commands are handled separately"),
    }
}

#[allow(clippy::too_many_lines, clippy::cognitive_complexity)]
async fn run_command(
    cli: Cli,
    manager: &ConfigManager<OsFileSystem>,
    output: &Output,
) -> Result<(), Error> {
    use aperture_cli::cli::commands::config;

    if let Commands::Config { command } = &cli.command {
        config::execute_config_command(manager, command.clone(), output).await?;
        return Ok(());
    }

    run_non_config_command(&cli, manager, output).await
}
