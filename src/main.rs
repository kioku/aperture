use aperture_cli::cli::commands::config::validate_api_name;
use aperture_cli::cli::{Cli, Commands, DiscoveryFormat};
use aperture_cli::config::manager::ConfigManager;
use aperture_cli::constants;
use aperture_cli::error::Error;
use aperture_cli::fs::OsFileSystem;
use aperture_cli::output::Output;
use clap::{CommandFactory, FromArgMatches};
use std::path::PathBuf;

#[tokio::main]
async fn main() {
    #[cfg(not(windows))]
    let _ = rustls::crypto::ring::default_provider().install_default();
    #[cfg(windows)]
    let _ = rustls::crypto::aws_lc_rs::default_provider().install_default();
    let matches = parse_cli_matches();
    let mut cli = Cli::from_arg_matches(&matches).unwrap_or_else(|error| error.exit());
    if print_build_info(&cli.command) {
        return;
    }
    aperture_cli::cli::tracing_init::init_tracing(cli.verbosity);
    let json_errors = cli.json_errors;

    let manager = config_manager(json_errors);

    let config = manager.load_global_config().unwrap_or_else(|error| {
        aperture_cli::cli::errors::print_error_with_json(&error, json_errors);
        std::process::exit(1);
    });
    if matches.value_source("json_errors") != Some(clap::parser::ValueSource::CommandLine) {
        cli.json_errors = config.agent_defaults.json_errors;
    }
    let json_errors = cli.json_errors;
    let output = Output::new(cli.quiet, json_errors);

    if let Err(e) = run_command(cli, &manager, &output).await {
        aperture_cli::cli::errors::print_error_with_json(&e, json_errors);
        std::process::exit(1);
    }
}

fn config_manager(json_errors: bool) -> ConfigManager<OsFileSystem> {
    std::env::var(constants::ENV_APERTURE_CONFIG_DIR).map_or_else(
        |_| match ConfigManager::new() {
            Ok(manager) => manager,
            Err(e) => {
                aperture_cli::cli::errors::print_error_with_json(&e, json_errors);
                std::process::exit(1);
            }
        },
        |config_dir| ConfigManager::with_fs(OsFileSystem, PathBuf::from(config_dir)),
    )
}

/// Build information is independent of configuration and execution defaults.
fn print_build_info(command: &Commands) -> bool {
    let Commands::BuildInfo { json } = command else {
        return false;
    };
    let info = aperture_cli::build_info::current();
    if *json {
        println!(
            "{}",
            serde_json::to_string(&info).expect("build info is serializable")
        );
    } else {
        println!(
            "aperture-cli {}\nSource revision: {}\nSource state: {}",
            info.version, info.revision, info.source_state
        );
    }
    true
}

fn parse_cli_matches() -> clap::ArgMatches {
    let args: Vec<_> = std::env::args_os().collect();
    Cli::command()
        .try_get_matches_from(&args)
        .unwrap_or_else(|error| {
            if error.use_stderr() && parse_failure_uses_json_errors(&args) {
                aperture_cli::cli::errors::print_error_with_json(
                    &Error::invalid_command("cli", error.to_string()),
                    true,
                );
                std::process::exit(2);
            }
            error.exit()
        })
}

/// Clap can fail before configuration-backed command handling starts. Preserve
/// native help/version output, but apply error defaults to actual parse failures.
fn parse_failure_uses_json_errors(args: &[std::ffi::OsString]) -> bool {
    // Reuse Clap's grammar rather than mistaking option values or trailing API
    // arguments for global flags. Only successfully parsed overrides apply.
    let partial = Cli::command()
        .ignore_errors(true)
        .try_get_matches_from(args)
        .ok();
    let explicit = partial
        .as_ref()
        .filter(|matches| {
            matches.value_source("json_errors") == Some(clap::parser::ValueSource::CommandLine)
        })
        .and_then(|matches| matches.get_one::<bool>("json_errors").copied());
    explicit.unwrap_or_else(configured_json_errors)
}

fn configured_json_errors() -> bool {
    let manager = std::env::var(constants::ENV_APERTURE_CONFIG_DIR).map_or_else(
        |_| ConfigManager::new().ok(),
        |dir| Some(ConfigManager::with_fs(OsFileSystem, PathBuf::from(dir))),
    );
    manager
        .and_then(|manager| manager.load_global_config().ok())
        .is_some_and(|config| config.agent_defaults.json_errors)
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
        Commands::Skills { .. }
        | Commands::BuildInfo { .. }
        | Commands::Completion { .. }
        | Commands::Complete { .. } => {
            unreachable!()
        }
        Commands::Config { .. } => unreachable!("config commands are handled separately"),
    }
}

async fn run_non_config_command(
    cli: &Cli,
    manager: &ConfigManager<OsFileSystem>,
    output: &Output,
) -> Result<(), Error> {
    if let Commands::Skills { command } = &cli.command {
        return aperture_cli::skills::execute(manager, command.as_ref()).await;
    }
    if let Some(result) = run_completion_command(cli) {
        return result;
    }

    run_user_command(cli, manager, output).await
}

/// Keep the dispatcher on the heap so its largest command future does not
/// consume stack alongside Clap's debug-build command-tree construction.
fn run_command<'a>(
    cli: Cli,
    manager: &'a ConfigManager<OsFileSystem>,
    output: &'a Output,
) -> impl std::future::Future<Output = Result<(), Error>> + 'a {
    Box::pin(async move {
        use aperture_cli::cli::commands::config;

        if let Commands::Config { command } = &cli.command {
            config::execute_config_command(manager, command.clone(), output).await?;
            return Ok(());
        }

        run_non_config_command(&cli, manager, output).await
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use clap::Parser;

    #[test]
    fn command_dispatch_future_stays_bounded() {
        let cli =
            Cli::try_parse_from(["aperture", "config", "add", "stack-probe", "spec.yaml"]).unwrap();
        let dir = tempfile::tempdir().unwrap();
        let manager = ConfigManager::with_fs(OsFileSystem, dir.path().into());
        let output = Output::new(true, false);
        let future = run_command(cli, &manager, &output);
        assert!(
            std::mem::size_of_val(&future) < 1024,
            "command dispatcher must not retain an inline command future"
        );
    }
}
