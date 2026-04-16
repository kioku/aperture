use aperture_cli::cli::commands::config::validate_api_name;
use aperture_cli::cli::{Cli, Commands, DiscoveryFormat};
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

fn run_list_commands(
    context: &str,
    format: &DiscoveryFormat,
    output: &Output,
) -> Result<(), Error> {
    let context = validate_api_name(context)?;
    aperture_cli::cli::commands::docs::list_commands(&context, format, output)
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
    api: Option<&str>,
    cli: &Cli,
) -> Result<(), Error> {
    let validated_api = api.map(validate_api_name).transpose()?;
    aperture_cli::cli::commands::api::execute_shortcut_command(
        manager,
        args.to_vec(),
        validated_api.as_deref(),
        cli,
    )
    .await
}

fn run_docs_command(
    manager: &ConfigManager<OsFileSystem>,
    api: Option<&str>,
    tag: Option<&str>,
    operation: Option<&str>,
    enhanced: bool,
    format: &DiscoveryFormat,
    output: &Output,
) -> Result<(), Error> {
    let validated_api = api.map(validate_api_name).transpose()?;
    aperture_cli::cli::commands::docs::execute_help_command(
        manager,
        validated_api.as_deref(),
        tag,
        operation,
        enhanced,
        format,
        output,
    )
}

fn run_overview_command(
    manager: &ConfigManager<OsFileSystem>,
    api: Option<&str>,
    all: bool,
    format: &DiscoveryFormat,
    output: &Output,
) -> Result<(), Error> {
    let validated_api = api.map(validate_api_name).transpose()?;
    aperture_cli::cli::commands::docs::execute_overview_command(
        manager,
        validated_api.as_deref(),
        all,
        format,
        output,
    )
}

fn run_completion_command(cli: &Cli) -> Option<Result<(), Error>> {
    match &cli.command {
        Commands::Completion { shell } => {
            Some(aperture_cli::cli::commands::completion::execute_completion_script_command(shell))
        }
        Commands::Complete {
            shell,
            cword,
            words,
        } => Some(
            aperture_cli::cli::commands::completion::execute_completion_runtime_command(
                shell, *cword, words,
            ),
        ),
        _ => None,
    }
}

async fn run_user_command(
    cli: &Cli,
    manager: &ConfigManager<OsFileSystem>,
    output: &Output,
) -> Result<(), Error> {
    match &cli.command {
        Commands::ListCommands { context, format } => run_list_commands(context, format, output),
        Commands::Api { context, args, .. } => run_api_command(cli, context, args).await,
        Commands::Search {
            query,
            api,
            verbose,
        } => run_search_command(manager, query, api.as_deref(), *verbose, output),
        Commands::Exec { api, args, .. } => {
            run_shortcut_command(manager, args, api.as_deref(), cli).await
        }
        Commands::Docs {
            api,
            tag,
            operation,
            enhanced,
            format,
        } => run_docs_command(
            manager,
            api.as_deref(),
            tag.as_deref(),
            operation.as_deref(),
            *enhanced,
            format,
            output,
        ),
        Commands::Overview { api, all, format } => {
            run_overview_command(manager, api.as_deref(), *all, format, output)
        }
        Commands::Completion { .. } | Commands::Complete { .. } => unreachable!(),
        Commands::Config { .. } => unreachable!("config commands are handled separately"),
    }
}

async fn run_non_config_command(
    cli: &Cli,
    manager: &ConfigManager<OsFileSystem>,
    output: &Output,
) -> Result<(), Error> {
    if let Some(result) = run_completion_command(cli) {
        return result;
    }

    run_user_command(cli, manager, output).await
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
