use aperture::cli::{Cli, Commands, ConfigCommands};
use aperture::config::manager::ConfigManager;
use aperture::error::Error;
use aperture::fs::OsFileSystem;
use clap::Parser;
use std::path::PathBuf;

fn main() {
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

    if let Err(e) = run_command(cli, &manager) {
        eprintln!("Error: {e}");
        std::process::exit(1);
    }
}

fn run_command(cli: Cli, manager: &ConfigManager<OsFileSystem>) -> Result<(), Error> {
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
    }

    Ok(())
}
