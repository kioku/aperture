use clap::{Parser, Subcommand};
use std::path::PathBuf;

#[derive(Parser, Debug)]
#[command(author, version, about, long_about = None)]
pub struct Cli {
    #[command(subcommand)]
    pub command: Commands,
}

#[derive(Subcommand, Debug)]
pub enum Commands {
    /// Manage API specifications
    Config {
        #[command(subcommand)]
        command: ConfigCommands,
    },
}

#[derive(Subcommand, Debug)]
pub enum ConfigCommands {
    /// Add a new API specification
    Add {
        /// Name of the API specification
        name: String,
        /// Path to the `OpenAPI` specification file
        file: PathBuf,
        /// Overwrite existing specification if it exists
        #[arg(long)]
        force: bool,
    },
    /// List all registered API specifications
    List {},
    /// Remove an API specification
    Remove {
        /// Name of the API specification to remove
        name: String,
    },
    /// Edit an API specification
    Edit {
        /// Name of the API specification to edit
        name: String,
    },
}
