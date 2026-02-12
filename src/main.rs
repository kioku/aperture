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

#[allow(clippy::too_many_lines, clippy::cognitive_complexity)]
async fn run_command(
    cli: Cli,
    manager: &ConfigManager<OsFileSystem>,
    output: &Output,
) -> Result<(), Error> {
    use aperture_cli::cli::commands::{api, config, docs, search};

    match cli.command {
        Commands::Config { command } => {
            config::execute_config_command(manager, command, output).await?;
        }
        Commands::ListCommands { ref context } => {
            let context = validate_api_name(context)?;
            docs::list_commands(&context, output)?;
        }
        Commands::Api {
            ref context,
            ref args,
        } => {
            let context = validate_api_name(context)?;
            api::execute_api_command(&context, args.clone(), &cli).await?;
        }
        Commands::Search {
            ref query,
            ref api,
            verbose,
        } => {
            let validated_api = api.as_deref().map(validate_api_name).transpose()?;
            search::execute_search_command(
                manager,
                query,
                validated_api.as_deref(),
                verbose,
                output,
            )?;
        }
        Commands::Exec { ref args } => {
            api::execute_shortcut_command(manager, args.clone(), &cli).await?;
        }
        Commands::Docs {
            ref api,
            ref tag,
            ref operation,
            enhanced,
        } => {
            let validated_api = api.as_deref().map(validate_api_name).transpose()?;
            docs::execute_help_command(
                manager,
                validated_api.as_deref(),
                tag.as_deref(),
                operation.as_deref(),
                enhanced,
                output,
            )?;
        }
        Commands::Overview { ref api, all } => {
            let validated_api = api.as_deref().map(validate_api_name).transpose()?;
            docs::execute_overview_command(manager, validated_api.as_deref(), all, output)?;
        }
    }
    Ok(())
}
