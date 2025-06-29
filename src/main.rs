use aperture::cli::{Cli, Commands, ConfigCommands};
use aperture::config::manager::{get_config_dir, ConfigManager};
use aperture::engine::{executor, generator, loader};
use aperture::error::Error;
use aperture::fs::OsFileSystem;
use clap::Parser;
use std::path::PathBuf;

#[tokio::main]
async fn main() {
    let cli = Cli::parse();

    let manager = std::env::var("APERTURE_CONFIG_DIR").map_or_else(
        |_| match ConfigManager::new() {
            Ok(manager) => manager,
            Err(e) => {
                eprintln!("Error: {e}");
                std::process::exit(1);
            }
        },
        |config_dir| ConfigManager::with_fs(OsFileSystem, PathBuf::from(config_dir)),
    );

    if let Err(e) = run_command(cli, &manager).await {
        eprintln!("Error: {e}");
        std::process::exit(1);
    }
}

async fn run_command(cli: Cli, manager: &ConfigManager<OsFileSystem>) -> Result<(), Error> {
    match cli.command {
        Commands::Config { command } => match command {
            ConfigCommands::Add { name, file, force } => {
                manager.add_spec(&name, &file, force)?;
                println!("Spec '{name}' added successfully.");
            }
            ConfigCommands::List {} => {
                let specs = manager.list_specs()?;
                if specs.is_empty() {
                    println!("No API specifications found.");
                } else {
                    println!("Registered API specifications:");
                    for spec in specs {
                        println!("- {spec}");
                    }
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
        },
        Commands::Api { context, args } => {
            execute_api_command(&context, args).await?;
        }
    }

    Ok(())
}

async fn execute_api_command(context: &str, args: Vec<String>) -> Result<(), Error> {
    // Get the cache directory
    let config_dir = get_config_dir()?;
    let cache_dir = config_dir.join(".cache");

    // Load the cached spec for the context
    let spec = loader::load_cached_spec(&cache_dir, context)?;

    // Generate the dynamic command tree
    let command = generator::generate_command_tree(&spec);

    // Parse the arguments against the dynamic command
    let matches = command
        .try_get_matches_from(std::iter::once("api".to_string()).chain(args))
        .map_err(|e| Error::Config(format!("Command parsing failed: {e}")))?;

    // Execute the request (None = use environment variable)
    executor::execute_request(&spec, &matches, None).await?;

    Ok(())
}
